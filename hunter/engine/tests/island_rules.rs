//! The island rules, authored as the engine's own `strategy_rules.params` JSON.
//!
//! They are the rules derived in `hunter/docs/plans/strategies/island-map.md`, and this
//! file is the contract that they PARSE and VALIDATE against the live registry through
//! `RuleParams::parse` — the same gate a rule save runs — so a typo in a group, metric
//! or operator name fails at build time rather than silently never matching.
//!
//! It also pins the shape of each island: the entry terms that define it and the shared
//! `stop 5 / trail 20` exit, so a later edit that quietly drops a term is caught.
//!
//! Three rules, not the previous four. `island-map.md` records why the impulse and
//! quiet-accumulation islands are gone: both were positive only at a fill that arrives
//! before the next print, and both are negative on cohorts the search never saw.

use hunter_engine::metrics::evaluator::Operator;
use hunter_engine::metrics::MetricId;
use hunter_engine::rule_params::{RuleParams, SideConditions};

/// Absorption — a minute of buyer-dominated tape on a live but unrun token.
///
/// The one island of the previous map that survives its own execution. The entry is
/// unchanged; only the exit moved, from `stop 3 / trail 5` to `stop 5 / trail 20`,
/// which is worth +5.62 -> +18.93 SOL over the study week on the entry alone.
const ABSORPTION: &str = r#"{
  "stop_loss": 5,
  "entry": {
    "m_snapshot": {
      "time": [{"operator": ">", "value": 5}],
      "liquidity": [{"operator": ">=", "value": 3}, {"operator": "<=", "value": 64}],
      "ix_count": [{"operator": "<=", "value": 5}]
    },
    "m_flow_lifetime": { "gross_flow": [{"operator": "<=", "value": 148}] },
    "m_flow_window": [
      { "window_size_sec": 60, "buy_share": [{"operator": ">", "value": 84}] },
      { "window_size_sec": 30, "trade_count": [{"operator": ">", "value": 8}] }
    ]
  },
  "exit": { "m_position": { "retrace": [{"operator": ">=", "value": 20}] } }
}"#;

/// Island A — continuation. Buy what has ALREADY tripled and is still being bought,
/// then leave on a clock. The only rule in the map confirmed on the real kernel.
///
/// The direct inversion of the refuted impulse island, which required `rise(3) <= 9`:
/// anticipating a move needs a reaction the bot does not have, joining one does not.
/// Carries no `liquidity` ceiling and no lifetime-flow gate — `rise(30) >= 207` already
/// selects a token that has run, so the previous map's "has not run yet" filter would
/// contradict it.
///
/// The exit is a TIME CAP with a wide disaster stop, not a stop-and-trail. Same entry,
/// same trades, same 95 ms fill: `stop 5 + retrace 20` books -2.56 SOL and
/// `stop 20 + held 40` books +2.53. A reactive exit fires right after an adverse move and
/// then waits 95 ms, into the continuation of that move; a clock fires at an instant the
/// market did not choose, so its fill is not selected against.
const CONTINUATION: &str = r#"{
  "stop_loss": 20,
  "entry": {
    "m_snapshot": {
      "time": [{"operator": ">", "value": 5}],
      "liquidity": [{"operator": ">=", "value": 3}]
    },
    "m_flow_window": [
      {
        "window_size_sec": 30,
        "net_flow": [{"operator": ">=", "value": 26.9}],
        "buy_share": [{"operator": ">=", "value": 92.1}]
      }
    ],
    "m_price_window": [{ "window_size_sec": 30, "rise": [{"operator": ">=", "value": 207}] }]
  },
  "exit": { "m_position": { "held": [{"operator": ">=", "value": 40}] } }
}"#;

