//! When each persisted field starts being written, and where the `ix_labels`
//! vocabulary changes underneath the stored tape.
//!
//! Every column added by a forward-only migration splits `trades` / `tokens` into
//! a before and an after. Most of those splits announce themselves: the column is
//! NULL on the old side, so a reader that wants it gets nothing and knows it.
//! **`ix_labels` is the exception.** Its spelling changed without its type
//! changing, so an old label is a well-formed string that means something else:
//!
//! ```text
//! before   "Unknown (6Vo3245eszAb5wuqEMw8mGdbfRUdKbHhDHP5LcaGuTAB)"
//! after    "Unknown (6Vo3245eszAb5wuqEMw8mGdbfRUdKbHhDHP5LcaGuTAB): CreateCoinAndBuyBondingCurveV3"
//! ```
//!
//! One instruction, two strings, no NULL and no error. A group-by, an `ix_hash`,
//! or an exact-sequence match over a window that spans the break splits one thing
//! in two and reports both halves as real. [`ix_vocabulary_for_window`] is the
//! guard: ask it before reading labels over a time range.
//!
//! The break is narrower than it looks. Labels for programs that already resolve
//! by name (`Pump.Fun: Create_v2`, `System Program: Transfer`, `Compute Budget: *`
//! — the overwhelming majority of the tape) spell identically on both sides. Only
//! a previously-unnamed program or instruction moves. That is what makes the
//! straddle quiet enough to need a guard rather than a comment.
//!
//! **These instants are read off the data, not off a commit date.** A change
//! reaches the tape when the ingest binary carrying it restarts, which is its own
//! event; the labelling rewrite is committed two days before the restart that
//! makes it real. Each constant below records the observation that pins it.
//!
//! Nothing here can be backfilled. `raw_txs` is opt-in with 3-day retention and
//! holds no payload for any of these windows, so an old row can never be
//! re-decoded into the new vocabulary. The boundary is one-way and permanent.

use chrono::{DateTime, NaiveDate, Utc};

/// Builds a UTC instant from its parts. Panics on an impossible date, which is a
/// typo in a constant below and therefore a compile-time-shaped bug — the tests
/// at the bottom of this module evaluate every one of them.
fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32, micro: u32) -> DateTime<Utc> {
    NaiveDate::from_ymd_opt(y, mo, d)
        .and_then(|d| d.and_hms_micro_opt(h, mi, s, micro))
        .expect("tape epoch constant is a real instant")
        .and_utc()
}

/// First `tokens` row carrying `meta->>'uri'`, on `tokens.created_at`.
///
/// A token created before this has an empty `meta` and no off-chain metadata
/// pointer, ever. After it, an absent `uri` is a FACT about the launch (the venue
/// emitted none) and not a gap — the two cases are only separable by this line.
pub fn uri_captured_from() -> DateTime<Utc> {
    utc(2026, 8, 18, 10, 47, 26, 684_563)
}

/// First `trades` row carrying `fee_lamports`, on `trades.block_time`.
pub fn fee_lamports_captured_from() -> DateTime<Utc> {
    utc(2026, 8, 23, 0, 0, 0, 17_453)
}

/// The ingest restart that changes the `ix_labels` vocabulary AND starts writing
/// the fee budget (`cu_limit`, `cu_price`, `tip_lamports`), on `trades.block_time`.
///
/// One deploy, one instant, both facts. The last old-style label lands at
/// `17:48:10.201701Z` and the first new-style one at `17:48:13.387180Z`; no row
/// falls in the 3.2 s between, so this boundary is exact rather than approximate.
///
/// On `tokens.created_at` the same restart shows at `17:47:43Z` (last old) and
/// `18:07:46Z` (first new) — creates are rarer, so the observed gap is wider. This
/// instant bounds both: no token create between them carries a moved label.
pub fn ix_vocabulary_v2_from() -> DateTime<Utc> {
    utc(2026, 8, 30, 17, 48, 13, 387_180)
}

/// First `trades` row carrying `payer_id` / `is_proxied`, on `trades.block_time`.
/// The same restart adds the jsonParsed rebuild arms, so an instruction the RPC
/// view could not re-encode stops rendering `Unknown` from here on.
///
/// Before this, a per-wallet aggregate cannot tell a router's proxy PDA from a
/// trader on the row alone. `wallet_dict.is_proxy` covers history instead: it is
/// derived from the address bytes, so it applies to every row regardless of age.
pub fn payer_captured_from() -> DateTime<Utc> {
    utc(2026, 9, 1, 16, 24, 13, 860_129)
}

/// Which spelling of `ix_labels` a stored row uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IxVocabulary {
    /// `"<program>: <instruction>"` with the instruction half collapsing to
    /// `Unknown`, and an unnameable program rendering as `Unknown (<id>)` with no
    /// instruction half at all.
    V1,
    /// Both halves resolve independently. An unprovable instruction keeps a stable
    /// key (`ix#01`, `ix#af051981a0d8389d`) instead of collapsing, and an
    /// unnameable program still carries one: `Unknown (<id>): ix#c3`.
    V2,
}

