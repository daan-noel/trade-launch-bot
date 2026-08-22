//! `RuleParams` — the typed form of `strategy_rules.params` (JSONB), the "WHEN
//! it trades" half of a generic rule. Canonical JSON shape (design SSOT:
//! `hunter/docs/arch/strategies.md`):
//!
//! ```json
//! {
//!   "take_profit": 100,
//!   "stop_loss": 30,
//!   "entry": {
//!     "m_snapshot":   { "time": [{"operator": ">", "value": 10}], ... },
//!     "m_flow_window": { "window_size_sec": 10, "gross_flow": [ ... ] }
//!   },
//!   "exit": { ... }
//! }
//! ```
//!
//! Group objects mix **strict params** (e.g. `window_size_sec`) with **metric
//! condition lists** at the same level, so parsing is a registry-guided walk
//! rather than a plain derive: every group / strict-param / metric / operator
//! name is checked against [`crate::metrics::REGISTRY`], so a typo fails the
//! save instead of silently never matching.
//!
//! **Multi-window per group.** A dynamic group (`m_flow_window`, `m_price_window`,
//! `m_flow_split_window`) may appear once as a plain object (one window — the legacy,
//! backward-compatible shape every stored rule uses) OR as a JSON **array** of
//! objects, each with its own `window_size_sec`, to place gates on the same group
//! at several window sizes simultaneously (e.g. a 30 s `gross_flow` hot gate AND a
//! 2 s `net_flow` exhaustion gate on entry):
//!
//! ```json
//! "m_flow_window": [
//!   { "window_size_sec": 30, "gross_flow": [{"operator": ">=", "value": 10}] },
//!   { "window_size_sec": 2,  "net_flow":   [{"operator": ">=", "value": 0}]  }
//! ]
//! ```
//!
//! Each window is an independent clause (entry-AND / exit-OR, same combinator as
//! across groups). Windows within a group must be distinct; static groups take a
//! single object only. The single-object form round-trips byte-identically, so no
//! DB migration and existing rules are untouched.
//!
//! Absent = unconstrained: `entry: None` ⇒ enter on arm (fingerprint alone);
//! `exit: None` ⇒ TP/SL/death only; absent TP/SL ⇒ that guard is off.
//!
//! **Parked (disabled) conditions.** The optional root key `disabled` holds an
//! `entry` / `exit` pair in the *same* [`SideConditions`] shape plus a `scale_out`
//! ladder in the *same* [`ExitStage`] shape, for conditions and stages the author
//! has toggled off in the editor but wants to keep:
//!
//! ```json
//! "disabled": {
//!   "entry": { "m_snapshot": { "liquidity": [{"operator": ">", "value": 30}] } },
//!   "scale_out": [{ "sell_bps": 5000, "take_profit": 40 }]
//! }
//! ```
//!
//! It is parsed and validated exactly like a live side/stage — so re-enabling a
//! parked condition can never produce a rule that fails to save — but **nothing
//! compiles it**: [`crate::arm::CompiledRule`] reads `entry`/`exit`/`scale_out`
//! only, so the engine, the sweep, and simulate are untouched and the hot path pays
//! nothing. A parked condition may duplicate a live one (same group/window/metric) —
//! that is the point of the feature (park `trail >= 12` while trying `trail >= 20`),
//! and the two live in separate bags so neither overwrites the other. Absent by
//! default, so every stored rule round-trips byte-identically (no migration).
//!
//! The one deliberate asymmetry: a parked stage is validated **per stage** only.
//! The cross-stage rules (`remainder must be last`, the explicit-stage count, the
//! `sell_bps` sum) describe a *ladder*, and the parked bag is a shelf of spare
//! stages, not a ladder — summing them would make parking a stage useless (the
//! ladder it was parked out of would still be over budget).
//!
//! Parse **once at rule load** into these structs — never per event.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::metrics::evaluator::{
    check_expr_satisfiable, condition_expr_to_value, normalize_condition_expr,
    parse_condition_expr, ConditionExpr,
};
use crate::metrics::position::is_trailing;
use crate::metrics::{
    group_by_name, group_spec, metric_spec, MetricGroupId, MetricId, MetricScope, REGISTRY,
};

/// Hard cap on explicit (non-remainder) scale-out stages. Fee cost (~1% notional
/// per extra leg at 0.1 SOL) and in-flight-sell blindness both argue against
/// unbounded ladders — raise only when measurement demands it.
pub const MAX_EXPLICIT_SCALE_STAGES: usize = 3;

/// Max `sell_bps` on an explicit stage, and max sum of all explicit stages — a
/// remainder must exist for whatever closes the stub (remainder stage or global
/// exit). `9900` = 99% of the initial bag.
pub const MAX_SCALE_SELL_BPS: u16 = 9900;

/// Ceiling on [`RuleParams::buy_pct_of_vsol`], in percent of the pool's SOL reserve.
///
/// A rail on real money rather than a view on strategy: this figure multiplies a live
/// pool balance into a buy size, so an authoring slip has to stay survivable. The
/// wallets this feature exists to imitate size at 1.2-1.9% of vsol, and our own impact
/// is `buy / vsol` exactly — 10% is already a 10% self-inflicted move.
pub const MAX_BUY_PCT_OF_VSOL: f64 = 10.0;

/// Typed, registry-checked `params`. See module docs for the JSON shape.
/// `default()` is the legal empty rule (fingerprint-only, no TP/SL/conditions).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RuleParams {
    /// Take-profit as % of entry price (e.g. `100` = +100%). `None` = off.
    pub take_profit: Option<f64>,
    /// Stop-loss as % drop from entry (e.g. `30` = −30%). `None` = off.
    pub stop_loss: Option<f64>,
    /// Entry conditions. `None` = enter on arm (the fingerprint alone decides).
    pub entry: Option<SideConditions>,
    /// Exit conditions. `None` = TP/SL/death only.
    pub exit: Option<SideConditions>,
    /// Ordered partial-exit ladder. `None` / empty = no scale-out (legacy full
    /// close only). See [`ExitStage`] and `docs/plans/strategies/partial-exits.md`.
    pub scale_out: Option<Vec<ExitStage>>,
    /// Size the buy as a percent of the pool's SOL reserve instead of a fixed amount.
    /// `None` (every stored rule) = use the rule's `buy_amount_lamports`.
    ///
    /// Why it exists: `impact_per_leg = buy / vsol` exactly, so a fixed size inside a
    /// liquidity band that spans 2× moves our own price by 2× as much at one end as at
    /// the other. Sizing as a fraction of the pool holds impact constant — which is how
    /// the wallets worth copying size, and why their realised slippage is flat.
    /// Resolved at the entry decision against the depth the fold already holds, so
    /// live-real, live-paper and simulate get one answer. See
    /// `docs/plans/strategies/execution-costs.md`.
    pub buy_pct_of_vsol: Option<f64>,
    /// Re-entry lifecycle. `None` = one-shot (`Done` is terminal per (token, rule) —
    /// every stored rule's behavior). See [`ReEntry`].
    pub reentry: Option<ReEntry>,
    /// Single-position-per-token exclusivity. `false` (default, every stored rule)
    /// = today's behavior — this rule ignores what other rules hold. `true` = skip
    /// entry while **any** other arm on the token holds a position (in-flight buys
    /// and sells included, manual arms included), for rules meant to compete for an
    /// opportunity rather than stack on it.
    pub exclusive: bool,
    /// Tiebreak when two `exclusive` rules would enter the same token on the same
    /// event: higher wins (it is visited first, claims the token, and every later
    /// exclusive rule then sees the claim). Ties fall back to rule-id order.
    /// Meaningless on a non-exclusive rule.
    pub priority: i32,
    /// Conditions the author toggled OFF but kept (see module docs). Validated like
    /// a live side, compiled by nothing. `None` (every stored rule) = nothing parked.
    pub disabled: Option<DisabledConditions>,
}

/// The parked half of a rule: entry/exit conditions and scale-out stages kept but
/// not evaluated. Same shape as the live sides/ladder so a toggle is a move between
/// the two bags, nothing more.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DisabledConditions {
    /// Parked entry-side conditions (`None`/empty = none parked).
    pub entry: Option<SideConditions>,
    /// Parked exit-side conditions (`None`/empty = none parked).
    pub exit: Option<SideConditions>,
    /// Parked scale-out stages (`None`/empty = none parked). Order is the authoring
    /// order the editor showed them in; it carries no execution meaning here — only
    /// the live [`RuleParams::scale_out`] ladder is ordered.
    pub scale_out: Option<Vec<ExitStage>>,
}

