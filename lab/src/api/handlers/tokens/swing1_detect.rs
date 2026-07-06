//! `POST /api/tokens/{mint}/swing1-detect` — the swing1 classification funnel for
//! ONE token, surfaced for the UI's per-token detection page.
//!
//! Reads the **same uncapped Parquet-lake corpus the backtest/sweep price on**
//! (`fetch_sim_history_one`) and runs the shared [`build_swing1_funnel`] +
//! `find_phase_entry` + `find_trade_driven_exit` — so the funnel shown is identical
//! to the sim's decision for this token *by construction* (one corpus, one builder),
//! not merely "should match". Full history, no `MAX_TRADES_RETAINED` cap — that
//! constant is the live in-RAM cache trim, never an analysis bound. It is the JSON
//! twin of `lab swing-probe` (`lab/src/swing_probe.rs`); both call the shared funnel.
//!
//! No new strategy logic lives here — only the per-low verdicts + latch + entry +
//! exit are collected into one response so the page can render the table + chart
//! overlay.

use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use serde::{Deserialize, Serialize};

use trading_core::models::trade::TradeRow;
use trading_core::models::Swing1Rule;
use trading_core::strategies::swing_1::{
    entry::find_phase_entry,
    exit::find_trade_driven_exit,
    funnel::{build_swing1_funnel, Swing1LatchInfo, Swing1LowVerdict},
    swing::SwingLeg,
};

use crate::sweep::projection::CorpusTrade;
use crate::{state::local_state::LocalState, strategies::sim_fetch::fetch_sim_history_one};

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
    pub mint_address: String,
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

/// Restrict a `CorpusTrade` slice to a window measured in ms relative to the token's
/// first SOL-carrying trade (the opening trade detection anchors on) — the lake twin
/// of the generic endpoint's `filter_trades_to_window`. Both bounds unset (or no
/// usable anchor) ⇒ the whole slice.
fn window_corpus_trades(
    trades: &[CorpusTrade],
    window_start_ms: Option<i64>,
    window_end_ms: Option<i64>,
) -> Vec<CorpusTrade> {
    if window_start_ms.is_none() && window_end_ms.is_none() {
        return trades.to_vec();
    }
    let anchor = trades
        .iter()
        .filter(|t| t.amount_sol() > 0.0)
        .map(|t| t.block_time().timestamp_millis())
        .min();
    let Some(anchor) = anchor else {
        return trades.to_vec();
    };
    let lo = window_start_ms.map(|s| anchor + s);
    let hi = window_end_ms.map(|e| anchor + e);
    trades
        .iter()
        .filter(|t| {
            let ts = t.block_time().timestamp_millis();
            lo.map_or(true, |lo| ts >= lo) && hi.map_or(true, |hi| ts <= hi)
        })
        .cloned()
        .collect()
}

/// Build the response from a windowed `CorpusTrade` slice — pure CPU, no I/O. The
/// shared [`build_swing1_funnel`] produces the legs + verdicts + latch (the same core
/// the backtest carries); entry + exit are resolved here the same way the sim does.
fn build_response(mint: String, trades: &[CorpusTrade], rule: &Swing1Rule) -> Swing1DetectResponse {
    let funnel = build_swing1_funnel(trades, rule);

    // Entry + exit (only if a gate is configured — else find_phase_entry bails).
    let mut entry_info = None;
    let mut exit_info = None;
    if funnel.gate_configured {
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
        mint_address: mint,
        trade_count: trades.len(),
        gate_configured: funnel.gate_configured,
        legs: funnel.legs,
        lows: funnel.lows,
        latch: funnel.latch,
        entry: entry_info,
        exit: exit_info,
    }
}

/// `POST /api/tokens/{mint}/swing1-detect` — see module docs.
pub async fn detect_token_swing1(
    _state: web::Data<Arc<LocalState>>,
    path: web::Path<String>,
    body: web::Json<Swing1DetectRequest>,
) -> impl Responder {
    let mint = path.into_inner();
    let Swing1DetectRequest { params, window_start_ms, window_end_ms, curve_only } =
        body.into_inner();
    let rule = params.to_rule();

    // Uncapped, full-history read from the SAME lake corpus the backtest prices on —
    // `curve_only` is applied at load (the projected `CorpusTrade` has no `venue`).
    let trades: Arc<Vec<CorpusTrade>> = match fetch_sim_history_one(&mint, curve_only).await {
        Ok(trades) => trades,
        Err(e) => {
            tracing::error!("lake trade fetch failed for swing1 detect {mint}: {e}");
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({ "error": "lake trade fetch failed" }));
        }
    };

    let result = web::block(move || {
        let windowed = window_corpus_trades(&trades, window_start_ms, window_end_ms);
        build_response(mint, &windowed, &rule)
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