/// Island B — the quiet pause. A large move, then the tape stops.
///
/// Two terms, the cheapest island in the map to state, and the flattest across the lag
/// ladder: it keeps 98% of its money when every fill is resolved at the bot's p90
/// reaction instead of its p50.
const QUIET_PAUSE: &str = r#"{
  "stop_loss": 5,
  "entry": {
    "m_snapshot": {
      "time": [{"operator": ">", "value": 5}],
      "liquidity": [{"operator": ">=", "value": 3}]
    },
    "m_flow_window": [
      { "window_size_sec": 10, "trade_count": [{"operator": "<=", "value": 22}] }
    ],
    "m_price_window": [{ "window_size_sec": 60, "rise": [{"operator": ">=", "value": 322}] }]
  },
  "exit": { "m_position": { "retrace": [{"operator": ">=", "value": 20}] } }
}"#;

/// A and B agreeing — the quality peak: +0.98% net per trade against +0.53% and +0.36%
/// standalone, 42.8% win forward, and only 3% of its money inside a 50 ms gap.
const A_AND_B: &str = r#"{
  "stop_loss": 5,
  "entry": {
    "m_snapshot": {
      "time": [{"operator": ">", "value": 5}],
      "liquidity": [{"operator": ">=", "value": 3}]
    },
    "m_flow_window": [
      {
        "window_size_sec": 30,
        "net_flow": [{"operator": ">=", "value": 26.9}],
        "buy_share": [{"operator": ">=", "value": 92.1}]
      },
      { "window_size_sec": 10, "trade_count": [{"operator": "<=", "value": 22}] }
    ],
    "m_price_window": [
      { "window_size_sec": 30, "rise": [{"operator": ">=", "value": 207}] },
      { "window_size_sec": 60, "rise": [{"operator": ">=", "value": 322}] }
    ]
  },
  "exit": { "m_position": { "retrace": [{"operator": ">=", "value": 20}] } }
}"#;

const ISLANDS: &[(&str, &str)] = &[
    ("absorption", ABSORPTION),
    ("A continuation", CONTINUATION),
    ("B quiet pause", QUIET_PAUSE),
    ("A AND B", A_AND_B),
];

fn parse(json: &str) -> RuleParams {
    let v: serde_json::Value = serde_json::from_str(json).expect("rule JSON is well formed");
    RuleParams::parse(&v).unwrap_or_else(|e| panic!("rule failed the save gate: {e}"))
}

fn has(side: &SideConditions, id: MetricId) -> bool {
    side.0.values().flatten().any(|g| g.metrics.contains_key(&id))
}

/// Every authored `(operator, value)` on `id`, flattened out of the DNF, paired with the
/// window its group instance was authored at.
fn terms(side: &SideConditions, id: MetricId) -> Vec<(Operator, f64, Option<f64>)> {
    side.0
        .values()
        .flatten()
        .flat_map(|g| {
            let w = g.strict_param("window_size_sec");
            g.metrics
                .get(&id)
                .into_iter()
                .flatten()
                .flatten()
                .map(move |c| (c.operator, c.value, w))
        })
        .collect()
}

/// Every window any condition on this side is authored at.
fn windows(side: &SideConditions) -> Vec<f64> {
    side.0
        .values()
        .flatten()
        .filter(|g| !g.metrics.is_empty())
        .filter_map(|g| g.strict_param("window_size_sec"))
        .collect()
}

#[test]
fn every_island_rule_parses_and_validates() {
    for (name, json) in ISLANDS {
        let p = parse(json);
        assert!(p.entry.is_some(), "{name}: entry conditions are the island");
        assert!(p.exit.is_some(), "{name}: needs the trailing exit");
        assert!(p.stop_loss.is_some(), "{name}: every island carries a disaster stop");
        assert_eq!(
            p.take_profit, None,
            "{name}: a static take-profit caps the right tail the book is paid by"
        );
        let exit = p.exit.as_ref().expect("exit present");
        assert!(
            has(exit, MetricId::Retrace) || has(exit, MetricId::Held),
            "{name}: the exit is either the wide trail or the clock"
        );
    }
}

/// Each rule round-trips through the canonical JSONB shape, so what this test pins is
/// what a save writes and a load reads back.
#[test]
fn every_island_rule_round_trips() {
    for (name, json) in ISLANDS {
        let once = parse(json);
        let twice = RuleParams::parse(&once.to_value())
            .unwrap_or_else(|e| panic!("{name}: re-parse of its own output failed: {e}"));
        assert_eq!(once, twice, "{name}: JSONB round trip is not lossless");
    }
}

