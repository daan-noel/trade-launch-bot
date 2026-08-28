//! **The fingerprint condition grammar** — the one text ⇄ [`AxisPredicate`]
//! translation, shared by the axis form, the dashboard filter box and every chip
//! that has to be pasted back into either.
//!
//! ```text
//! expr    := arm ( '|' arm )*          OR   — union of the arms
//! arm     := atom ( ',' atom )*        AND  — intersection of the atoms
//! atom    := op? operand
//! op      := '>=' | '<=' | '>' | '<' | '=' | '==' | '!='
//! operand := n | n '..' n | n '-' n | n '–' n
//! ```
//!
//! Every atom denotes a set of values, so the whole expression is set algebra over
//! [`SpanSet`] — and because the domain is the non-negative integers, a union or a
//! complement of windows is just more windows. `!=` and `|` therefore need no
//! matching rule the engine did not already have.
//!
//! Three things this grammar commits to:
//!
//! * **`..` is inclusive, `-` is half-open.** `1..2` is `[1, 2]`; `1-2` is
//!   `[1, 2)`, which is what a group chip spans — so a chip's own text pasted into
//!   a filter box selects exactly that chip's tokens. Two spellings because they
//!   answer two different questions, and the parsed result is always echoed back
//!   in the inclusive form, so which one was typed is never hidden.
//! * **`>` and `<` are exact, not approximate.** The domain is integer, so `>1.5◎`
//!   is `>= 1500000001` lamports — the same set, named in the storage vocabulary.
//! * **Amounts parse as decimal text, never `f64`.** `max_sol_cost = u64::MAX` is
//!   real launch data past 2^53, so a float round-trip maps distinct amounts onto
//!   one. Mirrors the TS `solLabelToLamports` digit for digit.
//!
//! Strict: any malformed fragment fails the whole parse (`None`). A dropped
//! fragment would read as "no constraint", which *widens* a match instead of
//! failing the write.
//!
//! TS mirror: `frontend/src/shared/lib/strategy/fingerprintGrammar.ts`.

use super::axis::{AxisPredicate, AxisUnit, Span, SpanSet};

const LAMPORTS_PER_SOL: u128 = 1_000_000_000;

/// Parse a condition expression in the axis's own display unit.
///
/// `None` on anything malformed **and** on the empty string — an axis with nothing
/// typed is not configured, which is the one spelling of "not part of identity".
pub fn parse_predicate(text: &str, unit: AxisUnit) -> Option<AxisPredicate> {
    let set = parse_span_set(text, unit)?;
    // The full domain constrains nothing. Refusing it here (rather than storing a
    // row that reads as narrowed and matches everything) is the same call
    // `parse_criteria` makes for an all-open range.
    if set.is_empty() || set.is_all() {
        return None;
    }
    Some(set.into_predicate())
}

/// The value set an expression denotes. Public for callers that compose further
/// (the sweep partition, a filter that ANDs several boxes) before storing.
pub fn parse_span_set(text: &str, unit: AxisUnit) -> Option<SpanSet> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    let mut union = SpanSet::none();
    for raw_arm in t.split('|') {
        let arm = raw_arm.trim();
        if arm.is_empty() {
            return None; // an empty OR arm is malformed, never "everything"
        }
        let mut acc = SpanSet::all();
        for raw_atom in arm.split(',') {
            acc = acc.intersect(parse_atom(raw_atom.trim(), unit)?);
        }
        union = union.union(acc);
    }
    Some(union)
}

