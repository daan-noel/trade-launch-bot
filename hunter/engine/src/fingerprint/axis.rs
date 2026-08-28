//! **The fingerprint axis registry** — one `AxisDef` per matchable creation axis,
//! and the predicate vocabulary every axis is matched with.
//!
//! This table is the extension point. The matcher loop, the criterion guards, the
//! auto-name and its grammar, JSON parsing, validation, the dashboard's SQL mirror,
//! the sweep partition, and the UI form all derive from it — so **adding an axis is
//! one [`AxisDef`] plus one arm in [`AxisId::read_num`] / [`AxisId::read_seq`]**, and
//! adding a predicate shape is one [`AxisPredicate`] variant.
//!
//! Two rules hold the design together:
//!
//! * **Identity is integer.** Every numeric axis is a non-negative integer —
//!   lamports, compute units, tallies — carried as `u128` in memory and as a
//!   **decimal string** on the wire. No `f64` reaches a match, so there is no
//!   boundary epsilon, no rounding to keep two implementations in lockstep, and
//!   no value that stops being representable past 2^53 (`max_sol_cost = u64::MAX`
//!   is real data, not a hypothetical). SOL is a display unit, converted at the UI
//!   edge only.
//! * **Exact is the degenerate range.** `min == max` IS exact match, so a
//!   fingerprint never carries a mode flag two readers can disagree about. Because
//!   lamports are integers, the inclusive `[min, max]` here is exactly as
//!   expressive as a half-open `[lo, hi)` window — `hi` and `max + 1` name the same
//!   set — which is what makes the conversion from the retired bucket widths
//!   lossless.
//! * **A numeric predicate is a set of spans, stored canonically.** `!=` and `|`
//!   are unions and complements of inclusive windows, which over the integers are
//!   just more windows — so they need no new matching rule, only
//!   [`AxisPredicate::Spans`]. Every builder routes through [`SpanSet`], which
//!   sorts, merges and collapses a one-span set back to
//!   [`AxisPredicate::Range`]. One set therefore has exactly ONE stored spelling,
//!   which is what keeps `criteria` usable as identity: `<=2 | >=4` and `!=3`
//!   select the same tokens, so they must be the same fingerprint row.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use super::token::TokenFingerprint;

// ─────────────────────────────────────────────────────────────────────────────
// Axis identity
// ─────────────────────────────────────────────────────────────────────────────

/// One matchable token-creation axis. Serialises to its wire key, so a
/// [`Criteria`] map is a plain JSON object keyed by these names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AxisId {
    /// Compute-unit limit of the creation transaction.
    CuLimit,
    /// Compute-unit price of the creation transaction.
    CuPrice,
    /// Creator's initial dev-buy, in lamports.
    InitBuyLamports,
    /// `max_sol_cost` arg of the initial buy instruction, in lamports.
    MaxCostLamports,
    /// Spendable lamports the creator wallet held going in.
    SpendableLamportsIn,
    /// Buy lamports summed across the creation slot (deferred).
    FirstSlotBuyLamports,
    /// Sell lamports summed across the creation slot (deferred).
    FirstSlotSellLamports,
    /// Exact ordered instruction-label sequence of the creation transaction.
    IxLabels,
    /// How many instructions the creation transaction carried.
    IxCount,
    /// How many tokens this creator launched before this one.
    PriorLaunches,
}

/// The value shape an axis carries — which [`AxisPredicate`] variant is legal on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisKind {
    /// A non-negative integer, matched by [`AxisPredicate::Range`] or
    /// [`AxisPredicate::Spans`].
    Numeric,
    /// An ordered string sequence, matched by [`AxisPredicate::Sequence`].
    Sequence,
}

/// What an axis's numbers *are* — drives how the UI renders and parses a bound.
/// Never read by the matcher: identity is integer regardless of unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AxisUnit {
    /// Integer lamports; the UI shows human SOL and converts at its own edge.
    Lamports,
    /// Compute units, shown as-is.
    ComputeUnits,
    /// A tally, shown as-is.
    Count,
    /// Instruction labels — no numeric bound.
    Labels,
}

/// When an axis's observed value is known.
///
/// The two `first_slot_*` axes are trade-derived: they only settle once the
/// creation slot closes, so a fingerprint carrying one cannot resolve at
/// `TokenCreated`. This flag is the **only** place that fact is recorded — the
/// matcher, the pending-arm bookkeeping and the producer all read it from here,
/// so a new deferred axis needs no change anywhere else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AxisPhase {
    /// Known at token creation.
    Instant,
    /// Known only once the creation slot settles.
    FirstSlot,
}

/// One row of the registry. See the module docs: everything about an axis lives
/// here, so a new axis is one entry and one reader arm.
#[derive(Debug, Clone, Copy)]
pub struct AxisDef {
    pub id: AxisId,
    /// Wire key — the JSONB object key, the query tag, the TS field name.
    pub key: &'static str,
    /// Human label for the form and the summary.
    pub label: &'static str,
    /// Short token used by the auto-name chip (`max=1.5`).
    pub chip: &'static str,
    pub kind: AxisKind,
    pub unit: AxisUnit,
    pub phase: AxisPhase,
    /// **The one definition of this axis**, rendered into the UI from this text.
    /// Changing what an axis measures changes this string in the same edit.
    pub definition: &'static str,
}

