//! Registry → entry roles and exit bags ([rule-search-method.md] §3).
//!
//! A metric is a quantity. Side + operator + cut make the clause. Classification
//! reads registry flags (`scope`, `monotonic`, `kind`, `family`, unit) plus a
//! small set of named compete pairs (giveback, clock, trigger family). A new
//! registry row joins by those flags; unknown / `Standalone` is its own exclusive
//! trigger family and may OR with other exit families.

use hunter_engine::metrics::{
    group_of, metric_spec, MetricFamily, MetricGroupId, MetricId, MetricKind, MetricScope,
};

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
    /// A group whose family is unknown / `Standalone`.
    Standalone,
}

/// How an exit clause joins the OR bag (0–2 different metrics).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExitRole {
    /// `trail` XOR `retrace` (token ATH vs since-entry peak).
    Giveback,
    /// `stall` XOR `held` (token quiet vs our fill).
    Clock,
    /// `pnl` / `bounce` / `rise` — different metrics may OR.
    Progress,
    /// Any distinct flow or split metric.
    Flow,
}

/// Compete slot: at most one filling per combo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CompeteKey {
    /// Same metric id (one threshold / one windowed view of the same quantity).
    Metric(MetricId),
    Giveback,
    Clock,
    Trigger(TriggerFamily),
}

/// Token-ATH trail views (lifetime + window) share the giveback / dip slot.
pub fn is_token_trail(id: MetricId) -> bool {
    matches!(id, MetricId::Trail | MetricId::WinTrail)
}

/// Token-path rise views share the rise trigger / progress slot.
pub fn is_token_rise(id: MetricId) -> bool {
    matches!(id, MetricId::LifeRise | MetricId::WinRise)
}

pub fn is_position(id: MetricId) -> bool {
    group_of(id).scope == MetricScope::Position
}

/// Entry-side role. `None` ⇒ not legal on entry (position-scoped).
pub fn entry_role(id: MetricId) -> Option<EntryRole> {
    let g = group_of(id);
    if g.scope == MetricScope::Position {
        return None;
    }
    if is_token_trail(id) {
        return Some(EntryRole::Trigger(TriggerFamily::Dip));
    }
    if is_token_rise(id) {
        return Some(EntryRole::Trigger(TriggerFamily::Rise));
    }
    match id {
        MetricId::Time | MetricId::Liquidity | MetricId::Stall => Some(EntryRole::Selector),
        MetricId::UniqueWallets => Some(EntryRole::Extra),
        MetricId::UntaggedBuy
        | MetricId::UntaggedNet
        | MetricId::UntaggedGross
        | MetricId::WinUntaggedBuy
        | MetricId::WinUntaggedNet
        | MetricId::WinUntaggedGross => Some(EntryRole::Trigger(TriggerFamily::Organic)),
        _ => match (g.family, g.kind, g.scope, metric_is_monotonic(id)) {
            (MetricFamily::FlowIx, MetricKind::Dynamic, _, _) => Some(EntryRole::Extra),
            (MetricFamily::Flow, MetricKind::Dynamic, _, _) => {
                if matches!(
                    id,
                    MetricId::Buy | MetricId::GrossFlow | MetricId::NetFlow
                ) {
                    Some(EntryRole::Trigger(TriggerFamily::Accumulation))
                } else {
                    Some(EntryRole::Extra)
                }
            }
            (_, MetricKind::Static, MetricScope::Token, true) => Some(EntryRole::WaitOnly),
            (MetricFamily::Standalone | MetricFamily::Burst, _, _, _) => {
                Some(EntryRole::Trigger(TriggerFamily::Standalone))
            }
            (MetricFamily::FlowIx, MetricKind::Static, _, true) => Some(EntryRole::WaitOnly),
            (MetricFamily::Flow, MetricKind::Static, _, true) => Some(EntryRole::WaitOnly),
            _ => Some(EntryRole::Selector),
        },
    }
}

