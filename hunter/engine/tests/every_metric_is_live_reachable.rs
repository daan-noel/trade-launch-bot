//! Registry-driven guard: **every** metric in [`REGISTRY`] produces a real reading
//! on the LIVE path.
//!
//! Adding a metric touches two files - the registry entry and its group's compute
//! arm - and only the first is compile-enforced. `TokenTrack::value` routes by
//! group, but each group's own `value(id)` ends in `_ => f64::NAN`, so a registered
//! metric whose compute arm was never written reads `NaN` forever. `NaN` satisfies
//! no condition (evaluator contract), so the rule simply never fires: no panic, no
//! log line, no failing test - a gate that is silently always-false.
//!
//! This test drives each metric through the same surface hunter-live uses
//! (`EngineState` + `reduce` + `readout::read_state`) over a stream rich enough to
//! define every one of them, and asserts the readout value is finite. It walks
//! `REGISTRY` rather than a list, so a metric added tomorrow is covered without
//! touching this file.

use std::sync::Arc;

use chrono::{Duration, TimeZone, Utc};
use hunter_engine::event::{Effect, Event, Fill, LoadedRule, Mint, RuleId, TradeMode};
use hunter_engine::fingerprint::{Fingerprint, FingerprintId};
use hunter_engine::grouping::TokenFingerprint;
use hunter_engine::metrics::{
    flow_burst, flow_split, GroupSpec, MetricKind, MetricScope, MetricSpec, Side, TradeLite, Ts,
    REGISTRY,
};
use hunter_engine::readout::read_state;
use hunter_engine::reduce::reduce;
use hunter_engine::rule_params::RuleParams;
use hunter_engine::EngineState;
use serde_json::{json, Map, Value};
use uuid::Uuid;

const RULE: u128 = 1;
const FP: u128 = 2;

fn ts(secs: f64) -> Ts {
    Utc.timestamp_opt(1_700_000_000, 0).unwrap() + Duration::milliseconds((secs * 1000.0) as i64)
}

/// The volume-side label sequence the probe's "volume" trades carry.
const VOL_LABELS: [&str; 1] = ["Pump.Fun: Buy"];

/// A wildcard fingerprint configured for `m_flow_split`, so the flow-split groups
/// have a classifier and do not read `NaN` for want of config.
fn probe_fp() -> Fingerprint {
    Fingerprint {
        wildcard: true,
        id: FingerprintId(Uuid::from_u128(FP)),
        cu_limit: None,
        cu_price: None,
        ix_labels: None,
        init_buy_lamports: None,
        max_cost_lamports: None,
        spendable_lamports_in: None,
        first_slot_buy_lamports: None,
        first_slot_sell_lamports: None,
        bucket_size_amount: Some(0.1),
        metric_config: json!({
            "m_flow_split": { "volume_ix_patterns": [VOL_LABELS] }
        }),
    }
}

/// Which clock a dynamic group's window is authored on. Time is continuous while
/// slots and prints are discrete, so these are three window implementations, not one
/// with a unit label - a metric can read on one and be `NaN` on another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Basis {
    Sec,
    Slot,
    Print,
}

impl Basis {
    fn label(self) -> &'static str {
        match self {
            Basis::Sec => "seconds",
            Basis::Slot => "slots",
            Basis::Print => "prints",
        }
    }
}

/// Fill in this group's strict params with values that define a reading on `basis`.
///
/// Driven off the registry's own param names: an unrecognised **required** param is
/// a hard failure telling the author to teach the probe, never a silent skip that
/// would let a new group's metrics go unchecked.
fn strict_params(g: &GroupSpec, basis: Basis, obj: &mut Map<String, Value>) {
    for p in g.strict_params {
        let v = match (p.name, basis) {
            (hunter_engine::metrics::WINDOW_SEC_PARAM, Basis::Sec) => json!(10.0),
            (flow_burst::BURST_PARAM, Basis::Sec) => json!(2.0),
            // The probe's trades span slots 100..110, so a 20-slot window covers them
            // and a 4-slot burst nests inside it.
            (hunter_engine::metrics::WINDOW_SLOT_PARAM, Basis::Slot) => json!(20.0),
            (flow_burst::BURST_SLOT_PARAM, Basis::Slot) => json!(4.0),
            // Wide enough to hold every print the probe folds, with a burst nested
            // inside it, so an empty window is never what a NaN would be blamed on.
            (hunter_engine::metrics::WINDOW_PRINT_PARAM, Basis::Print) => json!(50.0),
            (flow_burst::BURST_PRINT_PARAM, Basis::Print) => json!(4.0),
            // The other bases' size params, plus two optional knobs deliberately left
            // unset: a lag would push the window off the probe's trades, and
            // `arm_above_pct` would report the trailing metrics as `disarmed` rather
            // than read them.
            (
                hunter_engine::metrics::WINDOW_SEC_PARAM
                | hunter_engine::metrics::WINDOW_SLOT_PARAM
                | hunter_engine::metrics::WINDOW_PRINT_PARAM
                | hunter_engine::metrics::WINDOW_LAG_PARAM
                | flow_burst::BURST_PARAM
                | flow_burst::BURST_SLOT_PARAM
                | flow_burst::BURST_PRINT_PARAM
                | "arm_above_pct",
                _,
            ) => continue,
            (other, _) => {
                assert!(
                    !p.required,
                    "{}: required strict param `{other}` is unknown to this probe - \
                     add a value for it here so the group's metrics stay covered",
                    g.name
                );
                continue;
            }
        };
        obj.insert(p.name.to_string(), v);
    }
}

