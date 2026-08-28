//! Strategy **bundle** — move a rule + the fingerprint it needs between two boxes
//! (workstation lab <-> EC2 live) as one block of JSON.
//!
//! `scripts/db-incremental-sync.ps1` already mirrors `fingerprints` /
//! `strategy_rules` **server -> local**, but only as part of a full FDW pull (SSH
//! tunnel, superuser role, backend stopped) and only in that one direction. Editing
//! one ix pattern or one metric param changes TWO ROWS; this is the door for those
//! two rows, both ways, from the page you edited them on.
//!
//! Three steps, and the middle one is the point:
//!
//! * [`export_bundle`] — the source box serializes the selected rules plus their
//!   fingerprints.
//! * [`plan_bundle`] — the target box says what WOULD change, field by field.
//!   **Reads only.** A paste you cannot inspect first has the same failure mode as
//!   the hand-written SQL it replaces: you cannot see what you are about to
//!   overwrite.
//! * [`apply_bundle`] — executes that same plan.
//!
//! Preview and apply run the **one** resolver ([`plan_bundle`]), so what you
//! approved is what runs.
//!
//! # What travels, and what does not
//!
//! Travelling is the *strategy*: fingerprint `criteria` / `metric_config` / `name`,
//! rule `params` / sizing / caps / tags. Staying put is the *box*: `is_active`,
//! `is_enabled`, `trade_mode`, and every history table (`strategy_runs`,
//! `strategy_positions`, ...), which the incremental sync already owns.
//!
//! `is_active` is the safety-critical one. If arming rode along in the bundle, a
//! paste from the paper lab would arm a real-money rule on the live box. It cannot:
//! an update is applied through [`rules::apply_rule_update`], which does not patch
//! `is_active` / `is_enabled` / `fingerprint_id` by design, and an insert goes
//! through [`rules::create_with_id`], which builds every new rule inactive. An
//! imported rule that is new here lands as **paper**, and promoting it to real is a
//! deliberate act on the target box.
//!
//! # Identity
//!
//! The UUID is the join key, and it is already shared — the incremental sync copies
//! `id` across, so the same strategy carries the same UUID on both boxes. That is
//! what makes an import idempotent: paste twice, the second is `identical`.
//!
//! A fingerprint that is new *by id* may still be present *by identity*
//! (`fingerprints_identity_uniq` on criteria + wildcard + metric_config). Inserting
//! it would trip that index, so the plan resolves it to [`ItemStatus::ReuseExisting`]
//! and rebinds the bundle's rules onto the row already here.
//!
//! # Atomicity
//!
//! Every foreseeable collision is a [`ItemStatus::Conflict`], and one conflict
//! blocks the whole apply **before the first write**. Past that gate the writes are
//! sequential (fingerprints, then rules — FK order), not one transaction: the repos
//! are pool-bound. A mid-flight DB error can therefore leave the fingerprints
//! written and the rules not, which re-running the same bundle finishes, because
//! every step is an upsert keyed on the UUID.

use std::collections::HashMap;

use actix_web::HttpResponse;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::models::{Fingerprint, StrategyRule};
use crate::storage::repositories::fingerprint_repo::FingerprintRepo;
use crate::storage::repositories::rule_repo::RuleRepo;
use crate::strategies::rules::{self, normalize_tags, RuleDraft, RuleError};

use hunter_engine::rule_params::RuleParams;

/// Wire-format version. Bumped only for a change a reader cannot absorb; the
/// target box refuses a bundle it does not know how to read rather than applying
/// the half of it that happens to parse.
pub const BUNDLE_FORMAT_VERSION: u32 = 1;

/// `trade_mode` for a rule the target box has never seen. Paper, always — see the
/// module docs. An existing rule keeps whatever mode it already has here.
const IMPORT_TRADE_MODE: &str = "paper";

// ═══════════════════════════════════════════════════════════════════════════
// Wire shapes
// ═══════════════════════════════════════════════════════════════════════════

/// One fingerprint as it travels. Identity (`criteria` + `wildcard` +
/// `metric_config`) plus the label, and `updated_at` so the preview can say which
/// side is older.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleFingerprint {
    pub id: Uuid,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub wildcard: bool,
    #[serde(default)]
    pub criteria: Value,
    #[serde(default = "empty_object")]
    pub metric_config: Value,
    pub updated_at: DateTime<Utc>,
}

