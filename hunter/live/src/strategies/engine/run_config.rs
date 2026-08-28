//! What a run is running under, and what changed when it was edited mid-run.
//!
//! A rule edit does **not** rotate the run: `reload_rules` folds `RulesReloaded`,
//! the engine decides on the new config from that instant, and the run keeps its
//! open positions. So one run's numbers can span two configs, and
//! `strategy_runs.params_snapshot` — written once at launch — cannot say it did.
//! [`RunConfigSig`] is the diffable digest that can (`strategy_runs.config_hash` /
//! `config_edits`, mig 0012).
//!
//! **Why a signature and not the config itself.** The point is a marker, not an
//! audit trail: the board and the run navigator ask "did this change while the run
//! was scoring", which one hash per part answers in a fixed-size row. The parts are
//! separate so the answer names *what* changed — "ix structure" is a different
//! sentence to the operator than "take-profit".
//!
//! **Why the fingerprint is in here.** `m_flow_ix.ix_patterns` and the identity
//! axes live on `fingerprints`, never on the rule, and one fingerprint is shared by
//! every rule pointing at it — so an ix-structure edit re-defines several live rules
//! at once and touches no `strategy_rules` row. A signature built from the rule
//! alone would report exactly nothing for the edit most worth reporting.

use hunter_engine::event::LoadedRule;
use hunter_engine::fingerprint::Fingerprint;
use hunter_engine::hash::fnv1a;
use serde_json::Value;

/// One part of the config, in the words the operator edited it in. Order is the
/// order [`RunConfigSig::changed_since`] reports them.
const PART_NAMES: [&str; 5] = ["params", "buy size", "caps", "identity", "ix structure"];

/// The digest of everything a run's decisions read that an operator can edit,
/// split into the parts a change is reported as.
///
/// Deliberately NOT part of it: `trade_mode` (a mode flip already mints its own run
/// — `Sink::ensure_run` is mode-checked), `rule_name`, and `tags` — labels the
/// kernel never reads, so calling them a config change would cry wolf on a rename.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunConfigSig {
    parts: [u64; PART_NAMES.len()],
}

impl RunConfigSig {
    /// The signature of something that has no rule config at all — a manual-buy
    /// episode. Manual positions hang off the one manual run, which never goes
    /// through `Sink::ensure_run`, so this value is carried and never diffed.
    pub const fn absent() -> Self {
        RunConfigSig { parts: [0; PART_NAMES.len()] }
    }

    /// Build the signature for one loaded rule against the reloaded fingerprint set.
    ///
    /// `params` is the rule's `RuleParams` already rendered to JSON — the same value
    /// the sink freezes into `params_snapshot`, so the snapshot and the signature can
    /// never describe different params.
    ///
    /// A fingerprint the reload did not carry hashes as absent rather than as any
    /// particular shape: the alternative is a signature that silently equals some
    /// real config, which would report "unchanged" for a rule pointed at a deleted
    /// fingerprint.
    pub fn compute(rule: &LoadedRule, params: &Value, fps: &[Fingerprint]) -> Self {
        let fp = fps.iter().find(|f| f.id == rule.fingerprint_id);
        RunConfigSig {
            parts: [
                hash_json(params),
                fnv1a(&rule.buy_amount_lamports.to_le_bytes()),
                fnv1a(
                    &[
                        rule.max_concurrent_tokens.to_le_bytes(),
                        rule.max_total_tokens.to_le_bytes(),
                    ]
                    .concat(),
                ),
                // Identity is the wildcard flag AND the axes: `wildcard` is a
                // criterion of its own (it arms on everything), so hashing the axes
                // alone would miss the widest edit there is.
                fp.map_or(0, |f| {
                    hash_json(&serde_json::json!({
                        "wildcard": f.wildcard,
                        "criteria": f.criteria,
                    }))
                }),
                fp.map_or(0, |f| hash_json(&f.metric_config)),
            ],
        }
    }

    /// Which parts differ from `prev`, in [`PART_NAMES`] order. Empty = identical.
    pub fn changed_since(&self, prev: &Self) -> Vec<&'static str> {
        PART_NAMES
            .iter()
            .enumerate()
            .filter(|(i, _)| self.parts[*i] != prev.parts[*i])
            .map(|(_, name)| *name)
            .collect()
    }

    /// The persisted form (`strategy_runs.config_hash`) — the parts concatenated
    /// rather than folded together, so a run adopted across a restart can be diffed
    /// **part by part** and still name what changed while the process was down.
    pub fn hash_hex(&self) -> String {
        let mut out = String::with_capacity(PART_NAMES.len() * 16);
        for p in self.parts {
            out.push_str(&format!("{p:016x}"));
        }
        out
    }

    /// Read back a [`hash_hex`](Self::hash_hex). `None` for anything else — a hash
    /// written by an older part list is not a config this build can diff, and
    /// treating it as one would date a change that is really a version skew.
    pub fn from_hash_hex(s: &str) -> Option<Self> {
        if s.len() != PART_NAMES.len() * 16 {
            return None;
        }
        let mut parts = [0u64; PART_NAMES.len()];
        for (i, part) in parts.iter_mut().enumerate() {
            *part = u64::from_str_radix(&s[i * 16..(i + 1) * 16], 16).ok()?;
        }
        Some(RunConfigSig { parts })
    }
}