/// Rule params placing `metric` on the side its scope allows, with a permissive
/// condition. The condition is never the point - the reading is.
fn params_for(g: &GroupSpec, m: &MetricSpec, basis: Basis) -> Value {
    let mut group_obj = Map::new();
    strict_params(g, basis, &mut group_obj);
    group_obj.insert(m.name.to_string(), json!([{ "operator": ">=", "value": -1.0e9 }]));
    let side = if g.scope == MetricScope::Position { "exit" } else { "entry" };
    json!({ side: { g.name: Value::Object(group_obj) } })
}

fn loaded_rule(params: Value, g: &GroupSpec, m: &MetricSpec) -> LoadedRule {
    LoadedRule {
        id: RuleId(Uuid::from_u128(RULE)),
        fingerprint_id: FingerprintId(Uuid::from_u128(FP)),
        trade_mode: TradeMode::Paper,
        buy_amount_lamports: 100_000_000,
        max_concurrent_tokens: 1,
        max_total_tokens: 0,
        params: RuleParams::parse(&params)
            .unwrap_or_else(|e| panic!("{}.{} does not validate as a rule: {e}", g.name, m.name)),
        entry_enabled: true,
    }
}

/// One trade. `vol` picks the volume-side label sequence (so `m_flow_split` books it
/// as volume); the wallet varies so `unique_wallets` / `trades_per_wallet` are real.
#[allow(clippy::too_many_arguments)]
fn trade(
    side: Side,
    sol: f64,
    price: f64,
    reserve: f64,
    at: f64,
    slot: u64,
    vol: bool,
    wallet: u64,
) -> TradeLite {
    TradeLite {
        side,
        sol,
        price,
        reserve_sol: reserve,
        priced_reserve_sol: reserve + 30.0,
        at: ts(at),
        ix_hash: Some(if vol {
            flow_split::ix_hash(&VOL_LABELS)
        } else {
            flow_split::ix_hash(&["Pump.Fun: Sell"])
        }),
        wallet_hash: wallet,
        slot,
        marker_bits: 0,
    }
}

/// An event stream rich enough to define every registered metric: a creation with
/// labels and a known creator, a settled first slot, then buys and sells from
/// several wallets, on both sides of the classifier, across slots, with the price
/// rising and falling so every extremum is non-trivial.
fn drive(state: &mut EngineState, mint: &Mint) -> Vec<Effect> {
    let fp = Box::new(TokenFingerprint {
        cu_limit: Some(200_000),
        ix_labels: vec!["Pump.Fun: Create".into(), "Pump.Fun: Buy".into()],
        ..Default::default()
    });
    let mut fx = reduce(
        state,
        Event::TokenCreated {
            mint: mint.clone(),
            fp,
            at: ts(0.0),
            // Seeds `prior_launches`; without a creator it stays NaN by design.
            creator_wallet_hash: Some(flow_split::wallet_hash("creator-wallet")),
            identity: None,
        },
    );
    // Seeds `first_slot_buy`, which does not exist until the creation slot closes.
    fx.extend(reduce(
        state,
        Event::FirstSlotSettled {
            mint: mint.clone(),
            buy_lamports: 500_000_000,
            sell_lamports: 100_000_000,
            at: ts(0.4),
        },
    ));
    let script = [
        (Side::Buy, 1.0, 1.0, 40.0, 1.0, 100u64, true, 11u64),
        (Side::Buy, 2.0, 1.4, 42.0, 2.0, 102, false, 12),
        (Side::Sell, 0.5, 1.2, 41.5, 3.0, 104, false, 13),
        (Side::Buy, 1.5, 1.8, 43.0, 4.0, 106, true, 12),
        (Side::Sell, 0.8, 1.5, 42.2, 5.0, 108, false, 14),
    ];
    for (side, sol, price, reserve, at, slot, vol, wallet) in script {
        fx.extend(reduce(
            state,
            Event::Trade {
                mint: mint.clone(),
                trade: trade(side, sol, price, reserve, at, slot, vol, wallet),
            },
        ));
    }
    fx.to_vec()
}