/// Exit-side role. Every token- or position-scoped metric is legal on exit.
pub fn exit_role(id: MetricId) -> ExitRole {
    if is_token_trail(id) || matches!(id, MetricId::Retrace) {
        return ExitRole::Giveback;
    }
    if matches!(id, MetricId::Stall | MetricId::Held) {
        return ExitRole::Clock;
    }
    if matches!(id, MetricId::Pnl | MetricId::Bounce) || is_token_rise(id) {
        return ExitRole::Progress;
    }
    match group_of(id).family {
        MetricFamily::Flow | MetricFamily::FlowIx => ExitRole::Flow,
        _ => ExitRole::Progress,
    }
}

pub fn entry_compete(id: MetricId) -> Option<CompeteKey> {
    match entry_role(id)? {
        EntryRole::Trigger(fam) => Some(CompeteKey::Trigger(fam)),
        EntryRole::WaitOnly => None,
        _ => Some(CompeteKey::Metric(id)),
    }
}

pub fn exit_compete(id: MetricId) -> CompeteKey {
    match exit_role(id) {
        ExitRole::Giveback => CompeteKey::Giveback,
        ExitRole::Clock => CompeteKey::Clock,
        ExitRole::Progress | ExitRole::Flow => CompeteKey::Metric(id),
    }
}

/// Two exit clauses may not share a bag.
pub fn exit_competes(a: MetricId, b: MetricId) -> bool {
    if a == b {
        return true;
    }
    exit_compete(a) == exit_compete(b)
}

fn metric_is_monotonic(id: MetricId) -> bool {
    metric_spec(id).monotonic
}

/// Every registry metric that is legal on `side`.
pub fn legal_metrics(entry: bool) -> Vec<(MetricGroupId, MetricId)> {
    let mut out = Vec::new();
    for g in hunter_engine::metrics::REGISTRY {
        for m in g.metrics {
            if entry && g.scope == MetricScope::Position {
                continue;
            }
            out.push((g.id, m.id));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_is_exit_only() {
        assert!(entry_role(MetricId::Retrace).is_none());
        assert!(entry_role(MetricId::Held).is_none());
        assert!(entry_role(MetricId::Pnl).is_none());
        assert_eq!(exit_role(MetricId::Retrace), ExitRole::Giveback);
        assert_eq!(exit_role(MetricId::Held), ExitRole::Clock);
        assert_eq!(exit_role(MetricId::Pnl), ExitRole::Progress);
    }

    #[test]
    fn giveback_and_clock_compete() {
        assert!(exit_competes(MetricId::Trail, MetricId::Retrace));
        assert!(exit_competes(MetricId::WinTrail, MetricId::Retrace));
        assert!(exit_competes(MetricId::Stall, MetricId::Held));
        assert!(!exit_competes(MetricId::Trail, MetricId::Stall));
        assert!(!exit_competes(MetricId::Pnl, MetricId::Bounce));
    }

    #[test]
    fn trigger_families_are_exclusive() {
        assert_eq!(
            entry_role(MetricId::Trail),
            Some(EntryRole::Trigger(TriggerFamily::Dip))
        );
        assert_eq!(
            entry_role(MetricId::LifeRise),
            Some(EntryRole::Trigger(TriggerFamily::Rise))
        );
        assert_eq!(
            entry_compete(MetricId::Trail),
            entry_compete(MetricId::WinTrail)
        );
        assert_ne!(
            entry_compete(MetricId::Trail),
            entry_compete(MetricId::LifeRise)
        );
    }

    #[test]
    fn wait_only_is_not_a_selector() {
        assert_eq!(entry_role(MetricId::LifeBuy), Some(EntryRole::WaitOnly));
        assert_eq!(entry_role(MetricId::LifeGrossFlow), Some(EntryRole::WaitOnly));
        assert_eq!(entry_role(MetricId::Time), Some(EntryRole::Selector));
    }

    #[test]
    fn flow_window_is_extra_or_accumulation() {
        assert_eq!(
            entry_role(MetricId::Buy),
            Some(EntryRole::Trigger(TriggerFamily::Accumulation))
        );
        assert_eq!(entry_role(MetricId::UniqueWallets), Some(EntryRole::Extra));
        assert_eq!(exit_role(MetricId::Buy), ExitRole::Flow);
        assert_eq!(exit_role(MetricId::UntaggedNet), ExitRole::Flow);
    }
}
