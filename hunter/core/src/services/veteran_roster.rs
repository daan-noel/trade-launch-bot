//! Veteran-roster refresher — the launch-history source behind `m_bundle`.
//!
//! The engine is a pure fold and cannot ask "has this wallet bought this launcher's
//! earlier tokens?". This service answers that question offline and parks the answer on
//! `fingerprints.metric_config.m_bundle.veteran_wallets`, which
//! `hunter_engine::metrics::bundle::veterans_from_metric_config` reads back when a token
//! track is created.
//!
//! **Causality.** A roster refreshed at time T contains only launches before T, and it
//! is read by tokens created after T — so a token is never scored against its own
//! bundle, nor against later launches. That is the whole correctness argument for the
//! metric, and it is why the roster is a stored snapshot rather than a live query.
//! Backtests must rebuild the roster per evaluation point or they leak the future; see
//! `hunter/docs/plans/strategies/veteran-wallets.md`.
//!
//! **Cost.** One keyset scan of `tokens` over the lookback plus one aggregate over
//! `trades` restricted to the matched mints — no per-token round trip, no RPC. Intended
//! to run on a slow timer (hours), because a wallet's launch count moves by one per
//! launch and the gate sits on a bimodal distribution.

use std::collections::HashMap;

use anyhow::Result;
use chrono::{Duration, Utc};
use serde_json::{json, Value};
use sqlx::{Pool, Postgres, Row};
use tracing::{info, warn};
use uuid::Uuid;

use hunter_engine::fingerprint::{matches_phase, MatchPhase};
use hunter_engine::metrics::bundle::DEFAULT_VETERAN_MIN_LAUNCHES;

use crate::models::Fingerprint as ModelFingerprint;
use crate::storage::repositories::fingerprint_repo::FingerprintRepo;
use crate::storage::repositories::token_repo::TokenRepo;
use crate::strategies::analysis::collect_matching_tokens;
use crate::strategies::fingerprint_axes::{fp_to_engine, observed_axes};

/// How far back a refresh reads launches.
///
/// The roster is a recurrence count, so it wants enough history to separate "shows up
/// every launch" from "showed up once" — but not so much that a wallet retired weeks ago
/// still counts as active.
pub const DEFAULT_LOOKBACK_DAYS: i64 = 30;

/// Outcome of one refresh, for the log line and tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RosterStats {
    /// Tokens that matched the fingerprint over the lookback.
    pub launches: usize,
    /// Distinct wallets seen in any matched launch window.
    pub wallets: usize,
    /// Wallets that cleared `min_launches`.
    pub veterans: usize,
}

