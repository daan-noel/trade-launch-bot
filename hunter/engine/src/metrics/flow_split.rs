//! Volume/organic flow split — SSOT hashes, classifier, and per-fingerprint state
//! for `m_flow_split` (lifetime) + `m_flow_split_window` (trailing window).
//!
//! See `hunter/docs/plans/strategies/metrics-reference.md`.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use chrono::Duration;
use serde_json::Value;

use super::flow_window::window_key;
use super::{MetricId, Side, TradeLite, Ts};

/// FNV-1a offset basis (64-bit).
const FNV_OFFSET: u64 = 0xcbf29ce484222325;
/// FNV-1a prime (64-bit).
const FNV_PRIME: u64 = 0x100000001b3;

#[inline]
fn fnv1a_byte(mut h: u64, b: u8) -> u64 {
    h ^= u64::from(b);
    h.wrapping_mul(FNV_PRIME)
}

#[inline]
fn fnv1a_bytes(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h = fnv1a_byte(h, b);
    }
    h
}

/// Stable hash of an ordered instruction-label sequence (exact-order match
/// semantics, same as the fingerprint matcher's `ix_labels`). Labels are
/// separated with a single `0x1f` unit-separator byte so `["ab","c"]` ≠ `["a","bc"]`.
///
/// Empty input still returns a defined hash; callers that mean "missing labels"
/// should set `TradeLite::ix_hash = None` instead of hashing an empty slice.
pub fn ix_hash(labels: &[impl AsRef<str>]) -> u64 {
    let mut h = FNV_OFFSET;
    let mut first = true;
    for lab in labels {
        if !first {
            h = fnv1a_byte(h, 0x1f);
        }
        first = false;
        h = fnv1a_bytes(h, lab.as_ref().as_bytes());
    }
    h
}

/// `Some(ix_hash(labels))` when `labels` is non-empty; `None` when missing/empty
/// (pre-0002 history, absent lake columns) ⇒ organic unless wallet-tagged/creator.
pub fn ix_hash_opt(labels: &[impl AsRef<str>]) -> Option<u64> {
    if labels.is_empty() {
        None
    } else {
        Some(ix_hash(labels))
    }
}

/// Stable hash of a wallet address string (base58 or the lake's `unknown:{id}`
/// fallback). Contagion and creator checks compare these hashes only.
pub fn wallet_hash(addr: &str) -> u64 {
    fnv1a_bytes(FNV_OFFSET, addr.as_bytes())
}

// ── Patterns (compiled at RulesReloaded) ─────────────────────────────────────

/// Compiled volume-ix pattern set for one fingerprint (`m_flow_split.volume_ix_patterns`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FlowPatterns {
    /// FNV-1a of each configured label sequence.
    hashes: BTreeSet<u64>,
}

impl FlowPatterns {
    pub fn new(hashes: BTreeSet<u64>) -> Self {
        Self { hashes }
    }

    pub fn contains(&self, h: u64) -> bool {
        self.hashes.contains(&h)
    }

    pub fn is_empty(&self) -> bool {
        self.hashes.is_empty()
    }

    /// Compile an ordered list of label sequences (the sweep run's
    /// `volume_ix_patterns` / fingerprint config array).
    pub fn from_label_sequences(patterns: &[Vec<String>]) -> Self {
        let mut hashes = BTreeSet::new();
        for p in patterns {
            if !p.is_empty() {
                hashes.insert(ix_hash(p));
            }
        }
        Self { hashes }
    }

