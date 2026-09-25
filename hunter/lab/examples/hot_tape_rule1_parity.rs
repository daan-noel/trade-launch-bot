//! Does simulate book hot-tape rule 1 (or 1b) the way its Python reference booked it?
//!
//! Rule 1 was re-derived in Python with every term spelled the way the engine
//! computes it (`node-derivation/hot-tape/r1_exact.py`), and its tickets were frozen
//! (`r1_ref_{study_exact,holdout_exact}.parquet`); rule 1b's are frozen the same way by
//! `r1b_exit.py ref` (`r1b_ref_*.parquet`). This replays the same lake
//! through the SAME code simulate runs - the lab's lake load, `run_replay` over one
//! `EngineState`, the `LagMs` fill (115 ms unless `R1_LAG_MS` says otherwise), the engine cost kernel - and writes one row
//! per position with its trigger, entry-fill and exit prints named by
//! `(slot, tx_index, leg)`, so a script can compare the two books ticket by ticket.
//! Plan: `hunter/docs/roadmap/hot-tape-rule-1-engine-plan.md` (step 3).
//!
//! Two things differ from a simulate request, and neither changes a decision:
//!   * the universe is an explicit mint list (every token the reference tape holds,
//!     born inside it) instead of a fingerprint scan over Postgres;
//!   * the corpus is replayed in token batches. Tokens are independent under this
//!     rule (no caps, no copycat guard), and every batch carries a token-less anchor
//!     created at the reference tape's first print, so every batch ticks on the one
//!     grid the reference used (first event + 200 ms).
//!
//! Needs the lake (`SWEEP_LAKE_DIR`, or `hunter/lake-data`) and the inputs
//! `r1_engine_parity.py prep` writes (`r1p_rule.json` or `r1b_rule.json`, the mint,
//! creator and tick-origin files).
//!
//! A rule that reads `m_holder_book` needs the daily build-breadth table:
//! `R1_BREADTH_UNTIL=YYYY-MM-DD` loads it from Postgres (`DATABASE_URL`) for every UTC
//! day from the tick origin's to that one, through the repo fn simulate and the live
//! refresh call, and the replay folds one `BuildBreadthReloaded` per day at 00:00.
//!
//! ```text
//! R1_RULE=rule.json R1_MINTS=mints.txt R1_CREATORS=creators.csv R1_TICK0_US=... \
//! R1_OUT=out.csv [R1_CURVE_ONLY=1] [R1_BATCH=20000] [R1_BUY_SOL=0.2] [R1_LAG_MS=115] \
//! [R1_BREADTH_UNTIL=2026-09-12] cargo run -p hunter-lab --release --example hot_tape_rule1_parity
//! ```

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use hunter_engine::event::{LoadedRule, RuleId, TradeMode};
use hunter_engine::fingerprint::{Criteria, Fingerprint, FingerprintId};
use hunter_engine::grouping::TokenFingerprint;
use hunter_engine::metrics::trade_keys::wallet_hash;
use lab::lake::duck::LakeSource;
use lab::strategies::replay::{run_replay, PositionOutcome, ReplayConfig, ReplayToken};
use lab::sweep::corpus::{CorpusSource, Selection, TradeWindow};
use lab::sweep::projection::CorpusTrade;
use trading_core::models::trade::TradeRow;
use trading_core::strategies::kernel::CostModel;
use trading_core::strategies::paper_fill::FillModel;
use uuid::Uuid;

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is required"))
}

/// The build-breadth table for every UTC day from `first` to `last`, loaded (and
/// computed where missing) through the repo simulate and the live refresh use.
async fn load_breadth(
    first: chrono::NaiveDate,
    last: chrono::NaiveDate,
) -> Arc<[(chrono::NaiveDate, Arc<[hunter_engine::event::BuildBreadth]>)]> {
    use trading_core::storage::repositories::build_breadth_repo::BuildBreadthRepo;
    let pool = sqlx::PgPool::connect(&env("DATABASE_URL")).await.expect("postgres");
    let repo = BuildBreadthRepo::new(pool);
    let mut out = Vec::new();
    let mut day = first;
    while day <= last {
        let rows = repo.load_or_compute_day(day).await.expect("build breadth day");
        let table = BuildBreadthRepo::to_engine(&rows);
        let public = table.iter().filter(|b| hunter_engine::metrics::holder_book::is_public_app(b)).count();
        println!("build breadth {day}: {} recipes, {public} public", rows.len());
        out.push((day, Arc::from(table)));
        day = day.succ_opt().expect("a date has a successor");
    }
    Arc::from(out)
}

