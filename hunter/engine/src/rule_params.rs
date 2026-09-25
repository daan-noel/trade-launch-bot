//! `RuleParams` — the typed form of `strategy_rules.params`: WHEN a rule buys and HOW it
//! sells. Parsed and validated once at save and at load, never per event.
//!
//! ```json
//! {
//!   "enter": {
//!     "event":         [cond],          // the print that triggers the buy (AND)
//!     "filters":       [cond],          // must also hold; if one fails, keep watching
//!     "final_filters": [cond],          // checked on that print; if one fails, give up the coin
//!     "lock":          "token",         // or "slot": one chance per coin / per slot
//!     "size_pct_of_pool": 1.5           // buy this % of the pool's SOL instead of the fixed amount
//!   },
//!   "take_profit": 100,                 // shortcut: always-line on m_position.pnl_pct
//!   "stop_loss":   30,
//!   "signals": { "cashout": [[cond, cond], [cond]] },   // named: any group holds (AND inside)
//!   "always":  [line],                  // checked first, in every stage
//!   "stages":  [{ "name": "early", "ends": {"age_sec": 20}, "on": [line], "at_end": [line], "then": "late" }],
//!   "reentry": { "cooldown_sec": 30, "max_per_coin": 3 },
//!   "exclusive": true, "priority": 2
//! }
//! cond = {"metric": "m_flow.buy_sol", "tag": "!volume", "span": "10s", "is": [{"operator": ">=", "value": 2}]}
//!      | {"signal": "cashout"} | {"signal": "cashout", "not": true}
//! line = {"if": [cond], "sell": "label" | true, "sell_pct": 50, "go": "stage"}
//! ```
//!
//! Any condition or line may carry `"off": true`: kept in place and validated like a
//! live one, compiled by nothing — how the editor parks a condition without losing it.
//!
//! **Evaluation, one step per print or tick** (see `arm.rs`): the `always` lines, then
//! (at the stage's deadline) its `at_end` lines or the move to `then`, else its `on`
//! lines; the first line that holds acts, and a move takes effect from the next print or
//! tick. A partial sell moves when its fill lands.

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

use crate::metrics::evaluator::{
    check_expr_satisfiable, condition_expr_to_value, normalize_condition_expr, parse_condition_expr, ConditionExpr,
};
use crate::metrics::{metric_spec, MetricRef};

/// Ceiling on [`Enter::size_pct_of_pool`], percent of the pool's SOL reserve. A rail on
/// real money: this multiplies a live pool balance into a buy size, so an authoring slip
/// has to stay survivable. Our own impact is `buy / vsol` exactly.
pub const MAX_SIZE_PCT_OF_POOL: f64 = 10.0;

/// The rule format this engine reads and writes: `2` = the v2 document (`enter`,
/// `signals`, `always`, `stages`). Anything that stores a result computed from a rule
/// stamps it, so a result from an older format can be told apart.
pub const RULE_FORMAT_VERSION: u8 = 2;

/// Largest partial sell, basis points of the first buy's bag — the one cap a rule's
/// `sell_pct` and a manual partial sell (`sell_bps`) both obey.
pub const MAX_SELL_BPS: u16 = 9_900;

/// [`MAX_SELL_BPS`] as the percent a rule writes.
pub const MAX_SELL_PCT: f64 = MAX_SELL_BPS as f64 / 100.0;

/// Most stages a rule may have (a stage index is a `u8`, and a plan this long is a
/// mistake, not a strategy).
pub const MAX_STAGES: usize = 32;

/// The first print that makes the event true is the only candidate — per slot, or once
/// per coin. A filter that fails on it spends that chance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryLock {
    /// One chance per slot.
    Slot,
    /// One chance per coin: the first PRINT (a clock tick never decides) that makes the
    /// event true enters or, failing a filter, ends the coin for this rule.
    Token,
}

