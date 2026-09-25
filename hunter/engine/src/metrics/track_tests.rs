//! Tests for [`super::TokenTrack`]: routing, window dedupe and decay, the three window
//! bases, tags, and the cross-check against the SQL the rules were fitted in.

use super::*;
use crate::metrics::tags::config::compile_tags;
use crate::metrics::trade_keys::ix_hash;
use crate::metrics::{Side, TagRef};
use chrono::{Duration, TimeZone, Utc};
use serde_json::json;
use uuid::Uuid;

fn ts(secs: f64) -> Ts {
    Utc.timestamp_opt(1_700_000_000, 0).unwrap() + Duration::milliseconds((secs * 1000.0) as i64)
}

fn buy(sol: f64, price: f64, reserve: f64, secs: f64) -> TradeLite {
    TradeLite { side: Side::Buy, sol, price, reserve_sol: reserve, at: ts(secs), ..Default::default() }
}

fn fp(n: u128) -> FingerprintId {
    FingerprintId(Uuid::from_u128(n))
}

fn life(m: Metric) -> MetricRef {
    MetricRef::life(m)
}

fn win(m: Metric, spec: WindowSpec) -> MetricRef {
    MetricRef::life(m).with_span(Span::window(spec))
}

fn tagged(m: Metric, tag: &str) -> MetricRef {
    MetricRef::life(m).with_tag(TagRef::parse(tag).unwrap())
}

/// Register one tag on `track` from a definition document.
fn add_tag(
    track: &mut TokenTrack,
    fp: FingerprintId,
    name: &str,
    def: serde_json::Value,
    windows: &[WindowSpec],
) -> TagKey {
    let tags = compile_tags(&json!({ name: def }));
    let t = &tags[0];
    track.ensure_tag(fp, t.key, &t.patterns, windows);
    if let Some(tp) = t.patterns.templates() {
        track.ensure_template_tag(fp, t.key, &tp);
    }
    t.key
}

#[test]
fn routes_each_family() {
    let mut track = TokenTrack::new(ts(0.0));
    track.ensure_window(WindowSpec::secs(10.0));
    track.on_trade(buy(3.0, 2.0, 15.0, 1.0));
    let at = ts(5.0);
    assert_eq!(track.value(life(Metric::AgeSec), None, at), 5.0);
    assert_eq!(track.value(life(Metric::LiquiditySol), None, at), 15.0);
    assert_eq!(track.value(life(Metric::StallSec), None, at), 4.0);
    assert_eq!(track.value(life(Metric::TrailPct), None, at), 0.0);
    assert_eq!(track.value(life(Metric::RisePct), None, at), 0.0);
    assert_eq!(track.value(win(Metric::BuySol, WindowSpec::secs(10.0)), None, at), 3.0);
    assert_eq!(track.value(win(Metric::GrossSol, WindowSpec::secs(10.0)), None, at), 3.0);
    assert_eq!(track.value(life(Metric::BuySol), None, at), 3.0);
    assert_eq!(track.value(life(Metric::BuyCount), None, at), 1.0);
    assert!(track.value(life(Metric::PnlPct), None, at).is_nan(), "a position metric is not on the track");
}

#[test]
fn lifetime_flow_survives_window_decay() {
    let mut track = TokenTrack::new(ts(0.0));
    track.ensure_window(WindowSpec::secs(10.0));
    track.on_trade(buy(4.0, 1.0, 20.0, 0.0));
    let w = win(Metric::BuySol, WindowSpec::secs(10.0));
    assert_eq!(track.value(w, None, ts(5.0)), 4.0);
    track.on_tick(ts(11.0), None);
    assert_eq!(track.value(w, None, ts(11.0)), 0.0);
    assert_eq!(track.value(life(Metric::BuySol), None, ts(11.0)), 4.0);
}

#[test]
fn an_unregistered_window_is_nan() {
    let mut track = TokenTrack::new(ts(0.0));
    track.on_trade(buy(3.0, 2.0, 15.0, 1.0));
    assert!(track.value(win(Metric::BuySol, WindowSpec::secs(10.0)), None, ts(5.0)).is_nan());
    assert!(track.value(win(Metric::TrailPct, WindowSpec::secs(30.0)), None, ts(5.0)).is_nan());
}

