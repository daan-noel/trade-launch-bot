//! Time-travel debugger backend (plan 6.1) — load a recorded live event log, re-run
//! the pure engine [`reduce`](hunter_engine::reduce) over it, and dump every
//! `event → effects` decision as JSON.
//!
//! This is the offline reproduction of a live run: the log records every loggable
//! engine event ([`LoggedEvent`], the SSOT format shared with the live recorder), so
//! folding it through the same `reduce` the live loop uses reproduces the same
//! decisions. Two inputs are **not** in the log and are supplied here:
//! * **rules** — reloaded from PG (the log deliberately omits `RulesReloaded`), so an
//!   inspection replays the recorded events against the *current* rule set. A rule
//!   changed since the run will decide differently — that is the intended "what would
//!   this token do under today's rules" lens; note it when diffing against live.
//! * **ticks** — regenerable, so (like the replay driver and boot recovery) we
//!   interleave synthetic 500 ms `Tick`s on the [`TICK`] grid between logged event
//!   timestamps, letting quiet-token decisions (stall/dead/TP-on-tick) reproduce.
//!
//! Unlike [`crate::strategies::replay`] (which synthesizes fills because the lake has
//! none), the log already contains the real `FillConfirmed`/`FillFailed` events, so
//! this driver replays them verbatim — no sim fill model.
//!
//! **Slicing caveat:** the engine's concurrency/lifetime caps are *cross-token*, so a
//! faithful replay must fold the whole log against one [`EngineState`]. The `mint` /
//! time filters therefore narrow only the **output** — every event is still folded —
//! except `date`, which selects which day-files are loaded at all (a token created on
//! an earlier day won't be armed if that day's file is excluded).

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use serde::Serialize;

use hunter_engine::event::{
    Effect, Event, ExitReason, Fill, IntentId, LoadedRule, PositionId, PositionStatus, RuleId,
};
use hunter_engine::event::{ArmedStateTag, PositionDelta};
use hunter_engine::event_log::LoggedEvent;
use hunter_engine::fingerprint::Fingerprint as EngineFingerprint;
use hunter_engine::metrics::Ts;
use hunter_engine::{reduce, EngineState};

use crate::strategies::replay::TICK;

/// Default cap on returned steps (a full day's log can be large; the dump is meant
/// for focused inspection, so bound it and flag truncation).
pub const DEFAULT_MAX_STEPS: usize = 10_000;

/// Env: the directory the live recorder writes to (mirrors the live recorder's
/// default *and* its anchoring so an inspection with no explicit `dir` reads the
/// same place — see [`resolve_dir`]).
const ENV_DIR: &str = "EVENT_LOG_DIR";
const DEFAULT_DIR: &str = "event_log";

/// Output filters for an inspection run. `mint`/`since`/`until` narrow only the
/// dumped steps (every event is still folded — see the module "slicing caveat").
pub struct InspectConfig {
    /// Only dump steps that touch this mint (input or an effect references it).
    pub mint: Option<String>,
    /// Only dump steps at or after this instant.
    pub since: Option<Ts>,
    /// Only dump steps at or before this instant.
    pub until: Option<Ts>,
    /// Interleave synthetic ticks (default on) so tick-driven decisions reproduce.
    pub synthetic_ticks: bool,
    /// Stop recording once this many steps are dumped (`truncated` flags it).
    pub max_steps: usize,
}

/// Resolve the log directory from the request (`None`/empty ⇒ `EVENT_LOG_DIR` env,
/// else the `event_log` default) — the same resolution the live recorder uses.
/// A relative path (from the request *or* the env) anchors to the loaded `.env`'s
/// directory via [`trading_core::config::env_paths`], so the inspector reads the
/// exact directory the live bin wrote regardless of either process's CWD.
pub fn resolve_dir(req_dir: Option<&str>) -> PathBuf {
    match req_dir {
        Some(d) if !d.trim().is_empty() => trading_core::config::env_paths::resolve(d),
        _ => trading_core::config::dir_from_env(ENV_DIR, DEFAULT_DIR),
    }
}

