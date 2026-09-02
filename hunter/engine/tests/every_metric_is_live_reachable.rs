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
use hunter_engine::fingerprint::{Criteria, Fingerprint, FingerprintId};
use hunter_engine::grouping::TokenFingerprint;
use hunter_engine::metrics::{
    flow_ix, flow_slice, is_two_window, GroupSpec, MetricKind, MetricScope, MetricSpec, Side,
    TradeLite, Ts, REGISTRY,
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

/// The sequence every NON-volume trade carries, and therefore the build
/// `m_dump_ix` counts the sells of. Disjoint from [`VOL_LABELS`] so this probe reads
/// each group's own state: the two lists MAY overlap, and a shared build would leave
/// a flow bug looking like a passing dump assertion.
const NONVOL_LABELS: [&str; 1] = ["Pump.Fun: Sell"];

/// The probe's wallets, as ADDRESSES. `m_copy` matches on the same `wallet_hash`
/// digest every adapter puts on a print, so the list has to be written the way a
/// rule author writes it - base58 in, hash out - rather than as bare `u64`s.
const WALLETS: [&str; 5] = ["w-eleven", "w-twelve", "w-thirteen", "w-fourteen", "w-fifteen"];

/// The two the copy list names. `w-twelve` buys twice in the script and
/// `w-thirteen` sells, so all four copy metrics get a non-zero reading; a
/// single-wallet list would leave one side at `0`, which is a reading but not a
/// demonstration.
const COPY_TARGETS: [&str; 2] = [WALLETS[1], WALLETS[2]];

/// A wildcard fingerprint configured for every fingerprint-scoped group, so none
/// reads `NaN` for want of config. Each has its own list: `m_flow_ix` tags the
/// volume side, `m_dump_ix` counts sells built the other way, `m_copy` names two of
/// the probe's wallets.
fn probe_fp() -> Fingerprint {
    Fingerprint {
        id: FingerprintId(Uuid::from_u128(FP)),
        wildcard: true,
        criteria: Criteria::new(),
        metric_config: json!({
            "m_flow_ix": { "ix_patterns": [VOL_LABELS] },
            "m_dump_ix": { "ix_patterns": [NONVOL_LABELS] },
            "m_burst_slot": { "working_templates": ["Pump.Fun"] },
            "m_copy": { "target_wallets": COPY_TARGETS }
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
fn strict_params(g: &GroupSpec, m: &MetricSpec, basis: Basis, obj: &mut Map<String, Value>) {
    // The slice axis is declared by `m_flow_window` for every instance but read only
    // by the metrics `is_two_window` names. Setting it for the others is rejected at
    // save as a no-op, so the probe follows the same per-metric rule the engine does -
    // which is what makes this test cover the contract rather than work around it.
    let reads_slice = is_two_window(m.id);
    for p in g.strict_params {
        if !reads_slice && flow_slice::SLICE_AXIS.params().contains(&p.name) {
            continue;
        }
        let v = match (p.name, basis) {
            (hunter_engine::metrics::WINDOW_SEC_PARAM, Basis::Sec) => json!(10.0),
            (flow_slice::SLICE_PARAM, Basis::Sec) => json!(2.0),
            // The probe's trades span slots 100..110, so a 20-slot window covers them
            // and a 4-slot burst nests inside it.
            (hunter_engine::metrics::WINDOW_SLOT_PARAM, Basis::Slot) => json!(20.0),
            (flow_slice::SLICE_SLOT_PARAM, Basis::Slot) => json!(4.0),
            // Wide enough to hold every print the probe folds, with a burst nested
            // inside it, so an empty window is never what a NaN would be blamed on.
            (hunter_engine::metrics::WINDOW_PRINT_PARAM, Basis::Print) => json!(50.0),
            (flow_slice::SLICE_PRINT_PARAM, Basis::Print) => json!(4.0),
            // The other bases' size params, plus two optional knobs deliberately left
            // unset: a lag would push the window off the probe's trades, and
            // `arm_above_pct` would report the trailing metrics as `disarmed` rather
            // than read them.
            (
                hunter_engine::metrics::WINDOW_SEC_PARAM
                | hunter_engine::metrics::WINDOW_SLOT_PARAM
                | hunter_engine::metrics::WINDOW_PRINT_PARAM
                | hunter_engine::metrics::WINDOW_LAG_PARAM
                | flow_slice::SLICE_PARAM
                | flow_slice::SLICE_SLOT_PARAM
                | flow_slice::SLICE_PRINT_PARAM
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
    strict_params(g, m, basis, &mut group_obj);
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

/// One trade. `vol` picks the volume-side label sequence (so `m_flow_ix` books it
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
    wallet: &str,
) -> TradeLite {
    TradeLite {
        side,
        sol,
        price,
        reserve_sol: reserve,
        priced_reserve_sol: reserve + 30.0,
        at: ts(at),
        ix_hash: Some(if vol {
            flow_ix::ix_hash(&VOL_LABELS)
        } else {
            flow_ix::ix_hash(&NONVOL_LABELS)
        }),
        wallet_hash: flow_ix::wallet_hash(wallet),
        slot,
        marker_bits: 0,
        leg_index: 0,
        tx_index: Some(slot as u32),
        template_hash: Some(if vol {
            hunter_engine::metrics::template_grain::grain_hash(&VOL_LABELS).unwrap()
        } else {
            hunter_engine::metrics::template_grain::grain_hash(&NONVOL_LABELS).unwrap()
        }),
        ..Default::default()
    }
}

/// An event stream rich enough to define every registered metric: a creation with
/// labels and a known creator, a settled first slot, then buys and sells from
/// several wallets, on both sides of the classifier, across slots, with the price
/// rising and falling so every extremum is non-trivial.
fn drive(state: &mut EngineState, mint: &Mint) -> Vec<Effect> {
    let fp = Box::new(TokenFingerprint {
        cu_limit: Some(200_000),
        ..Default::default()
    });
    let mut fx = reduce(
        state,
        Event::TokenCreated {
            mint: mint.clone(),
            fp,
            at: ts(0.0),
            // Seeds `prior_launches`; without a creator it stays NaN by design.
            creator_wallet_hash: Some(flow_ix::wallet_hash("creator-wallet")),
            identity: None,
            creation_slot: None,
        },
    );
    // Settles the deferred fingerprint axes, which do not exist until the creation
    // slot closes.
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
        (Side::Buy, 1.0, 1.0, 40.0, 1.0, 100u64, true, WALLETS[0]),
        (Side::Buy, 2.0, 1.4, 42.0, 2.0, 102, false, WALLETS[1]),
        (Side::Sell, 0.5, 1.2, 41.5, 3.0, 104, false, WALLETS[2]),
        (Side::Buy, 1.5, 1.8, 43.0, 4.0, 106, true, WALLETS[1]),
        (Side::Sell, 0.8, 1.5, 42.2, 5.0, 108, false, WALLETS[3]),
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
                trade: trade(Side::Buy, 1.0, 2.0, 44.0, 6.0, 110, false, WALLETS[4]),
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
