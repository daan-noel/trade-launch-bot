//! Reading **v1** documents — the rule params, fingerprint config and criteria written
//! before metric system v2 — into the v2 shape.
//!
//! One conversion, used by the data migration, by rule-bundle import and by the golden
//! tests (which keep their pinned v1 rules and so prove the conversion decision for
//! decision). Every v1 meaning maps onto v2 exactly; where v1 had a behaviour no v2 word
//! states directly, the converted rule spells it:
//!
//! * **A v1 metric** is a (group, metric, window) triple: it becomes one
//!   `m_family.metric @tag [span]` condition ([`map_metric`]). The fingerprint lists
//!   become tags: `m_flow_ix` = `volume`, `m_dump_ix` = `dump`, `m_copy` = `targets`,
//!   `m_burst_slot` = `working`.
//! * **Exit clauses** become `always` lines, in order, after the TP/SL shortcuts.
//! * **`arm_above_pct` on a lone trailing clause** gates it at each evaluation: the line
//!   also carries `m_position.pnl_pct >= X`.
//! * **The `armed` latch** becomes two stages, `start` and `armed`:
//!   - a pnl latch (`arm_above_pct` with `armed` clauses) sets before the exits of the
//!     event it crosses on, so `start` carries `pnl_pct >= X and <clause>` for each
//!     `armed = 1` clause, each `armed = 0` clause with `pnl_pct < X`, then
//!     `pnl_pct >= X -> go armed`;
//!   - `arm` clauses latch after the exits of their event, so `start` carries the
//!     `armed = 0` clauses, then one `go armed` line per arm clause;
//!   - `armed` carries the `armed = 1` clauses; `since_armed` becomes
//!     `m_position.stage_sec`.
//!
//!   v1 never let an `armed` clause stop a buy (the latch reads `NaN` before one), so a
//!   line converted from one also carries a position condition that reads `NaN` before
//!   a buy the same way: `pnl_pct < X` under a pnl latch, `held_sec >= 0` otherwise.
//! * **A `scale_out` ladder** becomes one stage per rung: each rung condition is a line
//!   that sells the rung's share of the first bag and moves to the next rung.
//! * **Parked (`disabled`)** conditions and lines keep their place with `"off": true`.
//!
//! Combinations no stored rule uses and v2 cannot state exactly (a latch together with
//! a ladder, a pnl latch together with `arm` clauses) are refused with an error.

use serde_json::{json, Map, Value};

use crate::metrics::WindowSpec;

/// The tag each v1 fingerprint list becomes.
pub const TAG_VOLUME: &str = "volume";
pub const TAG_DUMP: &str = "dump";
pub const TAG_TARGETS: &str = "targets";
pub const TAG_WORKING: &str = "working";

/// Whether a params document is v1: any v1 root key, or nothing of v2's.
pub fn is_v1_params(v: &Value) -> bool {
    let Some(o) = v.as_object() else { return false };
    const V1: [&str; 8] = ["entry", "entry_event", "entry_lock", "exit", "arm", "scale_out", "disabled", "buy_pct_of_vsol"];
    o.keys().any(|k| V1.contains(&k.as_str()))
        || o.get("reentry").and_then(|r| r.get("max_episodes_per_token")).is_some()
}

/// Parse rule params written in either format: v1 is converted first. The entry point
/// for anything that may still hold a v1 document (a rule bundle, a test fixture).
pub fn parse_params_any(v: &Value) -> Result<crate::rule_params::RuleParams, String> {
    if is_v1_params(v) {
        crate::rule_params::RuleParams::parse(&convert_params(v)?)
    } else {
        crate::rule_params::RuleParams::parse(v)
    }
}

/// Whether a fingerprint `metric_config` is v1 (keyed by metric group).
pub fn is_v1_metric_config(v: &Value) -> bool {
    v.as_object().is_some_and(|o| o.keys().any(|k| k.starts_with("m_")))
}

// ── Metric mapping ───────────────────────────────────────────────────────────