impl DisabledConditions {
    /// Whether anything at all is parked (an empty bag ≡ absent, and never reaches
    /// storage — [`RuleParams::to_value`] omits it).
    pub fn is_empty(&self) -> bool {
        let empty = |s: &Option<SideConditions>| s.as_ref().is_none_or(SideConditions::is_empty);
        empty(&self.entry)
            && empty(&self.exit)
            && self.scale_out.as_ref().is_none_or(Vec::is_empty)
    }
}

/// One ordered scale-out stage. Fires once while `stage == k`, sells
/// [`sell_bps`](Self::sell_bps) of the **initial** bag (or `All` when `None` —
/// the remainder stage), then advances. Reuses the full exit grammar: per-stage
/// TP sugar + [`SideConditions`] (incl. `arm_above_pct`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExitStage {
    /// Basis points of the initial bag to sell. `None` = remainder / `All`
    /// (must be last; closes the position outright under its own conditions).
    pub sell_bps: Option<u16>,
    /// Per-stage take-profit sugar (`pnl >= tp`), desugared at compile like the
    /// global field. `None` = off for this stage.
    pub take_profit: Option<f64>,
    /// Authored metric conditions for this stage. Empty + no `take_profit` is
    /// rejected at validate — a stage must be able to fire.
    pub conditions: SideConditions,
}

/// Optional re-entry lifecycle (plan Ph4). Absent ⇒ one-shot. Present ⇒ after a
/// **normal strategy exit** (TP / SL / a metric exit — never Dead / Manual /
/// Migrated) the (token, rule) re-arms: it waits `cooldown_sec` then returns to
/// `Armed`, up to `max_episodes_per_token` entries. The dip scalper's edge is rapid
/// re-entry (family median gap ~31 s, up to 31 episodes/token); this expresses it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReEntry {
    /// Seconds to wait after a close before re-arming (`>= 0`; `0` ⇒ next event).
    pub cooldown_sec: f64,
    /// Hard cap on entries per token for this rule (`>= 1`). The Nth close re-arms
    /// only while the episode count stays below this.
    pub max_episodes_per_token: u32,
}

/// One side's (entry or exit) metric-condition groups. Each group maps to one or
/// more [`GroupConditions`] **instances** — a static group has exactly one; a
/// dynamic group has one per distinct `window_size_sec` (multi-window per group).
/// The instance list is kept sorted by window (ascending; a static group's single
/// window-less instance sorts first) so parsing is authoring-order-independent and
/// [`to_value`](RuleParams::to_value) round-trips deterministically.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SideConditions(pub BTreeMap<MetricGroupId, Vec<GroupConditions>>);

/// One group instance's authored content: strict params (its `window_size_sec`)
/// beside per-metric condition exprs (DNF: OR of AND-arms within a metric; across
/// metrics/groups/windows the side combinator is entry-AND / exit-OR — see `arm.rs`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GroupConditions {
    /// Strict (non-condition) params, e.g. `window_size_sec` — names validated
    /// against the group's registry entry.
    pub strict: BTreeMap<String, f64>,
    /// DNF condition arms per metric of this group.
    pub metrics: BTreeMap<MetricId, ConditionExpr>,
}

impl GroupConditions {
    /// A strict param's value by JSON name (`None` = not set).
    pub fn strict_param(&self, name: &str) -> Option<f64> {
        self.strict.get(name).copied()
    }
}

impl SideConditions {
    /// Whether this side constrains anything (an empty side ≡ absent).
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl RuleParams {
    /// `entry` absent/empty ⇒ arming the (token, rule) IS the entry signal.
    pub fn enter_on_arm(&self) -> bool {
        self.entry.as_ref().is_none_or(SideConditions::is_empty)
    }

    /// `exit` absent/empty ⇒ only TP / SL / death close the position.
    pub fn exit_is_tp_sl_only(&self) -> bool {
        self.exit.as_ref().is_none_or(SideConditions::is_empty)
    }

    /// Parse **and validate** stored/authored params JSON. This is the one
    /// entry point rule save and rule load share — everything it returns is
    /// registry-checked and semantically sane.
    pub fn parse(json: &Value) -> Result<Self, String> {
        let parsed = Self::parse_shape(json)?;
        parsed.validate()?;
        Ok(parsed)
    }

    /// Serialize back to the canonical JSONB shape (inverse of [`Self::parse`]).
    pub fn to_value(&self) -> Value {
        let mut root = Map::new();
        if let Some(tp) = self.take_profit {
            root.insert("take_profit".into(), tp.into());
        }
        if let Some(sl) = self.stop_loss {
            root.insert("stop_loss".into(), sl.into());
        }
        if let Some(side) = &self.entry {
            root.insert("entry".into(), side_to_value(side));
        }
        if let Some(side) = &self.exit {
            root.insert("exit".into(), side_to_value(side));
        }
        if let Some(stages) = &self.scale_out {
            if !stages.is_empty() {
                root.insert(
                    "scale_out".into(),
                    Value::Array(stages.iter().map(stage_to_value).collect()),
                );
            }
        }
        if let Some(re) = self.reentry {
            let mut o = Map::new();
            o.insert("cooldown_sec".into(), re.cooldown_sec.into());
            o.insert(
                "max_episodes_per_token".into(),
                (re.max_episodes_per_token as u64).into(),
            );
            root.insert("reentry".into(), Value::Object(o));
        }
        // An empty parked bag is the same as none — never store the ambiguous state.
        if let Some(d) = self.disabled.as_ref().filter(|d| !d.is_empty()) {
            let mut o = Map::new();
            for (key, side) in [("entry", &d.entry), ("exit", &d.exit)] {
                if let Some(side) = side.as_ref().filter(|s| !s.is_empty()) {
                    o.insert(key.into(), side_to_value(side));
                }
            }
            if let Some(stages) = d.scale_out.as_ref().filter(|s| !s.is_empty()) {
                o.insert(
                    "scale_out".into(),
                    Value::Array(stages.iter().map(stage_to_value).collect()),
                );
            }
            root.insert("disabled".into(), Value::Object(o));
        }
        if let Some(pct) = self.buy_pct_of_vsol {
            root.insert("buy_pct_of_vsol".into(), pct.into());
        }
        // Defaults stay absent so every stored rule round-trips byte-identically.
        if self.exclusive {
            root.insert("exclusive".into(), Value::Bool(true));
        }
        if self.priority != 0 {
            root.insert("priority".into(), (self.priority as i64).into());
        }
        Value::Object(root)
    }

    // ── Shape walk (names + JSON types) ────────────────────────────────────

    fn parse_shape(json: &Value) -> Result<Self, String> {
        let obj = json.as_object().ok_or("params must be a JSON object")?;
        for key in obj.keys() {
            if !matches!(
                key.as_str(),
                "take_profit"
                    | "stop_loss"
                    | "entry"
                    | "exit"
                    | "scale_out"
                    | "reentry"
                    | "exclusive"
                    | "priority"
                    | "disabled"
                    | "buy_pct_of_vsol"
            ) {
                return Err(format!("unknown params key '{key}'"));
            }
        }
        Ok(RuleParams {
            take_profit: parse_opt_number(obj.get("take_profit"), "take_profit")?,
            stop_loss: parse_opt_number(obj.get("stop_loss"), "stop_loss")?,
            entry: parse_opt_side(obj.get("entry"), "entry")?,
            exit: parse_opt_side(obj.get("exit"), "exit")?,
            scale_out: parse_opt_scale_out(obj.get("scale_out"), "scale_out")?,
            reentry: parse_opt_reentry(obj.get("reentry"))?,
            exclusive: parse_opt_bool(obj.get("exclusive"), "exclusive")?,
            priority: parse_opt_priority(obj.get("priority"))?,
            disabled: parse_opt_disabled(obj.get("disabled"))?,
            buy_pct_of_vsol: parse_opt_number(
                obj.get("buy_pct_of_vsol"),
                "buy_pct_of_vsol",
            )?,
        })
    }

    // ── Semantic validation (values + satisfiability) ──────────────────────

    fn validate(&self) -> Result<(), String> {
        for (name, v) in [("take_profit", self.take_profit), ("stop_loss", self.stop_loss)] {
            if let Some(v) = v {
                if !v.is_finite() || v <= 0.0 {
                    return Err(format!("{name} must be a finite number > 0"));
                }
            }
        }
        // The ceiling is a safety rail on REAL money, not a modelling opinion: this
        // number multiplies a live pool balance into a buy size, so a fat-fingered
        // `50` would spend half the pool. The wallets worth copying size at 1-2% of
        // vsol, so `MAX_BUY_PCT_OF_VSOL` sits an order of magnitude above them and
        // still far below anything that could drain an account.
        if let Some(pct) = self.buy_pct_of_vsol {
            if !pct.is_finite() || pct <= 0.0 || pct > MAX_BUY_PCT_OF_VSOL {
                return Err(format!(
                    "buy_pct_of_vsol must be a finite number in (0, {MAX_BUY_PCT_OF_VSOL}]"
                ));
            }
        }
        // Parked sides validate exactly like live ones (`is_entry` and all), so a
        // toggle back on can never turn a saved rule into an unsavable one.
        let d = self.disabled.as_ref();
        for (label, is_entry, side) in [
            ("entry", true, self.entry.as_ref()),
            ("exit", false, self.exit.as_ref()),
            ("disabled.entry", true, d.and_then(|d| d.entry.as_ref())),
            ("disabled.exit", false, d.and_then(|d| d.exit.as_ref())),
        ] {
            let Some(side) = side else { continue };
            for (group_id, instances) in &side.0 {
                validate_group_instances(label, is_entry, *group_id, instances)?;
            }
        }
        if let Some(stages) = &self.scale_out {
            validate_scale_out(stages)?;
        }
        // Parked stages: per-stage rules only (see module docs — the cross-stage
        // rules describe a ladder, and this bag is a shelf).
        for (i, stage) in d.and_then(|d| d.scale_out.as_ref()).into_iter().flatten().enumerate() {
            validate_stage(&format!("disabled.scale_out[{i}]"), stage)?;
        }
        Ok(())
    }
}

/// Parse the optional `disabled` bag — the parked `entry`/`exit` sides and
/// `scale_out` stages. Reuses [`parse_opt_side`] / [`parse_opt_scale_out`] verbatim
/// (one grammar, one registry walk); an all-empty bag folds to `None`, the same
/// sentinel discipline as `scale_out: []`.
fn parse_opt_disabled(v: Option<&Value>) -> Result<Option<DisabledConditions>, String> {
    match v {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Object(o)) => {
            for k in o.keys() {
                if !matches!(k.as_str(), "entry" | "exit" | "scale_out") {
                    return Err(format!("disabled: unknown key '{k}'"));
                }
            }
            let d = DisabledConditions {
                entry: parse_opt_side(o.get("entry"), "disabled.entry")?,
                exit: parse_opt_side(o.get("exit"), "disabled.exit")?,
                scale_out: parse_opt_scale_out(o.get("scale_out"), "disabled.scale_out")?,
            };
            Ok(if d.is_empty() { None } else { Some(d) })
        }
        Some(_) => Err("disabled must be an object".into()),
    }
}

