//! `m_bundle` — who is in a token's **launch bundle** (fingerprint-scoped statics).
//!
//! Every other group asks how much SOL moved or where the price went. This one asks
//! **whose** SOL it was, by matching launch-window buyers against a per-fingerprint set
//! of wallets that have bought this launcher's earlier tokens.
//!
//! * `veteran_share` — percent of launch-window **buy** SOL from veteran wallets.
//! * `veteran_wallets` — count of distinct veteran wallets that bought in the window.
//! * `fresh_wallets` — count of distinct non-veteran wallets that bought in the window.
//!
//! **The window closes and never reopens.** Buys are folded only while
//! `at <= created_at + LAUNCH_WINDOW_SECS`; afterwards the values are frozen. That is
//! deliberate — the metric describes the launch, so a rule reads the same value at
//! `time = 2` as at `time = 200` and an entry condition on it can never be re-triggered
//! by later trading. It also makes the group **monotonic-free**: the values simply stop
//! moving, so no unsatisfiability derivation applies.
//!
//! Read the share as "the fingerprint's REGULARS opened this token, rather than wallets
//! never seen on it before" - not as "the launcher's own bundlers funded it". Most
//! launch-window wallets are one-shot and rotate away; what the roster catches is the
//! small persistent crowd that does not. Same number, different reason to trust it - see
//! `hunter/docs/plans/strategies/veteran-wallets.md`.
//!
//! Sells are ignored. The question is whose money OPENED the token; a launch-window sell
//! is an exit from a position taken in the same window and would double-count the wallet.
//!
//! Undefined (`NaN`) until the first launch-window buy — with no bundle observed there is
//! no share to compare, and a `NaN` satisfies no condition (evaluator contract).
//!
//! Veteran membership is **injected**, not derived: the engine is a pure fold and cannot
//! query launch history. [`crate::metrics::track::TokenTrack::ensure_bundle`] seeds the
//! set, exactly as `m_flow_split` receives its pattern set. The seed must be built from
//! launches strictly **earlier** than this token or the metric is look-ahead — see
//! `hunter/docs/plans/strategies/veteran-wallets.md` for the causality contract.

use serde_json::Value;

use super::{secs_between, MetricId, Side, TradeLite, Ts};
use super::flow_split::wallet_hash;
use crate::hash::HashedSet;

/// Default veteran bar: a wallet counts once it has bought this many of the
/// fingerprint's earlier launches.
///
/// 25 on the `3ix:Buy · max=0.108` cohort. The gate is insensitive to it — 10, 25 and
/// 50 all score within 0.5pp — because the underlying distribution is bimodal, so this
/// is a reasonable default rather than a tuned constant.
pub const DEFAULT_VETERAN_MIN_LAUNCHES: u32 = 25;

/// Read the veteran roster out of a fingerprint's `metric_config`.
///
/// Shape (written by the roster refresher, never hand-authored):
/// ```json
/// { "m_bundle": { "veteran_min_launches": 25, "veteran_wallets": ["Addr1", "Addr2"] } }
/// ```
/// `None` ⇒ the fingerprint has no `m_bundle` config, so `m_bundle` metrics read `NaN`
/// and no rule on them can fire. An empty/absent `veteran_wallets` is a *configured
/// empty* roster (every wallet reads as fresh), matching `FlowPatterns`' contract —
/// that distinction is what lets [`bundle_unconfigured_warning`] tell "not set up" from
/// "set up, nobody qualifies yet".
pub fn veterans_from_metric_config(cfg: &Value) -> Option<HashedSet> {
    let obj = cfg.get("m_bundle")?;
    if !obj.is_object() {
        return None;
    }
    let mut set = HashedSet::default();
    let Some(arr) = obj.get("veteran_wallets") else {
        return Some(set);
    };
    let Value::Array(wallets) = arr else {
        return None;
    };
    for w in wallets {
        set.insert(wallet_hash(w.as_str()?));
    }
    Some(set)
}

