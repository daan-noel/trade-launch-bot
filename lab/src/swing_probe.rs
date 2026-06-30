//! `lab swing-probe` — a diagnostic for the swing1 zero-fire problem.
//!
//! Loads the Parquet lake (the **same** corpus source the grouped sweep uses) and,
//! for the first N tokens of a selection, prints the entry funnel stage by stage:
//!
//!   trades → raw legs → swing-LOW legs → per-low kill/volume verdicts → phase
//!   latch → higher-low confirmation → resolved entry
//!
//! so we can see exactly *which* stage drops every token to zero fires. It runs the
//! identical pure fns (`detect_swing_legs_raw`, `classify_phase`, `find_phase_entry`)
//! the sweep runs, against a single, explicit `Swing1Rule` you can tweak here — no
//! DB, no HTTP, no sweep engine. Read-only.

use anyhow::Context;
use chrono::{DateTime, Utc};

use trading_core::config;
use trading_core::models::trade::TradeRow;
use trading_core::models::Swing1Rule;
use trading_core::strategies::swing_1::{
    classifier::{self, LowFeatures},
    entry::find_phase_entry,
    phase_profile_from_rule,
    rule_configures_any_entry_gate,
    swing::{detect_swing_legs_raw, SwingType},
    swing_params_from_rule,
};

use crate::lake::duck::LakeSource;
use crate::sweep::corpus::{CorpusSource, Selection, TradeWindow};

/// The probe rule — mirrors the second sweep's defaults (SOL floor off, pct
/// reversal widened, volume gate loosened). Edit these to bisect the blocker.
fn probe_rule() -> Swing1Rule {
    let mut r = Swing1Rule::new(
        "swing-probe".into(),
        None,
        None,
        None,
        serde_json::json!([]),
        "paper".into(),
        0.05, // buy_amount_sol
        100.0, // p_exit_take_profit
        50.0,  // p_exit_stop_loss
        None,
        None,
        None,
        None,
        None,
        None,        // p_exit_trailing_stop_pct
        None,        // p_exit_time_stop_secs
        Some(60),    // p_exit_stall_secs
        None,        // p_exit_liquidity_drop_pct
    );
    // Swing reversal: SOL floor OFF (0 ⇒ no bound), pct widened to catch small
    // early curve swings.
    r.p_swing_high_to_low_sol = Some(0.0);
    r.p_swing_low_to_high_sol = Some(0.0);
    r.p_swing_high_to_low_pct = Some(15.0);
    r.p_swing_low_to_high_pct = Some(15.0);
    r.p_swing_min_leg_trades = None;
    // Kill profile.
    r.p_kill_depth_min_pct = Some(0.4);
    r.p_kill_max_duration_ms = Some(10_000);
    r.p_kill_min_net_flow_per_sec = None;
    // Volume profile loosened: duration gate OFF, deeper "shallow" allowed.
    r.p_vol_depth_max_pct = Some(0.6);
    r.p_vol_min_duration_ms = None;
    r.p_vol_min_up_duration_ms = None;
    r.p_min_kills_before_volume = Some(0);
    // Entry confirmation.
    r.p_entry_pullback_pct = Some(10.0);
    r.p_entry_higher_low_secs = None;
    r.p_entry_max_age_secs = None;
    r.p_entry_min_liquidity_sol = None;
    r.p_entry_max_cohort_held = None;
    // Exit next-kill.
    r.p_exit_next_kill_depth_min_pct = None;
    r.p_exit_next_kill_max_duration_ms = Some(8_000);
    r
}

