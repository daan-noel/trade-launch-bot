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
//! A backtest has no such ordering for free, so it does not read the stored roster at
//! all: [`walk_forward_timeline`] rebuilds one snapshot per day over the run window,
//! each counting only launches before its own anchor, and the engine picks the snapshot
//! in force at each token's birth. See
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

/// One matched launch: when it happened and who bought into its creation slot.
///
/// The unit both consumers fold: [`refresh_roster`] counts recurrence over all of
/// them, [`walk_forward_timeline`] counts it over a sliding window.
#[derive(Debug, Clone)]
pub struct Launch {
    pub created_at: chrono::DateTime<Utc>,
    /// Distinct wallets that bought in the creation slot (a wallet buying twice
    /// appears once).
    pub buyers: Vec<String>,
}

/// Every launch of `engine_fp` in `[since, until)`, with its creation-slot buyers.
///
/// The ONE place launch history is read. `until = None` means "up to now".
pub async fn launch_history(
    pool: &Pool<Postgres>,
    token_repo: &TokenRepo,
    engine_fp: &hunter_engine::fingerprint::Fingerprint,
    since: chrono::DateTime<Utc>,
    until: Option<chrono::DateTime<Utc>>,
) -> Result<Vec<Launch>> {
    // Phase 1 - cheap `Instant`-phase match over the token table. First-slot axes are
    // not readable from `tokens` alone, so they are resolved in phase 2 rather than
    // guessed here; `Instant` is a superset of `Full`, so nothing is missed.
    let candidates = collect_matching_tokens(token_repo, Some(since), until, |t| {
        matches_phase(engine_fp, &observed_axes(t, None, None), MatchPhase::Instant)
    })
    .await?;
    if candidates.is_empty() {
        return Ok(Vec::new());
    }

    let mints: Vec<String> = candidates.iter().map(|t| t.mint_address.clone()).collect();

    // Phase 2 - one aggregate over the creation slot of every candidate: per-mint
    // buy/sell totals (to settle the `Full` match) and per-wallet buy totals (the
    // roster itself). The creation SLOT is the exact bundle; the engine's one-second
    // window is its online approximation (r = 0.999 on the cohort this was derived on).
    let rows = sqlx::query(
        r#"
        -- `SUM(bigint)` is NUMERIC in Postgres, which does not decode as `i64` - the
        -- cast is what keeps this readable as one. Safe: these are per-(mint, wallet)
        -- creation-slot totals, orders of magnitude below the i64 ceiling.
        SELECT tr.mint_address,
               w.address                                                              AS wallet,
               SUM(tr.amount_lamports) FILTER (WHERE tr.trade_type = 'buy')::bigint   AS buy_lamports,
               SUM(tr.amount_lamports) FILTER (WHERE tr.trade_type = 'sell')::bigint  AS sell_lamports
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

    // Phase 3 - settle the `Full` match now that the first-slot axes are known.
    let mut launches = Vec::with_capacity(candidates.len());
    for token in &candidates {
        let mint = token.mint_address.as_str();
        let buy_sol = lamports_to_sol(slot_buy.get(mint).copied().unwrap_or(0));
        let sell_sol = lamports_to_sol(slot_sell.get(mint).copied().unwrap_or(0));
        let axes = observed_axes(token, Some(buy_sol), Some(sell_sol));
        if !matches_phase(engine_fp, &axes, MatchPhase::Full) {
            continue;
        }
        launches.push(Launch {
            created_at: token.created_at,
            buyers: buyers
                .get(mint)
                .into_iter()
                .flatten()
                .map(|w| (*w).to_string())
                .collect(),
        });
    }
    launches.sort_by_key(|l| l.created_at);
    Ok(launches)
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

    let history = launch_history(pool, token_repo, &engine_fp, since, None).await?;
    if history.is_empty() {
        warn!("veteran roster: fingerprint {fingerprint_id} matched no launches since {since}");
        return persist(fp_repo, &mut fp, &[], min_launches)
            .await
            .map(|_| RosterStats { launches: 0, wallets: 0, veterans: 0 });
    }

    let counts = tally(&history);
    let veterans = qualifying(&counts, min_launches);
    let stats = RosterStats {
        launches: history.len(),
        wallets: counts.len(),
        veterans: veterans.len(),
    };
    let refs: Vec<&str> = veterans.iter().map(String::as_str).collect();
    persist(fp_repo, &mut fp, &refs, min_launches).await?;
    info!(
        "veteran roster {fingerprint_id}: {} launches, {} wallets, {} veterans (>= {min_launches})",
        stats.launches, stats.wallets, stats.veterans
    );
    Ok(stats)
}

/// How wide a step the walk-forward timeline re-anchors on.
///
/// One day. The roster is a recurrence count over a 30-day lookback, so it moves by at
/// most a few wallets a day; a finer step would multiply the snapshot list without
/// changing an answer, and a coarser one lets the last tokens of a step read a roster
/// already stale by more than a day.
pub const TIMELINE_STEP_DAYS: i64 = 1;

/// A fingerprint's roster **as a function of time** over `[from, to]`, in the shape
/// `hunter_engine::metrics::bundle::RosterTimeline` parses.
///
/// This is what makes an `m_bundle` backtest honest. Live is causal for free (the
/// refresh always runs before the tokens that read it); a backtest is not, because the
/// stored roster was built from launches that lie in the future of most of the corpus.
/// Each snapshot here counts only launches strictly BEFORE its own anchor.
pub async fn walk_forward_timeline(
    pool: &Pool<Postgres>,
    token_repo: &TokenRepo,
    engine_fp: &hunter_engine::fingerprint::Fingerprint,
    min_launches: u32,
    from: chrono::DateTime<Utc>,
    to: chrono::DateTime<Utc>,
    lookback_days: i64,
) -> Result<Value> {
    let history = launch_history(
        pool,
        token_repo,
        engine_fp,
        from - Duration::days(lookback_days),
        Some(to),
    )
    .await?;
    Ok(timeline_from_launches(&history, min_launches, lookback_days, from, to))
}

/// The pure half of [`walk_forward_timeline`] - one snapshot every
/// [`TIMELINE_STEP_DAYS`] from `from` to `to`, each counting only the launches in the
/// `lookback_days` window that ENDS at its own anchor.
pub fn timeline_from_launches(
    history: &[Launch],
    min_launches: u32,
    lookback_days: i64,
    from: chrono::DateTime<Utc>,
    to: chrono::DateTime<Utc>,
) -> Value {
    let step = Duration::days(TIMELINE_STEP_DAYS.max(1));
    let lookback = Duration::days(lookback_days);
    let mut out = Vec::new();
    let mut anchor = from;
    loop {
        let window_start = anchor - lookback;
        // `< anchor` is the causality contract: a snapshot never counts a launch at or
        // after the instant it takes effect.
        let window: Vec<Launch> = history
            .iter()
            .filter(|l| l.created_at >= window_start && l.created_at < anchor)
            .cloned()
            .collect();
        out.push(json!({
            "from": anchor.to_rfc3339(),
            "wallets": qualifying(&tally(&window), min_launches),
        }));
        if anchor >= to {
            break;
        }
        anchor = (anchor + step).min(to);
    }
    Value::Array(out)
}

/// Install a walk-forward timeline on an **in-memory** `metric_config`, replacing any
/// flat roster so the two can never disagree. Never persisted - the stored row keeps
/// the flat `veteran_wallets` live reads.
pub fn install_timeline(metric_config: &mut Value, min_launches: u32, timeline: Value) {
    if !metric_config.is_object() {
        *metric_config = json!({});
    }
    let cfg = metric_config.as_object_mut().expect("object");
    cfg.insert(
        "m_bundle".into(),
        json!({
            "veteran_min_launches": min_launches,
            "veteran_timeline": timeline,
        }),
    );
}

/// Launches per wallet. A wallet buying twice in one launch still counts once - the
/// roster measures how many LAUNCHES a wallet shows up for.
fn tally(history: &[Launch]) -> HashMap<&str, u32> {
    let mut counts: HashMap<&str, u32> = HashMap::new();
    for launch in history {
        for w in &launch.buyers {
            *counts.entry(w.as_str()).or_default() += 1;
        }
    }
    counts
}

/// Wallets at or above the bar, sorted so an unchanged roster serializes
/// byte-identically and the UPDATE is a no-op diff rather than a spurious
/// `updated_at` bump every cycle.
fn qualifying(counts: &HashMap<&str, u32>, min_launches: u32) -> Vec<String> {
    let mut out: Vec<String> = counts
        .iter()
        .filter(|(_, &n)| n >= min_launches)
        .map(|(w, _)| (*w).to_string())
        .collect();
    out.sort_unstable();
    out
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
pub fn configured_min_launches(metric_config: &Value) -> u32 {
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

    fn ts(day: u32, hour: u32) -> chrono::DateTime<Utc> {
        format!("2026-08-{day:02}T{hour:02}:00:00Z").parse().unwrap()
    }

    fn launch(day: u32, hour: u32, buyers: &[&str]) -> Launch {
        Launch {
            created_at: ts(day, hour),
            buyers: buyers.iter().map(|w| (*w).to_string()).collect(),
        }
    }

    /// The whole point of the timeline: a snapshot counts only launches BEFORE its
    /// own anchor, so no token is ever scored against its own future.
    #[test]
    fn a_snapshot_never_counts_a_launch_at_or_after_its_own_anchor() {
        // "A" only qualifies once the 03rd's launches are in the past.
        let history = vec![
            launch(1, 0, &["A"]),
            launch(2, 0, &["A"]),
            launch(3, 0, &["A", "B"]),
        ];
        let tl = timeline_from_launches(&history, 3, 30, ts(1, 0), ts(4, 0));
        let snaps = tl.as_array().expect("array");
        let wallets = |i: usize| snaps[i]["wallets"].as_array().unwrap().len();
        assert_eq!(snaps.len(), 4); // 01, 02, 03, 04
        assert_eq!(wallets(0), 0); // nothing has happened yet
        assert_eq!(wallets(1), 0); // one launch behind us
        assert_eq!(wallets(2), 0); // two - the 03rd is not yet countable
        assert_eq!(wallets(3), 1); // three, so "A" clears the bar of 3
    }

    /// The lookback is a *rolling* window, so a wallet that stops showing up ages out
    /// instead of counting as a veteran forever.
    #[test]
    fn the_lookback_window_slides_with_the_anchor() {
        let history = vec![launch(1, 0, &["A"]), launch(1, 1, &["A"])];
        let two_day = |anchor_day| {
            let tl = timeline_from_launches(&history, 2, 2, ts(anchor_day, 0), ts(anchor_day, 0));
            tl[0]["wallets"].as_array().unwrap().len()
        };
        assert_eq!(two_day(2), 1); // both launches inside a 2-day lookback
        assert_eq!(two_day(4), 0); // both aged out
    }

    #[test]
    fn the_last_snapshot_always_covers_the_end_of_the_window() {
        let history = vec![launch(1, 0, &["A"])];
        // `to` is not on a step boundary - the loop must still emit a snapshot at it,
        // or the tokens in the final partial day would read no roster at all.
        let tl = timeline_from_launches(&history, 1, 30, ts(1, 0), ts(3, 12));
        let snaps = tl.as_array().expect("array");
        let last: chrono::DateTime<Utc> =
            snaps.last().unwrap()["from"].as_str().unwrap().parse().unwrap();
        assert_eq!(last, ts(3, 12));
    }

    #[test]
    fn install_timeline_replaces_the_flat_roster_and_keeps_other_groups() {
        let mut cfg = json!({
            "m_flow_split": { "volume_ix_patterns": ["x"] },
            "m_bundle": { "veteran_wallets": ["stale"] }
        });
        install_timeline(&mut cfg, 25, json!([{ "from": "2026-08-01T00:00:00Z", "wallets": [] }]));
        assert!(cfg["m_bundle"].get("veteran_wallets").is_none());
        assert_eq!(cfg["m_bundle"]["veteran_min_launches"], 25);
        assert_eq!(cfg["m_flow_split"]["volume_ix_patterns"][0], "x");
    }
}
