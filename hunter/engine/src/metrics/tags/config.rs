//! A fingerprint's `tags` document: each tag's name and how a trade qualifies.
//!
//! ```json
//! {
//!   "volume": {
//!     "match": {
//!       "program":     ["Unknown (9ddjzq...)"],
//!       "ix_shape":    [["A","B"], {"labels": ["A","C"], "cu_price": 1000}],
//!       "ix_template": ["Axiom Trade|CU|ATA|1|0|0"],
//!       "ix_contains": ["Axiom Trade"],
//!       "ix_lacks":    ["Photon"],
//!       "wallet":      ["7xk..."],
//!       "creator":     true,
//!       "cluster":     {"min_prints": 3, "sol_tol_pct": 10}
//!     },
//!     "side": "sell",
//!     "sticky": true,
//!     "exclude_creation_slot": true
//!   }
//! }
//! ```
//!
//! A trade carries the tag when ANY `match` entry holds (on the tag's `side` only).
//! Validated in full on save ([`validate_tags`]) and compiled once per rules reload
//! ([`compile_tags`]), never per event.

use serde_json::{json, Map, Value};

use super::{check_name, TagKey, BUILTIN};
use crate::hash::HashedSet;
use crate::metrics::burst_slot::TemplatePatterns;
use crate::metrics::fee::BuildPatterns;
use crate::metrics::template_grain::{grain_id_hash, program_id_hash};
use crate::metrics::trade_keys::{marker_mask, wallet_hash, MARKERS, ROUTER_MARKERS};
use crate::metrics::Side;

/// The volume-cluster matcher: a trade qualifies once it is the `min_prints`-th trade
/// of its slot carrying the same ix shape, side, compute-unit limit, compute-unit price
/// and tip, with SOL within `sol_tol_pct` percent of the group's FIRST trade.
///
/// Many identical transactions landing at once is one machine making volume; one person
/// buying is not. Read as trades land: the first `min_prints - 1` members stay
/// untagged, because nothing has shown them to be a cluster yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cluster {
    pub min_prints: u32,
    pub sol_tol_pct: u32,
}

/// One tag, compiled.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TagPatterns {
    pub builds: BuildPatterns,
    pub programs: HashedSet,
    pub templates: HashedSet,
    /// `ix_contains`: a trade whose marker bits intersect this qualifies.
    pub contains: u16,
    /// `ix_lacks`: a trade whose marker bits miss ALL of these qualifies.
    pub lacks: u16,
    pub wallets: HashedSet,
    pub creator: bool,
    pub cluster: Option<Cluster>,
    /// Only trades on this side can carry the tag.
    pub side: Option<Side>,
    pub sticky: bool,
    pub exclude_creation_slot: bool,
}

impl TagPatterns {
    /// The ix-template view of this tag — what the slot and wave families read. Only
    /// `ix_template` and `program` mean anything at that level; `None` when the tag
    /// has neither.
    pub fn templates(&self) -> Option<TemplatePatterns> {
        (!self.templates.is_empty() || !self.programs.is_empty())
            .then(|| TemplatePatterns::new(self.templates.clone()).with_programs(self.programs.clone()))
    }

    /// Whether a marker rule qualifies these bits.
    pub fn marks(&self, bits: u16) -> bool {
        (self.contains != 0 && bits & self.contains != 0) || (self.lacks != 0 && bits & self.lacks == 0)
    }
}

/// A compiled tag with its name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledTag {
    pub key: TagKey,
    pub name: &'static str,
    pub patterns: TagPatterns,
}

// ── The documented vocabulary ────────────────────────────────────────────────

/// One `match` entry kind or tag option, for the registry document and the editor.
pub struct TagFieldSpec {
    pub key: &'static str,
    /// `"match"` or `"option"`.
    pub kind: &'static str,
    /// Value shape the editor renders: `program[]`, `ix_shape[]`, `ix_template[]`,
    /// `marker[]`, `wallet[]`, `bool`, `cluster`, `side`.
    pub value_type: &'static str,
    pub title: &'static str,
    pub summary: &'static str,
    pub example: &'static str,
}