impl EntryLock {
    fn parse(v: &Value) -> Result<Self, String> {
        match v.as_str() {
            Some("slot") => Ok(Self::Slot),
            Some("token") => Ok(Self::Token),
            _ => Err("enter.lock must be \"token\" or \"slot\"".into()),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Slot => "slot",
            Self::Token => "token",
        }
    }
}

/// One condition.
#[derive(Debug, Clone, PartialEq)]
pub enum Cond {
    /// `metric <is>`: a reading judged against a DNF expression.
    Metric { r: MetricRef, is: ConditionExpr, off: bool },
    /// A named signal holds (or, `negated`, does not).
    Signal { name: &'static str, negated: bool, off: bool },
}

impl Cond {
    pub fn is_off(&self) -> bool {
        match self {
            Cond::Metric { off, .. } | Cond::Signal { off, .. } => *off,
        }
    }
}

/// What a line does when its conditions hold.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sell {
    /// The exit reason. `None` = labelled from the line's first condition.
    pub label: Option<&'static str>,
    /// Percent of the FIRST buy's bag; `None` = everything left.
    pub pct: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// AND. Empty only in `at_end` (an unconditional checkpoint action).
    pub when: Vec<Cond>,
    pub sell: Option<Sell>,
    /// Stage to move to.
    pub go: Option<&'static str>,
    pub off: bool,
}

/// What a stage deadline is measured from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeadlineBasis {
    /// The coin's age.
    Age,
    /// Time since our buy filled.
    Held,
    /// Time since this stage began.
    Stage,
}

impl DeadlineBasis {
    fn key(self) -> &'static str {
        match self {
            Self::Age => "age_sec",
            Self::Held => "held_sec",
            Self::Stage => "stage_sec",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Deadline {
    pub basis: DeadlineBasis,
    pub secs: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stage {
    pub name: &'static str,
    pub ends: Option<Deadline>,
    pub on: Vec<Line>,
    pub at_end: Vec<Line>,
    /// Where the deadline leads when no `at_end` line acts. `None` = the next stage.
    pub then: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Enter {
    pub event: Vec<Cond>,
    pub filters: Vec<Cond>,
    pub final_filters: Vec<Cond>,
    pub lock: Option<EntryLock>,
    pub size_pct_of_pool: Option<f64>,
}

/// Re-entry: after a normal sell (a line, TP or SL — never Dead / Manual / Migrated),
/// wait `cooldown_sec` and watch the coin again, up to `max_per_coin` buys.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReEntry {
    pub cooldown_sec: f64,
    pub max_per_coin: u32,
}

/// Typed, registry-checked `params`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RuleParams {
    pub enter: Enter,
    /// Percent; compiles to an `always` line `m_position.pnl_pct >= tp`, after the stop.
    pub take_profit: Option<f64>,
    /// Percent; compiles to the FIRST `always` line `m_position.pnl_pct <= -sl`.
    pub stop_loss: Option<f64>,
    /// Name -> OR of AND-groups. Sorted by name.
    pub signals: BTreeMap<&'static str, Vec<Vec<Cond>>>,
    pub always: Vec<Line>,
    /// Empty = one implicit stage with no lines of its own.
    pub stages: Vec<Stage>,
    pub reentry: Option<ReEntry>,
    pub exclusive: bool,
    pub priority: i32,
}

// ── Parse ────────────────────────────────────────────────────────────────────

fn obj<'a>(v: &'a Value, at: &str) -> Result<&'a Map<String, Value>, String> {
    v.as_object().ok_or_else(|| format!("{at} must be an object"))
}

fn arr<'a>(v: &'a Value, at: &str) -> Result<&'a Vec<Value>, String> {
    v.as_array().ok_or_else(|| format!("{at} must be an array"))
}

fn num(v: &Value, at: &str) -> Result<f64, String> {
    v.as_f64().filter(|x| x.is_finite()).ok_or_else(|| format!("{at} must be a finite number"))
}

