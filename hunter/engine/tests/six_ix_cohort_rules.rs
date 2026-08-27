//! The two 6ix-cohort rules, authored as the engine's own `strategy_rules.params` JSON.
//!
//! They are the rules derived in
//! `hunter/docs/plans/strategies/6ix-instant-crowd-launch.md` ("RE-FIT on exact engine
//! semantics"), and this file is the contract that they PARSE and VALIDATE against the
//! live registry through `RuleParams::parse` — the same gate a rule save runs — so a
//! typo in a group, metric or operator name fails at build time rather than silently
//! never matching.
//!
//! It also pins the terms, because each one was measured: drop-one-keep-the-rest is how
//! the rule was built, so a later edit that quietly loses a term changes what was
//! validated. And it pins the two **unit conversions** that a hand edit gets wrong:
//! `trade_share` is a PERCENT (the fitted `0.0769` is `7.69` here), and `liquidity` is
//! REAL reserves, `vsol - 30`, so the graduation exit at `vsol 105` is `75`.
//!
//! These are the same JSON documents inserted into `strategy_rules`; the DB rows and
//! this file are one text, so a change to either without the other fails here.

use hunter_engine::metrics::evaluator::Operator;
use hunter_engine::metrics::flow_burst::BURST_PARAM;
use hunter_engine::metrics::{MetricId, Windows};
use hunter_engine::rule_params::{RuleParams, SideConditions};

/// The winner — what the multi-start search returned, unmodified.
///
/// Shape: "high lifetime volume on comparatively FEW trades" (large average
/// participants), plus buying right now and no single outsized print.
///
/// 443 fires, 17.0/day: mean +4.56%, IS +4.39 / OOS +4.70, ex-top-5 +1.78%, 4/4 weeks
/// positive, 0 of 8 permutation shuffles beat it, and the latency ladder is FLAT
/// (115 ms +4.56 -> 2000 ms +3.73) — a signal, not a race.
const WINNER: &str = r#"{
  "entry": {
    "m_snapshot": { "time": [{"operator": ">=", "value": 60}] },
    "m_flow_window": [
      { "window_size_sec": 5, "buy": [{"operator": ">=", "value": 2.94}] },
      { "window_size_sec": 3, "buy": [{"operator": "<=", "value": 23.1}] }
    ],
    "m_flow_lifetime": {
      "trade_count": [{"operator": "<=", "value": 140}],
      "gross_flow":  [{"operator": ">=", "value": 43.6}]
    }
  },
  "exit": {
    "m_position": {
      "retrace": [{"operator": ">=", "value": 10}],
      "held":    [{"operator": ">=", "value": 60}]
    },
    "m_snapshot": { "liquidity": [{"operator": ">=", "value": 75}] }
  }
}"#;

/// The runner-up — stronger on money and on migration rate, but only 3 of 4 weeks
/// positive, and a post-hoc construction rather than what the search returned. Carried
/// as exploratory.
///
/// It is the cleaner statement of the six transferable questions: money going in now
/// (`net_flow(5)`), accelerating against its own pace (`trade_share`), a real launch
/// (`first_slot_buy`), and a crowd rather than one wallet churning (`trades_per_wallet`).
///
/// 17.2/day: mean +5.22%, OOS +5.59, ex-top-5 +2.33, migration 22.9% against a 6.26%
/// pool baseline, latency-flat.
const RUNNER_UP: &str = r#"{
  "entry": {
    "m_snapshot": {
      "time":           [{"operator": ">=", "value": 60}],
      "first_slot_buy": [{"operator": ">=", "value": 6.41}]
    },
    "m_flow_window": [
      { "window_size_sec": 5,  "net_flow":          [{"operator": ">=", "value": 0.79}] },
      { "window_size_sec": 10, "trades_per_wallet": [{"operator": "<=", "value": 2}] }
    ],
    "m_flow_burst": {
      "window_size_sec": 60,
      "burst_size_sec": 3,
      "trade_share": [{"operator": ">=", "value": 7.69}]
    }
  },
  "exit": {
    "m_position": {
      "retrace": [{"operator": ">=", "value": 10}],
      "held":    [{"operator": ">=", "value": 60}]
    },
    "m_snapshot": { "liquidity": [{"operator": ">=", "value": 75}] }
  }
}"#;

