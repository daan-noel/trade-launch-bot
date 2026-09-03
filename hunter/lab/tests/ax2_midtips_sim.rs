//! Lake simulate of the frozen ax2-midtips leftover vs the Python book
//! (`ix7-forward.json`). Ignored by default — full-window load + fold.
//!
//! ```
//! cargo test -p hunter-lab --test ax2_midtips_sim -- --ignored --nocapture
//! ```

use std::collections::HashMap;

use chrono::{DateTime, Datelike, Utc};
use serde_json::json;
use uuid::Uuid;

use hunter_engine::event::{LoadedRule, RuleId, TradeMode};
use hunter_engine::fingerprint::{
    AxisId, AxisPredicate, Criteria, Fingerprint, FingerprintId,
};
use hunter_engine::rule_params::RuleParams;

use lab::lake::duck::LakeSource;
use lab::sweep::corpus::{CorpusSource, CorpusToken, Selection, TradeWindow};
use lab::strategies::replay::{outcome_to_row, run_replay, ReplayConfig, ReplayToken};

use trading_core::strategies::kernel::CostModelKind;
use trading_core::strategies::paper_fill::FillModel;

fn lake_root() -> String {
    let raw = std::env::var("FLOW_SCALPER_LAKE")
        .or_else(|_| std::env::var("SWEEP_LAKE_DIR"))
        .unwrap_or_else(|_| "C:/Users/User/Documents/Bot/hunter/lake-data".to_string());
    let p = std::path::Path::new(&raw);
    if p.is_absolute() {
        return raw;
    }
    let hunter = std::path::Path::new("C:/Users/User/Documents/Bot/hunter").join(p);
    if hunter.exists() {
        return hunter.to_string_lossy().replace('\\', "/");
    }
    raw
}

const BUY_SOL: f64 = 0.10;
const FP_ID: u128 = 0x00A2_01D7_0001;
/// Python book window (ix7-forward). `tip_lamports` starts 2026-08-30 17:48 UTC;
/// earlier lake days cannot score this leftover.
const SINCE: &str = "2026-08-30T00:00:00Z";
const UNTIL: &str = "2026-09-04T00:00:00Z";
const IS_END: &str = "2026-09-03T00:00:00Z";
/// First instant the tape carries `tip_lamports` (tape-epochs).
const TIP_ERA: &str = "2026-08-30T17:48:13Z";

const WORKING_PROGRAMS: &[&str] = &["Axiom Trade"];

fn door_fp() -> Fingerprint {
    Fingerprint {
        id: FingerprintId(Uuid::from_u128(FP_ID)),
        wildcard: false,
        criteria: Criteria::new()
            .with(AxisId::CreateAta, AxisPredicate::exact(1))
            .with(
                AxisId::InitBuyLamports,
                AxisPredicate::range(Some(200_000_000), None),
            )
            .with(
                AxisId::FirstSlotBuyLamports,
                AxisPredicate::range(Some(500_000_000), None),
            ),
        metric_config: json!({
            "m_burst_slot": {
                "working_templates": [],
                "working_programs": WORKING_PROGRAMS
            }
        }),
    }
}

fn ax2_params() -> serde_json::Value {
    json!({
        "exclusive": true,
        "priority": 10,
        "reentry": { "cooldown_sec": 0, "max_episodes_per_token": 1 },
        "entry_lock": "slot",
        "entry_event": {
            "m_burst_wave": {
                "this_member": [{"operator": "=", "value": 1}],
                "this_working": [{"operator": "=", "value": 1}],
                "working_buy_count": [{"operator": "=", "value": 2}],
                "gap_slots": [{"operator": ">=", "value": 2}],
                "this_tip": [
                    {"operator": ">=", "value": 100000},
                    {"operator": "<", "value": 1000000}
                ]
            }
        },
        "entry": {
            "m_burst_wave": {
                "hole": [{"operator": "=", "value": 1}],
                "tip_seen": [{"operator": "=", "value": 1}]
            }
        },
        "exit": [
            { "m_position": {
                "armed": [{"operator": "=", "value": 1}],
                "retrace": [{"operator": ">=", "value": 18}],
                "arm_above_pct": 10
            } },
            {
                "m_position": { "armed": [{"operator": "=", "value": 0}] },
                "m_flow_window": {
                    "window_size_sec": 8,
                    "buy_count": [{"operator": "=", "value": 0}]
                }
            }
        ]
    })
}

fn loaded() -> LoadedRule {
    let fp_id = FingerprintId(Uuid::from_u128(FP_ID));
    LoadedRule {
        id: RuleId(Uuid::from_u128(1)),
        fingerprint_id: fp_id,
        trade_mode: TradeMode::Paper,
        buy_amount_lamports: 100_000_000,
        max_concurrent_tokens: 0,
        max_total_tokens: 0,
        entry_enabled: true,
        params: RuleParams::parse(&ax2_params()).expect("ax2 params"),
    }
}

