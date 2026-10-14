//! What identifies a trade, as the engine compares it: the hash of its ordered
//! instruction labels ([`ix_hash`]), its build recipe ([`build_hash`]), its build core
//! ([`core_hash`], [`core_marks`]), its structural markers ([`marker_bits`]) and its
//! wallet ([`wallet_hash`]).
//!
//! The producers (live ingest, the lab's lake reader, the readout) compute these once
//! per trade from the strings, which only they hold, all through [`LabelKeys::of`];
//! the engine compares integers.

use serde_json::Value;

use crate::grouping::normalize_labels;
use crate::hash::{fnv1a_byte, fnv1a_bytes, FNV_OFFSET};
use crate::metrics::template_grain;

/// Stable hash of an ordered instruction-label sequence (exact-order match
/// semantics, same as the fingerprint matcher's `ix_labels`). Labels are
/// separated with a single `0x1f` unit-separator byte so `["ab","c"]` != `["a","bc"]`.
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

/// Whether a label is account setup, account teardown or a memo: instructions a
/// sender adds or drops around the same trade without changing what it does. A
/// build RECIPE is the label sequence without them ([`build_hash`]).
pub fn is_build_noise(label: &str) -> bool {
    label.starts_with("Associated Token: Create")
        || label.ends_with(": CloseAccount")
        || label.starts_with("Memo Program")
}

/// The build recipe: [`ix_hash`] over the ordered labels with [`is_build_noise`]
/// dropped, so two transactions that differ only in a token account opened, closed
/// or a memo are one recipe. `None` when labels are missing or empty, the same
/// sentinel as [`ix_hash_opt`].
///
/// The offline twin is the node-derivation toolkit's `lake_export.build_core` (the
/// same drop list, md5 of the kept labels joined by `|`); the two partition label
/// sequences identically, pinned by `build_hash_partitions_like_the_study_build_core`.
pub fn build_hash(labels: &[impl AsRef<str>]) -> Option<u64> {
    if labels.is_empty() {
        return None;
    }
    let mut h = FNV_OFFSET;
    let mut first = true;
    for lab in labels.iter().map(AsRef::as_ref).filter(|l| !is_build_noise(l)) {
        if !first {
            h = fnv1a_byte(h, 0x1f);
        }
        first = false;
        h = fnv1a_bytes(h, lab.as_bytes());
    }
    Some(h)
}

/// [`build_hash`] over labels decoded into a [`Value`], through [`normalize_labels`]
/// so both persisted shapes read alike (see [`ix_hash_from_labels_value`]).
pub fn build_hash_from_labels_value(labels: &Value) -> Option<u64> {
    build_hash(&normalize_labels(labels))
}

// ── Build core ───────────────────────────────────────────────────────────────

/// Whether a label is an EXTRA around a trade: something a sender adds, drops or moves
/// without changing what the transaction does. The template grain's boilerplate
/// ([`template_grain::is_boilerplate`]) plus every System Program instruction and the
/// Lighthouse guard. A build's [core](core_labels) is what is left.
pub fn is_core_extra(label: &str) -> bool {
    template_grain::is_boilerplate(label) || label.starts_with("System Program:") || label.starts_with("Lighthouse")
}

/// One label of a core: pump.fun's verb variants merged (`Buy`, `BuyV2`,
/// `BuyExactSolIn` -> `BUY`; `Sell`, `SellV2` -> `SELL`; `Create`, `Create_v2` ->
/// `CREATE`). Every other label, an app's own instruction included, is itself. An
/// already-merged verb maps to itself, so a core is its own core.
fn core_verb(label: &str) -> &str {
    if let Some(rest) = label.strip_prefix("Pump.Fun: ") {
        if rest.starts_with("Buy") || rest == "BUY" {
            return "Pump.Fun: BUY";
        }
        if rest.starts_with("Sell") || rest == "SELL" {
            return "Pump.Fun: SELL";
        }
        if rest.starts_with("Create") || rest == "CREATE" {
            return "Pump.Fun: CREATE";
        }
    }
    label
}

/// A build's core: its labels without the [extras](is_core_extra), in order, with
/// pump.fun's verb variants merged. A dev who rotates small variants of one build
/// (a memo added, the compute-budget pair swapped, `BuyV2` for `Buy`, a nonce) keeps
/// one core. The owner split defines it (`_!___owner-split.md`, "Match levels"); the
/// chart's twin is `ixCore.ts`.
pub fn core_labels<S: AsRef<str>>(labels: &[S]) -> Vec<&str> {
    labels.iter().map(AsRef::as_ref).filter(|l| !is_core_extra(l)).map(core_verb).collect()
}

