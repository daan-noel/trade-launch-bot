//! What a transaction's sender declared it would pay to be early — the compute
//! budget it requested and the tip it attached.
//!
//! This is the second half of a build's identity. An ix label sequence says what a
//! transaction DOES; the fee budget says what the client that assembled it was
//! configured to spend. A dump shape is often both: the same four instructions AND
//! the same `cu_limit`, because one operator's tool compiles one preset.
//!
//! **Per TRANSACTION, denormalized onto every leg.** A bundle selling four wallets'
//! bags is four trades carrying one budget, exactly like `ix_labels`. As a *filter*
//! that is harmless (all legs match or none). Anything that ever SUMS a fee across
//! trades must collapse on `leg_index == 0` first or it multiplies the money by the
//! leg count.
//!
//! **Three states, not two.** Each field is a reading, absent, or — for the tip — a
//! real zero. `None` means "not captured": the trade predates core migration `0013`,
//! or its source does not carry the column. A pinned criterion must FAIL against
//! `None` rather than pass, both for `>=` and `<=`; see [`FeeSpec`].

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::flow_ix::ix_hash;

/// The fee budget of one transaction, packed.
///
/// Stored as three raw integers plus a presence mask rather than three `Option`s
/// because this rides on every [`CorpusTrade`] of a corpus load: three `Option`s
/// cost 40 bytes to this layout's 24, which is ~400 MB against ~800 MB on a 20M-row
/// read. The accessors hand back `Option`, so every caller still reads three-state.
///
/// The mask is what keeps a real `0` distinct from an absent reading — the
/// distinction the whole tip-coverage design rests on (`Some(0)` = "transfers
/// landed, none went to a known tip account", the meter on how stale the tip-account
/// registry has become).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct FeeKeys {
    /// Micro-lamports per compute unit. Meaningful only against `cu_limit`.
    cu_price: u64,
    /// Lamports landed on a known tip account.
    tip_lamports: u64,
    /// Requested compute units. Caps at 1,400,000 on chain, so `u32` is exact.
    cu_limit: u32,
    /// Which of the three above are real readings.
    present: u8,
}

const HAS_CU_LIMIT: u8 = 1 << 0;
const HAS_CU_PRICE: u8 = 1 << 1;
const HAS_TIP: u8 = 1 << 2;

impl FeeKeys {
    /// Build from three optional readings. `Default` (nothing captured) is the state
    /// every pre-`0013` trade and every source without the columns lands in.
    pub fn new(cu_limit: Option<u32>, cu_price: Option<u64>, tip_lamports: Option<u64>) -> Self {
        let mut present = 0u8;
        if cu_limit.is_some() {
            present |= HAS_CU_LIMIT;
        }
        if cu_price.is_some() {
            present |= HAS_CU_PRICE;
        }
        if tip_lamports.is_some() {
            present |= HAS_TIP;
        }
        Self {
            cu_price: cu_price.unwrap_or(0),
            tip_lamports: tip_lamports.unwrap_or(0),
            cu_limit: cu_limit.unwrap_or(0),
            present,
        }
    }

    pub fn cu_limit(self) -> Option<u32> {
        (self.present & HAS_CU_LIMIT != 0).then_some(self.cu_limit)
    }

    pub fn cu_price(self) -> Option<u64> {
        (self.present & HAS_CU_PRICE != 0).then_some(self.cu_price)
    }

    pub fn tip_lamports(self) -> Option<u64> {
        (self.present & HAS_TIP != 0).then_some(self.tip_lamports)
    }

    /// Whether nothing at all was captured — the state a rule pinning any fee field
    /// can never match.
    pub fn is_empty(self) -> bool {
        self.present == 0
    }