#[test]
fn equal_windows_dedupe_and_families_keep_their_own_buffers() {
    let mut track = TokenTrack::new(ts(0.0));
    track.ensure_window(WindowSpec::secs(10.0));
    track.ensure_window(WindowSpec::secs(10.0));
    track.ensure_price_window(WindowSpec::secs(10.0));
    assert_eq!(track.windows.len(), 1);
    assert_eq!(track.price_windows.len(), 1);
    track.on_trade(buy(1.0, 2.0, 15.0, 1.0));
    track.on_trade(buy(1.0, 1.5, 15.0, 2.0));
    let trail = track.value(win(Metric::TrailPct, WindowSpec::secs(10.0)), None, ts(2.0));
    assert!((trail - 25.0).abs() < 1e-9);
}

/// Three one-SOL prints and one three-SOL print are the same over any seconds window;
/// over `1p` they are 1 and 3. That difference IS the print basis.
#[test]
fn a_single_print_window_reads_one_transaction() {
    let mut spread = TokenTrack::new(ts(0.0));
    let mut one = TokenTrack::new(ts(0.0));
    for t in [&mut spread, &mut one] {
        t.ensure_window(WindowSpec::prints(1.0, 0.0));
        t.ensure_window(WindowSpec::secs(10.0));
    }
    for i in 0..3 {
        spread.on_trade(buy(1.0, 1.0, 20.0, f64::from(i)));
    }
    one.on_trade(buy(3.0, 1.0, 20.0, 2.0));
    let (p, s, now) = (WindowSpec::prints(1.0, 0.0), WindowSpec::secs(10.0), ts(2.0));
    assert_eq!(spread.value(win(Metric::GrossSol, s), None, now), 3.0);
    assert_eq!(one.value(win(Metric::GrossSol, s), None, now), 3.0);
    assert_eq!(spread.value(win(Metric::GrossSol, p), None, now), 1.0);
    assert_eq!(one.value(win(Metric::GrossSol, p), None, now), 3.0);
}

/// A print window spans trades, not time, and a lag of 1 excludes the trade being read.
#[test]
fn a_print_window_spans_trades_and_lags_by_trades() {
    let mut track = TokenTrack::new(ts(0.0));
    let (last3, prior3) = (WindowSpec::prints(3.0, 0.0), WindowSpec::prints(3.0, 1.0));
    track.ensure_window(last3);
    track.ensure_window(prior3);
    for (sol, secs) in [(1.0, 0.0), (2.0, 900.0), (4.0, 1800.0), (8.0, 3600.0)] {
        track.on_trade(buy(sol, 1.0, 20.0, secs));
    }
    let now = ts(3600.0);
    assert_eq!(track.value(win(Metric::GrossSol, last3), None, now), 14.0);
    assert_eq!(track.value(win(Metric::GrossSol, prior3), None, now), 7.0);
}

/// Silence is not a print: a seconds window decays, a print window holds.
#[test]
fn ticks_never_decay_a_print_window() {
    let mut track = TokenTrack::new(ts(0.0));
    track.ensure_window(WindowSpec::prints(5.0, 0.0));
    track.ensure_window(WindowSpec::secs(10.0));
    track.on_trade(buy(4.0, 1.0, 20.0, 0.0));
    track.on_tick(ts(3600.0), Some(9_999));
    assert_eq!(track.value(win(Metric::BuySol, WindowSpec::secs(10.0)), None, ts(3600.0)), 0.0);
    assert_eq!(track.value(win(Metric::BuySol, WindowSpec::prints(5.0, 0.0)), None, ts(3600.0)), 4.0);
    assert_eq!(track.n_prints(), 1);
}

#[test]
fn print_and_slot_windows_of_one_size_do_not_dedupe() {
    let mut track = TokenTrack::new(ts(0.0));
    track.ensure_window(WindowSpec::prints(1.0, 0.0));
    track.ensure_window(WindowSpec::slots(1.0, 0.0));
    for sol in [1.0, 2.0, 5.0] {
        track.on_trade(TradeLite { slot: 7, ..buy(sol, 1.0, 20.0, 0.0) });
    }
    let now = ts(0.0);
    assert_eq!(track.value(win(Metric::GrossSol, WindowSpec::prints(1.0, 0.0)), None, now), 5.0);
    assert_eq!(track.value(win(Metric::GrossSol, WindowSpec::slots(1.0, 0.0)), None, now), 8.0);
}