fn flag(o: &Map<String, Value>, key: &str, at: &str) -> Result<bool, String> {
    match o.get(key) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(format!("{at}.{key} must be true or false")),
    }
}

fn unknown_keys(o: &Map<String, Value>, allowed: &[&str], at: &str) -> Result<(), String> {
    match o.keys().find(|k| !allowed.contains(&k.as_str())) {
        Some(k) => Err(format!("{at}: unknown key `{k}` (expected {})", allowed.join(", "))),
        None => Ok(()),
    }
}

/// A signal or stage name: `[a-z0-9_]`, 1 to 32 characters.
fn name(v: &Value, at: &str) -> Result<&'static str, String> {
    let s = v.as_str().ok_or_else(|| format!("{at} must be a name"))?;
    check_name(s, at)?;
    Ok(crate::intern::intern(s))
}

fn check_name(s: &str, at: &str) -> Result<(), String> {
    if s.is_empty() || s.len() > 32 || !s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_') {
        return Err(format!("{at}: `{s}` must be 1-32 characters of a-z, 0-9 and _"));
    }
    Ok(())
}

fn parse_cond(v: &Value, at: &str) -> Result<Cond, String> {
    let o = obj(v, at)?;
    let off = flag(o, "off", at)?;
    if let Some(sig) = o.get("signal") {
        unknown_keys(o, &["signal", "not", "off"], at)?;
        return Ok(Cond::Signal { name: name(sig, &format!("{at}.signal"))?, negated: flag(o, "not", at)?, off });
    }
    unknown_keys(o, &["metric", "tag", "span", "slice", "is", "off"], at)?;
    let r = MetricRef::from_json(o).map_err(|e| format!("{at}: {e}"))?;
    let tol = metric_spec(r.metric).eq_tolerance;
    let raw = o.get("is").ok_or_else(|| format!("{at}: `is` is missing (e.g. [{{\"operator\": \">=\", \"value\": 2}}])"))?;
    let is = parse_condition_expr(raw).map_err(|e| format!("{at}.is: {e}"))?;
    if is.is_empty() || is.iter().any(Vec::is_empty) {
        return Err(format!("{at}.is is empty"));
    }
    if is.iter().flatten().any(|c| !c.value.is_finite()) {
        return Err(format!("{at}.is: every value must be a finite number"));
    }
    check_expr_satisfiable(&is, tol).map_err(|e| format!("{at} ({}): {e}", r.label()))?;
    Ok(Cond::Metric { r, is: normalize_condition_expr(is, tol), off })
}

fn parse_conds(v: Option<&Value>, at: &str) -> Result<Vec<Cond>, String> {
    let Some(v) = v else { return Ok(Vec::new()) };
    arr(v, at)?.iter().enumerate().map(|(i, c)| parse_cond(c, &format!("{at}[{i}]"))).collect()
}

fn parse_line(v: &Value, at: &str) -> Result<Line, String> {
    let o = obj(v, at)?;
    unknown_keys(o, &["if", "sell", "sell_pct", "go", "off"], at)?;
    let when = parse_conds(o.get("if"), &format!("{at}.if"))?;
    let label = match o.get("sell") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => None,
        Some(Value::Bool(true)) => Some(None),
        Some(Value::String(s)) if !s.trim().is_empty() => Some(Some(crate::intern::intern(s.trim()))),
        Some(_) => return Err(format!("{at}.sell must be a label or true")),
    };
    let pct = o.get("sell_pct").map(|v| num(v, &format!("{at}.sell_pct"))).transpose()?;
    if pct.is_some() && label.is_none() {
        return Err(format!("{at}: sell_pct without sell"));
    }
    let sell = label.map(|label| Sell { label, pct });
    let go = o.get("go").map(|g| name(g, &format!("{at}.go"))).transpose()?;
    if sell.is_none() && go.is_none() {
        return Err(format!("{at} does nothing: give it `sell` and/or `go`"));
    }
    Ok(Line { when, sell, go, off: flag(o, "off", at)? })
}