/// Every axis, in declaration order. Iteration order of a [`Criteria`] map follows
/// [`AxisId`]'s `Ord`, which follows this order, so every derived rendering
/// (auto-name, summary, SQL) is stable without anyone sorting.
pub static AXES: &[AxisDef] = &[
    AxisDef {
        id: AxisId::CuLimit,
        key: "cu_limit",
        label: "CU limit",
        chip: "cu_limit",
        kind: AxisKind::Numeric,
        unit: AxisUnit::ComputeUnits,
        phase: AxisPhase::Instant,
        definition: "Compute-unit limit requested by the creation transaction. A launch \
                     tool's fixed setting, so it identifies the tool.",
    },
    AxisDef {
        id: AxisId::CuPrice,
        key: "cu_price",
        label: "CU price",
        chip: "cu_price",
        kind: AxisKind::Numeric,
        unit: AxisUnit::ComputeUnits,
        phase: AxisPhase::Instant,
        definition: "Compute-unit price (micro-lamports) paid by the creation transaction \
                     — the launcher's priority-fee setting.",
    },
    AxisDef {
        id: AxisId::InitBuyLamports,
        key: "init_buy_lamports",
        label: "Initial buy",
        chip: "init",
        kind: AxisKind::Numeric,
        unit: AxisUnit::Lamports,
        phase: AxisPhase::Instant,
        definition: "Lamports the creator spent on their own first buy of the token.",
    },
    AxisDef {
        id: AxisId::MaxCostLamports,
        key: "max_cost_lamports",
        label: "Max cost",
        chip: "max",
        kind: AxisKind::Numeric,
        unit: AxisUnit::Lamports,
        phase: AxisPhase::Instant,
        definition: "The `max_sol_cost` slippage ceiling on the creator's initial buy, in \
                     lamports. `u64::MAX` is the \"fill at any price\" sentinel, carried \
                     exactly and matchable as itself.",
    },
    AxisDef {
        id: AxisId::SpendableLamportsIn,
        key: "spendable_lamports_in",
        label: "Spendable in",
        chip: "spend",
        kind: AxisKind::Numeric,
        unit: AxisUnit::Lamports,
        phase: AxisPhase::Instant,
        definition: "Lamports the creator wallet held going into the launch.",
    },
    AxisDef {
        id: AxisId::FirstSlotBuyLamports,
        key: "first_slot_buy_lamports",
        label: "First-slot buy",
        chip: "fs_buy",
        kind: AxisKind::Numeric,
        unit: AxisUnit::Lamports,
        phase: AxisPhase::FirstSlot,
        definition: "Buy lamports summed across every trade landing in the creation slot \
                     — how funded the launch was. Known only once that slot settles.",
    },
    AxisDef {
        id: AxisId::FirstSlotSellLamports,
        key: "first_slot_sell_lamports",
        label: "First-slot sell",
        chip: "fs_sell",
        kind: AxisKind::Numeric,
        unit: AxisUnit::Lamports,
        phase: AxisPhase::FirstSlot,
        definition: "Sell lamports summed across every trade landing in the creation slot. \
                     Known only once that slot settles.",
    },
    AxisDef {
        id: AxisId::IxLabels,
        key: "ix_labels",
        label: "Instruction labels",
        chip: "ix",
        kind: AxisKind::Sequence,
        unit: AxisUnit::Labels,
        phase: AxisPhase::Instant,
        definition: "The creation transaction's instruction labels, matched as an EXACT \
                     ordered sequence — same length, same label at every position.",
    },
    AxisDef {
        id: AxisId::IxCount,
        key: "ix_count",
        label: "Instruction count",
        chip: "ix_count",
        kind: AxisKind::Numeric,
        unit: AxisUnit::Count,
        phase: AxisPhase::Instant,
        definition: "How many instructions the creation transaction carried — launch \
                     tooling as one number, without pinning which instructions they were.",
    },
    AxisDef {
        id: AxisId::PriorLaunches,
        key: "prior_launches",
        label: "Prior launches",
        chip: "prior",
        kind: AxisKind::Numeric,
        unit: AxisUnit::Count,
        phase: AxisPhase::Instant,
        definition: "How many tokens this creator launched BEFORE this one, over a \
                     trailing 30-day window. A strictly-prior tally, so a first-time \
                     creator reads 0; unknown when the creator wallet is not on the \
                     creation event.",
    },
];

impl AxisId {
    /// Every axis, for exhaustive iteration in guards, forms and SQL builders.
    pub const ALL: [AxisId; 10] = [
        AxisId::CuLimit,
        AxisId::CuPrice,
        AxisId::InitBuyLamports,
        AxisId::MaxCostLamports,
        AxisId::SpendableLamportsIn,
        AxisId::FirstSlotBuyLamports,
        AxisId::FirstSlotSellLamports,
        AxisId::IxLabels,
        AxisId::IxCount,
        AxisId::PriorLaunches,
    ];

