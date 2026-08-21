//! The island rules, authored as the engine's own `strategy_rules.params` JSON.
//!
//! Four rules, kept separate so each island can be shipped and evaluated on its own.
//! They are the rules derived in `hunter/docs/plans/strategies/island-map.md`, and this
//! file is the contract that they PARSE and VALIDATE against the live registry through
//! `RuleParams::parse` — the same gate a rule save runs — so a typo in a group, metric
//! or operator name fails at build time rather than silently never matching.
//!
//! It also pins the shape of each island: the entry terms that define it and the shared
//! `stop 3 / trail 20` exit, so a later edit that quietly drops a term is caught.

use hunter_engine::metrics::MetricId;
use hunter_engine::rule_params::RuleParams;

/// Island 1 — absorption. A minute of buyer-dominated tape on a live but unrun token.
const ISLAND_1: &str = r#"{
  "stop_loss": 3,
  "entry": {
    "m_snapshot": {
      "time": [{"operator": ">", "value": 5}],
      "liquidity": [{"operator": ">=", "value": 3}, {"operator": "<=", "value": 64}],
      "ix_count": [{"operator": "<=", "value": 5}]
    },
    "m_flow_lifetime": { "gross_flow": [{"operator": "<=", "value": 148}] },
    "m_flow_window": [
      { "window_size_sec": 60, "buy_share": [{"operator": ">", "value": 84}] },
      { "window_size_sec": 30, "unique_wallets": [{"operator": ">", "value": 6}] }
    ]
  },
  "exit": { "m_position": { "retrace": [{"operator": ">=", "value": 20}] } }
}"#;

/// Island 2 — quiet accumulation. Real net inflow while almost nobody is trading.
const ISLAND_2: &str = r#"{
  "stop_loss": 3,
  "entry": {
    "m_snapshot": {
      "time": [{"operator": ">", "value": 5}],
      "liquidity": [{"operator": ">=", "value": 3}, {"operator": "<=", "value": 64}],
      "ix_count": [{"operator": "<=", "value": 5}]
    },
    "m_flow_lifetime": { "gross_flow": [{"operator": "<=", "value": 148}] },
    "m_flow_window": [
      { "window_size_sec": 60, "buy_share": [{"operator": ">", "value": 75}] },
      {
        "window_size_sec": 30,
        "unique_wallets": [{"operator": "<=", "value": 6}],
        "net_flow": [{"operator": ">", "value": 6.5}]
      }
    ]
  },
  "exit": { "m_position": { "retrace": [{"operator": ">=", "value": 20}] } }
}"#;

/// Island 3 — impulse inception. A one-slot buy impulse, before price has moved.
const ISLAND_3: &str = r#"{
  "stop_loss": 3,
  "entry": {
    "m_snapshot": {
      "time": [{"operator": ">", "value": 5}],
      "liquidity": [{"operator": ">=", "value": 3}],
      "ix_count": [{"operator": "<=", "value": 5}]
    },
    "m_flow_window": [{ "window_size_sec": 0.4, "net_flow": [{"operator": ">=", "value": 0.5}] }],
    "m_price_window": [{ "window_size_sec": 3, "rise": [{"operator": "<=", "value": 9}] }]
  },
  "exit": { "m_position": { "retrace": [{"operator": ">=", "value": 20}] } }
}"#;

/// Islands 1 AND 3 — both readings agree. The highest-quality, most cap-proof rule.
const ISLAND_1_AND_3: &str = r#"{
  "stop_loss": 3,
  "entry": {
    "m_snapshot": {
      "time": [{"operator": ">", "value": 5}],
      "liquidity": [{"operator": ">=", "value": 3}, {"operator": "<=", "value": 64}],
      "ix_count": [{"operator": "<=", "value": 5}]
    },
    "m_flow_lifetime": { "gross_flow": [{"operator": "<=", "value": 148}] },
    "m_flow_window": [
      { "window_size_sec": 60, "buy_share": [{"operator": ">", "value": 84}] },
      { "window_size_sec": 30, "unique_wallets": [{"operator": ">", "value": 6}] },
      { "window_size_sec": 0.4, "net_flow": [{"operator": ">=", "value": 0.5}] }
    ],
    "m_price_window": [{ "window_size_sec": 3, "rise": [{"operator": "<=", "value": 9}] }]
  },
  "exit": { "m_position": { "retrace": [{"operator": ">=", "value": 20}] } }
}"#;