/// [`ix_hash`] of the [core](core_labels); `None` when labels are absent (the same
/// sentinel as [`ix_hash_opt`]). A build of extras only has an empty core, which is
/// a real core with its own hash.
pub fn core_hash(labels: &[impl AsRef<str>]) -> Option<u64> {
    (!labels.is_empty()).then(|| ix_hash(&core_labels(labels)))
}

/// The extras of a build, packed: presence flags in bits 0-7 ([`CORE_MARK_FLAGS`]),
/// the System `Transfer` count in bits 8-15 and the `Associated Token: Create*`
/// count in bits 16-23 (both capped at 255). Order never counts; the two counts do.
/// Text form: [`core_marks_text`].
pub fn core_marks(labels: &[impl AsRef<str>]) -> u32 {
    let mut flags = 0u32;
    let (mut transfers, mut opens) = (0u32, 0u32);
    for l in labels {
        let l = l.as_ref();
        for (bit, (_, test)) in CORE_MARK_FLAGS.iter().enumerate() {
            if test(l) {
                flags |= 1 << bit;
            }
        }
        if l == "System Program: Transfer" {
            transfers += 1;
        }
        if l.starts_with("Associated Token: Create") {
            opens += 1;
        }
    }
    flags | (transfers.min(255) << 8) | (opens.min(255) << 16)
}

/// The presence flags of [`core_marks`], in their bit and text order.
#[allow(clippy::type_complexity)]
pub const CORE_MARK_FLAGS: [(&str, fn(&str) -> bool); 8] = [
    ("CL", |l| l == "Compute Budget: SetComputeUnitLimit"),
    ("CP", |l| l == "Compute Budget: SetComputeUnitPrice"),
    ("N", |l| l == "System Program: AdvanceNonceAccount"),
    ("L", |l| l.starts_with("Lighthouse")),
    ("M", |l| l.starts_with("Memo Program")),
    ("S", |l| l.starts_with("System Program: CreateAccount")),
    ("C", |l| l.ends_with(": CloseAccount")),
    ("W", |l| l.ends_with(": SyncNative")),
];

/// [`core_marks`] as text: the present flags in [`CORE_MARK_FLAGS`] order, then
/// `T<transfers> A<account opens>` — `"CL CP C T1 A3"`.
pub fn core_marks_text(marks: u32) -> String {
    let mut out: Vec<String> = CORE_MARK_FLAGS
        .iter()
        .enumerate()
        .filter(|(bit, _)| marks & (1 << bit) != 0)
        .map(|(_, (name, _))| (*name).to_string())
        .collect();
    out.push(format!("T{}", (marks >> 8) & 0xff));
    out.push(format!("A{}", (marks >> 16) & 0xff));
    out.join(" ")
}

/// Parse [`core_marks_text`]. Tokens in any order; a missing count reads `0`; an
/// unknown token or a count past 255 is an error.
pub fn core_marks_from_text(text: &str) -> Result<u32, String> {
    let mut marks = 0u32;
    for tok in text.split_whitespace() {
        if let Some(bit) = CORE_MARK_FLAGS.iter().position(|(n, _)| *n == tok) {
            marks |= 1 << bit;
            continue;
        }
        let (shift, n) = match (tok.strip_prefix('T'), tok.strip_prefix('A')) {
            (Some(n), _) => (8, n),
            (_, Some(n)) => (16, n),
            _ => return Err(format!("unknown core mark `{tok}` (flags CL CP N L M S C W, counts T<n> A<n>)")),
        };
        let n: u32 = n.parse().map_err(|_| format!("core mark `{tok}` needs a count"))?;
        if n > 255 {
            return Err(format!("core mark `{tok}` exceeds 255"));
        }
        marks = (marks & !(0xff << shift)) | (n << shift);
    }
    Ok(marks)
}

// ── Every label key at once ──────────────────────────────────────────────────

/// Every key a trade's instruction labels yield. Each producer derives a trade's
/// label keys through [`LabelKeys::of`], so one computation serves every adapter. An
/// adapter that builds its `TradeLite` from `Default` sets them with
/// [`TradeLite::with_label_keys`](crate::metrics::TradeLite::with_label_keys), which
/// leaves no key at its default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LabelKeys {
    pub ix_hash: Option<u64>,
    pub marker_bits: u16,
    pub template_hash: Option<u64>,
    pub program_hash: Option<u64>,
    pub build_hash: Option<u64>,
    pub core_hash: Option<u64>,
    pub core_marks: u32,
    pub is_launch: bool,
}

