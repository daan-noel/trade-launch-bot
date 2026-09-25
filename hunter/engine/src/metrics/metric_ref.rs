//! [`MetricRef`] — one read, fully named: which metric, whose trades, over what span.
//! `m_flow.buy_sol @!volume [10s]`.
//!
//! A reference is a condition's identity: two conditions with equal references read
//! the same number from the same buffer, which is how the engine dedupes windows and
//! how a readout, a blocker or a sweep axis names what it read.

use serde_json::{json, Map, Value};

use super::registry::{metric_by_path, metric_spec, Family, Metric, TagLevel, TagUse};
use super::span::Span;
use super::tags::TagRef;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MetricRef {
    pub metric: Metric,
    pub tag: Option<TagRef>,
    pub span: Span,
}

impl MetricRef {
    /// An untagged read over the coin's life.
    pub fn life(metric: Metric) -> Self {
        Self { metric, tag: None, span: Span::LIFE }
    }

    pub fn with_span(mut self, span: Span) -> Self {
        self.span = span;
        self
    }

    pub fn with_tag(mut self, tag: TagRef) -> Self {
        self.tag = Some(tag);
        self
    }

    /// Parse the `metric` / `tag` / `span` / `slice` keys of a condition object and
    /// check them against what the metric accepts.
    pub fn from_json(obj: &Map<String, Value>) -> Result<Self, String> {
        let path = obj
            .get("metric")
            .and_then(Value::as_str)
            .ok_or("a condition needs \"metric\": \"m_family.name\"")?;
        let metric = metric_by_path(path).ok_or_else(|| unknown_metric(path))?;
        let text = |k: &str| -> Result<Option<&str>, String> {
            match obj.get(k) {
                None | Some(Value::Null) => Ok(None),
                Some(Value::String(s)) => Ok(Some(s.as_str())),
                Some(_) => Err(format!("{path}: `{k}` must be a string")),
            }
        };
        let tag = text("tag")?.map(TagRef::parse).transpose().map_err(|e| format!("{path}: {e}"))?;
        let span = Span::parse(text("span")?, text("slice")?).map_err(|e| format!("{path}: {e}"))?;
        let r = Self { metric, tag, span };
        r.check()?;
        Ok(r)
    }

    /// The `metric` / `tag` / `span` / `slice` keys, the inverse of [`from_json`](Self::from_json).
    pub fn write_json(&self, obj: &mut Map<String, Value>) {
        obj.insert("metric".into(), json!(metric_spec(self.metric).path()));
        if let Some(t) = self.tag {
            obj.insert("tag".into(), json!(t.text()));
        }
        if let Some(s) = self.span.span_text() {
            obj.insert("span".into(), json!(s));
        }
        if let Some(s) = self.span.slice_text() {
            obj.insert("slice".into(), json!(s));
        }
    }

    /// Whether the metric accepts this tag and this span.
    pub fn check(&self) -> Result<(), String> {
        let spec = metric_spec(self.metric);
        let path = spec.path();
        match (spec.tags, self.tag) {
            (TagUse::None, Some(t)) => return Err(format!("{path} takes no tag (got {})", t.at())),
            (TagUse::Required, None) => {
                return Err(format!("{path} needs a tag: whose trades? e.g. `\"tag\": \"volume\"`"))
            }
            _ => {}
        }
        if let Some(t) = self.tag {
            match spec.tag_level {
                TagLevel::WalletClass if !t.is_builtin() => {
                    return Err(format!(
                        "{path} reads a wallet class: `{}` (bundled or public_app)",
                        super::tags::BUILTIN.join("` or `")
                    ))
                }
                TagLevel::Trade | TagLevel::Template if t.is_builtin() => {
                    return Err(format!("{path}: `{}` is a wallet class, not a fingerprint tag", t.name))
                }
                TagLevel::Template | TagLevel::WalletClass if t.negated => {
                    return Err(format!("{path} reads the tagged trades only; `@!{}` has no meaning here", t.name))
                }
                _ => {}
            }
        }
        self.span.check_allowed(spec)
    }

