//! Metrics framework — the self-describing vocabulary of the generic strategy
//! engine. Self-contained: no strategy/DB/tokio imports (parity backbone — see
//! `hunter/docs/plans/strategy-redesign/fingerprint-metrics-engine-plan.md`).
//!
//! A **metric** is a named per-token quantity a rule can put `{operator, value}`
//! conditions on. Metrics live in **groups** (one file per group):
//! * `m_snapshot` (static) — `time`, `liquidity`
//! * `m_price_path` (static) — `stall`, `trail`
//! * `m_time_window` (dynamic, strict param `window_size_sec`) — `gross_flow`,
//!   `net_flow`, `buy`, `sell`
//!
//! The **registry** below is the single source of truth for group/metric names,
//! units, `=`-tolerances, static/dynamic kind, monotonicity, and required strict
//! params. Params validation, the evaluator, the engine, replay, and sweep axes
//! all read it — adding a metric here (plus its compute logic in the group file)
//! makes it immediately usable everywhere, with no schema change.

pub mod evaluator;

use std::fmt;

/// A metric group — one compute module, one JSON key under `entry`/`exit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MetricGroupId {
    /// `m_snapshot` — instantaneous token state.
    Snapshot,
    /// `m_price_path` — incremental price-path state.
    PricePath,
    /// `m_time_window` — trailing-window flow aggregates.
    TimeWindow,
}

impl MetricGroupId {
    pub fn name(self) -> &'static str {
        group_spec(self).name
    }
}

impl fmt::Display for MetricGroupId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// One metric within a group — a JSON key holding `{operator, value}` lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MetricId {
    /// Seconds since token creation (`m_snapshot`).
    Time,
    /// SOL reserves (`m_snapshot`).
    Liquidity,
    /// Seconds since the price last moved (`m_price_path`).
    Stall,
    /// Percent off the peak price (`m_price_path`).
    Trail,
    /// Buy + sell SOL over the trailing window (`m_time_window`).
    GrossFlow,
    /// Buy − sell SOL over the trailing window (`m_time_window`).
    NetFlow,
    /// Buy SOL over the trailing window (`m_time_window`).
    Buy,
    /// Sell SOL over the trailing window (`m_time_window`).
    Sell,
}

impl MetricId {
    pub fn name(self) -> &'static str {
        metric_spec(self).name
    }
}

impl fmt::Display for MetricId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Unit a metric's values (and its condition values) are expressed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Seconds,
    Sol,
    Percent,
}

/// Whether a group's metrics are rule-independent (one value per token) or need
/// per-rule strict params (deduped by those params across rules).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricKind {
    Static,
    Dynamic,
}

/// Registry entry for one metric: name, unit, `=`-tolerance, monotonicity.
///
/// `eq_tolerance` is the metric's own bucket-equality width for `=`/`!=`
/// (deliberately independent of any fingerprint's `bucket_size_amount`).
/// `monotonic` (non-decreasing over a token's life) powers derived
/// unsatisfiability disarm — an entry upper bound on a monotonic metric that is
/// permanently crossed can never re-satisfy.
#[derive(Debug, Clone, Copy)]
pub struct MetricSpec {
    pub id: MetricId,
    pub name: &'static str,
    pub unit: Unit,
    pub eq_tolerance: f64,
    pub monotonic: bool,
}

/// A strict (non-condition) group parameter, e.g. `m_time_window`'s
/// `window_size_sec`. Values must be finite and `> 0`.
#[derive(Debug, Clone, Copy)]
pub struct StrictParamSpec {
    pub name: &'static str,
    pub required: bool,
}

/// Registry entry for one metric group.
#[derive(Debug, Clone, Copy)]
pub struct GroupSpec {
    pub id: MetricGroupId,
    pub name: &'static str,
    pub kind: MetricKind,
    pub strict_params: &'static [StrictParamSpec],
    pub metrics: &'static [MetricSpec],
}

impl GroupSpec {
    /// Resolve a metric of this group by its JSON name.
    pub fn metric_by_name(&self, name: &str) -> Option<&'static MetricSpec> {
        self.metrics.iter().find(|m| m.name == name)
    }

    /// Resolve a strict param of this group by its JSON name.
    pub fn strict_param_by_name(&self, name: &str) -> Option<&'static StrictParamSpec> {
        self.strict_params.iter().find(|p| p.name == name)
    }
}