/// Read the `events-YYYY-MM-DD.jsonl` day-files in `dir` (all, or just `date` if
/// given), oldest day first, returning the file names read plus the parsed events in
/// file order. Unparseable lines are skipped. (Filename convention mirrors the live
/// recorder in `live/src/strategies/engine/event_log.rs`.)
pub fn read_logs(dir: &Path, date: Option<&str>) -> std::io::Result<(Vec<String>, Vec<LoggedEvent>)> {
    let want: Option<NaiveDate> = date.and_then(|d| NaiveDate::parse_from_str(d.trim(), "%Y-%m-%d").ok());
    let mut files: Vec<(NaiveDate, PathBuf, String)> = Vec::new();
    for entry in std::fs::read_dir(dir)?.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(d) = name
            .strip_prefix("events-")
            .and_then(|s| s.strip_suffix(".jsonl"))
            .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
        else {
            continue;
        };
        if want.is_some_and(|w| w != d) {
            continue;
        }
        files.push((d, entry.path(), name.to_string()));
    }
    files.sort_by_key(|a| a.0);

    let mut names = Vec::with_capacity(files.len());
    let mut events = Vec::new();
    for (_, path, name) in files {
        names.push(name);
        let file = File::open(&path)?;
        for line in BufReader::new(file).lines().map_while(Result::ok) {
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<LoggedEvent>(&line) {
                Ok(ev) => events.push(ev),
                Err(e) => tracing::warn!("replay-inspect: skipping unparseable line in {name:?}: {e}",
                    name = path.display()),
            }
        }
    }
    Ok((names, events))
}

/// One serialized effect in the dump — a projection of [`Effect`] (which is not
/// itself `Serialize`), `effect`-tagged for a readable JSON shape.
#[derive(Serialize)]
#[serde(tag = "effect")]
enum InspectEffect {
    SubmitBuy { intent: IntentId, rule: RuleId, mint: String, lamports: u64 },
    SubmitSell {
        intent: IntentId,
        position: PositionId,
        reason: ExitReason,
        portion: hunter_engine::event::Portion,
    },
    PositionUpdate {
        position: PositionId,
        rule: RuleId,
        mint: String,
        status: PositionStatus,
        #[serde(skip_serializing_if = "Option::is_none")]
        fill: Option<Fill>,
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<ExitReason>,
        #[serde(skip_serializing_if = "Option::is_none")]
        intent: Option<IntentId>,
        #[serde(skip_serializing_if = "Option::is_none")]
        stage: Option<u8>,
    },
    ArmedChanged { mint: String, rule: RuleId, state: ArmedStateTag },
}

impl From<&Effect> for InspectEffect {
    fn from(fx: &Effect) -> Self {
        match fx {
            Effect::SubmitBuy { intent, rule, mint, lamports } => InspectEffect::SubmitBuy {
                intent: intent.clone(),
                rule: *rule,
                mint: mint.to_string(),
                lamports: *lamports,
            },
            Effect::SubmitSell { intent, position, reason, portion } => InspectEffect::SubmitSell {
                intent: intent.clone(),
                position: *position,
                reason: *reason,
                portion: *portion,
            },
            Effect::PositionUpdate(d) => InspectEffect::PositionUpdate {
                position: d.position,
                rule: d.rule,
                mint: d.mint.to_string(),
                status: d.status,
                fill: d.fill,
                reason: d.reason,
                intent: d.intent.clone(),
                stage: d.stage,
            },
            Effect::ArmedChanged(d) => InspectEffect::ArmedChanged {
                mint: d.mint.to_string(),
                rule: d.rule,
                state: d.state,
            },
        }
    }
}

/// One `event → effects` decision in the dump.
#[derive(Serialize)]
struct InspectStep {
    seq: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    at: Option<Ts>,
    /// The input event as JSON — a `LoggedEvent` (`{"Trade": {…}}`) or a synthetic
    /// `{"Tick": {"now": …}}`.
    event: serde_json::Value,
    effects: Vec<InspectEffect>,
}

/// The result of an inspection run — replay counts plus the (bounded) step dump.
#[derive(Serialize)]
pub struct InspectRun {
    pub rules_loaded: usize,
    pub fingerprints_loaded: usize,
    /// Logged events folded (excludes synthetic ticks).
    pub logged_events: usize,
    /// Synthetic ticks folded.
    pub synthetic_ticks: usize,
    /// Total events folded through `reduce` (logged + ticks).
    pub events_replayed: usize,
    pub steps_returned: usize,
    /// Recording stopped at `max_steps` before the log was exhausted.
    pub truncated: bool,
    steps: Vec<InspectStep>,
}

