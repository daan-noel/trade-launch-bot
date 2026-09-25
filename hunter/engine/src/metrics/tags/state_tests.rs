//! Tests for [`super::state`]: the tag classifier and its totals, including every
//! behaviour the three classifiers it replaces (volume list, dump list, copy wallets)
//! pinned.

use super::config::compile_tags;
use super::state::*;
use crate::metrics::fee::FeeKeys;
use crate::metrics::registry::Metric;
use crate::metrics::template_grain::program_id_hash;
use crate::metrics::trade_keys::{ix_hash, ix_hash_opt, marker_mask, wallet_hash};
use crate::metrics::{Cursor, Side, TradeLite, Ts, WindowSpec};
use chrono::{Duration, TimeZone, Utc};
use serde_json::{json, Value};

fn c(slot: u64) -> Cursor {
    Cursor { slot, print: 0 }
}

fn ts(secs: f64) -> Ts {
    Utc.timestamp_opt(1_700_000_000, 0).unwrap() + Duration::milliseconds((secs * 1000.0) as i64)
}

fn trade(side: Side, sol: f64, ix: Option<u64>, wallet: u64, secs: f64) -> TradeLite {
    TradeLite {
        side,
        sol,
        price: 1.0,
        reserve_sol: 10.0,
        priced_reserve_sol: 10.0,
        at: ts(secs),
        ix_hash: ix,
        wallet_hash: wallet,
        ..Default::default()
    }
}

/// One tag from a definition document.
fn tag(def: Value) -> TagState {
    let tags = compile_tags(&json!({ "t": def }));
    TagState::new(tags.into_iter().next().expect("tag compiles").patterns)
}

fn shapes(seqs: &[&[&str]]) -> Value {
    Value::Array(seqs.iter().map(|s| json!(s)).collect())
}

const LIFE: Option<WindowSpec> = None;

fn read(st: &TagState, m: Metric, negated: bool, w: Option<WindowSpec>, at: Ts, cur: Cursor) -> f64 {
    st.value(m, negated, w, at, cur)
}

#[test]
fn shape_sticky_creator_and_missing_ix() {
    let create_buy = ix_hash(&["Pump.Fun: Create", "Pump.Fun: Buy"]);
    let mut st = tag(json!({
        "match": { "ix_shape": shapes(&[&["Pump.Fun: Create", "Pump.Fun: Buy"]]), "creator": true },
        "sticky": true
    }));
    st.set_creator(wallet_hash("creator"));
    assert!(st.carries(&trade(Side::Buy, 1.0, None, wallet_hash("creator"), 0.0)), "creator");
    let w = wallet_hash("bot1");
    let t = trade(Side::Buy, 2.0, Some(create_buy), w, 1.0);
    assert!(st.carries(&t), "listed shape");
    st.on_trade(&t, c(0));
    assert!(st.carries(&trade(Side::Sell, 1.0, None, w, 2.0)), "sticky wallet, any shape");
    assert!(!st.carries(&trade(Side::Buy, 1.0, None, wallet_hash("normie"), 3.0)), "no labels");
    assert!(!st.carries(&trade(Side::Buy, 1.0, Some(ix_hash(&["Pump.Fun: Buy"])), wallet_hash("n2"), 4.0)));
}

#[test]
fn lifetime_halves_and_share() {
    let vol = ix_hash(&["vol"]);
    let mut st = tag(json!({ "match": { "ix_shape": shapes(&[&["vol"]]) } }));
    st.on_trade(&trade(Side::Buy, 4.0, Some(vol), 1, 0.0), c(0));
    st.on_trade(&trade(Side::Buy, 6.0, None, 2, 1.0), c(0));
    st.on_trade(&trade(Side::Sell, 1.0, Some(vol), 1, 2.0), c(0));
    let at = ts(2.0);
    assert_eq!(read(&st, Metric::BuySol, false, LIFE, at, c(0)), 4.0);
    assert_eq!(read(&st, Metric::SellSol, false, LIFE, at, c(0)), 1.0);
    assert_eq!(read(&st, Metric::GrossSol, false, LIFE, at, c(0)), 5.0);
    assert_eq!(read(&st, Metric::NetSol, false, LIFE, at, c(0)), 3.0);
    assert_eq!(read(&st, Metric::BuySol, true, LIFE, at, c(0)), 6.0);
    assert_eq!(read(&st, Metric::GrossSol, true, LIFE, at, c(0)), 6.0);
    let share = read(&st, Metric::TagSharePct, false, LIFE, at, c(0));
    assert!((share - 500.0 / 11.0).abs() < 1e-9);
    let rest = read(&st, Metric::TagSharePct, true, LIFE, at, c(0));
    assert!((share + rest - 100.0).abs() < 1e-9, "the two halves share 100 %");
    let empty = tag(json!({ "match": { "creator": true } }));
    assert!(empty.value(Metric::TagSharePct, false, LIFE, at, c(0)).is_nan());
}

