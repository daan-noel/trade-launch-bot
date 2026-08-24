//! The first-launch impulse rule, authored as the engine's own `strategy_rules.params`
//! JSON — the contract that it PARSES and VALIDATES through `RuleParams::parse`, the
//! same gate a rule save runs, so a typo in a group, metric or operator name fails at
//! build time rather than silently never matching.
//!
//! Derivation and every measured number:
//!   hunter/docs/history/2026-08-23-3xk2-derived-first-launch-rule.md
//! The seed that puts this exact JSON in the database:
//!   hunter/scripts/seed-first-launch-rule.sql
//!
//! Three terms, and each is negative on its own — the rule is the conjunction, not a
//! ranking of its parts. This file pins all three, because dropping any one is not a
//! weaker rule but a different and losing one.

use hunter_engine::metrics::evaluator::Operator;
use hunter_engine::metrics::MetricId;
use hunter_engine::rule_params::{RuleParams, SideConditions};

/// A first-time creator's token, up 150% in ten seconds, on one-sided flow.
///
/// `prior_launches = 0` is the token filter and carries the rule (dropping it costs
/// +0.0736 -> +0.0023/trade out of sample). `rise@10 >= 150` is the moment. `buy_share@30
/// >= 80` is the tape's direction — note the engine's PERCENT scale, so 80 and not 0.8.
///
/// `time > 5` and `liquidity >= 3` reproduce the study's decision-print filter: every
/// measured decision was on a token at least 5 s old with a real pool behind it.
///
/// The exit is a 30% trailing stop and NO stop-loss. The trail has a true interior
/// optimum there — 40% halves the expectancy, 50% crosses zero — and every stop-loss
/// tested costs money inside this gate.
const FIRST_LAUNCH: &str = r#"{
  "entry": {
    "m_snapshot": {
      "time": [{"operator": ">", "value": 5}],
      "liquidity": [{"operator": ">=", "value": 3}],
      "prior_launches": [{"operator": "=", "value": 0}]
    },
    "m_flow_window": [
      { "window_size_sec": 30, "buy_share": [{"operator": ">=", "value": 80}] }
    ],
    "m_price_window": [
      { "window_size_sec": 10, "rise": [{"operator": ">=", "value": 150}] }
    ]
  },
  "exit": { "m_position": { "retrace": [{"operator": ">=", "value": 30}] } }
}"#;

fn parse(json: &str) -> RuleParams {
    let v: serde_json::Value = serde_json::from_str(json).expect("valid JSON");
    RuleParams::parse(&v).unwrap_or_else(|e| panic!("rule failed the save gate: {e}"))
}

fn has(side: &SideConditions, id: MetricId) -> bool {
    side.0.values().flatten().any(|g| g.metrics.contains_key(&id))
}

fn terms(side: &SideConditions, id: MetricId) -> Vec<(Operator, f64)> {
    side.0
        .values()
        .flatten()
        .filter_map(|g| g.metrics.get(&id))
        .flat_map(|arms| arms.iter().flatten())
        .map(|c| (c.operator, c.value))
        .collect()
}

fn windows(side: &SideConditions) -> Vec<f64> {
    let mut w: Vec<f64> = side
        .0
        .values()
        .flatten()
        .filter(|g| !g.metrics.is_empty())
        .filter_map(|g| g.strict_param("window_size_sec"))
        .collect();
    w.sort_by(|a, b| a.partial_cmp(b).unwrap());
    w.dedup();
    w
}

#[test]
fn the_rule_parses_and_round_trips() {
    let once = parse(FIRST_LAUNCH);
    let twice = RuleParams::parse(&once.to_value()).expect("re-parse of its own output");
    assert_eq!(once.to_value(), twice.to_value(), "params must survive a save/load cycle");
}

/// All three terms, at the exact thresholds the measurement was taken at. A rule with
/// two of them is a different rule: `rise@10 >= 150` ALONE is -0.0049/trade, and
/// dropping the creator term takes the out-of-sample expectancy from +0.0736 to +0.0023.
#[test]
fn all_three_terms_are_present_at_their_measured_thresholds() {
    let p = parse(FIRST_LAUNCH);
    let entry = p.entry.as_ref().expect("entry side");

    assert_eq!(
        terms(entry, MetricId::PriorLaunches),
        vec![(Operator::Eq, 0.0)],
        "the creator term is the one that carries the rule"
    );
    assert_eq!(terms(entry, MetricId::BuyShare), vec![(Operator::Gte, 80.0)]);
    assert_eq!(terms(entry, MetricId::WinRise), vec![(Operator::Gte, 150.0)]);
}

/// `buy_share` is a PERCENT metric on the engine's 0-100 scale, so the threshold is 80.
/// The analysis carried it as a 0-1 ratio; authoring `0.8` here would be a gate every
/// token passes, which reads as a working rule that took every trade in the universe.
#[test]
fn buy_share_is_authored_on_the_engine_percent_scale() {
    let p = parse(FIRST_LAUNCH);
    let entry = p.entry.as_ref().unwrap();
    let (_, v) = terms(entry, MetricId::BuyShare)[0];
    assert!(v > 1.0, "buy_share {v} looks like a 0-1 ratio; the engine's scale is 0-100");
    assert!(v <= 100.0);
}

/// The two windows are 30 s (flow) and 10 s (price) — different groups, different
/// lookbacks. Both are well above one slot, so neither can be a same-slot artifact.
#[test]
fn the_windows_are_ten_and_thirty_seconds() {
    let p = parse(FIRST_LAUNCH);
    assert_eq!(windows(p.entry.as_ref().unwrap()), vec![10.0, 30.0]);
}

/// A 30% trailing stop and NO stop-loss. Inside this gate the exit is monotone in trail
/// width up to 30 and every stop-loss costs money, so a `stop_loss` appearing here would
/// be a silent downgrade of the tested rule.
#[test]
fn the_exit_is_a_wide_trail_with_no_stop_loss() {
    let p = parse(FIRST_LAUNCH);
    assert!(p.stop_loss.is_none(), "a stop-loss inside this gate is a measured loss");
    assert!(p.take_profit.is_none(), "the trail is the exit; a TP truncates the tail it lives on");
    let exit = p.exit.as_ref().expect("exit side");
    assert_eq!(terms(exit, MetricId::Retrace), vec![(Operator::Gte, 30.0)]);
    assert!(!has(exit, MetricId::Held), "the exit is a trail, not a clock");
}

/// `prior_launches` reads the creator ACROSS other tokens, so it is unavailable on the
/// lake-corpus paths. Pinned here because the rule is only meaningful where the metric
/// is really seeded — live, `simulate`, and the rule readout.
#[test]
fn the_creator_term_is_flagged_as_needing_token_history() {
    assert!(MetricId::PriorLaunches.needs_creator_history());
    assert!(!MetricId::BuyShare.needs_creator_history());
    assert!(!MetricId::WinRise.needs_creator_history());
}