pub const TAG_FIELDS: &[TagFieldSpec] = &[
    TagFieldSpec { key: "program", kind: "match", value_type: "program[]", title: "Program",
        summary: "The transaction's main program (its first instruction past compute budget, system, token, ATA and memo) is one of these. Catches every build a tool ships.",
        example: "\"program\": [\"Axiom Trade\"] : every Axiom trade, whatever its instruction list." },
    TagFieldSpec { key: "ix_shape", kind: "match", value_type: "ix_shape[]", title: "Exact ix shape",
        summary: "The transaction's exact ordered instruction list is one of these, optionally pinned to its fee preset (CU limit, CU price, tip).",
        example: "[\"Compute Budget: SetComputeUnitLimit\", \"Pump.Fun: Buy\"] with cu_price 1000." },
    TagFieldSpec { key: "ix_template", kind: "match", value_type: "ix_template[]", title: "Ix template",
        summary: "The transaction's coarse template (program|CU|ATA|N|S|F) is one of these. The only matcher, with program, the slot and wave families can read.",
        example: "\"Axiom Trade|CU|ATA|1|0|0\"." },
    TagFieldSpec { key: "ix_contains", kind: "match", value_type: "marker[]", title: "Contains marker",
        summary: "The instruction list contains one of these markers (a mechanism or a retail router).",
        example: "[\"CreateAccountWithSeed\"] : throwaway-account machines." },
    TagFieldSpec { key: "ix_lacks", kind: "match", value_type: "marker[]", title: "Lacks marker",
        summary: "The instruction list contains NONE of these markers: everything except them.",
        example: "[\"Axiom Trade\", \"Photon\"] : every trade that did not come through those routers." },
    TagFieldSpec { key: "wallet", kind: "match", value_type: "wallet[]", title: "Wallet",
        summary: "The wallet the venue credited is one of these addresses.",
        example: "[\"7xKX...\"] : a wallet to copy." },
    TagFieldSpec { key: "creator", kind: "match", value_type: "bool", title: "Creator",
        summary: "The trader is the coin's creator.",
        example: "true : the dev's own buys and sells." },
    TagFieldSpec { key: "cluster", kind: "match", value_type: "cluster", title: "Same-slot cluster",
        summary: "The trade is the Nth or later print in one slot with the same ix shape, side and fees, its SOL within P % of the first. Checked last.",
        example: "{\"min_prints\": 3, \"sol_tol_pct\": 10} : the 3rd of three matching 0.5 SOL buys in one slot." },
    TagFieldSpec { key: "side", kind: "option", value_type: "side", title: "Side",
        summary: "Only buys, or only sells, can carry the tag. Absent = both.",
        example: "\"sell\" : a dump list judges sells only." },
    TagFieldSpec { key: "sticky", kind: "option", value_type: "bool", title: "Sticky",
        summary: "A wallet that carried the tag once carries it for the rest of the coin.",
        example: "true : a wallet caught in a volume cluster stays volume for its later trades." },
    TagFieldSpec { key: "exclude_creation_slot", kind: "option", value_type: "bool", title: "Ignore creation-slot buyers",
        summary: "A wallet that buys in the creation slot and matches nothing counts on NEITHER side for the rest of the coin: it is the dev's birth bundle or a sniper, not the audience.",
        example: "true : @!volume then means outside buyers only." },
];

/// The tag vocabulary for the registry document.
pub fn tags_json() -> Value {
    let fields: Vec<Value> = TAG_FIELDS
        .iter()
        .map(|f| json!({
            "key": f.key, "kind": f.kind, "value_type": f.value_type,
            "title": f.title, "summary": f.summary, "example": f.example,
        }))
        .collect();
    let markers: Vec<Value> = MARKERS
        .iter()
        .map(|&(name, bit)| json!({ "name": name, "router": bit & ROUTER_MARKERS != 0 }))
        .collect();
    json!({
        "summary": "A tag is a named trade list. It splits every coin's trades in two: @name = the trades that carry it, @!name = the rest.",
        "example": "volume = program 9ddjzq or a 3-print cluster or the creator, sticky. Then m_flow.buy_sol @!volume = what outsiders bought.",
        "fields": fields,
        "markers": markers,
        "builtin": [
            { "name": super::BUNDLED, "summary": "Wallets whose first buy landed in a slot where 3+ wallets first bought with one ix shape." },
            { "name": super::PUBLIC_APP, "summary": "Wallets whose first buy went through an app with 100+ buyers the day before." },
        ],
    })
}