const RULES: &[(&str, &str)] = &[("winner", WINNER), ("runner-up", RUNNER_UP)];

fn parse(json: &str) -> RuleParams {
    let v: serde_json::Value = serde_json::from_str(json).expect("rule JSON is well formed");
    RuleParams::parse(&v).unwrap_or_else(|e| panic!("rule failed the save gate: {e}"))
}

/// Every authored `(operator, value)` on `id`, flattened out of the DNF, paired with
/// the window(s) its group instance was authored at.
fn terms(side: &SideConditions, id: MetricId) -> Vec<(Operator, f64, Windows)> {
    side.0
        .values()
        .flatten()
        .flat_map(|g| {
            let w = Windows {
                primary: g.window_spec(hunter_engine::metrics::WINDOW_SEC_PARAM, hunter_engine::metrics::WINDOW_SLOT_PARAM),
                secondary: g.window_spec(BURST_PARAM, hunter_engine::metrics::flow_burst::BURST_SLOT_PARAM),
            };
            g.metrics
                .get(&id)
                .into_iter()
                .flatten()
                .flatten()
                .map(move |c| (c.operator, c.value, w))
        })
        .collect()
}

fn one(side: &SideConditions, id: MetricId) -> (Operator, f64, Windows) {
    let t = terms(side, id);
    assert_eq!(t.len(), 1, "{id} should be authored exactly once, got {t:?}");
    t[0]
}

#[test]
fn both_rules_parse_and_validate() {
    for (name, json) in RULES {
        let p = parse(json);
        assert!(p.entry.is_some(), "{name}: the entry terms ARE the rule");
        assert!(p.exit.is_some(), "{name}: needs the three-way exit");
        // The book is a tail harvester (median negative, win rate ~41%), so a static
        // take-profit caps the right tail the whole result is paid by. The `liquidity`
        // exit is not a take-profit: it fires on arriving at graduation, not on a
        // percentage gain.
        assert_eq!(p.take_profit, None, "{name}: a TP inverts a tail harvester");
        assert_eq!(p.stop_loss, None, "{name}: the give-back trail IS the stop");
        // One fire per token: no `reentry` block means a closed position ends the
        // (token, rule) pair forever. The backtest counted one fire per token, so
        // re-entry here would double-count every runner.
        assert!(p.reentry.is_none(), "{name}: one fire per token, as measured");
    }
}

/// Each rule round-trips through the canonical JSONB shape, so what this file pins is
/// what a save writes and a load reads back.
#[test]
fn both_rules_round_trip() {
    for (name, json) in RULES {
        let once = parse(json);
        let twice = RuleParams::parse(&once.to_value())
            .unwrap_or_else(|e| panic!("{name}: re-parse of its own output failed: {e}"));
        assert_eq!(once, twice, "{name}: JSONB round trip is not lossless");
    }
}

/// The age floor is a measured term, not decoration. Below it every `m_flow_window(W)`
/// is clipped by the token's own age and all five windows return the SAME number — 100%
/// of a launch-fired set, 48% of the whole fire set. A multi-window rule fired younger
/// than its longest window is not the rule that was validated.
#[test]
fn both_rules_fire_past_the_window_degeneracy_age() {
    for (name, json) in RULES {
        let p = parse(json);
        let entry = p.entry.as_ref().expect("entry");
        let (op, floor, _) = one(entry, MetricId::Time);
        assert_eq!(op, Operator::Gte, "{name}: age is a FLOOR");
        assert_eq!(floor, 60.0, "{name}: the ladder rung the rule was fitted on");

        // Every window the rule reads must be populated at that floor.
        for g in entry.0.values().flatten() {
            for w in [g.strict_param("window_size_sec"), g.strict_param(BURST_PARAM)]
                .into_iter()
                .flatten()
            {
                assert!(w <= floor, "{name}: window {w}s is clipped at age {floor}s");
            }
        }
    }
}