fn ts(s: &str) -> DateTime<Utc> {
    s.parse().expect("rfc3339")
}

fn env_ts(key: &str, default: &str) -> DateTime<Utc> {
    std::env::var(key)
        .ok()
        .map(|s| ts(&s))
        .unwrap_or_else(|| ts(default))
}

fn load_creation_slots(
    rt: &tokio::runtime::Runtime,
    since: DateTime<Utc>,
    until: DateTime<Utc>,
) -> HashMap<String, u64> {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL unset — using first-trade slot as create slot");
        return HashMap::new();
    };
    rt.block_on(async {
        let pool = sqlx::PgPool::connect(&url).await.expect("pg");
        let rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT mint_address, creation_slot
               FROM tokens
              WHERE creation_slot IS NOT NULL
                AND created_at >= $1
                AND created_at < $2",
        )
        .bind(since)
        .bind(until)
        .fetch_all(&pool)
        .await
        .expect("creation_slot");
        rows.into_iter()
            .filter_map(|(m, s)| (s > 0).then_some((m, s as u64)))
            .collect()
    })
}

struct Book {
    n: usize,
    sol: f64,
    mean: f64,
    days_pos: usize,
    days: usize,
}

fn book(rows: &[(DateTime<Utc>, f64, f64)]) -> Book {
    if rows.is_empty() {
        return Book {
            n: 0,
            sol: 0.0,
            mean: f64::NAN,
            days_pos: 0,
            days: 0,
        };
    }
    let n = rows.len();
    let sol: f64 = rows.iter().map(|(_, s, _)| s).sum();
    let mean = rows.iter().map(|(_, _, p)| p).sum::<f64>() / n as f64;
    let mut by: HashMap<(i32, u32, u32), f64> = HashMap::new();
    for (t, s, _) in rows {
        let d = t.date_naive();
        *by.entry((d.year(), d.month(), d.day())).or_default() += s;
    }
    let days = by.len();
    let days_pos = by.values().filter(|v| **v > 0.0).count();
    Book {
        n,
        sol,
        mean,
        days_pos,
        days,
    }
}

fn report(label: &str, b: &Book, py_n: usize, py_sol: f64, py_days: &str) {
    eprintln!(
        "  {label:<8} n={:>5} SOL={:>+7.2} mean={:>+6.2}% days+={}/{}   python n={} SOL={:>+.2} {}",
        b.n,
        b.sol,
        b.mean * 100.0,
        b.days_pos,
        b.days,
        py_n,
        py_sol,
        py_days
    );
}

fn report_days(rows: &[(DateTime<Utc>, f64, f64)]) {
    let mut by: HashMap<(i32, u32, u32), (usize, f64)> = HashMap::new();
    for (t, s, _) in rows {
        let d = t.date_naive();
        let e = by.entry((d.year(), d.month(), d.day())).or_default();
        e.0 += 1;
        e.1 += s;
    }
    let mut keys: Vec<_> = by.keys().copied().collect();
    keys.sort();
    eprintln!("  per-day:");
    for k in keys {
        let (n, sol) = by[&k];
        eprintln!(
            "    {:04}-{:02}-{:02}  n={:>5} SOL={:>+7.2}",
            k.0, k.1, k.2, n, sol
        );
    }
}