fn parse_lines(v: Option<&Value>, at: &str) -> Result<Vec<Line>, String> {
    let Some(v) = v else { return Ok(Vec::new()) };
    arr(v, at)?.iter().enumerate().map(|(i, l)| parse_line(l, &format!("{at}[{i}]"))).collect()
}

fn parse_stage(v: &Value, at: &str) -> Result<Stage, String> {
    let o = obj(v, at)?;
    unknown_keys(o, &["name", "ends", "on", "at_end", "then"], at)?;
    let name = name(o.get("name").ok_or_else(|| format!("{at} needs a name"))?, &format!("{at}.name"))?;
    let at = format!("stage `{name}`");
    let ends = match o.get("ends") {
        None | Some(Value::Null) => None,
        Some(e) => {
            let eo = obj(e, &format!("{at}.ends"))?;
            if eo.len() != 1 {
                return Err(format!("{at}.ends names one clock: age_sec, held_sec or stage_sec"));
            }
            let (k, v) = eo.iter().next().expect("one key");
            let basis = match k.as_str() {
                "age_sec" => DeadlineBasis::Age,
                "held_sec" => DeadlineBasis::Held,
                "stage_sec" => DeadlineBasis::Stage,
                other => return Err(format!("{at}.ends: unknown clock `{other}` (age_sec, held_sec, stage_sec)")),
            };
            let secs = num(v, &format!("{at}.ends.{k}"))?;
            if secs < 0.0 {
                return Err(format!("{at}.ends.{k} must be >= 0"));
            }
            Some(Deadline { basis, secs })
        }
    };
    Ok(Stage {
        name,
        ends,
        on: parse_lines(o.get("on"), &format!("{at}.on"))?,
        at_end: parse_lines(o.get("at_end"), &format!("{at}.at_end"))?,
        then: o.get("then").map(|t| self::name(t, &format!("{at}.then"))).transpose()?,
    })
}

fn parse_enter(v: Option<&Value>) -> Result<Enter, String> {
    let Some(v) = v else { return Ok(Enter::default()) };
    let o = obj(v, "enter")?;
    unknown_keys(o, &["event", "filters", "final_filters", "lock", "size_pct_of_pool"], "enter")?;
    Ok(Enter {
        event: parse_conds(o.get("event"), "enter.event")?,
        filters: parse_conds(o.get("filters"), "enter.filters")?,
        final_filters: parse_conds(o.get("final_filters"), "enter.final_filters")?,
        lock: o.get("lock").filter(|v| !v.is_null()).map(EntryLock::parse).transpose()?,
        size_pct_of_pool: o.get("size_pct_of_pool").map(|v| num(v, "enter.size_pct_of_pool")).transpose()?,
    })
}