fn micros(us: i64) -> DateTime<Utc> {
    DateTime::from_timestamp_micros(us).expect("a valid instant")
}

/// Where a position's print sits on the tape: `(slot, tx_index, leg)` plus how many
/// prints share its signature, time and price (more than one = the leg is a guess).
fn locate(
    trades: &[CorpusTrade],
    tx: Option<&str>,
    at: Option<DateTime<Utc>>,
    matches_price: impl Fn(&CorpusTrade) -> bool,
) -> (i64, i64, i64, usize) {
    let (Some(tx), Some(at)) = (tx, at) else { return (-1, -1, -1, 0) };
    let hits: Vec<&CorpusTrade> = trades
        .iter()
        .filter(|t| t.tx_signature.as_deref() == Some(tx) && t.block_time == at && matches_price(t))
        .collect();
    match hits.first() {
        Some(t) => (t.slot as i64, i64::from(t.tx_index), i64::from(t.leg_index), hits.len()),
        None => (-1, -1, -1, 0),
    }
}

#[tokio::main]
async fn main() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let hunter = manifest.parent().expect("hunter/").to_path_buf();
    // The fixed per-leg cost is env-derived (tip + priority fee), exactly as simulate
    // reads it, so the SOL here is simulate's SOL.
    let _ = dotenvy::from_path(hunter.join(".env"));
    let tuning = trading_core::config::FeeTuning::from_env().expect("fee tuning");
    let costs = CostModel::pumpfun_with_impact_with(&tuning);
    println!("cost model {costs:?}");

    let rule_json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(env("R1_RULE")).expect("rule file"))
            .expect("rule json");
    let buy_sol: f64 = std::env::var("R1_BUY_SOL").ok().and_then(|v| v.parse().ok()).unwrap_or(0.2);
    let fp_id = FingerprintId(Uuid::from_u128(0xF1));
    let rule = LoadedRule {
        id: RuleId(Uuid::from_u128(0x1)),
        fingerprint_id: fp_id,
        trade_mode: TradeMode::Paper,
        buy_amount_lamports: (buy_sol * 1e9).round() as u64,
        max_concurrent_tokens: 0,
        max_total_tokens: 0,
        // The rule file may be a v1 export; the one converter reads either.
        params: hunter_engine::v1::parse_params_any(&rule_json).expect("rule 1 validates"),
        entry_enabled: true,
    };
    let fp = Fingerprint {
        id: fp_id,
        wildcard: true,
        criteria: Criteria::new(),
        tags: serde_json::json!({}),
    };

    let mints: Vec<String> = std::fs::read_to_string(env("R1_MINTS"))
        .expect("mints file")
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect();
    let creators: HashMap<String, u64> = std::fs::read_to_string(env("R1_CREATORS"))
        .expect("creators file")
        .lines()
        .skip(1)
        .filter_map(|l| l.split_once(','))
        .map(|(m, c)| (m.to_string(), wallet_hash(c.trim())))
        .collect();
    let tick0 = micros(env("R1_TICK0_US").parse().expect("R1_TICK0_US is µs"));
    let curve_only = std::env::var("R1_CURVE_ONLY").is_ok_and(|v| v == "1");
    let lag_ms: u32 = std::env::var("R1_LAG_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(115);
    let batch: usize = std::env::var("R1_BATCH").ok().and_then(|v| v.parse().ok()).unwrap_or(20_000);
    let root = std::env::var_os("SWEEP_LAKE_DIR")
        .map(|p| hunter.join(p))
        .unwrap_or_else(|| hunter.join("lake-data"));
    println!(
        "{} mints, {} creators, tick origin {tick0}, curve_only {curve_only}, lag {lag_ms} ms, lake {}",
        mints.len(),
        creators.len(),
        root.display()
    );

    let build_breadth = match std::env::var("R1_BREADTH_UNTIL") {
        Ok(until) => load_breadth(tick0.date_naive(), until.parse().expect("R1_BREADTH_UNTIL is YYYY-MM-DD")).await,
        Err(_) => Arc::from(Vec::new()),
    };

    let mut out = std::io::BufWriter::new(std::fs::File::create(env("R1_OUT")).expect("out file"));
    writeln!(
        out,
        "mint,trig_t_us,trig_slot,trig_tx,trig_leg,trig_n,fill_t_us,fill_slot,fill_tx,fill_leg,fill_n,\
         exit_t_us,exit_slot,exit_tx,exit_leg,exit_n,reason,entry_price,entry_reserve,exit_price,\
         exit_reserve,pnl_sol,pnl_pct"
    )
    .unwrap();

    let source = LakeSource::new(root);
    let mut n_pos = 0usize;
    for (bi, chunk) in mints.chunks(batch).enumerate() {
        let sel = Selection {
            mints: Some(chunk.to_vec()),
            token_cap: chunk.len(),
            created_after: None,
            created_before: None,
            per_mint_cap: i64::MAX,
            window: TradeWindow::LaunchWindow,
            curve_only,
            with_signatures: true,
            with_flow: true,
            with_flow_text: false,
            with_oracle: false,
        };
        let corpus = source.load(&sel).await.expect("lake load");
        let by_mint: HashMap<String, Arc<Vec<CorpusTrade>>> =
            corpus.tokens.iter().map(|t| (t.mint.clone(), Arc::clone(&t.trades))).collect();
        let mut tokens: Vec<ReplayToken> = corpus
            .tokens
            .into_iter()
            .map(|t| ReplayToken {
                creator_wallet_hash: creators.get(&t.mint).copied(),
                mint: t.mint,
                symbol: t.symbol,
                created_at: t.created_at,
                tf: TokenFingerprint::default(),
                trades: t.trades,
                identity: None,
                creation_slot: None,
            })
            .collect();
        let n_tok = tokens.len();
        // The grid anchor: a token with no trades, created at the reference tape's
        // first print. It arms and never enters (every entry term reads NaN on it).
        tokens.push(ReplayToken {
            mint: "~tick-origin".into(),
            symbol: String::new(),
            created_at: tick0,
            tf: TokenFingerprint::default(),
            trades: Arc::new(Vec::new()),
            creator_wallet_hash: None,
            identity: None,
            creation_slot: None,
        });
        let as_of = by_mint
            .values()
            .filter_map(|t| t.last().map(|x| x.block_time))
            .max()
            .unwrap_or(tick0)
            + chrono::Duration::hours(1);
        let outcomes: Vec<PositionOutcome> = tokio::task::spawn_blocking({
            let rule = rule.clone();
            let fp = fp.clone();
            let build_breadth = Arc::clone(&build_breadth);
            move || {
                run_replay(
                    std::slice::from_ref(&rule),
                    std::slice::from_ref(&fp),
                    tokens,
                    ReplayConfig {
                        as_of,
                        fill_model: FillModel::LagMs(lag_ms),
                        build_breadth,
                        ..Default::default()
                    },
                )
            }
        })
        .await
        .expect("replay");

        for o in &outcomes {
            let trades = &by_mint[&o.mint];
            let trig = locate(trades, o.target_tx.as_deref(), o.target_time, |t| {
                Some(t.price_per_token) == o.target_price
            });
            let fill = locate(trades, Some(&o.entry_tx), Some(o.entry_time), |t| {
                t.fill_basis() == o.entry_price
            });
            let leg = o.exit_legs.last();
            let exit = locate(trades, o.exit_tx.as_deref(), o.exit_time, |t| {
                Some(t.fill_basis()) == o.exit_price
            });
            let (pnl_sol, pnl_pct) = o.pnl_with_costs(buy_sol, &costs);
            writeln!(
                out,
                "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
                o.mint,
                o.target_time.map_or(-1, |t| t.timestamp_micros()),
                trig.0,
                trig.1,
                trig.2,
                trig.3,
                o.entry_time.timestamp_micros(),
                fill.0,
                fill.1,
                fill.2,
                fill.3,
                o.exit_time.map_or(-1, |t| t.timestamp_micros()),
                exit.0,
                exit.1,
                exit.2,
                exit.3,
                o.exit_reason.map_or("Open".to_string(), |r| r.label().into_owned()),
                o.entry_price,
                o.entry_reserve_sol.unwrap_or(f64::NAN),
                o.exit_price.unwrap_or(f64::NAN),
                leg.and_then(|l| l.reserve_sol).unwrap_or(f64::NAN),
                pnl_sol,
                pnl_pct,
            )
            .unwrap();
        }
        n_pos += outcomes.len();
        println!("batch {bi}: {n_tok} tokens, {} positions (total {n_pos})", outcomes.len());
    }
    out.flush().unwrap();
    println!("{n_pos} positions written");
}