// ── Parse ────────────────────────────────────────────────────────────────────

fn strings<'a>(v: &'a Value, at: &str) -> Result<Vec<&'a str>, String> {
    let arr = v.as_array().ok_or_else(|| format!("{at} must be an array of strings"))?;
    let mut out = Vec::with_capacity(arr.len());
    for (i, s) in arr.iter().enumerate() {
        let s = s.as_str().ok_or_else(|| format!("{at}[{i}] must be a string"))?;
        if s.trim().is_empty() {
            return Err(format!("{at}[{i}] is blank"));
        }
        out.push(s);
    }
    Ok(out)
}

fn parse_cluster(v: &Value, at: &str) -> Result<Cluster, String> {
    let obj = v.as_object().ok_or_else(|| format!("{at} must be {{min_prints, sol_tol_pct}}"))?;
    let int = |k: &str| -> Result<u32, String> {
        obj.get(k)
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| format!("{at}.{k} must be a non-negative integer"))
    };
    let c = Cluster { min_prints: int("min_prints")?, sol_tol_pct: int("sol_tol_pct")? };
    if c.min_prints < 2 {
        return Err(format!("{at}.min_prints must be at least 2 (one trade is not a cluster)"));
    }
    if c.sol_tol_pct > 100 {
        return Err(format!("{at}.sol_tol_pct must be at most 100"));
    }
    if let Some(k) = obj.keys().find(|k| !matches!(k.as_str(), "min_prints" | "sol_tol_pct")) {
        return Err(format!("{at}: unknown key `{k}`"));
    }
    Ok(c)
}

fn parse_bool(v: &Value, at: &str) -> Result<bool, String> {
    v.as_bool().ok_or_else(|| format!("{at} must be true or false"))
}

/// Compile one tag definition, with every error named by its path.
fn parse_tag(name: &str, def: &Value) -> Result<TagPatterns, String> {
    let at = format!("tags.{name}");
    let obj = def.as_object().ok_or_else(|| format!("{at} must be an object"))?;
    let mut p = TagPatterns::default();
    for (k, v) in obj {
        let here = format!("{at}.{k}");
        match k.as_str() {
            "match" => {}
            "side" => {
                p.side = Some(match v.as_str() {
                    Some("buy") => Side::Buy,
                    Some("sell") => Side::Sell,
                    _ => return Err(format!("{here} must be \"buy\" or \"sell\"")),
                })
            }
            "sticky" => p.sticky = parse_bool(v, &here)?,
            "exclude_creation_slot" => p.exclude_creation_slot = parse_bool(v, &here)?,
            other => return Err(format!("{at}: unknown key `{other}` (expected match, side, sticky, exclude_creation_slot)")),
        }
    }
    let m = obj
        .get("match")
        .and_then(Value::as_object)
        .ok_or_else(|| format!("{at}.match must be an object naming at least one matcher"))?;
    if m.is_empty() {
        return Err(format!("{at}.match names no matcher, so no trade could carry the tag"));
    }
    for (k, v) in m {
        let here = format!("{at}.match.{k}");
        match k.as_str() {
            "program" => {
                for s in strings(v, &here)? {
                    p.programs.insert(program_id_hash(s));
                }
            }
            "ix_shape" => {
                let rows = v.as_array().ok_or_else(|| format!("{here} must be an array"))?;
                BuildPatterns::validate(rows, &here)?;
                p.builds = BuildPatterns::parse(rows).ok_or_else(|| format!("{here} is malformed"))?;
            }
            "ix_template" => {
                for s in strings(v, &here)? {
                    if !s.contains('|') {
                        return Err(format!("{here}: `{s}` is not a template (program|CU|ATA|N|S|F); a bare program name goes under `program`"));
                    }
                    p.templates.insert(grain_id_hash(s));
                }
            }
            "ix_contains" => p.contains = marker_mask(&strings(v, &here)?).map_err(|e| format!("{here}: {e}"))?,
            "ix_lacks" => p.lacks = marker_mask(&strings(v, &here)?).map_err(|e| format!("{here}: {e}"))?,
            "wallet" => {
                for s in strings(v, &here)? {
                    p.wallets.insert(wallet_hash(s.trim()));
                }
            }
            "creator" => p.creator = parse_bool(v, &here)?,
            "cluster" => p.cluster = Some(parse_cluster(v, &here)?),
            other => {
                return Err(format!(
                    "{at}.match: unknown matcher `{other}` (known: program, ix_shape, ix_template, ix_contains, ix_lacks, wallet, creator, cluster)"
                ))
            }
        }
    }
    Ok(p)
}

