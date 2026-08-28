//! Volume/organic flow split — SSOT hashes, classifier, and per-fingerprint state
//! for `m_flow_ix` (lifetime) + `m_flow_ix_window` (trailing window).
//!
//! See `hunter/docs/plans/strategies/metrics-reference.md`.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde_json::Value;

use super::flow_window::push_sorted;
use super::{MetricId, Side, TradeLite, Ts, WindowKey, WindowSpec};

use crate::grouping::normalize_labels;
use crate::hash::{fnv1a_byte, fnv1a_bytes, HashedSet, FNV_OFFSET};

/// Stable hash of an ordered instruction-label sequence (exact-order match
/// semantics, same as the fingerprint matcher's `ix_labels`). Labels are
/// separated with a single `0x1f` unit-separator byte so `["ab","c"]` ≠ `["a","bc"]`.
///
/// Empty input still returns a defined hash; callers that mean "missing labels"
/// should set `TradeLite::ix_hash = None` instead of hashing an empty slice.
/// The config key this group reads, inside `fingerprints.metric_config`.
pub const CONFIG_KEY: &str = "m_flow_ix";

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

// ── Structural markers ───────────────────────────────────────────────────────

/// The structural markers a build can carry, one bit each.
///
/// A marker is a *mechanism*, not a snapshot of one. `CreateAccountWithSeed` means
/// the transaction creates a throwaway account inline — nobody is coming back to it,
/// so it is a disposable machine rather than a person with a wallet. That stays true
/// for every future build, which is exactly what an exact-sequence pattern list
/// cannot promise: on this tape 531 distinct label sequences carry the seed marker
/// and new variants ship continuously, so a list books the unlisted ones as human.
///
/// **Matching is substring containment** over each label, because a label carries its
/// program prefix (`System Program: CreateAccountWithSeed`). The vocabulary is fixed
/// and small on purpose - a marker set that grows per rule is a pattern list again.
///
/// Two kinds live here, and both are mechanisms:
///
/// * **machinery** - what the transaction DOES (a throwaway account, a nonce, a memo);
/// * **router** - the retail front-end a person clicked through. A named router is a
///   human decision with a UI in front of it, which is a property of the BUILD and not
///   of who sent it, so it belongs beside the machinery markers rather than in a wallet
///   list. The set grows only when a new front-end carries retail order flow - never
///   per rule.
pub const MARKERS: [(&str, u16); 10] = [
    ("AdvanceNonceAccount", 1 << 0),
    ("CreateAccountWithSeed", 1 << 1),
    ("System Program: Transfer", 1 << 2),
    ("Pump.Fun: Create", 1 << 3),
    ("Memo Program", 1 << 4),
    // Routers. The label carries the program prefix, e.g. `Bloom Router: Unknown`.
    ("Axiom Trade", 1 << 5),
    ("Photon", 1 << 6),
    ("Bloom Router", 1 << 7),
    ("Trojan Trade", 1 << 8),
    ("Terminal", 1 << 9),
];

/// Every router bit as one mask - the "a person clicked this" side of the vocabulary.
pub const ROUTER_MARKERS: u16 = (1 << 5) | (1 << 6) | (1 << 7) | (1 << 8) | (1 << 9);

/// Structural markers present in an ordered label list. The producer's job — it is
/// the only layer that holds the strings.
pub fn marker_bits(labels: &[impl AsRef<str>]) -> u16 {
    let mut bits = 0u16;
    for lab in labels {
        let s = lab.as_ref();
        for (name, bit) in MARKERS {
            if bits & bit == 0 && s.contains(name) {
                bits |= bit;
            }
        }
    }
    bits
}

/// [`marker_bits`] over labels in their stored JSON form. Goes through
/// [`normalize_labels`] so both persisted shapes (bare array and
/// `{"instructions": [...]}`) read alike, for the same reason
/// [`ix_hash_from_labels_value`] does.
pub fn marker_bits_from_labels_value(labels: &Value) -> u16 {
    marker_bits(&normalize_labels(labels))
}