    /// This axis's registry row.
    pub fn def(self) -> &'static AxisDef {
        // `AXES` is declared in `ALL` order and the guard test below locks that,
        // so this is an index, not a scan — it sits on the match hot path.
        &AXES[self as usize]
    }

    /// Wire key (`"max_cost_lamports"`).
    pub fn key(self) -> &'static str {
        self.def().key
    }

    /// Parse a wire key back to an axis. `None` for anything unregistered — an
    /// unknown key is a client error, never silently dropped (a dropped axis reads
    /// as "not part of identity", which widens the match instead of failing).
    pub fn from_key(key: &str) -> Option<AxisId> {
        AxisId::ALL.into_iter().find(|a| a.key() == key)
    }

    /// Whether this axis's observed value settles only after the creation slot.
    pub fn is_deferred(self) -> bool {
        self.def().phase == AxisPhase::FirstSlot
    }

    /// The observed integer value of a numeric axis, or `None` when the token does
    /// not carry it. **One arm per numeric axis — the only reader a new axis adds.**
    ///
    /// `None` on a configured axis fails the match: an unknown value can never be
    /// shown to satisfy a bound, and failing closed is the only direction that
    /// cannot arm a rule on a token nobody screened.
    pub fn read_num(self, tf: &TokenFingerprint) -> Option<u128> {
        let v = match self {
            AxisId::CuLimit => u128::from(tf.cu_limit?),
            AxisId::CuPrice => u128::from(tf.cu_price?),
            AxisId::InitBuyLamports => u128::from(tf.init_buy_lamports?),
            AxisId::MaxCostLamports => u128::from(tf.max_cost_lamports?),
            AxisId::SpendableLamportsIn => u128::from(tf.spendable_lamports_in?),
            AxisId::FirstSlotBuyLamports => u128::from(tf.first_slot_buy_lamports?),
            AxisId::FirstSlotSellLamports => u128::from(tf.first_slot_sell_lamports?),
            // Derived, not stored: the count IS the label sequence's length, so the
            // two axes can never disagree about the same transaction.
            AxisId::IxCount => tf.ix_labels.len() as u128,
            AxisId::PriorLaunches => u128::from(tf.prior_launches?),
            AxisId::IxLabels => return None,
        };
        Some(v)
    }

    /// The observed label sequence of a sequence axis. An empty sequence is a real
    /// observation (a transaction whose labels are unknown carries none), and it
    /// simply fails any configured axis — a predicate can never be empty, because
    /// [`AxisPredicate::is_satisfiable`] rejects one at the write edge.
    pub fn read_seq(self, tf: &TokenFingerprint) -> Option<&[String]> {
        match self {
            AxisId::IxLabels => Some(&tf.ix_labels),
            _ => None,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Predicates
// ─────────────────────────────────────────────────────────────────────────────

/// How one axis is matched. Externally tagged by `kind` so a new shape (a value
/// set, a prefix, a negation) is additive on the wire — an old reader rejects an
/// unknown kind rather than silently reading it as something else.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AxisPredicate {
    /// Inclusive `[min, max]`; either bound `None` is open. `min == max` is exact.
    ///
    /// Bounds are decimal **strings** on the wire. A JSON number loses precision
    /// past 2^53 and `max_sol_cost = u64::MAX` is a value real launches carry, so
    /// the string form is what keeps a bound meaning one amount on every surface.
    Range {
        #[serde(default, skip_serializing_if = "Option::is_none", with = "u128_wire")]
        min: Option<u128>,
        #[serde(default, skip_serializing_if = "Option::is_none", with = "u128_wire")]
        max: Option<u128>,
    },
    /// Two or more **disjoint, ascending, non-adjacent** inclusive spans — the
    /// shape `!=` and `|` produce (`!=3` is `[…2] ∪ [4…]`).
    ///
    /// Canonical by construction: every builder routes through
    /// [`SpanSet::into_predicate`], which sorts, merges and collapses a one-span
    /// set back to [`AxisPredicate::Range`]. That is what keeps identity intact —
    /// `<=2 | >=4` and `!=3` name one token set, so they must produce one stored
    /// value, or `find_or_create` forks a second row for the same fingerprint.
    Spans { spans: Vec<Span> },
    /// Exact ordered sequence — same length, same string at every position.
    Sequence { labels: Vec<String> },
}

/// One inclusive `[min, max]` interval; either bound `None` is open. The element
/// of a [`AxisPredicate::Spans`] list and the whole of a
/// [`AxisPredicate::Range`], so the two variants are one vocabulary — see
/// [`AxisPredicate::spans`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Span {
    #[serde(default, skip_serializing_if = "Option::is_none", with = "u128_wire")]
    pub min: Option<u128>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "u128_wire")]
    pub max: Option<u128>,
}

impl Span {
    pub fn new(min: Option<u128>, max: Option<u128>) -> Self {
        Span { min, max }
    }

    /// Match one amount and nothing else.
    pub fn exact(v: u128) -> Self {
        Span { min: Some(v), max: Some(v) }
    }

    /// The whole axis domain — the span that configures nothing.
    pub fn all() -> Self {
        Span { min: None, max: None }
    }