/// A v1 (group, metric) as its v2 metric path and tag. `None` = no v2 metric (the
/// `armed` / `since_armed` latch reads, handled by the clause walk).
fn map_metric(group: &str, metric: &str) -> Option<(&'static str, Option<&'static str>)> {
    let t = |p: &'static str, tag: &'static str| Some((p, Some(tag)));
    let u = |p: &'static str| Some((p, None));
    match (group, metric) {
        ("m_state", "time") => u("m_state.age_sec"),
        ("m_state", "liquidity") => u("m_state.liquidity_sol"),
        ("m_state", "on_curve") => u("m_state.on_curve"),
        ("m_price_lifetime" | "m_price_window", "trail") => u("m_price.trail_pct"),
        ("m_price_lifetime" | "m_price_window", "rise") => u("m_price.rise_pct"),
        ("m_price_lifetime", "stall") => u("m_price.stall_sec"),
        ("m_flow_lifetime" | "m_flow_window", "gross_flow") => u("m_flow.gross_sol"),
        ("m_flow_lifetime" | "m_flow_window", "net_flow") => u("m_flow.net_sol"),
        ("m_flow_lifetime" | "m_flow_window", "buy") => u("m_flow.buy_sol"),
        ("m_flow_lifetime" | "m_flow_window", "sell") => u("m_flow.sell_sol"),
        ("m_flow_lifetime" | "m_flow_window", "trade_count") => u("m_flow.trade_count"),
        ("m_flow_window", "buy_count") => u("m_flow.buy_count"),
        ("m_flow_window", "sell_count") => u("m_flow.sell_count"),
        ("m_flow_window", "buy_share") => u("m_flow.buy_share_pct"),
        ("m_flow_window", "trade_share") => u("m_flow.slice_trade_share_pct"),
        ("m_flow_window", "sol_share") => u("m_flow.slice_sol_share_pct"),
        ("m_crowd_window", "unique_wallets") => u("m_crowd.unique_wallets"),
        ("m_crowd_window", "trades_per_wallet") => u("m_crowd.trades_per_wallet"),
        ("m_crowd_after_age", "non_creator_buyers") => u("m_crowd.buyer_count"),
        ("m_crowd_after_age", "this_buyer_is_new") => u("m_crowd.buyer_is_new"),
        ("m_build_window", "unique_builds") => u("m_crowd.unique_ix_shapes"),
        ("m_print_wallet", "since_buy") => u("m_print.since_buy_sec"),
        ("m_holder_book", "public_app_share") => t("m_holdings.bag_share_pct", "public_app"),
        ("m_holder_book", "bundled_share") => t("m_holdings.bag_share_pct", "bundled"),
        ("m_copy" | "m_copy_window", "buy_sol") => t("m_flow.buy_sol", TAG_TARGETS),
        ("m_copy" | "m_copy_window", "buy_count") => t("m_flow.buy_tx_count", TAG_TARGETS),
        ("m_copy" | "m_copy_window", "sell_sol") => t("m_flow.sell_sol", TAG_TARGETS),
        ("m_copy" | "m_copy_window", "sell_count") => t("m_flow.sell_tx_count", TAG_TARGETS),
        ("m_flow_ix" | "m_flow_ix_window", m) => {
            let (half, rest) = if let Some(r) = m.strip_prefix("untagged_") {
                ("!volume", r)
            } else if let Some(r) = m.strip_prefix("tagged_") {
                ("volume", r)
            } else {
                return None;
            };
            let path = match rest {
                "buy" => "m_flow.buy_sol",
                "sell" => "m_flow.sell_sol",
                "net" => "m_flow.net_sol",
                "gross" => "m_flow.gross_sol",
                "share" if half == "volume" => "m_flow.tag_share_pct",
                "buy_count" if half == "volume" => "m_flow.buy_count",
                "sell_count" if half == "volume" => "m_flow.sell_count",
                "pnl" if half == "volume" && group == "m_flow_ix" => "m_holdings.profit_sol",
                _ => return None,
            };
            Some((path, Some(if half == "volume" { TAG_VOLUME } else { "!volume" })))
        }
        ("m_dump_ix" | "m_dump_ix_window", "dump_sell") => t("m_flow.sell_sol", TAG_DUMP),
        ("m_dump_ix" | "m_dump_ix_window", "dump_sell_count") => t("m_flow.sell_tx_count", TAG_DUMP),
        ("m_burst_slot", m) => match m {
            "this_member" => u("m_slot.this_joined"),
            "this_working" => t("m_slot.this_has_tag", TAG_WORKING),
            "same_buy_count" => u("m_slot.same_template_buy_count"),
            "same_buy_sol" => u("m_slot.same_template_buy_sol"),
            "same_wallet_count" => u("m_slot.same_template_wallet_count"),
            "member_template_count" => u("m_slot.template_count"),
            "working_buy_count" => t("m_slot.buy_count", TAG_WORKING),
            "working_buy_sol" => t("m_slot.buy_sol", TAG_WORKING),
            "working_wallet_count" => t("m_slot.wallet_count", TAG_WORKING),
            "working_template_count" => t("m_slot.template_count", TAG_WORKING),
            "working_templates_seen" => t("m_crowd.unique_ix_templates", TAG_WORKING),
            "working_buy_share" => t("m_slot.buy_share_pct", TAG_WORKING),
            "has_new" => t("m_slot.has_new_wallet", TAG_WORKING),
            "has_unknown" => u("m_slot.has_unknown_wallet"),
            "packed" => u("m_slot.packed"),
            "pre_slot_liquidity" => u("m_slot.liquidity_before_sol"),
            "pre_print_trail" => u("m_slot.trail_before_pct"),
            _ => None,
        },
        ("m_burst_wave", m) => match m {
            "this_member" => u("m_wave.this_joined"),
            "wallet_count" => u("m_wave.wallet_count"),
            "buy_sol" => u("m_wave.buy_sol"),
            "gap_slots" => u("m_wave.gap_slots"),
            "all_new" => u("m_wave.all_new_wallets"),
            "has_unknown" => u("m_wave.has_unknown_wallet"),
            "working_buy_count" => t("m_wave.buy_count", TAG_WORKING),
            "this_working" => t("m_wave.this_has_tag", TAG_WORKING),
            "this_tip" => u("m_wave.this_tip_lamports"),
            "hole" => u("m_wave.has_tx_gap"),
            "tip_seen" => u("m_wave.tip_band_seen"),
            _ => None,
        },
        ("m_position", m) => match m {
            "retrace" => u("m_position.retrace_pct"),
            "bounce" => u("m_position.bounce_pct"),
            "pnl" => u("m_position.pnl_pct"),
            "held" => u("m_position.held_sec"),
            "room_taken" => u("m_position.room_taken_pct"),
            _ => None,
        },
        _ => None,
    }
}