/// The fold driver: one [`EngineState`], synthetic-tick interleaving, and the
/// filtered step accumulator.
struct Inspector<'a> {
    state: EngineState,
    cfg: &'a InspectConfig,
    steps: Vec<InspectStep>,
    seq: usize,
    logged: usize,
    ticks: usize,
    /// The next synthetic tick instant on the grid (`None` until the first
    /// timestamped event anchors the grid).
    next_tick: Option<Ts>,
    truncated: bool,
}

impl<'a> Inspector<'a> {
    fn new(rules: &[LoadedRule], fps: &[EngineFingerprint], cfg: &'a InspectConfig) -> Self {
        let mut state = EngineState::new();
        reduce(
            &mut state,
            Event::RulesReloaded { rules: rules.to_vec().into(), fps: fps.to_vec().into() },
        );
        Self {
            state,
            cfg,
            steps: Vec::new(),
            seq: 0,
            logged: 0,
            ticks: 0,
            next_tick: None,
            truncated: false,
        }
    }

    /// Fold every logged event in order, interleaving synthetic ticks up to each
    /// event's timestamp.
    fn run(&mut self, events: Vec<LoggedEvent>) {
        for le in events {
            if self.truncated {
                return;
            }
            let at = le.at();
            if self.cfg.synthetic_ticks {
                if let Some(target) = at {
                    self.emit_ticks_until(target);
                }
            }
            self.fold_logged(le, at);
            // Anchor / advance the tick grid past this event's time.
            if let Some(a) = at {
                match self.next_tick {
                    None => self.next_tick = Some(a + TICK),
                    Some(nt) if nt <= a => {
                        // Realign to the first grid point strictly after `a`.
                        let behind = (a - nt).num_milliseconds() / TICK.num_milliseconds() + 1;
                        self.next_tick = Some(nt + TICK * (behind as i32));
                    }
                    Some(_) => {}
                }
            }
        }
    }

    /// Emit synthetic ticks on the grid for every point in `[next_tick, until)`.
    /// When no token is tracked, ticks are pure no-ops, so the whole quiet gap is
    /// skipped in O(1) (mirrors the replay driver's tail logic — bounds multi-hour
    /// quiet stretches in a day-file).
    fn emit_ticks_until(&mut self, until: Ts) {
        let Some(mut nt) = self.next_tick else { return };
        while nt < until && !self.truncated {
            if self.state.tokens.is_empty() {
                let gap_ms = (until - nt).num_milliseconds();
                let steps = gap_ms / TICK.num_milliseconds() + 1;
                nt += TICK * (steps as i32);
                break;
            }
            let now = nt;
            let effects = reduce(&mut self.state, Event::Tick { now });
            self.ticks += 1;
            self.record(Some(now), serde_json::json!({ "Tick": { "now": now } }), &effects);
            nt += TICK;
        }
        self.next_tick = Some(nt);
    }

    /// Fold one logged event and record the step if it passes the output filter.
    fn fold_logged(&mut self, le: LoggedEvent, at: Option<Ts>) {
        // Capture the mint touch + JSON before `into_event` consumes `le`.
        let input_touches = self
            .cfg
            .mint
            .as_deref()
            .map(|m| logged_touches_mint(&le, m));
        let value = serde_json::to_value(&le).unwrap_or(serde_json::Value::Null);
        let event = le.into_event();
        let effects = reduce(&mut self.state, event);
        self.logged += 1;
        self.record_filtered(at, value, &effects, input_touches);
    }

    /// Record a logged-event step, honoring the mint filter (input **or** any effect
    /// touching the mint qualifies).
    fn record_filtered(
        &mut self,
        at: Option<Ts>,
        value: serde_json::Value,
        effects: &[Effect],
        input_touches: Option<bool>,
    ) {
        if let Some(input_touches) = input_touches {
            let mint = self.cfg.mint.as_deref().unwrap_or_default();
            if !input_touches && !effects.iter().any(|fx| effect_touches_mint(fx, mint)) {
                return;
            }
        }
        self.record(at, value, effects);
    }