    /// Low edge as a number. Identity is a **non-negative** integer, so "unbounded
    /// below" and "from 0" are the same set; this is the one place that fact is
    /// used, and only for ordering and overlap — never to rewrite a stored `None`
    /// into a `Some(0)`, which would fork the identity of every `<=` row already
    /// written.
    fn lo(self) -> u128 {
        self.min.unwrap_or(0)
    }

    /// Whether an observed integer falls in this span.
    pub fn contains(self, v: u128) -> bool {
        self.min.is_none_or(|lo| v >= lo) && self.max.is_none_or(|hi| v <= hi)
    }

    /// Whether any value can fall in this span.
    pub fn is_satisfiable(self) -> bool {
        match (self.min, self.max) {
            (Some(a), Some(b)) => a <= b,
            _ => true,
        }
    }

    /// Whether `other` starts at or before this span's first uncovered value, so
    /// the two are one interval. Adjacency counts: the domain is integer, so
    /// `[0,2]` and `[3,5]` cover exactly `[0,5]` and storing them apart would be a
    /// second spelling of one set.
    fn touches(self, other: Span) -> bool {
        match self.max {
            None => true,
            Some(hi) => other.lo() <= hi.saturating_add(1),
        }
    }
}

impl AxisPredicate {
    /// Match one amount and nothing else.
    pub fn exact(v: u128) -> Self {
        AxisPredicate::Range { min: Some(v), max: Some(v) }
    }

    /// Match the inclusive window `[min, max]`.
    pub fn range(min: Option<u128>, max: Option<u128>) -> Self {
        AxisPredicate::Range { min, max }
    }

    /// Match the **half-open** window `[lo, hi)` — the shape a partition edge pair
    /// and a retired bucket both have. Integer domain, so this is exactly
    /// `[lo, hi - 1]`; `hi <= lo` yields an empty (unsatisfiable) predicate, which
    /// [`Self::is_satisfiable`] then rejects rather than storing a row that matches
    /// nothing.
    pub fn half_open(lo: u128, hi: Option<u128>) -> Self {
        AxisPredicate::Range { min: Some(lo), max: hi.map(|h| h.saturating_sub(1)) }
    }

    /// Match everything **except** the inclusive window `[min, max]` — the `!=`
    /// atom. The complement of one span over the non-negative integers is at most
    /// two spans, so this needs no shape the storage layer does not already have.
    pub fn not_range(min: Option<u128>, max: Option<u128>) -> Self {
        SpanSet::one(Span::new(min, max)).complement().into_predicate()
    }

    /// Which axis kind this predicate can be applied to.
    pub fn kind(&self) -> AxisKind {
        match self {
            AxisPredicate::Range { .. } | AxisPredicate::Spans { .. } => AxisKind::Numeric,
            AxisPredicate::Sequence { .. } => AxisKind::Sequence,
        }
    }

    /// The inclusive spans this predicate accepts, ascending — **the one reader**
    /// of a numeric predicate's shape. A [`AxisPredicate::Range`] is the one-span
    /// case, so every consumer (the SQL mirror, the auto-name, the group key)
    /// writes one loop and gains `!=`/`|` for free.
    ///
    /// Inline for the one- and two-span cases, so reading a predicate this way
    /// never allocates; [`Self::matches_num`] still reads the variant directly and
    /// touches none of this.
    pub fn spans(&self) -> SmallVec<[Span; 2]> {
        match self {
            AxisPredicate::Range { min, max } => {
                SmallVec::from_buf_and_len([Span::new(*min, *max), Span::all()], 1)
            }
            AxisPredicate::Spans { spans } => SmallVec::from_slice(spans),
            AxisPredicate::Sequence { .. } => SmallVec::new(),
        }
    }

    /// The single amount this predicate pins, if it pins one. The one reader of
    /// "is this exact" — nothing else compares `min` against `max`.
    pub fn as_exact(&self) -> Option<u128> {
        match self {
            AxisPredicate::Range { min: Some(a), max: Some(b) } if a == b => Some(*a),
            _ => None,
        }
    }

    /// Whether any value can satisfy this predicate. An unsatisfiable predicate is
    /// rejected at every write edge: stored, it would silently disarm every rule
    /// bound to the fingerprint while the row still reads as configured.
    pub fn is_satisfiable(&self) -> bool {
        match self {
            AxisPredicate::Range { min: Some(a), max: Some(b) } => a <= b,
            AxisPredicate::Range { .. } => true,
            // An empty list is how "no value at all" arrives here — `a > b | c > d`
            // with both arms empty, or a `!=` of an open range.
            AxisPredicate::Spans { spans } => {
                !spans.is_empty() && spans.iter().all(|s| s.is_satisfiable())
            }
            AxisPredicate::Sequence { labels } => !labels.is_empty(),
        }
    }