/// Compile a configured list of marker names into a mask. Errors on an unknown
/// name rather than ignoring it — a typo that silently matches nothing would make
/// a cleanliness gate pass on bot traffic.
pub fn marker_mask(names: &[impl AsRef<str>]) -> Result<u16, String> {
    let mut mask = 0u16;
    for n in names {
        let n = n.as_ref();
        match MARKERS.iter().find(|(name, _)| *name == n) {
            Some((_, bit)) => mask |= bit,
            None => {
                let known: Vec<&str> = MARKERS.iter().map(|(n, _)| *n).collect();
                return Err(format!("unknown ix marker `{n}` (known: {})", known.join(", ")));
            }
        }
    }
    Ok(mask)
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

/// [`ix_hash_opt`] over labels still in their stored **JSON** form, without
/// allocating on the hot shape.
///
/// Turning each row back into a `Vec<String>` just to feed [`ix_hash`] costs a
/// `serde_json` parse plus one heap allocation per label, **per trade**, on corpora
/// of millions of rows — so the offline paths must not: this walks the common
/// bare-array form in place.
///
/// Exactness is not traded away: the scanner handles only the shape the writers
/// emit most (a flat array of unescaped strings) and **falls back to
/// [`ix_hash_from_labels_value`]** the moment it meets an escape, the object
/// wrapper, or anything unexpected — so the result is by construction whatever the
/// shape-complete reader would have returned, including the "unparseable ⇒ `None`
/// ⇒ organic" behaviour. Locked by `json_scanner_matches_the_normalized_hash`.
pub fn ix_hash_from_labels_json(json: &str) -> Option<u64> {
    match scan_labels_json(json.as_bytes()) {
        Some(h) => h,
        None => {
            let value: Value = serde_json::from_str(json).ok()?;
            ix_hash_from_labels_value(&value)
        }
    }
}

/// [`ix_hash_opt`] over labels already decoded into a [`Value`] — the shape a
/// Postgres `trades.ix_labels` / `tokens.ix_labels` row arrives in.
///
/// Goes through [`normalize_labels`], so **both** persisted shapes hash alike: the
/// bare array `["A","B"]` and the object wrapper `{"instructions":["A","B"]}`.
/// Every reader of that column must be shape-complete or it books object-shaped
/// rows as organic — silently, because "this trade has no labels" is a legal state
/// that looks identical. That is the same class of defect
/// `storage::ix_labels_sql` exists to prevent on the SQL side.
pub fn ix_hash_from_labels_value(labels: &Value) -> Option<u64> {
    ix_hash_opt(&normalize_labels(labels))
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

/// Compiled classifier config for one fingerprint (`m_flow_ix`).
///
/// Three independent ways to call a trade volume-side, each switchable:
/// exact-sequence patterns, structural markers, and the two wallet-keyed rules
/// (contagion + creator). The wallet rules default **on**, so every fingerprint
/// stored before markers existed classifies exactly as it did.
///
/// A structural rule wants them **off**: "does this transaction carry a throwaway
/// account" is a property of the transaction, and contagion makes it a property of
/// the sender's history on that token instead. Leaving them on does not merely
/// tighten such a gate, it measures a different thing — the fire set stops matching
/// the one the rule was derived on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowPatterns {
    /// FNV-1a of each configured label sequence.
    hashes: BTreeSet<u64>,
    /// Structural marker mask ([`MARKERS`]); `0` = no marker rule.
    markers: u16,
    /// Which SIDE the mask names. `false` (default): a marker means volume-side.
    /// `true`: a marker means ORGANIC, so everything without one is volume-side.
    ///
    /// The inverse is a different claim, not a convenience. "Carries a throwaway
    /// account" identifies machines and leaves everything else unjudged; "came
    /// through a named router" identifies people and judges everything else machine.
    /// On the 8dtx tape that difference is the whole edge - the two gates read +0.99
    /// and +6.86 per trade on the same fires - so the classifier has to be able to
    /// say which one a rule means.
    markers_are_organic: bool,
    /// Tag a wallet volume-side for the rest of this token once it trades that way.
    wallet_contagion: bool,
    /// The creator wallet is volume-side unconditionally.
    creator_is_tagged: bool,
}

impl Default for FlowPatterns {
    fn default() -> Self {
        Self {
            hashes: BTreeSet::new(),
            markers: 0,
            markers_are_organic: false,
            wallet_contagion: true,
            creator_is_tagged: true,
        }
    }
}

impl FlowPatterns {
    pub fn new(hashes: BTreeSet<u64>) -> Self {
        Self { hashes, ..Self::default() }
    }

    /// A purely structural classifier: markers only, both wallet rules off. The mask
    /// names the VOLUME side.
    pub fn markers_only(markers: u16) -> Self {
        Self {
            hashes: BTreeSet::new(),
            markers,
            markers_are_organic: false,
            wallet_contagion: false,
            creator_is_tagged: false,
        }
    }

    /// A purely structural classifier whose mask names the ORGANIC side: a trade is
    /// volume-side unless it carries one of these markers. This is how "every buy in
    /// the burst came through a named retail router" is stated.
    pub fn organic_markers_only(markers: u16) -> Self {
        Self {
            hashes: BTreeSet::new(),
            markers,
            markers_are_organic: true,
            wallet_contagion: false,
            creator_is_tagged: false,
        }
    }

    /// Whether the configured mask names the organic side.
    pub fn markers_are_organic(&self) -> bool {
        self.markers_are_organic
    }

    pub fn contains(&self, h: u64) -> bool {
        self.hashes.contains(&h)
    }

    /// True when the trade's structural markers say VOLUME under the configured mask:
    /// intersecting it when the mask names the volume side, missing it entirely when
    /// the mask names the organic side.
    pub fn marks(&self, bits: u16) -> bool {
        if self.markers == 0 {
            return false;
        }
        if self.markers_are_organic {
            bits & self.markers == 0
        } else {
            bits & self.markers != 0
        }
    }

    pub fn wallet_contagion(&self) -> bool {
        self.wallet_contagion
    }

    pub fn creator_is_tagged(&self) -> bool {
        self.creator_is_tagged
    }

    pub fn is_empty(&self) -> bool {
        self.hashes.is_empty() && self.markers == 0
    }

    /// Compile an ordered list of label sequences (the sweep run's
    /// `ix_patterns` / fingerprint config array).
    pub fn from_label_sequences(patterns: &[Vec<String>]) -> Self {
        let mut hashes = BTreeSet::new();
        for p in patterns {
            if !p.is_empty() {
                hashes.insert(ix_hash(p));
            }
        }
        Self { hashes, ..Self::default() }
    }

    /// Parse `metric_config["m_flow_ix"]`. `None` = key absent (unconfigured ⇒
    /// flow metrics stay `NaN`). `Some` = configured (patterns may be empty —
    /// only contagion + creator classify as volume).
    pub fn from_metric_config(cfg: &Value) -> Option<Self> {
        let obj = cfg.get(CONFIG_KEY)?;
        if !obj.is_object() {
            return None;
        }
        let mut out = Self::default();
        if let Some(v) = obj.get("wallet_contagion") {
            out.wallet_contagion = v.as_bool()?;
        }
        if let Some(v) = obj.get("creator_is_tagged") {
            out.creator_is_tagged = v.as_bool()?;
        }
        for (key, organic) in [("tagged_ix_markers", false), ("untagged_ix_markers", true)] {
            let Some(arr) = obj.get(key) else { continue };
            let Value::Array(names) = arr else {
                return None;
            };
            let mut strs: Vec<&str> = Vec::with_capacity(names.len());
            for n in names {
                strs.push(n.as_str()?);
            }
            out.markers = marker_mask(&strs).ok()?;
            out.markers_are_organic = organic;
        }
        let Some(arr) = obj.get("ix_patterns") else {
            // Key present but no patterns field — markers and switches still apply.
            return Some(out);
        };
        let Value::Array(patterns) = arr else {
            return None;
        };
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
                out.hashes.insert(ix_hash(&strs));
            }
        }
        Some(out)
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
        if let Some(flow) = obj.get("m_flow_ix") {
            let Some(flow_obj) = flow.as_object() else {
                return Err("m_flow_ix must be an object".into());
            };
            if let Some(patterns) = flow_obj.get("ix_patterns") {
                let Some(arr) = patterns.as_array() else {
                    return Err("m_flow_ix.ix_patterns must be an array".into());
                };
                for (i, p) in arr.iter().enumerate() {
                    let Some(labels) = p.as_array() else {
                        return Err(format!(
                            "m_flow_ix.ix_patterns[{i}] must be an array of strings"
                        ));
                    };
                    for (j, lab) in labels.iter().enumerate() {
                        if !lab.is_string() {
                            return Err(format!(
                                "m_flow_ix.ix_patterns[{i}][{j}] must be a string"
                            ));
                        }
                    }
                }
            }
            for key in ["tagged_ix_markers", "untagged_ix_markers"] {
                let Some(markers) = flow_obj.get(key) else { continue };
                let Some(arr) = markers.as_array() else {
                    return Err(format!("m_flow_ix.{key} must be an array"));
                };
                let mut names: Vec<&str> = Vec::with_capacity(arr.len());
                for (i, m) in arr.iter().enumerate() {
                    let Some(s) = m.as_str() else {
                        return Err(format!("m_flow_ix.{key}[{i}] must be a string"));
                    };
                    names.push(s);
                }
                // An unknown marker silently matching nothing would let a
                // cleanliness gate pass on bot traffic, so it is an error.
                marker_mask(&names).map_err(|e| format!("m_flow_ix.{e}"))?;
            }
            // A mask names ONE side. Configuring both is two contradictory classifiers
            // on one axis, and `ix_patterns` is itself a volume-side statement,
            // so none of it composes with an organic mask. Letting one silently win is
            // how a rule stops measuring what it says.
            if flow_obj.contains_key("untagged_ix_markers") {
                for other in ["tagged_ix_markers", "ix_patterns"] {
                    if flow_obj.contains_key(other) {
                        return Err(format!(
                            "m_flow_ix: untagged_ix_markers and {other} name opposite                              sides of the same split - configure exactly one"
                        ));
                    }
                }
            }
            for flag in ["wallet_contagion", "creator_is_tagged"] {
                if let Some(v) = flow_obj.get(flag) {
                    if !v.is_boolean() {
                        return Err(format!("m_flow_ix.{flag} must be a boolean"));
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
            if obj.contains_key("m_flow_ix") || obj.contains_key("m_flow_ix_window") {
                return true;
            }
        }
    }
    false
}

/// Warning text when a rule uses flow groups but the fingerprint has no
/// `m_flow_ix` key (metrics will read `NaN`).
pub fn flow_unconfigured_warning(params: &Value, metric_config: &Value) -> Option<String> {
    if !params_reference_flow(params) {
        return None;
    }
    if FlowPatterns::from_metric_config(metric_config).is_some() {
        return None;
    }
    Some(
        "rule references m_flow_ix/m_flow_ix_window but the fingerprint has no \
         m_flow_ix.ix_patterns config — flow metrics will be NaN"
            .into(),
    )
}

// ── Totals ───────────────────────────────────────────────────────────────────

/// Running vol/organic SOL totals (buy/sell absolute; net/gross/share derived),
/// plus the tagged TRANSACTION tallies the SOL sums cannot express.
///
/// The counts are tagged-side only because that is the side a pattern list names:
/// `ix_patterns` says which builds are volume-side, so "how many of them landed" is a
/// statement about the tagged set. The untagged remainder is everyone the classifier
/// declined to judge, and a tally of it counts strangers, not a machine.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FlowTotals {
    pub tagged_buy: f64,
    pub tagged_sell: f64,
    pub untagged_buy: f64,
    pub untagged_sell: f64,
    pub tagged_buy_n: u32,
    pub tagged_sell_n: u32,
}

impl FlowTotals {
    fn add(&mut self, side: Side, sol: f64, is_tagged: bool) {
        match (is_tagged, side) {
            (true, Side::Buy) => {
                self.tagged_buy += sol;
                self.tagged_buy_n += 1;
            }
            (true, Side::Sell) => {
                self.tagged_sell += sol;
                self.tagged_sell_n += 1;
            }
            (false, Side::Buy) => self.untagged_buy += sol,
            (false, Side::Sell) => self.untagged_sell += sol,
        }
    }

    /// Remove one entry - the exact inverse of [`add`](Self::add).
    ///
    /// The side is the sign BIT, not `signed >= 0.0`. A zero-SOL sell is stored as
    /// `-0.0`, which compares `>= 0.0` and takes the buy arm; the SOL sums never
    /// noticed, because subtracting zero from the wrong one changes nothing. A COUNT
    /// notices - it decrements the buy tally for a sell and stays wrong for the rest
    /// of the token. `is_sign_negative` reads the bit, so `-0.0` is the sell it is.
    fn sub_signed(&mut self, signed: f64, is_tagged: bool) {
        if !signed.is_sign_negative() {
            if is_tagged {
                self.tagged_buy -= signed;
                self.tagged_buy_n = self.tagged_buy_n.saturating_sub(1);
            } else {
                self.untagged_buy -= signed;
            }
        } else if is_tagged {
            self.tagged_sell += signed; // signed < 0
            self.tagged_sell_n = self.tagged_sell_n.saturating_sub(1);
        } else {
            self.untagged_sell += signed;
        }
    }

    pub fn value(self, id: MetricId) -> f64 {
        use MetricId::*;
        match id {
            TaggedBuy | WinTaggedBuy => self.tagged_buy,
            TaggedSell | WinTaggedSell => self.tagged_sell,
            TaggedNet | WinTaggedNet => self.tagged_buy - self.tagged_sell,
            TaggedGross | WinTaggedGross => self.tagged_buy + self.tagged_sell,
            UntaggedBuy | WinUntaggedBuy => self.untagged_buy,
            UntaggedSell | WinUntaggedSell => self.untagged_sell,
            UntaggedNet | WinUntaggedNet => self.untagged_buy - self.untagged_sell,
            UntaggedGross | WinUntaggedGross => self.untagged_buy + self.untagged_sell,
            TaggedBuyCount | WinTaggedBuyCount => f64::from(self.tagged_buy_n),
            TaggedSellCount | WinTaggedSellCount => f64::from(self.tagged_sell_n),
            TaggedShare | WinTaggedShare => {
                let vg = self.tagged_buy + self.tagged_sell;
                let ng = self.untagged_buy + self.untagged_sell;
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
/// rebuilding the totals from a full scan. This one matters most: `value` is called
/// once **per flow metric per rule per event**, so a full scan per call costs a rule
/// with three `m_flow_ix_window` conditions three whole-window walks on every
/// 200 ms tick of every tracked token.
#[derive(Debug, Clone, PartialEq)]
struct FlowIxWindowState {
    spec: WindowSpec,
    /// `(pos, signed SOL, is_volume)` - buy positive, sell negative; oldest at
    /// front, kept position-sorted. `pos` is already in the window's own unit, so
    /// this one implementation serves seconds and slots alike.
    buf: VecDeque<(i64, (f64, bool))>,
    /// Running totals over **all** of `buf`.
    totals: FlowTotals,
}

impl FlowIxWindowState {
    fn new(spec: WindowSpec) -> Self {
        Self { spec, buf: VecDeque::new(), totals: FlowTotals::default() }
    }

    fn on_trade(&mut self, side: Side, sol: f64, is_tagged: bool, pos: i64, now_pos: i64) {
        let signed = match side {
            Side::Buy => sol,
            Side::Sell => -sol,
        };
        push_sorted(&mut self.buf, pos, (signed, is_tagged));
        self.totals.add(side, sol, is_tagged);
        self.evict(now_pos);
    }

    fn evict(&mut self, now_pos: i64) {
        let (lo, _) = self.spec.bounds(now_pos);
        while let Some(&(pos, (signed, is_tagged))) = self.buf.front() {
            if pos >= lo {
                break;
            }
            self.buf.pop_front();
            self.totals.sub_signed(signed, is_tagged);
        }
    }

    /// Totals over the window at `now_pos`, from the running totals minus the ends
    /// that fall outside: not-yet-evicted entries at the front, and entries past the
    /// high bound at the back - a lagged window's excluded head, or anything a
    /// regressed `block_time` pushed in. Sortedness makes the first in-window entry
    /// a valid stop for both loops.
    fn totals_at(&self, now_pos: i64) -> FlowTotals {
        let (lo, hi) = self.spec.bounds(now_pos);
        let mut out = self.totals;
        for &(pos, (signed, is_tagged)) in self.buf.iter() {
            if pos >= lo {
                break;
            }
            out.sub_signed(signed, is_tagged);
        }
        for &(pos, (signed, is_tagged)) in self.buf.iter().rev() {
            if pos <= hi {
                break;
            }
            out.sub_signed(signed, is_tagged);
        }
        out
    }

    fn value(&self, id: MetricId, now_pos: i64) -> f64 {
        self.totals_at(now_pos).value(id)
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
    windows: BTreeMap<WindowKey, FlowIxWindowState>,
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

    /// Adopt an edited pattern set (rules reload). Cheap no-op when unchanged, which
    /// is the common case — a reload fires for any rule edit, not just a pattern one.
    ///
    /// Contagion tags are deliberately **kept**: a wallet already shown to be a
    /// volume maker does not stop being one because the pattern that caught it was
    /// re-worded, and dropping the set would reclassify its future trades as organic.
    pub fn set_patterns(&mut self, patterns: &FlowPatterns) {
        if &self.patterns != patterns {
            self.patterns = patterns.clone();
        }
    }

    pub fn ensure_window(&mut self, spec: WindowSpec) {
        self.windows.entry(spec.key()).or_insert_with(|| FlowIxWindowState::new(spec));
    }

    /// Classify + fold one trade into lifetime and every registered window.
    pub fn on_trade(&mut self, t: &TradeLite, cur: super::Cursor) {
        if !t.sol.is_finite() || t.sol < 0.0 {
            return;
        }
        let is_tagged = self.classify(t);
        if is_tagged {
            self.tagged_wallets.insert(t.wallet_hash);
        }
        self.lifetime.add(t.side, t.sol, is_tagged);
        for w in self.windows.values_mut() {
            let pos = w.spec.pos(t.at, cur.at_trade(t));
            let now_pos = w.spec.now_pos(t.at, cur);
            w.on_trade(t.side, t.sol, is_tagged, pos, now_pos);
        }
    }

    pub fn on_tick(&mut self, now: Ts, cur: super::Cursor) {
        for w in self.windows.values_mut() {
            let now_pos = w.spec.now_pos(now, cur);
            w.evict(now_pos);
        }
    }

    /// Lifetime (`m_flow_ix`) or windowed (`m_flow_ix_window`) read at `now`.
    pub fn value(&self, id: MetricId, window: Option<WindowSpec>, now: Ts, cur: super::Cursor) -> f64 {
        match window {
            None => self.lifetime.value(id),
            Some(spec) => match self.windows.get(&spec.key()) {
                Some(w) => w.value(id, spec.now_pos(now, cur)),
                None => f64::NAN,
            },
        }
    }

    /// Volume-side iff a structural marker matches, an exact pattern matches, or —
    /// when the wallet rules are enabled — the wallet is already tagged or is the
    /// creator.
    ///
    /// Markers are tested first because they are the only rule that reads the
    /// transaction alone. With `wallet_contagion` and `creator_is_tagged` both off
    /// this function is a pure function of the trade, which is what a structural
    /// gate needs and what the two wallet rules would quietly take away.
    pub fn classify(&self, t: &TradeLite) -> bool {
        if self.patterns.marks(t.marker_bits) {
            return true;
        }
        if self.patterns.creator_is_tagged() && self.creator_wallet_hash == Some(t.wallet_hash) {
            return true;
        }
        if self.patterns.wallet_contagion() && self.tagged_wallets.contains(&t.wallet_hash) {
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
    use crate::metrics::{Cursor, Ts};
    use chrono::{Duration, TimeZone, Utc};
    use serde_json::json;

    /// A slot-only cursor. Every case here reads a seconds or slot window, where the
    /// print ordinal is never consulted; a print-window case states its own cursor.
    fn c(slot: u64) -> Cursor {
        Cursor { slot, print: 0 }
    }

    fn ts(secs: f64) -> Ts {
        Utc.timestamp_opt(1_700_000_000, 0).unwrap()
            + Duration::milliseconds((secs * 1000.0) as i64)
    }

    fn trade(side: Side, sol: f64, ix: Option<u64>, wallet: u64, secs: f64) -> TradeLite {
        TradeLite {
            slot: 0,
            marker_bits: 0,
            side,
            sol,
            price: 1.0,
            reserve_sol: 10.0,
            priced_reserve_sol: 10.0,
            at: ts(secs),
            ix_hash: ix,
            wallet_hash: wallet,
            leg_index: 0,
            ..Default::default()
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

    /// The in-place JSON scanner must agree with "decode, normalize, then hash" on
    /// every input — the happy shapes the writers emit, the degenerate ones, the
    /// object wrapper, and the escaped / malformed ones where it is required to fall
    /// back rather than guess. This is the whole safety argument for skipping the
    /// per-trade `serde_json` parse.
    #[test]
    fn json_scanner_matches_the_normalized_hash() {
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
            // Object wrapper — the second persisted shape (see `ix_labels_sql`).
            r#"{"instructions":["Pump.Fun: Create","Pump.Fun: Buy"]}"#,
            r#"{ "instructions" : [ "a" , "b" ] }"#,
            r#"{"instructions":[]}"#,
            r#"{"instructions":null}"#,
            r#"{"other":["a"]}"#,
            "{}",
            "null",            // unparseable ⇒ None
            "not json",
            "[1,2]",           // wrong element type ⇒ None
            r#"["unterminated"#,
            r#"["a"] trailing"#,
            "",
        ];
        for case in cases {
            let value: Value = serde_json::from_str(case).unwrap_or(Value::Null);
            assert_eq!(
                ix_hash_from_labels_json(case),
                ix_hash_opt(&normalize_labels(&value)),
                "scanner disagreed on {case:?}"
            );
        }
        // And it really does produce the SSOT hash on the shape that matters.
        assert_eq!(
            ix_hash_from_labels_json(r#"["Pump.Fun: Create","Pump.Fun: Buy"]"#),
            Some(ix_hash(&["Pump.Fun: Create", "Pump.Fun: Buy"])),
        );
    }

    /// The two persisted `ix_labels` shapes are the SAME label sequence, so they
    /// must hash to the same volume-pattern identity — whichever entry point a
    /// caller reaches them through (stored text or a decoded `Value`).
    ///
    /// A reader that understands only the bare array books every object-shaped row
    /// as organic, which is silent: "no labels" is a legal state, so the flow split
    /// just quietly under-counts volume and over-counts organic.
    #[test]
    fn both_persisted_label_shapes_hash_alike() {
        let want = Some(ix_hash(&["Pump.Fun: Create", "Pump.Fun: Buy"]));
        let bare = json!(["Pump.Fun: Create", "Pump.Fun: Buy"]);
        let wrapped = json!({ "instructions": ["Pump.Fun: Create", "Pump.Fun: Buy"] });

        assert_eq!(ix_hash_from_labels_value(&bare), want);
        assert_eq!(ix_hash_from_labels_value(&wrapped), want);
        assert_eq!(ix_hash_from_labels_json(&bare.to_string()), want);
        assert_eq!(ix_hash_from_labels_json(&wrapped.to_string()), want);

        // Absent / empty stays the missing sentinel in both shapes.
        assert_eq!(ix_hash_from_labels_value(&json!([])), None);
        assert_eq!(ix_hash_from_labels_value(&json!({ "instructions": [] })), None);
        assert_eq!(ix_hash_from_labels_value(&Value::Null), None);
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
            "m_flow_ix": {
                "ix_patterns": [
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
        st.on_trade(&t, c(0));
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
    fn lifetime_totals_and_tagged_share() {
        let patterns = FlowPatterns::new(BTreeSet::from([ix_hash(&["vol"])]));
        let mut st = FlowState::new(patterns);
        st.on_trade(&trade(Side::Buy, 4.0, Some(ix_hash(&["vol"])), 1, 0.0), c(0));
        st.on_trade(&trade(Side::Buy, 6.0, None, 2, 1.0), c(0));
        st.on_trade(&trade(Side::Sell, 1.0, Some(ix_hash(&["vol"])), 1, 2.0), c(0));

        assert_eq!(st.value(MetricId::TaggedBuy, None, ts(2.0), c(0)), 4.0);
        assert_eq!(st.value(MetricId::TaggedSell, None, ts(2.0), c(0)), 1.0);
        assert_eq!(st.value(MetricId::TaggedGross, None, ts(2.0), c(0)), 5.0);
        assert_eq!(st.value(MetricId::TaggedNet, None, ts(2.0), c(0)), 3.0);
        assert_eq!(st.value(MetricId::UntaggedBuy, None, ts(2.0), c(0)), 6.0);
        assert_eq!(st.value(MetricId::UntaggedGross, None, ts(2.0), c(0)), 6.0);
        // tagged_share = 5 / 11 * 100
        let share = st.value(MetricId::TaggedShare, None, ts(2.0), c(0));
        assert!((share - 500.0 / 11.0).abs() < 1e-9);
    }

    #[test]
    fn tagged_share_nan_at_zero() {
        let st = FlowState::new(FlowPatterns::default());
        assert!(st.value(MetricId::TaggedShare, None, ts(0.0), c(0)).is_nan());
    }

    #[test]
    fn window_evicts_on_tick() {
        let patterns = FlowPatterns::new(BTreeSet::from([ix_hash(&["vol"])]));
        let mut st = FlowState::new(patterns);
        st.ensure_window(WindowSpec::secs(10.0));
        st.on_trade(&trade(Side::Buy, 4.0, Some(ix_hash(&["vol"])), 1, 0.0), c(0));
        st.on_trade(&trade(Side::Buy, 6.0, None, 2, 1.0), c(0));
        assert_eq!(st.value(MetricId::TaggedBuy, Some(WindowSpec::secs(10.0)), ts(1.0), c(0)), 4.0);
        assert_eq!(st.value(MetricId::UntaggedBuy, Some(WindowSpec::secs(10.0)), ts(1.0), c(0)), 6.0);
        // Trailing window is (now−w, now] — at t=11 the t=1 trade is still in;
        // one ms past the edge drops everything.
        st.on_tick(ts(11.0), c(0));
        assert_eq!(st.value(MetricId::TaggedBuy, Some(WindowSpec::secs(10.0)), ts(11.0), c(0)), 0.0);
        assert_eq!(st.value(MetricId::UntaggedBuy, Some(WindowSpec::secs(10.0)), ts(11.0), c(0)), 6.0);
        st.on_tick(ts(11.001), c(0));
        assert_eq!(st.value(MetricId::UntaggedBuy, Some(WindowSpec::secs(10.0)), ts(11.001), c(0)), 0.0);
        // Lifetime unchanged.
        assert_eq!(st.value(MetricId::TaggedBuy, None, ts(11.001), c(0)), 4.0);
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
            (Side::Sell, 0.0, true, 13.0), // zero-SOL sell: `-0.0`, the sign-bit case
            (Side::Sell, 0.5, true, 25.0),
        ];
        let ids = [
            MetricId::WinTaggedBuy,
            MetricId::WinTaggedSell,
            MetricId::WinTaggedNet,
            MetricId::WinTaggedGross,
            MetricId::WinUntaggedBuy,
            MetricId::WinUntaggedSell,
            MetricId::WinUntaggedNet,
            MetricId::WinUntaggedGross,
            MetricId::WinTaggedShare,
            MetricId::WinTaggedBuyCount,
            MetricId::WinTaggedSellCount,
        ];
        for window in [1.0_f64, 5.0, 10.0, 60.0] {
            let mut st = FlowState::new(FlowPatterns::new(BTreeSet::from([vol])));
            st.ensure_window(WindowSpec::secs(window));
            let mut wallet = 100u64;
            for &(side, sol, is_tagged, at) in script {
                wallet += 1; // fresh wallet each trade so contagion can't reclassify
                st.on_trade(&trade(side, sol, is_tagged.then_some(vol), wallet, at), c(0));
                for probe in [-3.0, 0.0, 0.5, 3.0, 12.0] {
                    let now_ts = ts(at + probe);
                    let now = now_ts.timestamp_millis();
                    let w = &st.windows[&WindowSpec::secs(window).key()];
                    let mut want = FlowTotals::default();
                    for &(t, (signed, v)) in &w.buf {
                        if !in_window(WindowSpec::secs(window), t, now) {
                            continue;
                        }
                        // Sign BIT, matching `sub_signed` - a zero-SOL sell is `-0.0`.
                        if !signed.is_sign_negative() {
                            want.add(Side::Buy, signed, v);
                        } else {
                            want.add(Side::Sell, -signed, v);
                        }
                    }
                    for id in ids {
                        let (got, exp) = (st.value(id, Some(WindowSpec::secs(window)), now_ts, c(0)), want.value(id));
                        assert!(
                            (got - exp).abs() < 1e-9 || (got.is_nan() && exp.is_nan()),
                            "{id:?} w={window} at={at} probe={probe}: {got} != {exp}"
                        );
                    }
                }
            }
        }
    }

    /// The shared parity fixture, from the Rust side. Its twin is
    /// `classifyFlow.parity.test.ts`, which asserts the SAME file with the chart's
    /// TS port — the two implementations exist because the chart must redraw
    /// without a round trip, and that is only safe while they agree.
    ///
    /// The drift this catches is silent: a misclassified trade still yields a
    /// plausible split, so it surfaces as "the chart and the metric pane disagree"
    /// long after the change that caused it.
    #[test]
    fn flow_ix_matches_the_shared_parity_fixture() {
        #[derive(serde::Deserialize)]
        struct Case {
            name: String,
            patterns: Vec<Vec<String>>,
            creator: Option<String>,
            trades: Vec<FixtureTrade>,
            expect: Expect,
        }
        #[derive(serde::Deserialize)]
        struct FixtureTrade {
            wallet: String,
            side: String,
            sol: f64,
            labels: Option<Vec<String>>,
        }
        #[derive(serde::Deserialize)]
        struct Expect {
            tagged_buy: f64,
            tagged_sell: f64,
            untagged_buy: f64,
            untagged_sell: f64,
        }
        #[derive(serde::Deserialize)]
        struct Fixture {
            cases: Vec<Case>,
        }

        let raw = include_str!("../../fixtures/flow_ix_parity.json");
        let fixture: Fixture = serde_json::from_str(raw).expect("parity fixture parses");
        assert!(!fixture.cases.is_empty(), "fixture must carry cases");

        for case in fixture.cases {
            let mut st = FlowState::new(FlowPatterns::from_label_sequences(&case.patterns));
            if let Some(creator) = &case.creator {
                st.set_creator(wallet_hash(creator));
            }
            for (i, t) in case.trades.iter().enumerate() {
                st.on_trade(&TradeLite {
                    slot: 0,
                    marker_bits: 0,
                    side: if t.side == "buy" { Side::Buy } else { Side::Sell },
                    sol: t.sol,
                    price: 1.0,
                    reserve_sol: 10.0,
                    priced_reserve_sol: 10.0,
                    at: ts(i as f64),
                    ix_hash: t.labels.as_deref().and_then(ix_hash_opt),
                    wallet_hash: wallet_hash(&t.wallet),
                    leg_index: 0,
                    ..Default::default()
                }, c(0));
            }
            let now = ts(case.trades.len() as f64);
            for (id, want, label) in [
                (MetricId::TaggedBuy, case.expect.tagged_buy, "tagged_buy"),
                (MetricId::TaggedSell, case.expect.tagged_sell, "tagged_sell"),
                (MetricId::UntaggedBuy, case.expect.untagged_buy, "untagged_buy"),
                (MetricId::UntaggedSell, case.expect.untagged_sell, "untagged_sell"),
            ] {
                let got = st.value(id, None, now, c(0));
                assert!(
                    (got - want).abs() < 1e-9,
                    "case {:?}: {label} = {got}, expected {want}",
                    case.name,
                );
            }
        }
    }

    #[test]
    fn flow_unconfigured_warning_fires() {
        let params = json!({"entry": {"m_flow_ix": {"tagged_buy": [{"operator": ">", "value": 1}]}}});
        assert!(flow_unconfigured_warning(&params, &json!({})).is_some());
        let cfg = json!({"m_flow_ix": {"ix_patterns": [["Pump.Fun: Buy"]]}});
        assert!(flow_unconfigured_warning(&params, &cfg).is_none());
    }
    /// The marker is the mechanism, and an exact-sequence list is only a snapshot of
    /// it: a NEW bot build still creates a throwaway account, so a marker classifier
    /// catches it where a pattern list books it as human demand. That miss is the
    /// whole failure mode a cleanliness gate exists to prevent.
    #[test]
    fn a_marker_catches_a_build_the_pattern_list_has_never_seen() {
        let seed = marker_mask(&["CreateAccountWithSeed"]).unwrap();
        let known = ix_hash(&["System Program: CreateAccountWithSeed", "Pump.Fun: Buy"]);

        // Pattern-only: the listed build is volume, an unlisted variant is not.
        let by_pattern = FlowState::new(FlowPatterns::new(BTreeSet::from([known])));
        let unlisted = TradeLite {
            ix_hash: Some(ix_hash(&["Compute Budget: SetComputeUnitLimit",
                                    "System Program: CreateAccountWithSeed",
                                    "Pump.Fun: Buy"])),
            marker_bits: seed,
            ..Default::default()
        };
        assert!(!by_pattern.classify(&unlisted), "an unlisted variant reads as human");

        // Marker-only: it is caught on the structure alone.
        let by_marker = FlowState::new(FlowPatterns::markers_only(seed));
        assert!(by_marker.classify(&unlisted), "the marker catches it regardless");
    }

    /// With both wallet rules off the classifier is a pure function of the TRADE.
    /// Leaving them on does not merely tighten a structural gate - it measures a
    /// different thing, because a person who once used a bot build stays tagged.
    #[test]
    fn the_wallet_rules_can_be_switched_off_for_a_structural_gate() {
        let seed = marker_mask(&["CreateAccountWithSeed"]).unwrap();
        let router = TradeLite { wallet_hash: 7, marker_bits: 0, ..Default::default() };
        let bot = TradeLite { wallet_hash: 7, marker_bits: seed, sol: 1.0, ..Default::default() };

        // The default keeps both wallet rules on, which is what every stored
        // fingerprint relies on.
        let mut with_wallets = FlowState::new(FlowPatterns::new(BTreeSet::new()));
        with_wallets.set_creator(7);
        assert!(with_wallets.classify(&router), "creator rule tags the trade");

        let structural = FlowState::new(FlowPatterns::markers_only(seed));
        assert!(!structural.classify(&router), "no marker, no tag - whoever sent it");
        assert!(structural.classify(&bot), "and the marker still decides");
    }

    /// A cleanliness gate reads `tagged_buy == 0`. One bot transaction in the slot has
    /// to move it off zero, or the gate passes on a burst that contains a machine.
    #[test]
    fn one_marked_buy_moves_the_volume_side_off_zero() {
        let seed = marker_mask(&["CreateAccountWithSeed"]).unwrap();
        let mut st = FlowState::new(FlowPatterns::markers_only(seed));
        st.ensure_window(crate::metrics::WindowSpec::slots(1.0, 0.0));
        let at = ts(0.0);
        let buy = |sol: f64, bits: u16, slot: u64| TradeLite {
            side: Side::Buy, sol, at, slot, marker_bits: bits, ..Default::default()
        };
        st.on_trade(&buy(0.7, 0, 100), c(100));
        st.on_trade(&buy(0.5, 0, 100), c(100));
        assert_eq!(st.value(MetricId::WinTaggedBuy, Some(crate::metrics::WindowSpec::slots(1.0, 0.0)), at, c(100)), 0.0);
        assert_eq!(st.value(MetricId::WinUntaggedBuy, Some(crate::metrics::WindowSpec::slots(1.0, 0.0)), at, c(100)), 1.2);

        st.on_trade(&buy(0.4, seed, 100), c(100));
        assert_eq!(
            st.value(MetricId::WinTaggedBuy, Some(crate::metrics::WindowSpec::slots(1.0, 0.0)), at, c(100)),
            0.4,
            "the machine is on the volume side and the gate must see it"
        );
    }

    /// The rule `tagged_sell_count(1sl) >= 2` — "two sells carrying the dump build
    /// landed in the same slot".
    ///
    /// This is the reading `tagged_sell` cannot give: the two sells below total the
    /// same SOL as the single sell that precedes them, so a SOL threshold either
    /// fires on both or on neither. The count separates them, and it must fall back
    /// when the slot rolls — a latched counter would exit every later token.
    #[test]
    fn two_tagged_sells_in_one_slot_are_a_count_of_two() {
        let dump = ix_hash(&["Pump.Fun: Sell", "Token Program: CloseAccount"]);
        let w = crate::metrics::WindowSpec::slots(1.0, 0.0);
        let mut st = FlowState::new(FlowPatterns::new(BTreeSet::from([dump])));
        st.ensure_window(w);
        let at = ts(0.0);
        let sell = |sol: f64, ix: Option<u64>, wallet: u64, slot: u64| TradeLite {
            side: Side::Sell, sol, at, slot, ix_hash: ix, wallet_hash: wallet,
            ..Default::default()
        };
        let n = |st: &FlowState, slot: u64| st.value(MetricId::WinTaggedSellCount, Some(w), at, c(slot));

        // One dump-build sell of 2.0 SOL.
        st.on_trade(&sell(2.0, Some(dump), 1, 100), c(100));
        assert_eq!(n(&st, 100), 1.0);
        // Read the SOL sum NOW: a later slot evicts this one, so the two readings
        // have to be taken as the engine takes them, each at its own slot.
        let sol_one_big = st.value(MetricId::WinTaggedSell, Some(w), at, c(100));

        // Next slot: two dump-build sells of 1.0 each — same SOL, different count.
        st.on_trade(&sell(1.0, Some(dump), 2, 101), c(101));
        st.on_trade(&sell(1.0, Some(dump), 3, 101), c(101));
        assert_eq!(n(&st, 101), 2.0, "two sells at once is the fire");
        assert_eq!(
            st.value(MetricId::WinTaggedSell, Some(w), at, c(101)),
            sol_one_big,
            "and the SOL sum reads the same on both slots, which is why it cannot say it",
        );

        // A sell on some other build does not count, however big.
        st.on_trade(&sell(9.0, Some(ix_hash(&["Pump.Fun: Sell"])), 4, 102), c(102));
        assert_eq!(n(&st, 102), 0.0, "only the saved builds are counted");

        // And the window releases: an empty slot reads zero, not a latch.
        assert_eq!(n(&st, 103), 0.0);
    }

    /// A zero-SOL tagged sell must not be booked as a buy.
    ///
    /// It is stored as `-0.0`, and `-0.0 >= 0.0` is TRUE, so a magnitude test puts it
    /// on the buy side. The SOL sums cannot tell (zero subtracts the same either way)
    /// — the counts can, and a single one of these would leave `tagged_buy_count`
    /// permanently one short for the rest of the token.
    #[test]
    fn a_zero_sol_tagged_sell_is_a_sell_on_both_sides_of_the_window() {
        let dump = ix_hash(&["Pump.Fun: Sell"]);
        let w = crate::metrics::WindowSpec::slots(1.0, 0.0);
        let mut st = FlowState::new(FlowPatterns::new(BTreeSet::from([dump])));
        st.ensure_window(w);
        let at = ts(0.0);
        let t = |side: Side, sol: f64, wallet: u64, slot: u64| TradeLite {
            side, sol, at, slot, ix_hash: Some(dump), wallet_hash: wallet,
            ..Default::default()
        };
        st.on_trade(&t(Side::Buy, 1.0, 1, 100), c(100));
        st.on_trade(&t(Side::Sell, 0.0, 2, 100), c(100));
        assert_eq!(st.value(MetricId::WinTaggedBuyCount, Some(w), at, c(100)), 1.0);
        assert_eq!(st.value(MetricId::WinTaggedSellCount, Some(w), at, c(100)), 1.0);

        // Roll past the window so both entries are evicted, then re-read: eviction
        // is where the sign is consulted a second time, and the arm it takes there
        // must match the one `add` took.
        st.on_trade(&t(Side::Buy, 1.0, 3, 200), c(200));
        assert_eq!(
            st.value(MetricId::WinTaggedBuyCount, Some(w), at, c(200)),
            1.0,
            "the evicted zero-SOL SELL must not have decremented the BUY tally",
        );
        assert_eq!(st.value(MetricId::WinTaggedSellCount, Some(w), at, c(200)), 0.0);
        // Lifetime keeps every one of them.
        assert_eq!(st.value(MetricId::TaggedBuyCount, None, at, c(200)), 2.0);
        assert_eq!(st.value(MetricId::TaggedSellCount, None, at, c(200)), 1.0);
    }

    /// An unknown marker name is an ERROR, not an empty mask: a typo that silently
    /// matched nothing would let the gate pass on bot traffic.
    #[test]
    fn an_unknown_marker_name_is_rejected() {
        assert!(marker_mask(&["CreateAccountWithSeed"]).is_ok());
        let e = marker_mask(&["CreateAcountWithSeed"]).unwrap_err();
        assert!(e.contains("unknown ix marker"), "{e}");
    }

    /// Both persisted label shapes must yield the same bits, for the same reason
    /// `ix_hash_from_labels_value` is shape-complete.
    #[test]
    fn marker_bits_read_both_persisted_label_shapes() {
        let bare = serde_json::json!(["System Program: CreateAccountWithSeed", "Pump.Fun: Buy"]);
        let wrapped = serde_json::json!({
            "instructions": ["System Program: CreateAccountWithSeed", "Pump.Fun: Buy"]
        });
        let seed = marker_mask(&["CreateAccountWithSeed"]).unwrap();
        assert_eq!(marker_bits_from_labels_value(&bare), seed);
        assert_eq!(marker_bits_from_labels_value(&wrapped), seed);
    }

}