#[test]
fn a_window_decays_on_a_tick_and_the_life_does_not() {
    let vol = ix_hash(&["vol"]);
    let w = WindowSpec::secs(10.0);
    let mut st = tag(json!({ "match": { "ix_shape": shapes(&[&["vol"]]) } }));
    st.ensure_window(w);
    st.on_trade(&trade(Side::Buy, 4.0, Some(vol), 1, 0.0), c(0));
    st.on_trade(&trade(Side::Buy, 6.0, None, 2, 1.0), c(0));
    assert_eq!(read(&st, Metric::BuySol, false, Some(w), ts(1.0), c(0)), 4.0);
    assert_eq!(read(&st, Metric::BuySol, true, Some(w), ts(1.0), c(0)), 6.0);
    // Closed window [now - 10 s, now]: at t = 11 the t = 1 trade is still in.
    st.on_tick(ts(11.0), c(0));
    assert_eq!(read(&st, Metric::BuySol, false, Some(w), ts(11.0), c(0)), 0.0);
    assert_eq!(read(&st, Metric::BuySol, true, Some(w), ts(11.0), c(0)), 6.0);
    st.on_tick(ts(11.001), c(0));
    assert_eq!(read(&st, Metric::BuySol, true, Some(w), ts(11.001), c(0)), 0.0);
    assert_eq!(st.window_len(w), Some(0), "the tick evicted, not just the read");
    assert_eq!(read(&st, Metric::BuySol, false, LIFE, ts(11.001), c(0)), 4.0);
    let unregistered = Some(WindowSpec::secs(3.0));
    assert!(read(&st, Metric::BuySol, false, unregistered, ts(0.0), c(0)).is_nan());
}

/// The running-totals read equals a brute-force re-fold of the in-window trades at
/// every probe instant, including out-of-order arrivals and instants no eviction ran at.
#[test]
fn windowed_running_totals_equal_a_brute_force_refold() {
    let vol = ix_hash(&["vol"]);
    let script: &[(Side, f64, bool, f64, u8)] = &[
        (Side::Buy, 3.0, true, 0.0, 0),
        (Side::Sell, 1.0, false, 4.0, 0),
        (Side::Buy, 2.0, true, 9.0, 1),
        (Side::Buy, 5.0, false, 7.0, 0), // regressed
        (Side::Sell, 4.0, true, 12.0, 0),
        (Side::Buy, 1.5, false, 11.0, 2), // regressed
        (Side::Sell, 0.0, true, 13.0, 0), // zero-SOL sell: `-0.0`
        (Side::Sell, 0.5, true, 25.0, 1),
    ];
    let metrics = [
        Metric::BuySol,
        Metric::SellSol,
        Metric::NetSol,
        Metric::GrossSol,
        Metric::BuyCount,
        Metric::SellCount,
        Metric::TradeCount,
        Metric::BuyTxCount,
        Metric::SellTxCount,
        Metric::TagSharePct,
    ];
    for size in [1.0_f64, 5.0, 10.0, 60.0] {
        let spec = WindowSpec::secs(size);
        let mut st = tag(json!({ "match": { "ix_shape": shapes(&[&["vol"]]) } }));
        st.ensure_window(spec);
        let mut folded: Vec<TradeLite> = Vec::new();
        let mut wallet = 100u64;
        let mut latest = f64::MIN;
        for &(side, sol, tagged, at, leg) in script {
            wallet += 1;
            latest = latest.max(at);
            let mut t = trade(side, sol, tagged.then_some(vol), wallet, at);
            t.leg_index = leg;
            st.on_trade(&t, c(0));
            folded.push(t);
            // Reads happen at or after the newest folded print (the fold never reads
            // the past); a regressed print lands behind `latest`.
            for probe in [0.0, 0.5, 3.0, 12.0] {
                let now = ts(latest + probe);
                let (lo, hi) = spec.bounds(now.timestamp_millis());
                // A fresh lifetime-only state fed just the in-window trades.
                let mut want = tag(json!({ "match": { "ix_shape": shapes(&[&["vol"]]) } }));
                for t in folded.iter().filter(|t| (lo..=hi).contains(&t.at.timestamp_millis())) {
                    want.on_trade(t, c(0));
                }
                for m in metrics {
                    for negated in [false, true] {
                        let got = st.value(m, negated, Some(spec), now, c(0));
                        let exp = want.value(m, negated, LIFE, now, c(0));
                        assert!(
                            (got - exp).abs() < 1e-9 || (got.is_nan() && exp.is_nan()),
                            "{m:?} negated={negated} w={size} at={at} latest={latest} probe={probe}: {got} != {exp}"
                        );
                    }
                }
            }
        }
    }
}

