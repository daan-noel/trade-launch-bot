// ============================================================
// PumpSwap AMM (migrated tokens).
//
// Once a token graduates off the bonding curve, trading moves to the PumpSwap
// (pump_amm) program. This module implements buy/sell against that AMM.
//
// Account layouts, discriminators, PDA seeds, and the Pool/GlobalConfig struct
// offsets are taken from the on-chain Anchor IDL committed at
// `pump-trader/idl/pump_amm.json` (fetched from program pAMMBay…).
//
//   buy  → uses `buy_exact_quote_in` (spend a fixed SOL budget, min base out).
//   sell → uses `sell` (exact base in, min SOL out).
//
// WSOL handling: the AMM quote mint is wrapped SOL, so each swap wraps SOL into
// a WSOL token account (buy) / unwraps proceeds (sell) and closes it afterward.
// ============================================================

use executor_core::{classify_swap_revert, SwapDirection, SwapRetryDecision, SwapRoute};
use super::{AmmGlobalConfig, AmmPoolInfo, PumpFunTrader};
use crate::error::{bail, Context, Result, TradeError};
use crate::protocol::{
    self, AMM_CONFIG_COIN_CREATOR_FEE_BPS_OFFSET, AMM_CONFIG_FEE_RECIPIENTS_OFFSET,
    AMM_CONFIG_LP_FEE_BPS_OFFSET, AMM_CONFIG_MIN_LEN, AMM_CONFIG_PROTOCOL_FEE_BPS_OFFSET,
    AMM_POOL_BASE_VAULT_OFFSET, AMM_POOL_COIN_CREATOR_OFFSET, AMM_POOL_IS_CASHBACK_OFFSET,
    AMM_POOL_MIN_LEN, AMM_POOL_QUOTE_VAULT_OFFSET,
};
use serde_json::json;
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    system_instruction, system_program,
};
use spl_associated_token_account::{
    get_associated_token_address_with_program_id,
    instruction::create_associated_token_account_idempotent,
};
use std::str::FromStr;
use std::time::Instant;
use tracing::{debug, info};

// Anchor instruction discriminators (from pump_amm.json). We use the original
// `buy` (exact base out, max quote in); `buy_exact_quote_in` would also work.
//
// NOTE: the deployed pump_amm program is newer than its published IDL. Real
// swaps carry a trailing "upgrade-fee" block (pfee global + recipient + the
// recipient's WSOL ATA) that the IDL omits — without it the program hits an
// Overflow in fee accounting. That block is appended in `amm_swap_accounts`;
// the full layout (buy = 26 accounts, sell = 24) is verified against on-chain
// swaps and a `simulateTransaction` dry-run.
// SSOT: shared with the curve `buy`/`sell` (same Anchor name) in `crate::protocol`;
// aliased here so the AMM swap builders + tests read unchanged.
const BUY_DISC: [u8; 8] = crate::protocol::BUY_DISC;
const SELL_DISC: [u8; 8] = crate::protocol::SELL_DISC;

const BPS_DENOM: u128 = 10_000;

/// Max age of the cached PumpSwap GlobalConfig before `amm_config` re-fetches.
/// Fee bps are governance-mutable, so the cache must be freshness-bounded.
const AMM_CONFIG_MAX_AGE: std::time::Duration = std::time::Duration::from_secs(300);

impl PumpFunTrader {
    // -----------------------------------------------------------------------
    // Public API
    // -----------------------------------------------------------------------

    /// Buy a migrated token on the PumpSwap AMM, spending `sol_amount` SOL.
    ///
    /// `base_token_program_id` is the token's SPL program (legacy or 2022).
    /// `pool_override` lets the caller supply the known pool address; when
    /// `None` the canonical index-0 / WSOL pool is derived. `slippage_bps = None`
    /// means no floor (`min_out = 1`); pass `Some(bps)` for explicit protection. `confirm` mirrors
    /// `sell_token_once`/`amm_sell`: `true` blocks on the RPC signature poll
    /// (manual/API callers); `false` returns once the sender accepts and leaves
    /// confirmation to the caller's own feed — saving the ~4 s `confirm_transaction`
    /// poll on the latency-critical path. The base ATA is cached regardless, so a
    /// `confirm=false` caller can still sell.
    /// Returns the submitted transaction signature.
    #[allow(clippy::too_many_arguments)]
    pub async fn amm_buy(
        &self,
        token_mint: &str,
        base_token_program_id: &str,
        sol_amount: f64,
        pool_override: Option<&str>,
        slippage_bps: Option<u64>,
        confirm: bool,
    ) -> Result<String> {
        let user = self.config.signer.pubkey();

        // At most one self-heal resend on a confirmed stale-`coin_creator` 2006 —
        // the AMM-buy analogue of `amm_sell_inner`'s heal. A confirmed revert
        // bought nothing, and the resend takes a fresh blockhash, so it can't
        // double-buy. `confirm=false` callers (none today — AMM buys have no bot
        // path) never reach this branch.
        let mut healed = false;

        loop {
            let t0 = Instant::now();

            let (core_ixs, user_base) = self
                .build_amm_buy_ixs(
                    token_mint,
                    base_token_program_id,
                    sol_amount,
                    pool_override,
                    slippage_bps,
                    &user,
                )
                .await?;

            let mut ixs = Vec::with_capacity(core_ixs.len() + self.engine.cu_ixs_amm.len() + 1);
            ixs.extend_from_slice(&self.engine.cu_ixs_amm);
            ixs.extend(core_ixs);
            ixs.push(self.jito_tip_ix(0));

            // Recent blockhash (not durable nonce): the swap already carries ~27
            // accounts, and a nonce-advance would push the legacy tx over 1232 bytes.
            let tx = self.build_recent_tx(ixs, self.config.signer.as_ref()).await?;
            let sig = self.send_transaction(&tx).await?;
            info!(
                "📤 AMM buy sent — sig: {} | SOL: {} | {}ms",
                sig,
                sol_amount,
                t0.elapsed().as_millis()
            );
            // Tokens land in the base ATA — cache it for a later sell. Done before
            // the (optional) confirm so a `confirm=false` caller still gets it cached.
            self.user_token_accounts
                .insert(token_mint.to_string(), user_base);
            if confirm {
                match self.confirm_transaction(&sig, self.config.retry.confirm_max_retries).await {
                    Ok(()) => {
                        info!(
                            "✅ AMM buy confirmed — sig: {} | {}ms",
                            sig,
                            t0.elapsed().as_millis()
                        );
                        return Ok(sig);
                    }
                    Err(TradeError::Reverted { custom })
                        if !healed
                            && classify_swap_revert(custom, SwapRoute::Amm, SwapDirection::Buy)
                                == SwapRetryDecision::RefreshCoinCreator =>
                    {
                        match self.refresh_amm_pool_info(token_mint, base_token_program_id).await {
                            Ok(Some(vault)) => {
                                info!(
                                    "🔄 AMM buy reverted on a stale coin_creator; refreshed the \
                                     pool (new coin_creator_vault_authority {vault}), resending \
                                     once"
                                );
                                healed = true;
                                continue;
                            }
                            // Unchanged coin_creator or the refresh itself failed — stop
                            // rather than re-pay fees on a resend that can't fix anything.
                            Ok(None) | Err(_) => {
                                return Err(TradeError::Reverted { custom });
                            }
                        }
                    }
                    Err(e) => return Err(e),
                }
            }
            return Ok(sig);
        }
    }

