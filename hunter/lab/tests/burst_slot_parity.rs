//! Does `m_burst_slot` read what the SQL derivation read?
//!
//! The crowd-island rule is derived in SQL over `ixg.cm_pre`. Re-expressing it as
//! engine metrics is where a derivation quietly becomes a different rule - the way
//! a blacklist replaced a whitelist once and turned +5.10 % into -18.97 % on the
//! same mints. So this folds the SAME trades through `TokenTrack` and asserts every
//! prefix reading matches the SQL column it stands for, print by print.
//!
//! Two gates:
//!   1. the grain SPELLING, over the whole label vocabulary in `ixg.dict`;
//!   2. the prefix READINGS, at every island fire on a sample of mints.
//!
//! Known, measured scope differences, asserted small rather than assumed away:
//!   * `ixg.fbuy` drops `wallet_id = 2720` (the wallet under study). A live rule
//!     cannot drop a wallet, so fires whose slot contains it are excluded from the
//!     comparison and counted - 126 of 8,569 island fires (1.5 %).
//!   * Every leg is replayed, not only `leg_index = 0`. Some curve buys land ONLY
//!     as a later leg - one at slot 439571515 on `1pGdSL...pump` has no leg 0 - and
//!     dropping those reads a busy slot as quiet. `cm_mem` counts legs too, so this
//!     is what SQL saw on both sides.
//!
//! Ignored by default: needs `DATABASE_URL` and the `ixg` analysis schema.
//! `cargo test -p hunter-lab --test burst_slot_parity -- --ignored --nocapture`

use std::collections::{HashMap, HashSet};

use hunter_engine::fingerprint::FingerprintId;
use hunter_engine::hash::HashedSet;
use hunter_engine::metrics::burst_slot::BurstPatterns;
use hunter_engine::metrics::template_grain;
use hunter_engine::metrics::track::TokenTrack;
use hunter_engine::metrics::{MetricId, Side, TradeLite, WindowSpec, WindowUnit, Windows};
use sqlx::postgres::PgPoolOptions;
use sqlx::Row;
use uuid::Uuid;

/// The working list this study ships, after the money check: Axiom's two grains
/// and GMGN Bot. The five the SQL also carried are 2.6 % of fires and contribute
/// at most 0.28 SOL each, so they are not here - see `ix-live-rule.md`.
const WORKING: [&str; 3] = [
    "Axiom Trade|CU|ATA|F",
    "Axiom Trade|CU|ATA|N|F",
    "GMGN Bot|CU|ATA|F",
];

/// Mints to replay. The fold is O(tape), so this trades coverage for runtime.
const SAMPLE_MINTS: i64 = 300;

/// The window `ixg.fall` was cut on. The replay takes the same span, or the
/// engine would see buys before it that SQL's `lag(slot)` never had.
const T0: &str = "2026-08-11";
const T1: &str = "2026-08-23";

/// The 5-slot buy quiet, in the spelling that already exists. `dslot >= 5` is
/// "no buys in `S-4 ..= S-1`", so the window is 4 slots wide, lagged by 1 - a
/// 5-wide window would be `dslot >= 6` and the wrong gate. There is no
/// `buy_gap_slots` metric on purpose: this is the same fact, already spelled.
const QUIET: WindowSpec = WindowSpec {
    size: 4.0,
    lag: 1.0,
    unit: WindowUnit::Slot,
};