/// The confirmed rule exits on a CLOCK, and its stop is a disaster brake rather than a
/// working part. Both halves are load-bearing and were measured on the real kernel:
/// swapping this exit for `stop 5 + retrace 20` takes the same entry from +2.53 to -2.56,
/// and tightening the stop from 20 to 8 costs 12% in-sample and 51% forward.
#[test]
fn the_confirmed_island_exits_on_a_clock_with_a_wide_stop() {
    let p = parse(CONTINUATION);
    assert_eq!(p.stop_loss, Some(20.0), "the stop is a disaster brake, not a working part");
    let exit = p.exit.as_ref().expect("exit");
    assert!(has(exit, MetricId::Held), "the clock IS the exit");
    assert!(
        !has(exit, MetricId::Retrace),
        "a reactive exit is adversely selected at its own fill - that is the whole finding"
    );
    let held = terms(exit, MetricId::Held);
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].1, 40.0, "forty seconds");
}

/// `trade_count` is what island B is built on, and it is a real metric rather than the
/// `unique_wallets` stand-in the previous map used. One wallet re-entering ten times is
/// ten trades and one wallet, and B's "the tape has gone quiet" means the former.
#[test]
fn trade_count_is_reachable_and_is_not_unique_wallets() {
    for (name, json) in [("B quiet pause", QUIET_PAUSE), ("absorption", ABSORPTION)] {
        let e = parse(json).entry.expect("entry");
        assert!(has(&e, MetricId::TradeCount), "{name}: trade_count reaches the rule");
        assert!(
            !has(&e, MetricId::UniqueWallets),
            "{name}: the stand-in is replaced, not kept alongside"
        );
    }
}

/// The surviving islands buy tokens that HAVE moved. A `rise` lower bound is the term
/// that inverts the refuted impulse island, so assert its direction, not just that a
/// `rise` condition exists — an edit flipping it back to `<=` would rebuild the rule
/// that does not survive its own fill.
#[test]
fn the_new_islands_buy_a_move_that_has_already_happened() {
    for (name, json) in [("A continuation", CONTINUATION), ("B quiet pause", QUIET_PAUSE)] {
        let e = parse(json).entry.expect("entry");
        let rises = terms(&e, MetricId::WinRise);
        assert!(!rises.is_empty(), "{name}: a rise term defines the island");
        for (op, v, w) in rises {
            assert!(
                matches!(op, Operator::Gte | Operator::Gt),
                "{name}: rise {} {v} at {w:?}s must be a LOWER bound — an upper bound                  rebuilds the refuted impulse island",
                op.symbol(),
            );
        }
    }
}

/// No island may read a window narrower than 2 seconds. A sub-slot window cannot survive
/// the bot's 95 ms reaction, and that is the single finding that killed the previous map.
#[test]
fn no_island_reads_a_sub_slot_window() {
    for (name, json) in ISLANDS {
        let e = parse(json).entry.expect("entry");
        for w in windows(&e) {
            assert!(w >= 2.0, "{name}: reads a {w}s window — below 2s is unreachable at 95 ms");
        }
    }
}

/// The conjunction must carry BOTH islands' terms — a merge that silently dropped one
/// side would look like a working rule while being only half of one.
#[test]
fn the_conjunction_carries_both_islands() {
    let e = parse(A_AND_B).entry.expect("entry");
    assert!(has(&e, MetricId::NetFlow), "A's inflow term");
    assert!(has(&e, MetricId::BuyShare), "A's direction term");
    assert!(has(&e, MetricId::TradeCount), "B's quiet-tape term");
    let mut ws: Vec<f64> = terms(&e, MetricId::WinRise).iter().map(|t| t.2.unwrap()).collect();
    ws.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(ws, vec![30.0, 60.0], "both islands' rise terms, at 30s and 60s");
}