impl LabelKeys {
    /// Every key of one ordered label list. Empty labels are the "not captured"
    /// state: every hash `None`, no markers, no marks, not a launch.
    pub fn of(labels: &[impl AsRef<str>]) -> Self {
        Self {
            ix_hash: ix_hash_opt(labels),
            marker_bits: marker_bits(labels),
            template_hash: template_grain::grain_hash(labels),
            program_hash: template_grain::program_hash(labels),
            build_hash: build_hash(labels),
            core_hash: core_hash(labels),
            core_marks: core_marks(labels),
            is_launch: template_grain::is_launch(labels),
        }
    }

    /// [`of`](Self::of) over labels decoded into a [`Value`], through
    /// [`normalize_labels`] so both persisted shapes read alike.
    pub fn from_labels_value(labels: &Value) -> Self {
        Self::of(&normalize_labels(labels))
    }
}

/// Programs every sender's transaction carries around the trade, whatever app sent
/// it: a label whose program name starts with one of these never names the app.
pub const APP_INFRA: [&str; 5] = ["Compute Budget", "System Program", "Token Program", "Associated Token", "Memo Program"];

/// The app a transaction was sent through: the program of its first label that is not
/// [`APP_INFRA`] (a label's program is the text before `": "`). `None` for a direct
/// pump.fun call and for labels of infrastructure only: the pump.fun site and the bots
/// that call the program directly differ only in their recipe, so there the recipe is
/// the app.
///
/// The offline twin is the hot-tape study's `r1e_public.app_of` (case file L11).
pub fn recipe_app<S: AsRef<str>>(labels: &[S]) -> Option<&str> {
    let app = labels
        .iter()
        .map(|l| l.as_ref().split(": ").next().unwrap_or_default())
        .find(|p| !APP_INFRA.iter().any(|i| p.starts_with(i)))?;
    (app != "Pump.Fun").then_some(app)
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

/// Stable hash of a wallet address string (base58 or the lake's `unknown:{id}`
/// fallback). Contagion and creator checks compare these hashes only.
pub fn wallet_hash(addr: &str) -> u64 {
    fnv1a_bytes(FNV_OFFSET, addr.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeMap;

    /// The shared core vectors (twin: the chart's `ixCore.test.ts`): every
    /// implementation reduces a build alike.
    #[test]
    fn the_shared_core_fixture() {
        #[derive(serde::Deserialize)]
        struct Case {
            name: String,
            labels: Vec<String>,
            core: Vec<String>,
            marks: String,
        }
        #[derive(serde::Deserialize)]
        struct Fixture {
            cases: Vec<Case>,
        }
        let f: Fixture =
            serde_json::from_str(include_str!("../../fixtures/ix_core_parity.json")).expect("fixture parses");
        assert!(!f.cases.is_empty());
        for c in f.cases {
            assert_eq!(core_labels(&c.labels), c.core, "case {:?}: core", c.name);
            assert_eq!(core_marks_text(core_marks(&c.labels)), c.marks, "case {:?}: marks", c.name);
            assert_eq!(core_marks_from_text(&c.marks), Ok(core_marks(&c.labels)), "case {:?}: marks text", c.name);
            assert_eq!(core_hash(&c.labels), Some(ix_hash(&c.core)), "case {:?}: hash", c.name);
            // A core is its own core.
            assert_eq!(core_labels(&c.core), c.core, "case {:?}: idempotent", c.name);
        }
    }

    #[test]
    fn core_marks_text_reads_any_order_and_refuses_junk() {
        assert_eq!(core_marks_from_text("A3 CP CL T1"), core_marks_from_text("CL CP T1 A3"));
        assert_eq!(core_marks_from_text(""), Ok(0));
        assert!(core_marks_from_text("CU").is_err());
        assert!(core_marks_from_text("T").is_err());
        assert!(core_marks_from_text("A256").is_err());
        assert_eq!(core_hash(&[] as &[&str]), None, "absent labels stay the missing sentinel");
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

    /// The two persisted `ix_labels` shapes are the SAME label sequence, so they
    /// must yield the same keys, every one of them.
    ///
    /// A reader that understands only the bare array books every object-shaped row
    /// as organic, which is silent: "no labels" is a legal state, so the flow split
    /// just quietly under-counts volume and over-counts organic.
    #[test]
    fn both_persisted_label_shapes_key_alike() {
        let labels = ["System Program: CreateAccountWithSeed", "Pump.Fun: Create", "Pump.Fun: Buy"];
        let want = LabelKeys::of(&labels);
        assert_eq!(want.ix_hash, Some(ix_hash(&labels)));
        assert_eq!(want.marker_bits, marker_bits(&labels));
        assert_eq!(want.core_hash, core_hash(&labels));
        assert!(want.is_launch);

        assert_eq!(LabelKeys::from_labels_value(&json!(labels)), want);
        assert_eq!(LabelKeys::from_labels_value(&json!({ "instructions": labels })), want);

        // Absent / empty stays the missing state in both shapes.
        for absent in [json!([]), json!({ "instructions": [] }), Value::Null] {
            assert_eq!(LabelKeys::from_labels_value(&absent), LabelKeys::default());
        }
    }

    #[test]
    fn wallet_hash_stable() {
        assert_eq!(wallet_hash("Abc123"), wallet_hash("Abc123"));
        assert_ne!(wallet_hash("Abc123"), wallet_hash("abc123"));
        assert_ne!(wallet_hash("unknown:7"), wallet_hash("7"));
    }

    /// The shared parity fixture, from the Rust side. Its twin is
    /// `classifyFlow.parity.test.ts`, which asserts the SAME file with the chart's
    /// TS port — the two implementations exist because the chart must redraw
    /// without a round trip, and that is only safe while they agree.
    ///
    /// The drift this catches is silent: a misclassified trade still yields a
    /// plausible split, so it surfaces as "the chart and the metric pane disagree"
    /// long after the change that caused it.
    /// Two label sequences are one recipe exactly when the study's `build_core`
    /// says so. The fixture holds real lake sequences with their `build_core`,
    /// written by the toolkit function the hot-tape rules were derived with.
    fn assert_build_partition(raw: &str) {
        let v: Value = serde_json::from_str(raw).expect("fixture parses");
        let seqs = v["sequences"].as_array().expect("sequences");
        let mut by_core: BTreeMap<&str, u64> = BTreeMap::new();
        let mut by_hash: BTreeMap<u64, &str> = BTreeMap::new();
        for e in seqs {
            let core = e["build_core"].as_str().expect("build_core");
            let h = build_hash_from_labels_value(&e["labels"]).expect("labels present");
            assert_eq!(*by_core.entry(core).or_insert(h), h, "one build_core, two hashes: {e}");
            assert_eq!(*by_hash.entry(h).or_insert(core), core, "one hash, two build_cores: {e}");
        }
        assert!(by_core.len() < seqs.len(), "the fixture must hold sequences that collapse");
    }

    #[test]
    fn build_hash_partitions_like_the_study_build_core() {
        assert_build_partition(include_str!("../../fixtures/build_core_parity.json"));
        // The noise is dropped wherever it sits, and only the noise.
        let core = build_hash(&["Compute Budget: SetComputeUnitLimit", "Pump.Fun: Buy"]);
        assert_eq!(
            build_hash(&[
                "Compute Budget: SetComputeUnitLimit",
                "Associated Token: CreateIdempotent",
                "Pump.Fun: Buy",
                "Token Program: CloseAccount",
                "Memo Program: Memo",
            ]),
            core
        );
        assert_ne!(build_hash(&["Pump.Fun: Buy", "Compute Budget: SetComputeUnitLimit"]), core);
        assert_eq!(build_hash(&[] as &[&str]), None);
    }

    #[test]
    fn a_recipes_app_is_its_first_program_past_the_infrastructure() {
        let axiom = ["Compute Budget: SetComputeUnitPrice", "System Program: Transfer", "Axiom Trade: ix#00"];
        assert_eq!(recipe_app(&axiom), Some("Axiom Trade"));
        let unknown = ["Token Program: SyncNative", "Unknown (6Vo3245e): ix#01", "Pump.Fun: Buy"];
        assert_eq!(recipe_app(&unknown), Some("Unknown (6Vo3245e)"));
        // A direct pump.fun call, and infrastructure only: the recipe is the app.
        assert_eq!(recipe_app(&["Compute Budget: SetComputeUnitLimit", "Pump.Fun: Buy"]), None);
        assert_eq!(recipe_app(&["Compute Budget: SetComputeUnitLimit"]), None);
        assert_eq!(recipe_app(&[] as &[&str]), None);
    }

    /// Every distinct sequence in a lake export, not a sample: point
    /// `BUILD_CORE_ALL` at a file in the fixture's shape and run with `--ignored`.
    #[test]
    #[ignore]
    fn build_hash_partitions_every_exported_sequence() {
        let path = std::env::var("BUILD_CORE_ALL").expect("BUILD_CORE_ALL names the file");
        assert_build_partition(&std::fs::read_to_string(path).expect("file reads"));
    }

    /// An unknown marker name is an ERROR, not an empty mask: a typo that silently
    /// matched nothing would let the gate pass on bot traffic.
    #[test]
    fn an_unknown_marker_name_is_rejected() {
        assert!(marker_mask(&["CreateAccountWithSeed"]).is_ok());
        let e = marker_mask(&["CreateAcountWithSeed"]).unwrap_err();
        assert!(e.contains("unknown ix marker"), "{e}");
    }
}
