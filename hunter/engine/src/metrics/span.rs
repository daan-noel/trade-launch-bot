//! **Spans** — the stretch of the tape a metric counts over, written after the metric
//! in brackets: `m_flow.buy_sol [10s]`.
//!
//! | JSON `span`   | meaning                                                   |
//! | ------------- | --------------------------------------------------------- |
//! | absent        | the coin's whole life                                     |
//! | `"10s"`       | the last 10 seconds, closed `[now - 10 s, now]`           |
//! | `"20sl"`      | the last 20 slots (a bundle lands in one slot)            |
//! | `"5p"`        | the last 5 prints of this coin (`1p` = the print read now)|
//! | `"10s@2"`     | the same 10 s, ending 2 s ago                             |
//! | `"age60s"`    | from coin age 60 s until now                              |
//!
//! Two metrics (`slice_*_share_pct`) also take a `slice`: a shorter window nested in the
//! span, same unit, same lag (`"span": "30s", "slice": "2s"`).
//!
//! A span is part of a condition's identity: two conditions on `buy_sol [10s]` share
//! one buffer, `buy_sol [10s]` and `buy_sol [10sl]` do not.

use serde_json::{json, Value};

use super::crowd_after_age::AgeAnchor;
use super::registry::MetricSpec;
use super::{WindowSpec, WindowUnit};

/// Prefix of a since-age span: `age60s`.
const AGE_PREFIX: &str = "age";

/// Which stretch a read covers. [`Span::LIFE`] is the default (no span written).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Span {
    /// A trailing window (`10s`, `20sl`, `5p`, with an optional `@lag`).
    pub window: Option<WindowSpec>,
    /// The nested slice of a two-window metric; same unit and lag as `window`.
    pub slice: Option<WindowSpec>,
    /// A since-age anchor (`age60s`).
    pub since_age: Option<AgeAnchor>,
}

impl Span {
    /// The coin's whole life.
    pub const LIFE: Self = Self { window: None, slice: None, since_age: None };

    pub fn window(spec: WindowSpec) -> Self {
        Self { window: Some(spec), slice: None, since_age: None }
    }

    /// A wall-clock window ending now.
    pub fn secs(size: f64) -> Self {
        Self::window(WindowSpec::secs(size))
    }

    pub fn sliced(window: WindowSpec, slice: WindowSpec) -> Self {
        Self { window: Some(window), slice: Some(slice), since_age: None }
    }

    pub fn since_age(anchor: AgeAnchor) -> Self {
        Self { window: None, slice: None, since_age: Some(anchor) }
    }

    pub fn is_life(self) -> bool {
        self.window.is_none() && self.since_age.is_none()
    }

    /// A trailing window on either axis.
    pub fn is_windowed(self) -> bool {
        self.window.is_some() || self.slice.is_some()
    }

    /// Either axis counts in slots — a loader must then supply the slot column, or the
    /// window's cursor never moves and it reads like a gate that never fires.
    pub fn needs_slot(self) -> bool {
        [self.window, self.slice].into_iter().flatten().any(|w| w.unit == WindowUnit::Slot)
    }

    /// Parse the `span` / `slice` pair of a condition.
    pub fn parse(span: Option<&str>, slice: Option<&str>) -> Result<Self, String> {
        let mut out = Self::LIFE;
        if let Some(s) = span.map(str::trim).filter(|s| !s.is_empty()) {
            if let Some(rest) = s.strip_prefix(AGE_PREFIX) {
                let secs = rest
                    .strip_suffix('s')
                    .and_then(|n| n.trim().parse::<f64>().ok())
                    .filter(|v| v.is_finite() && *v >= 0.0)
                    .ok_or_else(|| format!("span `{s}`: a since-age span is `age<seconds>s`, e.g. `age60s`"))?;
                out.since_age = Some(AgeAnchor::secs(secs));
            } else {
                out.window = Some(WindowSpec::parse(s).ok_or_else(|| {
                    format!("span `{s}`: expected `10s` (seconds), `20sl` (slots), `5p` (prints), `10s@2` (lagged) or `age60s`")
                })?);
            }
        }
        if let Some(s) = slice.map(str::trim).filter(|s| !s.is_empty()) {
            let w = out.window.ok_or("a slice needs a window span beside it")?;
            let sl = WindowSpec::parse(s).ok_or_else(|| format!("slice `{s}`: expected e.g. `2s`"))?;
            if sl.unit != w.unit {
                return Err(format!("slice `{s}` must count in the span's unit (`{}`)", w.label()));
            }
            if sl.lag != 0.0 {
                return Err(format!("slice `{s}` takes the span's lag; write the lag on the span"));
            }
            if sl.size > w.size {
                return Err(format!("slice `{s}` is wider than its span `{}`", w.label()));
            }
            out.slice = Some(WindowSpec { size: sl.size, lag: w.lag, unit: w.unit });
        }
        Ok(out)
    }