    /// Total lamports the sender bid for priority: the compute rail plus the tip.
    ///
    /// The compute rail is `ceil(cu_limit * cu_price / 1e6)` because `cu_price` is
    /// priced **per compute unit** — which is why the two are never comparable
    /// apart. `300_000 @ 3_333_333` and `100_000 @ 10_000_000` are the same
    /// 0.001 SOL, and a band on `cu_price` alone would separate them.
    ///
    /// `None` when neither rail was captured. A rail that is absent contributes 0
    /// rather than voiding the sum, so a tip-only sender still prices.
    pub fn priority_lamports(self) -> Option<u64> {
        let compute = match (self.cu_limit(), self.cu_price()) {
            (Some(l), Some(p)) => {
                Some((u128::from(l) * u128::from(p)).div_ceil(1_000_000).min(u128::from(u64::MAX))
                    as u64)
            }
            _ => None,
        };
        match (compute, self.tip_lamports()) {
            (None, None) => None,
            (c, t) => Some(c.unwrap_or(0).saturating_add(t.unwrap_or(0))),
        }
    }
}

/// Wire form: three plain optional fields, so an event-log line stays readable and a
/// line written before the fields existed still parses (every key defaults to absent).
#[derive(Serialize, Deserialize)]
struct FeeKeysRepr {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cu_limit: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cu_price: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tip_lamports: Option<u64>,
}

impl From<FeeKeysRepr> for FeeKeys {
    fn from(r: FeeKeysRepr) -> Self {
        Self::new(r.cu_limit, r.cu_price, r.tip_lamports)
    }
}

impl From<FeeKeys> for FeeKeysRepr {
    fn from(f: FeeKeys) -> Self {
        Self { cu_limit: f.cu_limit(), cu_price: f.cu_price(), tip_lamports: f.tip_lamports() }
    }
}

impl Serialize for FeeKeys {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        FeeKeysRepr::from(*self).serialize(s)
    }
}

impl<'de> Deserialize<'de> for FeeKeys {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        FeeKeysRepr::deserialize(d).map(FeeKeys::from)
    }
}

// -- The build list: an ix shape, optionally pinned to a fee budget ----------

/// The most compute units one Solana transaction may request. A `cu_limit` above
/// this is rejected by the chain, so no landed transaction carries one.
pub const MAX_TX_COMPUTE_UNITS: u64 = 1_400_000;

/// A fee criterion on one list entry. Every field is optional and an absent field
/// is a **wildcard**, so [`FeeSpec::wildcard`] matches any budget — which is what
/// lets an ix-only entry and an ix+fee entry sit in one list rather than behind a
/// mode switch.
///
/// Equality, never a band. This list answers "is this the same machine", and a
/// machine emits the pair its client compiled: `cu_limit = 300_000`, every time. A
/// band answers a different question — how urgent is this sender, in money — and
/// its quantity is [`FeeKeys::priority_lamports`], not these fields.
///
/// **Pin only what is a constant.** A client that reads `cu_price` off a fee oracle
/// emits a different value per transaction; pinning that value matches the one
/// transaction it was copied from and then silently never fires again. `cu_limit`
/// is usually a compiled-in preset and pins well; a tip is an auction bid and
/// almost never does.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct FeeSpec {
    pub cu_limit: Option<u32>,
    pub cu_price: Option<u64>,
    pub tip_lamports: Option<u64>,
}

/// One pinned criterion against one reading. An absent criterion accepts any
/// reading; an absent READING is accepted by nothing, which is the rule that keeps
/// "not captured" from satisfying a bound. Every trade older than core migration
/// `0013` is in that state, so a fee-pinned entry matches none of them — by
/// construction rather than by luck.
fn pinned<T: PartialEq>(want: Option<T>, got: Option<T>) -> bool {
    match want {
        None => true,
        Some(w) => got == Some(w),
    }
}

impl FeeSpec {
    /// Matches any budget — the compiled form of a bare label-array list entry.
    pub fn wildcard() -> Self {
        Self::default()
    }

    /// Whether this entry pins nothing, i.e. is ix-only.
    pub fn is_wildcard(&self) -> bool {
        self == &Self::default()
    }

    pub fn matches(&self, fee: FeeKeys) -> bool {
        pinned(self.cu_limit, fee.cu_limit())
            && pinned(self.cu_price, fee.cu_price())
            && pinned(self.tip_lamports, fee.tip_lamports())
    }

