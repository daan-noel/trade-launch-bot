//! Registry-driven guard: **every** metric, on every tag and span it accepts, produces a
//! real reading on the LIVE path.
//!
//! Adding a metric touches two places - the registry entry and its compute arm - and
//! only the first is compile-enforced. Each compute module's `value` ends in
//! `_ => f64::NAN`, so a registered metric whose arm was never written reads `NaN`
//! forever. `NaN` satisfies no condition, so the rule simply never fires: no panic, no
//! log line, no failing test - a gate that is silently always-false.
//!
//! This test drives each (metric, tag, span) through the same surface hunter-live uses
//! (`EngineState` + `reduce` + `readout::read_state`) over a stream rich enough to
//! define every one of them, and asserts the readout value is finite. It walks the
//! registry rather than a list, so a metric, a tag level or a span kind added tomorrow
//! is covered without touching this file.

use std::sync::Arc;

use chrono::{Duration, TimeZone, Utc};
use hunter_engine::event::{Effect, Event, Fill, LoadedRule, Mint, RuleId, TradeMode};
use hunter_engine::fingerprint::{Criteria, Fingerprint, FingerprintId};
use hunter_engine::grouping::TokenFingerprint;
use hunter_engine::metrics::registry::{MetricSpec, TagLevel, TagUse, METRICS};
use hunter_engine::metrics::{trade_keys, Side, TradeLite, Ts};
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

/// The label sequence the probe's `volume` trades carry.
const VOL_LABELS: [&str; 1] = ["Pump.Fun: Buy"];

/// The sequence every other trade carries, and so the shape the `dump` tag lists.
/// Disjoint from [`VOL_LABELS`] so each tag reads its own state.
const NONVOL_LABELS: [&str; 1] = ["Pump.Fun: Sell"];

/// The probe's wallets, as addresses (a `wallet` matcher hashes them the way every
/// adapter hashes a print's wallet).
const WALLETS: [&str; 5] = ["w-eleven", "w-twelve", "w-thirteen", "w-fourteen", "w-fifteen"];

/// The two the `targets` tag names: one buys twice, one sells.
const COPY_TARGETS: [&str; 2] = [WALLETS[1], WALLETS[2]];

/// A wildcard fingerprint with one tag of each kind the probe reads.
fn probe_fp() -> Fingerprint {
    Fingerprint {
        id: FingerprintId(Uuid::from_u128(FP)),
        wildcard: true,
        criteria: Criteria::new(),
        tags: json!({
            "volume": { "match": { "ix_shape": [VOL_LABELS], "creator": true }, "sticky": true },
            "dump": { "match": { "ix_shape": [NONVOL_LABELS] }, "side": "sell" },
            "targets": { "match": { "wallet": COPY_TARGETS } },
            "working": { "match": { "program": ["Pump.Fun"] } }
        }),
    }
}

/// Every (tag, span) a metric accepts, as the condition keys that spell it.
fn variants(m: &MetricSpec) -> Vec<Map<String, Value>> {
    let mut tags: Vec<Option<&str>> = Vec::new();
    if m.tags != TagUse::Required {
        tags.push(None);
    }
    if m.tags != TagUse::None {
        match m.tag_level {
            TagLevel::Trade => tags.extend([Some("volume"), Some("!volume"), Some("dump"), Some("targets")]),
            TagLevel::Template => tags.push(Some("working")),
            TagLevel::WalletClass => tags.extend([Some("bundled"), Some("public_app")]),
        }
    }
    let mut spans: Vec<(Option<&str>, Option<&str>)> = Vec::new();
    if m.spans.life {
        spans.push((None, None));
    }
    if m.spans.window {
        // The probe's trades span slots 100..110 and a handful of prints: every window
        // covers them, and a slice nests inside.
        for (w, s) in [("10s", "2s"), ("20sl", "4sl"), ("50p", "4p")] {
            spans.push((Some(w), m.spans.slice.then_some(s)));
        }
    }
    if m.spans.since_age {
        spans.push((Some("age0s"), None));
    }
    let mut out = Vec::new();
    for tag in &tags {
        for (span, slice) in &spans {
            let mut c = Map::new();
            c.insert("metric".into(), json!(m.path()));
            if let Some(t) = tag {
                c.insert("tag".into(), json!(t));
            }
            if let Some(s) = span {
                c.insert("span".into(), json!(s));
            }
            if let Some(s) = slice {
                c.insert("slice".into(), json!(s));
            }
            out.push(c);
        }
    }
    out
}