pub const ISLANDS: &[(&str, &str)] = &[
    ("island-1-absorption", ISLAND_1),
    ("island-2-quiet-accumulation", ISLAND_2),
    ("island-3-impulse-inception", ISLAND_3),
    ("island-1-and-3", ISLAND_1_AND_3),
];

fn parse(json: &str) -> RuleParams {
    let v: serde_json::Value = serde_json::from_str(json).expect("rule JSON is valid JSON");
    RuleParams::parse(&v).expect("rule parses against the live metric registry")
}

/// Is `id` conditioned on anywhere in this side (any group, any window)?
fn has(side: &hunter_engine::rule_params::SideConditions, id: MetricId) -> bool {
    side.0.values().flatten().any(|g| g.metrics.contains_key(&id))
}

/// Every island parses and validates. A group, metric or operator name that does not
/// exist in [`hunter_engine::metrics::REGISTRY`] fails the save in production;
/// `RuleParams::parse` is that same gate, so this catches it at build time instead.
#[test]
fn every_island_rule_parses_and_validates() {
    for (name, json) in ISLANDS {
        let p = parse(json);
        assert!(p.entry.is_some(), "{name}: entry conditions are the island");
        assert!(p.exit.is_some(), "{name}: needs the trailing exit");
        assert_eq!(p.stop_loss, Some(3.0), "{name}: the falsification stop is 3%");
        assert_eq!(
            p.take_profit, None,
            "{name}: a static take-profit caps the right tail the book is paid by"
        );
        let exit = p.exit.as_ref().expect("exit present");
        assert!(has(exit, MetricId::Retrace), "{name}: the wide trail IS the exit");
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

/// The two metrics this work added must actually be reachable from rule JSON.
///
/// `buy_share` is what makes islands 1 and 2 expressible at all, and `ix_count` is the
/// token filter that carries the edge across a one-slot fill delay. Without them these
/// rules could only parse by matching some *other* metric, so assert the exact ids.
#[test]
fn the_new_metrics_are_reachable_from_rule_json() {
    let p = parse(ISLAND_1);
    let entry = p.entry.expect("island 1 has entry conditions");
    assert!(has(&entry, MetricId::BuyShare), "buy_share reaches the rule");
    assert!(has(&entry, MetricId::IxCount), "ix_count reaches the rule");
    assert!(has(&entry, MetricId::UniqueWallets), "the trade-count stand-in reaches it");
    assert!(has(&entry, MetricId::LifeGrossFlow), "the not-yet-run gate reaches it");
}

/// Island 3 is the tier-0 rule: apart from the token filter it uses only metrics that
/// predate this work, so it can ship fingerprint-scoped on an engine without them.
#[test]
fn island_3_needs_no_buy_share() {
    let p = parse(ISLAND_3);
    let entry = p.entry.expect("island 3 has entry conditions");
    assert!(!has(&entry, MetricId::BuyShare), "island 3 is authorable without buy_share");
    assert!(has(&entry, MetricId::NetFlow), "the one-slot impulse is the trigger");
    assert!(has(&entry, MetricId::WinRise), "rise(3) conditions it on not-yet-moved");
}

/// Islands 1 and 3 are separate readings, and their conjunction must carry BOTH sets of
/// entry terms — a merge that silently dropped one side would look like a working rule.
#[test]
fn the_conjunction_carries_both_islands() {
    let p = parse(ISLAND_1_AND_3);
    let e = p.entry.expect("entry");
    for id in [MetricId::BuyShare, MetricId::UniqueWallets] {
        assert!(has(&e, id), "{id:?} comes from island 1");
    }
    for id in [MetricId::NetFlow, MetricId::WinRise] {
        assert!(has(&e, id), "{id:?} comes from island 3");
    }
}
