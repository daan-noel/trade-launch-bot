// ============================================================
// PumpSwap AMM (migrated tokens).
//
// Once a token graduates off the bonding curve, trading moves to the PumpSwap
// (pump_amm) program. This module implements buy/sell against that AMM, plus a
// `simulateTransaction` dry-run for validating the account layout before any
// live send.
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

use super::{AmmGlobalConfig, AmmPoolInfo, AmmSimulation, PumpFunTrader};
use crate::constants::{
    AMM_CONFIG_COIN_CREATOR_FEE_BPS_OFFSET, AMM_CONFIG_FEE_RECIPIENTS_OFFSET,
    AMM_CONFIG_LP_FEE_BPS_OFFSET, AMM_CONFIG_MIN_LEN, AMM_CONFIG_PROTOCOL_FEE_BPS_OFFSET,
    AMM_DEFAULT_SLIPPAGE_BPS, AMM_POOL_BASE_VAULT_OFFSET, AMM_POOL_COIN_CREATOR_OFFSET,
    AMM_POOL_IS_CASHBACK_OFFSET, AMM_POOL_MIN_LEN, AMM_POOL_QUOTE_VAULT_OFFSET, CONFIRM_MAX_RETRIES,
    LAMPORTS_PER_SOL, PUMP_AMM_FEE_GLOBAL, TOKEN_PROGRAM_ID,
};
use anyhow::{anyhow, bail, Context, Result};
use solana_client::rpc_config::RpcSimulateTransactionConfig;
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    message::Message,
    pubkey::Pubkey,
    signature::Signer,
    system_instruction,
    transaction::Transaction,
};
use spl_associated_token_account::{
    get_associated_token_address_with_program_id,
    instruction::create_associated_token_account_idempotent,
};
use std::str::FromStr;
use std::time::Instant;
use tracing::{info, warn};

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
    /// defaults to [`AMM_DEFAULT_SLIPPAGE_BPS`].
    pub async fn amm_buy(
        &self,
        token_mint: &str,
        base_token_program_id: &str,
        sol_amount: f64,
        pool_override: Option<&str>,
        slippage_bps: Option<u64>,
    ) -> Result<bool> {
        let t0 = Instant::now();
        let user = self.config.keypair.pubkey();
        let (nonce_pubkey, nonce_hash) = self.acquire_nonce().await?;

        let result: Result<bool> = async {
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

            let mut ixs = Vec::with_capacity(core_ixs.len() + self.compute_budget_ixs.len() + 1);
            ixs.extend_from_slice(&self.compute_budget_ixs);
            ixs.extend(core_ixs);
            if let Some(tip) = self.jito_tip_ix.lock().await.clone() {
                ixs.push(tip);
            }

            let tx = self.build_nonce_tx(ixs, &nonce_pubkey, nonce_hash, &self.config.keypair)?;
            let sig = self.send_transaction(&tx).await?;
            info!(
                "📤 AMM buy sent — sig: {} | SOL: {} | {}ms",
                sig,
                sol_amount,
                t0.elapsed().as_millis()
            );
            self.confirm_transaction(&sig, CONFIRM_MAX_RETRIES).await?;
            // Tokens land in the base ATA — cache it for a later sell.
            self.user_token_accounts
                .lock()
                .await
                .insert(token_mint.to_string(), user_base);
            info!(
                "✅ AMM buy confirmed — sig: {} | {}ms",
                sig,
                t0.elapsed().as_millis()
            );
            Ok(true)
        }
        .await;

        self.schedule_nonce_refresh(nonce_pubkey);
        result
    }

    /// Sell `token_amount` raw base-token units of a migrated token on the AMM.
    pub async fn amm_sell(
        &self,
        token_mint: &str,
        token_amount: u64,
        base_token_program_id: &str,
        pool_override: Option<&str>,
        token_account_override: Option<&str>,
        slippage_bps: Option<u64>,
    ) -> Result<bool> {
        let t0 = Instant::now();
        let user = self.config.keypair.pubkey();
        let (nonce_pubkey, nonce_hash) = self.acquire_nonce().await?;

        let result: Result<bool> = async {
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

            let mut ixs = Vec::with_capacity(core_ixs.len() + self.compute_budget_ixs.len() + 1);
            ixs.extend_from_slice(&self.compute_budget_ixs);
            ixs.extend(core_ixs);
            if let Some(tip) = self.jito_tip_ix.lock().await.clone() {
                ixs.push(tip);
            }

            let tx = self.build_nonce_tx(ixs, &nonce_pubkey, nonce_hash, &self.config.keypair)?;
            let sig = self.send_transaction(&tx).await?;
            info!(
                "📤 AMM sell sent — sig: {} | amount: {} | {}ms",
                sig,
                token_amount,
                t0.elapsed().as_millis()
            );
            self.confirm_transaction(&sig, CONFIRM_MAX_RETRIES).await?;
            info!(
                "✅ AMM sell confirmed — sig: {} | {}ms",
                sig,
                t0.elapsed().as_millis()
            );
            Ok(true)
        }
        .await;

        self.schedule_nonce_refresh(nonce_pubkey);
        result
    }

    /// Dry-run an AMM buy via `simulateTransaction` (no nonce, no send).
    /// Use this to validate account derivations before enabling live sends.
    pub async fn amm_simulate_buy(
        &self,
        token_mint: &str,
        base_token_program_id: &str,
        sol_amount: f64,
        pool_override: Option<&str>,
        slippage_bps: Option<u64>,
    ) -> Result<AmmSimulation> {
        let user = self.config.keypair.pubkey();
        let (core_ixs, _) = self
            .build_amm_buy_ixs(
                token_mint,
                base_token_program_id,
                sol_amount,
                pool_override,
                slippage_bps,
                &user,
            )
            .await?;
        self.simulate_amm(core_ixs).await
    }

    /// Dry-run an AMM sell via `simulateTransaction` (no nonce, no send).
    pub async fn amm_simulate_sell(
        &self,
        token_mint: &str,
        token_amount: u64,
        base_token_program_id: &str,
        pool_override: Option<&str>,
        token_account_override: Option<&str>,
        slippage_bps: Option<u64>,
    ) -> Result<AmmSimulation> {
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
        self.simulate_amm(core_ixs).await
    }

    // -----------------------------------------------------------------------
    // Instruction builders
    // -----------------------------------------------------------------------

    async fn build_amm_buy_ixs(
        &self,
        token_mint: &str,
        base_token_program_id: &str,
        sol_amount: f64,
        pool_override: Option<&str>,
        slippage_bps: Option<u64>,
        user: &Pubkey,
    ) -> Result<(Vec<Instruction>, Pubkey)> {
        let pool = self
            .amm_pool_info(token_mint, pool_override, base_token_program_id)
            .await?;
        let cfg = self.amm_config().await?;
        let (base_res, quote_res) = self.amm_reserves(&pool).await?;

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

        let quote_tp = Pubkey::from_str(TOKEN_PROGRAM_ID)?; // WSOL is legacy SPL
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
            accounts: self.amm_swap_accounts(&pool, user, &cfg, user_base, user_quote, quote_tp, true),
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
        let pool = self
            .amm_pool_info(token_mint, pool_override, base_token_program_id)
            .await?;
        let cfg = self.amm_config().await?;
        let (base_res, quote_res) = self.amm_reserves(&pool).await?;

        let slip = slippage_bps.unwrap_or(AMM_DEFAULT_SLIPPAGE_BPS) as u128;
        let fee_bps = (cfg.lp_fee_bps + cfg.protocol_fee_bps + cfg.coin_creator_fee_bps) as u128;

        let gross = cp_amount_out(token_amount as u128, base_res, quote_res);
        let net = gross.saturating_mul(BPS_DENOM - fee_bps) / BPS_DENOM;
        let min_quote_out = (net.saturating_mul(BPS_DENOM - slip) / BPS_DENOM) as u64;

        let quote_tp = Pubkey::from_str(TOKEN_PROGRAM_ID)?;
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
            accounts: self.amm_swap_accounts(&pool, user, &cfg, user_base, user_quote, quote_tp, false),
            data,
        });

        // Unwrap proceeds (and recover rent) back to SOL.
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
        let global_config = self.amm_global_config_pda();
        let event_authority = self.amm_event_authority();
        let cc_authority = self.amm_coin_creator_vault_authority(&pool.coin_creator);
        let cc_ata = get_associated_token_address_with_program_id(
            &cc_authority,
            &self.wsol_mint,
            &quote_token_program,
        );
        let fee_config = self.amm_fee_config();
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
            accounts.push(AccountMeta::new_readonly(self.amm_global_volume_accumulator(), false)); // 19
            accounts.push(AccountMeta::new(self.amm_user_volume_accumulator(user), false)); // 20
        }
        accounts.push(AccountMeta::new_readonly(fee_config, false)); // fee_config
        accounts.push(AccountMeta::new_readonly(self.fee_program, false)); // fee_program

        // Trailing upgrade-fee block — required by the deployed pump_amm program
        // but absent from its published IDL (verified against real on-chain
        // swaps): the pfee global, the upgrade-fee recipient, and that
        // recipient's WSOL ATA. The recipient rotates across a set on-chain; we
        // use the canonical upgrade-fee recipient, which the program accepts.
        let fee_global = Pubkey::from_str(PUMP_AMM_FEE_GLOBAL).expect("valid PUMP_AMM_FEE_GLOBAL");
        let fee_recipient = self.upgrade_fee_recipient;
        let fee_recipient_wsol = get_associated_token_address_with_program_id(
            &fee_recipient,
            &self.wsol_mint,
            &quote_token_program,
        );
        accounts.push(AccountMeta::new(fee_global, false));
        accounts.push(AccountMeta::new(fee_recipient, false));
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
        if let Some(info) = self.amm_pool_cache.lock().await.get(token_mint).copied() {
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

        let info = AmmPoolInfo {
            pool,
            base_mint: read_pubkey(data, 43)?,
            quote_mint: read_pubkey(data, 75)?,
            base_token_program: Pubkey::from_str(base_token_program_id)?,
            pool_base_token_account: read_pubkey(data, AMM_POOL_BASE_VAULT_OFFSET)?,
            pool_quote_token_account: read_pubkey(data, AMM_POOL_QUOTE_VAULT_OFFSET)?,
            coin_creator: read_pubkey(data, AMM_POOL_COIN_CREATOR_OFFSET)?,
            is_cashback_coin: data[AMM_POOL_IS_CASHBACK_OFFSET] != 0,
        };
        self.amm_pool_cache
            .lock()
            .await
            .insert(token_mint.to_string(), info);
        Ok(info)
    }

    /// Fetch + cache GlobalConfig (fee bps + a protocol fee recipient).
    async fn amm_config(&self) -> Result<AmmGlobalConfig> {
        if let Some(c) = *self.amm_global_config.lock().await {
            return Ok(c);
        }

        let pda = self.amm_global_config_pda();
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
        *self.amm_global_config.lock().await = Some(cfg);
        Ok(cfg)
    }

    /// Current pool reserves = the base/quote vault token balances (raw units).
    async fn amm_reserves(&self, pool: &AmmPoolInfo) -> Result<(u128, u128)> {
        let base = self
            .rpc
            .get_token_account_balance(&pool.pool_base_token_account)
            .await
            .context("read pool base reserve")?;
        let quote = self
            .rpc
            .get_token_account_balance(&pool.pool_quote_token_account)
            .await
            .context("read pool quote reserve")?;
        let base_res: u128 = base.amount.parse().unwrap_or(0);
        let quote_res: u128 = quote.amount.parse().unwrap_or(0);
        if base_res == 0 || quote_res == 0 {
            bail!("PumpSwap pool has zero reserves");
        }
        Ok((base_res, quote_res))
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
        if let Some(pk) = self.user_token_accounts.lock().await.get(token_mint).copied() {
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

    fn amm_global_config_pda(&self) -> Pubkey {
        Pubkey::find_program_address(&[b"global_config"], &self.pump_swap_program).0
    }
    fn amm_event_authority(&self) -> Pubkey {
        Pubkey::find_program_address(&[b"__event_authority"], &self.pump_swap_program).0
    }
    fn amm_global_volume_accumulator(&self) -> Pubkey {
        Pubkey::find_program_address(&[b"global_volume_accumulator"], &self.pump_swap_program).0
    }
    fn amm_user_volume_accumulator(&self, user: &Pubkey) -> Pubkey {
        Pubkey::find_program_address(
            &[b"user_volume_accumulator", user.as_ref()],
            &self.pump_swap_program,
        )
        .0
    }
    fn amm_coin_creator_vault_authority(&self, coin_creator: &Pubkey) -> Pubkey {
        Pubkey::find_program_address(
            &[b"creator_vault", coin_creator.as_ref()],
            &self.pump_swap_program,
        )
        .0
    }
    /// fee_config = PDA(["fee_config", pump_amm_program_id]) under the fee program.
    fn amm_fee_config(&self) -> Pubkey {
        Pubkey::find_program_address(
            &[b"fee_config", self.pump_swap_program.as_ref()],
            &self.fee_program,
        )
        .0
    }

    // -----------------------------------------------------------------------
    // Simulation
    // -----------------------------------------------------------------------

    async fn simulate_amm(&self, core_ixs: Vec<Instruction>) -> Result<AmmSimulation> {
        let mut ixs = Vec::with_capacity(core_ixs.len() + self.compute_budget_ixs.len());
        ixs.extend_from_slice(&self.compute_budget_ixs);
        ixs.extend(core_ixs);

        let payer = self.config.keypair.pubkey();
        let msg = Message::new(&ixs, Some(&payer));
        let tx = Transaction::new_unsigned(msg);

        let cfg = RpcSimulateTransactionConfig {
            sig_verify: false,
            replace_recent_blockhash: true,
            ..Default::default()
        };
        let res = self
            .rpc
            .simulate_transaction_with_config(&tx, cfg)
            .await
            .context("simulateTransaction failed")?;

        let sim = AmmSimulation {
            err: res.value.err.map(|e| format!("{:?}", e)),
            units_consumed: res.value.units_consumed,
            logs: res.value.logs.unwrap_or_default(),
        };
        if let Some(err) = &sim.err {
            warn!("🧪 AMM simulate returned error: {}", err);
        } else {
            info!(
                "🧪 AMM simulate OK — {} CU",
                sim.units_consumed.unwrap_or(0)
            );
        }
        Ok(sim)
    }
}

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

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
