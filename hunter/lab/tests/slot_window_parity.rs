//! Does the engine reproduce the SQL derivation, folding the real tape?
//!
//! The rule the 8dtx study derives is written in SLOTS with a lagged quiet window
//! ([`docs/plans/strategies/wallet-8dtx-derived-rule.md`]). Re-expressing it in the
//! engine is where a derivation quietly becomes a different rule: a one-second window
//! is 2.5 slots, an unlagged quiet gate reads the burst it is supposed to precede, and
//! a wallet-contaminated classifier changes which transactions count as human.
//!
//! So this fold takes the SAME trades the SQL read, through the engine's own
//! `TokenTrack`, and asserts the two fire on the same `(mint, slot)`.
//!
//! Ignored by default: it needs `DATABASE_URL` and the `w8` analysis schema.
//! `cargo test -p hunter-lab --test slot_window_parity -- --ignored --nocapture`

use std::collections::BTreeSet;

use hunter_engine::metrics::flow_ix::{marker_mask, FlowPatterns, ROUTER_MARKERS};
use hunter_engine::metrics::{
    MetricId, Side, TradeLite, WindowSpec, Windows,
};
use hunter_engine::metrics::track::TokenTrack;
use hunter_engine::fingerprint::FingerprintId;
use sqlx::postgres::PgPoolOptions;
use sqlx::Row;
use uuid::Uuid;

/// The rule, in the engine's own vocabulary.
const BURST: WindowSpec = WindowSpec { size: 1.0, lag: 0.0, unit: hunter_engine::metrics::WindowUnit::Slot };
const QUIET: WindowSpec = WindowSpec { size: 30.0, lag: 1.0, unit: hunter_engine::metrics::WindowUnit::Slot };

/// Every named retail router, as the rule spells them. The classifier masks the
/// ORGANIC side with these, so a build without one is machine flow - which is the
/// claim the cleanliness gate rests on, and the reason this test states the same set
/// twice: once as engine markers, once as the SQL `~` alternation below.
const ROUTERS: [&str; 5] =
    ["Axiom Trade", "Photon", "Bloom Router", "Trojan Trade", "Terminal"];

const BURST_MIN_SOL: f64 = 1.2;
const BURST_MAX_SOL: f64 = 10.0;
const BURST_MIN_BUYS: f64 = 2.0;
/// "No bot flow" cannot be spelled as float equality: the running sums correct at the
/// window ends, so an emptied side lands on dust rather than an exact zero. A floor
/// an order of magnitude below any real transaction says the same thing and is not a
/// float-comparison trap - which a rule authored against this metric must also respect.
const NO_BOT_FLOW_SOL: f64 = 0.01;
const QUIET_MAX_SOL: f64 = 3.0;