/// The winner's four terms. Each was measured drop-one-keep-the-rest; the pair of
/// opposite-signed `buy` bounds at two windows is the shape, not a typo.
#[test]
fn the_winner_keeps_all_four_measured_terms() {
    let p = parse(WINNER);
    let entry = p.entry.as_ref().expect("entry");

    let buys = terms(entry, MetricId::Buy);
    assert_eq!(buys.len(), 2, "two windowed buy bounds: {buys:?}");
    assert!(buys.contains(&(Operator::Gte, 2.94, Windows::secs(5.0))), "buying NOW");
    assert!(
        buys.contains(&(Operator::Lte, 23.1, Windows::secs(3.0))),
        "and no single outsized print"
    );

    // "High lifetime volume on comparatively FEW trades" — the two halves are one
    // shape and neither states it alone.
    assert_eq!(one(entry, MetricId::LifeTradeCount), (Operator::Lte, 140.0, Windows::NONE));
    assert_eq!(one(entry, MetricId::LifeGrossFlow), (Operator::Gte, 43.6, Windows::NONE));
}

/// The runner-up's four terms, and the two unit conversions a hand edit gets wrong.
#[test]
fn the_runner_up_keeps_its_terms_in_the_units_it_was_fitted_in() {
    let p = parse(RUNNER_UP);
    let entry = p.entry.as_ref().expect("entry");

    assert_eq!(one(entry, MetricId::NetFlow), (Operator::Gte, 0.79, Windows::secs(5.0)));
    assert_eq!(
        one(entry, MetricId::TradesPerWallet),
        (Operator::Lte, 2.0, Windows::secs(10.0)),
        "a crowd, not one wallet churning — a COUNT ratio, never an identity"
    );
    assert_eq!(
        one(entry, MetricId::FirstSlotBuy),
        (Operator::Gte, 6.41, Windows::NONE),
        "a THRESHOLD, which a fingerprint bucket cannot express"
    );
    // The fitted `trade_count(3)/trade_count(60) >= 0.0769` is a FRACTION; the metric
    // unit is percent. Shipping 0.0769 here would gate on ~0.08%, i.e. on nothing.
    assert_eq!(
        one(entry, MetricId::BurstTradeShare),
        (Operator::Gte, 7.69, Windows::two(hunter_engine::metrics::WindowSpec::secs(60.0), hunter_engine::metrics::WindowSpec::secs(3.0))),
        "the fitted 0.0769 fraction is 7.69 percent"
    );
}

/// The exit is shared and answers the seventh question — what ends the trade: it gave
/// back, it ran out of time, or it arrived. All three are ORed (the exit combinator),
/// and persistent-state exits beat barriers because a barrier is adversely selected at
/// its own fill while a clock is not.
#[test]
fn the_shared_exit_is_give_back_or_clock_or_arrival() {
    for (name, json) in RULES {
        let p = parse(json);
        let exit = p.exit.as_ref().expect("exit");
        assert_eq!(one(exit, MetricId::Retrace), (Operator::Gte, 10.0, Windows::NONE));
        assert_eq!(one(exit, MetricId::Held), (Operator::Gte, 60.0, Windows::NONE));
        // `liquidity` is REAL reserves = `vsol - PUMP_INITIAL_VIRTUAL_SOL` (30), and the
        // curve graduation wall is vsol 115.005 — measured, not assumed. So 75 here is
        // vsol 105, 87% of the way to the wall: leave as it arrives, not after.
        assert_eq!(
            one(exit, MetricId::Liquidity),
            (Operator::Gte, 75.0, Windows::NONE),
            "{name}: 75 real reserves == vsol 105 == 87% of the way to graduation"
        );
        // No `arm_above_pct`: the peak seeds at the entry fill, so the unarmed trail
        // doubles as a hard stop from entry. That is what was graded.
        assert!(
            exit.0.values().flatten().all(|g| g.strict_param("arm_above_pct").is_none()),
            "{name}: the trail was graded unarmed",
        );
    }
}
