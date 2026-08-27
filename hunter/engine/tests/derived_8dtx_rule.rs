//! The 8dtx-derived rule, authored in the engine's own vocabulary.
//!
//! The derivation is in `docs/plans/strategies/wallet-8dtx-derived-rule.md`; that it
//! reproduces the SQL fire set on the real tape is
//! `hunter/lab/tests/slot_window_parity.rs`. This file pins the AUTHORING: the exact
//! JSON, that it parses, that it compiles to the spans intended, and that nothing in
//! it depends on a wall clock.
//!
//! Re-expressing a slot-native derivation in seconds is the failure mode these
//! assertions exist to catch - a one-second window is 2.5 slots, and an unlagged
//! quiet gate reads the very burst it is supposed to precede.

use hunter_engine::metrics::flow_split::{marker_mask, FlowPatterns, MARKERS, ROUTER_MARKERS};
use hunter_engine::metrics::{MetricGroupId, WindowUnit};
use hunter_engine::rule_params::RuleParams;
use serde_json::json;

/// The rule. Quiet, then a clean burst, on a token old enough to have gone quiet.
fn params() -> serde_json::Value {
    json!({
        "entry": {
            // The burst: the CURRENT slot alone, read as its transactions arrive.
            "m_flow_window": [
                {
                    "window_size_slots": 1.0,
                    "buy_count": [{"operator": ">=", "value": 2.0}]
                },
                // The tape BEFORE it. `window_lag: 1` is what makes this causal: the
                // burst slot is outside the span, so the gate cannot read the event
                // it is gating on.
                {
                    "window_size_slots": 30.0,
                    "window_lag": 1.0,
                    "buy": [{"operator": "<=", "value": 3.0}]
                }
            ],
            "m_flow_split_window": {
                "window_size_slots": 1.0,
                // Router flow in the burst.
                "nonvol_buy": [{"operator": ">=", "value": 1.5}],
                // And NOTHING ELSE in it. Not `== 0`: the running sums correct at the
                // window ends, so an emptied side lands on dust. A floor an order of
                // magnitude under any real transaction says the same thing without
                // resting on float equality.
                "vol_buy": [{"operator": "<=", "value": 0.01}]
            },
            // Old enough to have gone quiet on its own - 75 slots at ~400ms.
            "m_snapshot": {
                "time": [{"operator": ">=", "value": 30.0}],
                // vsol <= 42; `liquidity` is the REAL reserve, vsol - 30.
                "liquidity": [{"operator": "<=", "value": 12.0}]
            }
        },
        "exit": {
            // No stop, no take-profit, no trail: every one of them is monotonically
            // harmful on this trade, because 2.3% of fires carry 56% of the profit.
            "m_position": { "held": [{"operator": ">=", "value": 8.0}] }
        }
    })
}

/// The classifier this rule needs: structural, and nothing else.
///
/// The mask names the ORGANIC side. That is the whole gate: the derivation's
/// cleanliness term is "every buy in the burst came through a named retail router",
/// which judges everything without one as machine. The tempting inverse - mask the
/// machinery marker `CreateAccountWithSeed` as volume - identifies machines and
/// leaves everything else unjudged, and it is a materially looser rule: on the same
/// fires it reads +0.99 per trade against +6.86, because the 8,566 fires it admits
/// and router-purity rejects average -0.68.
fn metric_config() -> serde_json::Value {
    json!({
        "m_flow_split": {
            "organic_ix_markers":
                ["Axiom Trade", "Photon", "Bloom Router", "Trojan Trade", "Terminal"],
            // Both OFF. Contagion makes "did this transaction come through a router"
            // a property of the sender's history instead of the transaction, and the
            // creator rule adds an identity term. Either one measures something the
            // derivation did not.
            "wallet_contagion": false,
            "creator_is_volume": false
        }
    })
}

#[test]
fn the_rule_parses_and_round_trips() {
    let p = RuleParams::parse(&params()).expect("parses");
    assert_eq!(p.to_value(), params(), "authored JSON survives a round trip");
    FlowPatterns::validate_metric_config(&metric_config()).expect("classifier config is valid");
}

#[test]
fn every_window_counts_in_slots_and_the_quiet_span_excludes_the_burst() {
    let p = RuleParams::parse(&params()).expect("parses");
    let entry = p.entry.as_ref().expect("entry side");

    let mut spans = Vec::new();
    for gid in [MetricGroupId::FlowWindow, MetricGroupId::FlowSplitWindow] {
        for g in &entry.0[&gid] {
            let spec = g
                .window_spec(&hunter_engine::metrics::WINDOW_AXIS)
                .expect("a dynamic group carries a span");
            assert_eq!(spec.unit, WindowUnit::Slot, "a wall clock cannot group a bundle");
            spans.push(spec);
        }
    }

    let burst: Vec<_> = spans.iter().filter(|s| s.size == 1.0).collect();
    assert_eq!(burst.len(), 2, "both burst reads share one span");
    assert_eq!(burst[0].bounds(500), (500, 500), "the current slot, alone");

    let quiet = spans.iter().find(|s| s.size == 30.0).expect("the quiet span");
    assert_eq!(quiet.lag, 1.0);
    assert_eq!(
        quiet.bounds(500),
        (470, 499),
        "30 slots ending one BEFORE the burst - it cannot see slot 500"
    );
    // The two spans are different buffers, which is what keeps them independent.
    assert_ne!(burst[0].key(), quiet.key());
}