    /// Read one object row's fee fields. `None` = a field is present but is not a
    /// non-negative integer of the right width; the caller turns that into its own
    /// message via [`BuildPatterns::validate`].
    fn from_row(obj: &serde_json::Map<String, Value>) -> Option<Self> {
        let field = |k: &str| -> Option<Option<u64>> {
            match obj.get(k) {
                None | Some(Value::Null) => Some(None),
                Some(v) => v.as_u64().map(Some),
            }
        };
        let cu_limit = match field("cu_limit")? {
            None => None,
            Some(v) => Some(u32::try_from(v).ok()?),
        };
        Some(Self { cu_limit, cu_price: field("cu_price")?, tip_lamports: field("tip_lamports")? })
    }
}

/// A list of build identities: ix shapes, each carrying the fee criteria that
/// qualify it.
///
/// Keyed by `ix_hash` because the shape is the one field every entry has, so a
/// lookup is one map probe plus a walk of that shape's criteria — almost always a
/// single wildcard. The fee fields are deliberately **not** folded into the hash:
/// `ix_hash` is stored identity (in `metric_config`, in sweep results, in every
/// derived rule), so hashing a budget into it would fork one shape into one identity
/// per distinct budget AND put every stored hash on the far side of a one-way break.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BuildPatterns {
    by_shape: BTreeMap<u64, Vec<FeeSpec>>,
}

impl BuildPatterns {
    /// Shapes with no fee criterion — what every list was before fee capture.
    pub fn from_hashes(hashes: BTreeSet<u64>) -> Self {
        Self { by_shape: hashes.into_iter().map(|h| (h, vec![FeeSpec::wildcard()])).collect() }
    }

    /// Compile ordered label sequences, all fee-wildcard.
    pub fn from_label_sequences(patterns: &[Vec<String>]) -> Self {
        let mut out = Self::default();
        for p in patterns {
            if !p.is_empty() {
                out.insert(ix_hash(p), FeeSpec::wildcard());
            }
        }
        out
    }

    fn insert(&mut self, hash: u64, spec: FeeSpec) {
        let specs = self.by_shape.entry(hash).or_default();
        // A wildcard subsumes every criterion on its shape: once an ix-only entry is
        // present the shape matches regardless of budget, so keeping the narrower
        // rows would only cost the matcher a walk it can never need.
        if spec.is_wildcard() {
            specs.clear();
            specs.push(spec);
        } else if !specs.iter().any(|s| s.is_wildcard() || *s == spec) {
            specs.push(spec);
        }
    }

    /// Parse an `ix_patterns` array. Rows come in two forms and both are current:
    ///
    /// * `["A","B"]` — a bare label array, fee-wildcard. Every list stored before
    ///   fee capture is entirely this form and compiles to exactly what it always
    ///   did.
    /// * `{"labels":["A","B"],"cu_limit":300000}` — the same shape pinned to a
    ///   budget. Only the fields present are pinned.
    ///
    /// `None` on any shape error, matching the surrounding parsers: an unusable
    /// config leaves the group unconfigured (metrics read `NaN`) rather than
    /// silently compiling to a shorter list.
    pub fn parse(rows: &[Value]) -> Option<Self> {
        let mut out = Self::default();
        for row in rows {
            let (labels, spec) = match row {
                Value::Array(labels) => (labels, FeeSpec::wildcard()),
                Value::Object(obj) => {
                    (obj.get("labels")?.as_array()?, FeeSpec::from_row(obj)?)
                }
                _ => return None,
            };
            let mut seq: Vec<&str> = Vec::with_capacity(labels.len());
            for l in labels {
                seq.push(l.as_str()?);
            }
            if !seq.is_empty() {
                out.insert(ix_hash(&seq), spec);
            }
        }
        Some(out)
    }