#[tokio::test]
#[ignore]
async fn the_engine_fires_where_the_sql_derivation_fires() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let pool = PgPoolOptions::new().max_connections(4).connect(&url).await.expect("connect");

    // A sample of mints the SQL fire set covers, plus every other mint in the same
    // slots so the fold sees the identical tape.
    let mints: Vec<String> = sqlx::query(
        "SELECT DISTINCT mint FROM w8.mx
         WHERE NOT his AND allsol > 0 AND rsol >= allsol AND s30 <= 3
         ORDER BY mint LIMIT 400",
    )
    .fetch_all(&pool)
    .await
    .expect("sample mints")
    .into_iter()
    .map(|r| r.get::<String, _>("mint"))
    .collect();
    assert!(!mints.is_empty(), "w8.mx must be populated");

    let fp = FingerprintId(Uuid::nil());
    // Purely structural, and the mask names the ORGANIC side: a buy is human only if
    // it came through a named router. That is the derivation's cleanliness term; the
    // machinery-marker inverse is a different, looser gate.
    let mask = marker_mask(&ROUTERS).unwrap();
    assert_eq!(mask, ROUTER_MARKERS, "the rule masks every router in the vocabulary");
    let patterns = FlowPatterns::organic_markers_only(mask);

    let mut engine_fires: BTreeSet<(String, i64)> = BTreeSet::new();

    for mint in &mints {
        let rows = sqlx::query(
            "SELECT t.slot, t.tx_index, t.trade_type, t.amount_lamports,
                    t.reserve_lamports, t.block_time, t.ix_labels
             FROM trades t
             WHERE t.mint_address = $1 AND t.leg_index = 0
             ORDER BY t.slot, t.tx_index",
        )
        .bind(mint)
        .fetch_all(&pool)
        .await
        .expect("trades");
        if rows.is_empty() {
            continue;
        }

        let created_at: chrono::DateTime<chrono::Utc> = rows[0].get("block_time");
        let mut track = TokenTrack::new(created_at);
        track.ensure_window(BURST);
        track.ensure_window(QUIET);
        track.ensure_flow(fp, &patterns, &[BURST]);

        for r in &rows {
            let slot: i64 = r.get("slot");
            let sol = r.get::<i64, _>("amount_lamports") as f64 / 1e9;
            let vsol = r
                .try_get::<Option<i64>, _>("reserve_lamports")
                .ok()
                .flatten()
                .map(|v| v as f64 / 1e9);
            let labels: serde_json::Value =
                r.try_get("ix_labels").unwrap_or(serde_json::Value::Null);
            let at: chrono::DateTime<chrono::Utc> = r.get("block_time");
            let is_buy = r.get::<String, _>("trade_type") == "buy";

            track.on_trade(TradeLite {
                side: if is_buy { Side::Buy } else { Side::Sell },
                sol,
                price: 1.0,
                // `liquidity` means REAL deposited SOL; the pair carries the priced one.
                reserve_sol: vsol.map(|v| (v - 30.0).max(0.0)).unwrap_or(f64::NAN),
                priced_reserve_sol: vsol.unwrap_or(f64::NAN),
                at,
                ix_hash: hunter_engine::metrics::flow_ix::ix_hash_from_labels_value(&labels),
                wallet_hash: 0,
                slot: slot as u64,
                marker_bits: hunter_engine::metrics::flow_ix::marker_bits_from_labels_value(
                    &labels,
                ),
            });

            // Evaluate the entry gate exactly where the fold would: after each trade.
            let burst_w: Windows = Windows::one(BURST);
            let quiet_w: Windows = Windows::one(QUIET);
            let vol = track.value(MetricId::WinTaggedBuy, burst_w, Some(fp), at);
            let nonvol = track.value(MetricId::WinUntaggedBuy, burst_w, Some(fp), at);
            let buys = track.value(MetricId::BuyCount, burst_w, None, at);
            let quiet = track.value(MetricId::Buy, quiet_w, None, at);
            let fires = vol <= NO_BOT_FLOW_SOL
                && nonvol >= BURST_MIN_SOL
                && nonvol <= BURST_MAX_SOL
                && buys >= BURST_MIN_BUYS
                && quiet <= QUIET_MAX_SOL;
            if fires {
                engine_fires.insert((mint.clone(), slot));
            }
            if std::env::var("PARITY_DEBUG").as_deref() == Ok(mint.as_str()) {
                println!(
                    "  slot {slot} tx {} sol {sol:.4} buys {buys} nonvol {nonvol:.4} vol {vol:.4} quiet {quiet:.4}",
                    r.get::<i32, _>("tx_index")
                );
            }
        }
    }

    // The SQL reference, rebuilt from the RAW tape with the engine's exact terms.
    //
    // `w8.mx` cannot be the reference: it carries base-gate terms this rule does not
    // have (a distinct-group floor, a candidate-level liquidity cap) and a slot-entry
    // liquidity reading, so a mismatch against it would measure the derivation's
    // scaffolding rather than the window machinery under test. Same trades, same
    // running accumulation, same lagged quiet span - the only thing that differs is
    // WHO computes it.
    let sql_fires: BTreeSet<(String, i64)> = sqlx::query(
        "WITH b AS (
             SELECT t.mint_address AS mint, t.slot, t.tx_index,
                    t.amount_lamports / 1e9 AS sol,
                    coalesce(t.ix_labels::text ~ $2, false) AS rtr,
                    (t.trade_type = 'buy') AS isbuy
             FROM trades t
             WHERE t.mint_address = ANY($1) AND t.leg_index = 0
         ),
         slotagg AS (
             SELECT mint, slot,
                    coalesce(sum(sol) FILTER (WHERE isbuy), 0) AS bs
             FROM b GROUP BY 1, 2
         ),
         quiet AS (
             SELECT mint, slot,
                    coalesce(sum(bs) OVER (PARTITION BY mint ORDER BY slot
                        RANGE BETWEEN 30 PRECEDING AND 1 PRECEDING), 0) AS q
             FROM slotagg
         ),
         run AS (
             SELECT mint, slot,
                    count(*) FILTER (WHERE isbuy) OVER w AS nbuy,
                    sum(CASE WHEN isbuy AND rtr THEN sol ELSE 0 END) OVER w AS rsol,
                    sum(CASE WHEN isbuy AND NOT rtr THEN sol ELSE 0 END) OVER w AS xsol
             FROM b
             WINDOW w AS (PARTITION BY mint, slot ORDER BY tx_index ROWS UNBOUNDED PRECEDING)
         )
         SELECT DISTINCT r.mint, r.slot
         FROM run r JOIN quiet q ON q.mint = r.mint AND q.slot = r.slot
         WHERE r.nbuy >= 2 AND r.xsol <= 0.01
           AND r.rsol >= 1.2 AND r.rsol <= 10 AND q.q <= 3",
    )
    .bind(&mints)
    .bind(format!("({})", ROUTERS.join("|")))
    .fetch_all(&pool)
    .await
    .expect("sql fires")
    .into_iter()
    .map(|r| (r.get::<String, _>("mint"), r.get::<i64, _>("slot")))
    .collect();

    let both = engine_fires.intersection(&sql_fires).count();
    let only_engine = engine_fires.difference(&sql_fires).count();
    let only_sql = sql_fires.difference(&engine_fires).count();
    println!(
        "mints {} | engine {} | sql {} | both {} | engine-only {} | sql-only {}",
        mints.len(),
        engine_fires.len(),
        sql_fires.len(),
        both,
        only_engine,
        only_sql
    );
    for x in engine_fires.difference(&sql_fires).take(5) {
        println!("  engine-only {x:?}");
    }
    for x in sql_fires.difference(&engine_fires).take(5) {
        println!("  sql-only    {x:?}");
    }

    assert!(!sql_fires.is_empty(), "the sample must contain SQL fires");
    // Recall is the claim under test: every moment the derivation fires, the engine
    // fires. Extra engine fires are reported above and must be explainable.
    // Same tape, same terms, so this is an equality claim - with one bounded escape.
    //
    // The engine sums `f64`, Postgres sums `NUMERIC`. On a slot whose prior-30 buy
    // total lands EXACTLY on the threshold the two disagree in the last bit and the
    // gate flips. That is float associativity, not a window defect, and it is why the
    // registry carries an `eq_tolerance` per metric and why a threshold should not be
    // authored on a value the tape hits exactly. Bound it rather than paper over it:
    // a real divergence in the window machinery is not one row in thousands.
    assert_eq!(only_engine, 0, "the engine fires where the SQL derivation does not");
    assert!(
        only_sql * 500 <= sql_fires.len(),
        "engine misses {only_sql} of {} - beyond exact-threshold rounding",
        sql_fires.len()
    );
}