/// The shared parity fixture (twin: the chart's `classifyFlow.parity.test.ts`). A case's
/// `patterns` + `creator` is a tag with those shapes, the creator matcher and `sticky`.
#[test]
fn the_shared_parity_fixture() {
    #[derive(serde::Deserialize)]
    struct Case {
        name: String,
        patterns: Vec<Vec<String>>,
        creator: Option<String>,
        trades: Vec<FixtureTrade>,
        expect: Expect,
    }
    #[derive(serde::Deserialize)]
    struct FixtureTrade {
        wallet: String,
        side: String,
        sol: f64,
        labels: Option<Vec<String>>,
    }
    #[derive(serde::Deserialize)]
    struct Expect {
        tagged_buy: f64,
        tagged_sell: f64,
        untagged_buy: f64,
        untagged_sell: f64,
    }
    #[derive(serde::Deserialize)]
    struct Fixture {
        cases: Vec<Case>,
    }
    let fixture: Fixture =
        serde_json::from_str(include_str!("../../../fixtures/flow_ix_parity.json")).expect("fixture parses");
    assert!(!fixture.cases.is_empty());
    for case in fixture.cases {
        let mut st = tag(json!({ "match": { "ix_shape": case.patterns, "creator": true }, "sticky": true }));
        if let Some(creator) = &case.creator {
            st.set_creator(wallet_hash(creator));
        }
        for (i, t) in case.trades.iter().enumerate() {
            let side = if t.side == "buy" { Side::Buy } else { Side::Sell };
            let ix = t.labels.as_deref().and_then(ix_hash_opt);
            st.on_trade(&trade(side, t.sol, ix, wallet_hash(&t.wallet), i as f64), c(0));
        }
        let now = ts(case.trades.len() as f64);
        for (m, negated, want, label) in [
            (Metric::BuySol, false, case.expect.tagged_buy, "tagged_buy"),
            (Metric::SellSol, false, case.expect.tagged_sell, "tagged_sell"),
            (Metric::BuySol, true, case.expect.untagged_buy, "untagged_buy"),
            (Metric::SellSol, true, case.expect.untagged_sell, "untagged_sell"),
        ] {
            let got = st.value(m, negated, LIFE, now, c(0));
            assert!((got - want).abs() < 1e-9, "case {:?}: {label} = {got}, expected {want}", case.name);
        }
    }
}