    /// `m_flow.buy_sol @!volume [10s]` — the one spelling in labels, readouts and
    /// errors.
    pub fn label(&self) -> String {
        let mut s = metric_spec(self.metric).path();
        if let Some(t) = self.tag {
            s.push(' ');
            s.push_str(&t.at());
        }
        let b = self.span.bracket();
        if !b.is_empty() {
            s.push(' ');
            s.push_str(&b);
        }
        s
    }

    /// Reads our position, not the coin.
    pub fn is_position(&self) -> bool {
        self.metric.family() == Family::Position
    }

    /// Reads per-fingerprint state (a fingerprint tag), so the read needs a
    /// fingerprint. Built-in wallet classes are coin-level.
    pub fn is_fingerprint_scoped(&self) -> bool {
        self.tag.is_some_and(|t| !t.is_builtin())
    }

    /// Never decreases over the coin's life (life or since-age span only).
    pub fn is_monotonic(&self) -> bool {
        metric_spec(self.metric).monotonic && !self.span.is_windowed()
    }

    pub fn needs_wallet_identity(&self) -> bool {
        self.metric.needs_wallet_identity(self.tag.is_some())
    }

    pub fn needs_ix_labels(&self) -> bool {
        self.metric.needs_ix_labels(self.tag.is_some())
    }
}

/// Every read of `spec` a chart draws, each one a reference the rule grammar accepts:
/// untagged, and each tag it can take — every name in `trade_tags` or
/// `template_tags` (by the metric's [`TagLevel`]) with its negation, or the built-in
/// wallet classes — over each span it accepts: the life, each of `windows`, each nested
/// pair of `windows` for a sliced metric (same unit, slice narrower), and since age 0
/// for a since-age metric (the whole life, the one anchor that needs no choosing).
pub fn chart_reads(
    spec: &super::registry::MetricSpec,
    trade_tags: &[&str],
    template_tags: &[&str],
    windows: &[super::WindowSpec],
) -> Vec<MetricRef> {
    let mut tags: Vec<Option<TagRef>> = vec![None];
    let names: Vec<&str> = match spec.tag_level {
        TagLevel::Trade => trade_tags.to_vec(),
        TagLevel::Template => template_tags.to_vec(),
        TagLevel::WalletClass => super::tags::BUILTIN.to_vec(),
    };
    if spec.tags != TagUse::None {
        for n in names {
            for text in [n.to_string(), format!("!{n}")] {
                if let Ok(t) = TagRef::parse(&text) {
                    tags.push(Some(t));
                }
            }
        }
    }
    let mut spans: Vec<Span> = Vec::new();
    if spec.spans.life {
        spans.push(Span::default());
    }
    if spec.spans.slice {
        for &w in windows {
            for &s in windows.iter().filter(|s| s.unit == w.unit && s.size < w.size) {
                spans.push(Span::sliced(w, s));
            }
        }
    } else if spec.spans.window {
        spans.extend(windows.iter().map(|&w| Span::window(w)));
    }
    if spec.spans.since_age {
        spans.push(Span::since_age(super::crowd_after_age::AgeAnchor::secs(0.0)));
    }
    let mut out = Vec::new();
    for tag in &tags {
        for &span in &spans {
            let r = MetricRef { metric: spec.id, tag: *tag, span };
            if r.check().is_ok() && !out.contains(&r) {
                out.push(r);
            }
        }
    }
    out
}