    /// Shape errors in an `ix_patterns` array, reported against `key` (e.g.
    /// `m_dump_ix.ix_patterns`) so the message names the field that was edited.
    pub fn validate(rows: &[Value], key: &str) -> Result<(), String> {
        for (i, row) in rows.iter().enumerate() {
            let labels = match row {
                Value::Array(labels) => labels,
                Value::Object(obj) => {
                    let Some(l) = obj.get("labels") else {
                        return Err(format!("{key}[{i}] carries no labels"));
                    };
                    let Some(l) = l.as_array() else {
                        return Err(format!("{key}[{i}].labels must be an array of strings"));
                    };
                    for f in ["cu_limit", "cu_price", "tip_lamports"] {
                        match obj.get(f) {
                            None | Some(Value::Null) => {}
                            Some(v) if v.as_u64().is_some() => {}
                            Some(_) => {
                                return Err(format!(
                                    "{key}[{i}].{f} must be a non-negative integer"
                                ))
                            }
                        }
                    }
                    // The chain rejects a request above the per-transaction ceiling,
                    // so a value beyond it can never match a landed trade — a filter
                    // that is empty by arithmetic rather than by intent.
                    if let Some(v) = obj.get("cu_limit").and_then(Value::as_u64) {
                        if v > MAX_TX_COMPUTE_UNITS {
                            return Err(format!(
                                "{key}[{i}].cu_limit {v} exceeds the {MAX_TX_COMPUTE_UNITS} \
                                 compute units a transaction may request - no landed \
                                 transaction can carry it"
                            ));
                        }
                    }
                    l
                }
                _ => {
                    return Err(format!(
                        "{key}[{i}] must be an array of strings or an object with labels"
                    ))
                }
            };
            for (j, lab) in labels.iter().enumerate() {
                if !lab.is_string() {
                    return Err(format!("{key}[{i}][{j}] must be a string"));
                }
            }
        }
        Ok(())
    }

    /// Whether this trade's build is on the list: its shape is listed AND some
    /// criterion for that shape accepts its budget.
    pub fn matches(&self, ix_hash: Option<u64>, fee: FeeKeys) -> bool {
        let Some(h) = ix_hash else {
            return false;
        };
        self.by_shape.get(&h).is_some_and(|specs| specs.iter().any(|s| s.matches(fee)))
    }

    /// Whether any entry pins a fee field — i.e. whether this list needs the fee
    /// columns to classify the way it is written.
    pub fn pins_fee(&self) -> bool {
        self.by_shape.values().flatten().any(|s| !s.is_wildcard())
    }

    pub fn is_empty(&self) -> bool {
        self.by_shape.is_empty()
    }

    /// Number of distinct ix shapes on the list.
    pub fn len(&self) -> usize {
        self.by_shape.len()
    }
}

