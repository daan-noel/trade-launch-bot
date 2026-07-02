//! `POST /api/tokens/{mint}/swing1-detect` — the swing1 classification funnel for
//! ONE token, surfaced for the UI's per-token detection page.
//!
//! Runs the **identical** pure fns the grouped sweep runs — `detect_swing_legs_raw`
//! → `LowFeatures::from_low` + `is_kill_low`/`is_volume_low` → `classify_phase`
//! → `find_phase_entry` → `find_trade_driven_exit` — so the funnel shown is
//! byte-identical to the sweep's decision for this token. It is the JSON twin of
//! `lab swing-probe` (`lab/src/swing_probe.rs`); keep the two in lockstep.
//!
//! No new strategy logic lives here — only the per-low verdicts + latch + entry +
//! exit are collected into one response so the page can render the table + chart
//! overlay.

use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use serde::{Deserialize, Serialize};

use trading_core::models::Swing1Rule;
use trading_core::strategies::swing_1::{
    classifier::{self, LowFeatures},
    entry::find_phase_entry,
    exit::find_trade_driven_exit,
    phase_profile_from_rule, rule_configures_any_entry_gate,
    swing::{detect_swing_legs_raw, SwingLeg, SwingType},
    swing_params_from_rule,
};

use crate::{
    api::handlers::tokens::filter_trades_to_window, models::trade::Trade,
    state::local_state::LocalState, state::token_cache::MAX_TRADES_RETAINED,
};

/// Same bounded DB read window the generic swing endpoint uses.
const SWING_DB_TRADE_CAP: i64 = MAX_TRADES_RETAINED as i64;

/// The page-editable swing1 params (the 24 swept knobs), all optional. A `None`
/// means "inert / no bound" — identical to a sweep axis left blank. `take_profit`/
/// `stop_loss` default to sane non-zero values so the exit ladder always resolves
/// a fill once an entry fires.
#[derive(Debug, Clone, Deserialize)]
pub struct Swing1DetectParams {
    pub take_profit: Option<f64>,
    pub stop_loss: Option<f64>,
    pub trailing_stop_pct: Option<f64>,
    pub time_stop_secs: Option<u64>,
    pub stall_secs: Option<u64>,
    pub liquidity_drop_pct: Option<f64>,
    pub swing_high_to_low_sol: Option<f64>,
    pub swing_high_to_low_pct: Option<f64>,
    pub swing_low_to_high_sol: Option<f64>,
    pub swing_low_to_high_pct: Option<f64>,
    pub swing_min_leg_trades: Option<u32>,
    /// Dynamic dust floor (fraction of the active leg's max trade). A trade is
    /// dropped if its SOL is `< dust_frac * active_leg_max_sol`. Scale-free.
    /// `None`/`0` = off.
    pub dust_frac: Option<f64>,
    pub kill_depth_min_pct: Option<f64>,
    pub kill_max_duration_ms: Option<i64>,
    pub kill_min_net_flow_per_sec: Option<f64>,
    pub vol_depth_max_pct: Option<f64>,
    pub vol_min_duration_ms: Option<i64>,
    pub vol_min_up_duration_ms: Option<i64>,
    pub min_kills_before_volume: Option<u32>,
    pub entry_pullback_pct: Option<f64>,
    pub entry_higher_low_secs: Option<u64>,
    pub entry_max_age_secs: Option<u64>,
    pub entry_min_liquidity_sol: Option<f64>,
    pub exit_next_kill_depth_min_pct: Option<f64>,
    pub exit_next_kill_max_duration_ms: Option<i64>,
}