impl RuleParams {
    /// Parse **and validate**. The one entry point rule save and rule load share.
    pub fn parse(json: &Value) -> Result<Self, String> {
        let root = obj(json, "params")?;
        unknown_keys(
            root,
            &["enter", "take_profit", "stop_loss", "signals", "always", "stages", "reentry", "exclusive", "priority"],
            "params",
        )?;
        let mut signals = BTreeMap::new();
        if let Some(s) = root.get("signals") {
            for (k, groups) in obj(s, "signals")? {
                check_name(k, "signals")?;
                let at = format!("signal `{k}`");
                let mut out = Vec::new();
                for (i, g) in arr(groups, &at)?.iter().enumerate() {
                    out.push(parse_conds(Some(g), &format!("{at}[{i}]"))?);
                }
                signals.insert(crate::intern::intern(k), out);
            }
        }
        let reentry = match root.get("reentry") {
            None | Some(Value::Null) => None,
            Some(r) => {
                let o = obj(r, "reentry")?;
                unknown_keys(o, &["cooldown_sec", "max_per_coin"], "reentry")?;
                let cooldown_sec = num(o.get("cooldown_sec").ok_or("reentry.cooldown_sec is missing")?, "reentry.cooldown_sec")?;
                let max = o
                    .get("max_per_coin")
                    .and_then(Value::as_u64)
                    .and_then(|n| u32::try_from(n).ok())
                    .ok_or("reentry.max_per_coin must be a whole number >= 1")?;
                Some(ReEntry { cooldown_sec, max_per_coin: max })
            }
        };
        let p = Self {
            enter: parse_enter(root.get("enter"))?,
            take_profit: root.get("take_profit").filter(|v| !v.is_null()).map(|v| num(v, "take_profit")).transpose()?,
            stop_loss: root.get("stop_loss").filter(|v| !v.is_null()).map(|v| num(v, "stop_loss")).transpose()?,
            signals,
            always: parse_lines(root.get("always"), "always")?,
            stages: match root.get("stages") {
                None | Some(Value::Null) => Vec::new(),
                Some(s) => arr(s, "stages")?.iter().enumerate().map(|(i, st)| parse_stage(st, &format!("stages[{i}]"))).collect::<Result<_, _>>()?,
            },
            reentry,
            exclusive: match root.get("exclusive") {
                None | Some(Value::Null) => false,
                Some(v) => v.as_bool().ok_or("exclusive must be true or false")?,
            },
            priority: match root.get("priority") {
                None | Some(Value::Null) => 0,
                Some(v) => v.as_i64().and_then(|n| i32::try_from(n).ok()).ok_or("priority must be a whole number")?,
            },
        };
        p.validate()?;
        Ok(p)
    }