/// Rebuild and persist one fingerprint's veteran roster.
///
/// `min_launches` defaults to [`DEFAULT_VETERAN_MIN_LAUNCHES`], overridable per
/// fingerprint via `metric_config.m_bundle.veteran_min_launches`.
pub async fn refresh_roster(
    pool: &Pool<Postgres>,
    token_repo: &TokenRepo,
    fp_repo: &FingerprintRepo,
    fingerprint_id: Uuid,
    lookback_days: i64,
) -> Result<RosterStats> {
    let Some(mut fp) = fp_repo.find(fingerprint_id).await? else {
        anyhow::bail!("fingerprint {fingerprint_id} not found");
    };
    let min_launches = configured_min_launches(&fp.metric_config);
    let engine_fp = fp_to_engine(&fp);
    let since = Utc::now() - Duration::days(lookback_days);

    // Phase 1 — cheap `Instant`-phase match over the token table. First-slot axes are
    // not readable from `tokens` alone, so they are resolved in phase 2 rather than
    // guessed here; `Instant` is a superset of `Full`, so nothing is missed.
    let candidates = collect_matching_tokens(token_repo, Some(since), None, |t| {
        matches_phase(&engine_fp, &observed_axes(t, None, None), MatchPhase::Instant)
    })
    .await?;
    if candidates.is_empty() {
        warn!("veteran roster: fingerprint {fingerprint_id} matched no launches since {since}");
        return persist(fp_repo, &mut fp, &[], min_launches)
            .await
            .map(|_| RosterStats { launches: 0, wallets: 0, veterans: 0 });
    }

    let mints: Vec<String> = candidates.iter().map(|t| t.mint_address.clone()).collect();

    // Phase 2 — one aggregate over the creation slot of every candidate: per-mint
    // buy/sell totals (to settle the `Full` match) and per-wallet buy totals (the
    // roster itself). The creation SLOT is the exact bundle; the engine's one-second
    // window is its online approximation (r = 0.999 on the cohort this was derived on).
    let rows = sqlx::query(
        r#"
        SELECT tr.mint_address,
               w.address                                                      AS wallet,
               SUM(tr.amount_lamports) FILTER (WHERE tr.trade_type = 'buy')   AS buy_lamports,
               SUM(tr.amount_lamports) FILTER (WHERE tr.trade_type = 'sell')  AS sell_lamports
          FROM trades tr
          JOIN tokens  tk ON tk.mint_address = tr.mint_address
          JOIN wallet_dict w ON w.id = tr.wallet_id
         WHERE tr.mint_address = ANY($1)
           AND tk.creation_slot IS NOT NULL
           AND tr.slot = tk.creation_slot
         GROUP BY 1, 2
        "#,
    )
    .bind(&mints)
    .fetch_all(pool)
    .await?;

    // Fold to per-mint slot totals and per-mint wallet lists in one pass.
    let mut slot_buy: HashMap<&str, i64> = HashMap::new();
    let mut slot_sell: HashMap<&str, i64> = HashMap::new();
    let mut buyers: HashMap<&str, Vec<&str>> = HashMap::new();
    let parsed: Vec<(String, String, i64, i64)> = rows
        .iter()
        .map(|r| {
            (
                r.get::<String, _>("mint_address"),
                r.get::<String, _>("wallet"),
                r.get::<Option<i64>, _>("buy_lamports").unwrap_or(0),
                r.get::<Option<i64>, _>("sell_lamports").unwrap_or(0),
            )
        })
        .collect();
    for (mint, wallet, buy, sell) in &parsed {
        *slot_buy.entry(mint).or_default() += buy;
        *slot_sell.entry(mint).or_default() += sell;
        if *buy > 0 {
            buyers.entry(mint).or_default().push(wallet);
        }
    }

    // Phase 3 — settle the `Full` match now that the first-slot axes are known, then
    // count each wallet's launches. A wallet buying twice in one slot still counts once:
    // the roster measures how many LAUNCHES it shows up for.
    let mut launches = 0usize;
    let mut counts: HashMap<&str, u32> = HashMap::new();
    for token in &candidates {
        let mint = token.mint_address.as_str();
        let buy_sol = lamports_to_sol(slot_buy.get(mint).copied().unwrap_or(0));
        let sell_sol = lamports_to_sol(slot_sell.get(mint).copied().unwrap_or(0));
        let axes = observed_axes(token, Some(buy_sol), Some(sell_sol));
        if !matches_phase(&engine_fp, &axes, MatchPhase::Full) {
            continue;
        }
        launches += 1;
        for w in buyers.get(mint).into_iter().flatten() {
            *counts.entry(w).or_default() += 1;
        }
    }

    let mut veterans: Vec<&str> = counts
        .iter()
        .filter(|(_, &n)| n >= min_launches)
        .map(|(w, _)| *w)
        .collect();
    // Sorted so an unchanged roster serializes byte-identically and the UPDATE is a
    // no-op diff rather than a spurious `updated_at` bump every cycle.
    veterans.sort_unstable();

    let stats = RosterStats {
        launches,
        wallets: counts.len(),
        veterans: veterans.len(),
    };
    persist(fp_repo, &mut fp, &veterans, min_launches).await?;
    info!(
        "veteran roster {fingerprint_id}: {} launches, {} wallets, {} veterans (>= {min_launches})",
        stats.launches, stats.wallets, stats.veterans
    );
    Ok(stats)
}

/// Merge the roster into the fingerprint's `metric_config`, leaving every other
/// metric group's config (notably `m_flow_split.volume_ix_patterns`) untouched.
async fn persist(
    fp_repo: &FingerprintRepo,
    fp: &mut ModelFingerprint,
    veterans: &[&str],
    min_launches: u32,
) -> Result<()> {
    let cfg = fp
        .metric_config
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("metric_config is not an object"))?;
    cfg.insert(
        "m_bundle".into(),
        json!({
            "veteran_min_launches": min_launches,
            "veteran_wallets": veterans,
        }),
    );
    fp_repo.update(fp).await
}

/// Per-fingerprint veteran bar, or [`DEFAULT_VETERAN_MIN_LAUNCHES`].
fn configured_min_launches(metric_config: &Value) -> u32 {
    metric_config
        .get("m_bundle")
        .and_then(|b| b.get("veteran_min_launches"))
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .filter(|&v| v > 0)
        .unwrap_or(DEFAULT_VETERAN_MIN_LAUNCHES)
}

fn lamports_to_sol(lamports: i64) -> f64 {
    lamports as f64 / 1_000_000_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn min_launches_falls_back_to_the_default() {
        assert_eq!(configured_min_launches(&json!({})), DEFAULT_VETERAN_MIN_LAUNCHES);
        assert_eq!(
            configured_min_launches(&json!({ "m_bundle": {} })),
            DEFAULT_VETERAN_MIN_LAUNCHES
        );
        // Zero would make every wallet a veteran — treated as unset, not obeyed.
        assert_eq!(
            configured_min_launches(&json!({ "m_bundle": { "veteran_min_launches": 0 } })),
            DEFAULT_VETERAN_MIN_LAUNCHES
        );
    }

    #[test]
    fn min_launches_reads_the_override() {
        assert_eq!(
            configured_min_launches(&json!({ "m_bundle": { "veteran_min_launches": 40 } })),
            40
        );
    }
}