fn tag_map(doc: &Value) -> Result<Option<&Map<String, Value>>, String> {
    match doc {
        Value::Null => Ok(None),
        Value::Object(m) => Ok(Some(m)),
        _ => Err("tags must be an object {name: definition}".into()),
    }
}

/// Every error in a fingerprint's `tags` document. `null` / `{}` = no tags.
pub fn validate_tags(doc: &Value) -> Result<(), String> {
    let Some(map) = tag_map(doc)? else { return Ok(()) };
    for (name, def) in map {
        check_name(name)?;
        if BUILTIN.contains(&name.as_str()) {
            return Err(format!("`{name}` is a built-in wallet class; pick another tag name"));
        }
        parse_tag(name, def)?;
    }
    Ok(())
}

/// Compile a validated `tags` document. A tag that fails to parse is left out (its
/// metrics then read `NaN`, which satisfies nothing): `validate_tags` refuses such a
/// document at save, so this only guards a row written around the validator.
pub fn compile_tags(doc: &Value) -> Vec<CompiledTag> {
    let Ok(Some(map)) = tag_map(doc) else { return Vec::new() };
    map.iter()
        .filter(|(name, _)| check_name(name).is_ok() && !BUILTIN.contains(&name.as_str()))
        .filter_map(|(name, def)| {
            parse_tag(name, def).ok().map(|patterns| CompiledTag {
                key: TagKey::of(name),
                name: crate::intern::intern(name),
                patterns,
            })
        })
        .collect()
}

/// The tag names a document defines.
pub fn tag_names(doc: &Value) -> Vec<String> {
    match doc {
        Value::Object(m) => m.keys().cloned().collect(),
        _ => Vec::new(),
    }
}

/// Warning text when a rule reads a tag its fingerprint does not define (every such read
/// is `NaN`, so the condition never holds), or reads a tag at the ix-template level
/// (`m_slot`, `m_wave`, `m_crowd.unique_ix_templates`) that has no `ix_template` or
/// `program` matcher. Said at save time, where the author can still act on it.
pub fn rule_tag_warning(params: &crate::rule_params::RuleParams, doc: &Value) -> Option<String> {
    use crate::metrics::registry::TagLevel;
    let defined = compile_tags(doc);
    let mut missing: Vec<&str> = Vec::new();
    let mut not_template: Vec<&str> = Vec::new();
    for r in params.metric_refs() {
        let Some(t) = r.tag.filter(|t| !t.is_builtin()) else { continue };
        match defined.iter().find(|d| d.key == t.key) {
            None if !missing.contains(&t.name) => missing.push(t.name),
            Some(d)
                if r.metric.spec().tag_level == TagLevel::Template
                    && d.patterns.templates().is_none()
                    && !not_template.contains(&t.name) =>
            {
                not_template.push(t.name);
            }
            _ => {}
        }
    }
    let mut parts = Vec::new();
    if !missing.is_empty() {
        parts.push(format!(
            "the rule reads tag {} but its fingerprint does not define it - those conditions read nothing and never hold",
            missing.join(", ")
        ));
    }
    if !not_template.is_empty() {
        parts.push(format!(
            "tag {} is read by a slot/wave metric but has no ix_template or program matcher - those conditions read nothing",
            not_template.join(", ")
        ));
    }
    (!parts.is_empty()).then(|| parts.join("; "))
}

