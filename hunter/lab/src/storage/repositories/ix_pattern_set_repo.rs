//! Persistence for **analysis-owned** pattern sets (`ix_pattern_sets`).
//!
//! The engine classifies flow from a fingerprint's `metric_config`; a trader
//! study has no fingerprint, so its charts have nothing to classify with. A
//! pattern set is the study-surface owner of that fact (Trader Analysis' flow
//! lens). Lab-only: `live` neither reads nor writes this table.
//!
//! A set is **one vocabulary**, chosen at insert (`kind`) and never updated:
//!
//! * `exact` — ordered `ix_labels` plus optional fee pins. Identity is the
//!   labels **and** the pins (same catch-all-vs-pin rule as a fingerprint's
//!   `ix_patterns`). `group` only labels a subset for the lens' narrowing UI.
//! * `templates` — `working_templates` grain ids (`program|CU|ATA|N|S|F`).
//!   No fee pins (the grain already encodes `|CU` as presence, not a budget).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

/// Hard ceiling on a set's size. A lens is a hand-derived shortlist; a paste
/// with thousands of sequences is a mistake (usually a raw trades dump), and
/// classification is a per-trade set lookup on every chart in the grid.
pub const MAX_PATTERNS: usize = 500;

/// Which vocabulary a set stores. Set at insert; the picker is the switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum IxPatternSetKind {
    #[default]
    Exact,
    Templates,
}

impl IxPatternSetKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Templates => "templates",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "templates" => Self::Templates,
            _ => Self::Exact,
        }
    }
}

/// One exact `ix_labels` sequence, optional fee pins, plus the group label the
/// lens narrows on. Unused on a `templates` set (that list lives in
/// `working_templates`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IxPattern {
    /// Free-text subset label (a launch client / aggregator name, typically).
    /// `None` means ungrouped. Never matched against — see the module doc.
    #[serde(default)]
    pub group: Option<String>,
    /// EXACT ordered instruction labels, verbatim from `trades.ix_labels`.
    #[serde(default)]
    pub ix_labels: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cu_limit: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cu_price: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tip_lamports: Option<i64>,
}

impl IxPattern {
    fn pins_fee(&self) -> bool {
        self.cu_limit.is_some() || self.cu_price.is_some() || self.tip_lamports.is_some()
    }