/// One `op? operand` atom.
fn parse_atom(text: &str, unit: AxisUnit) -> Option<SpanSet> {
    if text.is_empty() {
        return None;
    }
    // Longest operator first: `>=` must not be read as `>` with a `=` operand.
    for op in [">=", "<=", "==", "!=", ">", "<", "="] {
        let Some(rest) = text.strip_prefix(op) else { continue };
        let rest = rest.trim();
        return match op {
            // An inequality bounds one edge, so a range operand has no meaning on
            // it — `>1..2` is a typo, not a wide gate.
            ">=" => Some(SpanSet::one(Span::new(Some(parse_amount(rest, unit)?), None))),
            "<=" => Some(SpanSet::one(Span::new(None, Some(parse_amount(rest, unit)?)))),
            // Integer domain: the strict edge is the next representable value, so
            // this is the same set, spelled the way the row stores it.
            ">" => Some(SpanSet::one(Span::new(
                Some(parse_amount(rest, unit)?.checked_add(1)?),
                None,
            ))),
            "<" => Some(SpanSet::one(Span::new(
                None,
                Some(parse_amount(rest, unit)?.checked_sub(1)?),
            ))),
            "!=" => Some(parse_operand(rest, unit)?.complement()),
            _ => parse_operand(rest, unit),
        };
    }
    parse_operand(text, unit)
}

/// `n`, `n..n` (inclusive) or `n-n` / `n–n` (half-open, the chip form).
fn parse_operand(text: &str, unit: AxisUnit) -> Option<SpanSet> {
    if let Some((lo, hi)) = text.split_once("..") {
        let (lo, hi) = (parse_amount(lo, unit)?, parse_amount(hi, unit)?);
        return Some(SpanSet::one(Span::new(Some(lo), Some(hi))));
    }
    for sep in ['–', '-'] {
        let Some((lo, hi)) = text.split_once(sep) else { continue };
        let (lo, hi) = (lo.trim(), hi.trim());
        if lo.is_empty() || hi.is_empty() {
            continue;
        }
        let (lo, hi) = (parse_amount(lo, unit)?, parse_amount(hi, unit)?);
        // Half-open `[lo, hi)`: an empty window is a typo, not a gate that matches
        // nothing.
        if hi <= lo {
            return None;
        }
        return Some(SpanSet::one(Span::new(Some(lo), Some(hi - 1))));
    }
    Some(SpanSet::one(Span::exact(parse_amount(text, unit)?)))
}

/// One amount in the axis's display unit → the integer identity carries.
fn parse_amount(s: &str, unit: AxisUnit) -> Option<u128> {
    let t = s.trim();
    match unit {
        AxisUnit::Lamports => sol_text_to_lamports(t),
        // A tally or a compute-unit setting is typed as the integer it is.
        AxisUnit::Count | AxisUnit::ComputeUnits => {
            if t.is_empty() || !t.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            t.parse().ok()
        }
        AxisUnit::Labels => None,
    }
}

/// Human SOL → lamports, exactly. Decimal text arithmetic: parsing through `f64`
/// is lossless only below 2^53, and the `max_sol_cost` ceiling is above it — its
/// low digits would vanish, mapping distinct amounts onto one. The TS
/// `solLabelToLamports` is this function.
pub fn sol_text_to_lamports(s: &str) -> Option<u128> {
    let t = s.trim();
    let (whole, frac) = match t.split_once('.') {
        Some((w, f)) => (w, f),
        None => (t, ""),
    };
    if whole.is_empty() || !whole.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if !frac.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut padded = String::with_capacity(9);
    padded.push_str(frac.get(..9).unwrap_or(frac));
    while padded.len() < 9 {
        padded.push('0');
    }
    // Round half-up on the first dropped digit, so a pasted SOL label and the
    // amount it was rendered from agree.
    let round = u128::from(frac.as_bytes().get(9).is_some_and(|b| *b >= b'5'));
    let whole: u128 = whole.parse().ok()?;
    let frac: u128 = padded.parse().ok()?;
    whole.checked_mul(LAMPORTS_PER_SOL)?.checked_add(frac)?.checked_add(round)
}

// ─────────────────────────────────────────────────────────────────────────────
// Rendering
// ─────────────────────────────────────────────────────────────────────────────