fn run_window(
    rt: &tokio::runtime::Runtime,
    since: DateTime<Utc>,
    until: DateTime<Utc>,
    token_cap: usize,
) -> Vec<(DateTime<Utc>, f64, f64)> {
    let sel = Selection {
        mints: None,
        token_cap,
        created_after: Some(since),
        created_before: Some(until),
        per_mint_cap: i64::MAX,
        window: TradeWindow::LaunchWindow,
        curve_only: true,
        with_signatures: false,
        with_flow: true,
        with_flow_text: false,
        with_oracle: false,
    };
    let src = LakeSource::new(lake_root());
    let fp = door_fp();
    let (mints, capped) = rt
        .block_on(src.matching_mints(&sel, fp.clone()))
        .expect("door mints");
    eprintln!(
        "door-matched mints: {} capped={}  window {} .. {}",
        mints.len(),
        capped,
        since,
        until
    );
    let mut sel = sel;
    sel.mints = Some(mints);
    let corpus = rt.block_on(src.load(&sel)).expect("lake load");
    eprintln!(
        "loaded {} tokens / {} trades from {}",
        corpus.token_count(),
        corpus.trade_count(),
        lake_root()
    );

    let slots = load_creation_slots(rt, since, until);
    eprintln!("creation_slot from PG: {}", slots.len());

    let replay_tokens: Vec<ReplayToken> = corpus
        .tokens
        .into_iter()
        .map(|t: CorpusToken| ReplayToken {
            creation_slot: slots.get(&t.mint).copied(),
            mint: t.mint,
            symbol: t.symbol,
            created_at: t.created_at,
            tf: t.fp,
            trades: t.trades,
            creator_wallet_hash: None,
            identity: t.identity,
        })
        .collect();
    eprintln!("door-matched tokens: {}", replay_tokens.len());

    let outcomes = run_replay(
        &[loaded()],
        &[fp],
        replay_tokens,
        ReplayConfig {
            as_of: Utc::now(),
            fill_model: FillModel::LagMs(95),
            ..Default::default()
        },
    );

    let mut all = Vec::new();
    for o in &outcomes {
        let row = outcome_to_row(o, &o.mint, o.entry_time, BUY_SOL, CostModelKind::PumpfunImpact);
        let Some(sol) = row.pnl_sol.map(|v| v as f64) else {
            continue;
        };
        let pct = row.pnl_percent.map(|v| v as f64 / 100.0).unwrap_or(sol / BUY_SOL);
        all.push((o.entry_time, sol, pct));
    }
    all
}

#[test]
#[ignore]
fn ax2_midtips_lake_sim_vs_python() {
    let _ = dotenvy::from_filename("C:/Users/User/Documents/Bot/hunter/.env");
    let _ = dotenvy::dotenv();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio");

    let since = env_ts("AX2_SINCE", SINCE);
    let until = env_ts("AX2_UNTIL", UNTIL);
    let all = run_window(&rt, since, until, 200_000);

    let is_end = ts(IS_END);
    let is: Vec<_> = all.iter().copied().filter(|(t, _, _)| *t < is_end).collect();
    let oos: Vec<_> = all.iter().copied().filter(|(t, _, _)| *t >= is_end).collect();

    let all_b = book(&all);
    let is_b = book(&is);
    let oos_b = book(&oos);

    eprintln!("######## engine sim vs python ix7-forward ########");
    report("all", &all_b, 802, 4.82, "days+=5/5");
    report("IS", &is_b, 735, 4.33, "days+=4/4");
    report("OOS", &oos_b, 67, 0.48, "days+=1/1");
    report_days(&all);
    eprintln!(
        "verdict engine SOL={:+.2} days+={}/{}  still_positive={}",
        all_b.sol,
        all_b.days_pos,
        all_b.days,
        all_b.sol > 0.0 && all_b.days_pos == all_b.days && all_b.days > 0
    );

    assert!(
        all_b.n > 0,
        "engine fired nothing — lake window or door match is empty"
    );
    assert!(
        all_b.sol > 0.0,
        "engine book is red: n={} SOL={:+.2}",
        all_b.n,
        all_b.sol
    );
}

/// Widest window the leftover can actually score: `tip_lamports` exists from
/// 2026-08-30 17:48, lake sealed days run through yesterday. Earlier partitions
/// have no tip and cannot fire this_tip in [1e5, 1e6).
///
/// ```
/// cargo test -p hunter-lab --test ax2_midtips_sim ax2_midtips_tip_era_stability -- --ignored --nocapture
/// ```
#[test]
#[ignore]
fn ax2_midtips_tip_era_stability() {
    let _ = dotenvy::from_filename("C:/Users/User/Documents/Bot/hunter/.env");
    let _ = dotenvy::dotenv();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio");

    let since = env_ts("AX2_SINCE", SINCE);
    let until = env_ts("AX2_UNTIL", UNTIL);
    let all = run_window(&rt, since, until, 200_000);
    let tip_era = ts(TIP_ERA);
    let scored: Vec<_> = all.iter().copied().filter(|(t, _, _)| *t >= tip_era).collect();
    let b = book(&scored);

    eprintln!("######## ax2-midtips tip-era stability ########");
    eprintln!(
        "  window {} .. {}  n={} SOL={:+.2} mean={:+.2}% days+={}/{}",
        since,
        until,
        b.n,
        b.sol,
        b.mean * 100.0,
        b.days_pos,
        b.days
    );
    report_days(&scored);
    eprintln!(
        "verdict tip-era SOL={:+.2} days+={}/{}  still_positive={}",
        b.sol,
        b.days_pos,
        b.days,
        b.sol > 0.0 && b.days > 0
    );

    assert!(b.n > 0, "tip-era fired nothing");
    assert!(b.sol > 0.0, "tip-era book is red: n={} SOL={:+.2}", b.n, b.sol);
}