/// Warning text when a fingerprint's build lists pin a fee budget.
///
/// The failure mode this exists for is silent by construction: fee capture is
/// forward-only, so a pinned entry matches NOTHING in history recorded before it,
/// and a rule written against one looks exactly like a rule whose cohort went quiet.
/// The same shape unpinned would have fired. Said once, at save time, where the
/// author can still act on it.
pub fn fee_pin_warning(metric_config: &Value) -> Option<String> {
    let pinned: Vec<&str> = [("m_flow_ix", "ix_patterns"), ("m_dump_ix", "ix_patterns")]
        .iter()
        .filter(|(group, field)| {
            metric_config
                .get(group)
                .and_then(|g| g.get(field))
                .and_then(Value::as_array)
                .and_then(|rows| BuildPatterns::parse(rows))
                .is_some_and(|p| p.pins_fee())
        })
        .map(|(group, _)| *group)
        .collect();
    if pinned.is_empty() {
        return None;
    }
    Some(format!(
        "{} pins a fee budget on at least one build - fee capture is forward-only, so \
         those entries match no trade recorded before it. Check the entry against \
         RECENT data, and confirm the pinned field is a preset rather than a value the \
         sending client recomputes per transaction.",
        pinned.join(" and ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_real_zero_is_not_an_absent_reading() {
        let zero = FeeKeys::new(None, None, Some(0));
        assert_eq!(zero.tip_lamports(), Some(0));
        assert!(!zero.is_empty());

        let absent = FeeKeys::new(None, None, None);
        assert_eq!(absent.tip_lamports(), None);
        assert!(absent.is_empty());
    }

    #[test]
    fn each_field_is_independently_present() {
        let f = FeeKeys::new(Some(300_000), None, None);
        assert_eq!(f.cu_limit(), Some(300_000));
        assert_eq!(f.cu_price(), None);
        assert_eq!(f.tip_lamports(), None);
    }

    /// The reason `cu_price` is never read alone. `3_333_333` is the value the live
    /// tape actually carries at a 300k limit: it is `0.001 SOL` divided out, and it
    /// ceils to exactly 1,000,000 where `3_333_334` would ceil one lamport past it.
    #[test]
    fn different_pairs_price_the_same_spend() {
        let spend = |l, p| FeeKeys::new(Some(l), Some(p), None).priority_lamports();
        assert_eq!(spend(300_000, 3_333_333), Some(1_000_000));
        assert_eq!(spend(100_000, 10_000_000), Some(1_000_000));
        assert_eq!(spend(1_000_000, 1_000_000), Some(1_000_000));
    }

    #[test]
    fn both_rails_sum_and_a_missing_rail_is_not_a_void() {
        assert_eq!(
            FeeKeys::new(Some(300_000), Some(3_333_333), Some(2_000_000)).priority_lamports(),
            Some(3_000_000)
        );
        // Tip only — a sender on the Jito rail alone still prices.
        assert_eq!(FeeKeys::new(None, None, Some(50_000)).priority_lamports(), Some(50_000));
        // A half-set compute budget prices no compute rail: one of the pair is not
        // a spend, and guessing the other is how an approximation gets shipped.
        assert_eq!(FeeKeys::new(Some(300_000), None, None).priority_lamports(), None);
        assert_eq!(FeeKeys::new(None, None, None).priority_lamports(), None);
    }

    #[test]
    fn the_wire_form_round_trips_and_an_old_line_reads_absent() {
        let f = FeeKeys::new(Some(300_000), Some(3_333_333), Some(0));
        let json = serde_json::to_string(&f).unwrap();
        assert_eq!(serde_json::from_str::<FeeKeys>(&json).unwrap(), f);
        // A real 0 must survive the round trip, not be skipped as a default.
        assert!(json.contains("tip_lamports"));
        // A line written before the fields existed.
        assert!(serde_json::from_str::<FeeKeys>("{}").unwrap().is_empty());
    }


    // -- the build list ------------------------------------------------------

    use serde_json::json;

    const DUMP: &[&str] = &[
        "Pump.Fun: Sell",
        "System Program: Transfer",
        "Token Program: CloseAccount",
        "ComputeBudget: SetComputeUnitPrice",
    ];

    fn shape() -> u64 {
        ix_hash(DUMP)
    }

    fn parse(rows: serde_json::Value) -> BuildPatterns {
        BuildPatterns::parse(rows.as_array().unwrap()).expect("parses")
    }

    /// The whole point of per-entry optional fields: one list holds both kinds.
    #[test]
    fn an_ix_only_entry_and_a_pinned_entry_live_in_one_list() {
        let p = parse(json!([
            ["Pump.Fun: Buy"],
            { "labels": DUMP, "cu_limit": 300_000, "cu_price": 3_333_333 },
        ]));
        assert_eq!(p.len(), 2);
        assert!(p.pins_fee());

        // The ix-only entry takes any budget, including none at all.
        let buy = Some(ix_hash(&["Pump.Fun: Buy"]));
        assert!(p.matches(buy, FeeKeys::default()));
        assert!(p.matches(buy, FeeKeys::new(Some(1), Some(2), Some(3))));

        // The pinned entry takes exactly its pair.
        assert!(p.matches(Some(shape()), FeeKeys::new(Some(300_000), Some(3_333_333), None)));
        assert!(!p.matches(Some(shape()), FeeKeys::new(Some(300_000), Some(3_333_334), None)));
        assert!(!p.matches(Some(shape()), FeeKeys::new(Some(200_000), Some(3_333_333), None)));
    }

    /// Every list written before fee capture is bare label arrays, and must compile
    /// to exactly what it always did.
    #[test]
    fn a_legacy_list_still_matches_every_budget() {
        let p = parse(json!([DUMP]));
        assert!(!p.pins_fee());
        for fee in [
            FeeKeys::default(),
            FeeKeys::new(Some(300_000), Some(3_333_333), Some(0)),
            FeeKeys::new(Some(1_400_000), Some(999), Some(2_000_000)),
        ] {
            assert!(p.matches(Some(shape()), fee));
        }
    }

    /// The rule that keeps "not captured" from satisfying a criterion. Every trade
    /// older than core migration `0013` is in this state.
    #[test]
    fn an_absent_reading_matches_no_pinned_entry() {
        let p = parse(json!([{ "labels": DUMP, "cu_limit": 300_000 }]));
        assert!(!p.matches(Some(shape()), FeeKeys::default()));
        // Present, but a different field — the pinned one is still absent.
        assert!(!p.matches(Some(shape()), FeeKeys::new(None, Some(3_333_333), Some(0))));
        assert!(p.matches(Some(shape()), FeeKeys::new(Some(300_000), None, None)));
    }

    /// A pin on one field says nothing about the others.
    #[test]
    fn only_the_named_fields_are_pinned() {
        let p = parse(json!([{ "labels": DUMP, "cu_limit": 300_000 }]));
        assert!(p.matches(Some(shape()), FeeKeys::new(Some(300_000), Some(1), Some(2))));
        assert!(p.matches(Some(shape()), FeeKeys::new(Some(300_000), Some(999), None)));
    }

    /// A real `0` is a value like any other, and pinning it must not be read as
    /// "field absent" — the wildcard and the zero pin are different filters.
    #[test]
    fn a_pinned_zero_is_a_value_not_a_wildcard() {
        let p = parse(json!([{ "labels": DUMP, "tip_lamports": 0 }]));
        assert!(p.pins_fee());
        assert!(p.matches(Some(shape()), FeeKeys::new(None, None, Some(0))));
        assert!(!p.matches(Some(shape()), FeeKeys::new(None, None, Some(1))));
        assert!(!p.matches(Some(shape()), FeeKeys::new(None, None, None)));
    }

    /// One shape, several budgets — the form a preset MENU takes.
    #[test]
    fn one_shape_can_carry_several_budgets() {
        let p = parse(json!([
            { "labels": DUMP, "cu_limit": 300_000 },
            { "labels": DUMP, "cu_limit": 200_000 },
        ]));
        assert_eq!(p.len(), 1, "one shape, however many criteria");
        assert!(p.matches(Some(shape()), FeeKeys::new(Some(300_000), None, None)));
        assert!(p.matches(Some(shape()), FeeKeys::new(Some(200_000), None, None)));
        assert!(!p.matches(Some(shape()), FeeKeys::new(Some(100_000), None, None)));
    }

    /// An ix-only entry on a shape subsumes every criterion on it: the shape already
    /// matches regardless of budget, so keeping the narrower rows would only cost the
    /// matcher a walk it can never need.
    #[test]
    fn a_wildcard_entry_subsumes_the_pinned_ones_on_its_shape() {
        for rows in [
            json!([{ "labels": DUMP, "cu_limit": 300_000 }, DUMP]),
            json!([DUMP, { "labels": DUMP, "cu_limit": 300_000 }]),
        ] {
            let p = parse(rows);
            assert!(!p.pins_fee());
            assert!(p.matches(Some(shape()), FeeKeys::new(Some(999), None, None)));
            assert!(p.matches(Some(shape()), FeeKeys::default()));
        }
    }

    #[test]
    fn a_trade_with_no_labels_matches_nothing() {
        let p = parse(json!([DUMP]));
        assert!(!p.matches(None, FeeKeys::new(Some(300_000), Some(3_333_333), None)));
    }

    #[test]
    fn a_pinned_list_is_empty_against_history_that_predates_capture() {
        let p = parse(json!([{ "labels": DUMP, "cu_limit": 300_000, "cu_price": 3_333_333 }]));
        // Exactly what every pre-`0013` row decodes to.
        assert!(!p.matches(Some(shape()), FeeKeys::default()));
    }

    // -- config shape --------------------------------------------------------

    #[test]
    fn a_row_that_is_neither_a_label_array_nor_an_object_is_rejected() {
        assert!(BuildPatterns::parse(&[json!("Pump.Fun: Sell")]).is_none());
        assert!(BuildPatterns::parse(&[json!({ "cu_limit": 1 })]).is_none());
        assert!(BuildPatterns::parse(&[json!({ "labels": DUMP, "cu_limit": "300000" })]).is_none());
        assert!(BuildPatterns::parse(&[json!({ "labels": DUMP, "cu_limit": -1 })]).is_none());
    }

    /// A `cu_limit` wider than the chain's own field is a corrupt number, not a big
    /// budget, and must not truncate into a value that matches something.
    #[test]
    fn a_cu_limit_past_the_field_width_is_rejected_rather_than_truncated() {
        assert!(BuildPatterns::parse(&[json!({ "labels": DUMP, "cu_limit": 4_294_967_296u64 })])
            .is_none());
    }

    #[test]
    fn validate_names_the_field_that_is_wrong() {
        let ok = json!([DUMP, { "labels": DUMP, "cu_price": 1 }]);
        assert!(BuildPatterns::validate(ok.as_array().unwrap(), "k").is_ok());

        let cases = [
            (json!([{ "cu_limit": 1 }]), "carries no labels"),
            (json!([{ "labels": DUMP, "cu_price": "x" }]), "cu_price"),
            (json!([["a", 7]]), "must be a string"),
            (json!(["a"]), "must be an array of strings or an object"),
        ];
        for (rows, want) in cases {
            let err = BuildPatterns::validate(rows.as_array().unwrap(), "k").unwrap_err();
            assert!(err.contains(want), "{err} should mention {want}");
        }
    }

    /// A budget the chain would reject can never match a landed trade — a filter that
    /// is empty by arithmetic rather than by intent, which is worth saying out loud
    /// at save time rather than discovering as a rule that never fires.
    #[test]
    fn a_cu_limit_above_the_chain_ceiling_is_a_config_error() {
        let rows = json!([{ "labels": DUMP, "cu_limit": 3_000_000 }]);
        let err = BuildPatterns::validate(rows.as_array().unwrap(), "k").unwrap_err();
        assert!(err.contains("1400000"), "{err}");

        let ok = json!([{ "labels": DUMP, "cu_limit": MAX_TX_COMPUTE_UNITS }]);
        assert!(BuildPatterns::validate(ok.as_array().unwrap(), "k").is_ok());
    }

    #[test]
    fn an_empty_label_list_contributes_no_entry() {
        assert!(parse(json!([[], { "labels": [] , "cu_limit": 1 }])).is_empty());
    }

    /// `null` reads as absent, so a UI that emits every key can leave the ones the
    /// user did not fill in.
    #[test]
    fn a_null_fee_field_is_a_wildcard_not_a_value() {
        let p = parse(json!([{ "labels": DUMP, "cu_limit": null, "cu_price": 7 }]));
        assert!(p.matches(Some(shape()), FeeKeys::new(Some(123), Some(7), None)));
        assert!(!p.matches(Some(shape()), FeeKeys::new(Some(123), Some(8), None)));
    }


    #[test]
    fn a_pinned_list_warns_and_an_unpinned_one_does_not() {
        let pinned = json!({"m_dump_ix": {"ix_patterns": [
            {"labels": DUMP, "cu_limit": 300_000}
        ]}});
        let warning = fee_pin_warning(&pinned).expect("pinned config warns");
        assert!(warning.contains("m_dump_ix"), "{warning}");
        assert!(warning.contains("forward-only"), "{warning}");

        // The same builds without a pin have nothing to warn about.
        assert!(fee_pin_warning(&json!({"m_dump_ix": {"ix_patterns": [DUMP]}})).is_none());
        assert!(fee_pin_warning(&json!({})).is_none());
    }

    #[test]
    fn the_warning_names_every_pinned_list() {
        let both = json!({
            "m_flow_ix": {"ix_patterns": [{"labels": ["Pump.Fun: Buy"], "cu_price": 1}]},
            "m_dump_ix": {"ix_patterns": [{"labels": DUMP, "cu_limit": 300_000}]},
        });
        let warning = fee_pin_warning(&both).unwrap();
        assert!(warning.contains("m_flow_ix") && warning.contains("m_dump_ix"), "{warning}");
    }

    #[test]
    fn the_packed_layout_stays_smaller_than_three_options() {
        // The whole reason for the presence mask — see the struct doc.
        assert!(std::mem::size_of::<FeeKeys>() <= 24);
    }
}
