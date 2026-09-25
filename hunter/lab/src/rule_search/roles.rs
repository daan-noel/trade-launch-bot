//! Registry → entry roles and exit bags (`docs/arch/sweep.md` "Rule search").
//!
//! A read is a quantity. Side + operator + cut make the clause. Classification
//! reads the registry (family, `monotonic`) and the read's own tag and span, plus a
//! small set of named compete pairs (giveback, clock, trigger family). A new
//! registry row joins by those; a `m_slot` / `m_wave` read is its own exclusive
//! trigger family and may OR with other exit families.

use hunter_engine::metrics::{Family, Metric, MetricRef, TagRef};

/// How an entry clause joins the AND product.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EntryRole {
    /// Can permanently fail: time / liquidity bands and other bounded token metrics.
    Selector,
    /// Second can-fail: windowed flow / wallets / split floors at a checkpoint.
    Extra,
    /// Times the buy. One family per filling — never two.
    Trigger(TriggerFamily),
    /// Monotonic lifetime floor with no cap — not a selector, not generated as entry.
    WaitOnly,
}

/// Exclusive trigger families. One per entry filling.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TriggerFamily {
    Dip,
    Rise,
    Accumulation,
    Organic,
    /// A `m_slot` / `m_wave` read (this slot's or this wave's buys).
    Standalone,
}

/// How an exit clause joins the OR bag (0–2 different quantities).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExitRole {
    /// `trail` XOR `retrace` (token ATH vs since-entry peak).
    Giveback,
    /// `stall` XOR `held` (token quiet vs our fill).
    Clock,
    /// `pnl` / `bounce` / `rise` — different quantities may OR.
    Progress,
    /// Any distinct flow, crowd or holdings quantity.
    Flow,
}

/// One quantity a read measures: the metric, whose trades, and whether it counts
/// over a trailing window. Two reads of one quantity differ only in the window's
/// size — one threshold / one windowed view of the same number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Quantity {
    pub metric: Metric,
    pub windowed: bool,
    pub tag: Option<TagRef>,
}

/// The quantity `r` reads.
pub fn quantity(r: &MetricRef) -> Quantity {
    Quantity { metric: r.metric, windowed: r.span.is_windowed(), tag: r.tag }
}

/// Compete slot: at most one filling per combo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CompeteKey {
    /// Same quantity (one threshold / one windowed view of the same number).
    Metric(Quantity),
    Giveback,
    Clock,
    Trigger(TriggerFamily),
}

/// Token-ATH trail views (life + window) share the giveback / dip slot.
pub fn is_token_trail(r: &MetricRef) -> bool {
    r.metric == Metric::TrailPct
}

/// Token-path rise views share the rise trigger / progress slot.
pub fn is_token_rise(r: &MetricRef) -> bool {
    r.metric == Metric::RisePct
}

/// Money in / net / turnover: the flow quantities a trigger times the buy on.
fn is_flow_sum(m: Metric) -> bool {
    matches!(m, Metric::BuySol | Metric::NetSol | Metric::GrossSol)
}

/// Entry-side role. `None` ⇒ not legal on entry (position-scoped).
pub fn entry_role(r: &MetricRef) -> Option<EntryRole> {
    if r.is_position() {
        return None;
    }
    if is_token_trail(r) {
        return Some(EntryRole::Trigger(TriggerFamily::Dip));
    }
    if is_token_rise(r) {
        return Some(EntryRole::Trigger(TriggerFamily::Rise));
    }
    let family = r.metric.family();
    let windowed = r.span.is_windowed();
    // The trades a fingerprint tag leaves out: the organic tape.
    let untagged_split = r.tag.is_some_and(|t| t.negated);
    match r.metric {
        Metric::AgeSec | Metric::LiquiditySol | Metric::StallSec => Some(EntryRole::Selector),
        Metric::UniqueWallets => Some(EntryRole::Extra),
        m if family == Family::Flow && is_flow_sum(m) && untagged_split => {
            Some(EntryRole::Trigger(TriggerFamily::Organic))
        }
        // A tagged split or a build count over a window.
        _ if windowed && (r.tag.is_some() || r.metric == Metric::UniqueIxShapes) => Some(EntryRole::Extra),
        m if windowed && matches!(family, Family::Flow | Family::Crowd) => {
            if is_flow_sum(m) {
                Some(EntryRole::Trigger(TriggerFamily::Accumulation))
            } else {
                Some(EntryRole::Extra)
            }
        }
        _ if r.span.is_life() && r.is_monotonic() => Some(EntryRole::WaitOnly),
        _ if matches!(family, Family::Slot | Family::Wave) => Some(EntryRole::Trigger(TriggerFamily::Standalone)),
        _ => Some(EntryRole::Selector),
    }
}

/// Exit-side role. Every token- or position-scoped read is legal on exit.
pub fn exit_role(r: &MetricRef) -> ExitRole {
    if is_token_trail(r) || r.metric == Metric::RetracePct {
        return ExitRole::Giveback;
    }
    if matches!(r.metric, Metric::StallSec | Metric::HeldSec) {
        return ExitRole::Clock;
    }
    if matches!(r.metric, Metric::PnlPct | Metric::BouncePct) || is_token_rise(r) {
        return ExitRole::Progress;
    }
    match r.metric.family() {
        Family::Flow | Family::Crowd | Family::Holdings | Family::Print => ExitRole::Flow,
        _ => ExitRole::Progress,
    }
}