/// The marker set is the whole ix gate, so a typo in it must not degrade quietly
/// into "match nothing" - under an ORGANIC mask that would classify every trade as
/// machine and the rule would silently never fire; under a volume mask it would
/// classify every trade as human and the cleanliness gate would pass on bot traffic.
#[test]
fn the_markers_are_known_and_a_typo_is_rejected() {
    let cfg = metric_config();
    let names: Vec<&str> = cfg["m_flow_split"]["organic_ix_markers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    for n in &names {
        assert!(MARKERS.iter().any(|(m, _)| m == n), "unknown marker {n}");
    }
    assert_eq!(marker_mask(&names).unwrap(), ROUTER_MARKERS, "the rule masks every router");

    let typo = json!({"m_flow_split": {"organic_ix_markers": ["Bloom Rooter"]}});
    let e = FlowPatterns::validate_metric_config(&typo).unwrap_err();
    assert!(e.contains("unknown ix marker"), "{e}");
}

/// A mask names ONE side of the split. Both at once is two contradictory classifiers
/// on one axis, and picking a winner silently is how a rule stops measuring what it
/// says - which is exactly the failure this rule was shipped with once.
#[test]
fn naming_both_sides_of_the_split_is_rejected() {
    for cfg in [
        json!({"m_flow_split": {
            "organic_ix_markers": ["Photon"],
            "volume_ix_markers": ["CreateAccountWithSeed"]
        }}),
        json!({"m_flow_split": {
            "organic_ix_markers": ["Photon"],
            "volume_ix_patterns": [["Pump.Fun: Buy"]]
        }}),
    ] {
        let e = FlowPatterns::validate_metric_config(&cfg).unwrap_err();
        assert!(e.contains("exactly one"), "{e}");
    }
}

/// The classifier the rule declares is a pure function of the transaction: the same
/// trade classifies the same way whoever sent it and whatever they did before.
#[test]
fn the_declared_classifier_is_structural() {
    let p = FlowPatterns::from_metric_config(&metric_config()).expect("configured");
    assert!(!p.wallet_contagion());
    assert!(!p.creator_is_volume());
    assert!(p.markers_are_organic());
}

/// What the gate actually decides, trade by trade. `marks` means VOLUME - so under
/// this mask a router build is organic and everything else is volume, INCLUDING a
/// build carrying no marker at all.
#[test]
fn only_a_router_build_counts_as_a_person() {
    let p = FlowPatterns::from_metric_config(&metric_config()).expect("configured");

    for router in ["Axiom Trade", "Photon", "Bloom Router", "Trojan Trade", "Terminal"] {
        let bits = marker_mask(&[router]).unwrap();
        assert!(!p.marks(bits), "{router} is a person clicking a button");
    }
    // A router build that ALSO carries machinery is still a router build: the mask
    // asks whether a person is behind it, not whether the build is plain.
    let messy = marker_mask(&["Photon", "AdvanceNonceAccount"]).unwrap();
    assert!(!p.marks(messy));

    // Machinery alone, and - the case a volume-side mask gets wrong - an unlabelled
    // or entirely unknown build. Both are machine flow here.
    assert!(p.marks(marker_mask(&["CreateAccountWithSeed"]).unwrap()));
    assert!(p.marks(marker_mask(&["Memo Program"]).unwrap()));
    assert!(p.marks(0), "no marker at all is not evidence of a person");
}

/// The two masks are different rules, not two spellings of one, and the engine must
/// not let them look alike: the pair that decides this rule's edge is a build with no
/// marker at all - machine under the organic mask, human under the volume mask.
#[test]
fn the_organic_mask_is_not_the_volume_mask_inverted() {
    let organic = FlowPatterns::from_metric_config(&metric_config()).expect("configured");
    let volume = FlowPatterns::from_metric_config(&json!({
        "m_flow_split": {
            "volume_ix_markers": ["CreateAccountWithSeed"],
            "wallet_contagion": false,
            "creator_is_volume": false
        }
    }))
    .expect("configured");

    let unmarked = 0u16;
    assert!(organic.marks(unmarked));
    assert!(!volume.marks(unmarked));
}