/// v1 entry metrics that ended the coin on failure ("leftover"), now `final_filters`.
const LEFTOVER: [(&str, &str); 3] =
    [("m_burst_slot", "working_templates_seen"), ("m_burst_wave", "hole"), ("m_burst_wave", "tip_seen")];

/// A v1 condition read: the v2 condition, plus the v1 facts the clause walk needs.
#[derive(Debug, Clone)]
struct Read {
    /// The v2 condition object. `None` for `armed` (only `latch` is set).
    cond: Option<Map<String, Value>>,
    /// `Some(1.0)` / `Some(0.0)` for an `armed = x` condition.
    latch: Option<f64>,
    /// Came from `since_armed`.
    since_armed: bool,
    /// `arm_above_pct` on a trailing (retrace / bounce) metric.
    arm_above: Option<f64>,
    /// `(group, metric)` — for the leftover split and error messages.
    v1: (String, String),
}

fn size_param(g: &Map<String, Value>, sec: &str, slot: &str, print: &str) -> Result<Option<(f64, &'static str)>, String> {
    let mut out = None;
    for (k, suffix) in [(sec, "s"), (slot, "sl"), (print, "p")] {
        if let Some(v) = g.get(k) {
            let n = v.as_f64().ok_or_else(|| format!("{k} must be a number"))?;
            if out.is_some() {
                return Err(format!("two window sizes on one group ({k})"));
            }
            out = Some((n, suffix));
        }
    }
    Ok(out)
}

fn fmt_num(v: f64) -> String {
    crate::event::format_metric_threshold(v)
}