/// A fingerprint's veteran roster **as a function of time**.
///
/// Live carries a single roster — "as of the last refresh" — and reads it for every
/// token, which is sound because the refresh always precedes the tokens that read it.
/// A backtest cannot borrow that property: the stored roster was built from launches
/// that lie in the *future* of most of the corpus, so scoring those tokens against it
/// is look-ahead. A timeline restores the ordering by carrying one snapshot per
/// re-anchor point — a token created at `t` reads the newest snapshot whose
/// `from <= t`, and therefore only ever sees launches older than itself.
///
/// Shape (written into an **in-memory** `metric_config` by the backtest driver, never
/// persisted — the stored row keeps the flat `veteran_wallets` that live reads):
/// ```json
/// { "m_bundle": { "veteran_timeline": [
///     { "from": "2026-08-01T00:00:00Z", "wallets": ["Addr1"] },
///     { "from": "2026-08-02T00:00:00Z", "wallets": ["Addr1", "Addr2"] }
/// ] } }
/// ```
/// A flat `veteran_wallets` parses as one snapshot open from the beginning of time, so
/// live and backtest read the roster through the same path.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RosterTimeline {
    /// Snapshots by `from` ascending. Empty ⇒ a configured-empty roster.
    snaps: Vec<(Ts, HashedSet)>,
}

impl RosterTimeline {
    /// Parse a fingerprint's `metric_config`. `None` ⇒ no `m_bundle` config at all, so
    /// `m_bundle` metrics read `NaN` and no rule on them can fire — the same
    /// "not set up" contract [`veterans_from_metric_config`] carries.
    pub fn from_metric_config(cfg: &Value) -> Option<Self> {
        let obj = cfg.get("m_bundle")?;
        if !obj.is_object() {
            return None;
        }
        let Some(Value::Array(entries)) = obj.get("veteran_timeline") else {
            // No timeline — the flat roster, in force from the beginning of time.
            return veterans_from_metric_config(cfg)
                .map(|set| Self { snaps: vec![(Ts::MIN_UTC, set)] });
        };
        let mut snaps = Vec::with_capacity(entries.len());
        for e in entries {
            let from = e.get("from")?.as_str()?.parse::<Ts>().ok()?;
            let mut set = HashedSet::default();
            if let Some(Value::Array(wallets)) = e.get("wallets") {
                for w in wallets {
                    set.insert(wallet_hash(w.as_str()?));
                }
            }
            snaps.push((from, set));
        }
        snaps.sort_by_key(|(from, _)| *from);
        Some(Self { snaps })
    }

    /// The roster in force for a token created at `at` — the newest snapshot at or
    /// before it.
    ///
    /// `None` when every snapshot starts after `at`. That is the honest answer for a
    /// token older than any history the driver built: no roster is known for it, so
    /// the metric stays `NaN` and the rule stands down, rather than borrowing a
    /// roster assembled from its own future.
    pub fn at(&self, at: Ts) -> Option<&HashedSet> {
        let idx = self.snaps.partition_point(|(from, _)| *from <= at);
        (idx > 0).then(|| &self.snaps[idx - 1].1)
    }

    /// Number of snapshots — `1` for a flat roster. Diagnostics only.
    pub fn len(&self) -> usize {
        self.snaps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.snaps.is_empty()
    }
}

/// True when `params` (rule entry/exit JSON) references the `m_bundle` group.
pub fn params_reference_bundle(params: &Value) -> bool {
    ["entry", "exit"].iter().any(|side| {
        params
            .get(side)
            .and_then(|v| v.as_object())
            .is_some_and(|o| o.contains_key("m_bundle"))
    })
}

/// Warning for a rule that reads `m_bundle` on a fingerprint with no roster config —
/// its conditions would sit on `NaN` and never fire. `None` when configured.
pub fn bundle_unconfigured_warning(params: &Value, metric_config: &Value) -> Option<String> {
    if !params_reference_bundle(params) {
        return None;
    }
    if veterans_from_metric_config(metric_config).is_some() {
        return None;
    }
    Some(
        "rule references m_bundle but the fingerprint has no m_bundle.veteran_wallets \
         config — bundle metrics will be NaN"
            .into(),
    )
}

/// Length of the launch window, in seconds since token creation.
///
/// One second ≈ the creation slot plus its immediate successors. Measured against an
/// exact creation-slot ground truth on the `3ix:Buy · max=0.108` cohort this window
/// reproduces the per-token veteran share at r = 0.999 and agrees on a `>= 90%` gate
/// for 97.8% of tokens, so the engine's timestamp-only view loses nothing real. It is
/// deliberately NOT a `window_size_sec` strict param: the window is anchored on birth,
/// not trailing, and a rule that could slide it would be measuring a different thing.
pub const LAUNCH_WINDOW_SECS: f64 = 1.0;