/// A marker is a mechanism: a NEW bot build still creates a throwaway account, so
/// `ix_contains` catches it where a shape list books it as human.
#[test]
fn ix_contains_catches_a_shape_the_list_has_never_seen() {
    let unlisted = TradeLite {
        ix_hash: Some(ix_hash(&[
            "Compute Budget: SetComputeUnitLimit",
            "System Program: CreateAccountWithSeed",
            "Pump.Fun: Buy",
        ])),
        marker_bits: marker_mask(&["CreateAccountWithSeed"]).unwrap(),
        ..Default::default()
    };
    let by_shape =
        tag(json!({ "match": { "ix_shape": shapes(&[&["System Program: CreateAccountWithSeed", "Pump.Fun: Buy"]]) } }));
    assert!(!by_shape.carries(&unlisted));
    let by_marker = tag(json!({ "match": { "ix_contains": ["CreateAccountWithSeed"] } }));
    assert!(by_marker.carries(&unlisted));
}

/// `ix_lacks` is the inverse claim: everything that did NOT come through a router.
#[test]
fn ix_lacks_tags_everything_without_the_marker() {
    let st = tag(json!({ "match": { "ix_lacks": ["Axiom Trade"] } }));
    let axiom = TradeLite { marker_bits: marker_mask(&["Axiom Trade"]).unwrap(), ..Default::default() };
    assert!(!st.carries(&axiom));
    assert!(st.carries(&TradeLite::default()));
}

/// Without `creator` and `sticky` a tag is a pure function of the transaction.
#[test]
fn without_wallet_rules_a_tag_reads_the_transaction_alone() {
    let seed = marker_mask(&["CreateAccountWithSeed"]).unwrap();
    let mut st = tag(json!({ "match": { "ix_contains": ["CreateAccountWithSeed"] } }));
    st.set_creator(7);
    assert!(!st.carries(&TradeLite { wallet_hash: 7, ..Default::default() }), "the creator is not tagged");
    assert!(st.carries(&TradeLite { wallet_hash: 7, marker_bits: seed, ..Default::default() }));
}

/// A 4-leg dump transaction is one transaction and all of its SOL; only listed sells
/// count; a buy never carries a sell-side tag; the creator matcher lists the creator's
/// sells whatever their shape.
#[test]
fn a_dump_list_counts_transactions_and_every_legs_sol() {
    let dump = ix_hash(&["Pump.Fun: Sell"]);
    let one = WindowSpec::prints(1.0, 0.0);
    let mut st = tag(json!({
        "match": { "ix_shape": shapes(&[&["Pump.Fun: Sell"]]), "creator": true },
        "side": "sell"
    }));
    st.set_creator(wallet_hash("dev"));
    st.ensure_window(one);
    let at = ts(0.0);
    for leg in 0..4u8 {
        let mut t = trade(Side::Sell, 0.5, Some(dump), 10 + u64::from(leg), 0.0);
        t.leg_index = leg;
        st.on_trade(&t, Cursor { slot: 1, print: u64::from(leg) + 1 });
    }
    assert_eq!(read(&st, Metric::SellTxCount, false, LIFE, at, c(1)), 1.0);
    assert_eq!(read(&st, Metric::SellSol, false, LIFE, at, c(1)), 2.0);
    st.on_trade(&trade(Side::Buy, 3.0, Some(dump), 20, 1.0), Cursor { slot: 2, print: 5 });
    assert_eq!(read(&st, Metric::BuySol, false, LIFE, at, c(2)), 0.0, "a sell-side tag never takes a buy");
    let cur6 = Cursor { slot: 3, print: 6 };
    st.on_trade(&trade(Side::Sell, 9.0, Some(ix_hash(&["Other: Sell"])), 21, 2.0), cur6);
    assert_eq!(read(&st, Metric::SellTxCount, false, Some(one), at, cur6), 0.0, "unlisted shape");
    let cur7 = Cursor { slot: 4, print: 7 };
    st.on_trade(&trade(Side::Sell, 1.0, None, wallet_hash("dev"), 3.0), cur7);
    assert_eq!(read(&st, Metric::SellTxCount, false, Some(one), at, cur7), 1.0, "the creator's sell");
}