impl Swing1DetectParams {
    /// Overlay these knobs onto a synthetic base rule, producing the exact
    /// [`Swing1Rule`] the pure fns expect (mirrors the sweep's per-combo build).
    fn to_rule(&self) -> Swing1Rule {
        let mut r = Swing1Rule::new(
            "swing1-detect".into(),
            None,
            None,
            None,
            serde_json::json!([]),
            "paper".into(),
            1.0, // buy_amount_sol — notional only; PnL% is notional-independent here
            self.take_profit.unwrap_or(100.0),
            self.stop_loss.unwrap_or(50.0),
            None,
            None,
            None,
            None,
            None,
            self.trailing_stop_pct,
            self.time_stop_secs,
            self.stall_secs,
            self.liquidity_drop_pct,
        );
        r.p_swing_high_to_low_sol = self.swing_high_to_low_sol;
        r.p_swing_high_to_low_pct = self.swing_high_to_low_pct;
        r.p_swing_low_to_high_sol = self.swing_low_to_high_sol;
        r.p_swing_low_to_high_pct = self.swing_low_to_high_pct;
        r.p_swing_min_leg_trades = self.swing_min_leg_trades;
        r.p_dust_frac = self.dust_frac;
        r.p_kill_depth_min_pct = self.kill_depth_min_pct;
        r.p_kill_max_duration_ms = self.kill_max_duration_ms;
        r.p_kill_min_net_flow_per_sec = self.kill_min_net_flow_per_sec;
        r.p_vol_depth_max_pct = self.vol_depth_max_pct;
        r.p_vol_min_duration_ms = self.vol_min_duration_ms;
        r.p_vol_min_up_duration_ms = self.vol_min_up_duration_ms;
        r.p_min_kills_before_volume = self.min_kills_before_volume;
        r.p_entry_pullback_pct = self.entry_pullback_pct;
        r.p_entry_higher_low_secs = self.entry_higher_low_secs;
        r.p_entry_max_age_secs = self.entry_max_age_secs;
        r.p_entry_min_liquidity_sol = self.entry_min_liquidity_sol;
        r.p_exit_next_kill_depth_min_pct = self.exit_next_kill_depth_min_pct;
        r.p_exit_next_kill_max_duration_ms = self.exit_next_kill_max_duration_ms;
        r
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Swing1DetectRequest {
    pub params: Swing1DetectParams,
    #[serde(default)]
    pub window_start_ms: Option<i64>,
    #[serde(default)]
    pub window_end_ms: Option<i64>,
    #[serde(default)]
    pub curve_only: bool,
}

/// Per-swing-low verdict row — the classifier gates that drive the latch, plus the
/// leg's chart-anchoring timestamps so the UI can mark it on the candle chart.
#[derive(Debug, Clone, Serialize)]
pub struct Swing1LowVerdict {
    /// Index into the raw leg ledger (matches `legs[i]`).
    pub leg_index: usize,
    pub start_at_ms: i64,
    pub end_at_ms: i64,
    pub depth_pct: f64,
    pub duration_ms: i64,
    pub net_flow_per_sec: f64,
    pub trade_count: u32,
    pub pivot_price: f64,
    pub is_kill: bool,
    pub is_volume: bool,
    /// Whether this low is a higher-low vs the running last-kill pivot — the exact
    /// gate `classify_phase` applies (vacuously true with no prior kill).
    pub higher_low_ok: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Swing1LatchInfo {
    pub volume_phase_latched: bool,
    pub latched_leg_index: Option<usize>,
    pub kills_seen: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct Swing1EntryInfo {
    /// Trigger trade index in the (windowed) trade slice.
    pub trigger_index: usize,
    pub price: f64,
    pub time: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Swing1ExitInfo {
    pub reason: String,
    pub price: f64,
    pub time: String,
    pub holding_secs: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Swing1DetectResponse {
    pub mint: String,
    pub trade_count: usize,
    /// `false` ⇒ the rule configures no entry gate, so `find_phase_entry` bails
    /// immediately (the funnel still shows legs + verdicts + latch for diagnosis).
    pub gate_configured: bool,
    pub legs: Vec<SwingLeg>,
    pub lows: Vec<Swing1LowVerdict>,
    pub latch: Swing1LatchInfo,
    pub entry: Option<Swing1EntryInfo>,
    pub exit: Option<Swing1ExitInfo>,
}

/// Build the funnel from a windowed `Trade` slice — pure CPU, no I/O. Mirrors
/// `swing_probe::run`'s per-token block exactly so the page and the CLI agree.
fn build_funnel(mint: String, trades: &[Trade], rule: &Swing1Rule) -> Swing1DetectResponse {
    let gate_configured = rule_configures_any_entry_gate(rule);
    let sparams = swing_params_from_rule(rule);
    let profile = phase_profile_from_rule(rule);
    let legs = detect_swing_legs_raw(trades, &sparams);

    // Walk the ledger collecting per-low verdicts, tracking the preceding up-leg
    // duration and the running last-kill pivot — the same state `classify_phase`
    // keeps, so `higher_low_ok` matches the latch gate.
    let mut lows = Vec::new();
    let mut prev_up: Option<i64> = None;
    let mut last_kill_pivot: Option<f64> = None;
    for (i, leg) in legs.iter().enumerate() {
        match leg.leg_type {
            SwingType::SwingHigh => prev_up = Some(leg.duration_ms),
            SwingType::SwingLow => {
                if let Some(f) = LowFeatures::from_low(leg) {
                    let is_kill = profile.is_kill_low(&f);
                    let is_volume = profile.is_volume_low(&f, prev_up);
                    let higher_low_ok =
                        last_kill_pivot.map_or(true, |p| f.pivot_price >= p);
                    lows.push(Swing1LowVerdict {
                        leg_index: i,
                        start_at_ms: leg.start_at,
                        end_at_ms: leg.end_at,
                        depth_pct: f.depth_pct,
                        duration_ms: f.duration_ms,
                        net_flow_per_sec: f.net_flow_per_sec,
                        trade_count: f.trade_count,
                        pivot_price: f.pivot_price,
                        is_kill,
                        is_volume,
                        higher_low_ok,
                    });
                    if is_kill {
                        last_kill_pivot = Some(f.pivot_price);
                    }
                }
                prev_up = None;
            }
        }
    }

    let latch = classifier::classify_phase(&legs, &profile);
    let latch_info = Swing1LatchInfo {
        volume_phase_latched: latch.volume_phase_latched,
        latched_leg_index: latch.latched_leg_index,
        kills_seen: latch.kills_seen,
    };

    // Entry + exit (only if a gate is configured — else find_phase_entry bails).
    let mut entry_info = None;
    let mut exit_info = None;
    if gate_configured {
        if let Some((idx, fill)) = find_phase_entry(trades, rule) {
            if fill.price > 0.0 {
                entry_info = Some(Swing1EntryInfo {
                    trigger_index: idx,
                    price: fill.price,
                    time: fill.block_time.to_rfc3339(),
                });
                if let Some(ef) = find_trade_driven_exit(trades, fill.block_time, fill.price, rule) {
                    exit_info = Some(Swing1ExitInfo {
                        reason: ef.reason.as_str().to_string(),
                        price: ef.price,
                        time: ef.block_time.to_rfc3339(),
                        holding_secs: (ef.block_time - fill.block_time).num_seconds(),
                    });
                }
            }
        }
    }

    Swing1DetectResponse {
        mint,
        trade_count: trades.len(),
        gate_configured,
        legs,
        lows,
        latch: latch_info,
        entry: entry_info,
        exit: exit_info,
    }
}

/// `POST /api/tokens/{mint}/swing1-detect` — see module docs.
pub async fn detect_token_swing1(
    state: web::Data<Arc<LocalState>>,
    path: web::Path<String>,
    body: web::Json<Swing1DetectRequest>,
) -> impl Responder {
    let mint = path.into_inner();
    let Swing1DetectRequest { params, window_start_ms, window_end_ms, curve_only } =
        body.into_inner();
    let rule = params.to_rule();

    let repo = state.trade_repo();
    let trades: Arc<Vec<Trade>> = match repo.find_by_mint_paged(&mint, SWING_DB_TRADE_CAP, 0).await {
        Ok(trades) => Arc::new(trades),
        Err(e) => {
            tracing::error!("DB error fetching trades for swing1 detect {mint}: {e}");
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({ "error": "database error" }));
        }
    };

    let result = web::block(move || {
        let curve_filtered: Option<Vec<Trade>> = curve_only
            .then(|| trades.iter().filter(|t| t.venue == "curve").cloned().collect());
        let base: &[Trade] = curve_filtered.as_deref().unwrap_or(&trades);
        let windowed = filter_trades_to_window(base, window_start_ms, window_end_ms);
        build_funnel(mint, &windowed, &rule)
    })
    .await;

    match result {
        Ok(resp) => HttpResponse::Ok().json(resp),
        Err(e) => {
            tracing::error!("swing1 detect compute task panicked: {e}");
            HttpResponse::InternalServerError()
                .json(serde_json::json!({ "error": "swing1 detection failed" }))
        }
    }
}
