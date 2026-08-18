//! Rule-CRUD **domain** — validate → build a rule → repo write.
//!
//! [`RuleDraft`] → [`StrategyRule`] (`strategy_rules`): a fingerprint reference
//! plus [`RuleParams`] metric conditions — the one generic-engine rule shape (the
//! there is no per-strategy CRUD.)
//!
//! Touches only validation + the repo; never a runtime cache, SSE, or RPC. The
//! calling edge appends its side effects (`rules_changed` + an engine reload on
//! `live`, nothing on `lab`) and owns the request→draft mapping + the live-edit
//! frozen-field guard.

use chrono::Utc;
use serde_json::Value;
use uuid::Uuid;

use crate::models::fingerprint::opt_i64;
use crate::models::StrategyRule;
use crate::storage::repositories::fingerprint_repo::FingerprintRepo;
use crate::storage::repositories::rule_repo::RuleRepo;

use super::rule_params::RuleParams;

pub use hunter_engine::metrics::bundle::bundle_unconfigured_warning;
pub use hunter_engine::metrics::flow_split::flow_unconfigured_warning;

/// Outcome of a rule CRUD write, mapped to an HTTP status by the calling edge.
#[derive(Debug)]
pub enum RuleError {
    /// 400 — a validation check rejected the proposed rule.
    Invalid(String),
    /// 409 — an identity-identical rule already exists (`rule_name` is a label,
    /// not identity). Carries the existing row for the error message.
    Duplicate { existing_id: Uuid, rule_name: String },
    /// 500 — a repository call failed.
    Repo(anyhow::Error),
}

// ═══════════════════════════════════════════════════════════════════════════
// Rule tags — free-form labels, presentational only
// ═══════════════════════════════════════════════════════════════════════════

/// Max tags on one rule. A label set past this is a taxonomy problem, not a
/// storage one — the write is rejected rather than truncated.
pub const MAX_TAGS_PER_RULE: usize = 8;

/// Max chars in one tag. Long enough for `stage:paper-test`, short enough that a
/// chip stays a chip.
pub const MAX_TAG_LEN: usize = 24;