/// The v2 `span` / `slice` of one v1 group instance.
fn spans(group: &str, g: &Map<String, Value>) -> Result<(Option<String>, Option<String>), String> {
    if group == "m_crowd_after_age" {
        let a = g.get("after_age_sec").and_then(Value::as_f64).ok_or("m_crowd_after_age needs after_age_sec")?;
        return Ok((Some(format!("age{}s", fmt_num(a))), None));
    }
    let lag = g.get("window_lag").and_then(Value::as_f64).unwrap_or(0.0);
    let span = size_param(g, "window_size_sec", "window_size_slots", "window_size_prints")?.map(|(n, u)| {
        let w = WindowSpec { size: n, lag, unit: unit_of(u) };
        w.label()
    });
    let slice = size_param(g, "slice_size_sec", "slice_size_slots", "slice_size_prints")?
        .map(|(n, u)| WindowSpec { size: n, lag: 0.0, unit: unit_of(u) }.label());
    Ok((span, slice))
}

fn unit_of(suffix: &str) -> crate::metrics::WindowUnit {
    match suffix {
        "sl" => crate::metrics::WindowUnit::Slot,
        "p" => crate::metrics::WindowUnit::Print,
        _ => crate::metrics::WindowUnit::Sec,
    }
}

const STRICT: [&str; 9] = [
    "window_size_sec",
    "window_size_slots",
    "window_size_prints",
    "window_lag",
    "slice_size_sec",
    "slice_size_slots",
    "slice_size_prints",
    "after_age_sec",
    "arm_above_pct",
];

/// The reads of one v1 side object (`{group: instance | [instances]}`), in a stable
/// order: group name, then instance order, then metric name.
fn side_reads(side: &Value) -> Result<Vec<Read>, String> {
    let obj = side.as_object().ok_or("a v1 side must be an object")?;
    let mut out = Vec::new();
    for (group, inst) in obj {
        let instances: Vec<&Map<String, Value>> = match inst {
            Value::Object(o) => vec![o],
            Value::Array(a) => a.iter().map(|x| x.as_object().ok_or("group instance must be an object")).collect::<Result<_, _>>()?,
            _ => return Err(format!("{group} must be an object or an array")),
        };
        for g in instances {
            let (span, slice) = spans(group, g)?;
            let arm_above = g.get("arm_above_pct").and_then(Value::as_f64);
            for (metric, expr) in g {
                if STRICT.contains(&metric.as_str()) {
                    continue;
                }
                let v1 = (group.clone(), metric.clone());
                if group == "m_position" && metric == "armed" {
                    let x = latch_value(expr).ok_or("an `armed` condition must be `= 0` or `= 1`")?;
                    out.push(Read { cond: None, latch: Some(x), since_armed: false, arm_above: None, v1 });
                    continue;
                }
                let since_armed = group == "m_position" && metric == "since_armed";
                let (path, tag) = if since_armed {
                    ("m_position.stage_sec", None)
                } else {
                    map_metric(group, metric).ok_or_else(|| format!("no v2 metric for {group}.{metric}"))?
                };
                let mut c = Map::new();
                c.insert("metric".into(), json!(path));
                if let Some(t) = tag {
                    c.insert("tag".into(), json!(t));
                }
                let windowed = !matches!(
                    group.as_str(),
                    "m_state" | "m_price_lifetime" | "m_flow_lifetime" | "m_print_wallet" | "m_holder_book" | "m_copy"
                        | "m_flow_ix" | "m_dump_ix" | "m_burst_slot" | "m_burst_wave" | "m_position"
                );
                if windowed || group == "m_crowd_after_age" {
                    if let Some(s) = &span {
                        c.insert("span".into(), json!(s));
                    }
                }
                if matches!(metric.as_str(), "trade_share" | "sol_share") {
                    if let Some(s) = &slice {
                        c.insert("slice".into(), json!(s));
                    }
                }
                c.insert("is".into(), expr.clone());
                let trailing = group == "m_position" && matches!(metric.as_str(), "retrace" | "bounce");
                out.push(Read { cond: Some(c), latch: None, since_armed, arm_above: arm_above.filter(|_| trailing), v1 });
            }
        }
    }
    Ok(out)
}

/// `armed = 1` / `armed = 0` read off a v1 condition expression.
fn latch_value(expr: &Value) -> Option<f64> {
    let arms = crate::metrics::evaluator::parse_condition_expr(expr).ok()?;
    match arms.as_slice() {
        [arm] => match arm.as_slice() {
            [c] if c.operator == crate::metrics::evaluator::Operator::Eq && (c.value == 0.0 || c.value == 1.0) => Some(c.value),
            _ => None,
        },
        _ => None,
    }
}