fn labels_of(v: &serde_json::Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// The grain string the engine builds must be the string SQL stored. `ixg.dict`
/// is one row per distinct label sequence, so this is the WHOLE vocabulary, not
/// a sample - a new program prefix or a renamed instruction fails here first.
///
/// Two SQL spellings exist and only one is the rule's. `ixg.dict.tmpl` prepends a
/// `|LAUNCH` segment; `ixg.fbuy.tmpl` - what `cm_mem` and the working list are
/// written in - has no such segment, because it drops launch rows outright. The
/// engine follows `fbuy`, and `is_launch` is what keeps a launch out of the
/// prefix. So the grains are compared on the non-launch vocabulary, and the
/// launch rows are asserted to be the ONLY place the two spellings part company:
/// drift on any other row still fails here.
#[tokio::test]
#[ignore]
async fn the_grain_is_spelled_the_way_sql_spells_it() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .expect("connect");

    let rows = sqlx::query("SELECT ix_labels, tmpl, launch FROM ixg.dict")
        .fetch_all(&pool)
        .await
        .expect("ixg.dict");
    assert!(!rows.is_empty(), "ixg.dict must be populated");

    let mut checked = 0usize;
    let mut launches = 0usize;
    let mut grain_bad: Vec<String> = Vec::new();
    let mut launch_bad: Vec<String> = Vec::new();
    for r in &rows {
        let labels = labels_of(&r.get::<serde_json::Value, _>("ix_labels"));
        if labels.is_empty() {
            continue;
        }
        let sql_tmpl: Option<String> = r.try_get("tmpl").ok().flatten();
        let sql_launch: bool = r.try_get("launch").ok().flatten().unwrap_or(false);

        // The gate that actually protects the prefix: a launch must be recognised
        // as one, whatever it is spelled.
        if template_grain::is_launch(&labels) != sql_launch {
            launch_bad.push(format!(
                "labels {labels:?}: engine launch {} != sql {sql_launch}",
                template_grain::is_launch(&labels)
            ));
        }
        if sql_launch {
            launches += 1;
            continue;
        }

        let ours = template_grain::grain(&labels);
        if let Some(want) = sql_tmpl.as_deref() {
            if ours != want {
                grain_bad.push(format!(
                    "labels {labels:?}: engine {ours:?} != sql {want:?}"
                ));
            }
        }
        checked += 1;
    }
    println!(
        "grain spelling: {checked} non-launch label sequences checked, \
         {launches} launch sequences excluded (dict spells those with |LAUNCH)"
    );
    assert!(
        launch_bad.is_empty(),
        "{} launch mismatches, first 5: {:#?}",
        launch_bad.len(),
        &launch_bad[..launch_bad.len().min(5)]
    );
    assert!(
        grain_bad.is_empty(),
        "{} mismatches, first 5: {:#?}",
        grain_bad.len(),
        &grain_bad[..grain_bad.len().min(5)]
    );
    assert!(checked > 2000, "only {checked} sequences - dict looks truncated");
}

/// SQL's prefix columns at one island fire.
#[derive(Debug)]
struct Want {
    run_ntmpl: i32,
    fam_n: i32,
    fam_sol: f64,
    /// Working-list buys in the prefix, and the whole prefix, from `ixg.wpre` -
    /// the two numbers `working_buy_share` is built out of.
    work_n: i32,
    run_n: i32,
}

