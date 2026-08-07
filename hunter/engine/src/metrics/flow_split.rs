//! Volume/organic flow split — SSOT hashes, classifier, and per-fingerprint state
//! for `m_flow_split` (lifetime) + `m_flow_split_window` (trailing window).
//!
//! See `hunter/docs/plans/strategies/metrics-reference.md`.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use chrono::Duration;
use serde_json::Value;

use super::flow_window::{push_sorted, window_key, window_width};
use super::{MetricId, Side, TradeLite, Ts};

use crate::hash::{fnv1a_byte, fnv1a_bytes, HashedSet, FNV_OFFSET};

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

/// [`ix_hash_opt`] over labels still in their stored **JSON array** form, without
/// allocating.
///
/// Both the lake (`normalize_labels`) and Postgres (`trades.ix_labels`) hold the
/// label sequence as a JSON array of strings, and the offline paths used to turn
/// every row back into a `Vec<String>` — a `serde_json` parse plus one heap
/// allocation per label, **per trade**, on corpora of millions of rows — purely to
/// feed [`ix_hash`]. This walks the array in place instead.
///
/// Exactness is not traded away: the scanner handles only the shape the writers
/// actually emit (a flat array of unescaped strings) and **falls back to
/// `serde_json`** the moment it meets an escape or anything unexpected, so the
/// result is by construction whatever `ix_hash_opt(&parsed)` would have returned —
/// including the "unparseable ⇒ `None` ⇒ organic" behaviour. Locked by
/// `json_scanner_matches_the_parsed_hash`.
pub fn ix_hash_from_labels_json(json: &str) -> Option<u64> {
    match scan_labels_json(json.as_bytes()) {
        Some(h) => h,
        None => {
            let labels: Vec<String> = serde_json::from_str(json).ok()?;
            ix_hash_opt(&labels)
        }
    }
}

