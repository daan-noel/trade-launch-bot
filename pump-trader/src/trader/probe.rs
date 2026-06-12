// ============================================================
// Probes — zero-/low-SOL validation of the latency changes.
//
// Each probe drives the *real* code paths (the live Jito tip-floor cache, the
// fan-out send body, the curve-sell ix builder, simulateTransaction) so what's
// measured is what production runs — not a copy that can drift.
//
//   probe_tip_ladder            — read-only; refresh the live tip-floor feed and
//                                 report the tip each retry level would bid.
//                                 Zero SOL, no tx.
//   probe_fanout_self_transfer  — fan an idempotent self-transfer out to every
//                                 configured sender endpoint and report each
//                                 one's latency + acceptance. Costs only base fee
//                                 on the single landed tx (+ tip if requested);
//                                 the lamports return to the same wallet.
//   probe_simulate_curve_sell   — build the real curve-sell tx and simulate it
//                                 (accounts / data / CU). Zero SOL.
// ============================================================

use super::jito_tip::refresh_tip_floor;
use super::PumpFunTrader;
use crate::constants::CONFIRM_MAX_RETRIES;
use anyhow::{Context, Result};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::{signature::Signer, system_instruction};
use std::str::FromStr;
use std::time::Instant;

/// One sender endpoint's outcome in a fan-out probe.
pub struct EndpointResult {
    pub url: String,
    pub elapsed_ms: u128,
    /// `Ok(signature)` if the endpoint accepted the tx, `Err(message)` otherwise.
    pub outcome: Result<String, String>,
}

/// Result of a fan-out self-transfer probe.
pub struct FanoutReport {
    pub results: Vec<EndpointResult>,
    /// Confirmation latency (ms) if `do_confirm` and at least one send succeeded.
    pub confirm_ms: Option<u128>,
    /// Confirmation outcome (`Ok(())` = confirmed) if attempted.
    pub confirmed: Option<Result<(), String>>,
}

/// Result of a simulate probe.
pub struct SimReport {
    /// `Some(message)` if the tx would revert in simulation, `None` if it passes.
    pub err: Option<String>,
    pub units_consumed: Option<u64>,
    pub logs: Vec<String>,
}

impl PumpFunTrader {
    /// Refresh the *live* Jito tip-floor feed and return `(level, lamports)` for
    /// each retry level the escalation ladder would bid right now. Read-only —
    /// zero SOL, no transaction.
    pub async fn probe_tip_ladder(&self, levels: u8) -> Result<Vec<(u8, u64)>> {
        refresh_tip_floor(&self.http, &self.jito_tip_cache)
            .await
            .context("refresh live Jito tip-floor")?;
        Ok((0..levels.max(1))
            .map(|l| (l, self.jito_tip_cache.tip_lamports_for_level(l)))
            .collect())
    }

    /// Fan an idempotent self-transfer (wallet→wallet) out to every configured
    /// sender endpoint concurrently and report each one's latency + acceptance.
    /// The signed tx is byte-identical across endpoints, so the bank dedups by
    /// signature: at most one lands and the Jito tip (if `include_tip`) is paid
    /// once. Cost: base fee on the single landed tx (+ tip); the transferred
    /// lamports return to the same wallet, so no trading capital is at risk.
    pub async fn probe_fanout_self_transfer(
        &self,
        lamports: u64,
        include_tip: bool,
        do_confirm: bool,
    ) -> Result<FanoutReport> {
        let keypair = &self.config.keypair;
        let me = keypair.pubkey();
        let mut ixs = vec![system_instruction::transfer(&me, &me, lamports)];
        if include_tip {
            ixs.push(self.jito_tip_ix(0));
        }

        let (nonce_pubkey, nonce_hash) = self.acquire_nonce().await?;
        let tx = self.build_nonce_tx(ixs, &nonce_pubkey, nonce_hash, keypair)?;
        let body = self.encode_send_body(&tx)?;

        // Submit to every endpoint concurrently and await *all* of them (unlike
        // the production first-wins path) so the report shows each endpoint's
        // latency and which ones accepted vs. deduped.
        let mut joins = Vec::new();
        for url in self.config.helius_sender_urls.clone() {
            let http = self.http.clone();
            let body = body.clone();
            joins.push(tokio::spawn(async move {
                let t = Instant::now();
                let outcome = super::tx::post_tx(&http, &url, &body)
                    .await
                    .map_err(|e| e.to_string());
                EndpointResult {
                    url,
                    elapsed_ms: t.elapsed().as_millis(),
                    outcome,
                }
            }));
        }

        let mut results = Vec::with_capacity(joins.len());
        for j in joins {
            results.push(j.await.context("fan-out probe task panicked")?);
        }
        self.schedule_nonce_refresh(nonce_pubkey);

        let mut confirm_ms = None;
        let mut confirmed = None;
        if do_confirm {
            if let Some(sig) = results.iter().find_map(|r| r.outcome.as_ref().ok()) {
                let t = Instant::now();
                let res = self.confirm_transaction(sig, CONFIRM_MAX_RETRIES).await;
                confirm_ms = Some(t.elapsed().as_millis());
                confirmed = Some(res.map_err(|e| e.to_string()));
            }
        }

        Ok(FanoutReport {
            results,
            confirm_ms,
            confirmed,
        })
    }

    /// Build the real curve-sell tx for `token_mint`/`token_amount` (min_out=1,
    /// tip level `tip_level`) and simulate it — zero SOL. Validates the account
    /// list, instruction data, and CU consumption. Requires the wallet to hold
    /// the token (simulating a sell of a balance you don't have reverts too).
    pub async fn probe_simulate_curve_sell(
        &self,
        token_mint: &str,
        token_amount: u64,
        is_cashback: bool,
        tip_level: u8,
    ) -> Result<SimReport> {
        if !self.token_pdas.lock().unwrap().contains_key(token_mint) {
            self.get_creator_from_mint_pda(token_mint).await?;
        }
        if self.resolve_cached_token_account(token_mint).await?.is_none() {
            anyhow::bail!("No token account cached/found for mint {token_mint}");
        }

        let mint = Pubkey::from_str(token_mint)?;
        let pdas = self
            .token_pdas
            .lock()
            .unwrap()
            .get(token_mint)
            .copied()
            .context("PDAs not cached")?;
        let user_token_account = self
            .user_token_accounts
            .lock()
            .unwrap()
            .get(token_mint)
            .copied()
            .context("token account not cached")?;

        let ixs = self.build_curve_sell_ixs(
            &mint,
            &pdas,
            &user_token_account,
            token_amount,
            1,
            is_cashback,
            tip_level,
        )?;

        // Recent-blockhash tx + node-side blockhash replacement: the sell ix set
        // is identical to the production nonce tx (only the nonce-advance prefix
        // differs, irrelevant to ix validity), and this avoids depending on live
        // nonce-account state for a pure correctness check.
        let tx = self.build_recent_tx(ixs, &self.config.keypair).await?;
        let (units_consumed, err, logs) = self.simulate_transaction(&tx).await?;
        Ok(SimReport {
            err,
            units_consumed,
            logs,
        })
    }
}