/// Incremental `m_bundle` state for one (token, fingerprint) pair.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BundleState {
    /// Wallets that bought this fingerprint's earlier launches (FNV-1a hashes).
    veterans: HashedSet,
    /// Launch-window buy SOL from veteran wallets.
    vet_sol: f64,
    /// Launch-window buy SOL, all wallets.
    total_sol: f64,
    /// Distinct launch-window buyer hashes, split by veteran membership.
    vet_seen: HashedSet,
    fresh_seen: HashedSet,
}

impl BundleState {
    /// New state for a fingerprint whose veteran wallets are `veterans`.
    pub fn new(veterans: HashedSet) -> Self {
        Self {
            veterans,
            ..Self::default()
        }
    }

    /// Adopt a refreshed veteran set.
    ///
    /// Only affects trades folded **after** the swap — the totals are running sums and
    /// nothing retains the trades to reclassify. Same contract as
    /// `FlowState::set_patterns`: an operator edit moves a live token's future, never
    /// its past. In practice the seed is refreshed between launches, so a token that is
    /// already inside its one-second window is the rare case.
    pub fn set_veterans(&mut self, veterans: &HashedSet) {
        if &self.veterans != veterans {
            self.veterans = veterans.clone();
        }
    }

    /// Fold one trade. Ignores sells, non-positive SOL, and anything past the window.
    pub fn on_trade(&mut self, t: &TradeLite, created_at: Ts) {
        // `is_finite` first so a NaN notional is dropped rather than poisoning the
        // running totals — once `total_sol` is NaN the share is NaN forever.
        if t.side != Side::Buy || !t.sol.is_finite() || t.sol <= 0.0 {
            return;
        }
        if secs_between(created_at, t.at) > LAUNCH_WINDOW_SECS {
            return;
        }
        self.total_sol += t.sol;
        if self.veterans.contains(&t.wallet_hash) {
            self.vet_sol += t.sol;
            self.vet_seen.insert(t.wallet_hash);
        } else {
            self.fresh_seen.insert(t.wallet_hash);
        }
    }

    /// `veteran_share` — percent of launch-window buy SOL from veterans; `NaN` before
    /// the first launch-window buy.
    pub fn veteran_share(&self) -> f64 {
        if self.total_sol > 0.0 {
            100.0 * self.vet_sol / self.total_sol
        } else {
            f64::NAN
        }
    }

