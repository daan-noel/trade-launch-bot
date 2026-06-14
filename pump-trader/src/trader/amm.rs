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

use super::{AmmGlobalConfig, AmmPoolInfo, PumpFunTrader};
use crate::constants::{
    AMM_CONFIG_COIN_CREATOR_FEE_BPS_OFFSET, AMM_CONFIG_FEE_RECIPIENTS_OFFSET,
    AMM_CONFIG_LP_FEE_BPS_OFFSET, AMM_CONFIG_MIN_LEN, AMM_CONFIG_PROTOCOL_FEE_BPS_OFFSET,
    AMM_DEFAULT_SLIPPAGE_BPS, AMM_POOL_BASE_VAULT_OFFSET, AMM_POOL_COIN_CREATOR_OFFSET,
    AMM_POOL_IS_CASHBACK_OFFSET, AMM_POOL_MIN_LEN, AMM_POOL_QUOTE_VAULT_OFFSET,
    CONFIRM_MAX_RETRIES, LAMPORTS_PER_SOL, PUMP_AMM_CASHBACK_GLOBAL,
};
use anyhow::{anyhow, bail, Context, Result};
use serde_json::json;
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::Signer,
    system_instruction,
};
use spl_associated_token_account::{
    get_associated_token_address_with_program_id,
    instruction::create_associated_token_account_idempotent,
};
use std::str::FromStr;
use std::time::Instant;
use tracing::info;

// Anchor instruction discriminators (from pump_amm.json). We use the original
// `buy` (exact base out, max quote in); `buy_exact_quote_in` would also work.
//
// NOTE: the deployed pump_amm program is newer than its published IDL. Real
// swaps carry a trailing "upgrade-fee" block (pfee global + recipient + the
// recipient's WSOL ATA) that the IDL omits — without it the program hits an
// Overflow in fee accounting. That block is appended in `amm_swap_accounts`;
// the full layout (buy = 26 accounts, sell = 24) is verified against on-chain
// swaps and a `simulateTransaction` dry-run.
const BUY_DISC: [u8; 8] = [102, 6, 61, 18, 1, 218, 235, 234];
const SELL_DISC: [u8; 8] = [51, 230, 133, 164, 1, 127, 131, 173];

const BPS_DENOM: u128 = 10_000;

impl PumpFunTrader {
    // -----------------------------------------------------------------------
    // Public API
    // -----------------------------------------------------------------------

