//! What identifies a trade, as the engine compares it: the hash of its ordered
//! instruction labels ([`ix_hash`]), its build recipe ([`build_hash`]), its structural
//! markers ([`marker_bits`]) and its wallet ([`wallet_hash`]).
//!
//! The producers (live ingest, the lab's lake reader, the readout) compute these once
//! per trade from the strings, which only they hold; the engine compares integers.

use serde_json::Value;

use crate::grouping::normalize_labels;
use crate::hash::{fnv1a_byte, fnv1a_bytes, FNV_OFFSET};

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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeMap;

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