/// The v1 exit side as clauses: object form = one clause per metric, array form = one
/// clause per element.
fn exit_clauses(exit: &Value) -> Result<Vec<Vec<Read>>, String> {
    match exit {
        Value::Array(cs) => cs.iter().map(side_reads).collect(),
        other => Ok(side_reads(other)?.into_iter().map(|r| vec![r]).collect()),
    }
}

fn cond(metric: &str, op: &str, value: f64) -> Value {
    json!({ "metric": metric, "is": [{ "operator": op, "value": value }] })
}

fn conds_of(reads: &[Read]) -> Vec<Value> {
    reads.iter().filter_map(|r| r.cond.clone().map(Value::Object)).collect()
}

fn sell_line(when: Vec<Value>) -> Value {
    json!({ "if": when, "sell": true })
}

fn off(mut v: Value) -> Value {
    v.as_object_mut().expect("object").insert("off".into(), json!(true));
    v
}

/// Convert v1 rule params to v2.
pub fn convert_params(v1: &Value) -> Result<Value, String> {
    let o = v1.as_object().ok_or("params must be an object")?;
    let mut root = Map::new();

    // ── enter ──
    let mut enter = Map::new();
    let mut filters = Vec::new();
    let mut final_filters = Vec::new();
    if let Some(e) = o.get("entry") {
        for r in side_reads(e)? {
            let c = Value::Object(r.cond.ok_or("`armed` cannot gate an entry")?);
            if LEFTOVER.contains(&(r.v1.0.as_str(), r.v1.1.as_str())) {
                final_filters.push(c);
            } else {
                filters.push(c);
            }
        }
    }
    let event = match o.get("entry_event") {
        Some(e) => side_reads(e)?.into_iter().map(|r| r.cond.map(Value::Object).ok_or("`armed` cannot be an event")).collect::<Result<Vec<_>, _>>()?,
        None => Vec::new(),
    };
    // Parked entry conditions keep their place.
    if let Some(d) = o.get("disabled") {
        if let Some(e) = d.get("entry") {
            for r in side_reads(e)? {
                filters.push(off(Value::Object(r.cond.ok_or("`armed` cannot gate an entry")?)));
            }
        }
    }
    let mut parked_event = Vec::new();
    if let Some(e) = o.get("disabled").and_then(|d| d.get("entry_event")) {
        for r in side_reads(e)? {
            parked_event.push(off(Value::Object(r.cond.ok_or("`armed` cannot be an event")?)));
        }
    }
    let event: Vec<Value> = event.into_iter().chain(parked_event).collect();
    for (k, v) in [("event", event), ("filters", filters), ("final_filters", final_filters)] {
        if !v.is_empty() {
            enter.insert(k.into(), Value::Array(v));
        }
    }
    if let Some(l) = o.get("entry_lock").filter(|v| !v.is_null()) {
        enter.insert("lock".into(), l.clone());
    }
    if let Some(p) = o.get("buy_pct_of_vsol").filter(|v| !v.is_null()) {
        enter.insert("size_pct_of_pool".into(), p.clone());
    }
    if !enter.is_empty() {
        root.insert("enter".into(), Value::Object(enter));
    }
    for k in ["take_profit", "stop_loss", "exclusive", "priority"] {
        if let Some(v) = o.get(k).filter(|v| !v.is_null()) {
            root.insert(k.into(), v.clone());
        }
    }
    if let Some(r) = o.get("reentry").filter(|v| !v.is_null()) {
        root.insert(
            "reentry".into(),
            json!({ "cooldown_sec": r.get("cooldown_sec").cloned().unwrap_or(json!(0.0)),
                    "max_per_coin": r.get("max_episodes_per_token").cloned().unwrap_or(json!(1)) }),
        );
    }

    // ── exit ──
    let clauses = match o.get("exit") {
        Some(e) => exit_clauses(e)?,
        None => Vec::new(),
    };
    let arm_clauses = match o.get("arm") {
        Some(a) => exit_clauses(a)?,
        None => Vec::new(),
    };
    // The pnl latch threshold: the first `arm_above_pct` on the exit side, in clause order.
    let trail_arm = o.get("exit").and_then(first_arm_above_pct);
    let latched = clauses.iter().flatten().any(|r| r.latch.is_some() || r.since_armed);
    if latched && trail_arm.is_some() && !arm_clauses.is_empty() {
        return Err("a pnl latch together with `arm` clauses has no exact v2 form".into());
    }
    let has_ladder = o.get("scale_out").and_then(Value::as_array).is_some_and(|a| !a.is_empty());
    if (latched || !arm_clauses.is_empty()) && has_ladder {
        return Err("a latch together with a scale_out ladder has no exact v2 form".into());
    }

    let mut always = Vec::new();
    let mut start = Vec::new();
    let mut armed = Vec::new();
    for clause in &clauses {
        if clause.iter().any(|r| r.since_armed) && !clause.iter().any(|r| r.latch == Some(1.0)) {
            return Err("`since_armed` outside an `armed = 1` clause has no exact v2 form".into());
        }
        let latch = clause.iter().find_map(|r| r.latch);
        let mut when = conds_of(clause);
        // A lone trailing clause is gated at each evaluation by its `arm_above_pct`.
        if clause.len() == 1 {
            if let Some(x) = clause[0].arm_above {
                when.push(cond("m_position.pnl_pct", ">=", x));
            }
        }
        let authored = trail_arm.is_some() || !arm_clauses.is_empty();
        match (latch, authored) {
            (None, _) => always.push(sell_line(when)),
            // No latch authored: `armed` reads a vacuous 1 once held, NaN before.
            (Some(1.0), false) => {
                when.push(cond("m_position.held_sec", ">=", 0.0));
                always.push(sell_line(when));
            }
            (Some(_), false) => {} // `armed = 0` can never hold without a latch
            (Some(1.0), true) => {
                if when.is_empty() {
                    // A bare `armed = 1` clause sells as soon as the latch is set.
                    when.push(cond("m_position.held_sec", ">=", 0.0));
                }
                if let Some(x) = trail_arm {
                    // Latched before the exits of the crossing event.
                    let mut first = when.clone();
                    first.push(cond("m_position.pnl_pct", ">=", x));
                    start.push(sell_line(first));
                }
                armed.push(sell_line(when));
            }
            (Some(_), true) => {
                match trail_arm {
                    Some(x) => when.push(cond("m_position.pnl_pct", "<", x)),
                    None => when.push(cond("m_position.held_sec", ">=", 0.0)),
                }
                start.push(sell_line(when));
            }
        }
    }
    let staged = !start.is_empty() || !armed.is_empty() || !arm_clauses.is_empty();
    if staged {
        match trail_arm {
            Some(x) if arm_clauses.is_empty() => {
                start.push(json!({ "if": [cond("m_position.pnl_pct", ">=", x)], "go": "armed" }));
            }
            _ => {
                for clause in &arm_clauses {
                    if clause.iter().any(|r| r.latch.is_some() || r.since_armed) {
                        return Err("an `arm` clause cannot read the latch".into());
                    }
                    start.push(json!({ "if": conds_of(clause), "go": "armed" }));
                }
            }
        }
    }
    // Parked exit lines keep their place (validated like live, compiled by nothing).
    if let Some(e) = o.get("disabled").and_then(|d| d.get("exit")) {
        for clause in exit_clauses(e)? {
            let when = conds_of(&clause);
            if !when.is_empty() {
                always.push(off(sell_line(when)));
            }
        }
    }
    let mut stages = Vec::new();
    if staged {
        stages.push(json!({ "name": "start", "on": start }));
        stages.push(json!({ "name": "armed", "on": armed }));
    }
    if let Some(ladder) = o.get("scale_out").and_then(Value::as_array).filter(|a| !a.is_empty()) {
        let n = ladder.len();
        let name = |i: usize| format!("step_{}", i + 1);
        let mut remainder_seen = false;
        for (i, rung) in ladder.iter().enumerate() {
            let bps = rung.get("sell_bps").and_then(Value::as_f64);
            remainder_seen |= bps.is_none();
            let mut lines = Vec::new();
            let next = if i + 1 < n { name(i + 1) } else { "done".to_string() };
            let mk = |when: Vec<Value>| -> Value {
                match bps {
                    Some(b) => json!({ "if": when, "sell": true, "sell_pct": b / 100.0, "go": next }),
                    None => json!({ "if": when, "sell": true }),
                }
            };
            if let Some(tp) = rung.get("take_profit").and_then(Value::as_f64) {
                lines.push(mk(vec![cond("m_position.pnl_pct", ">=", tp)]));
            }
            if let Some(c) = rung.get("conditions") {
                for r in side_reads(c)? {
                    let mut when = vec![Value::Object(r.cond.clone().ok_or("`armed` cannot gate a rung")?)];
                    if let Some(x) = r.arm_above {
                        when.push(cond("m_position.pnl_pct", ">=", x));
                    }
                    lines.push(mk(when));
                }
            }
            stages.push(json!({ "name": name(i), "on": lines }));
        }
        if !remainder_seen {
            stages.push(json!({ "name": "done" }));
        }
    }
    // Parked ladder rungs: off lines in `always` (a parked rung is not a stage).
    if let Some(parked) = o.get("disabled").and_then(|d| d.get("scale_out")).and_then(Value::as_array) {
        for rung in parked {
            if let Some(c) = rung.get("conditions") {
                for r in side_reads(c)? {
                    if let Some(cv) = r.cond {
                        always.push(off(sell_line(vec![Value::Object(cv)])));
                    }
                }
            }
        }
    }
    if !always.is_empty() {
        root.insert("always".into(), Value::Array(always));
    }
    if !stages.is_empty() {
        root.insert("stages".into(), Value::Array(stages));
    }
    Ok(Value::Object(root))
}