/// In-place hash of a JSON array of unescaped strings.
///
/// `Some(result)` = decided (`result` is [`ix_hash_opt`]'s answer); `None` = this
/// scanner cannot answer exactly (escape, nested value, malformed input) and the
/// caller must fall back to a real parse.
fn scan_labels_json(b: &[u8]) -> Option<Option<u64>> {
    let mut i = 0usize;
    let skip_ws = |i: &mut usize| {
        while matches!(b.get(*i), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            *i += 1;
        }
    };
    skip_ws(&mut i);
    if b.get(i) != Some(&b'[') {
        return None;
    }
    i += 1;
    let mut h = FNV_OFFSET;
    let mut n = 0usize;
    loop {
        skip_ws(&mut i);
        match b.get(i) {
            Some(b']') => {
                i += 1;
                skip_ws(&mut i);
                // Trailing garbage after the array ⇒ not a shape we own.
                return (i == b.len()).then_some((n > 0).then_some(h));
            }
            Some(b'"') => {}
            _ => return None,
        }
        i += 1; // past the opening quote
        if n > 0 {
            h = fnv1a_byte(h, 0x1f);
        }
        let start = i;
        loop {
            match b.get(i) {
                Some(b'"') => break,
                // An escape changes the decoded bytes — only a real parse is exact.
                Some(b'\\') | None => return None,
                Some(_) => i += 1,
            }
        }
        h = fnv1a_bytes(h, &b[start..i]);
        i += 1; // past the closing quote
        n += 1;
        skip_ws(&mut i);
        match b.get(i) {
            Some(b',') => i += 1,
            Some(b']') => {}
            _ => return None,
        }
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
///
/// Same O(1)-read shape as [`WindowState`](super::flow_window::WindowState): a
/// **time-sorted** deque plus running [`FlowTotals`] over all of it, so
/// [`totals_at`](Self::totals_at) corrects the two out-of-window ends instead of
/// rebuilding the totals from a full scan. This one mattered most — `value` is
/// called once **per flow metric per rule per event**, and each call used to walk
/// the whole window; a rule with three `m_flow_split_window` conditions paid three
/// full scans on every 200 ms tick of every tracked token.
#[derive(Debug, Clone, PartialEq)]
struct FlowSplitWindowState {
    window_secs: f64,
    /// Precomputed window width — see [`window_width`].
    width: Duration,
    /// `(timestamp, signed SOL, is_volume)` — buy positive, sell negative; oldest
    /// at front, kept time-sorted.
    buf: VecDeque<(Ts, (f64, bool))>,
    /// Running totals over **all** of `buf`.
    totals: FlowTotals,
}

impl FlowSplitWindowState {
    fn new(window_secs: f64) -> Self {
        Self {
            window_secs,
            width: window_width(window_secs),
            buf: VecDeque::new(),
            totals: FlowTotals::default(),
        }
    }

    fn on_trade(&mut self, side: Side, sol: f64, is_vol: bool, at: Ts) {
        let signed = match side {
            Side::Buy => sol,
            Side::Sell => -sol,
        };
        push_sorted(&mut self.buf, at, (signed, is_vol));
        self.totals.add(side, sol, is_vol);
        self.evict(at);
    }

    fn evict(&mut self, now: Ts) {
        let cutoff = now - self.width;
        while let Some(&(ts, (signed, is_vol))) = self.buf.front() {
            if ts < cutoff {
                self.buf.pop_front();
                self.totals.sub_signed(signed, is_vol);
            } else {
                break;
            }
        }
    }

    /// Totals over `(now − w, now]`, from the running totals minus the ends that
    /// fall outside: not-yet-evicted entries at the front, future-dated entries
    /// (regressed `block_time`) at the back. Sortedness makes the first in-window
    /// entry a valid stop for both loops.
    fn totals_at(&self, now: Ts) -> FlowTotals {
        let mut out = self.totals;
        let cutoff = now - self.width;
        for &(ts, (signed, is_vol)) in self.buf.iter() {
            if ts >= cutoff {
                break;
            }
            out.sub_signed(signed, is_vol);
        }
        for &(ts, (signed, is_vol)) in self.buf.iter().rev() {
            if ts <= now {
                break;
            }
            out.sub_signed(signed, is_vol);
        }
        out
    }

    fn value(&self, id: MetricId, now: Ts) -> f64 {
        self.totals_at(now).value(id)
    }
}

// ── FlowState ────────────────────────────────────────────────────────────────

/// Per-(token, fingerprint) classifier + accumulators.
#[derive(Debug, Clone, PartialEq)]
pub struct FlowState {
    patterns: FlowPatterns,
    /// Wallets contagion has tagged volume-side. Membership-only (never iterated),
    /// so a flat [`HashedSet`] over the already-FNV-hashed addresses replaces the
    /// old `BTreeSet<u64>` — this is one lookup per trade per fingerprint.
    tagged_wallets: HashedSet,
    creator_wallet_hash: Option<u64>,
    lifetime: FlowTotals,
    windows: BTreeMap<u64, FlowSplitWindowState>,
}

impl FlowState {
    pub fn new(patterns: FlowPatterns) -> Self {
        Self {
            patterns,
            tagged_wallets: HashedSet::default(),
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

    /// Lifetime (`m_flow_split`) or windowed (`m_flow_split_window`) read at `now`.
    pub fn value(&self, id: MetricId, window_secs: Option<f64>, now: Ts) -> f64 {
        match window_secs {
            None => self.lifetime.value(id),
            Some(ws) => match self.windows.get(&window_key(ws)) {
                Some(w) => w.value(id, now),
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

    /// The in-place JSON scanner must agree with "parse, then hash" on every input
    /// — the happy shapes the writers emit, the degenerate ones, and the escaped /
    /// malformed ones where it is required to fall back rather than guess. This is
    /// the whole safety argument for skipping the per-trade `serde_json` parse.
    #[test]
    fn json_scanner_matches_the_parsed_hash() {
        let cases = [
            r#"["Pump.Fun: Create","Pump.Fun: Buy"]"#,
            r#"["Pump.Fun: Buy"]"#,
            r#"[ "a" , "b" , "c" ]"#,
            r#"["a","b"]"#,
            r#"["ab","c"]"#,   // separator sensitivity
            r#"["a","bc"]"#,
            r#"[""]"#,         // one empty label != empty array
            r#"["",""]"#,
            "[]",              // empty array ⇒ None (missing labels)
            "[ ]",
            r#"["with \"quote\""]"#,   // escape ⇒ fallback path
            r#"["tab\there"]"#,
            r#"["unicode é"]"#,
            r#"["émoji ✨ raw"]"#,     // multi-byte but unescaped ⇒ fast path
            "null",            // unparseable ⇒ None
            "not json",
            "[1,2]",           // wrong element type ⇒ None
            r#"["unterminated"#,
            r#"["a"] trailing"#,
            "",
        ];
        for case in cases {
            let parsed: Vec<String> = serde_json::from_str(case).unwrap_or_default();
            assert_eq!(
                ix_hash_from_labels_json(case),
                ix_hash_opt(&parsed),
                "scanner disagreed on {case:?}"
            );
        }
        // And it really does produce the SSOT hash on the shape that matters.
        assert_eq!(
            ix_hash_from_labels_json(r#"["Pump.Fun: Create","Pump.Fun: Buy"]"#),
            Some(ix_hash(&["Pump.Fun: Create", "Pump.Fun: Buy"])),
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

        assert_eq!(st.value(MetricId::VolBuy, None, ts(2.0)), 4.0);
        assert_eq!(st.value(MetricId::VolSell, None, ts(2.0)), 1.0);
        assert_eq!(st.value(MetricId::VolGross, None, ts(2.0)), 5.0);
        assert_eq!(st.value(MetricId::VolNet, None, ts(2.0)), 3.0);
        assert_eq!(st.value(MetricId::NonvolBuy, None, ts(2.0)), 6.0);
        assert_eq!(st.value(MetricId::NonvolGross, None, ts(2.0)), 6.0);
        // vol_share = 5 / 11 * 100
        let share = st.value(MetricId::VolShare, None, ts(2.0));
        assert!((share - 500.0 / 11.0).abs() < 1e-9);
    }

    #[test]
    fn vol_share_nan_at_zero() {
        let st = FlowState::new(FlowPatterns::default());
        assert!(st.value(MetricId::VolShare, None, ts(0.0)).is_nan());
    }

    #[test]
    fn window_evicts_on_tick() {
        let patterns = FlowPatterns::new(BTreeSet::from([ix_hash(&["vol"])]));
        let mut st = FlowState::new(patterns);
        st.ensure_window(10.0);
        st.on_trade(&trade(Side::Buy, 4.0, Some(ix_hash(&["vol"])), 1, 0.0));
        st.on_trade(&trade(Side::Buy, 6.0, None, 2, 1.0));
        assert_eq!(st.value(MetricId::VolBuy, Some(10.0), ts(1.0)), 4.0);
        assert_eq!(st.value(MetricId::NonvolBuy, Some(10.0), ts(1.0)), 6.0);
        // Trailing window is (now−w, now] — at t=11 the t=1 trade is still in;
        // one ms past the edge drops everything.
        st.on_tick(ts(11.0));
        assert_eq!(st.value(MetricId::VolBuy, Some(10.0), ts(11.0)), 0.0);
        assert_eq!(st.value(MetricId::NonvolBuy, Some(10.0), ts(11.0)), 6.0);
        st.on_tick(ts(11.001));
        assert_eq!(st.value(MetricId::NonvolBuy, Some(10.0), ts(11.001)), 0.0);
        // Lifetime unchanged.
        assert_eq!(st.value(MetricId::VolBuy, None, ts(11.001)), 4.0);
    }

    /// Guard on replacing the old full-buffer rescan: the running-totals read must
    /// equal a brute-force `in_window` scan at every probe instant, including
    /// out-of-order arrivals (regressed `block_time`) and instants no `evict` ran at.
    #[test]
    fn windowed_running_totals_equal_a_brute_force_scan() {
        use super::super::flow_window::in_window;

        let vol = ix_hash(&["vol"]);
        let script: &[(Side, f64, bool, f64)] = &[
            (Side::Buy, 3.0, true, 0.0),
            (Side::Sell, 1.0, false, 4.0),
            (Side::Buy, 2.0, true, 9.0),
            (Side::Buy, 5.0, false, 7.0), // regressed
            (Side::Sell, 4.0, true, 12.0),
            (Side::Buy, 1.5, false, 11.0), // regressed
            (Side::Sell, 0.5, true, 25.0),
        ];
        let ids = [
            MetricId::WinVolBuy,
            MetricId::WinVolSell,
            MetricId::WinVolNet,
            MetricId::WinVolGross,
            MetricId::WinNonvolBuy,
            MetricId::WinNonvolSell,
            MetricId::WinNonvolNet,
            MetricId::WinNonvolGross,
            MetricId::WinVolShare,
        ];
        for window in [1.0_f64, 5.0, 10.0, 60.0] {
            let mut st = FlowState::new(FlowPatterns::new(BTreeSet::from([vol])));
            st.ensure_window(window);
            let mut wallet = 100u64;
            for &(side, sol, is_vol, at) in script {
                wallet += 1; // fresh wallet each trade so contagion can't reclassify
                st.on_trade(&trade(side, sol, is_vol.then_some(vol), wallet, at));
                for probe in [-3.0, 0.0, 0.5, 3.0, 12.0] {
                    let now = ts(at + probe);
                    let w = &st.windows[&window_key(window)];
                    let mut want = FlowTotals::default();
                    for &(t, (signed, v)) in &w.buf {
                        if !in_window(t, now, window) {
                            continue;
                        }
                        if signed >= 0.0 {
                            want.add(Side::Buy, signed, v);
                        } else {
                            want.add(Side::Sell, -signed, v);
                        }
                    }
                    for id in ids {
                        let (got, exp) = (st.value(id, Some(window), now), want.value(id));
                        assert!(
                            (got - exp).abs() < 1e-9 || (got.is_nan() && exp.is_nan()),
                            "{id:?} w={window} at={at} probe={probe}: {got} != {exp}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn flow_unconfigured_warning_fires() {
        let params = json!({"entry": {"m_flow_split": {"vol_buy": [{"operator": ">", "value": 1}]}}});
        assert!(flow_unconfigured_warning(&params, &json!({})).is_some());
        let cfg = json!({"m_flow_split": {"volume_ix_patterns": [["Pump.Fun: Buy"]]}});
        assert!(flow_unconfigured_warning(&params, &cfg).is_none());
    }
}