    /// Every way this predicate is malformed *as stored*, beyond being empty.
    ///
    /// A `Spans` list is only ever produced canonical, so a non-canonical one
    /// reached the row by hand or by a writer that skipped [`SpanSet`]. It has to
    /// be refused rather than normalised on read: two spellings of one token set
    /// key as two fingerprints, which is the duplicate-row class the identity index
    /// exists to make impossible.
    fn shape_problem(&self) -> Option<&'static str> {
        let AxisPredicate::Spans { spans } = self else { return None };
        if spans.len() < 2 {
            return Some(
                "a multi-span predicate needs two or more spans — one span is spelled as a \
                 plain range",
            );
        }
        if spans.windows(2).any(|w| !w[0].is_satisfiable() || w[0].touches(w[1])) {
            return Some(
                "spans must be disjoint and ascending — overlapping or touching spans are two \
                 spellings of one window",
            );
        }
        None
    }

    /// Whether an observed integer satisfies this predicate.
    pub fn matches_num(&self, v: u128) -> bool {
        match self {
            AxisPredicate::Range { min, max } => {
                min.is_none_or(|lo| v >= lo) && max.is_none_or(|hi| v <= hi)
            }
            // Ascending and disjoint, so the first span that starts above `v` ends
            // the search — a `!=` gate costs one comparison more than a range.
            AxisPredicate::Spans { spans } => {
                spans.iter().take_while(|s| s.lo() <= v).any(|s| s.contains(v))
            }
            AxisPredicate::Sequence { .. } => false,
        }
    }

    /// Whether an observed label sequence satisfies this predicate.
    pub fn matches_seq(&self, obs: &[String]) -> bool {
        match self {
            AxisPredicate::Sequence { labels } => labels.len() == obs.len() && labels == obs,
            AxisPredicate::Range { .. } | AxisPredicate::Spans { .. } => false,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Span algebra
// ─────────────────────────────────────────────────────────────────────────────

/// A set of values on one axis, as a **canonical** ascending list of disjoint,
/// non-touching spans. The working form the condition grammar evaluates in: a
/// comma-AND is [`Self::intersect`], a `|` is [`Self::union`], a `!=` is
/// [`Self::complement`].
///
/// Canonical means *one list per set*. That is the whole point — a fingerprint's
/// identity is its stored `criteria` JSON, so if `<=2 | >=4` and `!=3` serialised
/// differently they would key as two fingerprints matching the same tokens, and
/// `find_or_create` would hand two rules two different rows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SpanSet(Vec<Span>);

impl SpanSet {
    /// The empty set — matches nothing. Refused at every write edge.
    pub fn none() -> Self {
        SpanSet(Vec::new())
    }

    /// Every value on the axis.
    pub fn all() -> Self {
        SpanSet(vec![Span::all()])
    }

    /// One span, canonicalised (an unsatisfiable one yields the empty set).
    pub fn one(span: Span) -> Self {
        Self::from_spans([span])
    }

    /// Canonicalise an arbitrary span list: drop the unsatisfiable, sort, merge
    /// everything that overlaps or touches.
    pub fn from_spans(spans: impl IntoIterator<Item = Span>) -> Self {
        let mut v: Vec<Span> = spans.into_iter().filter(|s| s.is_satisfiable()).collect();
        // By low edge, then by high edge with an open end last — an open end
        // absorbs everything after it, so it must not sort before a span it covers.
        v.sort_by(|a, b| a.lo().cmp(&b.lo()).then(cmp_hi(a.max, b.max)));
        let mut out: Vec<Span> = Vec::with_capacity(v.len());
        for s in v {
            match out.last_mut() {
                Some(prev) if prev.touches(s) => {
                    if prev.max.is_some() && (s.max.is_none() || s.max > prev.max) {
                        prev.max = s.max;
                    }
                }
                _ => out.push(s),
            }
        }
        SpanSet(out)
    }

    /// The predicate that stores this set: one span is a plain
    /// [`AxisPredicate::Range`], so the shape already in every row stays the
    /// spelling for everything `!=`/`|` did not widen.
    pub fn into_predicate(self) -> AxisPredicate {
        let mut spans = self.0;
        match spans.len() {
            1 => {
                let s = spans.pop().expect("len checked");
                AxisPredicate::Range { min: s.min, max: s.max }
            }
            _ => AxisPredicate::Spans { spans },
        }
    }

    /// The set a stored predicate accepts. Round-trips with
    /// [`Self::into_predicate`] for anything the write edge accepted.
    pub fn from_predicate(pred: &AxisPredicate) -> Self {
        Self::from_spans(pred.spans())
    }

    pub fn spans(&self) -> &[Span] {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether this set is the whole axis — the state that configures nothing, and
    /// therefore the one a write edge clears rather than stores.
    ///
    /// `>= 0` counts: identity is a non-negative integer, so a floor of zero
    /// excludes nothing. Read here rather than rewritten into the span, because
    /// rewriting a bottom edge would change the stored identity of every row that
    /// already spells one — and a `0 … 0` exact match is a real, different gate.
    pub fn is_all(&self) -> bool {
        matches!(self.0.as_slice(), [s] if s.max.is_none() && s.lo() == 0)
    }

    /// Everything in either set (`|`).
    pub fn union(self, other: SpanSet) -> Self {
        Self::from_spans(self.0.into_iter().chain(other.0))
    }

    /// Everything in both sets (`,`). Pairwise, because both sides are already
    /// disjoint and ascending, so no pair can produce more than one span.
    pub fn intersect(self, other: SpanSet) -> Self {
        let mut out = Vec::new();
        for a in &self.0 {
            for b in &other.0 {
                let min = match (a.min, b.min) {
                    (None, m) | (m, None) => m,
                    (Some(x), Some(y)) => Some(x.max(y)),
                };
                let max = match (a.max, b.max) {
                    (None, m) | (m, None) => m,
                    (Some(x), Some(y)) => Some(x.min(y)),
                };
                let s = Span { min, max };
                if s.is_satisfiable() {
                    out.push(s);
                }
            }
        }
        Self::from_spans(out)
    }

    /// Everything this set excludes (`!`). Over the non-negative integers, so the
    /// gap below the first span is open-ended `None` — the same spelling `<=` has
    /// always stored.
    pub fn complement(self) -> Self {
        let mut out = Vec::new();
        // `None` = "from the bottom of the domain", which is what the first gap
        // starts at; afterwards it is the value one past the previous span.
        let mut cursor: Option<u128> = None;
        for s in &self.0 {
            // Only the first span of a canonical set can be open below, and a `lo`
            // of 0 leaves no room beneath it — both are "no gap here", not a span.
            if let Some(lo) = s.min.filter(|lo| *lo > 0) {
                out.push(Span { min: cursor, max: Some(lo - 1) });
            }
            // An open top, or one at the ceiling of the domain, leaves nothing above.
            match s.max.and_then(|hi| hi.checked_add(1)) {
                None => return Self::from_spans(out),
                Some(next) => cursor = Some(next),
            }
        }
        out.push(Span { min: cursor, max: None });
        Self::from_spans(out)
    }
}

/// Order two high edges with an open end (`None` = unbounded) last.
fn cmp_hi(a: Option<u128>, b: Option<u128>) -> std::cmp::Ordering {
    match (a, b) {
        (None, None) => std::cmp::Ordering::Equal,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (Some(_), None) => std::cmp::Ordering::Less,
        (Some(x), Some(y)) => x.cmp(&y),
    }
}

/// **The one wire encoding for an identity integer**: `Option<u128>` as a decimal
/// string, accepting a JSON number on the way in so hand-written fixtures and small
/// values stay readable. Writing is always a string — one shape out means no
/// consumer has to guess which it will get, and no consumer parsing JSON as `f64`
/// can silently round a ceiling.
///
/// Shared by the axis bounds here and the group-key windows in
/// [`crate::grouping`], so a bound means the same integer on both surfaces.
pub mod u128_wire {
    use serde::{de::Error as _, Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(v: &Option<u128>, s: S) -> Result<S::Ok, S::Error> {
        match v {
            Some(n) => n.to_string().serialize(s),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u128>, D::Error> {
        // Deliberately NOT `#[serde(untagged)]`: serde's untagged buffer cannot
        // represent a `u128` above `u64::MAX`, so it rejects exactly the values this
        // encoding exists to carry. A `serde_json::Value` is the same trap. Matching
        // on the raw JSON text is what keeps the full domain readable.
        let raw = Option::<serde_json::Value>::deserialize(d)?;
        let Some(raw) = raw else { return Ok(None) };
        let text = match &raw {
            serde_json::Value::Null => return Ok(None),
            serde_json::Value::String(s) => s.trim().to_string(),
            serde_json::Value::Number(n) => n.to_string(),
            other => return Err(D::Error::custom(format!("expected a decimal integer, got {other}"))),
        };
        if text.is_empty() {
            return Ok(None);
        }
        text.parse().map(Some).map_err(D::Error::custom)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The criteria map
// ─────────────────────────────────────────────────────────────────────────────

/// A fingerprint's configured axes. An axis **absent** from the map is not part of
/// identity; there is no null-as-unset second spelling, because a `BTreeMap` has
/// only one way to say "not there".
///
/// Ordered by [`AxisId`] (registry order), so every derived rendering is stable
/// without a caller sorting. Serialises as a plain JSON object keyed by wire key —
/// the shape stored in `fingerprints.criteria`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Criteria(BTreeMap<AxisId, AxisPredicate>);

impl Criteria {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set one axis. Returns `self` so a builder reads as one expression.
    pub fn with(mut self, axis: AxisId, pred: AxisPredicate) -> Self {
        self.0.insert(axis, pred);
        self
    }

    pub fn insert(&mut self, axis: AxisId, pred: AxisPredicate) -> Option<AxisPredicate> {
        self.0.insert(axis, pred)
    }

    pub fn remove(&mut self, axis: AxisId) -> Option<AxisPredicate> {
        self.0.remove(&axis)
    }

    pub fn get(&self, axis: AxisId) -> Option<&AxisPredicate> {
        self.0.get(&axis)
    }

    pub fn contains(&self, axis: AxisId) -> bool {
        self.0.contains_key(&axis)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Configured axes in registry order.
    pub fn iter(&self) -> impl Iterator<Item = (AxisId, &AxisPredicate)> {
        self.0.iter().map(|(a, p)| (*a, p))
    }

    /// Configured axis ids in registry order.
    pub fn axes(&self) -> impl Iterator<Item = AxisId> + '_ {
        self.0.keys().copied()
    }

    /// Whether any configured axis defers to the settled creation slot — the one
    /// reader of "must this fingerprint wait before it fully resolves".
    pub fn has_deferred(&self) -> bool {
        self.axes().any(AxisId::is_deferred)
    }

    /// Whether any configured axis resolves at creation.
    pub fn has_instant(&self) -> bool {
        self.axes().any(|a| !a.is_deferred())
    }

    /// Every way this map is malformed, as operator-facing sentences. Empty ⇒ valid.
    /// The write edges (HTTP handler, repo insert/update) and the storage CHECK all
    /// read this one function, so a criterion can't reach the matcher through a
    /// side door.
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (axis, pred) in self.iter() {
            let def = axis.def();
            if pred.kind() != def.kind {
                out.push(format!(
                    "{}: a {:?} axis cannot carry a {:?} predicate",
                    def.key,
                    def.kind,
                    pred.kind()
                ));
                continue;
            }
            if !pred.is_satisfiable() {
                out.push(match pred {
                    AxisPredicate::Range { min, max } => format!(
                        "{}: no value can satisfy [{}, {}] — min must be <= max",
                        def.key,
                        min.map(|v| v.to_string()).unwrap_or_else(|| "-".into()),
                        max.map(|v| v.to_string()).unwrap_or_else(|| "-".into()),
                    ),
                    AxisPredicate::Spans { .. } => {
                        format!("{}: no value can satisfy those spans together", def.key)
                    }
                    AxisPredicate::Sequence { .. } => {
                        format!("{}: an empty label sequence configures nothing", def.key)
                    }
                });
                continue;
            }
            if let Some(why) = pred.shape_problem() {
                out.push(format!("{}: {why}", def.key));
            }
        }
        // `ix_count` is `ix_labels.len()`, so a row carrying both must agree with
        // itself. Left unchecked the contradiction is invisible: the row reads as
        // fully configured and matches nothing, which looks exactly like a cohort
        // that stopped launching.
        if let (Some(AxisPredicate::Sequence { labels }), Some(count)) =
            (self.get(AxisId::IxLabels), self.get(AxisId::IxCount))
        {
            if !count.matches_num(labels.len() as u128) {
                out.push(format!(
                    "ix_count excludes {}, the length of the ix_labels sequence on the same \
                     row — no token can satisfy both",
                    labels.len()
                ));
            }
        }
        out
    }
}

impl FromIterator<(AxisId, AxisPredicate)> for Criteria {
    fn from_iter<T: IntoIterator<Item = (AxisId, AxisPredicate)>>(iter: T) -> Self {
        Criteria(iter.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `def()` indexes `AXES` by discriminant, so the table must stay in `ALL`
    /// order — and every key must be unique, since `from_key` is the wire's only
    /// way back to an axis.
    #[test]
    fn the_registry_is_indexed_by_axis_order_and_keyed_uniquely() {
        assert_eq!(AXES.len(), AxisId::ALL.len());
        for axis in AxisId::ALL {
            assert_eq!(axis.def().id, axis, "AXES is out of order at {axis:?}");
            assert_eq!(AxisId::from_key(axis.key()), Some(axis));
            assert!(!axis.def().definition.is_empty(), "{axis:?} ships unexplained");
        }
        let mut keys: Vec<_> = AxisId::ALL.iter().map(|a| a.key()).collect();
        keys.sort_unstable();
        let before = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), before, "two axes share a wire key");
        assert_eq!(AxisId::from_key("not_an_axis"), None);
    }

    /// Every numeric axis must have a reader, and every sequence axis must not —
    /// a `Numeric` axis whose `read_num` arm was never written is an always-false
    /// gate that still renders as configured.
    #[test]
    fn every_axis_has_a_reader_for_its_own_kind() {
        let tf = TokenFingerprint {
            cu_limit: Some(200_000),
            cu_price: Some(1_000),
            init_buy_lamports: Some(1),
            max_cost_lamports: Some(u64::MAX),
            spendable_lamports_in: Some(2),
            first_slot_buy_lamports: Some(3),
            first_slot_sell_lamports: Some(4),
            ix_labels: vec!["A".into(), "B".into()],
            prior_launches: Some(7),
            ..TokenFingerprint::default()
        };
        for axis in AxisId::ALL {
            match axis.def().kind {
                AxisKind::Numeric => {
                    assert!(axis.read_num(&tf).is_some(), "{axis:?} has no numeric reader");
                    assert!(axis.read_seq(&tf).is_none(), "{axis:?} reads as a sequence too");
                }
                AxisKind::Sequence => {
                    assert!(axis.read_seq(&tf).is_some(), "{axis:?} has no sequence reader");
                    assert!(axis.read_num(&tf).is_none(), "{axis:?} reads as a number too");
                }
            }
        }
        // The ceiling survives the read intact — the whole reason bounds are u128.
        assert_eq!(AxisId::MaxCostLamports.read_num(&tf), Some(u128::from(u64::MAX)));
        // `ix_count` is derived from the labels, never stored beside them.
        assert_eq!(AxisId::IxCount.read_num(&tf), Some(2));
    }

    #[test]
    fn a_range_is_inclusive_and_exact_is_its_degenerate_case() {
        let exact = AxisPredicate::exact(1_515_000_000);
        assert_eq!(exact.as_exact(), Some(1_515_000_000));
        assert!(exact.matches_num(1_515_000_000));
        assert!(!exact.matches_num(1_515_000_001), "exact must be lamport-exact");

        let window = AxisPredicate::range(Some(10), Some(20));
        assert!(window.matches_num(10) && window.matches_num(20), "both bounds are in");
        assert!(!window.matches_num(9) && !window.matches_num(21));
        assert_eq!(window.as_exact(), None);

        // Open bounds.
        assert!(AxisPredicate::range(Some(10), None).matches_num(u128::MAX));
        assert!(AxisPredicate::range(None, Some(10)).matches_num(0));
    }

    /// A half-open `[lo, hi)` window and the inclusive range that stores it must
    /// select the same integers — the property that makes a partition edge pair
    /// and a retired bucket width convert losslessly.
    #[test]
    fn half_open_and_inclusive_name_the_same_integers() {
        let p = AxisPredicate::half_open(1_500_000_000, Some(1_600_000_000));
        assert!(p.matches_num(1_500_000_000), "lower edge is in");
        assert!(p.matches_num(1_599_999_999), "last lamport below the upper edge is in");
        assert!(!p.matches_num(1_600_000_000), "upper edge belongs to the next window");
        assert!(!p.matches_num(1_499_999_999));
        // An open top stays open.
        assert!(AxisPredicate::half_open(5, None).matches_num(u128::MAX));
    }

    #[test]
    fn an_unsatisfiable_predicate_is_named_not_stored() {
        let c = Criteria::new().with(AxisId::CuLimit, AxisPredicate::range(Some(20), Some(10)));
        assert_eq!(c.problems().len(), 1, "{:?}", c.problems());
        let c = Criteria::new()
            .with(AxisId::IxLabels, AxisPredicate::Sequence { labels: vec![] });
        assert_eq!(c.problems().len(), 1, "{:?}", c.problems());
        // A predicate on the wrong kind of axis is refused rather than silently
        // never matching.
        let c = Criteria::new().with(AxisId::CuLimit, AxisPredicate::Sequence { labels: vec!["A".into()] });
        assert_eq!(c.problems().len(), 1, "{:?}", c.problems());
    }

    /// The two label axes describe the same transaction, so a row that sets both
    /// must agree with itself.
    #[test]
    fn ix_count_must_admit_the_ix_labels_length() {
        let labels = AxisPredicate::Sequence { labels: vec!["A".into(), "B".into(), "C".into()] };
        let ok = Criteria::new()
            .with(AxisId::IxLabels, labels.clone())
            .with(AxisId::IxCount, AxisPredicate::range(Some(2), Some(4)));
        assert!(ok.problems().is_empty(), "{:?}", ok.problems());

        let contradictory = Criteria::new()
            .with(AxisId::IxLabels, labels)
            .with(AxisId::IxCount, AxisPredicate::exact(5));
        assert_eq!(contradictory.problems().len(), 1, "{:?}", contradictory.problems());
    }

    /// Bounds round-trip as decimal strings so a `u64::MAX` ceiling survives JSON.
    #[test]
    fn bounds_round_trip_as_decimal_strings() {
        let c = Criteria::new()
            .with(AxisId::MaxCostLamports, AxisPredicate::exact(u128::from(u64::MAX)))
            .with(AxisId::IxLabels, AxisPredicate::Sequence { labels: vec!["A".into()] })
            .with(AxisId::CuLimit, AxisPredicate::range(Some(1), None));
        let json = serde_json::to_value(&c).unwrap();
        assert_eq!(
            json["max_cost_lamports"],
            serde_json::json!({ "kind": "range", "min": "18446744073709551615", "max": "18446744073709551615" }),
        );
        // An open bound is absent, not null — one spelling of "unbounded".
        assert_eq!(json["cu_limit"], serde_json::json!({ "kind": "range", "min": "1" }));
        assert_eq!(serde_json::from_value::<Criteria>(json).unwrap(), c);

        // A JSON number is accepted on the way in, for hand-written fixtures.
        let from_num: Criteria =
            serde_json::from_value(serde_json::json!({ "cu_limit": { "kind": "range", "min": 5, "max": 5 } }))
                .unwrap();
        assert_eq!(from_num.get(AxisId::CuLimit).unwrap().as_exact(), Some(5));

        // An unknown predicate kind is refused, never read as something else.
        assert!(serde_json::from_value::<Criteria>(
            serde_json::json!({ "cu_limit": { "kind": "prefix", "value": "x" } })
        )
        .is_err());
    }

    #[test]
    fn deferral_is_read_off_the_registry() {
        assert!(AxisId::FirstSlotBuyLamports.is_deferred());
        assert!(AxisId::FirstSlotSellLamports.is_deferred());
        for axis in AxisId::ALL {
            if !matches!(axis, AxisId::FirstSlotBuyLamports | AxisId::FirstSlotSellLamports) {
                assert!(!axis.is_deferred(), "{axis:?} unexpectedly defers");
            }
        }
        let c = Criteria::new().with(AxisId::CuLimit, AxisPredicate::exact(1));
        assert!(c.has_instant() && !c.has_deferred());
        let c = c.with(AxisId::FirstSlotBuyLamports, AxisPredicate::exact(1));
        assert!(c.has_instant() && c.has_deferred());
    }
}