    /// Record a step if it passes the time filter and the cap isn't hit. For a tick,
    /// the mint filter is applied here (a tick's input touches no mint, but a
    /// tick-driven effect may) via [`record_filtered`] for logged events; ticks route
    /// through here after their own effect check below.
    fn record(&mut self, at: Option<Ts>, value: serde_json::Value, effects: &[Effect]) {
        // Tick steps carry no input mint; drop a tick under a mint filter unless one
        // of its effects touches the mint.
        if let Some(mint) = self.cfg.mint.as_deref() {
            let is_tick = value.get("Tick").is_some();
            if is_tick && !effects.iter().any(|fx| effect_touches_mint(fx, mint)) {
                return;
            }
        }
        // Time filter (an event without a timestamp is always kept — it can't be judged).
        if let Some(a) = at {
            if self.cfg.since.is_some_and(|s| a < s) || self.cfg.until.is_some_and(|u| a > u) {
                return;
            }
        }
        if self.steps.len() >= self.cfg.max_steps {
            self.truncated = true;
            return;
        }
        let step = InspectStep {
            seq: self.seq,
            at,
            event: value,
            effects: effects.iter().map(InspectEffect::from).collect(),
        };
        self.seq += 1;
        self.steps.push(step);
    }

    fn finish(self, rules_loaded: usize, fingerprints_loaded: usize) -> InspectRun {
        InspectRun {
            rules_loaded,
            fingerprints_loaded,
            logged_events: self.logged,
            synthetic_ticks: self.ticks,
            events_replayed: self.logged + self.ticks,
            steps_returned: self.steps.len(),
            truncated: self.truncated,
            steps: self.steps,
        }
    }
}

/// Replay `events` through the engine under `rules`/`fps`, dumping the filtered
/// `event → effects` steps.
pub fn inspect(
    rules: &[LoadedRule],
    fps: &[EngineFingerprint],
    events: Vec<LoggedEvent>,
    cfg: &InspectConfig,
) -> InspectRun {
    let mut driver = Inspector::new(rules, fps, cfg);
    driver.run(events);
    driver.finish(rules.len(), fps.len())
}

/// Whether a logged event references `mint` (directly, or via a fill/close intent).
fn logged_touches_mint(le: &LoggedEvent, mint: &str) -> bool {
    if le.mint() == Some(mint) {
        return true;
    }
    match le {
        LoggedEvent::FillConfirmed { intent, .. } | LoggedEvent::FillFailed { intent, .. } => {
            intent.mint.as_str() == mint
        }
        _ => false,
    }
}