/// A tag reads only under the fingerprint it was registered for, both halves, life and
/// window; an unregistered tag reads NaN.
#[test]
fn a_tag_reads_under_its_own_fingerprint() {
    let mut track = TokenTrack::new(ts(0.0));
    let (a, b) = (fp(1), fp(2));
    let w = WindowSpec::secs(10.0);
    add_tag(&mut track, a, "volume", json!({ "match": { "ix_shape": [["vol"]] } }), &[w]);
    let mut t = buy(4.0, 1.0, 20.0, 0.0);
    t.ix_hash = Some(ix_hash(&["vol"]));
    t.wallet_hash = 1;
    track.on_trade(t);
    track.on_trade(TradeLite { wallet_hash: 2, ..buy(6.0, 1.0, 26.0, 1.0) });
    let now = ts(2.0);
    assert_eq!(track.value(tagged(Metric::BuySol, "volume"), Some(a), now), 4.0);
    assert_eq!(track.value(tagged(Metric::BuySol, "!volume"), Some(a), now), 6.0);
    let windowed = tagged(Metric::BuySol, "volume").with_span(Span::window(w));
    assert_eq!(track.value(windowed, Some(a), now), 4.0);
    assert!(track.value(tagged(Metric::BuySol, "volume"), Some(b), now).is_nan(), "another fingerprint");
    assert!(track.value(tagged(Metric::BuySol, "volume"), None, now).is_nan(), "no fingerprint");
    assert!(track.value(tagged(Metric::BuySol, "dump"), Some(a), now).is_nan(), "an unregistered tag");
}

/// One sell on two lists counts in both: the tags are independent answers.
#[test]
fn one_sell_on_two_tags_counts_in_both() {
    let mut track = TokenTrack::new(ts(0.0));
    let id = fp(1);
    let shape = json!([["Pump.Fun: Sell", "Token Program: CloseAccount"]]);
    add_tag(&mut track, id, "volume", json!({ "match": { "ix_shape": shape, "creator": true }, "sticky": true }), &[]);
    add_tag(&mut track, id, "dump", json!({ "match": { "ix_shape": shape }, "side": "sell" }), &[]);
    track.seed_creator(99);
    let mut t = buy(3.0, 1.0, 20.0, 0.0);
    t.side = Side::Sell;
    t.ix_hash = Some(ix_hash(&["Pump.Fun: Sell", "Token Program: CloseAccount"]));
    t.wallet_hash = 7;
    track.on_trade(t);
    let v = |r: MetricRef| track.value(r, Some(id), ts(1.0));
    assert_eq!(v(tagged(Metric::SellSol, "volume")), 3.0);
    assert_eq!(v(tagged(Metric::SellSol, "dump")), 3.0);
    assert_eq!(v(tagged(Metric::SellSol, "!volume")), 0.0);
    assert_eq!(v(tagged(Metric::SellTxCount, "dump")), 1.0);
}

/// A tag window decays on a tick like every other window.
#[test]
fn a_tag_window_decays_on_a_tick() {
    let mut track = TokenTrack::new(ts(0.0));
    let id = fp(1);
    let w = WindowSpec::secs(10.0);
    let key = add_tag(&mut track, id, "dump", json!({ "match": { "ix_shape": [["S"]] }, "side": "sell" }), &[w]);
    track.on_trade(TradeLite {
        side: Side::Sell,
        sol: 2.0,
        price: 1.0,
        at: ts(1.0),
        ix_hash: Some(ix_hash(&["S"])),
        ..Default::default()
    });
    let r = tagged(Metric::SellSol, "dump").with_span(Span::window(w));
    assert_eq!(track.value(r, Some(id), ts(2.0)), 2.0);
    track.on_tick(ts(20.0), None);
    assert_eq!(track.value(r, Some(id), ts(20.0)), 0.0);
    assert_eq!(track.tag_window_len(id, key, w), Some(0), "the tick evicted it");
}