/// Parse the optional `reentry` object. Validated inline (it is a small nested
/// object, unlike the flat TP/SL numbers `validate()` handles): `cooldown_sec`
/// finite and `>= 0`; `max_episodes_per_token` an integer `>= 1`.
fn parse_opt_reentry(v: Option<&Value>) -> Result<Option<ReEntry>, String> {
    match v {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Object(o)) => {
            for k in o.keys() {
                if !matches!(k.as_str(), "cooldown_sec" | "max_episodes_per_token") {
                    return Err(format!("reentry: unknown key '{k}'"));
                }
            }
            let cooldown_sec = o
                .get("cooldown_sec")
                .and_then(Value::as_f64)
                .ok_or("reentry.cooldown_sec must be a number")?;
            if !cooldown_sec.is_finite() || cooldown_sec < 0.0 {
                return Err("reentry.cooldown_sec must be a finite number >= 0".into());
            }
            let max = o
                .get("max_episodes_per_token")
                .and_then(Value::as_f64)
                .ok_or("reentry.max_episodes_per_token must be a number")?;
            if !max.is_finite() || max < 1.0 || max.fract() != 0.0 {
                return Err("reentry.max_episodes_per_token must be an integer >= 1".into());
            }
            Ok(Some(ReEntry {
                cooldown_sec,
                max_episodes_per_token: max as u32,
            }))
        }
        Some(_) => Err("reentry must be an object".into()),
    }
}

/// Parse a stage ladder — the live `scale_out` or the parked `disabled.scale_out`
/// (`path` is the error-message prefix, the only difference between the two). Empty
/// array folds to `None` (same sentinel as [`crate::fingerprint::configured_labels`]
/// — never leave `Some([])` in storage). Shape validation of counts/bps happens in
/// [`validate_scale_out`] / [`validate_stage`].
fn parse_opt_scale_out(v: Option<&Value>, path: &str) -> Result<Option<Vec<ExitStage>>, String> {
    match v {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(arr)) if arr.is_empty() => Ok(None),
        Some(Value::Array(arr)) => {
            let stages = arr
                .iter()
                .enumerate()
                .map(|(i, item)| parse_exit_stage(item, &format!("{path}[{i}]")))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Some(stages))
        }
        Some(_) => Err(format!("{path} must be an array")),
    }
}

fn parse_exit_stage(v: &Value, prefix: &str) -> Result<ExitStage, String> {
    let obj = v
        .as_object()
        .ok_or_else(|| format!("{prefix} must be an object"))?;
    for key in obj.keys() {
        if !matches!(key.as_str(), "sell_bps" | "take_profit" | "conditions") {
            return Err(format!("{prefix}: unknown key '{key}'"));
        }
    }
    let sell_bps = match obj.get("sell_bps") {
        None | Some(Value::Null) => None,
        Some(Value::Number(n)) => {
            let f = n
                .as_f64()
                .ok_or_else(|| format!("{prefix}.sell_bps is not a valid number"))?;
            if !f.is_finite() || f.fract() != 0.0 || f < 1.0 || f > MAX_SCALE_SELL_BPS as f64 {
                return Err(format!(
                    "{prefix}.sell_bps must be an integer in [1, {MAX_SCALE_SELL_BPS}]"
                ));
            }
            Some(f as u16)
        }
        Some(_) => return Err(format!("{prefix}.sell_bps must be a number")),
    };
    let take_profit = parse_opt_number(obj.get("take_profit"), &format!("{prefix}.take_profit"))?;
    let conditions = match obj.get("conditions") {
        None | Some(Value::Null) => SideConditions::default(),
        Some(c) => parse_opt_side(Some(c), &format!("{prefix}.conditions"))?
            .unwrap_or_default(),
    };
    Ok(ExitStage { sell_bps, take_profit, conditions })
}

fn validate_scale_out(stages: &[ExitStage]) -> Result<(), String> {
    if stages.is_empty() {
        return Ok(());
    }
    let mut explicit = 0usize;
    let mut sum_bps: u32 = 0;
    for (i, stage) in stages.iter().enumerate() {
        let prefix = format!("scale_out[{i}]");
        match stage.sell_bps {
            Some(bps) => {
                if i > 0 && stages[i - 1].sell_bps.is_none() {
                    return Err(format!(
                        "{prefix}: remainder stage (sell_bps omitted) must be last"
                    ));
                }
                explicit += 1;
                sum_bps += bps as u32;
            }
            None => {
                if i + 1 != stages.len() {
                    return Err(format!(
                        "{prefix}: remainder stage (sell_bps omitted) must be last"
                    ));
                }
            }
        }
        validate_stage(&prefix, stage)?;
    }
    if explicit > MAX_EXPLICIT_SCALE_STAGES {
        return Err(format!(
            "scale_out: at most {MAX_EXPLICIT_SCALE_STAGES} explicit stages \
             (+ optional remainder); got {explicit}"
        ));
    }
    if sum_bps > MAX_SCALE_SELL_BPS as u32 {
        return Err(format!(
            "scale_out: sum of explicit sell_bps must be <= {MAX_SCALE_SELL_BPS} \
             (got {sum_bps}) — a remainder must exist for the stub"
        ));
    }
    Ok(())
}