pub fn entry_compete(r: &MetricRef) -> Option<CompeteKey> {
    match entry_role(r)? {
        EntryRole::Trigger(fam) => Some(CompeteKey::Trigger(fam)),
        EntryRole::WaitOnly => None,
        _ => Some(CompeteKey::Metric(quantity(r))),
    }
}

pub fn exit_compete(r: &MetricRef) -> CompeteKey {
    match exit_role(r) {
        ExitRole::Giveback => CompeteKey::Giveback,
        ExitRole::Clock => CompeteKey::Clock,
        ExitRole::Progress | ExitRole::Flow => CompeteKey::Metric(quantity(r)),
    }
}

/// Two exit clauses may not share a bag.
pub fn exit_competes(a: &MetricRef, b: &MetricRef) -> bool {
    quantity(a) == quantity(b) || exit_compete(a) == exit_compete(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hunter_engine::metrics::Span;

    fn life(m: Metric) -> MetricRef {
        MetricRef::life(m)
    }

    fn win(m: Metric) -> MetricRef {
        MetricRef::life(m).with_span(Span::secs(10.0))
    }

    fn untagged(r: MetricRef) -> MetricRef {
        r.with_tag(TagRef::parse("!volume").unwrap())
    }

    #[test]
    fn position_is_exit_only() {
        assert!(entry_role(&life(Metric::RetracePct)).is_none());
        assert!(entry_role(&life(Metric::HeldSec)).is_none());
        assert!(entry_role(&life(Metric::PnlPct)).is_none());
        assert_eq!(exit_role(&life(Metric::RetracePct)), ExitRole::Giveback);
        assert_eq!(exit_role(&life(Metric::HeldSec)), ExitRole::Clock);
        assert_eq!(exit_role(&life(Metric::PnlPct)), ExitRole::Progress);
    }

    #[test]
    fn giveback_and_clock_compete() {
        assert!(exit_competes(&life(Metric::TrailPct), &life(Metric::RetracePct)));
        assert!(exit_competes(&win(Metric::TrailPct), &life(Metric::RetracePct)));
        assert!(exit_competes(&life(Metric::StallSec), &life(Metric::HeldSec)));
        assert!(!exit_competes(&life(Metric::TrailPct), &life(Metric::StallSec)));
        assert!(!exit_competes(&life(Metric::PnlPct), &life(Metric::BouncePct)));
    }

    #[test]
    fn trigger_families_are_exclusive() {
        assert_eq!(entry_role(&life(Metric::TrailPct)), Some(EntryRole::Trigger(TriggerFamily::Dip)));
        assert_eq!(entry_role(&life(Metric::RisePct)), Some(EntryRole::Trigger(TriggerFamily::Rise)));
        assert_eq!(entry_compete(&life(Metric::TrailPct)), entry_compete(&win(Metric::TrailPct)));
        assert_ne!(entry_compete(&life(Metric::TrailPct)), entry_compete(&life(Metric::RisePct)));
    }

    #[test]
    fn wait_only_is_not_a_selector() {
        assert_eq!(entry_role(&life(Metric::BuySol)), Some(EntryRole::WaitOnly));
        assert_eq!(entry_role(&life(Metric::GrossSol)), Some(EntryRole::WaitOnly));
        assert_eq!(entry_role(&life(Metric::AgeSec)), Some(EntryRole::Selector));
    }

    #[test]
    fn flow_window_is_extra_or_accumulation() {
        assert_eq!(entry_role(&win(Metric::BuySol)), Some(EntryRole::Trigger(TriggerFamily::Accumulation)));
        assert_eq!(entry_role(&win(Metric::UniqueWallets)), Some(EntryRole::Extra));
        assert_eq!(exit_role(&win(Metric::BuySol)), ExitRole::Flow);
        assert_eq!(exit_role(&untagged(life(Metric::NetSol))), ExitRole::Flow);
    }

    #[test]
    fn the_untagged_split_is_organic_and_the_tagged_one_extra() {
        let organic = Some(EntryRole::Trigger(TriggerFamily::Organic));
        assert_eq!(entry_role(&untagged(life(Metric::BuySol))), organic);
        assert_eq!(entry_role(&untagged(win(Metric::GrossSol))), organic);
        let tagged = win(Metric::BuySol).with_tag(TagRef::parse("volume").unwrap());
        assert_eq!(entry_role(&tagged), Some(EntryRole::Extra));
    }

    #[test]
    fn one_quantity_is_one_compete_slot_across_window_sizes() {
        let a = win(Metric::UniqueWallets);
        let b = MetricRef::life(Metric::UniqueWallets).with_span(Span::secs(30.0));
        assert_eq!(entry_compete(&a), entry_compete(&b));
        // Windowed and life views of one metric, and its two tag halves, are distinct.
        assert_ne!(quantity(&win(Metric::SellSol)), quantity(&life(Metric::SellSol)));
        assert_ne!(quantity(&life(Metric::SellSol)), quantity(&untagged(life(Metric::SellSol))));
    }
}