/// Whether an effect references `mint`.
fn effect_touches_mint(fx: &Effect, mint: &str) -> bool {
    match fx {
        Effect::SubmitBuy { mint: m, .. } => m.as_str() == mint,
        Effect::SubmitSell { intent, .. } => intent.mint.as_str() == mint,
        Effect::PositionUpdate(PositionDelta { mint: m, .. }) => m.as_str() == mint,
        Effect::ArmedChanged(d) => d.mint.as_str() == mint,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone, Utc};
    use hunter_engine::event::{Fill, IntentId, Mint, RuleId, TradeMode};
    use hunter_engine::fingerprint::FingerprintId;
    use hunter_engine::grouping::TokenFingerprint;
    use hunter_engine::metrics::{Side, TradeLite};
    use hunter_engine::rule_params::RuleParams;
    use uuid::Uuid;

    fn base() -> Ts {
        Utc.timestamp_opt(1_700_000_000, 0).unwrap()
    }
    fn at(secs: f64) -> Ts {
        base() + Duration::milliseconds((secs * 1000.0) as i64)
    }

    fn fp(id: u128) -> EngineFingerprint {
        EngineFingerprint {
            id: FingerprintId(Uuid::from_u128(id)),
            cu_limit: Some(200_000),
            cu_price: None,
            ix_labels: None,
            init_buy_lamports: None,
            max_cost_lamports: None,
            spendable_lamports_in: None,
            first_slot_buy_lamports: None,
            first_slot_sell_lamports: None,
            bucket_size_amount: 0.1,
            metric_config: serde_json::json!({}),
        }
    }

    fn rule(params: serde_json::Value) -> LoadedRule {
        LoadedRule {
            id: RuleId(Uuid::from_u128(1)),
            fingerprint_id: FingerprintId(Uuid::from_u128(1)),
            trade_mode: TradeMode::Paper,
            buy_amount_lamports: 1_000_000_000,
            max_concurrent_tokens: 1,
            max_total_tokens: 0,
            params: RuleParams::parse(&params).unwrap(),
            entry_enabled: true,
        }
    }

    fn tf() -> TokenFingerprint {
        TokenFingerprint { cu_limit: Some(200_000), ..Default::default() }
    }

    fn cfg() -> InspectConfig {
        InspectConfig {
            mint: None,
            since: None,
            until: None,
            synthetic_ticks: true,
            max_steps: DEFAULT_MAX_STEPS,
        }
    }

    fn trade_ev(mint: &str, secs: f64, is_buy: bool, sol: f64, price: f64, reserve: f64) -> LoggedEvent {
        LoggedEvent::Trade {
            mint: Mint::from(mint),
            trade: TradeLite {
                side: if is_buy { Side::Buy } else { Side::Sell },
                sol,
                price,
                reserve_sol: reserve,
                at: at(secs),
                ..Default::default()
            },
        }
    }

    /// A minimal arm→enter→take-profit log reproduces the same decisions offline: at
    /// least one `SubmitBuy` and a `TakeProfit` sell appear in the dump.
    #[test]
    fn reproduces_arm_enter_take_profit() {
        let rules = [rule(serde_json::json!({ "take_profit": 100 }))];
        let fps = [fp(1)];
        let mint = "aaa";
        let events = vec![
            LoggedEvent::TokenCreated { mint: Mint::from(mint), fp: Box::new(tf()), at: at(0.0) , creator_wallet_hash: None},
            LoggedEvent::FirstSlotSettled {
                mint: Mint::from(mint),
                buy_lamports: 0,
                sell_lamports: 0,
                at: at(0.1),
            },
            trade_ev(mint, 0.2, true, 1.0, 1.0, 100.0),
            // The engine's SubmitBuy fill would arrive as a FillConfirmed in a real
            // log; supply it so the position reaches Holding. The intent `seq` must be
            // the one the engine derives for this buy (`(rule, mint, seq)`, monotonic) —
            // in a real log it's exactly what the recorder wrote, and the deterministic
            // replay regenerates the same value, so it matches. For this scenario the
            // engine assigns `seq = 1`.
            LoggedEvent::FillConfirmed {
                intent: IntentId { rule: RuleId(Uuid::from_u128(1)), mint: Mint::from(mint), seq: 1 },
                fill: Fill { price: 1.0, sol: 1.0, token_amount: 1_000_000, at: at(0.2) },
            },
            trade_ev(mint, 2.0, true, 1.0, 2.5, 100.0), // +150% > TP
        ];
        let run = inspect(&rules, &fps, events, &cfg());
        assert_eq!(run.rules_loaded, 1);
        assert!(run.logged_events >= 5);
        let dump = serde_json::to_string(&run.steps).unwrap();
        assert!(dump.contains("SubmitBuy"), "a buy decision should appear: {dump}");
        assert!(dump.contains("TakeProfit"), "a take-profit exit should appear: {dump}");
    }

    /// The mint filter narrows the dump to one token while still folding the whole
    /// stream (so a second token's cap pressure is honored, but its steps are hidden).
    #[test]
    fn mint_filter_narrows_output() {
        let rules = [rule(serde_json::json!({ "take_profit": 1000 }))];
        let fps = [fp(1)];
        let events = vec![
            LoggedEvent::TokenCreated { mint: Mint::from("aaa"), fp: Box::new(tf()), at: at(0.0) , creator_wallet_hash: None},
            LoggedEvent::TokenCreated { mint: Mint::from("bbb"), fp: Box::new(tf()), at: at(0.1) , creator_wallet_hash: None},
            trade_ev("aaa", 0.2, true, 1.0, 1.0, 100.0),
            trade_ev("bbb", 0.3, true, 1.0, 1.0, 100.0),
        ];
        let mut c = cfg();
        c.mint = Some("aaa".to_string());
        // No synthetic ticks: keeps the step set to just the logged aaa events.
        c.synthetic_ticks = false;
        let run = inspect(&rules, &fps, events, &c);
        assert!(run.logged_events >= 4, "all events are still folded");
        let dump = serde_json::to_string(&run.steps).unwrap();
        assert!(dump.contains("aaa"));
        assert!(!dump.contains("bbb"), "bbb steps are filtered out: {dump}");
    }

    /// Replaying the same log twice is byte-identical (determinism — the engine is
    /// pure and the driver is order-stable).
    #[test]
    fn inspect_is_deterministic() {
        let rules = [rule(serde_json::json!({ "take_profit": 100 }))];
        let fps = [fp(1)];
        let mk = || {
            vec![
                LoggedEvent::TokenCreated { mint: Mint::from("aaa"), fp: Box::new(tf()), at: at(0.0) , creator_wallet_hash: None},
                trade_ev("aaa", 0.2, true, 1.0, 1.0, 100.0),
                trade_ev("aaa", 2.0, true, 1.0, 2.5, 100.0),
            ]
        };
        let a = serde_json::to_string(&inspect(&rules, &fps, mk(), &cfg()).steps).unwrap();
        let b = serde_json::to_string(&inspect(&rules, &fps, mk(), &cfg()).steps).unwrap();
        assert_eq!(a, b);
    }

    /// The empty-tracked-set skip keeps a long quiet gap O(1): tokens that match **no**
    /// fingerprint are never tracked, so `state.tokens` stays empty and a 2-hour gap
    /// between two such tokens folds **zero** synthetic ticks (not ~14k).
    #[test]
    fn empty_state_skips_ticks() {
        let rules = [rule(serde_json::json!({ "take_profit": 100 }))];
        let fps = [fp(1)]; // matches cu_limit 200_000
        // Both tokens have a cu_limit that does NOT match the fingerprint → never armed,
        // never tracked. The 2-hour span must not emit a tick per 500 ms.
        let no_match = || TokenFingerprint { cu_limit: Some(999), ..Default::default() };
        let events = vec![
            LoggedEvent::TokenCreated { mint: Mint::from("aaa"), fp: Box::new(no_match()), at: at(0.0) , creator_wallet_hash: None},
            LoggedEvent::TokenCreated { mint: Mint::from("bbb"), fp: Box::new(no_match()), at: at(7200.0) , creator_wallet_hash: None},
        ];
        let run = inspect(&rules, &fps, events, &cfg());
        assert_eq!(run.synthetic_ticks, 0, "no tracked token ⇒ no ticks folded");
    }

    /// A pathological log where an entry is submitted but its fill never resolves
    /// (truncated/corrupt log) leaves the token active forever — the tick fold must
    /// still terminate, bounded by `max_steps` (the compute-runaway backstop).
    #[test]
    fn unresolved_entry_is_bounded_by_max_steps() {
        let rules = [rule(serde_json::json!({ "take_profit": 100 }))];
        let fps = [fp(1)];
        // Enter-on-arm, but no FillConfirmed ever arrives → the arm stays EntryPending
        // (non-terminal) and the token is never pruned; without the cap the tick loop
        // would run to the far-future event.
        let events = vec![
            LoggedEvent::TokenCreated { mint: Mint::from("aaa"), fp: Box::new(tf()), at: at(0.0) , creator_wallet_hash: None},
            trade_ev("aaa", 0.2, true, 1.0, 1.0, 100.0),
            // A far-future event the tick grid would otherwise march to one 500 ms step
            // at a time (2 h ⇒ ~14k ticks) if the cap didn't stop it first.
            trade_ev("aaa", 7200.0, true, 1.0, 1.1, 100.0),
        ];
        let mut c = cfg();
        c.max_steps = 50;
        let run = inspect(&rules, &fps, events, &c);
        assert!(run.truncated, "the cap engaged");
        assert!(run.steps_returned <= 50, "output bounded: {}", run.steps_returned);
        assert!(run.synthetic_ticks <= 50, "fold stopped at the cap: {}", run.synthetic_ticks);
    }
}
