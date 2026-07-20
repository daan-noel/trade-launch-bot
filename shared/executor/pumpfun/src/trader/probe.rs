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
//
// Curve buy/sell simulation lives in `sim.rs` as the reusable engine
// (`simulate_curve_buy` / `simulate_curve_sell`); the `simulate-buy` /
// `simulate-sell` probe subcommands call it directly.
// ============================================================

use super::PumpFunTrader;
use crate::error::{Context, Result};
use executor_core::jito_tip::refresh_tip_floor;
use solana_sdk::system_instruction;
use std::time::Instant;

/// One sender endpoint's outcome in a fan-out probe.
pub struct EndpointResult {
    pub url: String,
    pub elapsed_ms: u128,
    /// `Ok(signature)` if the endpoint accepted the tx, `Err(message)` otherwise.
    /// (Fully-qualified `std::result::Result` — the module's `Result` alias is the
    /// crate's one-arg `Result<T, TradeError>`.)
    pub outcome: std::result::Result<String, String>,
}

/// Result of a fan-out self-transfer probe.
pub struct FanoutReport {
    pub results: Vec<EndpointResult>,
    /// Confirmation latency (ms) if `do_confirm` and at least one send succeeded.
    pub confirm_ms: Option<u128>,
    /// Confirmation outcome (`Ok(())` = confirmed) if attempted.
    pub confirmed: Option<std::result::Result<(), String>>,
}

/// Ranked pin recommendation from a [`FanoutReport`]: successful endpoints
/// sorted fastest-first, truncated to `keep` (for `HELIUS_FAST_SENDER_URLS`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SenderPinRecommendation {
    /// Fastest-first URLs that accepted the probe tx (≤ `keep`).
    pub urls: Vec<String>,
    /// Accept latency (ms) parallel to `urls`.
    pub elapsed_ms: Vec<u128>,
}

impl SenderPinRecommendation {
    /// Comma-joined value for `HELIUS_FAST_SENDER_URLS=…`.
    pub fn urls_csv(&self) -> String {
        self.urls.join(",")
    }

    /// Singular primary (fastest) for `HELIUS_FAST_SENDER_URL=…`.
    pub fn primary_url(&self) -> Option<&str> {
        self.urls.first().map(String::as_str)
    }
}

impl FanoutReport {
    /// Rank endpoints that **accepted** the send by `elapsed_ms` ascending and
    /// keep at most `keep` (minimum 1). Failures are omitted — a slow accept
    /// still ranks above a reject. Empty when nothing accepted.
    pub fn pin_recommendation(&self, keep: usize) -> Option<SenderPinRecommendation> {
        pin_recommendation_from_results(&self.results, keep)
    }
}

/// Pure ranking helper (unit-tested) — see [`FanoutReport::pin_recommendation`].
pub fn pin_recommendation_from_results(
    results: &[EndpointResult],
    keep: usize,
) -> Option<SenderPinRecommendation> {
    let keep = keep.max(1);
    let mut ok: Vec<(&str, u128)> = results
        .iter()
        .filter(|r| r.outcome.is_ok())
        .map(|r| (r.url.as_str(), r.elapsed_ms))
        .collect();
    if ok.is_empty() {
        return None;
    }
    ok.sort_by_key(|(_, ms)| *ms);
    ok.truncate(keep);
    Some(SenderPinRecommendation {
        urls: ok.iter().map(|(u, _)| (*u).to_string()).collect(),
        elapsed_ms: ok.iter().map(|(_, ms)| *ms).collect(),
    })
}

#[cfg(test)]
mod pin_tests {
    use super::*;

    fn ep(url: &str, ms: u128, ok: bool) -> EndpointResult {
        EndpointResult {
            url: url.to_string(),
            elapsed_ms: ms,
            outcome: if ok {
                Ok("sig".into())
            } else {
                Err("no".into())
            },
        }
    }

    #[test]
    fn ranks_fastest_first_and_drops_failures() {
        let results = vec![
            ep("http://slow", 80, true),
            ep("http://fail", 5, false),
            ep("http://fast", 20, true),
            ep("http://mid", 40, true),
        ];
        let pin = pin_recommendation_from_results(&results, 2).unwrap();
        assert_eq!(pin.urls, vec!["http://fast", "http://mid"]);
        assert_eq!(pin.elapsed_ms, vec![20, 40]);
        assert_eq!(pin.primary_url(), Some("http://fast"));
        assert_eq!(pin.urls_csv(), "http://fast,http://mid");
    }

    #[test]
    fn empty_when_nothing_accepted() {
        let results = vec![ep("http://a", 10, false)];
        assert!(pin_recommendation_from_results(&results, 2).is_none());
    }
}

impl PumpFunTrader {
    /// Refresh the *live* Jito tip-floor feed and return `(level, lamports)` for
    /// each retry level the escalation ladder would bid right now. Read-only —
    /// zero SOL, no transaction.
    pub async fn probe_tip_ladder(&self, levels: u8) -> Result<Vec<(u8, u64)>> {
        refresh_tip_floor(&self.http, &self.engine.jito_tip_cache)
            .await
            .context("refresh live Jito tip-floor")?;
        Ok((0..levels.max(1))
            .map(|l| (l, self.engine.jito_tip_cache.tip_lamports_for_level(l)))
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
        let signer = self.config.signer.as_ref();
        let me = signer.pubkey();
        let mut ixs = vec![system_instruction::transfer(&me, &me, lamports)];
        if include_tip {
            ixs.push(self.jito_tip_ix(0));
        }

        let (nonce_pubkey, nonce_hash) = self.acquire_nonce().await?;
        let tx = self.build_nonce_tx(ixs, &nonce_pubkey, nonce_hash, signer)?;
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
                let outcome = executor_core::send::post_tx(&http, &url, &body)
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
                let res = self
                    .confirm_transaction(sig, self.config.retry.confirm_max_retries)
                    .await;
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

}
