// ============================================================
// Initialization — runs once before any buy/sell.
//
// `initialize` performs all the up-front RPC work and pre-building;
// `fetch_global_account` and `collect_nonce_pubkeys` are its private
// helpers.
// ============================================================

use super::jito_tip::refresh_tip_floor;
use super::{GlobalAccount, NonceSlot, PumpFunTrader};
use crate::constants::{
    BLOCKHASH_REFRESH_MS, BUY_SEED_POOL_SIZE, COMPUTE_UNIT_LIMIT_AMM, COMPUTE_UNIT_LIMIT_CURVE_BUY,
    COMPUTE_UNIT_LIMIT_CURVE_SELL, COMPUTE_UNIT_PRICE_MICRO_LAMPORTS, JITO_TIP_FLOOR_REFRESH_MS,
    JITO_TIP_PERCENTILE, MAX_JITO_TIP_SOL, MIN_JITO_TIP_SOL, TOKEN_2022_PROGRAM_ID,
    TOKEN_PROGRAM_ID,
};
use anyhow::{Context, Result};
use solana_sdk::{compute_budget::ComputeBudgetInstruction, pubkey::Pubkey, signature::Signer};
use std::collections::HashSet;
use std::str::FromStr;
use std::time::Duration;
use tracing::{info, warn};

impl PumpFunTrader {
    // -----------------------------------------------------------------------
    // Initialization  (call once before any buy/sell)
    // -----------------------------------------------------------------------