/// The first `m_position.arm_above_pct` on a v1 exit side, in clause order.
fn first_arm_above_pct(exit: &Value) -> Option<f64> {
    let sides: Vec<&Value> = match exit {
        Value::Array(a) => a.iter().collect(),
        other => vec![other],
    };
    for s in sides {
        let insts = match s.get("m_position") {
            Some(Value::Array(a)) => a.iter().collect::<Vec<_>>(),
            Some(o @ Value::Object(_)) => vec![o],
            _ => continue,
        };
        for g in insts {
            if let Some(x) = g.get("arm_above_pct").and_then(Value::as_f64) {
                return Some(x);
            }
        }
    }
    None
}

// ── Fingerprints ─────────────────────────────────────────────────────────────

// ── Stored lab configs ───────────────────────────────────────────────────────

/// A v1 sweep's corpus-wide `ix_patterns` (`string[][]`) as the tags document the same
/// sweep reads in v2: tag `volume`, exactly what a fingerprint's `m_flow_ix` with those
/// patterns and default flags converts to.
pub fn ix_patterns_to_tags(patterns: &Value) -> Result<Value, String> {
    convert_metric_config(&json!({ "m_flow_ix": { "ix_patterns": patterns } }))
}

/// A v1 `scale_out` ladder (`ExitStage[]`) as v2 `stages`, the conversion a rule
/// carrying that ladder gets.
pub fn convert_ladder(ladder: &Value) -> Result<Value, String> {
    let v2 = convert_params(&json!({ "scale_out": ladder }))?;
    Ok(v2.get("stages").cloned().unwrap_or_else(|| json!([])))
}