    /// Parse `metric_config["m_flow_split"]`. `None` = key absent (unconfigured ⇒
    /// flow metrics stay `NaN`). `Some` = configured (patterns may be empty —
    /// only contagion + creator classify as volume).
    pub fn from_metric_config(cfg: &Value) -> Option<Self> {
        let obj = cfg.get("m_flow_split")?;
        if !obj.is_object() {
            return None;
        }
        let Some(arr) = obj.get("volume_ix_patterns") else {
            // Key present but no patterns field — treat as configured empty.
            return Some(Self::default());
        };
        let Value::Array(patterns) = arr else {
            return None;
        };
        let mut hashes = BTreeSet::new();
        for p in patterns {
            let Value::Array(labels) = p else {
                return None;
            };
            let mut strs: Vec<&str> = Vec::with_capacity(labels.len());
            for lab in labels {
                let s = lab.as_str()?;
                strs.push(s);
            }
            if !strs.is_empty() {
                hashes.insert(ix_hash(&strs));
            }
        }
        Some(Self { hashes })
    }

    /// Validate fingerprint `metric_config` against the flow-split contract.
    /// Unknown top-level keys are ignored here (registry-level unknown-group
    /// rejection can land with fingerprint CRUD later). Empty/`{}` is fine.
    pub fn validate_metric_config(cfg: &Value) -> Result<(), String> {
        if cfg.is_null() {
            return Ok(());
        }
        let Some(obj) = cfg.as_object() else {
            return Err("metric_config must be a JSON object".into());
        };
        if let Some(flow) = obj.get("m_flow_split") {
            let Some(flow_obj) = flow.as_object() else {
                return Err("m_flow_split must be an object".into());
            };
            if let Some(patterns) = flow_obj.get("volume_ix_patterns") {
                let Some(arr) = patterns.as_array() else {
                    return Err("m_flow_split.volume_ix_patterns must be an array".into());
                };
                for (i, p) in arr.iter().enumerate() {
                    let Some(labels) = p.as_array() else {
                        return Err(format!(
                            "m_flow_split.volume_ix_patterns[{i}] must be an array of strings"
                        ));
                    };
                    for (j, lab) in labels.iter().enumerate() {
                        if !lab.is_string() {
                            return Err(format!(
                                "m_flow_split.volume_ix_patterns[{i}][{j}] must be a string"
                            ));
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

/// True when `params` (rule entry/exit JSON) references a flow metric group.
pub fn params_reference_flow(params: &Value) -> bool {
    for side in ["entry", "exit"] {
        if let Some(obj) = params.get(side).and_then(|v| v.as_object()) {
            if obj.contains_key("m_flow_split") || obj.contains_key("m_flow_split_window") {
                return true;
            }
        }
    }
    false
}

/// Warning text when a rule uses flow groups but the fingerprint has no
/// `m_flow_split` key (metrics will read `NaN`).
pub fn flow_unconfigured_warning(params: &Value, metric_config: &Value) -> Option<String> {
    if !params_reference_flow(params) {
        return None;
    }
    if FlowPatterns::from_metric_config(metric_config).is_some() {
        return None;
    }
    Some(
        "rule references m_flow_split/m_flow_split_window but the fingerprint has no \
         m_flow_split.volume_ix_patterns config — flow metrics will be NaN"
            .into(),
    )
}

// ── Totals ───────────────────────────────────────────────────────────────────

/// Running vol/organic SOL totals (buy/sell absolute; net/gross/share derived).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FlowTotals {
    pub vol_buy: f64,
    pub vol_sell: f64,
    pub nonvol_buy: f64,
    pub nonvol_sell: f64,
}

impl FlowTotals {
    fn add(&mut self, side: Side, sol: f64, is_vol: bool) {
        match (is_vol, side) {
            (true, Side::Buy) => self.vol_buy += sol,
            (true, Side::Sell) => self.vol_sell += sol,
            (false, Side::Buy) => self.nonvol_buy += sol,
            (false, Side::Sell) => self.nonvol_sell += sol,
        }
    }

    fn sub_signed(&mut self, signed: f64, is_vol: bool) {
        if signed >= 0.0 {
            if is_vol {
                self.vol_buy -= signed;
            } else {
                self.nonvol_buy -= signed;
            }
        } else if is_vol {
            self.vol_sell += signed; // signed < 0
        } else {
            self.nonvol_sell += signed;
        }
    }

    pub fn value(self, id: MetricId) -> f64 {
        use MetricId::*;
        match id {
            VolBuy | WinVolBuy => self.vol_buy,
            VolSell | WinVolSell => self.vol_sell,
            VolNet | WinVolNet => self.vol_buy - self.vol_sell,
            VolGross | WinVolGross => self.vol_buy + self.vol_sell,
            NonvolBuy | WinNonvolBuy => self.nonvol_buy,
            NonvolSell | WinNonvolSell => self.nonvol_sell,
            NonvolNet | WinNonvolNet => self.nonvol_buy - self.nonvol_sell,
            NonvolGross | WinNonvolGross => self.nonvol_buy + self.nonvol_sell,
            VolShare | WinVolShare => {
                let vg = self.vol_buy + self.vol_sell;
                let ng = self.nonvol_buy + self.nonvol_sell;
                let total = vg + ng;
                if total > 0.0 {
                    100.0 * vg / total
                } else {
                    f64::NAN
                }
            }
            _ => f64::NAN,
        }
    }
}

// ── Window ───────────────────────────────────────────────────────────────────

/// Trailing-window vol/organic aggregator for one `window_size_sec`.
#[derive(Debug, Clone, PartialEq)]
struct FlowSplitWindowState {
    window_secs: f64,
    /// `(timestamp, signed SOL, is_volume)` — buy positive, sell negative.
    buf: VecDeque<(Ts, f64, bool)>,
    totals: FlowTotals,
}

impl FlowSplitWindowState {
    fn new(window_secs: f64) -> Self {
        Self {
            window_secs,
            buf: VecDeque::new(),
            totals: FlowTotals::default(),
        }
    }

    fn on_trade(&mut self, side: Side, sol: f64, is_vol: bool, at: Ts) {
        let signed = match side {
            Side::Buy => sol,
            Side::Sell => -sol,
        };
        self.buf.push_back((at, signed, is_vol));
        self.totals.add(side, sol, is_vol);
        self.evict(at);
    }

    fn evict(&mut self, now: Ts) {
        let width = Duration::milliseconds(window_key(self.window_secs) as i64);
        let cutoff = now - width;
        while let Some(&(ts, signed, is_vol)) = self.buf.front() {
            if ts < cutoff {
                self.buf.pop_front();
                self.totals.sub_signed(signed, is_vol);
            } else {
                break;
            }
        }
    }
}

// ── FlowState ────────────────────────────────────────────────────────────────

/// Per-(token, fingerprint) classifier + accumulators.
#[derive(Debug, Clone, PartialEq)]
pub struct FlowState {
    patterns: FlowPatterns,
    tagged_wallets: BTreeSet<u64>,
    creator_wallet_hash: Option<u64>,
    lifetime: FlowTotals,
    windows: BTreeMap<u64, FlowSplitWindowState>,
}

impl FlowState {
    pub fn new(patterns: FlowPatterns) -> Self {
        Self {
            patterns,
            tagged_wallets: BTreeSet::new(),
            creator_wallet_hash: None,
            lifetime: FlowTotals::default(),
            windows: BTreeMap::new(),
        }
    }

    pub fn set_creator(&mut self, hash: u64) {
        self.creator_wallet_hash = Some(hash);
        self.tagged_wallets.insert(hash);
    }

    pub fn ensure_window(&mut self, window_secs: f64) {
        self.windows
            .entry(window_key(window_secs))
            .or_insert_with(|| FlowSplitWindowState::new(window_secs));
    }

    /// Classify + fold one trade into lifetime and every registered window.
    pub fn on_trade(&mut self, t: &TradeLite) {
        if !t.sol.is_finite() || t.sol < 0.0 {
            return;
        }
        let is_vol = self.classify(t);
        if is_vol {
            self.tagged_wallets.insert(t.wallet_hash);
        }
        self.lifetime.add(t.side, t.sol, is_vol);
        for w in self.windows.values_mut() {
            w.on_trade(t.side, t.sol, is_vol, t.at);
        }
    }

    pub fn on_tick(&mut self, now: Ts) {
        for w in self.windows.values_mut() {
            w.evict(now);
        }
    }

    /// Lifetime (`m_flow_split`) or windowed (`m_flow_split_window`) read.
    pub fn value(&self, id: MetricId, window_secs: Option<f64>) -> f64 {
        match window_secs {
            None => self.lifetime.value(id),
            Some(ws) => match self.windows.get(&window_key(ws)) {
                Some(w) => w.totals.value(id),
                None => f64::NAN,
            },
        }
    }

    /// Volume-side iff pattern match, wallet already tagged, or creator wallet.
    pub fn classify(&self, t: &TradeLite) -> bool {
        if self.creator_wallet_hash == Some(t.wallet_hash) {
            return true;
        }
        if self.tagged_wallets.contains(&t.wallet_hash) {
            return true;
        }
        if let Some(h) = t.ix_hash {
            if self.patterns.contains(h) {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use serde_json::json;

    fn ts(secs: f64) -> Ts {
        Utc.timestamp_opt(1_700_000_000, 0).unwrap()
            + Duration::milliseconds((secs * 1000.0) as i64)
    }

    fn trade(side: Side, sol: f64, ix: Option<u64>, wallet: u64, secs: f64) -> TradeLite {
        TradeLite {
            side,
            sol,
            price: 1.0,
            reserve_sol: 10.0,
            at: ts(secs),
            ix_hash: ix,
            wallet_hash: wallet,
        }
    }

    #[test]
    fn ix_hash_is_order_and_boundary_sensitive() {
        let a = ix_hash(&["Pump.Fun: Create", "Pump.Fun: Buy"]);
        let b = ix_hash(&["Pump.Fun: Buy", "Pump.Fun: Create"]);
        let c = ix_hash(&["Pump.Fun: CreatePump.Fun: Buy"]);
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_eq!(a, ix_hash(&["Pump.Fun: Create", "Pump.Fun: Buy"]));
    }

    #[test]
    fn ix_hash_opt_none_on_empty() {
        let empty: &[&str] = &[];
        assert_eq!(ix_hash_opt(empty), None);
        assert_eq!(
            ix_hash_opt(&["Pump.Fun: Buy"]),
            Some(ix_hash(&["Pump.Fun: Buy"]))
        );
    }

    #[test]
    fn wallet_hash_stable() {
        assert_eq!(wallet_hash("Abc123"), wallet_hash("Abc123"));
        assert_ne!(wallet_hash("Abc123"), wallet_hash("abc123"));
        assert_ne!(wallet_hash("unknown:7"), wallet_hash("7"));
    }

    #[test]
    fn patterns_from_metric_config() {
        assert!(FlowPatterns::from_metric_config(&json!({})).is_none());
        let p = FlowPatterns::from_metric_config(&json!({
            "m_flow_split": {
                "volume_ix_patterns": [
                    ["Pump.Fun: Create", "Pump.Fun: Buy"],
                    ["Pump.Fun: Buy"]
                ]
            }
        }))
        .unwrap();
        assert!(p.contains(ix_hash(&["Pump.Fun: Create", "Pump.Fun: Buy"])));
        assert!(p.contains(ix_hash(&["Pump.Fun: Buy"])));
        assert!(!p.contains(ix_hash(&["Pump.Fun: Sell"])));
    }

    #[test]
    fn classify_pattern_contagion_creator_and_missing_ix() {
        let patterns =
            FlowPatterns::new(BTreeSet::from([ix_hash(&["Pump.Fun: Create", "Pump.Fun: Buy"])]));
        let mut st = FlowState::new(patterns);
        st.set_creator(wallet_hash("creator"));

        // Creator → volume.
        assert!(st.classify(&trade(Side::Buy, 1.0, None, wallet_hash("creator"), 0.0)));

        // Pattern match → volume + tags wallet.
        let w = wallet_hash("bot1");
        let t = trade(
            Side::Buy,
            2.0,
            Some(ix_hash(&["Pump.Fun: Create", "Pump.Fun: Buy"])),
            w,
            1.0,
        );
        assert!(st.classify(&t));
        st.on_trade(&t);
        assert!(st.tagged_wallets.contains(&w));

        // Same wallet, no ix → contagion volume.
        assert!(st.classify(&trade(Side::Sell, 1.0, None, w, 2.0)));

        // Unknown wallet, missing ix → organic.
        assert!(!st.classify(&trade(Side::Buy, 1.0, None, wallet_hash("normie"), 3.0)));

        // Unknown wallet, non-matching ix → organic.
        assert!(!st.classify(&trade(
            Side::Buy,
            1.0,
            Some(ix_hash(&["Pump.Fun: Buy"])),
            wallet_hash("normie2"),
            4.0
        )));
    }

    #[test]
    fn lifetime_totals_and_vol_share() {
        let patterns = FlowPatterns::new(BTreeSet::from([ix_hash(&["vol"])]));
        let mut st = FlowState::new(patterns);
        st.on_trade(&trade(Side::Buy, 4.0, Some(ix_hash(&["vol"])), 1, 0.0));
        st.on_trade(&trade(Side::Buy, 6.0, None, 2, 1.0));
        st.on_trade(&trade(Side::Sell, 1.0, Some(ix_hash(&["vol"])), 1, 2.0));

        assert_eq!(st.value(MetricId::VolBuy, None), 4.0);
        assert_eq!(st.value(MetricId::VolSell, None), 1.0);
        assert_eq!(st.value(MetricId::VolGross, None), 5.0);
        assert_eq!(st.value(MetricId::VolNet, None), 3.0);
        assert_eq!(st.value(MetricId::NonvolBuy, None), 6.0);
        assert_eq!(st.value(MetricId::NonvolGross, None), 6.0);
        // vol_share = 5 / 11 * 100
        let share = st.value(MetricId::VolShare, None);
        assert!((share - 500.0 / 11.0).abs() < 1e-9);
    }

    #[test]
    fn vol_share_nan_at_zero() {
        let st = FlowState::new(FlowPatterns::default());
        assert!(st.value(MetricId::VolShare, None).is_nan());
    }

    #[test]
    fn window_evicts_on_tick() {
        let patterns = FlowPatterns::new(BTreeSet::from([ix_hash(&["vol"])]));
        let mut st = FlowState::new(patterns);
        st.ensure_window(10.0);
        st.on_trade(&trade(Side::Buy, 4.0, Some(ix_hash(&["vol"])), 1, 0.0));
        st.on_trade(&trade(Side::Buy, 6.0, None, 2, 1.0));
        assert_eq!(st.value(MetricId::VolBuy, Some(10.0)), 4.0);
        assert_eq!(st.value(MetricId::NonvolBuy, Some(10.0)), 6.0);
        // Trailing window is (now−w, now] — at t=11 the t=1 trade is still in;
        // one ms past the edge drops everything.
        st.on_tick(ts(11.0));
        assert_eq!(st.value(MetricId::VolBuy, Some(10.0)), 0.0);
        assert_eq!(st.value(MetricId::NonvolBuy, Some(10.0)), 6.0);
        st.on_tick(ts(11.001));
        assert_eq!(st.value(MetricId::NonvolBuy, Some(10.0)), 0.0);
        // Lifetime unchanged.
        assert_eq!(st.value(MetricId::VolBuy, None), 4.0);
    }

    #[test]
    fn flow_unconfigured_warning_fires() {
        let params = json!({"entry": {"m_flow_split": {"vol_buy": [{"operator": ">", "value": 1}]}}});
        assert!(flow_unconfigured_warning(&params, &json!({})).is_some());
        let cfg = json!({"m_flow_split": {"volume_ix_patterns": [["Pump.Fun: Buy"]]}});
        assert!(flow_unconfigured_warning(&params, &cfg).is_none());
    }
}