/// An edited tag reaches a coin the engine is ALREADY tracking; the past keeps the half
/// it was folded under.
#[test]
fn a_reload_adopts_an_edited_tag_on_a_live_coin() {
    let mut track = TokenTrack::new(ts(0.0));
    let id = fp(1);
    let bot = ix_hash(&["bot", "buy"]);
    add_tag(&mut track, id, "volume", json!({ "match": { "ix_shape": [["other"]] } }), &[]);
    track.on_trade(TradeLite { ix_hash: Some(bot), wallet_hash: 11, ..buy(1.0, 1.0, 10.0, 1.0) });
    assert_eq!(track.value(tagged(Metric::BuySol, "!volume"), Some(id), ts(1.0)), 1.0);
    add_tag(&mut track, id, "volume", json!({ "match": { "ix_shape": [["bot", "buy"]] } }), &[]);
    track.on_trade(TradeLite { ix_hash: Some(bot), wallet_hash: 12, ..buy(2.0, 1.0, 10.0, 2.0) });
    assert_eq!(track.value(tagged(Metric::BuySol, "volume"), Some(id), ts(2.0)), 2.0);
    assert_eq!(track.value(tagged(Metric::BuySol, "!volume"), Some(id), ts(2.0)), 1.0);
}

/// A slot metric reads untagged without a tag, and NaN with a tag the fingerprint has
/// no template view for — never the untagged number under a tag's name.
#[test]
fn slot_reads_untagged_or_through_a_template_tag() {
    use crate::metrics::template_grain::grain_id_hash;
    let mut track = TokenTrack::new(ts(0.0));
    track.ensure_slot_state();
    let id = fp(1);
    add_tag(&mut track, id, "working", json!({ "match": { "ix_template": ["A|CU|F"] } }), &[]);
    for (w, tmpl) in [(1, grain_id_hash("A|CU|F")), (2, grain_id_hash("B|CU|F"))] {
        track.on_trade(TradeLite {
            slot: 10,
            template_hash: Some(tmpl),
            wallet_hash: w,
            on_curve: true,
            ..buy(1.0, 1.0, 10.0, 0.0)
        });
    }
    let now = ts(0.0);
    assert_eq!(track.value(life(Metric::SlotTemplateCount), None, now), 2.0);
    assert_eq!(track.value(tagged(Metric::SlotTemplateCount, "working"), Some(id), now), 1.0);
    assert_eq!(track.value(tagged(Metric::SlotBuyCount, "working"), Some(id), now), 1.0);
    assert!(track.value(tagged(Metric::SlotBuyCount, "other"), Some(id), now).is_nan());
    assert!(track.value(tagged(Metric::SlotTemplateCount, "other"), Some(id), now).is_nan());
}