/// A v1 sweep axis (`{kind, side, group, metric, operator, window, slice, values}`) as
/// the v2 axis (`{kind, side, metric: "m_family.name", tag, span, slice, operator,
/// values}`). A TP/SL axis and an already-v2 axis pass through.
pub fn convert_axis(axis: &Value) -> Result<Value, String> {
    let Some(o) = axis.as_object() else { return Err("an axis must be an object".into()) };
    let (Some(group), Some(metric)) = (o.get("group").and_then(Value::as_str), o.get("metric").and_then(Value::as_str)) else {
        return Ok(axis.clone());
    };
    let (path, tag) = map_metric(group, metric).ok_or_else(|| format!("{group}.{metric} has no v2 metric"))?;
    let span_of = |k: &str| -> Result<Option<String>, String> {
        match o.get(k) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Number(n)) => Ok(Some(format!("{}s", fmt_num(n.as_f64().unwrap_or(0.0))))),
            Some(Value::String(t)) => Ok(Some(t.clone())),
            Some(_) => Err(format!("{group}.{metric}: `{k}` must be a number or a span")),
        }
    };
    let mut out = Map::new();
    for k in ["kind", "side", "operator", "values"] {
        if let Some(v) = o.get(k) {
            out.insert(k.into(), v.clone());
        }
    }
    out.insert("metric".into(), json!(path));
    if let Some(t) = tag {
        out.insert("tag".into(), json!(t));
    }
    if let Some(sp) = span_of("window")? {
        out.insert("span".into(), json!(sp));
    }
    if let Some(sl) = span_of("slice")? {
        out.insert("slice".into(), json!(sl));
    }
    Ok(Value::Object(out))
}