/// The vocabulary a row stamped `at` is written in.
pub fn ix_vocabulary_at(at: DateTime<Utc>) -> IxVocabulary {
    if at < ix_vocabulary_v2_from() {
        IxVocabulary::V1
    } else {
        IxVocabulary::V2
    }
}

/// A window that reads `ix_labels` on both sides of the break.
///
/// Carries the boundary so a caller can split the window rather than widen a
/// comparison that cannot hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StraddlesIxBreak {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    pub boundary: DateTime<Utc>,
}

impl std::fmt::Display for StraddlesIxBreak {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "window {} .. {} spans the ix_labels vocabulary break at {}: \
             labels on either side are not comparable, so split the window there",
            self.from, self.to, self.boundary
        )
    }
}

impl std::error::Error for StraddlesIxBreak {}

/// The one vocabulary a half-open window `[from, to)` reads, or the straddle that
/// makes the question unanswerable.
///
/// Call this before grouping, hashing, or exact-matching `ix_labels` over a range.
/// An empty or inverted window is V1/V2 by its own start and never a straddle —
/// it reads no rows, so no comparison can cross the boundary.
pub fn ix_vocabulary_for_window(
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Result<IxVocabulary, StraddlesIxBreak> {
    let boundary = ix_vocabulary_v2_from();
    if from < boundary && to > boundary {
        return Err(StraddlesIxBreak { from, to, boundary });
    }
    Ok(ix_vocabulary_at(from))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// Every constant is a real instant, and they are in the order the tape gains
    /// them. A typo that lands a boundary out of order would silently mis-window
    /// every caller, so the ordering IS the check.
    #[test]
    fn boundaries_are_real_and_ordered() {
        assert!(uri_captured_from() < fee_lamports_captured_from());
        assert!(fee_lamports_captured_from() < ix_vocabulary_v2_from());
        assert!(ix_vocabulary_v2_from() < payer_captured_from());
    }

    /// The boundary is the first NEW row, so it is itself V2 and the instant
    /// before it is V1. An off-by-one here reads 3.2 s of tape in the wrong
    /// vocabulary, which is exactly the failure this module exists to prevent.
    #[test]
    fn boundary_instant_is_the_first_v2_row() {
        let b = ix_vocabulary_v2_from();
        assert_eq!(ix_vocabulary_at(b), IxVocabulary::V2);
        assert_eq!(
            ix_vocabulary_at(b - chrono::Duration::microseconds(1)),
            IxVocabulary::V1
        );
    }

    /// The last old-style and first new-style labels observed on `trades` bracket
    /// the boundary, and no row falls between them.
    #[test]
    fn observed_labels_land_on_their_own_side() {
        let last_v1 = utc(2026, 8, 30, 17, 48, 10, 201_701);
        let first_v2 = utc(2026, 8, 30, 17, 48, 13, 387_180);
        assert_eq!(ix_vocabulary_at(last_v1), IxVocabulary::V1);
        assert_eq!(ix_vocabulary_at(first_v2), IxVocabulary::V2);
    }

    #[test]
    fn a_window_on_one_side_reports_that_side() {
        let aug = Utc.with_ymd_and_hms(2026, 8, 24, 0, 0, 0).unwrap();
        let aug_end = Utc.with_ymd_and_hms(2026, 8, 25, 0, 0, 0).unwrap();
        assert_eq!(ix_vocabulary_for_window(aug, aug_end), Ok(IxVocabulary::V1));

        let sep = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
        let sep_end = Utc.with_ymd_and_hms(2026, 9, 2, 0, 0, 0).unwrap();
        assert_eq!(ix_vocabulary_for_window(sep, sep_end), Ok(IxVocabulary::V2));
    }

    /// The whole point: a window covering the break refuses to answer.
    #[test]
    fn a_window_over_the_break_is_an_error() {
        let from = Utc.with_ymd_and_hms(2026, 8, 29, 0, 0, 0).unwrap();
        let to = Utc.with_ymd_and_hms(2026, 9, 2, 0, 0, 0).unwrap();
        let err = ix_vocabulary_for_window(from, to).unwrap_err();
        assert_eq!(err.boundary, ix_vocabulary_v2_from());
        assert!(err.to_string().contains("split the window"));
    }

    /// A window that ends exactly ON the boundary reads only V1 rows (half-open),
    /// and one that starts there reads only V2. Neither is a straddle.
    #[test]
    fn a_window_touching_the_boundary_does_not_straddle() {
        let b = ix_vocabulary_v2_from();
        let before = Utc.with_ymd_and_hms(2026, 8, 29, 0, 0, 0).unwrap();
        let after = Utc.with_ymd_and_hms(2026, 9, 2, 0, 0, 0).unwrap();
        assert_eq!(ix_vocabulary_for_window(before, b), Ok(IxVocabulary::V1));
        assert_eq!(ix_vocabulary_for_window(b, after), Ok(IxVocabulary::V2));
    }
}