/// **The metric registry** — every group and metric the engine knows.
/// Compile-time data; every other layer derives its vocabulary from here.
pub const REGISTRY: &[GroupSpec] = &[
    GroupSpec {
        id: MetricGroupId::Snapshot,
        name: "m_snapshot",
        kind: MetricKind::Static,
        strict_params: &[],
        metrics: &[
            MetricSpec {
                id: MetricId::Time,
                name: "time",
                unit: Unit::Seconds,
                eq_tolerance: 0.5,
                monotonic: true,
            },
            MetricSpec {
                id: MetricId::Liquidity,
                name: "liquidity",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: false,
            },
        ],
    },
    GroupSpec {
        id: MetricGroupId::PricePath,
        name: "m_price_path",
        kind: MetricKind::Static,
        strict_params: &[],
        metrics: &[
            MetricSpec {
                id: MetricId::Stall,
                name: "stall",
                unit: Unit::Seconds,
                eq_tolerance: 0.5,
                monotonic: false,
            },
            MetricSpec {
                id: MetricId::Trail,
                name: "trail",
                unit: Unit::Percent,
                eq_tolerance: 1.0,
                monotonic: false,
            },
        ],
    },
    GroupSpec {
        id: MetricGroupId::TimeWindow,
        name: "m_time_window",
        kind: MetricKind::Dynamic,
        strict_params: &[StrictParamSpec { name: "window_size_sec", required: true }],
        metrics: &[
            MetricSpec {
                id: MetricId::GrossFlow,
                name: "gross_flow",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: false,
            },
            MetricSpec {
                id: MetricId::NetFlow,
                name: "net_flow",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: false,
            },
            MetricSpec {
                id: MetricId::Buy,
                name: "buy",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: false,
            },
            MetricSpec {
                id: MetricId::Sell,
                name: "sell",
                unit: Unit::Sol,
                eq_tolerance: 0.1,
                monotonic: false,
            },
        ],
    },
];

/// The registry entry for a group id (total — every id has an entry).
pub fn group_spec(id: MetricGroupId) -> &'static GroupSpec {
    REGISTRY
        .iter()
        .find(|g| g.id == id)
        .expect("every MetricGroupId has a REGISTRY entry")
}

/// Resolve a group by its JSON name (`m_snapshot`, …). `None` = unknown group.
pub fn group_by_name(name: &str) -> Option<&'static GroupSpec> {
    REGISTRY.iter().find(|g| g.name == name)
}

/// The registry entry for a metric id (total — every id has an entry).
pub fn metric_spec(id: MetricId) -> &'static MetricSpec {
    REGISTRY
        .iter()
        .flat_map(|g| g.metrics.iter())
        .find(|m| m.id == id)
        .expect("every MetricId has a REGISTRY entry")
}

/// The group a metric belongs to.
pub fn group_of(id: MetricId) -> &'static GroupSpec {
    REGISTRY
        .iter()
        .find(|g| g.metrics.iter().any(|m| m.id == id))
        .expect("every MetricId belongs to a REGISTRY group")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_names_resolve_both_ways() {
        for g in REGISTRY {
            assert_eq!(group_by_name(g.name).unwrap().id, g.id);
            assert_eq!(g.id.name(), g.name);
            for m in g.metrics {
                assert_eq!(g.metric_by_name(m.name).unwrap().id, m.id);
                assert_eq!(m.id.name(), m.name);
                assert_eq!(group_of(m.id).id, g.id);
            }
        }
    }

    #[test]
    fn registry_names_are_unique() {
        let mut group_names: Vec<_> = REGISTRY.iter().map(|g| g.name).collect();
        group_names.sort_unstable();
        group_names.dedup();
        assert_eq!(group_names.len(), REGISTRY.len());

        let mut metric_names: Vec<_> =
            REGISTRY.iter().flat_map(|g| g.metrics.iter().map(|m| m.name)).collect();
        let total = metric_names.len();
        metric_names.sort_unstable();
        metric_names.dedup();
        // Metric names are globally unique today (keeps error messages and sweep
        // axis labels unambiguous); relax to per-group uniqueness if ever needed.
        assert_eq!(metric_names.len(), total);
    }

    #[test]
    fn tolerances_are_positive_and_finite() {
        for g in REGISTRY {
            for m in g.metrics {
                assert!(m.eq_tolerance.is_finite() && m.eq_tolerance > 0.0, "{}", m.name);
            }
        }
    }

    #[test]
    fn only_time_is_monotonic() {
        for g in REGISTRY {
            for m in g.metrics {
                assert_eq!(m.monotonic, m.id == MetricId::Time, "{}", m.name);
            }
        }
    }
}