    /// Buy a migrated token on the PumpSwap AMM, spending `sol_amount` SOL.
    ///
    /// `base_token_program_id` is the token's SPL program (legacy or 2022).
    /// `pool_override` lets the caller supply the known pool address; when
    /// `None` the canonical index-0 / WSOL pool is derived. `slippage_bps`
    /// defaults to [`AMM_DEFAULT_SLIPPAGE_BPS`]. `confirm` mirrors
    /// `sell_token_once`/`amm_sell`: `true` blocks on the RPC signature poll
    /// (manual/API callers); `false` returns once the sender accepts and leaves
    /// confirmation to the caller's own feed — saving the ~4 s `confirm_transaction`
    /// poll on the latency-critical path. The base ATA is cached regardless, so a
    /// `confirm=false` caller can still sell.
    #[allow(clippy::too_many_arguments)]
    pub async fn amm_buy(
        &self,
        token_mint: &str,
        base_token_program_id: &str,
        sol_amount: f64,
        pool_override: Option<&str>,
        slippage_bps: Option<u64>,
        confirm: bool,
    ) -> Result<bool> {
        let t0 = Instant::now();
        let user = self.config.keypair.pubkey();

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

        let mut ixs = Vec::with_capacity(core_ixs.len() + self.cu_ixs_amm.len() + 1);
        ixs.extend_from_slice(&self.cu_ixs_amm);
        ixs.extend(core_ixs);
        ixs.push(self.jito_tip_ix(0));

        // Recent blockhash (not durable nonce): the swap already carries ~27
        // accounts, and a nonce-advance would push the legacy tx over 1232 bytes.
        let tx = self.build_recent_tx(ixs, &self.config.keypair).await?;
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
            self.confirm_transaction(&sig, CONFIRM_MAX_RETRIES).await?;
            info!(
                "✅ AMM buy confirmed — sig: {} | {}ms",
                sig,
                t0.elapsed().as_millis()
            );
        }
        Ok(true)
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
    ) -> Result<bool> {
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
    ) -> Result<bool> {
        let t0 = Instant::now();
        let user = self.config.keypair.pubkey();

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

        let mut ixs = Vec::with_capacity(core_ixs.len() + self.cu_ixs_amm.len() + 1);
        ixs.extend_from_slice(&self.cu_ixs_amm);
        ixs.extend(core_ixs);
        ixs.push(self.jito_tip_ix(tip_level));

        // Acquire the nonce only after `build_amm_sell_ixs`' pool/config/reserve
        // reads — don't hold the slot `in_use` across that RPC. The block below
        // always falls through to `schedule_nonce_refresh`.
        let (nonce_pubkey, nonce_hash) = self.acquire_nonce().await?;
        let result: Result<bool> = async {
            let tx = self.build_nonce_tx(ixs, &nonce_pubkey, nonce_hash, &self.config.keypair)?;
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
                self.confirm_transaction(&sig, CONFIRM_MAX_RETRIES).await?;
                info!(
                    "✅ AMM sell confirmed — sig: {} | {}ms",
                    sig,
                    t0.elapsed().as_millis()
                );
            }
            Ok(true)
        }
        .await;

        self.schedule_nonce_refresh(nonce_pubkey);
        result
    }

    // -----------------------------------------------------------------------
    // Instruction builders
    // -----------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    async fn build_amm_buy_ixs(
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

        let spendable = (sol_amount * LAMPORTS_PER_SOL as f64) as u64;
        let slip = slippage_bps.unwrap_or(AMM_DEFAULT_SLIPPAGE_BPS) as u128;
        let fee_bps = (cfg.lp_fee_bps + cfg.protocol_fee_bps + cfg.coin_creator_fee_bps) as u128;

        // Fee is taken off the quote (SOL) side before the curve swap.
        let quote_net = (spendable as u128).saturating_mul(BPS_DENOM - fee_bps) / BPS_DENOM;
        let base_out = cp_amount_out(quote_net, quote_res, base_res);
        // Exact-base-out buy: request slightly fewer tokens than the budget
        // buys (the slippage haircut) so the actual cost stays under the
        // wrapped `spendable`, which is the spend cap.
        let base_amount_out = (base_out.saturating_mul(BPS_DENOM - slip) / BPS_DENOM) as u64;

        let quote_tp = self.token_program; // WSOL is legacy SPL
        let user_base =
            get_associated_token_address_with_program_id(user, &pool.base_mint, &pool.base_token_program);
        let user_quote =
            get_associated_token_address_with_program_id(user, &self.wsol_mint, &quote_tp);

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
            &self.wsol_mint,
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
            program_id: self.pump_swap_program,
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

    #[allow(clippy::too_many_arguments)]
    async fn build_amm_sell_ixs(
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

        let slip = slippage_bps.unwrap_or(AMM_DEFAULT_SLIPPAGE_BPS) as u128;
        let fee_bps = (cfg.lp_fee_bps + cfg.protocol_fee_bps + cfg.coin_creator_fee_bps) as u128;

        let gross = cp_amount_out(token_amount as u128, base_res, quote_res);
        let net = gross.saturating_mul(BPS_DENOM - fee_bps) / BPS_DENOM;
        let min_quote_out = (net.saturating_mul(BPS_DENOM - slip) / BPS_DENOM) as u64;

        let quote_tp = self.token_program;
        let user_base = self
            .resolve_user_base_account(token_mint, token_account_override)
            .await?;
        let user_quote =
            get_associated_token_address_with_program_id(user, &self.wsol_mint, &quote_tp);

        let mut ixs = Vec::with_capacity(3);
        // Ensure a WSOL account exists to receive proceeds.
        ixs.push(create_associated_token_account_idempotent(
            user,
            user,
            &self.wsol_mint,
            &quote_tp,
        ));

        let mut data = SELL_DISC.to_vec();
        data.extend_from_slice(&token_amount.to_le_bytes());
        data.extend_from_slice(&min_quote_out.to_le_bytes());
        ixs.push(Instruction {
            program_id: self.pump_swap_program,
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
        let cc_authority = self.amm_coin_creator_vault_authority(&pool.coin_creator);
        let cc_ata = get_associated_token_address_with_program_id(
            &cc_authority,
            &self.wsol_mint,
            &quote_token_program,
        );
        let fee_config = self.amm_fee_config;
        // The rotating protocol_fee_recipients[0]; accepted by the program.
        let pf_recipient = cfg.protocol_fee_recipient;
        let pf_recipient_ta = get_associated_token_address_with_program_id(
            &pf_recipient,
            &self.wsol_mint,
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
            AccountMeta::new_readonly(self.system_program, false),    // 13 system_program
            AccountMeta::new_readonly(spl_associated_token_account::id(), false), // 14 associated_token_program
            AccountMeta::new_readonly(event_authority, false),        // 15 event_authority
            AccountMeta::new_readonly(self.pump_swap_program, false), // 16 program
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
        accounts.push(AccountMeta::new_readonly(self.fee_program, false)); // fee_program

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
                get_associated_token_address_with_program_id(&uva, &self.wsol_mint, &quote_token_program);
            accounts.push(AccountMeta::new(cashback_ata, false)); // writable
            if !with_volume {
                accounts.push(AccountMeta::new(uva, false)); // writable, sell-only
            }
            accounts.push(AccountMeta::new_readonly(
                Pubkey::from_str(PUMP_AMM_CASHBACK_GLOBAL).expect("valid PUMP_AMM_CASHBACK_GLOBAL"),
                false,
            ));
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
        let fee_recipient = self.upgrade_fee_recipient;
        let fee_recipient_wsol = get_associated_token_address_with_program_id(
            &fee_recipient,
            &self.wsol_mint,
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
            Pubkey::find_program_address(&[b"pool-authority", mint.as_ref()], &self.pump_program);
        let index: u16 = 0;
        Pubkey::find_program_address(
            &[
                b"pool",
                &index.to_le_bytes(),
                authority.as_ref(),
                mint.as_ref(),
                self.wsol_mint.as_ref(),
            ],
            &self.pump_swap_program,
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
            return Ok(info);
        }

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

        let info = AmmPoolInfo {
            pool,
            base_mint: read_pubkey(data, 43)?,
            quote_mint: read_pubkey(data, 75)?,
            base_token_program: Pubkey::from_str(base_token_program_id)?,
            pool_base_token_account: read_pubkey(data, AMM_POOL_BASE_VAULT_OFFSET)?,
            pool_quote_token_account: read_pubkey(data, AMM_POOL_QUOTE_VAULT_OFFSET)?,
            coin_creator: read_pubkey(data, AMM_POOL_COIN_CREATOR_OFFSET)?,
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
    async fn fetch_fee_share_marker(&self, pool: &Pubkey) -> Result<Option<Pubkey>> {
        let pool_str = pool.to_string();
        let sigs = self
            .rpc_json("getSignaturesForAddress", json!([pool_str, { "limit": 15 }]))
            .await?;
        let program_str = self.pump_swap_program.to_string();

        // The marker is per-coin constant, so *any* successful swap yields it.
        // Fetch the candidate transactions concurrently and take the first marker
        // instead of up to 15 sequential `getTransaction` round-trips gating the
        // (cold) first AMM swap of a coin. Failed txs don't carry the correct
        // account list, so skip them up front.
        let mut set = tokio::task::JoinSet::new();
        for s in sigs.as_array().into_iter().flatten() {
            let errored = s.get("err").map(|e| !e.is_null()).unwrap_or(false);
            if errored {
                continue;
            }
            let Some(sig) = s.get("signature").and_then(|v| v.as_str()) else {
                continue;
            };
            let http = self.http.clone();
            let rpc_url = self.config.rpc_url.clone();
            let sig = sig.to_string();
            set.spawn(async move {
                rpc_json_call(
                    &http,
                    &rpc_url,
                    "getTransaction",
                    json!([sig, { "encoding": "jsonParsed", "maxSupportedTransactionVersion": 0 }]),
                )
                .await
            });
        }

        while let Some(joined) = set.join_next().await {
            let tx = match joined {
                Ok(Ok(tx)) => tx,
                // A single failed lookup (join error or RPC error) shouldn't sink
                // the whole resolution — another candidate may still yield the marker.
                _ => continue,
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

    /// Fetch + cache GlobalConfig (fee bps + a protocol fee recipient).
    async fn amm_config(&self) -> Result<AmmGlobalConfig> {
        if let Some(c) = *self.amm_global_config.lock().unwrap() {
            return Ok(c);
        }

        let pda = self.amm_global_config_pda;
        let account = self
            .rpc
            .get_account(&pda)
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

        let cfg = AmmGlobalConfig {
            lp_fee_bps,
            protocol_fee_bps,
            coin_creator_fee_bps,
            protocol_fee_recipient,
        };
        *self.amm_global_config.lock().unwrap() = Some(cfg);
        Ok(cfg)
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
        let [base, quote]: [Option<_>; 2] = accounts
            .try_into()
            .map_err(|_| anyhow!("getMultipleAccounts returned an unexpected count"))?;
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
            std::time::Duration::from_millis(crate::constants::RESERVE_CACHE_MAX_AGE_MS),
            true,
        ) {
            return Ok(r);
        }
        self.amm_reserves(pool).await
    }

    /// Warm the per-token AMM caches (pool facts + fee-share marker + global
    /// config) ahead of a trade, off the hot path. Idempotent — returns fast once
    /// cached. Best-effort: callers spawn this in the background and ignore the
    /// error (the live swap path falls back to the same cold fetch on a miss).
    /// Needs at least one prior on-chain AMM swap to exist for the pool, so the
    /// fee-share marker can be read — i.e. call it on/after the first AMM trade.
    pub async fn prewarm_amm_pool(
        &self,
        token_mint: &str,
        base_token_program_id: &str,
    ) -> Result<()> {
        self.amm_pool_info(token_mint, None, base_token_program_id)
            .await?;
        self.amm_config().await?;
        Ok(())
    }

    /// override → cache → on-chain lookup (mirrors `sell_token`).
    async fn resolve_user_base_account(
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
            &self.pump_swap_program,
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

/// Minimal JSON-RPC POST against a full RPC node. Free function (no `&self`
/// borrow) so the concurrent fee-share-marker lookups can drive it from detached
/// `JoinSet` tasks; `PumpFunTrader::rpc_json` is a thin wrapper over it.
async fn rpc_json_call(
    http: &reqwest::Client,
    rpc_url: &str,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value> {
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
    Pubkey::try_from(&data[off..end]).map_err(|_| anyhow!("bad pubkey at offset {}", off))
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
    use crate::constants::{
        COMPUTE_UNIT_LIMIT_AMM, COMPUTE_UNIT_PRICE_MICRO_LAMPORTS, TOKEN_2022_PROGRAM_ID,
        TOKEN_PROGRAM_ID, WSOL_MINT,
    };
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
        let config = Arc::new(TraderConfig {
            rpc_url: "http://localhost".into(),
            helius_sender_urls: vec!["http://localhost".into()],
            keypair: Keypair::new(),
            nonce_accounts: vec![Pubkey::new_unique().to_string()],
        });
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
            quote_mint: Pubkey::from_str(WSOL_MINT).unwrap(),
            base_token_program: Pubkey::from_str(TOKEN_2022_PROGRAM_ID).unwrap(),
            pool_base_token_account: Pubkey::new_unique(),
            pool_quote_token_account: Pubkey::new_unique(),
            coin_creator: Pubkey::new_unique(),
            is_cashback_coin: true,
            fee_share_marker: None,
        }
    }

    fn compute_budget_ixs() -> Vec<Instruction> {
        vec![
            ComputeBudgetInstruction::set_compute_unit_limit(COMPUTE_UNIT_LIMIT_AMM),
            ComputeBudgetInstruction::set_compute_unit_price(COMPUTE_UNIT_PRICE_MICRO_LAMPORTS),
        ]
    }

    fn build_buy_ixs(t: &PumpFunTrader, user: &Pubkey) -> Vec<Instruction> {
        let legacy = Pubkey::from_str(TOKEN_PROGRAM_ID).unwrap();
        let wsol = Pubkey::from_str(WSOL_MINT).unwrap();
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
            program_id: t.pump_swap_program,
            accounts: t.amm_swap_accounts(&pool, user, &cfg(), user_base, user_quote, legacy, true),
            data,
        });
        ixs.push(spl_token::instruction::close_account(&legacy, &user_quote, user, user, &[]).unwrap());
        ixs.push(system_instruction::transfer(user, &Pubkey::new_unique(), 200_000)); // jito tip
        ixs
    }

    fn build_sell_ixs(t: &PumpFunTrader, user: &Pubkey) -> Vec<Instruction> {
        let legacy = Pubkey::from_str(TOKEN_PROGRAM_ID).unwrap();
        let wsol = Pubkey::from_str(WSOL_MINT).unwrap();
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
            program_id: t.pump_swap_program,
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