    fn clean_fee(v: Option<i64>) -> Option<i64> {
        v.filter(|&n| n >= 0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IxPatternSet {
    pub id: Uuid,
    pub name: String,
    pub wallet_address: Option<String>,
    pub kind: IxPatternSetKind,
    pub patterns: Vec<IxPattern>,
    pub working_templates: Vec<String>,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// The writable half of a set — what a create/update body carries.
#[derive(Debug, Clone, Deserialize)]
pub struct IxPatternSetDraft {
    pub name: String,
    #[serde(default)]
    pub wallet_address: Option<String>,
    #[serde(default)]
    pub kind: IxPatternSetKind,
    #[serde(default)]
    pub patterns: Vec<IxPattern>,
    #[serde(default)]
    pub working_templates: Vec<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

/// What `validate` hands the writer — one list populated, the other empty.
pub struct SanitizedSet {
    pub kind: IxPatternSetKind,
    pub patterns: Vec<IxPattern>,
    pub working_templates: Vec<String>,
}

fn labels_key(labels: &[String]) -> String {
    // `\u{1}` can't occur in a label, so joining on it keeps the identity
    // exact (a plain separator would merge ["a|b"] with ["a","b"]).
    labels.join("\u{1}")
}

fn row_key(labels: &[String], p: &IxPattern) -> String {
    format!(
        "{}\u{2}{}|{}|{}",
        labels_key(labels),
        p.cu_limit.map(|n| n.to_string()).unwrap_or_default(),
        p.cu_price.map(|n| n.to_string()).unwrap_or_default(),
        p.tip_lamports.map(|n| n.to_string()).unwrap_or_default(),
    )
}

/// Drop blank labels/sequences and exact duplicates, keeping first-seen order.
/// Identity is labels **plus** pins: the same sequence unpinned and pinned are
/// two rows. The same sequence under two group labels is ONE pattern. A
/// catch-all of a shape drops any pins of that shape (the engine would ignore
/// them — a catch-all already matches every budget).
pub fn sanitize_patterns(patterns: Vec<IxPattern>) -> Vec<IxPattern> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(patterns.len());
    for p in patterns {
        let labels: Vec<String> = p
            .ix_labels
            .into_iter()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
        if labels.is_empty() {
            continue;
        }
        let cleaned = IxPattern {
            group: p
                .group
                .map(|g| g.trim().to_string())
                .filter(|g| !g.is_empty()),
            ix_labels: labels.clone(),
            cu_limit: IxPattern::clean_fee(p.cu_limit),
            cu_price: IxPattern::clean_fee(p.cu_price),
            tip_lamports: IxPattern::clean_fee(p.tip_lamports),
        };
        if !seen.insert(row_key(&labels, &cleaned)) {
            continue;
        }
        out.push(cleaned);
    }
    collapse_catchall_vs_pins(out)
}

/// Per labels-shape: if any row is unpinned, keep only the first unpinned and
/// drop the pins. Otherwise keep every pin.
fn collapse_catchall_vs_pins(patterns: Vec<IxPattern>) -> Vec<IxPattern> {
    let mut has_wild: std::collections::HashSet<String> = std::collections::HashSet::new();
    for p in &patterns {
        if !p.pins_fee() {
            has_wild.insert(labels_key(&p.ix_labels));
        }
    }
    if has_wild.is_empty() {
        return patterns;
    }
    let mut seen_wild = std::collections::HashSet::new();
    patterns
        .into_iter()
        .filter(|p| {
            let k = labels_key(&p.ix_labels);
            if !has_wild.contains(&k) {
                return true;
            }
            if p.pins_fee() {
                return false;
            }
            seen_wild.insert(k)
        })
        .collect()
}

/// Unique trimmed grain ids, first-seen order.
pub fn sanitize_templates(templates: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(templates.len());
    for t in templates {
        let id = t.trim().to_string();
        if id.is_empty() || !seen.insert(id.clone()) {
            continue;
        }
        out.push(id);
    }
    out
}

/// Reject what a set cannot be stored as. The inactive list is emptied so a
/// templates set cannot carry leftover exact rows (and the reverse).
pub fn validate(draft: &IxPatternSetDraft) -> Result<SanitizedSet, String> {
    if draft.name.trim().is_empty() {
        return Err("name is required".into());
    }
    match draft.kind {
        IxPatternSetKind::Exact => {
            let patterns = sanitize_patterns(draft.patterns.clone());
            if patterns.len() > MAX_PATTERNS {
                return Err(format!(
                    "{} patterns exceeds the {MAX_PATTERNS} cap",
                    patterns.len()
                ));
            }
            Ok(SanitizedSet {
                kind: IxPatternSetKind::Exact,
                patterns,
                working_templates: Vec::new(),
            })
        }
        IxPatternSetKind::Templates => {
            let working_templates = sanitize_templates(draft.working_templates.clone());
            if working_templates.len() > MAX_PATTERNS {
                return Err(format!(
                    "{} templates exceeds the {MAX_PATTERNS} cap",
                    working_templates.len()
                ));
            }
            Ok(SanitizedSet {
                kind: IxPatternSetKind::Templates,
                patterns: Vec::new(),
                working_templates,
            })
        }
    }
}

#[derive(sqlx::FromRow)]
struct Row {
    id: Uuid,
    name: String,
    wallet_address: Option<String>,
    kind: String,
    patterns: sqlx::types::Json<Vec<IxPattern>>,
    working_templates: sqlx::types::Json<Vec<String>>,
    notes: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<Row> for IxPatternSet {
    fn from(r: Row) -> Self {
        Self {
            id: r.id,
            name: r.name,
            wallet_address: r.wallet_address,
            kind: IxPatternSetKind::parse(&r.kind),
            patterns: r.patterns.0,
            working_templates: r.working_templates.0,
            notes: r.notes,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

const COLS: &str =
    "id, name, wallet_address, kind, patterns, working_templates, notes, created_at, updated_at";

pub struct IxPatternSetRepo {
    pool: PgPool,
}

impl IxPatternSetRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Every set, most-recently-updated first (the picker's order — a lens under
    /// active authoring is the one you reach for next).
    pub async fn list(&self) -> Result<Vec<IxPatternSet>, sqlx::Error> {
        let rows: Vec<Row> = sqlx::query_as(&format!(
            "SELECT {COLS} FROM ix_pattern_sets ORDER BY updated_at DESC"
        ))
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    pub async fn find(&self, id: Uuid) -> Result<Option<IxPatternSet>, sqlx::Error> {
        let row: Option<Row> =
            sqlx::query_as(&format!("SELECT {COLS} FROM ix_pattern_sets WHERE id = $1"))
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(Into::into))
    }

    pub async fn insert(
        &self,
        draft: &IxPatternSetDraft,
        sanitized: &SanitizedSet,
    ) -> Result<IxPatternSet, sqlx::Error> {
        let row: Row = sqlx::query_as(&format!(
            "INSERT INTO ix_pattern_sets
                (id, name, wallet_address, kind, patterns, working_templates, notes)
             VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING {COLS}"
        ))
        .bind(Uuid::new_v4())
        .bind(draft.name.trim())
        .bind(draft.wallet_address.as_deref().map(str::trim))
        .bind(sanitized.kind.as_str())
        .bind(sqlx::types::Json(&sanitized.patterns))
        .bind(sqlx::types::Json(&sanitized.working_templates))
        .bind(draft.notes.as_deref())
        .fetch_one(&self.pool)
        .await?;
        Ok(row.into())
    }

    /// Full replace of the writable half **except kind** — kind is insert-only,
    /// so switching vocabulary is a new set (the picker is the switch).
    /// `updated_at` is stamped here rather than by a trigger so the list's
    /// order tracks the last edit.
    pub async fn update(
        &self,
        id: Uuid,
        draft: &IxPatternSetDraft,
        sanitized: &SanitizedSet,
    ) -> Result<Option<IxPatternSet>, sqlx::Error> {
        let row: Option<Row> = sqlx::query_as(&format!(
            "UPDATE ix_pattern_sets
                SET name = $2, wallet_address = $3, patterns = $4,
                    working_templates = $5, notes = $6, updated_at = now()
              WHERE id = $1 RETURNING {COLS}"
        ))
        .bind(id)
        .bind(draft.name.trim())
        .bind(draft.wallet_address.as_deref().map(str::trim))
        .bind(sqlx::types::Json(&sanitized.patterns))
        .bind(sqlx::types::Json(&sanitized.working_templates))
        .bind(draft.notes.as_deref())
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(Into::into))
    }

    pub async fn delete(&self, id: Uuid) -> Result<bool, sqlx::Error> {
        let res = sqlx::query("DELETE FROM ix_pattern_sets WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(res.rows_affected() > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(group: Option<&str>, labels: &[&str]) -> IxPattern {
        IxPattern {
            group: group.map(str::to_string),
            ix_labels: labels.iter().map(|s| s.to_string()).collect(),
            ..IxPattern::default()
        }
    }

    fn pinned(labels: &[&str], cu_limit: i64) -> IxPattern {
        IxPattern {
            ix_labels: labels.iter().map(|s| s.to_string()).collect(),
            cu_limit: Some(cu_limit),
            ..IxPattern::default()
        }
    }

    fn draft(name: &str, kind: IxPatternSetKind) -> IxPatternSetDraft {
        IxPatternSetDraft {
            name: name.into(),
            wallet_address: None,
            kind,
            patterns: vec![],
            working_templates: vec![],
            notes: None,
        }
    }

    #[test]
    fn sanitize_drops_blanks_and_dupes_keeping_order() {
        let out = sanitize_patterns(vec![
            p(Some(" Axiom "), &["A", " B "]),
            p(None, &[]),
            p(Some("GMGN"), &["A", "B"]), // same sequence, other group => one pattern
            p(None, &["", "  "]),
            p(Some(""), &["C"]),
        ]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].group.as_deref(), Some("Axiom"));
        assert_eq!(out[0].ix_labels, vec!["A", "B"]);
        assert_eq!(out[1].group, None);
        assert_eq!(out[1].ix_labels, vec!["C"]);
    }

    #[test]
    fn order_inside_a_pattern_is_identity() {
        let out = sanitize_patterns(vec![p(None, &["A", "B"]), p(None, &["B", "A"])]);
        assert_eq!(out.len(), 2, "reordered labels are a DIFFERENT structure");
    }

    #[test]
    fn unpinned_and_pinned_same_labels_are_two_rows_until_catchall_wins() {
        let only_pins = sanitize_patterns(vec![pinned(&["A"], 300_000), pinned(&["A"], 200_000)]);
        assert_eq!(only_pins.len(), 2);

        let mixed = sanitize_patterns(vec![
            pinned(&["A"], 300_000),
            p(None, &["A"]),
            pinned(&["A"], 200_000),
        ]);
        assert_eq!(mixed.len(), 1);
        assert!(mixed[0].cu_limit.is_none());
    }

    #[test]
    fn sanitize_templates_trims_and_dedupes() {
        assert_eq!(
            sanitize_templates(vec![
                " Axiom Trade|CU ".into(),
                "".into(),
                "Axiom Trade|CU".into(),
                "GMGN|ATA".into(),
            ]),
            vec!["Axiom Trade|CU", "GMGN|ATA"]
        );
    }

    #[test]
    fn validate_requires_a_name() {
        assert!(validate(&draft("  ", IxPatternSetKind::Exact)).is_err());
    }

    #[test]
    fn validate_exact_drops_templates_and_templates_drops_patterns() {
        let mut exact = draft("e", IxPatternSetKind::Exact);
        exact.patterns = vec![p(None, &["A"])];
        exact.working_templates = vec!["Axiom Trade|CU".into()];
        let s = validate(&exact).unwrap();
        assert_eq!(s.kind, IxPatternSetKind::Exact);
        assert_eq!(s.patterns.len(), 1);
        assert!(s.working_templates.is_empty());

        let mut tmpl = draft("t", IxPatternSetKind::Templates);
        tmpl.patterns = vec![p(None, &["A"])];
        tmpl.working_templates = vec!["Axiom Trade|CU".into(), "Axiom Trade|CU".into()];
        let s = validate(&tmpl).unwrap();
        assert_eq!(s.kind, IxPatternSetKind::Templates);
        assert!(s.patterns.is_empty());
        assert_eq!(s.working_templates, vec!["Axiom Trade|CU"]);
    }
}