/// One rule as it travels. No `is_active`, no `is_enabled`, no `trade_mode` — see
/// the module docs on what stays with the box.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleRule {
    pub id: Uuid,
    pub rule_name: String,
    pub fingerprint_id: Uuid,
    pub buy_amount_lamports: i64,
    #[serde(default)]
    pub max_concurrent_tokens: i64,
    #[serde(default)]
    pub max_total_tokens: i64,
    #[serde(default = "empty_object")]
    pub params: Value,
    #[serde(default)]
    pub tags: Vec<String>,
    pub updated_at: DateTime<Utc>,
}

/// The whole clipboard payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleBundle {
    pub bundle_format_version: u32,
    pub exported_at: DateTime<Utc>,
    /// Free-text provenance for the preview header (`hunter-live` / `hunter-lab`).
    /// Informational only — nothing branches on it.
    #[serde(default)]
    pub source: String,
    pub fingerprints: Vec<BundleFingerprint>,
    pub rules: Vec<BundleRule>,
}

fn empty_object() -> Value {
    json!({})
}

// ═══════════════════════════════════════════════════════════════════════════
// Plan shapes
// ═══════════════════════════════════════════════════════════════════════════

/// What the target box would do with one bundle item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    /// Already here, byte-identical on every travelling field. Apply skips it.
    Identical,
    /// Here under this UUID, with differences. Apply updates it.
    Changed,
    /// Not here at all. Apply inserts it, keeping the bundle's UUID.
    New,
    /// Fingerprint only: not here by UUID, but an identity-identical row IS here
    /// under a different UUID. Apply inserts nothing and rebinds this bundle's
    /// rules onto that row (inserting would trip `fingerprints_identity_uniq`).
    ReuseExisting,
    /// Rule only: not here by UUID, but a rule with the same trading identity
    /// already is. Apply skips it — the strategy is present, under another UUID.
    Duplicate,
    /// Cannot be applied. One of these blocks the whole apply, before any write.
    Conflict,
}

impl ItemStatus {
    /// Does applying this item write anything?
    fn writes(self) -> bool {
        matches!(self, ItemStatus::Changed | ItemStatus::New)
    }
}