    pub async fn initialize(&mut self) -> Result<()> {
        info!("🔧 Initializing PumpFunTrader...");

        // 1. Global account
        self.global_account = Some(self.fetch_global_account().await?);
        info!("✅ Global account initialized");

        // 2. Rent exemption amounts (one RPC call each)
        self.token_account_rent = self
            .rpc
            .get_minimum_balance_for_rent_exemption(self.token_account_space as usize)
            .await
            .context("Failed to get rent for token account")?;
        self.token_2022_account_rent = self
            .rpc
            .get_minimum_balance_for_rent_exemption(self.token_2022_account_space as usize)
            .await
            .context("Failed to get rent for token-2022 account")?;

        // 3. Compute budget instructions — built once, cloned per tx. The CU
        // limit is sized per path (curve trades are far lighter than AMM swaps),
        // so the priority fee = price × limit isn't inflated by sizing every tx
        // for the heaviest path. Shared price ix, per-path limit ix.
        let price_ix =
            ComputeBudgetInstruction::set_compute_unit_price(COMPUTE_UNIT_PRICE_MICRO_LAMPORTS);
        self.cu_ixs_curve_buy = vec![
            ComputeBudgetInstruction::set_compute_unit_limit(COMPUTE_UNIT_LIMIT_CURVE_BUY),
            price_ix.clone(),
        ];
        self.cu_ixs_curve_sell = vec![
            ComputeBudgetInstruction::set_compute_unit_limit(COMPUTE_UNIT_LIMIT_CURVE_SELL),
            price_ix.clone(),
        ];
        self.cu_ixs_amm = vec![
            ComputeBudgetInstruction::set_compute_unit_limit(COMPUTE_UNIT_LIMIT_AMM),
            price_ix,
        ];
        info!(
            "⚡ Priority fee: {} µlamports/cu | CU limit (buy/sell/amm): {}/{}/{}",
            COMPUTE_UNIT_PRICE_MICRO_LAMPORTS,
            COMPUTE_UNIT_LIMIT_CURVE_BUY,
            COMPUTE_UNIT_LIMIT_CURVE_SELL,
            COMPUTE_UNIT_LIMIT_AMM,
        );

        // 4. Jito tip — sized per trade from Jito's live tip-floor feed (see
        // jito_tip.rs). Prime the cache once so the first trade is already warm,
        // then refresh it in the background like the blockhash cache (step 8).
        if let Err(e) = refresh_tip_floor(&self.http, &self.jito_tip_cache).await {
            warn!("Initial Jito tip-floor fetch failed (using floor): {e}");
        }
        {
            let http = self.http.clone();
            let cache = self.jito_tip_cache.clone();
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_millis(JITO_TIP_FLOOR_REFRESH_MS)).await;
                    if let Err(e) = refresh_tip_floor(&http, &cache).await {
                        warn!("Jito tip-floor refresh failed: {e}");
                    }
                }
            });
        }
        info!(
            "💸 Jito tip: dynamic — p{} of live tip-floor, clamped {}–{} SOL → {}",
            JITO_TIP_PERCENTILE, MIN_JITO_TIP_SOL, MAX_JITO_TIP_SOL, self.jito_tip_account
        );

        // 5. Parse & deduplicate nonce accounts
        self.collect_nonce_pubkeys()?;

        // 6. Pre-fetch all nonce hashes
        info!("🔧 Pre-fetching nonce hashes...");
        {
            let mut slots = self.nonce_slots.lock().await;
            for &pubkey in &self.nonce_pubkeys {
                let hash = self.fetch_nonce_hash_async(&pubkey).await?;
                slots.insert(
                    pubkey,
                    NonceSlot {
                        cached_hash: Some(hash),
                        in_use: false,
                    },
                );
            }
        }
        info!(
            "✅ Nonce hashes cached for {} account(s)",
            self.nonce_pubkeys.len()
        );

        // 7. Pre-build buy seed pools for both token programs
        info!(
            "🌱 Pre-building buy seed pools (target={})",
            BUY_SEED_POOL_SIZE
        );
        self.fill_buy_pool(TOKEN_PROGRAM_ID).await?;
        self.fill_buy_pool(TOKEN_2022_PROGRAM_ID).await?;
        info!("✅ Buy seed pools ready");

        // 8. Recent-blockhash refresher for the AMM buy path. Prime it once now so
        // the first AMM buy is already warm, then refresh in the background.
        match self.rpc.get_latest_blockhash().await {
            Ok(hash) => self.blockhash_cache.store(hash),
            Err(e) => warn!("Initial blockhash fetch failed: {e}"),
        }
        {
            let rpc = self.rpc.clone();
            let cache = self.blockhash_cache.clone();
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_millis(BLOCKHASH_REFRESH_MS)).await;
                    match rpc.get_latest_blockhash().await {
                        Ok(hash) => cache.store(hash),
                        Err(e) => warn!("Blockhash refresh failed: {e}"),
                    }
                }
            });
        }
        info!("✅ Blockhash refresher started");

        info!(
            "🚀 PumpFunTrader ready — wallet: {}",
            self.config.keypair.pubkey()
        );
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Global account
    // -----------------------------------------------------------------------

    async fn fetch_global_account(&self) -> Result<GlobalAccount> {
        let pump = self.pump_program;
        let (global_pda, _) = Pubkey::find_program_address(&[b"global"], &pump);

        // Fetch fee_recipient from chain (offset 41 in account data)
        let fee_recipient = {
            let account = self
                .rpc
                .get_account(&global_pda)
                .await
                .context("Failed to fetch pump global account")?;
            if account.data.len() < 73 {
                anyhow::bail!("Global account data too short");
            }
            Pubkey::try_from(&account.data[41..73]).context("Failed to parse fee_recipient")?
        };

        let (global_volume_accumulator, _) =
            Pubkey::find_program_address(&[b"global_volume_accumulator"], &pump);

        let wallet = self.config.keypair.pubkey();
        let (user_volume_accumulator, _) =
            Pubkey::find_program_address(&[b"user_volume_accumulator", wallet.as_ref()], &pump);

        let fee_prog = self.fee_program;
        let (fee_config, _) =
            Pubkey::find_program_address(&[b"fee_config", pump.as_ref()], &fee_prog);

        Ok(GlobalAccount {
            global_pda,
            fee_recipient,
            global_volume_accumulator,
            user_volume_accumulator,
            fee_config,
        })
    }

    // -----------------------------------------------------------------------
    // Nonce account parsing
    // -----------------------------------------------------------------------

    fn collect_nonce_pubkeys(&mut self) -> Result<()> {
        if self.config.nonce_accounts.is_empty() {
            anyhow::bail!("At least one nonce account is required");
        }
        if BUY_SEED_POOL_SIZE == 0 {
            anyhow::bail!("buy_seed_pool_size must be >= 1");
        }

        let mut seen = HashSet::new();
        self.nonce_pubkeys.clear();

        for raw in &self.config.nonce_accounts {
            if raw.is_empty() {
                anyhow::bail!("Nonce account string must not be empty");
            }
            let pk =
                Pubkey::from_str(raw).with_context(|| format!("Invalid nonce pubkey: {}", raw))?;
            if seen.insert(pk) {
                self.nonce_pubkeys.push(pk);
            }
        }

        info!(
            "✅ {} unique nonce account(s) configured",
            self.nonce_pubkeys.len()
        );
        Ok(())
    }
}