/// Canonical text for a predicate — **round-trips through
/// [`parse_predicate`]**, so what a form shows is what it would re-parse.
///
/// Always the inclusive spelling, never the half-open one: `-` exists so a chip
/// can be pasted in, not so a stored window can hide which edge it includes.
pub fn format_predicate(pred: &AxisPredicate, unit: AxisUnit) -> String {
    let AxisPredicate::Sequence { labels } = pred else {
        let spans = pred.spans();
        // A gap set reads as the hole it names, not as the two half-lines around
        // it — `!=3` rather than `<=2 | >=4`. Derived from the set, so it is still
        // one text per set.
        if spans.len() > 1 {
            let holes = SpanSet::from_spans(spans.iter().copied()).complement();
            if let [hole] = holes.spans() {
                if hole.min.is_some() || hole.max.is_some() {
                    return format!("!={}", format_span_body(*hole, unit));
                }
            }
        }
        return spans.iter().map(|s| format_span(*s, unit)).collect::<Vec<_>>().join(" | ");
    };
    labels.join(" | ")
}

/// One span as a standalone atom (`1.5`, `1.5..2`, `>=1.5`, `<=2`).
fn format_span(span: Span, unit: AxisUnit) -> String {
    match (span.min, span.max) {
        (Some(_), Some(_)) | (None, None) => format_span_body(span, unit),
        (Some(a), None) => format!(">={}", render_amount(a, unit)),
        (None, Some(b)) => format!("<={}", render_amount(b, unit)),
    }
}

/// A span's operand text, without any leading operator (`1.5`, `1.5..2`).
fn format_span_body(span: Span, unit: AxisUnit) -> String {
    match (span.min, span.max) {
        (Some(a), Some(b)) if a == b => render_amount(a, unit),
        (Some(a), Some(b)) => format!("{}..{}", render_amount(a, unit), render_amount(b, unit)),
        (Some(a), None) => format!(">={}", render_amount(a, unit)),
        (None, Some(b)) => format!("<={}", render_amount(b, unit)),
        (None, None) => "any".to_string(),
    }
}