/// Digest of a JSON value through its canonical text. `serde_json::Value`'s object
/// map is a `BTreeMap` here (no `preserve_order` feature), so the text is key-sorted
/// and two equal configs cannot hash apart because PG returned their keys in a
/// different order.
fn hash_json(v: &Value) -> u64 {
    fnv1a(v.to_string().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hunter_engine::event::{RuleId, TradeMode};
    use hunter_engine::fingerprint::{Criteria, FingerprintId};
    use hunter_engine::rule_params::RuleParams;
    use serde_json::json;
    use uuid::Uuid;

    fn fp(metric_config: Value) -> Fingerprint {
        Fingerprint {
            id: FingerprintId(Uuid::nil()),
            wildcard: true,
            criteria: Criteria::new(),
            metric_config,
        }
    }

    fn rule() -> LoadedRule {
        LoadedRule {
            id: RuleId(Uuid::nil()),
            fingerprint_id: FingerprintId(Uuid::nil()),
            trade_mode: TradeMode::Paper,
            buy_amount_lamports: 100_000_000,
            max_concurrent_tokens: 3,
            max_total_tokens: 0,
            params: RuleParams::default(),
            entry_enabled: true,
        }
    }

    #[test]
    fn an_unchanged_config_reports_nothing() {
        let (r, fps) = (rule(), vec![fp(json!({}))]);
        let a = RunConfigSig::compute(&r, &json!({"take_profit": 20.0}), &fps);
        let b = RunConfigSig::compute(&r, &json!({"take_profit": 20.0}), &fps);
        assert_eq!(a, b);
        assert!(a.changed_since(&b).is_empty());
        assert_eq!(a.hash_hex(), b.hash_hex());
    }

    /// The edit this whole column set exists for: the build list moves and nothing
    /// on the rule row does.
    #[test]
    fn an_edited_ix_pattern_list_reports_ix_structure() {
        let r = rule();
        let before = RunConfigSig::compute(
            &r,
            &json!({}),
            &[fp(json!({"m_flow_ix": {"ix_patterns": [["Pump.Fun: Buy"]]}}))],
        );
        let after = RunConfigSig::compute(
            &r,
            &json!({}),
            &[fp(json!({"m_flow_ix": {"ix_patterns": [["Pump.Fun: Buy"], ["Photon"]]}}))],
        );
        assert_eq!(after.changed_since(&before), vec!["ix structure"]);
    }

    #[test]
    fn each_part_is_named_on_its_own() {
        let base = RunConfigSig::compute(&rule(), &json!({}), &[fp(json!({}))]);

        let mut sized = rule();
        sized.buy_amount_lamports = 200_000_000;
        assert_eq!(
            RunConfigSig::compute(&sized, &json!({}), &[fp(json!({}))]).changed_since(&base),
            vec!["buy size"]
        );

        let mut capped = rule();
        capped.max_total_tokens = 50;
        assert_eq!(
            RunConfigSig::compute(&capped, &json!({}), &[fp(json!({}))]).changed_since(&base),
            vec!["caps"]
        );

        assert_eq!(
            RunConfigSig::compute(&rule(), &json!({"stop_loss": 10.0}), &[fp(json!({}))])
                .changed_since(&base),
            vec!["params"]
        );

        let mut narrowed = fp(json!({}));
        narrowed.wildcard = false;
        assert_eq!(
            RunConfigSig::compute(&rule(), &json!({}), &[narrowed]).changed_since(&base),
            vec!["identity"]
        );
    }

    #[test]
    fn a_persisted_hash_round_trips_and_still_names_the_part() {
        let before = RunConfigSig::compute(&rule(), &json!({}), &[fp(json!({}))]);
        let after =
            RunConfigSig::compute(&rule(), &json!({}), &[fp(json!({"m_dump_ix": {}}))]);
        let reread = RunConfigSig::from_hash_hex(&before.hash_hex()).expect("round trip");
        assert_eq!(reread, before);
        assert_eq!(after.changed_since(&reread), vec!["ix structure"]);
        assert!(RunConfigSig::from_hash_hex("").is_none());
        assert!(RunConfigSig::from_hash_hex("not a hash").is_none());
    }

    /// "The fingerprint is gone" and "the fingerprint is empty" are different
    /// states, so they must not share a digest — else a rule left pointing at a
    /// deleted fingerprint reports "unchanged".
    #[test]
    fn a_missing_fingerprint_is_not_mistaken_for_an_empty_one() {
        let empty = RunConfigSig::compute(&rule(), &json!({}), &[fp(json!({}))]);
        let absent = RunConfigSig::compute(&rule(), &json!({}), &[]);
        assert_ne!(empty, absent);
    }
}
