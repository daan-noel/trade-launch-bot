//! Token identity — the `(name, symbol)` pair a copycat re-launch re-uses.
//!
//! A scam re-launch keeps the name and the icon and changes only the mint, so a
//! guard against re-buying the same trap needs a key that survives a mint change.
//!
//! **Why not the icon.** The image is not on-chain: pump.fun's create instruction
//! carries a metadata `uri`, and the picture lives inside the JSON behind it. The
//! `uri` itself is useless as a key (every launch uploads its own JSON, so a
//! byte-identical image still gets a fresh `uri`), and fetching the JSON is an
//! off-chain round trip that has no business on the decision path. Name + symbol
//! is what the create event already hands us, for free.
//!
//! **ONE hasher**, like [`crate::metrics::flow_split::wallet_hash`]: the live
//! producer, the lake exporter, and the replay driver all call
//! [`token_identity_hash`], so a lake row and a live event can never disagree
//! about what a token's identity is. A private re-implementation on either side
//! is the bug this module exists to prevent.

use crate::hash::{fnv1a_byte, fnv1a_bytes, FNV_OFFSET};

/// A token's `(name, symbol)` identity, hashed. Always fits an `i64` unchanged
/// (see [`token_identity_hash`]), so Parquet / Postgres store it as-is.
pub type IdentityHash = u64;

/// Keeps the hash inside `i64::MAX` so the lake's Parquet column and any
/// `BIGINT` mirror hold the exact value the engine compares — no bit-casting, no
/// `u64`-shaped ceiling to decode (super-root rule: a raw `u64` that can pass
/// 2^53 needs its own handling; a 63-bit hash simply never gets there).
const IDENTITY_MASK: u64 = i64::MAX as u64;

/// Hash a token's `(name, symbol)` into its identity key, or `None` when either
/// half is blank after normalization.
///
/// `None` is the "no identity" sentinel and it is load-bearing: a token with no
/// name (or no symbol) must never *match* another such token, because "both
/// blank" is not evidence of a shared identity. Every caller treats `None` as
/// "never blocks, never records" — the zero-as-unbound rule applied to a hash.
pub fn token_identity_hash(name: &str, symbol: &str) -> Option<IdentityHash> {
    let name = normalize(name);
    let symbol = normalize(symbol);
    if name.is_empty() || symbol.is_empty() {
        return None;
    }
    // Unit-separated so ("ab","c") and ("a","bc") are different identities.
    let mut h = fnv1a_bytes(FNV_OFFSET, name.as_bytes());
    h = fnv1a_byte(h, 0x1f);
    h = fnv1a_bytes(h, symbol.as_bytes());
    Some(h & IDENTITY_MASK)
}

/// Fold a name/symbol to its comparison form: lowercase, with whitespace and
/// invisible formatting characters removed.
///
/// Deliberately **not** fuzzy. Homoglyph folding (Cyrillic `а` for Latin `a`) and
/// edit-distance matching would catch more copycats, but both silently block
/// entries on tokens that merely look similar, and a false positive here is
/// invisible — it costs a trade that never happened. Exact-after-normalization
/// keeps every skip explainable; the skip logs are the evidence for widening this
/// later.
fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace() && !is_invisible(*c))
        .flat_map(char::to_lowercase)
        .collect()
}

/// Zero-width and directional-formatting characters — invisible padding a
/// re-launch can add to dodge an exact match without changing what a human reads.
fn is_invisible(c: char) -> bool {
    matches!(c,
        '\u{00ad}'                  // soft hyphen
        | '\u{200b}'..='\u{200f}'   // zero-width space/joiners, LTR/RTL marks
        | '\u{202a}'..='\u{202e}'   // directional embedding/override
        | '\u{2060}'..='\u{2064}'   // word joiner, invisible operators
        | '\u{feff}'                // BOM / zero-width no-break space
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_name_and_symbol_hash_equal_regardless_of_case_and_spacing() {
        let a = token_identity_hash("Moon Dog", "MDOG").unwrap();
        for variant in ["moon dog", "MOONDOG", " Moon  Dog ", "MoOnDoG"] {
            assert_eq!(token_identity_hash(variant, "mdog"), Some(a), "{variant}");
        }
    }

    #[test]
    fn invisible_padding_does_not_dodge_the_match() {
        // The whole point: a re-launch that pads with zero-width characters reads
        // identically to a human and must hash identically to us.
        let plain = token_identity_hash("Moon Dog", "MDOG").unwrap();
        let padded = token_identity_hash("Moon\u{200b}Dog", "M\u{feff}DOG").unwrap();
        assert_eq!(plain, padded);
    }

    #[test]
    fn name_and_symbol_are_separated_not_concatenated() {
        // ("ab","c") and ("a","bc") share a concatenation but are different tokens.
        assert_ne!(
            token_identity_hash("ab", "c"),
            token_identity_hash("a", "bc")
        );
    }

    #[test]
    fn either_half_blank_is_no_identity() {
        // Two nameless tokens are not "the same token" — they are both unknown.
        assert_eq!(token_identity_hash("", "MDOG"), None);
        assert_eq!(token_identity_hash("Moon Dog", ""), None);
        assert_eq!(token_identity_hash("   ", "\u{200b}"), None);
    }

    #[test]
    fn a_different_symbol_is_a_different_identity() {
        // Same name, different ticker ⇒ not a copycat by this key.
        assert_ne!(
            token_identity_hash("Moon Dog", "MDOG"),
            token_identity_hash("Moon Dog", "MOOND")
        );
    }

    #[test]
    fn hash_always_fits_an_i64_unchanged() {
        // The lake stores this in a Parquet i64 and any BIGINT mirror holds the
        // same digits — so the value must never use the top bit.
        for (n, s) in [("a", "b"), ("Moon Dog", "MDOG"), ("\u{4e2d}\u{6587}", "CN")] {
            let h = token_identity_hash(n, s).unwrap();
            assert!(h <= i64::MAX as u64, "{n}/{s} overflows i64");
            assert_eq!(i64::try_from(h).unwrap() as u64, h);
        }
    }

    /// The hash is persisted (lake column, and compared against live events), so
    /// its value is a contract. Pinning one keeps an "innocent" refactor of the
    /// normalizer or the byte layout from silently invalidating stored data.
    #[test]
    fn hash_value_is_pinned() {
        assert_eq!(token_identity_hash("Moon Dog", "MDOG"), Some(6832945047707646484));
    }
}