/// Confirm the entry fill if the rule submitted one, so position-scoped metrics have
/// a position to anchor on.
///
/// The buy can be submitted by ANY event in the stream - a rule whose entry side is
/// empty (which is every `m_position` probe, since that group is exit-only) enters on
/// arm, at `TokenCreated`, not at the tick. Searching only the tick's effects finds
/// nothing and leaves the position metrics reading `NaN` for want of a position,
/// which looks exactly like the compute-arm defect this test hunts.
fn confirm_entry(state: &mut EngineState, mint: &Mint, mut fx: Vec<Effect>) {
    fx.extend(reduce(state, Event::Tick { now: ts(5.5) }));
    let intent = fx.iter().find_map(|e| match e {
        Effect::SubmitBuy { intent, .. } => Some(intent.clone()),
        _ => None,
    });
    if let Some(intent) = intent {
        reduce(
            state,
            Event::FillConfirmed {
                intent,
                fill: Fill { price: 1.5, sol: 0.1, token_amount: 1_000_000, at: ts(5.6) },
            },
        );
        // A trade after the fill moves the position's peak/trough off the entry, so
        // `retrace` / `bounce` read a real excursion rather than a seeded zero.
        reduce(
            state,
            Event::Trade {
                mint: mint.clone(),
                trade: trade(Side::Buy, 1.0, 2.0, 44.0, 6.0, 110, false, 15),
            },
        );
    }
}

/// Read `metric` off the live engine after driving the probe stream, with the
/// metric's group authored on `basis`. Returns the reading itself, finite or not.
fn live_reading(g: &GroupSpec, m: &MetricSpec, basis: Basis) -> f64 {
    let mint = Mint(format!("probe-{}-{}-{}", g.name, m.name, basis.label()).into());
    let rule = loaded_rule(params_for(g, m, basis), g, m);

    let mut state = EngineState::default();
    reduce(
        &mut state,
        Event::RulesReloaded {
            rules: Arc::from(vec![rule]),
            fps: Arc::from(vec![probe_fp()]),
        },
    );
    let fx = drive(&mut state, &mint);
    confirm_entry(&mut state, &mint, fx);

    let out = read_state(&state, &mint, RuleId(Uuid::from_u128(RULE)), ts(7.0)).unwrap_or_else(
        || panic!("{}.{} ({}): the live engine has no arm to read", g.name, m.name, basis.label()),
    );
    out.reads
        .iter()
        .find(|r| r.metric == m.id)
        .unwrap_or_else(|| {
            panic!("{}.{} ({}): compiled away - no req reads it", g.name, m.name, basis.label())
        })
        .value
}

#[test]
fn every_registered_metric_reads_a_real_value_on_the_live_path() {
    let mut unreadable: Vec<String> = Vec::new();
    let mut checked = 0usize;

    for g in REGISTRY {
        for m in g.metrics {
            checked += 1;
            let v = live_reading(g, m, Basis::Sec);
            if !v.is_finite() {
                unreadable.push(format!("{}.{} reads {v}", g.name, m.name));
            }
        }
    }

    assert!(
        unreadable.is_empty(),
        "{} of {checked} registered metrics never produce a value on the live path \
         (a NaN gate is silently always-false, so no rule using one can ever fire).\n  {}",
        unreadable.len(),
        unreadable.join("\n  "),
    );
    assert!(checked >= 40, "probe covered only {checked} metrics - registry walk is broken");
}

/// The slot twin of the test above. A dynamic group's two window bases are two
/// implementations, not one with a unit label: the slot path counts in a discrete
/// cursor advanced off `TradeLite::slot`, so a group can read perfectly on seconds
/// and `NaN` on slots (a window never registered, a `now_pos` never advanced). Every
/// dynamic metric is authorable on either, so both have to be reachable.
#[test]
fn every_dynamic_metric_also_reads_on_a_slot_window() {
    let mut unreadable: Vec<String> = Vec::new();
    let mut checked = 0usize;

    for g in REGISTRY {
        if g.kind != MetricKind::Dynamic {
            continue;
        }
        for m in g.metrics {
            checked += 1;
            let v = live_reading(g, m, Basis::Slot);
            if !v.is_finite() {
                unreadable.push(format!("{}.{} reads {v} on a slot window", g.name, m.name));
            }
        }
    }

    assert!(
        unreadable.is_empty(),
        "{} of {checked} dynamic metrics are readable on seconds but not on slots.\n  {}",
        unreadable.len(),
        unreadable.join("\n  "),
    );
    assert!(checked > 0, "no dynamic groups walked - registry walk is broken");
}

/// The print twin of the two tests above. A print window's cursor is a fold counter
/// the engine keeps itself rather than a field the feed supplies, so it fails in its
/// own way: a counter never bumped, or bumped after the fold, leaves every reading
/// empty while seconds and slots stay perfect. Every dynamic metric is authorable on
/// this basis, so it has to be reachable on it.
#[test]
fn every_dynamic_metric_also_reads_on_a_print_window() {
    let mut unreadable: Vec<String> = Vec::new();
    let mut checked = 0usize;

    for g in REGISTRY {
        if g.kind != MetricKind::Dynamic {
            continue;
        }
        for m in g.metrics {
            checked += 1;
            let v = live_reading(g, m, Basis::Print);
            if !v.is_finite() {
                unreadable.push(format!("{}.{} reads {v} on a print window", g.name, m.name));
            }
        }
    }

    assert!(
        unreadable.is_empty(),
        "{} of {checked} dynamic metrics are readable on seconds but not on prints.\n  {}",
        unreadable.len(),
        unreadable.join("\n  "),
    );
    assert!(checked > 0, "no dynamic groups walked - registry walk is broken");
}