/// Canonicalize a tag set — **the** authority on tag shape (wire parse and save
/// both run it, so storage is canonical whichever door the write came through).
///
/// Lowercase, `-`-separated, `[a-z0-9:-]` only, deduped, sorted. `:` survives as
/// the namespace separator (`fam:scalper`); `,` does not, because it is the
/// DataTable filter grammar's separator and a tag containing one would be
/// unfilterable. Sorting is what makes the stored array comparable and the chip
/// order stable across rules.
///
/// **Total by design** — it only reshapes. Over-long / too-many is a *loud*
/// rejection in [`validate_rule_fields`], never a silent truncation here: a
/// writer that clamps user input away before any reader sees it is the failure
/// shape that produced the slippage-floor bug (see the zero-as-unbound section of
/// `hunter/CLAUDE.md`). A tag that normalizes to nothing (`"!!!"`) is dropped —
/// there is no label left to store.
///
/// The frontend deliberately has no copy of this grammar; its chip input only
/// trims + lowercases and re-renders from the response. See
/// `docs/plans/strategies/rule-tags.md`.
pub fn normalize_tags(tags: &[String]) -> Vec<String> {
    let mut out: Vec<String> = tags
        .iter()
        .filter_map(|raw| {
            let mut s = String::with_capacity(raw.len());
            for ch in raw.trim().chars() {
                match ch {
                    'a'..='z' | '0'..='9' | ':' => s.push(ch),
                    'A'..='Z' => s.push(ch.to_ascii_lowercase()),
                    // Every separator collapses to one `-`; a repeat falls through
                    // to the drop arm below, which is what suppresses `a--b`.
                    '-' | '_' | ' ' | '\t' if !s.ends_with('-') => s.push('-'),
                    // Anything else (incl. `,` and repeated separators) is dropped.
                    _ => {}
                }
            }
            let s = s.trim_matches('-').to_string();
            (!s.is_empty()).then_some(s)
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

// ═══════════════════════════════════════════════════════════════════════════
// Generic engine (fingerprint + metrics) rule CRUD
// ═══════════════════════════════════════════════════════════════════════════

/// The authored inputs for a new generic rule. `params` is the raw JSON body —
/// [`build_rule`] parses/validates it against the metric registry
/// ([`RuleParams::parse`]) and stores the canonical serialization.
#[derive(Debug, Clone)]
pub struct RuleDraft {
    pub rule_name: String,
    pub fingerprint_id: Uuid,
    pub trade_mode: String,
    pub buy_amount_lamports: i64,
    /// 0 = unlimited.
    pub max_concurrent_tokens: i64,
    /// 0 = unlimited.
    pub max_total_tokens: i64,
    pub params: Value,
    /// Presentational labels — already canonical (see [`normalize_tags`]).
    pub tags: Vec<String>,
}

impl RuleDraft {
    /// Parse a create body from raw HTTP JSON — SSOT for the wire shape (shared
    /// by the live + lab CRUD handlers). `fingerprint_id` is required; every
    /// other field falls back to its default ([`build_rule`] / [`RuleParams::parse`]
    /// do the real validation). Amounts are lamports on the wire.
    pub fn from_json(body: &Value) -> Result<Self, String> {
        let fingerprint_id = body
            .get("fingerprint_id")
            .and_then(|v| v.as_str())
            .and_then(|s| Uuid::parse_str(s).ok())
            .ok_or("fingerprint_id is required (uuid)")?;
        Ok(RuleDraft {
            rule_name: body.get("rule_name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            fingerprint_id,
            trade_mode: body
                .get("trade_mode")
                .and_then(|v| v.as_str())
                .unwrap_or("paper")
                .to_string(),
            buy_amount_lamports: opt_i64(body, "buy_amount_lamports").unwrap_or(0),
            max_concurrent_tokens: opt_i64(body, "max_concurrent_tokens").unwrap_or(0),
            max_total_tokens: opt_i64(body, "max_total_tokens").unwrap_or(0),
            params: body.get("params").cloned().unwrap_or_else(|| serde_json::json!({})),
            tags: normalize_tags(&tags_from_json(body).unwrap_or_default()),
        })
    }
}

/// Read a `tags` array off a request body. `None` = the key is absent (a PATCH
/// leaves tags alone); `Some(vec![])` = an explicit empty array (clear them).
/// Non-string entries are skipped rather than failing the whole write — the same
/// leniency the numeric fields get.
fn tags_from_json(body: &Value) -> Option<Vec<String>> {
    let arr = body.get("tags")?.as_array()?;
    Some(
        arr.iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
    )
}

/// Overlay the mutable fields of a PUT body onto a loaded rule — SSOT for the
/// patch shape (shared by live + lab). `fingerprint_id`, `is_active`, and
/// `is_enabled` are intentionally not patchable here (fingerprint frozen
/// post-create; active/enabled only via the lifecycle endpoints). `trade_mode`
/// is patchable — the editor gates it behind an unlock; open positions keep
/// their snapshotted mode so an in-flight entry retry cannot flip paper↔real.
pub fn apply_rule_update(rule: &mut StrategyRule, body: &Value) {
    if let Some(v) = body.get("rule_name").and_then(|v| v.as_str()) {
        rule.rule_name = v.to_string();
    }
    if let Some(v) = body.get("trade_mode").and_then(|v| v.as_str()) {
        rule.trade_mode = v.to_string();
    }
    if let Some(v) = opt_i64(body, "buy_amount_lamports") {
        rule.buy_amount_lamports = v;
    }
    if let Some(v) = opt_i64(body, "max_concurrent_tokens") {
        rule.max_concurrent_tokens = v;
    }
    if let Some(v) = opt_i64(body, "max_total_tokens") {
        rule.max_total_tokens = v;
    }
    if let Some(v) = body.get("params").cloned() {
        rule.params = v;
    }
    // Absent ⇒ leave tags alone (patch semantics); `[]` ⇒ clear them. `save`
    // re-normalizes, so a hand-rolled PUT can't store a non-canonical array.
    if let Some(tags) = tags_from_json(body) {
        rule.tags = normalize_tags(&tags);
    }
    rule.updated_at = Utc::now();
}

/// Validate the typed-column knobs shared by create and edit. `tags` are
/// expected already normalized ([`normalize_tags`]) — this checks the caps that
/// normalization deliberately does **not** silently enforce.
fn validate_rule_fields(
    rule_name: &str,
    trade_mode: &str,
    buy_amount_lamports: i64,
    max_concurrent_tokens: i64,
    max_total_tokens: i64,
    tags: &[String],
) -> Result<(), String> {
    if rule_name.trim().is_empty() {
        return Err("rule_name must not be empty".into());
    }
    if trade_mode != "paper" && trade_mode != "real" {
        return Err("trade_mode must be 'paper' or 'real'".into());
    }
    if buy_amount_lamports <= 0 {
        return Err("buy_amount_lamports must be > 0".into());
    }
    if max_concurrent_tokens < 0 {
        return Err("max_concurrent_tokens must be >= 0 (0 = unlimited)".into());
    }
    if max_total_tokens < 0 {
        return Err("max_total_tokens must be >= 0 (0 = unlimited)".into());
    }
    if tags.len() > MAX_TAGS_PER_RULE {
        return Err(format!(
            "at most {MAX_TAGS_PER_RULE} tags per rule (got {})",
            tags.len()
        ));
    }
    if let Some(long) = tags.iter().find(|t| t.chars().count() > MAX_TAG_LEN) {
        return Err(format!(
            "tag \"{long}\" is longer than {MAX_TAG_LEN} characters"
        ));
    }
    Ok(())
}

/// Validate + assemble a new (unpersisted) [`StrategyRule`] from a draft. The
/// new rule is inactive (`is_active = false`) and enabled (`is_enabled = true`);
/// lifecycle endpoints toggle those flags. Params are stored in their canonical
/// serialization ([`RuleParams::to_value`]) so the JSONB shape can't drift by
/// author.
pub fn build_rule(draft: &RuleDraft) -> Result<StrategyRule, String> {
    // Re-normalize rather than trust the draft: `from_json` already did it, but a
    // caller that builds a `RuleDraft` directly (promote, tests) must not be able
    // to store a non-canonical array.
    let tags = normalize_tags(&draft.tags);
    validate_rule_fields(
        &draft.rule_name,
        &draft.trade_mode,
        draft.buy_amount_lamports,
        draft.max_concurrent_tokens,
        draft.max_total_tokens,
        &tags,
    )?;
    let params = RuleParams::parse(&draft.params)?;
    let now = Utc::now();
    Ok(StrategyRule {
        id: Uuid::new_v4(),
        rule_name: draft.rule_name.clone(),
        fingerprint_id: draft.fingerprint_id,
        trade_mode: draft.trade_mode.clone(),
        is_active: false,
        is_enabled: true,
        buy_amount_lamports: draft.buy_amount_lamports,
        max_concurrent_tokens: draft.max_concurrent_tokens,
        max_total_tokens: draft.max_total_tokens,
        params: params.to_value(),
        tags,
        created_at: now,
        updated_at: now,
    })
}

/// Reject when another rule already shares trading identity (fingerprint +
/// sizing/caps + canonical params). `rule_name` / `is_active` / `is_enabled`
/// are not identity.
async fn reject_duplicate(
    repo: &RuleRepo,
    fingerprint_id: Uuid,
    trade_mode: &str,
    buy_amount_lamports: i64,
    max_concurrent_tokens: i64,
    max_total_tokens: i64,
    params: &Value,
    exclude_id: Option<Uuid>,
) -> Result<(), RuleError> {
    match repo
        .find_identical(
            fingerprint_id,
            trade_mode,
            buy_amount_lamports,
            max_concurrent_tokens,
            max_total_tokens,
            params,
            exclude_id,
        )
        .await
    {
        Ok(Some(existing)) => Err(RuleError::Duplicate {
            existing_id: existing.id,
            rule_name: existing.rule_name,
        }),
        Ok(None) => Ok(()),
        Err(e) => Err(RuleError::Repo(e)),
    }
}

/// Validate, build, and persist a new generic rule. Refuses an identity-identical
/// duplicate (promote / New / Duplicate all share this gate).
pub async fn create(repo: &RuleRepo, draft: &RuleDraft) -> Result<StrategyRule, RuleError> {
    let rule = build_rule(draft).map_err(RuleError::Invalid)?;
    reject_duplicate(
        repo,
        rule.fingerprint_id,
        &rule.trade_mode,
        rule.buy_amount_lamports,
        rule.max_concurrent_tokens,
        rule.max_total_tokens,
        &rule.params,
        None,
    )
    .await?;
    repo.insert(&rule).await.map_err(RuleError::Repo)?;
    Ok(rule)
}

/// [`create`] plus a soft warning when the rule references flow metrics but the
/// fingerprint has no `m_flow_split` config (metrics will be `NaN` until set).
pub async fn create_with_fp_check(
    repo: &RuleRepo,
    fp_repo: &FingerprintRepo,
    draft: &RuleDraft,
) -> Result<(StrategyRule, Option<String>), RuleError> {
    let rule = create(repo, draft).await?;
    let warning = flow_warning_for(fp_repo, rule.fingerprint_id, &rule.params).await;
    Ok((rule, warning))
}

/// Re-validate a fully-formed generic rule and persist the edit. The caller
/// merges its request patch into the loaded rule first (request-shaped,
/// edge-side) and enforces any live-edit frozen-field guard. Params are stored
/// in canonical form (same fixed point as [`build_rule`]) so identity compares
/// cannot drift by author JSON shape. Refuses colliding with another rule's
/// identity.
pub async fn save(repo: &RuleRepo, rule: &mut StrategyRule) -> Result<(), RuleError> {
    // Same fixed point as `params` below — canonical shape can't drift by author.
    rule.tags = normalize_tags(&rule.tags);
    validate_rule_fields(
        &rule.rule_name,
        &rule.trade_mode,
        rule.buy_amount_lamports,
        rule.max_concurrent_tokens,
        rule.max_total_tokens,
        &rule.tags,
    )
    .map_err(RuleError::Invalid)?;
    let params = RuleParams::parse(&rule.params)
        .map_err(|e| RuleError::Invalid(format!("invalid params: {e}")))?;
    rule.params = params.to_value();
    reject_duplicate(
        repo,
        rule.fingerprint_id,
        &rule.trade_mode,
        rule.buy_amount_lamports,
        rule.max_concurrent_tokens,
        rule.max_total_tokens,
        &rule.params,
        Some(rule.id),
    )
    .await?;
    repo.update(rule).await.map_err(RuleError::Repo)?;
    Ok(())
}

/// [`save`] plus the flow-unconfigured soft warning (see [`create_with_fp_check`]).
pub async fn save_with_fp_check(
    repo: &RuleRepo,
    fp_repo: &FingerprintRepo,
    rule: &mut StrategyRule,
) -> Result<Option<String>, RuleError> {
    save(repo, rule).await?;
    Ok(flow_warning_for(fp_repo, rule.fingerprint_id, &rule.params).await)
}

async fn flow_warning_for(
    fp_repo: &FingerprintRepo,
    fingerprint_id: Uuid,
    params: &Value,
) -> Option<String> {
    match fp_repo.find(fingerprint_id).await {
        // Both groups are fingerprint-scoped and fail the same silent way (NaN reads,
        // rule never fires), so one call site reports whichever is unconfigured.
        Ok(Some(fp)) => flow_unconfigured_warning(params, &fp.metric_config)
            .or_else(|| bundle_unconfigured_warning(params, &fp.metric_config)),
        _ => None,
    }
}

#[cfg(test)]
mod generic_tests {
    use super::*;
    use serde_json::json;

    fn generic_draft(params: Value) -> RuleDraft {
        RuleDraft {
            rule_name: "r".into(),
            fingerprint_id: Uuid::new_v4(),
            trade_mode: "paper".into(),
            buy_amount_lamports: 1_000_000_000, // 1 SOL
            max_concurrent_tokens: 3,
            max_total_tokens: 0,
            params,
            tags: Vec::new(),
        }
    }

    fn valid_params() -> Value {
        json!({
            "take_profit": 100,
            "stop_loss": 30,
            "entry": {
                "m_snapshot": {
                    "time": [
                        {"operator": ">", "value": 10},
                        {"operator": "<", "value": 30}
                    ]
                }
            }
        })
    }

    #[test]
    fn valid_generic_rule_builds_inactive_with_canonical_params() {
        let rule = build_rule(&generic_draft(valid_params())).expect("valid");
        assert!(!rule.is_active);
        assert_eq!(rule.buy_amount_lamports, 1_000_000_000);
        assert!((rule.buy_amount_sol() - 1.0).abs() < 1e-12);
        // Stored params are the canonical serialization — re-parsing and
        // re-serializing is a fixed point.
        let reparsed = RuleParams::parse(&rule.params).unwrap();
        assert_eq!(reparsed.to_value(), rule.params);
    }

    #[test]
    fn generic_field_validation() {
        let mut d = generic_draft(valid_params());
        d.trade_mode = "live".into();
        assert!(matches!(build_rule(&d), Err(e) if e.contains("trade_mode")));

        let mut d = generic_draft(valid_params());
        d.buy_amount_lamports = 0;
        assert!(matches!(build_rule(&d), Err(e) if e.contains("buy_amount_lamports")));

        let mut d = generic_draft(valid_params());
        d.max_concurrent_tokens = -1;
        assert!(matches!(build_rule(&d), Err(e) if e.contains("max_concurrent_tokens")));

        // `0` is the unbounded sentinel on BOTH caps, not a rejected value — a
        // blank Max concurrent in the editor saves as 0 and must round-trip.
        let mut d = generic_draft(valid_params());
        d.max_concurrent_tokens = 0;
        assert_eq!(build_rule(&d).expect("0 = unlimited").max_concurrent_tokens, 0);

        let mut d = generic_draft(valid_params());
        d.rule_name = "  ".into();
        assert!(matches!(build_rule(&d), Err(e) if e.contains("rule_name")));
    }

    #[test]
    fn apply_rule_update_patches_trade_mode() {
        let mut rule = build_rule(&generic_draft(valid_params())).expect("valid");
        assert_eq!(rule.trade_mode, "paper");
        apply_rule_update(
            &mut rule,
            &json!({
                "trade_mode": "real",
                "rule_name": "renamed",
                "buy_amount_lamports": 2_000_000_000_i64,
            }),
        );
        assert_eq!(rule.trade_mode, "real");
        assert_eq!(rule.rule_name, "renamed");
        assert_eq!(rule.buy_amount_lamports, 2_000_000_000);
    }

    #[test]
    fn generic_params_registry_checked() {
        // A typo'd metric can't silently no-op — it fails the save.
        let d = generic_draft(json!({
            "entry": {"m_snapshot": {"tyme": [{"operator": ">", "value": 1}]}}
        }));
        assert!(matches!(build_rule(&d), Err(e) if e.contains("tyme")));

        // Contradictory conditions fail the save. A *flat* same-field list is now
        // coalesced to OR arms (`> 30 OR < 10`, satisfiable — see
        // `normalize_condition_expr`), so contradiction is only unavoidable when every
        // explicit OR arm is itself unsatisfiable — the multi-arm form the engine keeps
        // as authored (`multi_arm_all_unsat_still_rejected`).
        let d = generic_draft(json!({
            "entry": {"m_snapshot": {"time": [
                [{"operator": ">", "value": 30}, {"operator": "<", "value": 10}],
                [{"operator": ">", "value": 50}, {"operator": "<", "value": 20}]
            ]}}
        }));
        assert!(matches!(build_rule(&d), Err(e) if e.contains("contradictory")));

        // Fingerprint-only rule (empty params) is legal: enter on arm, TP/SL off.
        assert!(build_rule(&generic_draft(json!({}))).is_ok());
    }

    #[test]
    fn rule_identity_excludes_name_and_active() {
        // Two drafts that trade the same way must canonicalize to equal identity
        // axes (what `find_identical` / Duplicate gate on) even when names differ.
        let mut a = generic_draft(valid_params());
        a.rule_name = "promoted g1 c2".into();
        let mut b = generic_draft(valid_params());
        b.rule_name = "other label".into();
        b.fingerprint_id = a.fingerprint_id;
        let ra = build_rule(&a).expect("a");
        let rb = build_rule(&b).expect("b");
        assert_eq!(ra.fingerprint_id, rb.fingerprint_id);
        assert_eq!(ra.trade_mode, rb.trade_mode);
        assert_eq!(ra.buy_amount_lamports, rb.buy_amount_lamports);
        assert_eq!(ra.max_concurrent_tokens, rb.max_concurrent_tokens);
        assert_eq!(ra.max_total_tokens, rb.max_total_tokens);
        assert_eq!(ra.params, rb.params, "canonical params must match for identity");
        assert_ne!(ra.rule_name, rb.rule_name);
    }

    // ── tags ────────────────────────────────────────────────────────────────

    fn tags(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn normalize_tags_canonicalizes_shape() {
        // Case, separators, and repeats all collapse to one canonical form.
        assert_eq!(
            normalize_tags(&tags(&["Fam:Scalper", "fam:scalper", "  FAM:SCALPER  "])),
            tags(&["fam:scalper"]),
            "case/whitespace variants must not become distinct chips"
        );
        assert_eq!(
            normalize_tags(&tags(&["paper test", "paper_test", "paper--test", "-paper-test-"])),
            tags(&["paper-test"]),
            "every separator collapses to a single inner dash"
        );
        // `:` survives (namespace separator); `,` does not — it is the DataTable
        // filter grammar's separator, so a tag holding one would be unfilterable.
        assert_eq!(normalize_tags(&tags(&["a,b"])), tags(&["ab"]));
        assert_eq!(normalize_tags(&tags(&["risk:high!"])), tags(&["risk:high"]));
        // Sorted + deduped ⇒ the stored array is comparable and renders stably.
        assert_eq!(normalize_tags(&tags(&["zeta", "alpha", "zeta"])), tags(&["alpha", "zeta"]));
        // A label that reshapes to nothing has nothing left to store.
        assert!(normalize_tags(&tags(&["!!!", "   ", "---"])).is_empty());
    }

    #[test]
    fn normalize_tags_is_idempotent() {
        // `from_json`, `build_rule`, and `save` all run it — a second pass must be
        // a no-op or the same rule would keep changing shape on every save.
        let once = normalize_tags(&tags(&["Fam Scalper", "src:SWEEP", "a,,b"]));
        assert_eq!(normalize_tags(&once), once);
    }

    #[test]
    fn tag_caps_are_rejected_not_truncated() {
        // Silently dropping the 9th tag would be the "writer clamps user input
        // away before any reader sees it" bug shape — reject loudly instead.
        let mut d = generic_draft(valid_params());
        d.tags = (0..MAX_TAGS_PER_RULE + 1).map(|i| format!("t{i}")).collect();
        assert!(matches!(build_rule(&d), Err(e) if e.contains("at most")));

        let mut d = generic_draft(valid_params());
        d.tags = tags(&["x".repeat(MAX_TAG_LEN + 1).as_str()]);
        assert!(matches!(build_rule(&d), Err(e) if e.contains("longer than")));

        // At the cap is fine.
        let mut d = generic_draft(valid_params());
        d.tags = (0..MAX_TAGS_PER_RULE).map(|i| format!("t{i}")).collect();
        assert!(build_rule(&d).is_ok());
    }

    #[test]
    fn build_rule_stores_canonical_tags() {
        let mut d = generic_draft(valid_params());
        d.tags = tags(&["Stage:Paper Test", "fam:scalper", "fam:scalper"]);
        let rule = build_rule(&d).expect("valid");
        assert_eq!(rule.tags, tags(&["fam:scalper", "stage:paper-test"]));
    }

    #[test]
    fn tags_are_not_trading_identity() {
        // The Duplicate gate compares fingerprint + knobs + canonical params. Tags
        // must stay out of it — that is the whole reason they are a column rather
        // than a `params` key (docs/plans/strategies/rule-tags.md).
        let mut a = generic_draft(valid_params());
        a.tags = tags(&["fam:scalper"]);
        let mut b = generic_draft(valid_params());
        b.fingerprint_id = a.fingerprint_id;
        b.tags = tags(&["stage:experiment", "risk:high"]);
        let ra = build_rule(&a).expect("a");
        let rb = build_rule(&b).expect("b");
        assert_eq!(ra.params, rb.params);
        assert_eq!(ra.buy_amount_lamports, rb.buy_amount_lamports);
        assert_ne!(ra.tags, rb.tags, "tags differ but identity does not");
    }

    #[test]
    fn apply_rule_update_tag_patch_semantics() {
        let mut rule = build_rule(&generic_draft(valid_params())).expect("valid");
        apply_rule_update(&mut rule, &json!({ "tags": ["Fam:Scalper"] }));
        assert_eq!(rule.tags, tags(&["fam:scalper"]));

        // Absent key ⇒ untouched (a rename must not wipe tags).
        apply_rule_update(&mut rule, &json!({ "rule_name": "renamed" }));
        assert_eq!(rule.tags, tags(&["fam:scalper"]));

        // Explicit empty array ⇒ cleared.
        apply_rule_update(&mut rule, &json!({ "tags": [] }));
        assert!(rule.tags.is_empty());
    }

    #[test]
    fn from_json_normalizes_tags() {
        let body = json!({
            "fingerprint_id": Uuid::new_v4().to_string(),
            // Non-string entries are skipped, not fatal — same leniency as the
            // numeric fields.
            "tags": ["Fam Scalper", 7, "fam-scalper"],
        });
        let d = RuleDraft::from_json(&body).expect("parses");
        assert_eq!(d.tags, tags(&["fam-scalper"]));
    }
}
