// ============================================================
// Initialization — runs once before any buy/sell.
//
// `initialize` performs all the up-front RPC work and pre-building;
// `fetch_global_account` and `collect_nonce_pubkeys` are its private
// helpers.
// ============================================================

use super::jito_tip::refresh_tip_floor;
use super::{GlobalAccount, NonceSlot, PumpFunTrader};
use crate::error::{bail, Context, Result};
use crate::protocol;
use crate::types::TokenProgram;
use solana_sdk::{compute_budget::ComputeBudgetInstruction, pubkey::Pubkey};
use std::collections::HashSet;
use std::time::Duration;
use tracing::{info, warn};

impl PumpFunTrader {
    // -----------------------------------------------------------------------
    // Initialization  (call once before any buy/sell)
    // -----------------------------------------------------------------------

    pub async fn initialize(&mut self) -> Result<()> {
        info!("🔧 Initializing PumpFunTrader...");

        // 0. Connection warmup — seed the keep-alive pool so the FIRST trade
        // doesn't pay a TLS handshake on the latency-critical send. Fire a cheap
        // `getHealth` at each Sender endpoint and the RPC concurrently and ignore
        // every failure: warmup is best-effort and must never fail `initialize`.
        self.warmup_connections().await;

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
        let compute = &self.config.compute;
        let price_ix =
            ComputeBudgetInstruction::set_compute_unit_price(compute.price_micro_lamports);
        let cu_ixs_curve_buy = vec![
            ComputeBudgetInstruction::set_compute_unit_limit(compute.curve_buy_cu),
            price_ix.clone(),
        ];
        let cu_ixs_curve_sell = vec![
            ComputeBudgetInstruction::set_compute_unit_limit(compute.curve_sell_cu),
            price_ix.clone(),
        ];
        let cu_ixs_curve_create = vec![
            ComputeBudgetInstruction::set_compute_unit_limit(compute.curve_create_cu),
            price_ix.clone(),
        ];
        let cu_ixs_amm = vec![
            ComputeBudgetInstruction::set_compute_unit_limit(compute.amm_cu),
            price_ix,
        ];
        info!(
            "⚡ Priority fee: {} µlamports/cu | CU limit (buy/sell/create/amm): {}/{}/{}/{}",
            compute.price_micro_lamports,
            compute.curve_buy_cu,
            compute.curve_sell_cu,
            compute.curve_create_cu,
            compute.amm_cu,
        );
        self.cu_ixs_curve_buy = cu_ixs_curve_buy;
        self.cu_ixs_curve_sell = cu_ixs_curve_sell;
        self.cu_ixs_curve_create = cu_ixs_curve_create;
        self.cu_ixs_amm = cu_ixs_amm;

        // 4. Jito tip — sized per trade from Jito's live tip-floor feed (see
        // jito_tip.rs). Prime the cache once so the first trade is already warm,
        // then refresh it in the background like the blockhash cache (step 8).
        if let Err(e) = refresh_tip_floor(&self.http, &self.jito_tip_cache).await {
            warn!("Initial Jito tip-floor fetch failed (using floor): {e}");
        }
        {
            let http = self.http.clone();
            let cache = self.jito_tip_cache.clone();
            let refresh_ms = self.config.jito.floor_refresh_ms;
            let handle = tokio::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_millis(refresh_ms)).await;
                    if let Err(e) = refresh_tip_floor(&http, &cache).await {
                        warn!("Jito tip-floor refresh failed: {e}");
                    }
                }
            });
            self.background_tasks.push(handle);
        }
        info!(
            "💸 Jito tip: dynamic — p{} of live tip-floor, clamped {}–{} SOL → {}",
            self.config.jito.percentile,
            self.config.jito.min_sol,
            self.config.jito.max_sol,
            self.jito_tip_account
        );

        // 5. Parse & deduplicate nonce accounts
        self.collect_nonce_pubkeys()?;

        // 6. Pre-fetch all nonce hashes
        info!("🔧 Pre-fetching nonce hashes...");
        {
            // Fetch each hash first (async), then take the sync lock only for the
            // inserts — a `std::sync::Mutex` guard must not be held across `.await`.
            let mut fetched = Vec::with_capacity(self.nonce_pubkeys.len());
            for &pubkey in &self.nonce_pubkeys {
                let hash = self.fetch_nonce_hash_async(&pubkey).await?;
                fetched.push((pubkey, hash));
            }
            let mut slots = self.nonce_slots.lock().unwrap_or_else(|p| p.into_inner());
            for (pubkey, hash) in fetched {
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
            self.config.limits.buy_seed_pool_size
        );
        self.fill_buy_pool(TokenProgram::Legacy).await?;
        self.fill_buy_pool(TokenProgram::Token2022).await?;
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
            let refresh_ms = self.config.cache.blockhash_refresh_ms;
            let handle = tokio::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_millis(refresh_ms)).await;
                    match rpc.get_latest_blockhash().await {
                        Ok(hash) => cache.store(hash),
                        Err(e) => warn!("Blockhash refresh failed: {e}"),
                    }
                }
            });
            self.background_tasks.push(handle);
        }
        info!("✅ Blockhash refresher started");

        info!(
            "🚀 PumpFunTrader ready — wallet: {}",
            self.config.signer.pubkey()
        );
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Connection warmup
    // -----------------------------------------------------------------------

    /// Best-effort TLS/keep-alive warmup: POST a cheap `getHealth` JSON-RPC to
    /// each Sender endpoint and the RPC URL concurrently, seeding the shared
    /// reqwest connection pool so the first real send reuses a warm connection
    /// instead of paying a fresh handshake. All errors are swallowed — a
    /// down/slow endpoint at startup must never block or fail `initialize`.
    async fn warmup_connections(&self) {
        // Dedup the targets (the RPC URL may also appear among the senders).
        let mut targets: Vec<&str> = Vec::with_capacity(self.config.helius_sender_urls.len() + 1);
        for url in &self.config.helius_sender_urls {
            if !targets.contains(&url.as_str()) {
                targets.push(url);
            }
        }
        if !targets.contains(&self.config.rpc_url.as_str()) {
            targets.push(&self.config.rpc_url);
        }

        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getHealth",
        });

        let mut set = tokio::task::JoinSet::new();
        for url in targets {
            let http = self.http.clone();
            let url = url.to_string();
            let body = body.clone();
            set.spawn(async move {
                // Ignore the result entirely — we only want the TCP+TLS session
                // established and pooled.
                let _ = http.post(&url).json(&body).send().await;
            });
        }
        while set.join_next().await.is_some() {}
        info!("🔥 Connection pool warmed");
    }

    // -----------------------------------------------------------------------
    // Global account
    // -----------------------------------------------------------------------

    async fn fetch_global_account(&self) -> Result<GlobalAccount> {
        let (global_pda, _) = Pubkey::find_program_address(&[b"global"], &protocol::PUMP_FUN);

        // Fetch fee_recipient (offset 41) + the stable quote mint from chain in
        // one read. Layout (after the 8-byte discriminator): the
        // `whitelisted_quote_mints[0]` pubkey sits at byte 1013 — see the `Global`
        // struct in the pump IDL. Parsed best-effort: a shorter/older account or a
        // default (all-zero) slot just yields no stable mint (claim path skipped).
        const OFF_FEE_RECIPIENT: usize = 41;
        const OFF_STABLE_QUOTE_MINT: usize = 1013;
        let (fee_recipient, stable_quote_mint) = {
            let account = self
                .rpc
                .get_account(&global_pda)
                .await
                .context("Failed to fetch pump global account")?;
            if account.data.len() < OFF_FEE_RECIPIENT + 32 {
                bail!("Global account data too short");
            }
            let fee_recipient = Pubkey::try_from(&account.data[OFF_FEE_RECIPIENT..OFF_FEE_RECIPIENT + 32])
                .context("Failed to parse fee_recipient")?;
            let stable_quote_mint = account
                .data
                .get(OFF_STABLE_QUOTE_MINT..OFF_STABLE_QUOTE_MINT + 32)
                .and_then(|s| Pubkey::try_from(s).ok())
                .filter(|pk| *pk != Pubkey::default());
            (fee_recipient, stable_quote_mint)
        };

        let (global_volume_accumulator, _) =
            Pubkey::find_program_address(&[b"global_volume_accumulator"], &protocol::PUMP_FUN);

        let wallet = self.config.signer.pubkey();
        let (user_volume_accumulator, _) =
            Pubkey::find_program_address(&[b"user_volume_accumulator", wallet.as_ref()], &protocol::PUMP_FUN);

        let (fee_config, _) =
            Pubkey::find_program_address(&[b"fee_config", protocol::PUMP_FUN.as_ref()], &protocol::FEE_PROGRAM);

        Ok(GlobalAccount {
            global_pda,
            fee_recipient,
            global_volume_accumulator,
            user_volume_accumulator,
            fee_config,
            stable_quote_mint,
        })
    }

    // -----------------------------------------------------------------------
    // Nonce account parsing
    // -----------------------------------------------------------------------

    fn collect_nonce_pubkeys(&mut self) -> Result<()> {
        if self.config.nonce_accounts.is_empty() {
            bail!("At least one nonce account is required");
        }
        if self.config.limits.buy_seed_pool_size == 0 {
            bail!("buy_seed_pool_size must be >= 1");
        }

        // Nonce accounts arrive already parsed (`Vec<Pubkey>`); just deduplicate.
        let mut seen = HashSet::new();
        self.nonce_pubkeys.clear();
        for &pk in &self.config.nonce_accounts {
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