    /// The `span` JSON value (`None` = life).
    pub fn span_text(self) -> Option<String> {
        if let Some(w) = self.window {
            return Some(w.label());
        }
        self.since_age
            .map(|a| format!("{AGE_PREFIX}{}s", crate::event::format_metric_threshold(a.secs_f64())))
    }

    /// The `slice` JSON value.
    pub fn slice_text(self) -> Option<String> {
        self.slice.map(|s| WindowSpec { lag: 0.0, ..s }.label())
    }

    /// `[10s]`, `[30s, slice 2s]`, `[age60s]`, or empty for life. The one spelling in
    /// labels and exit reasons.
    pub fn bracket(self) -> String {
        match (self.span_text(), self.slice_text()) {
            (None, _) => String::new(),
            (Some(s), None) => format!("[{s}]"),
            (Some(s), Some(sl)) => format!("[{s}, slice {sl}]"),
        }
    }

    /// Whether `spec` accepts this span.
    pub fn check_allowed(self, spec: &MetricSpec) -> Result<(), String> {
        let u = spec.spans;
        let path = spec.path();
        if self.since_age.is_some() {
            return if u.since_age {
                Ok(())
            } else {
                Err(format!("{path} does not take a since-age span"))
            };
        }
        match self.window {
            None if u.life => Ok(()),
            None if u.since_age => Err(format!("{path} needs a since-age span, e.g. `age60s`")),
            None => Err(format!("{path} needs a window span, e.g. `10s`")),
            Some(_) if !u.window => Err(format!("{path} counts over the whole life and takes no window")),
            Some(_) if u.slice && self.slice.is_none() => {
                Err(format!("{path} needs a slice beside its span, e.g. `\"slice\": \"2s\"`"))
            }
            Some(_) if !u.slice && self.slice.is_some() => Err(format!("{path} takes no slice")),
            Some(_) => Ok(()),
        }
    }
}

/// The span kinds, for the registry document.
pub fn span_kinds_json() -> Value {
    json!([
        { "key": "life", "text": "", "title": "Whole life",
          "summary": "Since the coin was created. The default: no span written.",
          "example": "m_flow.buy_sol >= 20 : 20+ SOL bought since the coin was created." },
        { "key": "sec", "text": "10s", "title": "Last N seconds",
          "summary": "The last N seconds up to now.",
          "example": "m_flow.buy_sol [10s] >= 5 : 5+ SOL bought in the last 10 s." },
        { "key": "slot", "text": "20sl", "title": "Last N slots",
          "summary": "The last N slots (about 0.4 s each). A bundle always lands in one slot, so 1sl is exactly 'this slot'.",
          "example": "m_flow.buy_count [1sl] >= 3 : 3+ buys landed in this slot." },
        { "key": "print", "text": "5p", "title": "Last N prints",
          "summary": "The last N prints of this coin. 1p is the print being read.",
          "example": "m_flow.sell_tx_count @dump [1p] >= 1 : the print being read is a dump sell." },
        { "key": "lag", "text": "10s@2", "title": "Lagged",
          "summary": "The same window ending N units ago, so it cannot see the most recent stretch.",
          "example": "m_flow.buy_count [30sl@1] = 0 : no buys in the 30 slots before this one." },
        { "key": "since_age", "text": "age60s", "title": "Since age",
          "summary": "From coin age N seconds until now.",
          "example": "m_crowd.buyer_count [age60s] >= 5 : 5+ new wallets bought after the coin was a minute old." },
        { "key": "slice", "text": "slice 2s", "title": "Slice",
          "summary": "A shorter window nested in the span, same unit, for the two slice_*_share_pct metrics.",
          "example": "m_flow.slice_sol_share_pct [30s, slice 2s] >= 50 : half of 30 s of SOL moved in the last 2 s." },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_round_trip() {
        for (span, slice) in [
            (None, None),
            (Some("10s"), None),
            (Some("20sl"), None),
            (Some("5p"), None),
            (Some("10s@2"), None),
            (Some("age60s"), None),
            (Some("age0s"), None),
            (Some("30s"), Some("2s")),
            (Some("30sl@1"), Some("4sl")),
        ] {
            let s = Span::parse(span, slice).unwrap();
            assert_eq!(s.span_text().as_deref(), span, "{span:?}");
            assert_eq!(s.slice_text().as_deref(), slice, "{slice:?}");
        }
    }

    #[test]
    fn a_slice_rides_the_span_clock() {
        let s = Span::parse(Some("30sl@1"), Some("4sl")).unwrap();
        assert_eq!(s.slice, Some(WindowSpec { size: 4.0, lag: 1.0, unit: WindowUnit::Slot }));
        assert!(Span::parse(Some("30s"), Some("4sl")).is_err(), "unit must match");
        assert!(Span::parse(Some("3s"), Some("4s")).is_err(), "slice wider than span");
        assert!(Span::parse(None, Some("4s")).is_err(), "slice without span");
    }

    #[test]
    fn garbage_is_an_error_not_a_default() {
        for bad in ["10x", "age", "ageXs", "-5s", "0s"] {
            assert!(Span::parse(Some(bad), None).is_err(), "{bad}");
        }
    }
}