/// The rules that describe ONE stage in isolation — shared by the live ladder and
/// the parked bag, which is the whole reason re-enabling a parked stage can never
/// produce a rule that refuses to save. (The cross-stage rules stay in
/// [`validate_scale_out`]: they are properties of a ladder, not of a stage.)
fn validate_stage(prefix: &str, stage: &ExitStage) -> Result<(), String> {
    if let Some(tp) = stage.take_profit {
        if !tp.is_finite() || tp <= 0.0 {
            return Err(format!("{prefix}.take_profit must be a finite number > 0"));
        }
    }
    // A stage must be able to fire: TP sugar and/or non-empty conditions.
    if stage.take_profit.is_none() && stage.conditions.is_empty() {
        return Err(format!(
            "{prefix}: stage needs take_profit and/or non-empty conditions"
        ));
    }
    for (group_id, instances) in &stage.conditions.0 {
        // Stages are exit-like — position metrics are legal, hence `is_entry: false`.
        validate_group_instances(&format!("{prefix}.conditions"), false, *group_id, instances)?;
    }
    Ok(())
}

fn stage_to_value(stage: &ExitStage) -> Value {
    let mut obj = Map::new();
    if let Some(bps) = stage.sell_bps {
        obj.insert("sell_bps".into(), (bps as u64).into());
    }
    if let Some(tp) = stage.take_profit {
        obj.insert("take_profit".into(), tp.into());
    }
    if !stage.conditions.is_empty() {
        obj.insert("conditions".into(), side_to_value(&stage.conditions));
    }
    Value::Object(obj)
}

fn parse_opt_bool(v: Option<&Value>, name: &str) -> Result<bool, String> {
    match v {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(format!("{name} must be a boolean")),
    }
}

/// Parse the optional `priority` tiebreak. Validated inline (an integer in `i32`
/// range) rather than in `validate()`, which handles the flat `f64` fields.
fn parse_opt_priority(v: Option<&Value>) -> Result<i32, String> {
    match v {
        None | Some(Value::Null) => Ok(0),
        Some(Value::Number(_)) => {
            let n = v.and_then(Value::as_f64).unwrap_or(f64::NAN);
            if !n.is_finite() || n.fract() != 0.0 || n < i32::MIN as f64 || n > i32::MAX as f64 {
                return Err("priority must be an integer within i32 range".into());
            }
            Ok(n as i32)
        }
        Some(_) => Err("priority must be a number".into()),
    }
}

fn parse_opt_number(v: Option<&Value>, name: &str) -> Result<Option<f64>, String> {
    match v {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => {
            n.as_f64().map(Some).ok_or_else(|| format!("{name} is not a valid number"))
        }
        Some(_) => Err(format!("{name} must be a number")),
    }
}