/// A wallet list reads both sides of a named wallet, never an anonymous print.
#[test]
fn a_wallet_list_counts_both_sides_and_never_wallet_zero() {
    let mut st = tag(json!({ "match": { "wallet": ["Target111"] } }));
    let who = wallet_hash("Target111");
    st.on_trade(&trade(Side::Buy, 1.0, None, who, 0.0), c(0));
    st.on_trade(&trade(Side::Sell, 0.4, None, who, 1.0), c(0));
    st.on_trade(&trade(Side::Buy, 5.0, None, wallet_hash("someone"), 2.0), c(0));
    st.on_trade(&trade(Side::Buy, 5.0, None, 0, 3.0), c(0));
    let at = ts(3.0);
    assert_eq!(read(&st, Metric::BuySol, false, LIFE, at, c(0)), 1.0);
    assert_eq!(read(&st, Metric::SellSol, false, LIFE, at, c(0)), 0.4);
    assert_eq!(read(&st, Metric::BuyTxCount, false, LIFE, at, c(0)), 1.0);
}

/// The zero-SOL sell is a sell on eviction too, so its count cannot drift.
#[test]
fn a_zero_sol_tagged_sell_is_a_sell_on_both_sides_of_the_window() {
    let dump = ix_hash(&["Pump.Fun: Sell"]);
    let w = WindowSpec::slots(1.0, 0.0);
    let mut st = tag(json!({ "match": { "ix_shape": shapes(&[&["Pump.Fun: Sell"]]) } }));
    st.ensure_window(w);
    let at = ts(0.0);
    let mk = |side: Side, sol: f64, wallet: u64, slot: u64| TradeLite {
        side,
        sol,
        at,
        slot,
        ix_hash: Some(dump),
        wallet_hash: wallet,
        ..Default::default()
    };
    st.on_trade(&mk(Side::Buy, 1.0, 1, 100), c(100));
    st.on_trade(&mk(Side::Sell, 0.0, 2, 100), c(100));
    assert_eq!(read(&st, Metric::BuyCount, false, Some(w), at, c(100)), 1.0);
    assert_eq!(read(&st, Metric::SellCount, false, Some(w), at, c(100)), 1.0);
    st.on_trade(&mk(Side::Buy, 1.0, 3, 200), c(200));
    assert_eq!(read(&st, Metric::BuyCount, false, Some(w), at, c(200)), 1.0, "the evicted zero-SOL SELL left the buys alone");
    assert_eq!(read(&st, Metric::SellCount, false, Some(w), at, c(200)), 0.0);
    assert_eq!(read(&st, Metric::BuyCount, false, LIFE, at, c(200)), 2.0);
    assert_eq!(read(&st, Metric::SellCount, false, LIFE, at, c(200)), 1.0);
}

#[test]
fn a_program_tags_every_variant_it_ships() {
    let st = tag(json!({ "match": { "program": ["Unknown (crew)"] } }));
    let mut t = trade(Side::Buy, 1.0, Some(ix_hash(&["new variant"])), wallet_hash("a"), 0.0);
    t.program_hash = Some(program_id_hash("Unknown (crew)"));
    assert!(st.carries(&t));
    t.program_hash = Some(program_id_hash("Axiom Trade"));
    assert!(!st.carries(&t));
    t.program_hash = None;
    assert!(!st.carries(&t));
}

fn crew_trade(side: Side, sol: f64, wallet: &str, slot: u64) -> TradeLite {
    TradeLite {
        slot,
        price: 1e-6,
        priced_reserve_sol: 40.0,
        token_amount: sol * 1e6,
        ..trade(side, sol, Some(ix_hash(&["Axiom Trade: ix#00"])), wallet_hash(wallet), 0.0)
    }
}

/// The 3rd close member of a slot group is the first tagged; a size outside the
/// tolerance of the FIRST member neither counts nor tags; a new slot starts empty;
/// another fee preset is another group.
#[test]
fn a_cluster_tags_from_the_nth_close_member() {
    let mut st = tag(json!({ "match": { "cluster": { "min_prints": 3, "sol_tol_pct": 10 } } }));
    let lift = |st: &TagState| st.value(Metric::BuySol, false, LIFE, ts(0.0), c(0));
    let fold = |st: &mut TagState, sol: f64, w: &str, slot: u64| {
        let before = lift(st);
        st.on_trade(&crew_trade(Side::Buy, sol, w, slot), c(slot));
        lift(st) > before
    };
    assert!(!fold(&mut st, 1.00, "a", 7));
    assert!(!fold(&mut st, 1.05, "b", 7));
    assert!(!fold(&mut st, 2.00, "c", 7), "outside 10 % of the first");
    assert!(fold(&mut st, 0.95, "d", 7), "the third close member");
    assert!(!fold(&mut st, 1.00, "e", 8), "a new slot starts empty");
    let mut t = crew_trade(Side::Buy, 1.0, "f", 8);
    t.fee = FeeKeys::new(Some(200_000), None, None);
    let before = lift(&st);
    st.on_trade(&t, c(8));
    assert_eq!(lift(&st), before, "another fee preset is another group");
}