    /// Value of one `m_bundle` metric. Non-bundle ids yield `NaN` (unreachable —
    /// `TokenTrack` routes by group).
    pub fn value(&self, id: MetricId) -> f64 {
        match id {
            MetricId::VeteranShare => self.veteran_share(),
            MetricId::VeteranWallets => self.vet_seen.len() as f64,
            MetricId::FreshWallets => self.fresh_seen.len() as f64,
            _ => f64::NAN,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone, Utc};

    fn t0() -> Ts {
        Utc.timestamp_opt(1_700_000_000, 0).unwrap()
    }

    fn ts(ms: i64) -> Ts {
        t0() + Duration::milliseconds(ms)
    }

    fn buy(sol: f64, wallet: u64, at: Ts) -> TradeLite {
        TradeLite {
            side: Side::Buy,
            sol,
            wallet_hash: wallet,
            at,
            ..TradeLite::default()
        }
    }

    fn vets(hashes: &[u64]) -> HashedSet {
        let mut s = HashedSet::default();
        for &h in hashes {
            s.insert(h);
        }
        s
    }

    #[test]
    fn nan_until_the_first_launch_window_buy() {
        let s = BundleState::new(vets(&[1]));
        assert!(s.veteran_share().is_nan());
        assert_eq!(s.value(MetricId::VeteranWallets), 0.0);
    }

    #[test]
    fn share_is_sol_weighted_not_wallet_weighted() {
        let mut s = BundleState::new(vets(&[1]));
        s.on_trade(&buy(9.0, 1, ts(0)), t0()); // one veteran, most of the SOL
        s.on_trade(&buy(0.5, 2, ts(10)), t0());
        s.on_trade(&buy(0.5, 3, ts(20)), t0());
        assert_eq!(s.veteran_share(), 90.0);
        assert_eq!(s.value(MetricId::VeteranWallets), 1.0);
        assert_eq!(s.value(MetricId::FreshWallets), 2.0);
    }

    #[test]
    fn a_wallet_buying_twice_counts_once() {
        let mut s = BundleState::new(vets(&[1]));
        s.on_trade(&buy(1.0, 1, ts(0)), t0());
        s.on_trade(&buy(1.0, 1, ts(10)), t0());
        assert_eq!(s.value(MetricId::VeteranWallets), 1.0);
        assert_eq!(s.veteran_share(), 100.0);
    }

    #[test]
    fn window_closes_and_later_trading_cannot_move_it() {
        let mut s = BundleState::new(vets(&[1]));
        s.on_trade(&buy(1.0, 1, ts(0)), t0());
        let frozen = s.veteran_share();
        // A whale arriving after the window must not dilute the launch reading.
        s.on_trade(&buy(500.0, 9, ts(1_001)), t0());
        assert_eq!(s.veteran_share(), frozen);
        assert_eq!(s.value(MetricId::FreshWallets), 0.0);
        // The boundary itself is inclusive.
        let mut s2 = BundleState::new(vets(&[1]));
        s2.on_trade(&buy(1.0, 9, ts(1_000)), t0());
        assert_eq!(s2.veteran_share(), 0.0);
    }

    #[test]
    fn sells_are_ignored() {
        let mut s = BundleState::new(vets(&[1]));
        s.on_trade(&buy(1.0, 1, ts(0)), t0());
        let mut sell = buy(50.0, 2, ts(10));
        sell.side = Side::Sell;
        s.on_trade(&sell, t0());
        assert_eq!(s.veteran_share(), 100.0);
        assert_eq!(s.value(MetricId::FreshWallets), 0.0);
    }

    #[test]
    fn reseeding_only_moves_the_future() {
        let mut s = BundleState::new(HashedSet::default());
        s.on_trade(&buy(1.0, 1, ts(0)), t0());
        assert_eq!(s.veteran_share(), 0.0);
        s.set_veterans(&vets(&[1, 2]));
        s.on_trade(&buy(1.0, 2, ts(10)), t0());
        // The first trade keeps the classification it was folded under.
        assert_eq!(s.veteran_share(), 50.0);
    }

    #[test]
    fn a_flat_roster_parses_as_one_snapshot_open_from_the_beginning_of_time() {
        let cfg = serde_json::json!({ "m_bundle": { "veteran_wallets": ["A", "B"] } });
        let tl = RosterTimeline::from_metric_config(&cfg).expect("configured");
        assert_eq!(tl.len(), 1);
        assert_eq!(tl.at(t0()).map(HashedSet::len), Some(2));
    }

    #[test]
    fn no_m_bundle_config_is_unconfigured_not_empty() {
        assert!(RosterTimeline::from_metric_config(&serde_json::json!({})).is_none());
        // Present but with no wallet list is a *configured empty* roster: every wallet
        // reads fresh, which is a real answer, not a missing one.
        let tl = RosterTimeline::from_metric_config(&serde_json::json!({ "m_bundle": {} }))
            .expect("configured");
        assert_eq!(tl.at(t0()).map(HashedSet::len), Some(0));
    }

    #[test]
    fn a_timeline_reads_the_newest_snapshot_at_or_before_the_token() {
        let cfg = serde_json::json!({ "m_bundle": { "veteran_timeline": [
            { "from": "2026-08-02T00:00:00Z", "wallets": ["A", "B"] },
            { "from": "2026-08-01T00:00:00Z", "wallets": ["A"] }
        ] } });
        let tl = RosterTimeline::from_metric_config(&cfg).expect("configured");
        assert_eq!(tl.len(), 2);
        let at = |s: &str| tl.at(s.parse::<Ts>().unwrap()).map(HashedSet::len);
        // Sorted on parse, so input order does not matter.
        assert_eq!(at("2026-08-01T12:00:00Z"), Some(1));
        assert_eq!(at("2026-08-02T00:00:00Z"), Some(2)); // the anchor itself is inclusive
        assert_eq!(at("2026-08-09T00:00:00Z"), Some(2));
        // Older than every snapshot: no roster is known for this token, so the metric
        // stays NaN rather than borrowing one built from its own future.
        assert_eq!(at("2026-07-31T23:59:59Z"), None);
    }

    #[test]
    fn a_timeline_wins_over_a_flat_roster_on_the_same_config() {
        // The backtest driver replaces the flat key, but a config carrying both must
        // never read the (look-ahead) flat one.
        let cfg = serde_json::json!({ "m_bundle": {
            "veteran_wallets": ["A", "B", "C"],
            "veteran_timeline": [{ "from": "2026-08-01T00:00:00Z", "wallets": ["A"] }]
        } });
        let tl = RosterTimeline::from_metric_config(&cfg).expect("configured");
        assert_eq!(tl.at("2026-08-05T00:00:00Z".parse::<Ts>().unwrap()).map(HashedSet::len), Some(1));
    }
}