/// Convert a v1 fingerprint `metric_config` into a v2 `tags` document.
pub fn convert_metric_config(cfg: &Value) -> Result<Value, String> {
    let Some(o) = cfg.as_object() else { return Ok(json!({})) };
    let mut tags = Map::new();
    if let Some(f) = o.get("m_flow_ix").and_then(Value::as_object) {
        let flag = |k: &str| f.get(k).and_then(Value::as_bool).unwrap_or(true);
        let mut m = Map::new();
        if let Some(p) = f.get("ix_patterns") {
            m.insert("ix_shape".into(), p.clone());
        }
        if let Some(p) = f.get("tagged_programs") {
            m.insert("program".into(), p.clone());
        }
        if let Some(c) = f.get("volume_cluster") {
            m.insert("cluster".into(), c.clone());
        }
        if let Some(x) = f.get("tagged_ix_markers") {
            m.insert("ix_contains".into(), x.clone());
        }
        if let Some(x) = f.get("untagged_ix_markers") {
            m.insert("ix_lacks".into(), x.clone());
        }
        if flag("creator_is_tagged") {
            m.insert("creator".into(), json!(true));
        }
        if m.is_empty() {
            // A v1 classifier that tags nothing: every trade on the untagged side.
            m.insert("ix_shape".into(), json!([]));
        }
        let mut t = Map::new();
        t.insert("match".into(), Value::Object(m));
        if flag("wallet_contagion") {
            t.insert("sticky".into(), json!(true));
        }
        if f.get("creation_slot_buyers").and_then(Value::as_str) == Some("excluded") {
            t.insert("exclude_creation_slot".into(), json!(true));
        }
        tags.insert(TAG_VOLUME.into(), Value::Object(t));
    }
    if let Some(d) = o.get("m_dump_ix").and_then(Value::as_object) {
        let mut m = Map::new();
        m.insert("ix_shape".into(), d.get("ix_patterns").cloned().unwrap_or(json!([])));
        if d.get("creator_is_listed").and_then(Value::as_bool) == Some(true) {
            m.insert("creator".into(), json!(true));
        }
        tags.insert(TAG_DUMP.into(), json!({ "match": m, "side": "sell" }));
    }
    if let Some(c) = o.get("m_copy").and_then(Value::as_object) {
        tags.insert(TAG_TARGETS.into(), json!({ "match": { "wallet": c.get("target_wallets").cloned().unwrap_or(json!([])) } }));
    }
    if let Some(b) = o.get("m_burst_slot").and_then(Value::as_object) {
        let mut templates = Vec::new();
        let mut programs = Vec::new();
        for k in ["working_templates", "working_programs"] {
            for v in b.get(k).and_then(Value::as_array).into_iter().flatten() {
                if let Some(s) = v.as_str().filter(|s| !s.is_empty()) {
                    if s.contains('|') {
                        templates.push(json!(s));
                    } else {
                        programs.push(json!(s));
                    }
                }
            }
        }
        let mut m = Map::new();
        if !templates.is_empty() {
            m.insert("ix_template".into(), Value::Array(templates));
        }
        if !programs.is_empty() {
            m.insert("program".into(), Value::Array(programs));
        }
        if !m.is_empty() {
            tags.insert(TAG_WORKING.into(), json!({ "match": m }));
        }
    }
    let doc = Value::Object(tags);
    crate::metrics::tags::config::validate_tags(&doc)?;
    Ok(doc)
}

/// Rename v1 criteria axis keys (`prior_identity_launches` -> `name_reuse_count`).
pub fn convert_criteria(criteria: &Value) -> Value {
    match criteria {
        Value::Object(o) => Value::Object(
            o.iter()
                .map(|(k, v)| (if k == "prior_identity_launches" { "name_reuse_count".to_string() } else { k.clone() }, v.clone()))
                .collect(),
        ),
        other => other.clone(),
    }
}

#[cfg(test)]
#[path = "v1_tests.rs"]
mod tests;