/// A rule reading one condition, on the side it can sit on, with a permissive
/// threshold. The condition is never the point - the reading is.
fn params_for(m: &MetricSpec, cond: &Map<String, Value>) -> Value {
    let mut c = cond.clone();
    c.insert("is".into(), json!([{ "operator": ">=", "value": -1.0e9 }]));
    if m.path().starts_with("m_position.") {
        json!({ "always": [{ "if": [Value::Object(c)], "sell": "probe" }] })
    } else {
        json!({ "enter": { "filters": [Value::Object(c)] } })
    }
}

fn loaded_rule(params: Value, what: &str) -> LoadedRule {
    LoadedRule {
        id: RuleId(Uuid::from_u128(RULE)),
        fingerprint_id: FingerprintId(Uuid::from_u128(FP)),
        trade_mode: TradeMode::Paper,
        buy_amount_lamports: 100_000_000,
        max_concurrent_tokens: 1,
        max_total_tokens: 0,
        params: RuleParams::parse(&params).unwrap_or_else(|e| panic!("{what} does not validate as a rule: {e}")),
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
            trade_keys::ix_hash(&VOL_LABELS)
        } else {
            trade_keys::ix_hash(&NONVOL_LABELS)
        }),
        wallet_hash: trade_keys::wallet_hash(wallet),
        slot,
        marker_bits: 0,
        leg_index: 0,
        tx_index: Some(slot as u32),
        template_hash: Some(if vol {
            hunter_engine::metrics::template_grain::grain_hash(&VOL_LABELS).unwrap()
        } else {
            hunter_engine::metrics::template_grain::grain_hash(&NONVOL_LABELS).unwrap()
        }),
        build_hash: if vol { trade_keys::build_hash(&VOL_LABELS) } else { trade_keys::build_hash(&NONVOL_LABELS) },
        fee: hunter_engine::metrics::fee::FeeKeys::new(None, None, Some(0)),
        // Tokens proportional to SOL at the print's price, so the holder book adds up.
        token_amount: (sol / price * 1e6).round(),
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
            creator_wallet_hash: Some(trade_keys::wallet_hash("creator-wallet")),
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
            creator_stand_in_wallet_hash: None,
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
        // `retrace` / `bounce` read a real excursion rather than a seeded zero. It is
        // a REPEAT buyer: `m_print_wallet.since_buy` is the reading of the print
        // folded last, and a wallet's first buy honestly has no earlier one.
        reduce(
            state,
            Event::Trade {
                mint: mint.clone(),
                trade: trade(Side::Buy, 1.0, 2.0, 44.0, 6.0, 110, false, WALLETS[0]),
            },
        );
    }
}

/// Read one condition off the live engine after driving the probe stream.
fn live_reading(m: &MetricSpec, cond: &Map<String, Value>) -> f64 {
    let what = Value::Object(cond.clone()).to_string();
    let mint = Mint(format!("probe-{what}").into());
    let rule = loaded_rule(params_for(m, cond), &what);

    let mut state = EngineState::default();
    // The build-breadth table the holder book stamps buys from: without one every holder
    // is classed unknown and `@public_app` reads NaN by design.
    reduce(
        &mut state,
        Event::BuildBreadthReloaded {
            breadth: Arc::from(vec![hunter_engine::event::BuildBreadth {
                build_hash: trade_keys::build_hash(&VOL_LABELS).unwrap(),
                app_buyers: 1_000,
                app_buys: 3_000,
            }]),
        },
    );
    reduce(&mut state, Event::RulesReloaded { rules: Arc::from(vec![rule]), fps: Arc::from(vec![probe_fp()]) });
    let fx = drive(&mut state, &mint);
    confirm_entry(&mut state, &mint, fx);

    let out = read_state(&state, &mint, RuleId(Uuid::from_u128(RULE)), ts(7.0))
        .unwrap_or_else(|| panic!("{what}: the live engine has no arm to read"));
    out.conditions.first().unwrap_or_else(|| panic!("{what}: compiled away - nothing reads it")).value
}

#[test]
fn every_metric_reads_a_real_value_on_every_tag_and_span_it_accepts() {
    let mut unreadable: Vec<String> = Vec::new();
    let mut checked = 0usize;
    for m in METRICS {
        for cond in variants(m) {
            checked += 1;
            let v = live_reading(m, &cond);
            if !v.is_finite() {
                unreadable.push(format!("{} reads {v}", Value::Object(cond)));
            }
        }
    }
    assert!(
        unreadable.is_empty(),
        "{} of {checked} metric reads never produce a value on the live path \
         (a NaN gate is silently always-false, so no rule using one can ever fire).\n  {}",
        unreadable.len(),
        unreadable.join("\n  "),
    );
    assert!(checked >= 150, "probe covered only {checked} reads - the registry walk is broken");
}