    /// The cross-part rules the shape walk cannot see.
    fn validate(&self) -> Result<(), String> {
        for (k, v, lo, hi) in [
            ("take_profit", self.take_profit, 0.0, f64::INFINITY),
            ("stop_loss", self.stop_loss, 0.0, f64::INFINITY),
            ("enter.size_pct_of_pool", self.enter.size_pct_of_pool, 0.0, MAX_SIZE_PCT_OF_POOL),
        ] {
            if let Some(x) = v {
                if !(x > lo && x <= hi) {
                    return Err(format!("{k} must be above {lo} and at most {hi}"));
                }
            }
        }
        if let Some(r) = self.reentry {
            if r.cooldown_sec < 0.0 {
                return Err("reentry.cooldown_sec must be >= 0".into());
            }
            if r.max_per_coin < 1 {
                return Err("reentry.max_per_coin must be >= 1".into());
            }
        }
        // Signals: metric conditions only (a signal naming a signal could loop).
        for (name, groups) in &self.signals {
            if groups.is_empty() || groups.iter().any(Vec::is_empty) {
                return Err(format!("signal `{name}` has an empty group"));
            }
            if groups.iter().flatten().any(|c| matches!(c, Cond::Signal { .. })) {
                return Err(format!("signal `{name}` may use metric conditions only, not other signals"));
            }
        }
        let signal_ok = |c: &Cond, at: &str, entry: bool| -> Result<(), String> {
            match c {
                Cond::Signal { name, .. } => {
                    let groups = self.signals.get(name).ok_or_else(|| format!("{at}: no signal `{name}`"))?;
                    if entry && groups.iter().flatten().any(|g| matches!(g, Cond::Metric { r, .. } if r.is_position())) {
                        return Err(format!("{at}: signal `{name}` reads our position, which does not exist before the buy"));
                    }
                    Ok(())
                }
                Cond::Metric { r, .. } if entry && r.is_position() => {
                    Err(format!("{at}: {} reads our position, which does not exist before the buy", r.label()))
                }
                Cond::Metric { .. } => Ok(()),
            }
        };
        for (at, conds) in [
            ("enter.event", &self.enter.event),
            ("enter.filters", &self.enter.filters),
            ("enter.final_filters", &self.enter.final_filters),
        ] {
            for c in conds {
                signal_ok(c, at, true)?;
            }
        }
        if self.enter.lock.is_some() && self.enter.event.iter().all(Cond::is_off) {
            return Err("enter.lock needs an enter.event: the lock is one chance at the event print".into());
        }
        // Stages: unique names, targets exist, deadlines lead somewhere.
        if self.stages.len() > MAX_STAGES {
            return Err(format!("at most {MAX_STAGES} stages"));
        }
        let mut names = std::collections::BTreeSet::new();
        for s in &self.stages {
            if !names.insert(s.name) {
                return Err(format!("two stages are named `{}`", s.name));
            }
        }
        let target = |t: &str, at: &str| -> Result<(), String> {
            if names.contains(t) {
                Ok(())
            } else {
                Err(format!("{at}: no stage named `{t}`"))
            }
        };
        let check_line = |l: &Line, at: &str, conditional: bool| -> Result<(), String> {
            if conditional && l.when.iter().all(Cond::is_off) && !l.off {
                return Err(format!("{at} has no condition, so it would act on the first print"));
            }
            for c in &l.when {
                signal_ok(c, at, false)?;
            }
            if let Some(g) = l.go {
                target(g, at)?;
            }
            if let Some(Sell { pct: Some(p), .. }) = l.sell {
                if !(p > 0.0 && p <= MAX_SELL_PCT) {
                    return Err(format!("{at}.sell_pct must be above 0 and at most {MAX_SELL_PCT}"));
                }
                if l.go.is_none() {
                    return Err(format!("{at}: a partial sell must also `go` to another stage, or it would sell again on the next print"));
                }
            }
            Ok(())
        };
        for (i, l) in self.always.iter().enumerate() {
            check_line(l, &format!("always[{i}]"), true)?;
        }
        for (si, s) in self.stages.iter().enumerate() {
            for (i, l) in s.on.iter().enumerate() {
                check_line(l, &format!("stage `{}`.on[{i}]", s.name), true)?;
            }
            for (i, l) in s.at_end.iter().enumerate() {
                check_line(l, &format!("stage `{}`.at_end[{i}]", s.name), false)?;
            }
            if !s.at_end.is_empty() && s.ends.is_none() {
                return Err(format!("stage `{}` has at_end lines but no deadline (`ends`)", s.name));
            }
            if let Some(t) = s.then {
                if s.ends.is_none() {
                    return Err(format!("stage `{}`: `then` needs a deadline (`ends`)", s.name));
                }
                target(t, &format!("stage `{}`.then", s.name))?;
            } else if s.ends.is_some() && si + 1 == self.stages.len() {
                return Err(format!("stage `{}` ends but no stage follows it: add `then`", s.name));
            }
        }
        Ok(())
    }

    // ── Serialize (the inverse of parse) ─────────────────────────────────────

    pub fn to_value(&self) -> Value {
        let mut root = Map::new();
        let e = &self.enter;
        let mut enter = Map::new();
        for (k, conds) in [("event", &e.event), ("filters", &e.filters), ("final_filters", &e.final_filters)] {
            if !conds.is_empty() {
                enter.insert(k.into(), conds_to_value(conds));
            }
        }
        if let Some(l) = e.lock {
            enter.insert("lock".into(), json!(l.as_str()));
        }
        if let Some(p) = e.size_pct_of_pool {
            enter.insert("size_pct_of_pool".into(), json!(p));
        }
        if !enter.is_empty() {
            root.insert("enter".into(), Value::Object(enter));
        }
        if let Some(tp) = self.take_profit {
            root.insert("take_profit".into(), json!(tp));
        }
        if let Some(sl) = self.stop_loss {
            root.insert("stop_loss".into(), json!(sl));
        }
        if !self.signals.is_empty() {
            let s: Map<String, Value> = self
                .signals
                .iter()
                .map(|(k, groups)| ((*k).to_string(), Value::Array(groups.iter().map(|g| conds_to_value(g)).collect())))
                .collect();
            root.insert("signals".into(), Value::Object(s));
        }
        if !self.always.is_empty() {
            root.insert("always".into(), lines_to_value(&self.always));
        }
        if !self.stages.is_empty() {
            root.insert("stages".into(), Value::Array(self.stages.iter().map(stage_to_value).collect()));
        }
        if let Some(r) = self.reentry {
            root.insert("reentry".into(), json!({ "cooldown_sec": r.cooldown_sec, "max_per_coin": r.max_per_coin }));
        }
        if self.exclusive {
            root.insert("exclusive".into(), json!(true));
        }
        if self.priority != 0 {
            root.insert("priority".into(), json!(self.priority));
        }
        Value::Object(root)
    }