/// One field that differs between the bundle and this box.
#[derive(Debug, Clone, Serialize)]
pub struct FieldChange {
    pub field: String,
    /// Value on THIS box (absent for a new row).
    pub from: Value,
    /// Value in the bundle.
    pub to: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct FingerprintPlan {
    /// UUID as it appears in the bundle.
    pub id: Uuid,
    pub name: String,
    pub status: ItemStatus,
    /// The row on THIS box this resolves to; the bundle's rules are rebound onto
    /// it. Differs from `id` only under [`ItemStatus::ReuseExisting`].
    pub target_id: Uuid,
    pub changes: Vec<FieldChange>,
    pub local_updated_at: Option<DateTime<Utc>>,
    pub incoming_updated_at: DateTime<Utc>,
    /// Why this is a conflict, or what a non-obvious status means.
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RulePlan {
    pub id: Uuid,
    pub rule_name: String,
    pub status: ItemStatus,
    /// Fingerprint row this rule binds to here, after any rebind.
    pub fingerprint_id: Uuid,
    pub changes: Vec<FieldChange>,
    pub local_updated_at: Option<DateTime<Utc>>,
    pub incoming_updated_at: DateTime<Utc>,
    pub note: Option<String>,
}

/// The full diff. `blocked` is the gate: true when any item is a
/// [`ItemStatus::Conflict`], and then apply refuses without writing.
#[derive(Debug, Clone, Serialize)]
pub struct BundlePlan {
    pub fingerprints: Vec<FingerprintPlan>,
    pub rules: Vec<RulePlan>,
    pub blocked: bool,
    /// Every conflict note, hoisted so the UI can show them without walking both
    /// item lists.
    pub blockers: Vec<String>,
    /// Count of items apply would write (`new` + `changed`, both lists).
    pub writes: usize,
}

/// What an apply actually did, per item, so the UI can report rather than assume.
#[derive(Debug, Clone, Serialize)]
pub struct BundleApplied {
    pub fingerprints_inserted: usize,
    pub fingerprints_updated: usize,
    pub rules_inserted: usize,
    pub rules_updated: usize,
    pub skipped: usize,
    /// The plan that was executed — the UI renders the same rows it previewed.
    pub plan: BundlePlan,
}

// ═══════════════════════════════════════════════════════════════════════════
// Export
// ═══════════════════════════════════════════════════════════════════════════

/// Build a bundle for `rule_ids`, or for every rule on this box when `None`.
///
/// The fingerprint set is *derived*, never chosen: exactly the distinct
/// fingerprints the selected rules reference. A bundle whose rule points at a
/// fingerprint it does not carry would resolve against whatever the target box
/// happens to have under that UUID.
pub async fn export_bundle(
    fp_repo: &FingerprintRepo,
    rule_repo: &RuleRepo,
    rule_ids: Option<&[Uuid]>,
    source: &str,
) -> anyhow::Result<RuleBundle> {
    let rules = match rule_ids {
        Some(ids) => rule_repo.find_many(ids).await?,
        None => rule_repo.list().await?,
    };
    if let Some(ids) = rule_ids {
        if rules.len() != ids.len() {
            let found: Vec<Uuid> = rules.iter().map(|r| r.id).collect();
            let missing: Vec<String> = ids
                .iter()
                .filter(|id| !found.contains(id))
                .map(|id| id.to_string())
                .collect();
            anyhow::bail!("no such rule: {}", missing.join(", "));
        }
    }

    // One read of the table, then pick — `find` per rule would be N round-trips for
    // a set that is a few dozen rows whole.
    let all_fps = fp_repo.list().await?;
    let by_id: HashMap<Uuid, &Fingerprint> = all_fps.iter().map(|f| (f.id, f)).collect();

    let mut wanted: Vec<Uuid> = rules.iter().map(|r| r.fingerprint_id).collect();
    wanted.sort();
    wanted.dedup();

    let mut fingerprints = Vec::with_capacity(wanted.len());
    for id in wanted {
        let fp = by_id
            .get(&id)
            .ok_or_else(|| anyhow::anyhow!("rule references missing fingerprint {id}"))?;
        fingerprints.push(BundleFingerprint {
            id: fp.id,
            name: fp.name.clone(),
            wildcard: fp.wildcard,
            criteria: serde_json::to_value(&fp.criteria)?,
            metric_config: fp.metric_config.clone(),
            updated_at: fp.updated_at,
        });
    }

    Ok(RuleBundle {
        bundle_format_version: BUNDLE_FORMAT_VERSION,
        exported_at: Utc::now(),
        source: source.to_string(),
        fingerprints,
        rules: rules.iter().map(bundle_rule_from).collect(),
    })
}

fn bundle_rule_from(r: &StrategyRule) -> BundleRule {
    BundleRule {
        id: r.id,
        rule_name: r.rule_name.clone(),
        fingerprint_id: r.fingerprint_id,
        buy_amount_lamports: r.buy_amount_lamports,
        max_concurrent_tokens: r.max_concurrent_tokens,
        max_total_tokens: r.max_total_tokens,
        params: r.params.clone(),
        tags: r.tags.clone(),
        updated_at: r.updated_at,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Plan — the one resolver, shared by preview and apply
// ═══════════════════════════════════════════════════════════════════════════

/// Resolve a bundle against this box **without writing**.
///
/// Every check that would fail at write time runs here instead: the axis registry
/// parse, the fingerprint validators, the metric-registry params parse, the
/// identity-index collision, the duplicate-rule gate. So a box that lacks a metric
/// group the bundle uses says which rule cannot land, in the preview, rather than
/// 500-ing halfway through an apply.
pub async fn plan_bundle(
    fp_repo: &FingerprintRepo,
    rule_repo: &RuleRepo,
    bundle: &RuleBundle,
) -> anyhow::Result<BundlePlan> {
    let mut fp_plans = Vec::with_capacity(bundle.fingerprints.len());
    // bundle fingerprint id -> the row on this box its rules must bind to.
    let mut rebind: HashMap<Uuid, Uuid> = HashMap::new();

    for incoming in &bundle.fingerprints {
        let plan = plan_one_fingerprint(fp_repo, incoming).await?;
        rebind.insert(plan.id, plan.target_id);
        fp_plans.push(plan);
    }

    let mut rule_plans = Vec::with_capacity(bundle.rules.len());
    for incoming in &bundle.rules {
        rule_plans.push(plan_one_rule(rule_repo, incoming, &rebind).await?);
    }

    let blockers: Vec<String> = fp_plans
        .iter()
        .filter(|p| p.status == ItemStatus::Conflict)
        .map(|p| format!("fingerprint \"{}\": {}", p.name, p.note.clone().unwrap_or_default()))
        .chain(
            rule_plans
                .iter()
                .filter(|p| p.status == ItemStatus::Conflict)
                .map(|p| {
                    format!("rule \"{}\": {}", p.rule_name, p.note.clone().unwrap_or_default())
                }),
        )
        .collect();

    let writes = fp_plans.iter().filter(|p| p.status.writes()).count()
        + rule_plans.iter().filter(|p| p.status.writes()).count();

    Ok(BundlePlan {
        blocked: !blockers.is_empty(),
        blockers,
        writes,
        fingerprints: fp_plans,
        rules: rule_plans,
    })
}

/// Parse + validate one incoming fingerprint exactly as the CRUD handlers do, so
/// the preview rejects what the write would reject.
fn incoming_fingerprint(b: &BundleFingerprint) -> Result<Fingerprint, String> {
    let body = json!({
        "name": b.name,
        "wildcard": b.wildcard,
        "criteria": b.criteria,
        "metric_config": b.metric_config,
    });
    // Timestamps are the target box's, never the bundle's: `created_at` is when the
    // row appeared HERE, and `updated_at` is this write. The bundle's own
    // `updated_at` is carried in the plan for the operator to read, not stored.
    let mut fp = Fingerprint::from_json(&body, b.id, Utc::now())?;
    fp.ensure_auto_name();
    fp.validate()?;
    hunter_engine::metrics::validate_fingerprint_metric_config(&fp.metric_config)?;
    Ok(fp)
}

async fn plan_one_fingerprint(
    fp_repo: &FingerprintRepo,
    b: &BundleFingerprint,
) -> anyhow::Result<FingerprintPlan> {
    let mut plan = FingerprintPlan {
        id: b.id,
        name: b.name.clone(),
        status: ItemStatus::Conflict,
        target_id: b.id,
        changes: Vec::new(),
        local_updated_at: None,
        incoming_updated_at: b.updated_at,
        note: None,
    };

    let incoming = match incoming_fingerprint(b) {
        Ok(fp) => fp,
        Err(e) => {
            plan.note = Some(e);
            return Ok(plan);
        }
    };
    plan.name = incoming.name.clone();

    let local = fp_repo.find(b.id).await?;
    let holder = fp_repo.find_by_identity(&incoming).await?;

    match local {
        Some(local) => {
            plan.local_updated_at = Some(local.updated_at);
            // The identity this row would take is already held by a DIFFERENT row
            // here. `fingerprints_identity_uniq` would reject the UPDATE, so say so
            // now instead of at write time. Silently repointing the rules at the
            // holder would be the wrong repair: the two rows are different
            // fingerprints to every rule already bound to them.
            if let Some(h) = holder.as_ref().filter(|h| h.id != local.id) {
                plan.note = Some(format!(
                    "these criteria + metric_config already belong to \"{}\" ({}) on this box; \
                     merge or delete that fingerprint first",
                    h.name, h.id
                ));
                return Ok(plan);
            }
            plan.changes = fingerprint_changes(&local, &incoming);
            plan.status = if plan.changes.is_empty() {
                ItemStatus::Identical
            } else {
                ItemStatus::Changed
            };
        }
        None => match holder {
            Some(h) => {
                plan.status = ItemStatus::ReuseExisting;
                plan.target_id = h.id;
                plan.local_updated_at = Some(h.updated_at);
                plan.note = Some(format!(
                    "identical fingerprint already here as \"{}\" ({}); \
                     the bundle's rules bind to it",
                    h.name, h.id
                ));
            }
            None => plan.status = ItemStatus::New,
        },
    }
    Ok(plan)
}

fn fingerprint_changes(local: &Fingerprint, incoming: &Fingerprint) -> Vec<FieldChange> {
    let mut out = Vec::new();
    push_change(&mut out, "name", json!(local.name), json!(incoming.name));
    push_change(&mut out, "wildcard", json!(local.wildcard), json!(incoming.wildcard));
    push_change(
        &mut out,
        "criteria",
        serde_json::to_value(&local.criteria).unwrap_or(Value::Null),
        serde_json::to_value(&incoming.criteria).unwrap_or(Value::Null),
    );
    push_change(
        &mut out,
        "metric_config",
        local.metric_config.clone(),
        incoming.metric_config.clone(),
    );
    out
}

async fn plan_one_rule(
    rule_repo: &RuleRepo,
    b: &BundleRule,
    rebind: &HashMap<Uuid, Uuid>,
) -> anyhow::Result<RulePlan> {
    let mut plan = RulePlan {
        id: b.id,
        rule_name: b.rule_name.clone(),
        status: ItemStatus::Conflict,
        fingerprint_id: b.fingerprint_id,
        changes: Vec::new(),
        local_updated_at: None,
        incoming_updated_at: b.updated_at,
        note: None,
    };

    // A rule may only bind to a fingerprint the bundle carries. Falling back to
    // "whatever this box has under that UUID" would let a bundle assembled by hand
    // point a rule at an unrelated creation shape.
    let Some(&fp_id) = rebind.get(&b.fingerprint_id) else {
        plan.note = Some(format!(
            "bundle does not carry its fingerprint ({}) — re-export the rule",
            b.fingerprint_id
        ));
        return Ok(plan);
    };
    plan.fingerprint_id = fp_id;

    // Canonical params, or the metric registry's own rejection. Canonicalizing
    // BEFORE the diff is what stops an author's JSON key order from reading as a
    // change on every paste — stored params are already canonical.
    let params = match RuleParams::parse(&b.params) {
        Ok(p) => p.to_value(),
        Err(e) => {
            plan.note = Some(format!("params rejected by this box: {e}"));
            return Ok(plan);
        }
    };
    let tags = normalize_tags(&b.tags);

    let local = rule_repo.find(b.id).await?;
    let (trade_mode, exclude) = match &local {
        Some(l) => (l.trade_mode.clone(), Some(l.id)),
        None => (IMPORT_TRADE_MODE.to_string(), None),
    };

    // Same gate the CRUD writers use, so a bundle cannot create the duplicate the
    // New/Duplicate buttons refuse to.
    let identical = rule_repo
        .find_identical(
            fp_id,
            &trade_mode,
            b.buy_amount_lamports,
            b.max_concurrent_tokens,
            b.max_total_tokens,
            &params,
            exclude,
        )
        .await?;

    match local {
        Some(local) => {
            plan.local_updated_at = Some(local.updated_at);
            // `fingerprint_id` is frozen post-create (`apply_rule_update` will not
            // patch it). A rule that means a different creation shape here is a
            // different rule; renaming it into place would silently retarget it.
            if local.fingerprint_id != fp_id {
                plan.note = Some(format!(
                    "this rule is bound to fingerprint {} here, the bundle binds it to {} \
                     — a rule's fingerprint is frozen after create",
                    local.fingerprint_id, fp_id
                ));
                return Ok(plan);
            }
            if let Some(dup) = identical {
                plan.note = Some(format!(
                    "would become identical to rule \"{}\" ({})",
                    dup.rule_name, dup.id
                ));
                return Ok(plan);
            }
            plan.changes = rule_changes(&local, b, &params, &tags);
            plan.status = if plan.changes.is_empty() {
                ItemStatus::Identical
            } else {
                ItemStatus::Changed
            };
        }
        None => match identical {
            Some(dup) => {
                plan.status = ItemStatus::Duplicate;
                plan.note = Some(format!(
                    "same strategy already here as \"{}\" ({}) under a different id — skipped",
                    dup.rule_name, dup.id
                ));
            }
            None => {
                plan.status = ItemStatus::New;
                plan.note = Some(format!(
                    "lands as {IMPORT_TRADE_MODE}, idle — activate it here deliberately"
                ));
            }
        },
    }
    Ok(plan)
}

fn rule_changes(
    local: &StrategyRule,
    b: &BundleRule,
    canonical_params: &Value,
    tags: &[String],
) -> Vec<FieldChange> {
    let mut out = Vec::new();
    push_change(&mut out, "rule_name", json!(local.rule_name), json!(b.rule_name));
    push_change(
        &mut out,
        "buy_amount_lamports",
        json!(local.buy_amount_lamports),
        json!(b.buy_amount_lamports),
    );
    push_change(
        &mut out,
        "max_concurrent_tokens",
        json!(local.max_concurrent_tokens),
        json!(b.max_concurrent_tokens),
    );
    push_change(
        &mut out,
        "max_total_tokens",
        json!(local.max_total_tokens),
        json!(b.max_total_tokens),
    );
    push_change(&mut out, "params", local.params.clone(), canonical_params.clone());
    // Stored tags are already canonical, so this compares canonical to canonical.
    push_change(&mut out, "tags", json!(local.tags), json!(tags));
    out
}

fn push_change(out: &mut Vec<FieldChange>, field: &str, from: Value, to: Value) {
    if from != to {
        out.push(FieldChange {
            field: field.to_string(),
            from,
            to,
        });
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Apply
// ═══════════════════════════════════════════════════════════════════════════

/// Execute a bundle. Re-plans first (never trusts a plan posted by the client) and
/// refuses the whole thing if anything conflicts, **before the first write**.
pub async fn apply_bundle(
    fp_repo: &FingerprintRepo,
    rule_repo: &RuleRepo,
    bundle: &RuleBundle,
) -> anyhow::Result<Result<BundleApplied, BundlePlan>> {
    let plan = plan_bundle(fp_repo, rule_repo, bundle).await?;
    if plan.blocked {
        return Ok(Err(plan));
    }

    let mut applied = BundleApplied {
        fingerprints_inserted: 0,
        fingerprints_updated: 0,
        rules_inserted: 0,
        rules_updated: 0,
        skipped: 0,
        plan: plan.clone(),
    };

    // Fingerprints first — a rule's FK must resolve.
    for (item, p) in bundle.fingerprints.iter().zip(plan.fingerprints.iter()) {
        match p.status {
            ItemStatus::New | ItemStatus::Changed => {
                let fp = incoming_fingerprint(item).map_err(|e| anyhow::anyhow!(e))?;
                if p.status == ItemStatus::New {
                    fp_repo.insert(&fp).await?;
                    applied.fingerprints_inserted += 1;
                } else {
                    fp_repo.update(&fp).await?;
                    applied.fingerprints_updated += 1;
                }
            }
            _ => applied.skipped += 1,
        }
    }

    for (item, p) in bundle.rules.iter().zip(plan.rules.iter()) {
        match p.status {
            ItemStatus::New => {
                let draft = RuleDraft {
                    rule_name: item.rule_name.clone(),
                    fingerprint_id: p.fingerprint_id,
                    trade_mode: IMPORT_TRADE_MODE.to_string(),
                    buy_amount_lamports: item.buy_amount_lamports,
                    max_concurrent_tokens: item.max_concurrent_tokens,
                    max_total_tokens: item.max_total_tokens,
                    params: item.params.clone(),
                    tags: item.tags.clone(),
                };
                // Keeps the bundle's UUID: that shared id is what makes the next
                // paste in either direction an update rather than a second copy.
                rules::create_with_id(rule_repo, &draft, item.id)
                    .await
                    .map_err(rule_error_to_anyhow)?;
                applied.rules_inserted += 1;
            }
            ItemStatus::Changed => {
                let Some(mut rule) = rule_repo.find(item.id).await? else {
                    anyhow::bail!("rule {} vanished mid-apply", item.id);
                };
                // The patch omits `trade_mode` on purpose, and `apply_rule_update`
                // refuses `is_active` / `is_enabled` / `fingerprint_id` outright —
                // between them, nothing about how this box RUNS the rule can ride
                // in on a bundle. This is the arming guarantee in the module docs.
                rules::apply_rule_update(&mut rule, &rule_patch(item));
                rules::save(rule_repo, &mut rule)
                    .await
                    .map_err(rule_error_to_anyhow)?;
                applied.rules_updated += 1;
            }
            _ => applied.skipped += 1,
        }
    }

    Ok(Ok(applied))
}

/// The PUT-shaped patch body for an existing rule — exactly the travelling fields.
fn rule_patch(b: &BundleRule) -> Value {
    json!({
        "rule_name": b.rule_name,
        "buy_amount_lamports": b.buy_amount_lamports,
        "max_concurrent_tokens": b.max_concurrent_tokens,
        "max_total_tokens": b.max_total_tokens,
        "params": b.params,
        "tags": b.tags,
    })
}

fn rule_error_to_anyhow(e: RuleError) -> anyhow::Error {
    match e {
        RuleError::Invalid(m) => anyhow::anyhow!(m),
        RuleError::Duplicate {
            existing_id,
            rule_name,
        } => anyhow::anyhow!("identical rule already exists: \"{rule_name}\" ({existing_id})"),
        RuleError::Repo(err) => err,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// HTTP edges — both bins serve the same three responses
// ═══════════════════════════════════════════════════════════════════════════

/// Parse a `?rules=<uuid>,<uuid>` query value. `None`/empty selects every rule.
pub fn parse_rule_selection(csv: Option<&str>) -> Result<Option<Vec<Uuid>>, String> {
    let Some(csv) = csv.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let mut out = Vec::new();
    for part in csv.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        out.push(Uuid::parse_str(part).map_err(|_| format!("not a uuid: {part}"))?);
    }
    Ok((!out.is_empty()).then_some(out))
}

/// `GET /api/strategy-bundle` — serialize the selection.
pub async fn export_response(
    fp_repo: &FingerprintRepo,
    rule_repo: &RuleRepo,
    rules_csv: Option<&str>,
    source: &str,
) -> HttpResponse {
    let selection = match parse_rule_selection(rules_csv) {
        Ok(s) => s,
        Err(e) => return HttpResponse::BadRequest().json(json!({ "error": e })),
    };
    match export_bundle(fp_repo, rule_repo, selection.as_deref(), source).await {
        Ok(bundle) => HttpResponse::Ok().json(bundle),
        Err(e) => {
            tracing::warn!("export strategy bundle: {e}");
            HttpResponse::BadRequest().json(json!({ "error": e.to_string() }))
        }
    }
}

/// `POST /api/strategy-bundle/preview` — the diff. Writes nothing.
pub async fn preview_response(
    fp_repo: &FingerprintRepo,
    rule_repo: &RuleRepo,
    body: &Value,
) -> HttpResponse {
    let bundle = match parse_bundle(body) {
        Ok(b) => b,
        Err(e) => return HttpResponse::BadRequest().json(json!({ "error": e })),
    };
    match plan_bundle(fp_repo, rule_repo, &bundle).await {
        Ok(plan) => HttpResponse::Ok().json(plan),
        Err(e) => {
            tracing::warn!("preview strategy bundle: {e}");
            HttpResponse::InternalServerError()
                .json(json!({ "error": format!("preview failed: {e}") }))
        }
    }
}

/// `POST /api/strategy-bundle/apply` — execute. Returns 409 with the plan when a
/// conflict blocked it, so the UI shows the same rows it previewed.
pub async fn apply_response(
    fp_repo: &FingerprintRepo,
    rule_repo: &RuleRepo,
    body: &Value,
) -> (HttpResponse, bool) {
    let bundle = match parse_bundle(body) {
        Ok(b) => b,
        Err(e) => return (HttpResponse::BadRequest().json(json!({ "error": e })), false),
    };
    match apply_bundle(fp_repo, rule_repo, &bundle).await {
        Ok(Ok(applied)) => {
            let wrote = applied.fingerprints_inserted
                + applied.fingerprints_updated
                + applied.rules_inserted
                + applied.rules_updated
                > 0;
            (HttpResponse::Ok().json(applied), wrote)
        }
        Ok(Err(plan)) => (
            HttpResponse::Conflict().json(json!({
                "error": "bundle has conflicts — nothing was applied",
                "plan": plan,
            })),
            false,
        ),
        Err(e) => {
            tracing::warn!("apply strategy bundle: {e}");
            (
                HttpResponse::InternalServerError()
                    .json(json!({ "error": format!("apply failed: {e}") })),
                // A mid-apply failure may still have written the fingerprints, so
                // the caller reloads regardless.
                true,
            )
        }
    }
}

/// Decode + version-gate a pasted bundle.
fn parse_bundle(body: &Value) -> Result<RuleBundle, String> {
    let bundle: RuleBundle =
        serde_json::from_value(body.clone()).map_err(|e| format!("not a strategy bundle: {e}"))?;
    if bundle.bundle_format_version != BUNDLE_FORMAT_VERSION {
        return Err(format!(
            "bundle format v{} — this box reads v{BUNDLE_FORMAT_VERSION}",
            bundle.bundle_format_version
        ));
    }
    Ok(bundle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fp(name: &str, criteria: Value, metric_config: Value) -> BundleFingerprint {
        BundleFingerprint {
            id: Uuid::nil(),
            name: name.into(),
            wildcard: false,
            criteria,
            metric_config,
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn a_bundle_carries_no_arming_or_mode_field() {
        // The safety property, asserted on the wire shape rather than trusted: if
        // any of these ever appeared in `BundleRule`, a paste from the paper lab
        // could arm a real-money rule on the live box.
        let wire = serde_json::to_value(BundleRule {
            id: Uuid::nil(),
            rule_name: "r".into(),
            fingerprint_id: Uuid::nil(),
            buy_amount_lamports: 1,
            max_concurrent_tokens: 0,
            max_total_tokens: 0,
            params: json!({}),
            tags: vec![],
            updated_at: Utc::now(),
        })
        .unwrap();
        for forbidden in ["is_active", "is_enabled", "trade_mode"] {
            assert!(
                wire.get(forbidden).is_none(),
                "{forbidden} must never travel in a bundle"
            );
        }
    }

    #[test]
    fn the_rule_patch_omits_trade_mode() {
        // `apply_rule_update` DOES patch `trade_mode` when the key is present, so
        // the guarantee is this body's shape, not the patcher's.
        let patch = rule_patch(&BundleRule {
            id: Uuid::nil(),
            rule_name: "r".into(),
            fingerprint_id: Uuid::nil(),
            buy_amount_lamports: 1,
            max_concurrent_tokens: 0,
            max_total_tokens: 0,
            params: json!({}),
            tags: vec![],
            updated_at: Utc::now(),
        });
        assert!(patch.get("trade_mode").is_none());
        assert!(patch.get("is_active").is_none());
        assert!(patch.get("fingerprint_id").is_none());
    }

    #[test]
    fn version_gate_rejects_a_foreign_bundle() {
        let mut body = json!({
            "bundle_format_version": BUNDLE_FORMAT_VERSION + 1,
            "exported_at": Utc::now(),
            "fingerprints": [],
            "rules": [],
        });
        assert!(parse_bundle(&body).is_err());
        body["bundle_format_version"] = json!(BUNDLE_FORMAT_VERSION);
        assert!(parse_bundle(&body).is_ok());
    }

    #[test]
    fn an_unknown_axis_is_a_conflict_not_a_silent_widening() {
        // Same rule as the CRUD handlers: a dropped axis reads as "not identity",
        // which WIDENS what the fingerprint matches. The preview must say so.
        let err = incoming_fingerprint(&fp(
            "x",
            json!({ "no_such_axis": { "min": "1", "max": "2" } }),
            json!({}),
        ))
        .unwrap_err();
        assert!(!err.is_empty(), "an unknown axis must be reported");
    }

    #[test]
    fn parse_rule_selection_reads_a_csv_and_defaults_to_all() {
        assert!(parse_rule_selection(None).unwrap().is_none());
        assert!(parse_rule_selection(Some("  ")).unwrap().is_none());
        let id = Uuid::new_v4();
        assert_eq!(
            parse_rule_selection(Some(&format!("{id}, {id}"))).unwrap(),
            Some(vec![id, id])
        );
        assert!(parse_rule_selection(Some("nope")).is_err());
    }

    #[test]
    fn only_new_and_changed_write() {
        assert!(ItemStatus::New.writes());
        assert!(ItemStatus::Changed.writes());
        for quiet in [
            ItemStatus::Identical,
            ItemStatus::ReuseExisting,
            ItemStatus::Duplicate,
            ItemStatus::Conflict,
        ] {
            assert!(!quiet.writes(), "{quiet:?} must not write");
        }
    }
}