/// **Cross-implementation parity.** Three real token tapes from the lake, replayed
/// through `TokenTrack`, asserted against the values an independent SQL
/// implementation computed for the same instant. The rule these metrics carry was
/// fitted in that SQL, so any semantic drift between the two - a window edge, a
/// poisoned-trade rule, a ratio's empty case - silently moves the threshold that
/// ships away from the one that was validated. Fixture: the three rule-firing
/// tokens with the fewest trades, so it stays readable.
#[test]
fn engine_metrics_match_the_sql_the_rule_was_fitted_in() {
    struct Case {
        mint: &'static str,
        /// `(offset_secs_from_first_trade, is_buy, sol, wallet)`
        trades: &'static [(f64, bool, f64, u64)],
        now: f64,
        buy3: f64,
        buy5: f64,
        gross60: f64,
        ntx60: f64,
        uw10: f64,
        tpw10: f64,
        lifegross: f64,
        lifentx: f64,
        buyshare10: f64,
        /// `m_flow.slice_trade_share_pct [60s, slice 3s]`
        share60_3: f64,
        /// Same reference window, a wider slice — the pair that proves the
        /// second axis is read and not ignored.
        share60_10: f64,
    }
    let cases = [
        Case {
            mint: "EmZkRz1q",
            trades: &[
                (0.000000, true, 4.000000000, 1),
                (0.000628, true, 2.300000000, 2),
                (0.001010, true, 2.700000000, 3),
                (7.921934, false, 5.173469387, 1),
                (8.425656, false, 2.005896761, 2),
                (8.455656, false, 1.820633850, 3),
                (8.606678, true, 0.296296296, 4),
                (190.617137, false, 0.296296295, 4),
                (332.726772, true, 3.111477703, 5),
                (332.730405, true, 2.110947953, 6),
                (332.731504, true, 1.707943763, 7),
                (332.739706, true, 3.394372016, 8),
                (332.751188, true, 4.341925230, 1),
                (334.436165, false, 4.127288448, 8),
                (334.440091, false, 4.567766132, 5),
                (334.440176, false, 1.622362176, 7),
                (334.588120, false, 2.010570212, 6),
                (334.824645, false, 2.338679695, 1),
            ],
            now: 334.824645,
            buy3: 14.666666665,
            buy5: 14.666666665,
            gross60: 29.333333328,
            ntx60: 10.0,
            uw10: 5.0,
            tpw10: 2.000000000,
            lifegross: 47.925925917,
            lifentx: 18.0,
            buyshare10: 50.000000003,
            share60_3: 100.0,
            share60_10: 100.0,
        },
        Case {
            mint: "2CewB2b1",
            trades: &[
                (0.000000, true, 5.000000000, 1),
                (0.000789, true, 3.965000000, 2),
                (0.331632, true, 0.545901233, 3),
                (1.400722, false, 0.545901232, 3),
                (1.486422, false, 7.349716113, 1),
                (1.498124, false, 3.511053484, 4),
                (1.505395, false, 2.272230402, 2),
                (1.559892, true, 1.975308641, 5),
                (1.672612, true, 0.109507785, 6),
                (3.790649, false, 0.109507784, 6),
                (76.600342, false, 1.975308640, 5),
                (140.667753, true, 1.950007476, 7),
                (140.668349, true, 2.264512947, 8),
                (140.668658, true, 2.669261795, 9),
                (140.668997, true, 2.804014410, 10),
                (140.713764, true, 2.059379313, 11),
                (140.715517, true, 1.925031387, 12),
                (140.715881, true, 2.420431215, 13),
                (140.716362, true, 1.980219096, 14),
                (140.716526, true, 2.019626664, 15),
                (140.717024, true, 2.272601898, 16),
            ],
            now: 140.717024,
            buy3: 22.365086201,
            buy5: 22.365086201,
            gross60: 22.365086201,
            ntx60: 10.0,
            uw10: 10.0,
            tpw10: 1.000000000,
            lifegross: 49.724521515,
            lifentx: 21.0,
            buyshare10: 100.000000000,
            share60_3: 100.0,
            share60_10: 100.0,
        },
        Case {
            mint: "NMYKtKLS",
            trades: &[
                (0.000000, true, 5.000000000, 1),
                (0.000692, true, 2.000000000, 2),
                (0.001253, true, 2.000000000, 3),
                (0.001507, true, 2.000000000, 4),
                (2.126228, false, 1.166551006, 1),
                (2.214598, false, 1.102004999, 1),
                (2.806099, false, 1.707148144, 1),
                (3.197055, false, 2.721507005, 1),
                (3.532430, true, 0.391111111, 5),
                (4.137951, true, 2.007635548, 6),
                (4.138246, true, 2.064413120, 7),
                (4.138885, true, 1.794618000, 8),
                (20.533181, false, 0.533549651, 5),
                (81.752705, true, 1.655506109, 4),
                (81.754106, true, 1.467052279, 3),
                (81.754148, true, 1.766330501, 2),
                (82.434242, true, 1.529342198, 3),
                (82.434570, true, 1.725509662, 2),
                (82.435232, true, 1.634037030, 4),
                (83.206192, true, 1.598867187, 2),
                (83.206245, true, 1.581689765, 3),
                (84.700243, false, 3.809554246, 4),
                (84.700304, false, 3.912501603, 3),
            ],
            now: 84.700304,
            buy3: 12.958334731,
            buy5: 12.958334731,
            gross60: 20.680390580,
            ntx60: 10.0,
            uw10: 3.0,
            tpw10: 3.333333333,
            lifegross: 45.168929164,
            lifentx: 23.0,
            buyshare10: 62.660009640,
            share60_3: 100.0,
            share60_10: 100.0,
        },
        // A 6ix-cohort tape read mid-life, where the two slice axes DIVERGE:
        // 4 of the last 20 trades landed in the last 3s, 17 in the last 10s. The
        // three cases above all read 100 on both (their whole 60s reference is
        // one cluster), so without this one the second axis could be dropped and
        // the harness would still pass.
        Case {
            mint: "22wow9yw",
            trades: &[
                (0.000000, true, 3.000000000, 1),
                (0.000042, true, 1.810000000, 2),
                (0.000590, true, 1.190000000, 3),
                (1.539222, false, 0.129135113, 1),
                (1.696796, false, 0.128211987, 1),
                (1.858192, false, 0.345055645, 1),
                (1.909494, true, 0.353495243, 4),
                (2.044103, false, 0.127358740, 1),
                (3.021644, false, 0.102623499, 1),
                (3.135742, false, 0.348984088, 4),
                (3.202521, false, 0.271507091, 1),
                (4.593791, false, 0.038128166, 3),
                (4.652663, false, 0.103563739, 3),
                (4.855668, false, 0.102950261, 3),
                (7.454992, false, 0.085213335, 1),
                (7.767080, false, 0.230284305, 1),
                (10.956288, false, 0.073401251, 1),
                (11.075448, false, 0.198599623, 1),
                (11.256284, true, 0.009876542, 5),
                (11.263913, false, 0.196424375, 1),
            ],
            now: 11.263913,
            buy3: 0.009876542,
            buy5: 0.009876542,
            gross60: 8.844813003,
            ntx60: 20.0,
            uw10: 4.0,
            tpw10: 4.250000000,
            lifegross: 8.844813003,
            lifentx: 20.0,
            buyshare10: 12.773134284,
            share60_3: 20.000000000,
            share60_10: 85.000000000,
        },
    ];
    for c in &cases {
        let mut track = TokenTrack::new(ts(0.0));
        for w in [3.0, 5.0, 10.0, 60.0] {
            track.ensure_window(WindowSpec::secs(w));
            // The crowd metrics read their own deque, so the parity harness has to
            // register it - exactly as a rule gating on them does.
            track.ensure_crowd_window(WindowSpec::secs(w));
        }
        for &(at, is_buy, sol, wallet) in c.trades {
            track.on_trade(TradeLite {
                slot: 0,
                marker_bits: 0,
                side: if is_buy { Side::Buy } else { Side::Sell },
                sol,
                price: 1.0,
                reserve_sol: 100.0,
                priced_reserve_sol: 100.0,
                at: ts(at),
                ix_hash: None,
                wallet_hash: wallet,
                leg_index: 0,
                ..Default::default()
            });
        }
        let now = ts(c.now);
        let got = |m: Metric, w: Option<f64>| {
            let span = w.map_or(Span::LIFE, Span::secs);
            track.value(MetricRef::life(m).with_span(span), None, now)
        };
        // Each assertion carries that metric's own `eq_tolerance`: parity means
        // "indistinguishable to a condition", not bit equality.
        let close = |a: f64, b: f64, tol: f64, what: &str| {
            assert!((a - b).abs() <= tol, "{} {}: engine {a} vs sql {b} (tol {tol})", c.mint, what);
        };
        close(got(Metric::BuySol, Some(3.0)), c.buy3, 0.1, "m_flow_window(3).buy");
        close(got(Metric::BuySol, Some(5.0)), c.buy5, 0.1, "m_flow_window(5).buy");
        close(got(Metric::GrossSol, Some(60.0)), c.gross60, 0.1, "gross_flow(60)");
        close(got(Metric::TradeCount, Some(60.0)), c.ntx60, 0.5, "trade_count(60)");
        close(got(Metric::UniqueWallets, Some(10.0)), c.uw10, 0.5, "unique_wallets(10)");
        close(got(Metric::TradesPerWallet, Some(10.0)), c.tpw10, 0.05, "trades_per_wallet(10)");
        close(got(Metric::BuySharePct, Some(10.0)), c.buyshare10, 0.5, "buy_share(10)");
        close(got(Metric::GrossSol, None), c.lifegross, 0.1, "m_flow_lifetime.gross_flow");
        close(got(Metric::TradeCount, None), c.lifentx, 0.5, "m_flow_lifetime.trade_count");
        // The two-window read, through the same public entry point a rule uses.
        let share = |reference: f64, slice: f64| {
            let span = Span::sliced(WindowSpec::secs(reference), WindowSpec::secs(slice));
            track.value(MetricRef::life(Metric::SliceTradeSharePct).with_span(span), None, now)
        };
        close(share(60.0, 3.0), c.share60_3, 0.5, "m_flow_window{60,3}.trade_share");
        close(share(60.0, 10.0), c.share60_10, 0.5, "m_flow_window{60,10}.trade_share");
        // An unregistered axis reads NaN, never a silently narrower window.
        assert!(share(60.0, 7.0).is_nan(), "{}: unregistered slice axis must be NaN", c.mint);
    }
}