/// An unknown-metric error that names the nearest spelling a reader might mean.
fn unknown_metric(path: &str) -> String {
    let name = path.rsplit('.').next().unwrap_or(path);
    let near: Vec<String> = super::registry::METRICS
        .iter()
        .filter(|m| m.name.contains(name) || name.contains(m.name))
        .map(|m| m.path())
        .take(4)
        .collect();
    if near.is_empty() {
        format!("unknown metric `{path}`")
    } else {
        format!("unknown metric `{path}` (did you mean {}?)", near.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(v: Value) -> Result<MetricRef, String> {
        MetricRef::from_json(v.as_object().unwrap())
    }

    #[test]
    fn a_full_reference_round_trips() {
        let v = json!({ "metric": "m_flow.buy_sol", "tag": "!volume", "span": "10s" });
        let r = parse(v.clone()).unwrap();
        assert_eq!(r.label(), "m_flow.buy_sol @!volume [10s]");
        let mut back = Map::new();
        r.write_json(&mut back);
        assert_eq!(Value::Object(back), v);
    }

    #[test]
    fn what_a_metric_does_not_accept_is_refused_with_a_reason() {
        for (v, needle) in [
            (json!({ "metric": "m_flow.buy_sol" , "span": "age60s" }), "since-age"),
            (json!({ "metric": "m_state.age_sec", "tag": "volume" }), "takes no tag"),
            (json!({ "metric": "m_holdings.profit_sol" }), "needs a tag"),
            (json!({ "metric": "m_holdings.profit_sol", "tag": "volume", "span": "10s" }), "whole life"),
            (json!({ "metric": "m_holdings.bag_share_pct", "tag": "volume" }), "wallet class"),
            (json!({ "metric": "m_flow.buy_sol", "tag": "bundled" }), "not a fingerprint tag"),
            (json!({ "metric": "m_slot.buy_count", "tag": "!working" }), "no meaning"),
            (json!({ "metric": "m_flow.slice_sol_share_pct", "span": "30s" }), "needs a slice"),
            (json!({ "metric": "m_crowd.unique_wallets" }), "needs a window"),
            (json!({ "metric": "m_flow.buys" }), "unknown metric"),
        ] {
            let e = parse(v.clone()).unwrap_err();
            assert!(e.contains(needle), "{v} -> {e}");
        }
    }

    #[test]
    fn chart_reads_cover_each_accepted_tag_and_span_and_nothing_else() {
        use super::super::registry::metric_spec;
        use super::super::WindowSpec;
        let w = [WindowSpec::secs(10.0), WindowSpec::secs(30.0)];
        let labels = |m: Metric| -> Vec<String> {
            chart_reads(metric_spec(m), &["volume"], &["working"], &w).iter().map(MetricRef::label).collect()
        };
        assert_eq!(labels(Metric::AgeSec), vec!["m_state.age_sec"]);
        let buy = labels(Metric::BuySol);
        for want in ["m_flow.buy_sol", "m_flow.buy_sol [10s]", "m_flow.buy_sol @volume [30s]", "m_flow.buy_sol @!volume"] {
            assert!(buy.contains(&want.to_string()), "{want} in {buy:?}");
        }
        assert_eq!(labels(Metric::SliceSolSharePct), vec!["m_flow.slice_sol_share_pct [30s, slice 10s]"]);
        assert!(labels(Metric::BagSharePct).iter().all(|l| l.contains("@bundled") || l.contains("@public_app")));
        assert!(labels(Metric::SlotBuyCount).iter().all(|l| !l.contains("volume") && !l.contains("!working")));
    }

    #[test]
    fn identity_and_scope() {
        let r = parse(json!({ "metric": "m_flow.buy_sol", "tag": "volume" })).unwrap();
        assert!(r.is_fingerprint_scoped() && r.is_monotonic() && r.needs_wallet_identity());
        let w = parse(json!({ "metric": "m_flow.buy_sol", "span": "10s" })).unwrap();
        assert!(!w.is_fingerprint_scoped() && !w.is_monotonic());
        let b = parse(json!({ "metric": "m_holdings.bag_share_pct", "tag": "bundled" })).unwrap();
        assert!(!b.is_fingerprint_scoped());
    }
}