    /// The rule buys on arming alone: no live entry condition at all.
    pub fn enter_on_arm(&self) -> bool {
        let live = |c: &[Cond]| c.iter().any(|c| !c.is_off());
        !live(&self.enter.event) && !live(&self.enter.filters) && !live(&self.enter.final_filters)
    }

    /// Every live metric reference the rule reads, in every part — what a loader asks
    /// "which columns and buffers does this rule need" of.
    pub fn metric_refs(&self) -> Vec<MetricRef> {
        let mut out = Vec::new();
        let mut take = |conds: &[Cond]| {
            for c in conds {
                if let Cond::Metric { r, off: false, .. } = c {
                    out.push(*r);
                }
            }
        };
        take(&self.enter.event);
        take(&self.enter.filters);
        take(&self.enter.final_filters);
        for g in self.signals.values().flatten() {
            take(g);
        }
        for l in self.all_lines() {
            if !l.off {
                take(&l.when);
            }
        }
        out
    }

    /// Every line: `always`, then each stage's `on` and `at_end`.
    pub fn all_lines(&self) -> impl Iterator<Item = &Line> {
        self.always.iter().chain(self.stages.iter().flat_map(|s| s.on.iter().chain(s.at_end.iter())))
    }
}

fn cond_to_value(c: &Cond) -> Value {
    let mut o = Map::new();
    match c {
        Cond::Metric { r, is, off } => {
            r.write_json(&mut o);
            o.insert("is".into(), condition_expr_to_value(is));
            if *off {
                o.insert("off".into(), json!(true));
            }
        }
        Cond::Signal { name, negated, off } => {
            o.insert("signal".into(), json!(name));
            if *negated {
                o.insert("not".into(), json!(true));
            }
            if *off {
                o.insert("off".into(), json!(true));
            }
        }
    }
    Value::Object(o)
}

fn conds_to_value(conds: &[Cond]) -> Value {
    Value::Array(conds.iter().map(cond_to_value).collect())
}

fn line_to_value(l: &Line) -> Value {
    let mut o = Map::new();
    if !l.when.is_empty() {
        o.insert("if".into(), conds_to_value(&l.when));
    }
    if let Some(s) = l.sell {
        o.insert("sell".into(), s.label.map_or(json!(true), |t| json!(t)));
        if let Some(p) = s.pct {
            o.insert("sell_pct".into(), json!(p));
        }
    }
    if let Some(g) = l.go {
        o.insert("go".into(), json!(g));
    }
    if l.off {
        o.insert("off".into(), json!(true));
    }
    Value::Object(o)
}

fn lines_to_value(lines: &[Line]) -> Value {
    Value::Array(lines.iter().map(line_to_value).collect())
}

fn stage_to_value(s: &Stage) -> Value {
    let mut o = Map::new();
    o.insert("name".into(), json!(s.name));
    if let Some(d) = s.ends {
        o.insert("ends".into(), json!({ d.basis.key(): d.secs }));
    }
    if !s.on.is_empty() {
        o.insert("on".into(), lines_to_value(&s.on));
    }
    if !s.at_end.is_empty() {
        o.insert("at_end".into(), lines_to_value(&s.at_end));
    }
    if let Some(t) = s.then {
        o.insert("then".into(), json!(t));
    }
    Value::Object(o)
}

#[cfg(test)]
#[path = "rule_params_tests.rs"]
mod tests;