/// One amount in the axis's display unit — the inverse of [`parse_amount`].
pub fn render_amount(v: u128, unit: AxisUnit) -> String {
    match unit {
        AxisUnit::Lamports => crate::grouping::sol_label(v),
        _ => v.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOL: u128 = 1_000_000_000;
    const CEILING: u128 = u64::MAX as u128;

    fn p(text: &str) -> Option<AxisPredicate> {
        parse_predicate(text, AxisUnit::Count)
    }

    fn lam(text: &str) -> Option<AxisPredicate> {
        parse_predicate(text, AxisUnit::Lamports)
    }

    #[test]
    fn exact_is_the_degenerate_range() {
        assert_eq!(p("5"), Some(AxisPredicate::exact(5)));
        assert_eq!(p("=5"), Some(AxisPredicate::exact(5)));
        assert_eq!(p("== 5"), Some(AxisPredicate::exact(5)));
        assert_eq!(p("5..5"), Some(AxisPredicate::exact(5)));
    }

    #[test]
    fn inclusive_and_half_open_ranges_are_different_spellings() {
        assert_eq!(p("3..5"), Some(AxisPredicate::range(Some(3), Some(5))));
        // `-` is the chip form: `[3, 5)` is `[3, 4]` on an integer axis.
        assert_eq!(p("3-5"), Some(AxisPredicate::range(Some(3), Some(4))));
        assert_eq!(p("3–5"), Some(AxisPredicate::range(Some(3), Some(4))));
        assert_eq!(p("5-3"), None);
    }

    #[test]
    fn strict_inequalities_land_on_the_next_integer() {
        assert_eq!(p(">3"), Some(AxisPredicate::range(Some(4), None)));
        assert_eq!(p("<3"), Some(AxisPredicate::range(None, Some(2))));
        assert_eq!(p(">=3"), Some(AxisPredicate::range(Some(3), None)));
        assert_eq!(p("<=3"), Some(AxisPredicate::range(None, Some(3))));
        // Nothing is below zero, so `<0` selects nothing and is refused rather
        // than silently stored as a gate that can never fire.
        assert_eq!(p("<0"), None);
    }

    #[test]
    fn and_arms_intersect_and_or_arms_unite() {
        assert_eq!(p(">=1, <=9"), Some(AxisPredicate::range(Some(1), Some(9))));
        assert_eq!(p("1..9"), p(">=1, <=9"));
        // Disjoint AND selects nothing: refused, never stored as a dead gate.
        assert_eq!(p("<=2, >=7"), None);
        assert_eq!(
            p("<=2 | >=7"),
            Some(AxisPredicate::Spans {
                spans: vec![Span::new(None, Some(2)), Span::new(Some(7), None)]
            })
        );
    }

    /// The whole reason spans are canonical: two spellings of one token set must
    /// produce one stored value, or `find_or_create` forks the fingerprint.
    #[test]
    fn one_token_set_has_exactly_one_stored_spelling() {
        assert_eq!(p("!=3"), p("<=2 | >=4"));
        assert_eq!(p("!=3"), p(">=4 | <=2"));
        assert_eq!(p("5..1"), None); // an inverted range selects nothing, so it is refused
        assert_eq!(p("1..3 | 4..6"), p("1..6")); // adjacent spans are one window
        assert_eq!(p("1..5 | 3..9"), p("1..9")); // overlapping too
    }

    #[test]
    fn not_of_zero_has_no_lower_half() {
        assert_eq!(p("!=0"), Some(AxisPredicate::range(Some(1), None)));
        // `<=`, `<` and every complement spell an open bottom edge the same way
        // the storage already does, so `!=` agrees with the operator form.
        assert_eq!(
            p("!=1..3"),
            Some(AxisPredicate::Spans {
                spans: vec![Span::new(None, Some(0)), Span::new(Some(4), None)]
            })
        );
    }

    #[test]
    fn a_full_domain_expression_configures_nothing() {
        assert_eq!(p("<=2 | >=3"), None);
        assert_eq!(p(">=0"), None);
        assert_eq!(p(""), None);
    }

    #[test]
    fn malformed_fragments_fail_the_whole_parse() {
        for bad in ["abc", "1..", "..1", ">", ">=x", "1,", "|1", "1||2", "-1", "1.5"] {
            assert_eq!(p(bad), None, "{bad} parsed");
        }
    }

    #[test]
    fn lamports_parse_as_decimal_text_not_float() {
        assert_eq!(lam("1.5"), Some(AxisPredicate::exact(1_500_000_000)));
        assert_eq!(lam("0.000000001"), Some(AxisPredicate::exact(1)));
        // The ceiling is 18446744073709.551615 SOL — a `f64` round trip loses its
        // low digits and calls it a different amount.
        let ceiling_sol = crate::grouping::sol_label(CEILING);
        assert_eq!(lam(&ceiling_sol), Some(AxisPredicate::exact(CEILING)));
        assert_eq!(lam(">1.5"), Some(AxisPredicate::range(Some(1_500_000_001), None)));
        assert_eq!(lam("1.5-1.6"), Some(AxisPredicate::range(Some(15 * SOL / 10), Some(1_599_999_999))));
    }

    #[test]
    fn canonical_text_round_trips() {
        for (unit, text) in [
            (AxisUnit::Count, "5"),
            (AxisUnit::Count, "3..5"),
            (AxisUnit::Count, ">=3"),
            (AxisUnit::Count, "<=3"),
            (AxisUnit::Count, "!=3"),
            (AxisUnit::Count, "!=3..5"),
            (AxisUnit::Count, "1..2 | 7..8"),
            (AxisUnit::Lamports, "1.5"),
            (AxisUnit::Lamports, "1.5..2"),
            (AxisUnit::Lamports, "!=1.5"),
        ] {
            let pred = parse_predicate(text, unit).unwrap_or_else(|| panic!("{text} did not parse"));
            let rendered = format_predicate(&pred, unit);
            assert_eq!(rendered, text, "canonical text drifted");
            assert_eq!(parse_predicate(&rendered, unit).as_ref(), Some(&pred));
        }
    }
}