    /// Sell `token_amount` raw base-token units of a migrated token on the AMM.
    /// `tip_level` escalates the Jito tip on retries (0 = first attempt); a
    /// caller-driven retry loop passes its attempt index so a sell that lost the
    /// auction bids up next time (see `jito_tip::JitoTipCache::tip_lamports_for_level`).
    /// `confirm` mirrors `sell_token_once`: `true` blocks on the RPC poll;
    /// `false` returns once the sender accepts and leaves confirmation to the
    /// caller's LaserStream-fed feed (the live TPSL balance poll).
    #[allow(clippy::too_many_arguments)]
    pub async fn amm_sell(
        &self,
        token_mint: &str,
        token_amount: u64,
        base_token_program_id: &str,
        pool_override: Option<&str>,
        token_account_override: Option<&str>,
        slippage_bps: Option<u64>,
        tip_level: u8,
        confirm: bool,
    ) -> Result<Option<String>> {
        self.amm_sell_inner(
            token_mint,
            token_amount,
            base_token_program_id,
            pool_override,
            token_account_override,
            slippage_bps,
            tip_level,
            confirm,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn amm_sell_inner(
        &self,
        token_mint: &str,
        token_amount: u64,
        base_token_program_id: &str,
        pool_override: Option<&str>,
        token_account_override: Option<&str>,
        slippage_bps: Option<u64>,
        tip_level: u8,
        confirm: bool,
    ) -> Result<Option<String>> {
        let user = self.config.signer.pubkey();

        // At most one self-heal resend on a confirmed stale-`coin_creator` 2006 —
        // mirrors the curve sell's heal in `sell.rs::execute_sell` (same shared
        // `classify_swap_revert` decision). `confirm=false` callers never see the
        // error here; their own feed classifies it off-path (the bot exit loop's
        // `RefreshCoinCreator` branch).
        let mut healed = false;
        let mut tip_level = tip_level;

        loop {
            let t0 = Instant::now();

            // Re-resolves `amm_pool_info` each iteration — on the healed retry this
            // reads the pool `refresh_amm_pool_info` just re-cached below, picking
            // up the fresh `coin_creator`.
            let core_ixs = self
                .build_amm_sell_ixs(
                    token_mint,
                    token_amount,
                    base_token_program_id,
                    pool_override,
                    token_account_override,
                    slippage_bps,
                    &user,
                )
                .await?;

            let mut ixs = Vec::with_capacity(core_ixs.len() + self.engine.cu_ixs_amm.len() + 1);
            ixs.extend_from_slice(&self.engine.cu_ixs_amm);
            ixs.extend(core_ixs);
            ixs.push(self.jito_tip_ix(tip_level));

            // Acquire the nonce only after `build_amm_sell_ixs`' pool/config/reserve
            // reads — don't hold the slot `in_use` across that RPC. The block below
            // always falls through to `schedule_nonce_refresh`.
            let (nonce_pubkey, nonce_hash) = self.acquire_nonce().await?;
            let result: Result<Option<String>> = async {
                let tx = self.build_nonce_tx(
                    ixs,
                    &nonce_pubkey,
                    nonce_hash,
                    self.config.signer.as_ref(),
                )?;
                let sig = self.send_transaction(&tx).await?;
                info!(
                    "📤 AMM sell sent — sig: {} | amount: {} | {}ms",
                    sig,
                    token_amount,
                    t0.elapsed().as_millis()
                );
                // See `sell_token_once`: skip the redundant RPC poll on the
                // feed-confirmed path (the caller watches the LaserStream `trades`).
                if confirm {
                    self.confirm_transaction(&sig, self.config.retry.confirm_max_retries)
                        .await?;
                    info!(
                        "✅ AMM sell confirmed — sig: {} | {}ms",
                        sig,
                        t0.elapsed().as_millis()
                    );
                }
                // Hand the signature back so a feed-confirm caller can classify a
                // landed-revert off its own poll window (see `sell_token_once`).
                Ok(Some(sig))
            }
            .await;

            self.schedule_nonce_refresh(nonce_pubkey);

            if let Err(TradeError::Reverted { custom }) = &result {
                if !healed
                    && classify_swap_revert(*custom, SwapRoute::Amm, SwapDirection::Sell)
                        == SwapRetryDecision::RefreshCoinCreator
                {
                    match self.refresh_amm_pool_info(token_mint, base_token_program_id).await {
                        Ok(Some(vault)) => {
                            info!(
                                "🔄 AMM sell reverted on a stale coin_creator; refreshed the \
                                 pool (new coin_creator_vault_authority {vault}), resending once"
                            );
                            healed = true;
                            tip_level = tip_level.saturating_add(1);
                            continue;
                        }
                        // Unchanged coin_creator or the refresh itself failed — stop
                        // rather than re-pay fees on a resend that can't fix anything.
                        Ok(None) | Err(_) => return result,
                    }
                }
            }
            return result;
        }
    }

    // -----------------------------------------------------------------------
    // Instruction builders
    // -----------------------------------------------------------------------

    /// `pub(super)` so the simulation engine (`sim.rs`) builds the byte-identical
    /// AMM buy ix set the live `amm_buy` sends — no copy that can drift. Returns
    /// the core swap ixs (no CU budget / tip) plus the user's base ATA.
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn build_amm_buy_ixs(
        &self,
        token_mint: &str,
        base_token_program_id: &str,
        sol_amount: f64,
        pool_override: Option<&str>,
        slippage_bps: Option<u64>,
        user: &Pubkey,
    ) -> Result<(Vec<Instruction>, Pubkey)> {
        // pool-info and global-config are independent reads — run them
        // concurrently; reserves depend on the resolved pool, so it follows.
        let (pool, cfg) = tokio::try_join!(
            self.amm_pool_info(token_mint, pool_override, base_token_program_id),
            self.amm_config(),
        )?;
        let (base_res, quote_res) = self.amm_reserves_cached(token_mint, &pool).await?;

        // Guard the real spend (NaN/∞, non-positive, oversized, or rounds-to-zero)
        // before building the swap — the AMM public entry, mirroring the curve path.
        let spendable = self.buy_lamports_checked(sol_amount)?;
        let fee_bps = (cfg.lp_fee_bps + cfg.protocol_fee_bps + cfg.coin_creator_fee_bps) as u128;
        // A garbage/misread global-config (fee >= 100%) would wrap `BPS_DENOM -
        // fee_bps` in release and silently drop slippage protection on a real
        // spend — bail instead. `saturating_sub` keeps `slip` (caller input) safe
        // even past 100%.
        if fee_bps >= BPS_DENOM {
            bail!("amm buy: fee_bps {fee_bps} >= 100% (bad global_config)");
        }

        // Fee is taken off the quote (SOL) side before the curve swap.
        let quote_net = (spendable as u128).saturating_mul(BPS_DENOM.saturating_sub(fee_bps)) / BPS_DENOM;
        let base_out = cp_amount_out(quote_net, quote_res, base_res);
        // Exact-base-out buy: request slightly fewer tokens than the budget
        // buys (the slippage haircut) so the actual cost stays under the
        // wrapped `spendable`, which is the spend cap. `None` = no floor (1).
        let base_amount_out: u64 = match slippage_bps {
            None => 1,
            Some(slip) => {
                let s = slip as u128;
                (base_out.saturating_mul(BPS_DENOM.saturating_sub(s)) / BPS_DENOM).max(1) as u64
            }
        };

        let quote_tp = protocol::TOKEN; // WSOL is legacy SPL
        let user_base =
            get_associated_token_address_with_program_id(user, &pool.base_mint, &pool.base_token_program);
        let user_quote =
            get_associated_token_address_with_program_id(user, &protocol::WSOL_MINT, &quote_tp);

        let mut ixs = Vec::with_capacity(6);
        // Ensure the base-token account exists (tokens land here).
        ixs.push(create_associated_token_account_idempotent(
            user,
            user,
            &pool.base_mint,
            &pool.base_token_program,
        ));
        // Wrap `spendable` lamports of SOL into the WSOL account.
        ixs.push(create_associated_token_account_idempotent(
            user,
            user,
            &protocol::WSOL_MINT,
            &quote_tp,
        ));
        ixs.push(system_instruction::transfer(user, &user_quote, spendable));
        ixs.push(spl_token::instruction::sync_native(&quote_tp, &user_quote)?);

        let mut data = BUY_DISC.to_vec();
        data.extend_from_slice(&base_amount_out.to_le_bytes()); // base_amount_out
        data.extend_from_slice(&spendable.to_le_bytes()); // max_quote_amount_in
        // track_volume: OptionBool (1 byte). Only accrue cashback volume for
        // cashback coins — accruing requires the user_volume_accumulator to be
        // initialized, so default off for non-cashback tokens.
        data.push(u8::from(pool.is_cashback_coin));
        ixs.push(Instruction {
            program_id: protocol::PUMP_SWAP,
            accounts: self.amm_swap_accounts(
                &pool, user, &cfg, user_base, user_quote, quote_tp, true,
            ),
            data,
        });

        // Unwrap any leftover WSOL (and recover the account rent) back to SOL.
        ixs.push(spl_token::instruction::close_account(
            &quote_tp, &user_quote, user, user, &[],
        )?);

        Ok((ixs, user_base))
    }

    /// `pub(super)` for the same reason as [`Self::build_amm_buy_ixs`]: the sim
    /// engine reuses this exact builder so the dry-run matches the live sell.
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn build_amm_sell_ixs(
        &self,
        token_mint: &str,
        token_amount: u64,
        base_token_program_id: &str,
        pool_override: Option<&str>,
        token_account_override: Option<&str>,
        slippage_bps: Option<u64>,
        user: &Pubkey,
    ) -> Result<Vec<Instruction>> {
        // pool-info and global-config are independent reads — run them
        // concurrently; reserves depend on the resolved pool, so it follows.
        let (pool, cfg) = tokio::try_join!(
            self.amm_pool_info(token_mint, pool_override, base_token_program_id),
            self.amm_config(),
        )?;
        let (base_res, quote_res) = self.amm_reserves_cached(token_mint, &pool).await?;

        let fee_bps = (cfg.lp_fee_bps + cfg.protocol_fee_bps + cfg.coin_creator_fee_bps) as u128;
        // See `build_amm_buy_ixs`: bail on a >= 100% fee rather than wrap the
        // slippage denominator; `saturating_sub` guards the caller-supplied slip.
        if fee_bps >= BPS_DENOM {
            bail!("amm sell: fee_bps {fee_bps} >= 100% (bad global_config)");
        }

        let gross = cp_amount_out(token_amount as u128, base_res, quote_res);
        let net = gross.saturating_mul(BPS_DENOM.saturating_sub(fee_bps)) / BPS_DENOM;
        // `None` = no floor (min_out = 1): always fills regardless of price
        // movement. This is the correct default for bot sells where clearing the
        // position matters more than getting a precise price.
        let min_quote_out: u64 = match slippage_bps {
            None => 1,
            Some(slip) => {
                let s = slip as u128;
                (net.saturating_mul(BPS_DENOM.saturating_sub(s)) / BPS_DENOM).max(1) as u64
            }
        };

        let quote_tp = protocol::TOKEN;
        let user_base = self
            .resolve_user_base_account(token_mint, token_account_override)
            .await?;
        let user_quote =
            get_associated_token_address_with_program_id(user, &protocol::WSOL_MINT, &quote_tp);

        let mut ixs = Vec::with_capacity(3);
        // Ensure a WSOL account exists to receive proceeds.
        ixs.push(create_associated_token_account_idempotent(
            user,
            user,
            &protocol::WSOL_MINT,
            &quote_tp,
        ));

        let mut data = SELL_DISC.to_vec();
        data.extend_from_slice(&token_amount.to_le_bytes());
        data.extend_from_slice(&min_quote_out.to_le_bytes());
        ixs.push(Instruction {
            program_id: protocol::PUMP_SWAP,
            accounts: self.amm_swap_accounts(
                &pool, user, &cfg, user_base, user_quote, quote_tp, false,
            ),
            data,
        });

        // Unwrap WSOL proceeds (and recover the WSOL-account rent) back to SOL.
        // The base token-account rent is recovered off this path by a separate
        // post-clear `close_token_account` tx, not bundled here — a bundled close
        // would revert the whole sell if any base dust remained.
        ixs.push(spl_token::instruction::close_account(
            &quote_tp, &user_quote, user, user, &[],
        )?);

        Ok(ixs)
    }

    /// Build the swap instruction's account list, matching the pump_amm IDL
    /// exactly. `with_volume` adds `global/user_volume_accumulator` (present on
    /// buy, absent on sell).
    #[allow(clippy::too_many_arguments)]
    fn amm_swap_accounts(
        &self,
        pool: &AmmPoolInfo,
        user: &Pubkey,
        cfg: &AmmGlobalConfig,
        user_base: Pubkey,
        user_quote: Pubkey,
        quote_token_program: Pubkey,
        with_volume: bool,
    ) -> Vec<AccountMeta> {
        let global_config = self.amm_global_config_pda;
        let event_authority = self.amm_event_authority;
        // Derived once at AmmPoolInfo construction (both the RPC and the harvest
        // path fill them), so the builder just reads.
        let cc_authority = pool.coin_creator_vault_authority;
        let cc_ata = pool.coin_creator_vault_ata;
        let fee_config = self.amm_fee_config;
        // The rotating protocol_fee_recipients[0]; accepted by the program.
        let pf_recipient = cfg.protocol_fee_recipient;
        let pf_recipient_ta = get_associated_token_address_with_program_id(
            &pf_recipient,
            &protocol::WSOL_MINT,
            &quote_token_program,
        );

        let mut accounts = vec![
            AccountMeta::new(pool.pool, false),                       // 0  pool
            AccountMeta::new(*user, true),                            // 1  user (signer)
            AccountMeta::new_readonly(global_config, false),          // 2  global_config
            AccountMeta::new_readonly(pool.base_mint, false),         // 3  base_mint
            AccountMeta::new_readonly(pool.quote_mint, false),        // 4  quote_mint
            AccountMeta::new(user_base, false),                       // 5  user_base_token_account
            AccountMeta::new(user_quote, false),                      // 6  user_quote_token_account
            AccountMeta::new(pool.pool_base_token_account, false),    // 7  pool_base_token_account
            AccountMeta::new(pool.pool_quote_token_account, false),   // 8  pool_quote_token_account
            AccountMeta::new_readonly(pf_recipient, false),           // 9  protocol_fee_recipient
            AccountMeta::new(pf_recipient_ta, false),                 // 10 protocol_fee_recipient_token_account
            AccountMeta::new_readonly(pool.base_token_program, false), // 11 base_token_program
            AccountMeta::new_readonly(quote_token_program, false),    // 12 quote_token_program
            AccountMeta::new_readonly(system_program::id(), false),    // 13 system_program
            AccountMeta::new_readonly(spl_associated_token_account::id(), false), // 14 associated_token_program
            AccountMeta::new_readonly(event_authority, false),        // 15 event_authority
            AccountMeta::new_readonly(protocol::PUMP_SWAP, false),    // 16 program
            AccountMeta::new(cc_ata, false),                          // 17 coin_creator_vault_ata
            AccountMeta::new_readonly(cc_authority, false),           // 18 coin_creator_vault_authority
        ];
        if with_volume {
            // global_volume_accumulator is WRITTEN only when cashback volume is
            // tracked; readonly otherwise (matches real swaps). user_volume_
            // accumulator is always writable on the buy path.
            if pool.is_cashback_coin {
                accounts.push(AccountMeta::new(self.amm_global_volume_accumulator, false)); // 19
            } else {
                accounts.push(AccountMeta::new_readonly(self.amm_global_volume_accumulator, false));
            }
            accounts.push(AccountMeta::new(self.amm_user_volume_accumulator, false)); // 20
        }
        accounts.push(AccountMeta::new_readonly(fee_config, false)); // fee_config
        accounts.push(AccountMeta::new_readonly(protocol::FEE_PROGRAM, false)); // fee_program

        // Cashback pools append the user's cashback accumulator + a fixed pfee
        // marker before the buyback pair. The layout differs by side (both
        // verified against live on-chain swaps, PDAs matched):
        //   buy  (volume block present): [cashback_ata(W), marker(r)]      → 27 accts
        //   sell (no volume block):      [cashback_ata(W), uva(W), marker(r)] → 26 accts
        // cashback_ata = ATA(user_volume_accumulator, WSOL). On the sell side the
        // user_volume_accumulator (uva) rides in this tail because there is no
        // earlier volume block to carry it.
        if pool.is_cashback_coin {
            let uva = self.amm_user_volume_accumulator;
            let cashback_ata =
                get_associated_token_address_with_program_id(&uva, &protocol::WSOL_MINT, &quote_token_program);
            accounts.push(AccountMeta::new(cashback_ata, false)); // writable
            if !with_volume {
                accounts.push(AccountMeta::new(uva, false)); // writable, sell-only
            }
            accounts.push(AccountMeta::new_readonly(protocol::PUMP_AMM_CASHBACK_GLOBAL, false));
        } else if let Some(marker) = pool.fee_share_marker {
            // Non-cashback swaps carry a single per-coin "fee-share" marker
            // (readonly) in this slot, on both buy and sell. The deployed program
            // derives it but no published IDL documents it and it isn't
            // reproducible offline — so it's read from a recent on-chain swap and
            // cached on `AmmPoolInfo` (see `fetch_fee_share_marker`).
            accounts.push(AccountMeta::new_readonly(marker, false));
        }

        // Trailing buyback-fee block — required by the deployed pump_amm program
        // but absent from its published IDL: the buyback recipient (readonly)
        // and that recipient's WSOL ATA (writable). Verified against live
        // on-chain swaps. The recipient rotates across a whitelist; we use
        // buyback_fee_recipients[0], which the program accepts.
        let fee_recipient = protocol::PUMP_AMM_BUYBACK_FEE_RECIPIENT;
        let fee_recipient_wsol = get_associated_token_address_with_program_id(
            &fee_recipient,
            &protocol::WSOL_MINT,
            &quote_token_program,
        );
        accounts.push(AccountMeta::new_readonly(fee_recipient, false));
        accounts.push(AccountMeta::new(fee_recipient_wsol, false));
        accounts
    }

    // -----------------------------------------------------------------------
    // Pool / config / reserves
    // -----------------------------------------------------------------------

    /// Canonical pool for a migrated token: PDA(["pool", 0u16, pool_authority,
    /// base_mint, WSOL]), where pool_authority = PDA(["pool-authority", mint])
    /// under the pump.fun program.
    fn derive_amm_pool(&self, mint: &Pubkey) -> Pubkey {
        let (authority, _) =
            Pubkey::find_program_address(&[b"pool-authority", mint.as_ref()], &protocol::PUMP_FUN);
        let index: u16 = 0;
        Pubkey::find_program_address(
            &[
                b"pool",
                &index.to_le_bytes(),
                authority.as_ref(),
                mint.as_ref(),
                protocol::WSOL_MINT.as_ref(),
            ],
            &protocol::PUMP_SWAP,
        )
        .0
    }

    /// Fetch + cache pool facts (vault addresses, coin_creator, cashback flag).
    async fn amm_pool_info(
        &self,
        token_mint: &str,
        pool_override: Option<&str>,
        base_token_program_id: &str,
    ) -> Result<AmmPoolInfo> {
        if let Some(info) = self.amm_pool_cache.get(token_mint).map(|r| *r) {
            debug!(mint = %token_mint, "amm_pool_info cache hit (feed harvest; no RPC)");
            return Ok(info);
        }
        info!(
            mint = %token_mint,
            "amm_pool_info cold path — RPC fallback (no feed harvest since boot)"
        );

        let mint = Pubkey::from_str(token_mint)?;
        let pool = match pool_override {
            Some(p) => Pubkey::from_str(p).context("invalid pool_override")?,
            None => self.derive_amm_pool(&mint),
        };

        let account = self
            .rpc
            .get_account(&pool)
            .await
            .with_context(|| format!("PumpSwap pool {} not found (is the token migrated?)", pool))?;
        let data = &account.data;
        if data.len() < AMM_POOL_MIN_LEN {
            bail!("PumpSwap pool account too short: {} bytes", data.len());
        }

        let is_cashback_coin = data[AMM_POOL_IS_CASHBACK_OFFSET] != 0;
        // Non-cashback swaps require a per-coin fee-share marker the deployed
        // program derives but we can't reproduce offline — recover it from a
        // recent on-chain swap. Cashback pools use a derivable block in that slot
        // and don't need it. Done once per coin (the whole AmmPoolInfo is cached).
        let fee_share_marker = if is_cashback_coin {
            None
        } else {
            match self.fetch_fee_share_marker(&pool).await? {
                Some(m) => Some(m),
                None => bail!(
                    "No recent PumpSwap swap found for pool {} to read its fee-share \
                     marker — cannot build a valid non-cashback AMM swap (token may \
                     have no AMM trades yet)",
                    pool
                ),
            }
        };

        let coin_creator = read_pubkey(data, AMM_POOL_COIN_CREATOR_OFFSET)?;
        let coin_creator_vault_authority = self.amm_coin_creator_vault_authority(&coin_creator);
        // The creator vault is always a WSOL ATA under the LEGACY token program
        // (the AMM quote side is wrapped SOL), regardless of the base program.
        let coin_creator_vault_ata = get_associated_token_address_with_program_id(
            &coin_creator_vault_authority,
            &protocol::WSOL_MINT,
            &protocol::TOKEN,
        );
        let info = AmmPoolInfo {
            pool,
            base_mint: read_pubkey(data, 43)?,
            quote_mint: read_pubkey(data, 75)?,
            base_token_program: Pubkey::from_str(base_token_program_id)?,
            pool_base_token_account: read_pubkey(data, AMM_POOL_BASE_VAULT_OFFSET)?,
            pool_quote_token_account: read_pubkey(data, AMM_POOL_QUOTE_VAULT_OFFSET)?,
            coin_creator,
            coin_creator_vault_ata,
            coin_creator_vault_authority,
            is_cashback_coin,
            fee_share_marker,
        };
        self.amm_pool_cache.insert(token_mint.to_string(), info);
        Ok(info)
    }

    /// Read the per-coin fee-share marker from a recent on-chain swap of `pool`.
    /// The deployed pump_amm places this account 3rd-from-last in every swap
    /// (`[marker, buyback_recipient, buyback_recipient_wsol]`); it's per-coin and
    /// constant, so any recent successful swap yields it. `None` if the pool has
    /// no swap in its recent history.
    ///
    /// COLD path only: the steady-state source of the marker is the passive feed
    /// harvest (`observe_amm_swap_accounts`), so this is reached solely from
    /// manual/exit trades of a pool with no observed swap since boot. The marker
    /// is in every successful swap, so tx #1 almost always suffices — fetch
    /// candidates sequentially with early exit instead of bursting
    /// `getTransaction` calls for all of them.
    async fn fetch_fee_share_marker(&self, pool: &Pubkey) -> Result<Option<Pubkey>> {
        let pool_str = pool.to_string();
        let sigs = self
            .rpc_json("getSignaturesForAddress", json!([pool_str, { "limit": 5 }]))
            .await?;
        let program_str = protocol::PUMP_SWAP.to_string();

        for s in sigs.as_array().into_iter().flatten() {
            // Failed txs don't carry the correct account list — skip up front.
            let errored = s.get("err").map(|e| !e.is_null()).unwrap_or(false);
            if errored {
                continue;
            }
            let Some(sig) = s.get("signature").and_then(|v| v.as_str()) else {
                continue;
            };
            let tx = match self
                .rpc_json(
                    "getTransaction",
                    json!([sig, { "encoding": "jsonParsed", "maxSupportedTransactionVersion": 0 }]),
                )
                .await
            {
                Ok(tx) => tx,
                // A single failed lookup shouldn't sink the whole resolution —
                // the next candidate may still yield the marker.
                Err(_) => continue,
            };
            if let Some(marker) = extract_swap_marker(&tx, &program_str, &pool_str) {
                return Ok(Some(Pubkey::from_str(&marker)?));
            }
        }
        Ok(None)
    }

    /// Minimal JSON-RPC call against the configured full RPC node, used for the
    /// read-only transaction-history lookups above.
    async fn rpc_json(&self, method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        rpc_json_call(&self.http, &self.config.rpc_url, method, params).await
    }

    /// Fetch + cache GlobalConfig (fee bps + a protocol fee recipient). Cached
    /// up to [`AMM_CONFIG_MAX_AGE`]: fee bps are governance-mutable, so a
    /// process-lifetime cache would silently keep slippage protection loose after
    /// a fee raise until restart.
    ///
    /// Stale-while-revalidate: past the max age the STALE value is served
    /// immediately and a background refresh is spawned, so a trade never blocks
    /// on this RPC read once the process has fetched the config once. (Fee bps
    /// only feed the slippage floor math — one stale window is harmless; the
    /// old RPC pool-prewarm used to keep this warm as a side effect, and that
    /// prewarm is gone.) Only the very first call per process fetches inline.
    async fn amm_config(&self) -> Result<AmmGlobalConfig> {
        if let Some((c, fetched)) = *self.amm_global_config.lock().unwrap_or_else(|p| p.into_inner()) {
            if fetched.elapsed() >= AMM_CONFIG_MAX_AGE {
                self.spawn_amm_config_refresh();
            }
            return Ok(c);
        }

        // Cold (first call this process) — fetch inline.
        let cfg = fetch_amm_config(&self.rpc, &self.amm_global_config_pda).await?;
        *self.amm_global_config.lock().unwrap_or_else(|p| p.into_inner()) = Some((cfg, Instant::now()));
        Ok(cfg)
    }

    /// Background revalidation for [`Self::amm_config`]. De-duplicated via the
    /// `amm_config_refresh_inflight` flag so a burst of stale reads spawns one
    /// task; failures are dropped (the stale value keeps serving, the next
    /// stale read re-tries).
    fn spawn_amm_config_refresh(&self) {
        use std::sync::atomic::Ordering;
        if self.amm_config_refresh_inflight.swap(true, Ordering::AcqRel) {
            return;
        }
        let rpc = self.rpc.clone();
        let pda = self.amm_global_config_pda;
        let cache = self.amm_global_config.clone();
        let inflight = self.amm_config_refresh_inflight.clone();
        tokio::spawn(async move {
            if let Ok(cfg) = fetch_amm_config(&rpc, &pda).await {
                *cache.lock().unwrap_or_else(|p| p.into_inner()) = Some((cfg, Instant::now()));
            }
            inflight.store(false, Ordering::Release);
        });
    }

    /// Current pool reserves = the base/quote vault token balances (raw units).
    async fn amm_reserves(&self, pool: &AmmPoolInfo) -> Result<(u128, u128)> {
        // Both vault balances in a single request (getMultipleAccounts). Read the
        // raw SPL `amount` (u64 @ offset 64, after mint[32] + owner[32]) — the base
        // token-account layout is identical for Token and Token-2022.
        let accounts = self
            .rpc
            .get_multiple_accounts(&[
                pool.pool_base_token_account,
                pool.pool_quote_token_account,
            ])
            .await
            .context("read pool reserves")?;
        let [base, quote]: [Option<_>; 2] = accounts.try_into().map_err(|_| {
            TradeError::Other("getMultipleAccounts returned an unexpected count".into())
        })?;
        let base = base.context("pool base vault not found")?;
        let quote = quote.context("pool quote vault not found")?;
        let base_res = read_u64(&base.data, 64)? as u128;
        let quote_res = read_u64(&quote.data, 64)? as u128;
        if base_res == 0 || quote_res == 0 {
            bail!("PumpSwap pool has zero reserves");
        }
        Ok((base_res, quote_res))
    }

    /// Pool reserves with a WS-cache fast path: serve a fresh AMM snapshot for
    /// `mint` (same `(base, quote=lamports)` units as [`amm_reserves`]) when one
    /// is available, otherwise read the vault balances on-chain. Curve snapshots
    /// are never served here (the cache is venue-tagged), so a just-migrated
    /// token reads on-chain until its first AMM trade lands.
    async fn amm_reserves_cached(
        &self,
        mint: &str,
        pool: &AmmPoolInfo,
    ) -> Result<(u128, u128)> {
        if let Some(r) = self.reserve_cache.get_fresh(
            mint,
            std::time::Duration::from_millis(self.config.cache.reserve_max_age_ms),
            true,
        ) {
            return Ok(r);
        }
        self.amm_reserves(pool).await
    }

    /// Harvest the per-token AMM pool facts from one observed PumpSwap buy/sell
    /// account list — the zero-RPC replacement for the old RPC pre-warm. Every
    /// swap instruction's accounts carry everything a future swap of the same
    /// coin needs: pool, both vaults, the creator-vault pair, and the tail that
    /// both discriminates cashback vs non-cashback AND carries the fee-share
    /// marker. Pure CPU (no I/O), so it's safe to call inline from an ingest
    /// consumer.
    ///
    /// `keys` is the fully resolved (ALT-included) account list of ONE top-level
    /// pump_amm `buy`/`sell` instruction, in instruction order. Returns `true`
    /// when the trader cache is warm for `token_mint` (already cached, or this
    /// parse succeeded); `false` on any mismatch — no side effects, the caller
    /// simply retries on the next observed swap, and the cold RPC path inside
    /// [`Self::amm_pool_info`] still covers a never-observed pool.
    ///
    /// Head indices mirror [`Self::amm_swap_accounts`] (the builder is the SSOT
    /// for the layout; the round-trip guard test pins the two together). The
    /// tail is addressed FROM THE END because the middle differs by side/coin:
    /// `[.., marker | cashback block, buyback_recipient, buyback_recipient_wsol]`.
    pub fn observe_amm_swap_accounts(
        &self,
        token_mint: &str,
        base_token_program_id: &str,
        keys: &[String],
    ) -> bool {
        if self.amm_pool_cache.contains_key(token_mint) {
            return true;
        }
        let Some(info) = self.parse_amm_swap_accounts(token_mint, base_token_program_id, keys)
        else {
            return false;
        };
        info!(
            mint = %token_mint,
            pool = %info.pool,
            cashback = info.is_cashback_coin,
            fee_share_marker = ?info.fee_share_marker.map(|p| p.to_string()),
            creator_vault = %info.coin_creator_vault_authority,
            "AMM pool facts harvested from feed (zero RPC)"
        );
        self.amm_pool_cache.insert(token_mint.to_string(), info);
        true
    }

    /// The pure parse half of [`Self::observe_amm_swap_accounts`]. `None` ⇒ the
    /// list is not a recognizable canonical-pool swap for `token_mint` (length
    /// drift after a program upgrade, wrong pool, tail sanity failure, …) — fail
    /// SAFE to the cold RPC path rather than cache a wrong layout.
    fn parse_amm_swap_accounts(
        &self,
        token_mint: &str,
        base_token_program_id: &str,
        keys: &[String],
    ) -> Option<AmmPoolInfo> {
        // Smallest known swap list is the non-cashback sell (24 accounts); the
        // largest is the cashback buy (27). Anything outside that band is a
        // layout we don't know.
        if keys.len() < 24 || keys.len() > 27 {
            return None;
        }
        let pk = |i: usize| Pubkey::from_str(keys.get(i)?.as_str()).ok();

        // Head, by fixed IDL index (mirrors `amm_swap_accounts`).
        let pool = pk(0)?;
        let base_mint = pk(3)?;
        let quote_mint = pk(4)?;
        let pool_base_token_account = pk(7)?;
        let pool_quote_token_account = pk(8)?;
        let base_token_program = pk(11)?;
        let coin_creator_vault_ata = pk(17)?;
        let coin_creator_vault_authority = pk(18)?;

        // Sanity: canonical pool for this mint, WSOL quote, and the base token
        // program the caller believes the token uses.
        let mint = Pubkey::from_str(token_mint).ok()?;
        if base_mint != mint
            || quote_mint != protocol::WSOL_MINT
            || pool != self.derive_amm_pool(&mint)
            || base_token_program != Pubkey::from_str(base_token_program_id).ok()?
        {
            return None;
        }
        // The creator vault ATA must be derivable from the authority next to it
        // — hardens against an index shift moving unrelated accounts into 17/18.
        let expect_ata = get_associated_token_address_with_program_id(
            &coin_creator_vault_authority,
            &protocol::WSOL_MINT,
            &protocol::TOKEN,
        );
        if coin_creator_vault_ata != expect_ata {
            return None;
        }

        // Tail, by position from the end. `len-2` is the buyback fee recipient
        // in every known layout — reject otherwise so a program upgrade that
        // reshapes the tail fails safe to the cold path.
        if pk(keys.len() - 2)? != protocol::PUMP_AMM_BUYBACK_FEE_RECIPIENT {
            return None;
        }
        let slot3 = pk(keys.len() - 3)?;
        let (is_cashback_coin, fee_share_marker) = if slot3 == protocol::PUMP_AMM_CASHBACK_GLOBAL {
            (true, None)
        } else {
            (false, Some(slot3))
        };

        Some(AmmPoolInfo {
            pool,
            base_mint,
            quote_mint,
            base_token_program,
            pool_base_token_account,
            pool_quote_token_account,
            // Not present in a swap's account list — see the field docs; the
            // 2006 self-heal re-reads the pool from chain, never this value.
            coin_creator: Pubkey::default(),
            coin_creator_vault_ata,
            coin_creator_vault_authority,
            is_cashback_coin,
            fee_share_marker,
        })
    }

    /// Force-refresh the cached [`AmmPoolInfo`] for `token_mint` by evicting the
    /// entry and re-reading the pool account, returning the freshly derived
    /// `coin_creator_vault_authority` **only if the pool's `coin_creator` actually
    /// changed** (`None` when the re-read matches what we already had).
    ///
    /// PumpSwap can populate/rotate a pool's `coin_creator` AFTER our first read
    /// cached it (`amm_pool_cache` has no TTL). The stale `coin_creator` then
    /// derives the wrong `coin_creator_vault_authority`, and every AMM sell reverts
    /// with Anchor `ConstraintSeeds` (2006) — the AMM analogue of the curve
    /// `set_creator` case [`Self::refresh_curve_creator_vault`] handles. After this
    /// call the cache holds the fresh pool, so the next `amm_sell` builds with the
    /// correct authority. Returning `None` (unchanged) lets the caller stop instead
    /// of re-paying fees on a retry that would derive the identical authority.
    /// OFF the hot path — the exit loop calls this only after an AMM 2006 revert.
    pub async fn refresh_amm_pool_info(
        &self,
        token_mint: &str,
        base_token_program_id: &str,
    ) -> Result<Option<Pubkey>> {
        // Compare the derived vault AUTHORITY, not the raw creator: the authority
        // is what the failing swap actually referenced, and a feed-harvested
        // entry doesn't know its `coin_creator` (only the derived pair).
        let prev_authority = self
            .amm_pool_cache
            .get(token_mint)
            .map(|r| r.coin_creator_vault_authority);
        // Evict so `amm_pool_info` performs a fresh on-chain read (canonical pool,
        // matching the sell path's `pool_override = None`) instead of serving the
        // stale cached entry.
        self.amm_pool_cache.remove(token_mint);
        let pool = self
            .amm_pool_info(token_mint, None, base_token_program_id)
            .await?;
        if prev_authority == Some(pool.coin_creator_vault_authority) {
            return Ok(None);
        }
        Ok(Some(pool.coin_creator_vault_authority))
    }

    /// override → cache → on-chain lookup (mirrors `sell_token`).
    pub(super) async fn resolve_user_base_account(
        &self,
        token_mint: &str,
        token_account_override: Option<&str>,
    ) -> Result<Pubkey> {
        if let Some(o) = token_account_override {
            return Pubkey::from_str(o).context("invalid token_account_override");
        }
        if let Some(pk) = self.user_token_accounts.get(token_mint).map(|r| *r) {
            return Ok(pk);
        }
        let holdings = self.get_all_token_accounts().await?;
        match holdings.iter().find(|h| h.mint == token_mint) {
            Some(h) => Pubkey::from_str(&h.token_account).context("invalid token account pubkey"),
            None => bail!("No token account found for mint {}", token_mint),
        }
    }

    // -----------------------------------------------------------------------
    // PDA helpers (program = pump_amm unless noted)
    // -----------------------------------------------------------------------

    /// Per-coin (not program-constant) vault authority, so it stays derived here
    /// rather than precomputed in `new()` like the other AMM PDAs.
    fn amm_coin_creator_vault_authority(&self, coin_creator: &Pubkey) -> Pubkey {
        Pubkey::find_program_address(
            &[b"creator_vault", coin_creator.as_ref()],
            &protocol::PUMP_SWAP,
        )
        .0
    }
}

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

/// Find a pump_amm buy/sell instruction (top-level or inner) for `pool` in a
/// jsonParsed transaction and return its fee-share marker — the 3rd-from-last
/// account. A swap is identified by program id, `accounts[0] == pool`, and the
/// buy/sell discriminator in its data (so deposit/withdraw don't match).
fn extract_swap_marker(tx: &serde_json::Value, program: &str, pool: &str) -> Option<String> {
    let msg = tx.get("transaction")?.get("message")?;

    let mut ixs: Vec<&serde_json::Value> = Vec::new();
    if let Some(arr) = msg.get("instructions").and_then(|v| v.as_array()) {
        ixs.extend(arr.iter());
    }
    if let Some(groups) = tx
        .get("meta")
        .and_then(|m| m.get("innerInstructions"))
        .and_then(|v| v.as_array())
    {
        for g in groups {
            if let Some(arr) = g.get("instructions").and_then(|v| v.as_array()) {
                ixs.extend(arr.iter());
            }
        }
    }

    for ix in ixs {
        if ix.get("programId").and_then(|v| v.as_str()) != Some(program) {
            continue;
        }
        let accounts = match ix.get("accounts").and_then(|v| v.as_array()) {
            Some(a) => a,
            None => continue,
        };
        if accounts.first().and_then(|v| v.as_str()) != Some(pool) {
            continue;
        }
        // Confirm buy/sell (not deposit/withdraw) via the instruction discriminator.
        let data = ix.get("data").and_then(|v| v.as_str()).unwrap_or("");
        let is_swap = bs58::decode(data)
            .into_vec()
            .ok()
            .map(|d| d.starts_with(&BUY_DISC) || d.starts_with(&SELL_DISC))
            .unwrap_or(false);
        if !is_swap || accounts.len() < 3 {
            continue;
        }
        // marker = 3rd-from-last ([marker, buyback_recipient, buyback_recipient_wsol]).
        return accounts[accounts.len() - 3].as_str().map(str::to_string);
    }
    None
}

/// Fetch + parse the PumpSwap `GlobalConfig` account. Free function (no `&self`)
/// so the background revalidation task in `spawn_amm_config_refresh` can run it
/// detached; `amm_config`'s inline cold path uses the same code.
async fn fetch_amm_config(
    rpc: &solana_client::nonblocking::rpc_client::RpcClient,
    pda: &Pubkey,
) -> Result<AmmGlobalConfig> {
    let account = rpc
        .get_account(pda)
        .await
        .context("Failed to fetch PumpSwap global_config")?;
    let data = &account.data;
    if data.len() < AMM_CONFIG_MIN_LEN {
        bail!("PumpSwap global_config too short: {} bytes", data.len());
    }

    let lp_fee_bps = read_u64(data, AMM_CONFIG_LP_FEE_BPS_OFFSET)?;
    let protocol_fee_bps = read_u64(data, AMM_CONFIG_PROTOCOL_FEE_BPS_OFFSET)?;
    let coin_creator_fee_bps = read_u64(data, AMM_CONFIG_COIN_CREATOR_FEE_BPS_OFFSET)?;

    // protocol_fee_recipients is [pubkey; 8]; pick the first non-default.
    let mut protocol_fee_recipient = Pubkey::default();
    for i in 0..8 {
        let pk = read_pubkey(data, AMM_CONFIG_FEE_RECIPIENTS_OFFSET + i * 32)?;
        if pk != Pubkey::default() {
            protocol_fee_recipient = pk;
            break;
        }
    }
    if protocol_fee_recipient == Pubkey::default() {
        bail!("No protocol fee recipient in PumpSwap global_config");
    }

    Ok(AmmGlobalConfig {
        lp_fee_bps,
        protocol_fee_bps,
        coin_creator_fee_bps,
        protocol_fee_recipient,
    })
}

/// Minimal JSON-RPC POST against a full RPC node.
/// `PumpFunTrader::rpc_json` is a thin wrapper over it.
async fn rpc_json_call(
    http: &reqwest::Client,
    rpc_url: &str,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value> {
    // Visible in the smoke "RPC log" — `getTransaction` here is the cold
    // fee-share-marker fallback only; steady-state harvest must keep this quiet.
    info!(method = method, "executor RPC read");
    let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
    let resp = http
        .post(rpc_url)
        .json(&body)
        .send()
        .await
        .with_context(|| format!("RPC {method} request failed"))?;
    let v: serde_json::Value = resp.json().await.context("RPC response not JSON")?;
    if let Some(e) = v.get("error") {
        bail!("RPC {method} error: {e}");
    }
    Ok(v.get("result").cloned().unwrap_or(serde_json::Value::Null))
}

/// Constant-product output amount: `reserve_out * amount_in / (reserve_in + amount_in)`.
fn cp_amount_out(amount_in: u128, reserve_in: u128, reserve_out: u128) -> u128 {
    if amount_in == 0 || reserve_in == 0 {
        return 0;
    }
    reserve_out.saturating_mul(amount_in) / reserve_in.saturating_add(amount_in)
}

fn read_pubkey(data: &[u8], off: usize) -> Result<Pubkey> {
    let end = off + 32;
    if data.len() < end {
        bail!("account data too short for pubkey at offset {}", off);
    }
    Pubkey::try_from(&data[off..end])
        .map_err(|_| TradeError::Other(format!("bad pubkey at offset {off}")))
}

fn read_u64(data: &[u8], off: usize) -> Result<u64> {
    let end = off + 8;
    if data.len() < end {
        bail!("account data too short for u64 at offset {}", off);
    }
    Ok(u64::from_le_bytes(data[off..end].try_into().unwrap()))
}

#[cfg(test)]
mod tests {
    use super::{read_u64, AmmGlobalConfig, AmmPoolInfo, PumpFunTrader, BUY_DISC, SELL_DISC};
    use crate::config::ComputeBudgetCfg;
    use crate::protocol::{self, TOKEN_2022_PROGRAM_ID};
    use crate::trader::TraderConfig;
    use solana_sdk::{
        compute_budget::ComputeBudgetInstruction, instruction::Instruction, message::Message,
        pubkey::Pubkey, signature::Keypair, system_instruction,
    };
    use spl_associated_token_account::{
        get_associated_token_address_with_program_id,
        instruction::create_associated_token_account_idempotent,
    };
    use std::str::FromStr;
    use std::sync::Arc;

    /// Hard Solana transaction wire-size limit (bytes).
    const TX_LIMIT: usize = 1232;

    #[test]
    fn token_account_amount_lives_at_offset_64() {
        // SPL / Token-2022 token account base layout: mint[0..32], owner[32..64],
        // amount: u64 LE @ 64. `amm_reserves` reads vault balances from this offset.
        let mut data = vec![0u8; 165];
        let amount: u64 = 987_654_321;
        data[64..72].copy_from_slice(&amount.to_le_bytes());
        assert_eq!(read_u64(&data, 64).unwrap(), amount);
    }

    #[test]
    fn read_u64_rejects_short_buffer() {
        assert!(read_u64(&[0u8; 40], 64).is_err());
    }

    // --- AMM swap transaction-size guards -------------------------------------
    //
    // Build the worst-case (cashback coin, Token-2022 base) AMM buy/sell with the
    // REAL account list (`amm_swap_accounts`) + the real wrapper instructions, and
    // measure the on-wire size. These lock in the nonce-vs-blockhash decision:
    //   - the sell (durable nonce) must stay under the limit;
    //   - the buy fits with a recent blockhash but NOT with a nonce-advance —
    //     which is exactly why `amm_buy` uses a recent blockhash.

    fn dummy_trader() -> PumpFunTrader {
        let config = Arc::new(TraderConfig::new(
            "http://localhost".into(),
            vec!["http://localhost".into()],
            Arc::new(Keypair::new()),
            vec![Pubkey::new_unique()],
        ));
        PumpFunTrader::new(config)
    }

    fn cfg() -> AmmGlobalConfig {
        AmmGlobalConfig {
            lp_fee_bps: 20,
            protocol_fee_bps: 5,
            coin_creator_fee_bps: 5,
            protocol_fee_recipient: Pubkey::new_unique(),
        }
    }

    /// Worst case for tx size: a cashback coin (largest swap account list) with a
    /// Token-2022 base program (distinct from the legacy quote program, so the two
    /// token-program accounts don't dedup).
    fn worst_case_pool() -> AmmPoolInfo {
        AmmPoolInfo {
            pool: Pubkey::new_unique(),
            base_mint: Pubkey::new_unique(),
            quote_mint: protocol::WSOL_MINT,
            base_token_program: Pubkey::from_str(TOKEN_2022_PROGRAM_ID).unwrap(),
            pool_base_token_account: Pubkey::new_unique(),
            pool_quote_token_account: Pubkey::new_unique(),
            coin_creator: Pubkey::new_unique(),
            coin_creator_vault_ata: Pubkey::new_unique(),
            coin_creator_vault_authority: Pubkey::new_unique(),
            is_cashback_coin: true,
            fee_share_marker: None,
        }
    }

    fn compute_budget_ixs() -> Vec<Instruction> {
        let c = ComputeBudgetCfg::default();
        vec![
            ComputeBudgetInstruction::set_compute_unit_limit(c.amm_cu),
            ComputeBudgetInstruction::set_compute_unit_price(c.price_micro_lamports),
        ]
    }

    fn build_buy_ixs(t: &PumpFunTrader, user: &Pubkey) -> Vec<Instruction> {
        let legacy = protocol::TOKEN;
        let wsol = protocol::WSOL_MINT;
        let pool = worst_case_pool();
        let user_base =
            get_associated_token_address_with_program_id(user, &pool.base_mint, &pool.base_token_program);
        let user_quote = get_associated_token_address_with_program_id(user, &wsol, &legacy);
        let mut ixs = compute_budget_ixs();
        ixs.push(create_associated_token_account_idempotent(
            user,
            user,
            &pool.base_mint,
            &pool.base_token_program,
        ));
        ixs.push(create_associated_token_account_idempotent(user, user, &wsol, &legacy));
        ixs.push(system_instruction::transfer(user, &user_quote, 1_000_000));
        ixs.push(spl_token::instruction::sync_native(&legacy, &user_quote).unwrap());
        let mut data = BUY_DISC.to_vec();
        data.extend_from_slice(&0u64.to_le_bytes()); // base_amount_out
        data.extend_from_slice(&0u64.to_le_bytes()); // max_quote_amount_in
        data.push(1); // track_volume
        ixs.push(Instruction {
            program_id: protocol::PUMP_SWAP,
            accounts: t.amm_swap_accounts(&pool, user, &cfg(), user_base, user_quote, legacy, true),
            data,
        });
        ixs.push(spl_token::instruction::close_account(&legacy, &user_quote, user, user, &[]).unwrap());
        ixs.push(system_instruction::transfer(user, &Pubkey::new_unique(), 200_000)); // jito tip
        ixs
    }

    fn build_sell_ixs(t: &PumpFunTrader, user: &Pubkey) -> Vec<Instruction> {
        let legacy = protocol::TOKEN;
        let wsol = protocol::WSOL_MINT;
        let pool = worst_case_pool();
        let user_base =
            get_associated_token_address_with_program_id(user, &pool.base_mint, &pool.base_token_program);
        let user_quote = get_associated_token_address_with_program_id(user, &wsol, &legacy);
        let mut ixs = compute_budget_ixs();
        ixs.push(create_associated_token_account_idempotent(user, user, &wsol, &legacy));
        let mut data = SELL_DISC.to_vec();
        data.extend_from_slice(&0u64.to_le_bytes()); // base_amount_in
        data.extend_from_slice(&0u64.to_le_bytes()); // min_quote_amount_out
        ixs.push(Instruction {
            program_id: protocol::PUMP_SWAP,
            accounts: t.amm_swap_accounts(&pool, user, &cfg(), user_base, user_quote, legacy, false),
            data,
        });
        ixs.push(spl_token::instruction::close_account(&legacy, &user_quote, user, user, &[]).unwrap());
        ixs.push(system_instruction::transfer(user, &Pubkey::new_unique(), 200_000)); // jito tip
        ixs
    }

    fn wire_size(msg: &Message) -> usize {
        // 1-byte signature count (compact-u16, 1 sig) + 64 B per signature + message.
        1 + 64 * msg.header.num_required_signatures as usize + msg.serialize().len()
    }

    // --- Harvest parser ↔ builder round-trip guard ---------------------------
    //
    // `observe_amm_swap_accounts` parses a swap's account list by the SAME fixed
    // indices `amm_swap_accounts` builds with. These tests pin the two to one
    // layout forever: build accounts for a synthetic pool (all four side × coin
    // variants), feed them to the parser, and require the round-tripped
    // `AmmPoolInfo` to equal the input. No DB / no network — runs on plain
    // `cargo test`.

    /// A pool whose derived facts are internally consistent, so the parser's
    /// sanity checks (canonical pool PDA, creator-vault ATA derivation, WSOL
    /// quote) all pass — i.e. what a real observed swap of `mint` looks like.
    fn harvestable_pool(
        t: &PumpFunTrader,
        mint: Pubkey,
        base_token_program: Pubkey,
        is_cashback_coin: bool,
    ) -> AmmPoolInfo {
        let coin_creator = Pubkey::new_unique();
        let authority = Pubkey::find_program_address(
            &[b"creator_vault", coin_creator.as_ref()],
            &protocol::PUMP_SWAP,
        )
        .0;
        let ata = get_associated_token_address_with_program_id(
            &authority,
            &protocol::WSOL_MINT,
            &protocol::TOKEN,
        );
        AmmPoolInfo {
            pool: t.derive_amm_pool(&mint),
            base_mint: mint,
            quote_mint: protocol::WSOL_MINT,
            base_token_program,
            pool_base_token_account: Pubkey::new_unique(),
            pool_quote_token_account: Pubkey::new_unique(),
            coin_creator,
            coin_creator_vault_ata: ata,
            coin_creator_vault_authority: authority,
            is_cashback_coin,
            fee_share_marker: if is_cashback_coin { None } else { Some(Pubkey::new_unique()) },
        }
    }

    fn swap_keys(t: &PumpFunTrader, pool: &AmmPoolInfo, is_buy: bool) -> Vec<String> {
        let user = Pubkey::new_unique();
        let legacy = protocol::TOKEN;
        let user_base = get_associated_token_address_with_program_id(
            &user,
            &pool.base_mint,
            &pool.base_token_program,
        );
        let user_quote =
            get_associated_token_address_with_program_id(&user, &protocol::WSOL_MINT, &legacy);
        t.amm_swap_accounts(pool, &user, &cfg(), user_base, user_quote, legacy, is_buy)
            .iter()
            .map(|m| m.pubkey.to_string())
            .collect()
    }

    #[test]
    fn harvest_round_trips_builder_layout() {
        let legacy = protocol::TOKEN;
        let t2022 = Pubkey::from_str(TOKEN_2022_PROGRAM_ID).unwrap();
        // side × coin-kind × base-program variants (list lengths 24–27).
        for (is_buy, is_cashback, base_tp) in [
            (true, false, legacy),
            (false, false, legacy),
            (true, true, t2022),
            (false, true, t2022),
        ] {
            let t = dummy_trader();
            let mint = Pubkey::new_unique();
            let pool = harvestable_pool(&t, mint, base_tp, is_cashback);
            let keys = swap_keys(&t, &pool, is_buy);

            assert!(
                t.observe_amm_swap_accounts(&mint.to_string(), &base_tp.to_string(), &keys),
                "harvest rejected a builder-produced list (is_buy={is_buy}, cashback={is_cashback})"
            );
            let got = *t.amm_pool_cache.get(&mint.to_string()).unwrap();
            // A swap's account list doesn't carry the raw coin_creator — the
            // harvested entry stores the default sentinel; everything else must
            // round-trip exactly.
            let expect = AmmPoolInfo { coin_creator: Pubkey::default(), ..pool };
            assert_eq!(got, expect, "is_buy={is_buy}, cashback={is_cashback}");
        }
    }

    #[test]
    fn harvest_rejects_tampered_or_foreign_lists() {
        let t = dummy_trader();
        let mint = Pubkey::new_unique();
        let tp = protocol::TOKEN;
        let pool = harvestable_pool(&t, mint, tp, false);
        let good = swap_keys(&t, &pool, true);

        // Wrong buyback recipient at len-2 (a program upgrade reshaping the
        // tail) must fail safe — no cache insert.
        let mut bad_tail = good.clone();
        let n = bad_tail.len();
        bad_tail[n - 2] = Pubkey::new_unique().to_string();
        assert!(!t.observe_amm_swap_accounts(&mint.to_string(), &tp.to_string(), &bad_tail));

        // Another mint's swap must not warm this mint (pool PDA mismatch).
        let other = Pubkey::new_unique();
        assert!(!t.observe_amm_swap_accounts(&other.to_string(), &tp.to_string(), &good));

        // Truncated list (layout drift) is rejected.
        assert!(!t.observe_amm_swap_accounts(&mint.to_string(), &tp.to_string(), &good[..20]));

        assert!(!t.amm_pool_cache.contains_key(&mint.to_string()));
        assert!(!t.amm_pool_cache.contains_key(&other.to_string()));

        // The untampered list parses — and once cached, observe is a fast no-op true.
        assert!(t.observe_amm_swap_accounts(&mint.to_string(), &tp.to_string(), &good));
        assert!(t.observe_amm_swap_accounts(&mint.to_string(), &tp.to_string(), &good));
    }

    #[test]
    fn cashback_amm_sell_with_nonce_fits() {
        let t = dummy_trader();
        let user = Pubkey::new_unique();
        let nonce = Pubkey::new_unique();
        let msg = Message::new_with_nonce(build_sell_ixs(&t, &user), Some(&user), &nonce, &user);
        let size = wire_size(&msg);
        eprintln!("worst-case AMM sell + nonce     = {size} B (limit {TX_LIMIT})");
        assert!(size <= TX_LIMIT, "cashback AMM sell + nonce = {size} B (limit {TX_LIMIT})");
    }

    #[test]
    fn cashback_amm_buy_with_blockhash_fits() {
        let t = dummy_trader();
        let user = Pubkey::new_unique();
        let msg = Message::new(&build_buy_ixs(&t, &user), Some(&user));
        let size = wire_size(&msg);
        eprintln!("worst-case AMM buy  + blockhash = {size} B (limit {TX_LIMIT})");
        assert!(size <= TX_LIMIT, "cashback AMM buy + blockhash = {size} B (limit {TX_LIMIT})");
    }

    #[test]
    fn cashback_amm_buy_with_nonce_overflows() {
        // The justification for `amm_buy` using a recent blockhash, not a durable
        // nonce: the nonce-advance's extra accounts push the largest buy over the
        // limit. If this ever stops being true, revisit `build_recent_tx`.
        let t = dummy_trader();
        let user = Pubkey::new_unique();
        let nonce = Pubkey::new_unique();
        let msg = Message::new_with_nonce(build_buy_ixs(&t, &user), Some(&user), &nonce, &user);
        let size = wire_size(&msg);
        eprintln!("worst-case AMM buy  + nonce     = {size} B (limit {TX_LIMIT})");
        assert!(size > TX_LIMIT, "expected cashback AMM buy + nonce > {TX_LIMIT}, got {size} B");
    }
}