fn parse_opt_side(v: Option<&Value>, side_name: &str) -> Result<Option<SideConditions>, String> {
    match v {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Object(groups)) => {
            let mut side = SideConditions::default();
            for (group_name, group_val) in groups {
                let spec = group_by_name(group_name).ok_or_else(|| {
                    format!("{side_name}: unknown metric group '{group_name}'")
                })?;
                // A group is one object (single window — the legacy shape) or an
                // array of objects (one per window). Both land as an instance list.
                let mut instances: Vec<GroupConditions> = match group_val {
                    Value::Array(arr) => {
                        if arr.is_empty() {
                            return Err(format!(
                                "{side_name}.{group_name}: empty group (expected an object \
                                 or a non-empty array of window clauses)"
                            ));
                        }
                        arr.iter()
                            .map(|item| parse_group_object(spec, item, side_name, group_name))
                            .collect::<Result<_, _>>()?
                    }
                    Value::Object(_) => {
                        vec![parse_group_object(spec, group_val, side_name, group_name)?]
                    }
                    _ => {
                        return Err(format!(
                            "{side_name}.{group_name} must be an object or an array of objects"
                        ))
                    }
                };
                // Sort by window (ascending; window-less static instance sorts first)
                // so the stored order is authoring-order-independent + round-trips.
                instances.sort_by(|a, b| {
                    let key = |g: &GroupConditions| g.strict_param("window_size_sec");
                    key(a)
                        .partial_cmp(&key(b))
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                side.0.insert(spec.id, instances);
            }
            Ok(Some(side))
        }
        Some(_) => Err(format!("{side_name} must be an object")),
    }
}

/// Parse one group **instance** object: strict params (e.g. `window_size_sec`)
/// beside per-metric condition lists, both registry-checked.
fn parse_group_object(
    spec: &crate::metrics::GroupSpec,
    group_val: &Value,
    side_name: &str,
    group_name: &str,
) -> Result<GroupConditions, String> {
    let group_obj = group_val
        .as_object()
        .ok_or_else(|| format!("{side_name}.{group_name} must be an object"))?;
    let mut group = GroupConditions::default();
    for (key, val) in group_obj {
        if spec.strict_param_by_name(key).is_some() {
            let n = val
                .as_f64()
                .ok_or_else(|| format!("{side_name}.{group_name}.{key} must be a number"))?;
            group.strict.insert(key.clone(), n);
        } else if let Some(m) = spec.metric_by_name(key) {
            let arms = parse_condition_expr(val).map_err(|e| {
                format!(
                    "{side_name}.{group_name}.{key}: invalid condition list \
                     (expect [{{\"operator\": \">\", \"value\": 10}}, ...] \
                     or nested OR arms): {e}"
                )
            })?;
            // Same metric, different ops: AND if feasible, else OR.
            group
                .metrics
                .insert(m.id, normalize_condition_expr(arms, m.eq_tolerance));
        } else {
            // Better message when the metric exists in another group.
            let owner = REGISTRY
                .iter()
                .find(|g| g.metric_by_name(key).is_some())
                .map(|g| g.name);
            return Err(match owner {
                Some(owner) => format!(
                    "{side_name}.{group_name}: metric '{key}' belongs to group '{owner}'"
                ),
                None => {
                    format!("{side_name}.{group_name}: unknown metric or param '{key}'")
                }
            });
        }
    }
    Ok(group)
}

/// Validate a group's instance list: shape rules that span the instances (a static
/// group takes exactly one; dynamic windows must be distinct), then each instance.
///
/// `side_name` is only the error-message path; `is_entry` is the semantic question
/// ("do position-scoped metrics make sense here?") — they diverge for `scale_out`
/// stages (exit-like under an `scale_out[i]` path) and for the parked `disabled.entry`
/// bag (entry-like under a `disabled.` path).
fn validate_group_instances(
    side_name: &str,
    is_entry: bool,
    group_id: MetricGroupId,
    instances: &[GroupConditions],
) -> Result<(), String> {
    use crate::metrics::MetricKind;
    let spec = group_spec(group_id);
    if instances.is_empty() {
        return Err(format!("{side_name}.{}: group has no instances", spec.name));
    }
    if spec.kind == MetricKind::Static && instances.len() > 1 {
        return Err(format!(
            "{side_name}.{}: static group takes a single object (no windows to distinguish)",
            spec.name
        ));
    }
    // Distinct windows within a dynamic group — two clauses on the same window are
    // ambiguous (they'd read the one buffer); combine them into one object instead.
    if spec.kind == MetricKind::Dynamic {
        for (i, a) in instances.iter().enumerate() {
            for b in &instances[i + 1..] {
                let (wa, wb) = (a.strict_param("window_size_sec"), b.strict_param("window_size_sec"));
                if let (Some(wa), Some(wb)) = (wa, wb) {
                    if (wa - wb).abs() <= f64::EPSILON {
                        return Err(format!(
                            "{side_name}.{}: duplicate window_size_sec {wa} — combine into one clause",
                            spec.name
                        ));
                    }
                }
            }
        }
    }
    for group in instances {
        validate_group(side_name, is_entry, group_id, group)?;
    }
    Ok(())
}

fn validate_group(
    side_name: &str,
    is_entry: bool,
    group_id: MetricGroupId,
    group: &GroupConditions,
) -> Result<(), String> {
    let spec = group_spec(group_id);
    // Position-scoped metrics anchor on YOUR entry fill — they have no value until a
    // position is held, so they are exit-only (pre-entry they read NaN and could
    // never satisfy an entry condition anyway; reject at save with a clear message).
    if is_entry && spec.scope == MetricScope::Position {
        return Err(format!(
            "{side_name}.{}: position-scoped metrics only exist while holding — exit side only",
            spec.name
        ));
    }
    // Required strict params present; all strict values finite and > 0.
    for p in spec.strict_params {
        if p.required && !group.strict.contains_key(p.name) {
            return Err(format!(
                "{side_name}.{}: missing required param '{}'",
                spec.name, p.name
            ));
        }
    }
    for (name, v) in &group.strict {
        // `allows_zero` params take `>= 0` — `0` is a real value of their domain
        // (`arm_above_pct: 0` = arm the trailing stop at break-even), distinct from
        // the param being absent, which is what means "off".
        let allows_zero = spec.strict_param_by_name(name).is_some_and(|p| p.allows_zero);
        let ok = v.is_finite() && (*v > 0.0 || (allows_zero && *v == 0.0));
        if !ok {
            let bound = if allows_zero { ">= 0" } else { "> 0" };
            return Err(format!(
                "{side_name}.{}.{name} must be a finite number {bound}",
                spec.name
            ));
        }
    }
    // A group with no metric conditions constrains nothing — reject the no-op.
    if group.metrics.is_empty() {
        return Err(format!(
            "{side_name}.{}: group has no metric conditions",
            spec.name
        ));
    }
    // `arm_above_pct` gates only the trailing metrics; authored without one it is a
    // silent no-op. Same principle as an unknown metric name: fail the save.
    if group.strict.contains_key("arm_above_pct")
        && !group.metrics.keys().copied().any(is_trailing)
    {
        return Err(format!(
            "{side_name}.{}: arm_above_pct gates the trailing metrics (retrace / bounce) \
             — authored without one it does nothing",
            spec.name
        ));
    }
    for (metric_id, arms) in &group.metrics {
        let m = metric_spec(*metric_id);
        if arms.is_empty() {
            return Err(format!(
                "{side_name}.{}.{}: empty condition list",
                spec.name, m.name
            ));
        }
        for (ai, arm) in arms.iter().enumerate() {
            if arm.is_empty() {
                return Err(format!(
                    "{side_name}.{}.{}: empty OR arm {ai}",
                    spec.name, m.name
                ));
            }
            for c in arm {
                if !c.value.is_finite() {
                    return Err(format!(
                        "{side_name}.{}.{}: condition value must be finite",
                        spec.name, m.name
                    ));
                }
            }
        }
        check_expr_satisfiable(arms, m.eq_tolerance).map_err(|why| {
            format!(
                "{side_name}.{}.{}: contradictory conditions ({why})",
                spec.name, m.name
            )
        })?;
    }
    Ok(())
}

fn side_to_value(side: &SideConditions) -> Value {
    let mut groups = Map::new();
    for (group_id, instances) in &side.0 {
        // One instance ⇒ the legacy object shape (byte-identical for stored rules);
        // several ⇒ a JSON array of window clauses.
        let value = match instances.as_slice() {
            [only] => group_to_value(only),
            many => Value::Array(many.iter().map(group_to_value).collect()),
        };
        groups.insert(group_spec(*group_id).name.to_string(), value);
    }
    Value::Object(groups)
}

/// Serialize one group instance back to its JSON object (strict params + per-metric
/// condition lists).
fn group_to_value(group: &GroupConditions) -> Value {
    let mut obj = Map::new();
    for (name, v) in &group.strict {
        obj.insert(name.clone(), (*v).into());
    }
    for (metric_id, arms) in &group.metrics {
        obj.insert(
            metric_spec(*metric_id).name.to_string(),
            condition_expr_to_value(arms),
        );
    }
    Value::Object(obj)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::evaluator::{Condition, Operator};
    use serde_json::json;

    /// The canonical params example — the shape documented in the module header
    /// above, pinned here so a serde change that breaks it fails a test.
    fn docs_example() -> Value {
        let side = json!({
            "m_snapshot": {
                "time": [
                    {"operator": ">", "value": 10},
                    {"operator": "<", "value": 30}
                ],
                "liquidity": [ {"operator": "=", "value": 20} ]
            },
            "m_price_lifetime": {
                "stall": [ {"operator": "<", "value": 10} ],
                "trail": [ {"operator": "<", "value": 10} ]
            },
            "m_flow_window": {
                "window_size_sec": 10,
                "gross_flow": [ {"operator": "=", "value": 15} ],
                "net_flow": [ {"operator": "=", "value": 5} ],
                "buy": [ {"operator": "=", "value": 10} ],
                "sell": [ {"operator": "=", "value": 5} ]
            }
        });
        json!({
            "take_profit": 100,
            "stop_loss": 30,
            "entry": side,
            "exit": side
        })
    }

    #[test]
    fn docs_example_round_trips() {
        let parsed = RuleParams::parse(&docs_example()).expect("docs example is valid");
        assert_eq!(parsed.take_profit, Some(100.0));
        assert_eq!(parsed.stop_loss, Some(30.0));
        let entry = parsed.entry.as_ref().unwrap();
        assert_eq!(entry.0.len(), 3);
        let tw = &entry.0[&MetricGroupId::FlowWindow][0];
        assert_eq!(tw.strict_param("window_size_sec"), Some(10.0));
        assert_eq!(
            tw.metrics[&MetricId::GrossFlow],
            vec![vec![Condition { operator: Operator::Eq, value: 15.0 }]]
        );
        // Full round-trip: to_value → parse → identical struct.
        let reparsed = RuleParams::parse(&parsed.to_value()).unwrap();
        assert_eq!(reparsed, parsed);
    }

    #[test]
    fn absent_sides_mean_unconstrained() {
        let p = RuleParams::parse(&json!({"take_profit": 50})).unwrap();
        assert!(p.enter_on_arm(), "entry: None ⇒ arm is enter");
        assert!(p.exit_is_tp_sl_only(), "exit: None ⇒ TP/SL only");
        assert_eq!(p.stop_loss, None);

        // Empty side objects behave like absent sides.
        let p = RuleParams::parse(&json!({"entry": {}, "exit": {}})).unwrap();
        assert!(p.enter_on_arm());
        assert!(p.exit_is_tp_sl_only());
        // And a fully-empty params object is legal: fingerprint-only rule.
        assert!(RuleParams::parse(&json!({})).is_ok());
    }

    #[test]
    fn unknown_names_rejected() {
        // Unknown top-level key.
        let e = RuleParams::parse(&json!({"take_profits": 1})).unwrap_err();
        assert!(e.contains("unknown params key"), "{e}");
        // Unknown group.
        let e = RuleParams::parse(&json!({"entry": {"m_snapshots": {}}})).unwrap_err();
        assert!(e.contains("unknown metric group"), "{e}");
        // Unknown metric within a known group.
        let e = RuleParams::parse(
            &json!({"entry": {"m_snapshot": {"tyme": [{"operator": ">", "value": 1}]}}}),
        )
        .unwrap_err();
        assert!(e.contains("unknown metric or param 'tyme'"), "{e}");
        // Unknown operator.
        let e = RuleParams::parse(
            &json!({"entry": {"m_snapshot": {"time": [{"operator": "=>", "value": 1}]}}}),
        )
        .unwrap_err();
        assert!(e.contains("invalid condition list"), "{e}");
    }

    #[test]
    fn metric_under_wrong_group_names_the_right_one() {
        let e = RuleParams::parse(
            &json!({"exit": {"m_snapshot": {"stall": [{"operator": "<", "value": 10}]}}}),
        )
        .unwrap_err();
        assert!(e.contains("belongs to group 'm_price_lifetime'"), "{e}");
    }

    #[test]
    fn position_group_is_exit_only() {
        // Exit side: the trailing stop is fine and round-trips.
        let p = RuleParams::parse(&json!({
            "exit": { "m_position": { "retrace": [{ "operator": ">=", "value": 3 }] } }
        }))
        .expect("m_position on exit is valid");
        assert_eq!(RuleParams::parse(&p.to_value()).unwrap(), p);

        // Entry side: position metrics only exist while holding → rejected clearly.
        let e = RuleParams::parse(&json!({
            "entry": { "m_position": { "retrace": [{ "operator": ">=", "value": 3 }] } }
        }))
        .unwrap_err();
        assert!(e.contains("exit side only"), "{e}");
    }

    #[test]
    fn arm_above_pct_parses_round_trips_and_rejects_no_ops() {
        // The armed trailing stop: retrace 3% off the since-entry peak, but only
        // once the position has cleared 2% (the pump.fun round-trip fee).
        let p = RuleParams::parse(&json!({
            "exit": { "m_position": {
                "retrace": [{ "operator": ">=", "value": 3 }],
                "arm_above_pct": 2
            } }
        }))
        .expect("armed trailing stop is valid");
        let g = &p.exit.as_ref().unwrap().0[&MetricGroupId::Position][0];
        assert_eq!(g.strict_param("arm_above_pct"), Some(2.0));
        assert_eq!(RuleParams::parse(&p.to_value()).unwrap(), p);

        // `0` is a real value here (arm at break-even), so it must survive the
        // strict-param validator that rejects `<= 0` everywhere else.
        assert!(RuleParams::parse(&json!({
            "exit": { "m_position": {
                "retrace": [{ "operator": ">=", "value": 3 }], "arm_above_pct": 0
            } }
        }))
        .is_ok());
        // ...while `window_size_sec: 0` still is not.
        let e = RuleParams::parse(&json!({
            "entry": { "m_flow_window": {
                "window_size_sec": 0, "gross_flow": [{ "operator": ">=", "value": 1 }]
            } }
        }))
        .unwrap_err();
        assert!(e.contains("must be a finite number > 0"), "{e}");

        // Authored without a trailing metric it silently does nothing — reject it,
        // the same way an unknown metric name is rejected rather than ignored.
        let e = RuleParams::parse(&json!({
            "exit": { "m_position": {
                "pnl": [{ "operator": "<=", "value": -10 }], "arm_above_pct": 2
            } }
        }))
        .unwrap_err();
        assert!(e.contains("retrace / bounce"), "{e}");

        // And it stays absent from the serialized shape when unset, so every stored
        // rule round-trips byte-identically (no migration).
        let plain = RuleParams::parse(&json!({
            "exit": { "m_position": { "retrace": [{ "operator": ">=", "value": 3 }] } }
        }))
        .unwrap();
        assert!(plain.to_value()["exit"]["m_position"]
            .get("arm_above_pct")
            .is_none());
    }

    #[test]
    fn price_window_group_parses_and_round_trips() {
        // The dip trigger: entry `m_price_window(30).trail >= 12`. Auto-surfaces via
        // the registry walk with no dedicated code (extensibility contract).
        let p = RuleParams::parse(&json!({
            "entry": { "m_price_window": {
                "window_size_sec": 30,
                "trail": [{ "operator": ">=", "value": 12 }],
                "rise":  [{ "operator": ">=", "value": 0 }]
            } }
        }))
        .expect("m_price_window rule is valid");
        let g = &p.entry.as_ref().unwrap().0[&MetricGroupId::PriceWindow][0];
        assert_eq!(g.strict_param("window_size_sec"), Some(30.0));
        assert_eq!(
            g.metrics[&MetricId::WinTrail],
            vec![vec![Condition { operator: Operator::Gte, value: 12.0 }]]
        );
        // Full round-trip: to_value → parse → identical struct.
        assert_eq!(RuleParams::parse(&p.to_value()).unwrap(), p);

        // Dynamic group ⇒ window_size_sec required, same as m_flow_window.
        let e = RuleParams::parse(&json!({
            "entry": { "m_price_window": { "trail": [{ "operator": ">=", "value": 12 }] } }
        }))
        .unwrap_err();
        assert!(e.contains("missing required param 'window_size_sec'"), "{e}");
    }

    #[test]
    fn flow_window_requires_window_size() {
        let e = RuleParams::parse(
            &json!({"entry": {"m_flow_window": {"buy": [{"operator": ">", "value": 1}]}}}),
        )
        .unwrap_err();
        assert!(e.contains("missing required param 'window_size_sec'"), "{e}");

        // And the strict value must be positive/finite.
        let e = RuleParams::parse(&json!({"entry": {"m_flow_window": {
            "window_size_sec": 0,
            "buy": [{"operator": ">", "value": 1}]
        }}}))
        .unwrap_err();
        assert!(e.contains("window_size_sec must be a finite number > 0"), "{e}");
    }

    #[test]
    fn multi_window_group_parses_dedupes_and_round_trips() {
        // The lever-#1 shape: a 30 s gross_flow hot gate AND a 2 s net_flow
        // exhaustion gate on the same group, same side — impossible under the old
        // single-object shape. Authored out of window order to prove the sort.
        let p = RuleParams::parse(&json!({
            "entry": { "m_flow_window": [
                { "window_size_sec": 30, "gross_flow": [{"operator": ">=", "value": 10}] },
                { "window_size_sec": 2,  "net_flow":   [{"operator": ">=", "value": 0}]  }
            ] }
        }))
        .expect("multi-window entry is valid");
        let instances = &p.entry.as_ref().unwrap().0[&MetricGroupId::FlowWindow];
        assert_eq!(instances.len(), 2);
        // Sorted by window ascending: 2 s first, then 30 s.
        assert_eq!(instances[0].strict_param("window_size_sec"), Some(2.0));
        assert_eq!(instances[1].strict_param("window_size_sec"), Some(30.0));
        assert!(instances[0].metrics.contains_key(&MetricId::NetFlow));
        assert!(instances[1].metrics.contains_key(&MetricId::GrossFlow));

        // Round-trips: multi-window serializes as an array, single stays an object.
        let rv = p.to_value();
        assert!(rv["entry"]["m_flow_window"].is_array(), "multi-window ⇒ array");
        assert_eq!(RuleParams::parse(&rv).unwrap(), p);

        // A single-instance group still serializes as a plain object (no migration).
        let one = RuleParams::parse(&json!({
            "entry": { "m_flow_window": { "window_size_sec": 30, "gross_flow": [{"operator": ">=", "value": 10}] } }
        }))
        .unwrap();
        assert!(one.to_value()["entry"]["m_flow_window"].is_object(), "one window ⇒ object");
        assert_eq!(RuleParams::parse(&one.to_value()).unwrap(), one);
    }

    #[test]
    fn multi_window_rejects_duplicates_and_static_arrays() {
        // Two clauses on the same window are ambiguous.
        let e = RuleParams::parse(&json!({
            "entry": { "m_flow_window": [
                { "window_size_sec": 30, "gross_flow": [{"operator": ">=", "value": 10}] },
                { "window_size_sec": 30, "net_flow":   [{"operator": ">=", "value": 0}]  }
            ] }
        }))
        .unwrap_err();
        assert!(e.contains("duplicate window_size_sec 30"), "{e}");

        // A static group has no window to distinguish instances — array > 1 rejected.
        let e = RuleParams::parse(&json!({
            "entry": { "m_snapshot": [
                { "time": [{"operator": ">=", "value": 10}] },
                { "liquidity": [{"operator": ">=", "value": 5}] }
            ] }
        }))
        .unwrap_err();
        assert!(e.contains("static group takes a single object"), "{e}");

        // An empty array is a no-op group — rejected with a clear message.
        let e = RuleParams::parse(&json!({ "entry": { "m_flow_window": [] } })).unwrap_err();
        assert!(e.contains("empty group"), "{e}");

        // A static group authored as a 1-element array is fine (degenerate array form).
        assert!(RuleParams::parse(&json!({
            "entry": { "m_snapshot": [{ "time": [{"operator": ">=", "value": 10}] }] }
        }))
        .is_ok());
    }

    #[test]
    fn tp_sl_must_be_positive() {
        for bad in [json!({"take_profit": 0}), json!({"stop_loss": -5})] {
            let e = RuleParams::parse(&bad).unwrap_err();
            assert!(e.contains("> 0"), "{e}");
        }
    }

    #[test]
    fn same_metric_unsat_and_normalizes_to_or() {
        // Flat `< 30, >= 70` (legacy AND) → OR arms, promotable / savable.
        let p = RuleParams::parse(&json!({"exit": {"m_snapshot": {"liquidity": [
            {"operator": "<", "value": 30},
            {"operator": ">=", "value": 70}
        ]}}}))
        .unwrap();
        let arms = &p.exit.as_ref().unwrap().0[&MetricGroupId::Snapshot][0].metrics
            [&MetricId::Liquidity];
        assert_eq!(arms.len(), 2);

        // Crossed time bounds similarly become OR (not rejected).
        let p = RuleParams::parse(&json!({"entry": {"m_snapshot": {"time": [
            {"operator": ">", "value": 30},
            {"operator": "<", "value": 10}
        ]}}}))
        .unwrap();
        assert_eq!(
            p.entry.as_ref().unwrap().0[&MetricGroupId::Snapshot][0].metrics[&MetricId::Time].len(),
            2
        );
    }

    #[test]
    fn same_metric_feasible_and_stays_and() {
        // > 10 AND >= 10 AND < 30 is fine (redundant, not contradictory).
        assert!(RuleParams::parse(&json!({"entry": {"m_snapshot": {"time": [
            {"operator": ">", "value": 10},
            {"operator": ">=", "value": 10},
            {"operator": "<", "value": 30}
        ]}}}))
        .is_ok());

        // >= 10 AND <= 10 is the single point 10 — satisfiable AND.
        let p = RuleParams::parse(&json!({"entry": {"m_snapshot": {"time": [
            {"operator": ">=", "value": 10},
            {"operator": "<=", "value": 10}
        ]}}}))
        .unwrap();
        assert_eq!(
            p.entry.as_ref().unwrap().0[&MetricGroupId::Snapshot][0].metrics[&MetricId::Time].len(),
            1
        );

        // Explicit nested OR arms left as authored.
        assert!(RuleParams::parse(&json!({"exit": {"m_snapshot": {"liquidity": [
            [{"operator": "<", "value": 30}],
            [{"operator": ">=", "value": 70}]
        ]}}}))
        .is_ok());
    }

    #[test]
    fn multi_arm_all_unsat_still_rejected() {
        // Explicit `|` of two unsatisfiable AND arms — normalize does not expand
        // multi-arm exprs, so the whole metric stays contradictory.
        let e = RuleParams::parse(&json!({"entry": {"m_snapshot": {"time": [
            [{"operator": ">", "value": 30}, {"operator": "<", "value": 10}],
            [{"operator": ">", "value": 50}, {"operator": "<", "value": 20}]
        ]}}}))
        .unwrap_err();
        assert!(e.contains("contradictory"), "{e}");
    }

    #[test]
    fn empty_lists_and_no_op_groups_rejected() {
        let e = RuleParams::parse(&json!({"entry": {"m_snapshot": {"time": []}}}))
            .unwrap_err();
        assert!(e.contains("empty condition list"), "{e}");

        // A group carrying only its strict param constrains nothing.
        let e = RuleParams::parse(
            &json!({"entry": {"m_flow_window": {"window_size_sec": 10}}}),
        )
        .unwrap_err();
        assert!(e.contains("no metric conditions"), "{e}");
    }

    #[test]
    fn non_finite_condition_value_rejected() {
        // JSON can't carry NaN/inf, so exercise validate() directly.
        let mut group = GroupConditions::default();
        group.metrics.insert(
            MetricId::Time,
            vec![vec![Condition {
                operator: Operator::Gt,
                value: f64::NAN,
            }]],
        );
        let mut side = SideConditions::default();
        side.0.insert(MetricGroupId::Snapshot, vec![group]);
        let p = RuleParams {
            take_profit: None,
            stop_loss: None,
            entry: Some(side),
            exit: None,
            scale_out: None,
            reentry: None,
            exclusive: false,
            priority: 0,
            disabled: None,
            buy_pct_of_vsol: None,
        };
        let e = p.validate().unwrap_err();
        assert!(e.contains("must be finite"), "{e}");
    }

    #[test]
    fn disabled_bag_parses_round_trips_and_is_never_compiled() {
        // The park-a-gate shape: a live `trail >= 20` entry beside the parked
        // `trail >= 12` it replaced — SAME group, SAME window, same metric. That is
        // the whole point, and the two bags keep them from overwriting each other.
        let p = RuleParams::parse(&json!({
            "entry": { "m_price_window": { "window_size_sec": 30, "trail": [{"operator": ">=", "value": 20}] } },
            "disabled": {
                "entry": { "m_price_window": { "window_size_sec": 30, "trail": [{"operator": ">=", "value": 12}] } },
                "exit":  { "m_position": { "retrace": [{"operator": ">=", "value": 3}] } }
            }
        }))
        .expect("parked conditions are valid");
        let d = p.disabled.as_ref().expect("bag present");
        assert_eq!(
            d.entry.as_ref().unwrap().0[&MetricGroupId::PriceWindow][0].metrics
                [&MetricId::WinTrail],
            vec![vec![Condition { operator: Operator::Gte, value: 12.0 }]]
        );
        assert!(d.exit.as_ref().unwrap().0.contains_key(&MetricGroupId::Position));
        assert_eq!(RuleParams::parse(&p.to_value()).unwrap(), p);

        // The live sides — the only thing the engine compiles — are untouched by it.
        assert!(!p.enter_on_arm());
        assert!(p.exit_is_tp_sl_only(), "a parked exit is NOT an exit");

        // Parking every entry condition really does mean enter-on-arm. The engine
        // reads that straight off the live side; nothing consults the bag.
        let all_parked = RuleParams::parse(&json!({
            "disabled": { "entry": { "m_snapshot": { "time": [{"operator": ">", "value": 10}] } } }
        }))
        .unwrap();
        assert!(all_parked.enter_on_arm());
    }

    #[test]
    fn disabled_scale_out_parks_stages_without_touching_the_live_ladder() {
        // Park two stages that could never coexist in ONE ladder (a second remainder,
        // and a bps sum way over the cap) beside a live ladder that is fine. This is
        // the shelf-not-a-ladder contract: parking a stage must actually free its
        // budget, otherwise the toggle does nothing useful.
        let p = RuleParams::parse(&json!({
            "scale_out": [{ "sell_bps": 7000, "take_profit": 50 }],
            "disabled": { "scale_out": [
                { "sell_bps": 9900, "take_profit": 40 },
                { "take_profit": 120 },
                { "conditions": { "m_position": { "retrace": [{"operator": ">=", "value": 8}] } } }
            ] }
        }))
        .expect("parked stages are validated per stage, not as a ladder");

        // The live ladder — the only thing the engine compiles — is exactly as authored.
        let live = p.scale_out.as_ref().expect("live ladder");
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].sell_bps, Some(7000));
        let parked = p.disabled.as_ref().unwrap().scale_out.as_ref().unwrap();
        assert_eq!(parked.len(), 3);
        assert_eq!(parked[1].take_profit, Some(120.0));
        assert_eq!(RuleParams::parse(&p.to_value()).unwrap(), p);

        // Per-stage rules DO apply, so a parked stage can always be switched back on.
        for (bad, needle) in [
            (
                json!({ "disabled": { "scale_out": [{ "sell_bps": 5000 }] } }),
                "stage needs take_profit and/or non-empty conditions",
            ),
            (
                json!({ "disabled": { "scale_out": [{ "sell_bps": 99999, "take_profit": 10 }] } }),
                "sell_bps must be an integer in [1, 9900]",
            ),
            (
                json!({ "disabled": { "scale_out": [
                    { "take_profit": 10, "conditions": { "m_price_window": { "trail": [{"operator": ">=", "value": 1}] } } }
                ] } }),
                "missing required param 'window_size_sec'",
            ),
            (json!({ "disabled": { "scale_out": {} } }), "disabled.scale_out must be an array"),
        ] {
            let e = RuleParams::parse(&bad).unwrap_err();
            assert!(e.contains(needle), "expected '{needle}' in: {e}");
            assert!(e.contains("disabled.scale_out"), "path must name the bag: {e}");
        }
    }

    #[test]
    fn disabled_bag_defaults_absent_and_empty_folds_to_none() {
        // Absent by default, and never serialized when unset ⇒ every stored rule
        // round-trips byte-identically (no migration).
        let p = RuleParams::parse(&json!({ "take_profit": 50 })).unwrap();
        assert_eq!(p.disabled, None);
        assert!(p.to_value().get("disabled").is_none());

        // An empty / all-empty bag is the same sentinel as `scale_out: []` → None.
        for empty in [
            json!({ "disabled": {} }),
            json!({ "disabled": { "entry": {}, "exit": {} } }),
            json!({ "disabled": { "scale_out": [] } }),
        ] {
            let p = RuleParams::parse(&empty).unwrap();
            assert_eq!(p.disabled, None, "{empty}");
            assert!(p.to_value().get("disabled").is_none());
        }
    }

    #[test]
    fn disabled_bag_validates_like_a_live_side() {
        // Entry-scope rule applies under `disabled.entry` too — otherwise toggling a
        // parked condition back on would produce a rule that refuses to save.
        let e = RuleParams::parse(&json!({
            "disabled": { "entry": { "m_position": { "retrace": [{"operator": ">=", "value": 3}] } } }
        }))
        .unwrap_err();
        assert!(e.contains("disabled.entry") && e.contains("exit side only"), "{e}");

        // ...and the ordinary per-group rules (unknown names, no-op groups,
        // contradictions, required strict params) all still fire under the bag.
        for (bad, needle) in [
            (json!({ "disabled": { "exit": { "m_snapshots": {} } } }), "unknown metric group"),
            (
                json!({ "disabled": { "entry": { "m_flow_window": { "window_size_sec": 10 } } } }),
                "no metric conditions",
            ),
            (
                json!({ "disabled": { "entry": { "m_price_window": { "trail": [{"operator": ">=", "value": 1}] } } } }),
                "missing required param 'window_size_sec'",
            ),
            (
                // Two explicit OR arms, both unsatisfiable (normalize does not expand
                // multi-arm exprs, so the metric stays contradictory).
                json!({ "disabled": { "entry": { "m_snapshot": { "time": [
                    [{"operator": ">", "value": 30}, {"operator": "<", "value": 10}],
                    [{"operator": ">", "value": 50}, {"operator": "<", "value": 20}]
                ] } } } }),
                "contradictory",
            ),
            (json!({ "disabled": { "entyr": {} } }), "unknown key"),
            (json!({ "disabled": [] }), "must be an object"),
        ] {
            let e = RuleParams::parse(&bad).unwrap_err();
            assert!(e.contains(needle), "expected '{needle}' in: {e}");
        }
    }

    #[test]
    fn reentry_parses_validates_and_round_trips() {
        let p = RuleParams::parse(&json!({
            "reentry": { "cooldown_sec": 5, "max_episodes_per_token": 10 },
            "exit": { "m_position": { "retrace": [{ "operator": ">=", "value": 3 }] } }
        }))
        .expect("reentry rule is valid");
        let re = p.reentry.expect("reentry present");
        assert_eq!(re.cooldown_sec, 5.0);
        assert_eq!(re.max_episodes_per_token, 10);
        // Round-trip: to_value → parse → identical struct (no DB migration needed).
        assert_eq!(RuleParams::parse(&p.to_value()).unwrap(), p);

        // Absent ⇒ one-shot (None), the backward-compatible default.
        assert_eq!(RuleParams::parse(&json!({})).unwrap().reentry, None);

        // cooldown_sec 0 is legal (re-arm on the next event).
        assert!(RuleParams::parse(&json!({
            "reentry": { "cooldown_sec": 0, "max_episodes_per_token": 1 }
        }))
        .is_ok());
    }

    #[test]
    fn exclusivity_parses_defaults_and_round_trips() {
        // Absent ⇒ the stored-rule default: non-exclusive, priority 0.
        let p = RuleParams::parse(&json!({})).unwrap();
        assert!(!p.exclusive);
        assert_eq!(p.priority, 0);
        // ...and defaults never appear in the serialized shape (no migration).
        let v = p.to_value();
        assert!(v.get("exclusive").is_none() && v.get("priority").is_none());

        let p = RuleParams::parse(&json!({ "exclusive": true, "priority": -3 })).unwrap();
        assert!(p.exclusive);
        assert_eq!(p.priority, -3);
        assert_eq!(RuleParams::parse(&p.to_value()).unwrap(), p);
    }

    #[test]
    fn exclusivity_rejects_bad_values() {
        for (bad, needle) in [
            (json!({ "exclusive": "yes" }), "must be a boolean"),
            (json!({ "priority": 1.5 }), "integer within i32 range"),
            (json!({ "priority": 1e18 }), "integer within i32 range"),
            (json!({ "priority": "high" }), "must be a number"),
        ] {
            let e = RuleParams::parse(&bad).unwrap_err();
            assert!(e.contains(needle), "expected '{needle}' in: {e}");
        }
    }

    #[test]
    fn reentry_rejects_bad_values() {
        for (bad, needle) in [
            (json!({ "reentry": { "cooldown_sec": -1, "max_episodes_per_token": 5 } }), ">= 0"),
            (json!({ "reentry": { "cooldown_sec": 5, "max_episodes_per_token": 0 } }), ">= 1"),
            (json!({ "reentry": { "cooldown_sec": 5, "max_episodes_per_token": 2.5 } }), ">= 1"),
            (json!({ "reentry": { "cooldown_sec": 5 } }), "max_episodes_per_token"),
            (json!({ "reentry": { "cooldown_sec": 5, "max_episodes_per_token": 5, "foo": 1 } }), "unknown key"),
        ] {
            let e = RuleParams::parse(&bad).unwrap_err();
            assert!(e.contains(needle), "expected '{needle}' in: {e}");
        }
    }

    #[test]
    fn scale_out_parses_validates_and_round_trips() {
        let p = RuleParams::parse(&json!({
            "stop_loss": 20,
            "exit": { "m_position": { "retrace": [{ "operator": ">=", "value": 7 }], "arm_above_pct": 2 } },
            "scale_out": [
                { "sell_bps": 7000, "take_profit": 25 },
                { "conditions": { "m_position": { "held": [{ "operator": ">=", "value": 30 }] } } }
            ]
        }))
        .expect("scale_out rule is valid");
        let stages = p.scale_out.as_ref().unwrap();
        assert_eq!(stages.len(), 2);
        assert_eq!(stages[0].sell_bps, Some(7000));
        assert_eq!(stages[0].take_profit, Some(25.0));
        assert!(stages[0].conditions.is_empty());
        assert_eq!(stages[1].sell_bps, None, "remainder stage");
        assert!(!stages[1].conditions.is_empty());
        assert_eq!(RuleParams::parse(&p.to_value()).unwrap(), p);

        // Empty array folds to None (configured_labels precedent).
        assert_eq!(RuleParams::parse(&json!({ "scale_out": [] })).unwrap().scale_out, None);
        assert!(RuleParams::parse(&json!({})).unwrap().to_value().get("scale_out").is_none());
    }

    #[test]
    fn scale_out_rejects_bad_ladders() {
        for (bad, needle) in [
            // sell_bps out of range
            (json!({ "scale_out": [{ "sell_bps": 0, "take_profit": 10 }] }), "[1, 9900]"),
            (json!({ "scale_out": [{ "sell_bps": 9901, "take_profit": 10 }] }), "[1, 9900]"),
            // no way to fire
            (json!({ "scale_out": [{ "sell_bps": 5000 }] }), "take_profit and/or"),
            // remainder not last
            (json!({ "scale_out": [
                { "conditions": { "m_position": { "held": [{ "operator": ">=", "value": 1 }] } } },
                { "sell_bps": 5000, "take_profit": 10 }
            ] }), "must be last"),
            // sum too high
            (json!({ "scale_out": [
                { "sell_bps": 5000, "take_profit": 10 },
                { "sell_bps": 5000, "take_profit": 20 }
            ] }), "<= 9900"),
            // too many explicit stages
            (json!({ "scale_out": [
                { "sell_bps": 1000, "take_profit": 10 },
                { "sell_bps": 1000, "take_profit": 20 },
                { "sell_bps": 1000, "take_profit": 30 },
                { "sell_bps": 1000, "take_profit": 40 }
            ] }), "at most 3 explicit"),
        ] {
            let e = RuleParams::parse(&bad).unwrap_err();
            assert!(e.contains(needle), "expected '{needle}' in: {e}");
        }
    }
}