/// Warning text when a tag pins a fee preset on an `ix_shape`.
///
/// Fee capture is forward-only, so a pinned entry matches NOTHING recorded before it,
/// and a rule written against one looks exactly like a rule whose coins went quiet.
/// Said once, at save time, where the author can still act on it.
pub fn fee_pin_warning(doc: &Value) -> Option<String> {
    let pinned: Vec<String> = compile_tags(doc)
        .into_iter()
        .filter(|t| t.patterns.builds.pins_fee())
        .map(|t| t.name.to_string())
        .collect();
    if pinned.is_empty() {
        return None;
    }
    Some(format!(
        "tag {} pins a fee preset on at least one ix shape - fee capture is forward-only, so \
         those entries match no trade recorded before it. Check the entry against RECENT \
         data, and confirm the pinned field is a preset rather than a value the sending \
         client recomputes per transaction.",
        pinned.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_full_tag_compiles() {
        let doc = json!({
            "volume": {
                "match": {
                    "program": ["Unknown (x)"],
                    "ix_shape": [["A", "B"], {"labels": ["A", "C"], "cu_price": 1000}],
                    "ix_template": ["Axiom Trade|CU|ATA|1|0|0"],
                    "ix_contains": ["Axiom Trade"],
                    "wallet": ["w1"],
                    "creator": true,
                    "cluster": {"min_prints": 3, "sol_tol_pct": 10}
                },
                "sticky": true,
                "exclude_creation_slot": true
            },
            "dump": {"match": {"ix_shape": [["S"]]}, "side": "sell"}
        });
        validate_tags(&doc).unwrap();
        let tags = compile_tags(&doc);
        assert_eq!(tags.len(), 2);
        let v = tags.iter().find(|t| t.name == "volume").unwrap();
        assert!(v.patterns.sticky && v.patterns.creator && v.patterns.exclude_creation_slot);
        assert_eq!(v.patterns.cluster, Some(Cluster { min_prints: 3, sol_tol_pct: 10 }));
        assert!(v.patterns.templates().is_some());
        let d = tags.iter().find(|t| t.name == "dump").unwrap();
        assert_eq!(d.patterns.side, Some(Side::Sell));
        assert!(d.patterns.templates().is_none());
    }

    #[test]
    fn every_mistake_names_its_path() {
        for (doc, needle) in [
            (json!({"Volume": {"match": {"creator": true}}}), "a-z"),
            (json!({"bundled": {"match": {"creator": true}}}), "built-in"),
            (json!({"v": {"match": {}}}), "no matcher"),
            (json!({"v": {"match": {"creator": "yes"}}}), "tags.v.match.creator"),
            (json!({"v": {"match": {"ix_contains": ["Nope"]}}}), "unknown ix marker"),
            (json!({"v": {"match": {"ix_template": ["Axiom Trade"]}}}), "under `program`"),
            (json!({"v": {"match": {"cluster": {"min_prints": 1, "sol_tol_pct": 5}}}}), "at least 2"),
            (json!({"v": {"match": {"creator": true}, "side": "both"}}), "\"buy\" or \"sell\""),
            (json!({"v": {"match": {"creator": true}, "contagion": true}}), "unknown key"),
            (json!({"v": {"match": {"nope": 1}}}), "unknown matcher"),
        ] {
            let err = validate_tags(&doc).unwrap_err();
            assert!(err.contains(needle), "{doc} -> {err}");
        }
    }

    #[test]
    fn the_documented_vocabulary_is_the_parsed_one() {
        let doc = tags_json();
        let keys: Vec<&str> = doc["fields"].as_array().unwrap().iter().map(|f| f["key"].as_str().unwrap()).collect();
        for k in ["program", "ix_shape", "ix_template", "ix_contains", "ix_lacks", "wallet", "creator", "cluster"] {
            assert!(keys.contains(&k), "matcher {k} undocumented");
        }
        for k in ["side", "sticky", "exclude_creation_slot"] {
            assert!(keys.contains(&k), "option {k} undocumented");
        }
    }
}