/// `lab swing-probe [N] [created_after_rfc3339]` — diagnose the entry funnel for
/// the first `N` tokens (default 8) of the lake selection.
pub async fn run(n_tokens: usize, created_after: Option<DateTime<Utc>>) -> anyhow::Result<()> {
    // Validate config (lake root via env) — no DB pool needed, the lake is files.
    let _settings = config::Settings::from_env_local().context("config load")?;
    let root = crate::lake::lake_root();
    println!("swing-probe: lake = {}", root.display());

    let sel = Selection {
        mints: None,
        token_cap: n_tokens.max(1),
        created_after,
        created_before: None,
        per_mint_cap: crate::sweep::corpus::sweep_per_mint_cap(),
        window: TradeWindow::LaunchWindow,
        curve_only: true,
    };
    let corpus = LakeSource::new(root).load(&sel).await.context("lake load")?;
    println!(
        "swing-probe: loaded {} token(s), {} trade(s) total\n",
        corpus.token_count(),
        corpus.trade_count()
    );

    let rule = probe_rule();
    let gate_ok = rule_configures_any_entry_gate(&rule);
    let sparams = swing_params_from_rule(&rule);
    let profile = phase_profile_from_rule(&rule);
    println!("rule_configures_any_entry_gate = {gate_ok}");
    println!("swing reversal: h2l_sol={} h2l_pct={} l2h_sol={} l2h_pct={} min_leg_trades={}",
        sparams.high_to_low_threshold_sol, sparams.high_to_low_threshold_pct,
        sparams.low_to_high_threshold_sol, sparams.low_to_high_threshold_pct,
        sparams.min_leg_trades);
    println!("phase profile: kill_depth_min={} kill_max_dur_ms={} vol_depth_max={} vol_min_dur_ms={} min_kills={}\n",
        profile.kill_depth_min_pct, profile.kill_max_duration_ms,
        profile.vol_depth_max_pct, profile.vol_min_duration_ms,
        profile.min_kills_before_volume);

    if !gate_ok {
        println!("!! entry gate NOT configured — find_phase_entry bails immediately. Fix the rule.");
        return Ok(());
    }

    for tok in corpus.tokens.iter().take(n_tokens) {
        let trades = tok.trades.as_ref();
        let legs = detect_swing_legs_raw(trades, &sparams);
        let lows = legs.iter().filter(|l| l.leg_type == SwingType::SwingLow).count();
        let highs = legs.len() - lows;

        println!("── {} ({} trades)", tok.mint, trades.len());
        println!("   legs={} (highs={}, lows={})", legs.len(), highs, lows);

        // Per-swing-low verdicts (the gates that drive the latch).
        let mut prev_up: Option<i64> = None;
        let mut any_kill = false;
        let mut any_vol = false;
        for leg in &legs {
            match leg.leg_type {
                SwingType::SwingHigh => prev_up = Some(leg.duration_ms),
                SwingType::SwingLow => {
                    if let Some(f) = LowFeatures::from_low(leg) {
                        let is_kill = profile.is_kill_low(&f);
                        let is_vol = profile.is_volume_low(&f, prev_up);
                        any_kill |= is_kill;
                        any_vol |= is_vol;
                        println!(
                            "     low: depth={:.3} dur_ms={} flow/s={:.3} trades={} pivot={:.6} → kill={} vol={}",
                            f.depth_pct, f.duration_ms, f.net_flow_per_sec, f.trade_count,
                            f.pivot_price, is_kill, is_vol
                        );
                    }
                    prev_up = None;
                }
            }
        }
        println!("   any_kill_low={any_kill}  any_volume_low={any_vol}");

        let latch = classifier::classify_phase(&legs, &profile);
        println!(
            "   latch: volume_phase_latched={} latched_leg_index={:?} kills_seen={}",
            latch.volume_phase_latched, latch.latched_leg_index, latch.kills_seen
        );

        match find_phase_entry(trades, &rule) {
            Some((idx, fill)) => {
                // Full precision + the sweep's own `fill.price > 0.0` gate verdict,
                // plus the trigger trade's raw price_per_token vs chart spot, so we
                // can see whether the fill price is a true zero or just tiny.
                let trig = &trades[idx];
                println!(
                    "   ENTRY idx {idx}: fill.price={:.12} (sweep_fires={}) | trigger ppt={:.12} chart_spot={:?}\n",
                    fill.price,
                    fill.price > 0.0,
                    trig.price_per_token(),
                    trig.chart_spot_price()
                );
            }
            None => println!("   no entry\n"),
        }
    }

    Ok(())
}