/// A wallet that buys untagged in the creation slot counts on neither side for the
/// rest of the coin; a tagged creation-slot buy stays tagged.
#[test]
fn creation_slot_buyers_are_excluded_from_both_halves() {
    let mut st = tag(json!({ "match": { "creator": true }, "sticky": true, "exclude_creation_slot": true }));
    st.set_creator(wallet_hash("dev"));
    let mut launch = crew_trade(Side::Buy, 0.5, "dev", 100);
    launch.is_launch = true;
    st.on_trade(&launch, c(100));
    st.on_trade(&crew_trade(Side::Buy, 2.0, "bundle", 100), c(100));
    st.on_trade(&crew_trade(Side::Sell, 1.0, "bundle", 105), c(105));
    st.on_trade(&crew_trade(Side::Buy, 3.0, "retail", 105), c(105));
    let at = ts(0.0);
    assert_eq!(read(&st, Metric::BuySol, false, LIFE, at, c(105)), 0.5, "the creator's buy");
    assert_eq!(read(&st, Metric::BuySol, true, LIFE, at, c(105)), 3.0, "retail only");
    assert_eq!(read(&st, Metric::SellSol, true, LIFE, at, c(105)), 0.0, "the bundle's sell is on no side");
}

/// `profit_sol` is the bag sold into the curve at the last print, minus the half's net
/// SOL, the bag floored at zero; a trade without an amount poisons that half only.
#[test]
fn profit_is_bag_value_minus_net_sol_in() {
    let mut st = tag(json!({ "match": { "program": ["crew"] } }));
    let crew = |side: Side, sol: f64, slot: u64, tokens: f64, vsol: f64, vtok: f64| TradeLite {
        program_hash: Some(program_id_hash("crew")),
        token_amount: tokens,
        priced_reserve_sol: vsol,
        price: vsol / vtok,
        ..crew_trade(side, sol, "w", slot)
    };
    st.on_trade(&crew(Side::Buy, 2.0, 1, 5.0e7, 40.0, 1.0e9), c(1));
    let (v, vt, bag) = (40.0_f64, 1.0e9_f64, 5.0e7_f64);
    let want = v - v * vt / (vt + bag) - 2.0;
    assert!((st.profit_sol(false) - want).abs() < 1e-9);
    st.on_trade(&crew(Side::Sell, 3.0, 2, 9.0e7, 38.0, 1.1e9), c(2));
    assert!((st.profit_sol(false) - 1.0).abs() < 1e-9, "3 out - 2 in = 1, got {}", st.profit_sol(false));
    st.on_trade(&crew(Side::Buy, 1.0, 3, f64::NAN, 38.0, 1.1e9), c(3));
    assert!(st.profit_sol(false).is_nan());
    assert!(st.profit_sol(true).is_finite(), "the other half keeps its own bag");
}

#[test]
fn an_edited_definition_moves_the_future_not_the_past() {
    let mut st = tag(json!({ "match": { "wallet": ["A"] } }));
    st.on_trade(&trade(Side::Buy, 1.0, None, wallet_hash("A"), 0.0), c(0));
    let edited = compile_tags(&json!({ "t": { "match": { "wallet": ["B"] } } }));
    st.set_patterns(&edited[0].patterns);
    st.on_trade(&trade(Side::Buy, 2.0, None, wallet_hash("B"), 1.0), c(0));
    st.on_trade(&trade(Side::Buy, 4.0, None, wallet_hash("A"), 2.0), c(0));
    assert_eq!(read(&st, Metric::BuySol, false, LIFE, ts(2.0), c(0)), 3.0);
}