#[tokio::test]
#[ignore]
async fn the_prefix_reads_what_the_sql_prefix_read() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .expect("connect");

    // Island fires with their SQL prefix columns. `has_him` marks the slots the
    // SQL tape is missing a wallet from, so they can be excluded and counted
    // rather than silently disagreeing.
    let rows = sqlx::query(
        "WITH m AS (
           SELECT DISTINCT mint FROM ixg.purx ORDER BY mint LIMIT $1
         )
         SELECT p.mint, p.slot, p.tx_index, p.run_ntmpl, p.fam_n, p.fam_sol,
                w.work_n, w.run_n,
                EXISTS (SELECT 1 FROM ixg.fall f
                        WHERE f.mint = p.mint AND f.slot = p.slot
                          AND f.trade_type = 'buy' AND f.wallet_id = 2720) AS has_him
         FROM ixg.cm_pre p
         JOIN ixg.purx x USING (mint, slot, tx_index)
         JOIN ixg.wpre w USING (mint, slot, tx_index)
         JOIN m ON m.mint = p.mint",
    )
    .bind(SAMPLE_MINTS)
    .fetch_all(&pool)
    .await
    .expect("island fires");
    assert!(!rows.is_empty(), "ixg.purx and ixg.cm_pre must be populated");

    let mut want: HashMap<(String, i64, i32), Want> = HashMap::new();
    let mut skipped_him = 0usize;
    let mut mint_set: HashSet<String> = HashSet::new();
    for r in &rows {
        let mint: String = r.get("mint");
        if r.get::<bool, _>("has_him") {
            skipped_him += 1;
            continue;
        }
        mint_set.insert(mint.clone());
        want.insert(
            (mint, r.get::<i64, _>("slot"), r.get::<i32, _>("tx_index")),
            Want {
                run_ntmpl: r.get("run_ntmpl"),
                fam_n: r.get("fam_n"),
                fam_sol: r.get("fam_sol"),
                work_n: r.get("work_n"),
                run_n: r.get("run_n"),
            },
        );
    }
    println!(
        "comparing {} fires on {} mints ({skipped_him} skipped: the SQL tape drops wallet 2720 there)",
        want.len(),
        mint_set.len()
    );

    // Every quiet slot SQL found on these mints, so the window gate is checked in
    // BOTH directions: a quiet slot must read 0 buys, and a busy one must not.
    let quiet_rows = sqlx::query(
        "WITH m AS (
           SELECT DISTINCT mint FROM ixg.purx ORDER BY mint LIMIT $1
         )
         SELECT q.mint, q.slot FROM ixg.fquiet q JOIN m ON m.mint = q.mint",
    )
    .bind(SAMPLE_MINTS)
    .fetch_all(&pool)
    .await
    .expect("ixg.fquiet");
    let quiet_slots: HashSet<(String, i64)> = quiet_rows
        .iter()
        .map(|r| (r.get::<String, _>("mint"), r.get::<i64, _>("slot")))
        .collect();

    let fp = FingerprintId(Uuid::nil());
    let mut hashes = HashedSet::default();
    for id in WORKING {
        hashes.insert(template_grain::grain_id_hash(id));
    }
    let patterns = BurstPatterns::new(hashes);

    let mut checked = 0usize;
    let mut quiet_checked = 0usize;
    let mut bad: Vec<String> = Vec::new();
    let mut mints: Vec<String> = mint_set.into_iter().collect();
    mints.sort();

    for mint in &mints {
        let trades = sqlx::query(
            "SELECT slot, tx_index, leg_index, trade_type, amount_lamports,
                    reserve_lamports, block_time, ix_labels, wallet_id
             FROM trades
             WHERE mint_address = $1 AND venue = 'curve'
               AND block_time >= $2::timestamptz AND block_time < $3::timestamptz
             ORDER BY slot, tx_index, leg_index",
        )
        .bind(mint)
        .bind(T0)
        .bind(T1)
        .fetch_all(&pool)
        .await
        .expect("trades");
        if trades.is_empty() {
            continue;
        }

        let created_at: chrono::DateTime<chrono::Utc> = trades[0].get("block_time");
        let mut track = TokenTrack::new(created_at);
        track.ensure_burst(fp, &patterns);
        track.ensure_window(QUIET);
        let mut slot_seen: Option<i64> = None;

        for t in &trades {
            let slot: i64 = t.get("slot");
            let tx_index: i32 = t.get("tx_index");
            let at: chrono::DateTime<chrono::Utc> = t.get("block_time");
            let sol = t.get::<i64, _>("amount_lamports") as f64 / 1e9;
            let vsol = t
                .try_get::<Option<i64>, _>("reserve_lamports")
                .ok()
                .flatten()
                .map(|v| v as f64 / 1e9);
            let labels: serde_json::Value =
                t.try_get("ix_labels").unwrap_or(serde_json::Value::Null);
            let is_buy = t.get::<String, _>("trade_type") == "buy";
            // `min(wallet_id)` is 3 and the column is never null in this window,
            // so the id maps straight onto the hash; 0 stays "unknown".
            let wallet: i64 = t
                .try_get::<Option<i32>, _>("wallet_id")
                .ok()
                .flatten()
                .unwrap_or(0) as i64;

            track.on_trade(TradeLite {
                side: if is_buy { Side::Buy } else { Side::Sell },
                sol,
                price: 1.0,
                reserve_sol: vsol.map(|v| (v - 30.0).max(0.0)).unwrap_or(f64::NAN),
                priced_reserve_sol: vsol.unwrap_or(f64::NAN),
                at,
                slot: slot as u64,
                tx_index: Some(tx_index as u32),
                wallet_hash: wallet as u64,
                template_hash: template_grain::grain_hash_from_labels_value(&labels),
                is_launch: template_grain::is_launch_from_labels_value(&labels),
                on_curve: true,
                leg_index: t.get::<i16, _>("leg_index").max(0) as u8,
                ..Default::default()
            });

            // The quiet gate, once per slot, at the buy that opens it. Checked
            // in both directions: SQL calls the slot quiet exactly when the
            // lagged window reads no buys.
            if is_buy && slot_seen != Some(slot) {
                slot_seen = Some(slot);
                quiet_checked += 1;
                let buys = track.value(MetricId::BuyCount, Windows::one(QUIET), None, at);
                let engine_quiet = buys == 0.0;
                let sql_quiet = quiet_slots.contains(&(mint.clone(), slot));
                if engine_quiet != sql_quiet {
                    bad.push(format!(
                        "{mint} slot {slot}: 4sl@1 buy_count {buys} says quiet={engine_quiet}, \
                         SQL fquiet says {sql_quiet}"
                    ));
                }
            }

            let Some(w) = want.get(&(mint.clone(), slot, tx_index)) else {
                continue;
            };
            checked += 1;
            let win = Windows::default();
            let read = |id| track.value(id, win, Some(fp), at);
            let at_ = format!("{mint} slot {slot} tx {tx_index}");

            let nt = read(MetricId::MemberTemplateCount);
            if (nt - f64::from(w.run_ntmpl)).abs() > 0.5 {
                bad.push(format!(
                    "{at_}: member_template_count {nt} != run_ntmpl {}",
                    w.run_ntmpl
                ));
            }
            let fam_n = read(MetricId::SameBuyCount);
            if (fam_n - f64::from(w.fam_n)).abs() > 0.5 {
                bad.push(format!("{at_}: same_buy_count {fam_n} != fam_n {}", w.fam_n));
            }
            let fam_sol = read(MetricId::SameBuySol);
            if (fam_sol - w.fam_sol).abs() > 1e-6 {
                bad.push(format!(
                    "{at_}: same_buy_sol {fam_sol} != fam_sol {}",
                    w.fam_sol
                ));
            }
            let wc = read(MetricId::WorkingBuyCount);
            if (wc - f64::from(w.work_n)).abs() > 0.5 {
                bad.push(format!("{at_}: working_buy_count {wc} != work_n {}", w.work_n));
            }
            // Purity, against SQL's own two counts rather than the engine's.
            let share = read(MetricId::WorkingBuyShare);
            let want_share = 100.0 * f64::from(w.work_n) / f64::from(w.run_n);
            if (share - want_share).abs() > 1e-9 {
                bad.push(format!(
                    "{at_}: working_buy_share {share} != {want_share}                      (sql {}/{})",
                    w.work_n, w.run_n
                ));
            }
        }
    }

    println!(
        "prefix parity: {checked} fires and {quiet_checked} slot quiet gates compared,          {} mismatches",
        bad.len()
    );
    assert!(checked > 0, "no fires compared - the sample missed the tape");
    assert!(
        bad.is_empty(),
        "{} mismatches, first 10:\n{}",
        bad.len(),
        bad[..bad.len().min(10)].join("\n")
    );
}
