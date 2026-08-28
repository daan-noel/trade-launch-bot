//! Persistence for **analysis-owned** `ix_labels` pattern sets (`ix_pattern_sets`).
//!
//! The engine classifies flow from a fingerprint's `metric_config.m_flow_ix.
//! ix_patterns`; a trader study has no fingerprint, so its charts have
//! nothing to classify with. A pattern set is the same fact — ordered label
//! sequences — owned by the study surface instead (Trader Analysis' flow lens).
//! Lab-only: `live` neither reads nor writes this table.
//!
//! Everything the classifier cares about is the ordered `ix_labels` array;
//! `group` only labels a subset for the lens' own narrowing UI.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

/// Hard ceiling on a set's size. A lens is a hand-derived shortlist; a paste
/// with thousands of sequences is a mistake (usually a raw trades dump), and
/// classification is a per-trade set lookup on every chart in the grid.
pub const MAX_PATTERNS: usize = 500;

/// One ordered `ix_labels` sequence plus the group label the lens narrows on.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IxPattern {
    /// Free-text subset label (a launch client / aggregator name, typically).
    /// `None` means ungrouped. Never matched against — see the module doc.
    #[serde(default)]
    pub group: Option<String>,
    /// EXACT ordered instruction labels, verbatim from `trades.ix_labels`.
    pub ix_labels: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IxPatternSet {
    pub id: Uuid,
    pub name: String,
    pub wallet_address: Option<String>,
    pub patterns: Vec<IxPattern>,
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
    pub patterns: Vec<IxPattern>,
    #[serde(default)]
    pub notes: Option<String>,
}

/// Drop blank labels/sequences and exact duplicates, keeping first-seen order.
/// Identity is the ordered label array alone: the same sequence under two group
/// labels is ONE pattern, because that is what the classifier sees.
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
        // `\u{1}` can't occur in a label, so joining on it keeps the identity
        // exact (a plain separator would merge ["a|b"] with ["a","b"]).
        if labels.is_empty() || !seen.insert(labels.join("\u{1}")) {
            continue;
        }
        let group = p
            .group
            .map(|g| g.trim().to_string())
            .filter(|g| !g.is_empty());
        out.push(IxPattern {
            group,
            ix_labels: labels,
        });
    }
    out
}

/// Reject what a set cannot be stored as. Returns the sanitized patterns.
pub fn validate(draft: &IxPatternSetDraft) -> Result<Vec<IxPattern>, String> {
    if draft.name.trim().is_empty() {
        return Err("name is required".into());
    }
    let patterns = sanitize_patterns(draft.patterns.clone());
    if patterns.len() > MAX_PATTERNS {
        return Err(format!(
            "{} patterns exceeds the {MAX_PATTERNS} cap",
            patterns.len()
        ));
    }
    Ok(patterns)
}

#[derive(sqlx::FromRow)]
struct Row {
    id: Uuid,
    name: String,
    wallet_address: Option<String>,
    patterns: sqlx::types::Json<Vec<IxPattern>>,
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
            patterns: r.patterns.0,
            notes: r.notes,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

const COLS: &str = "id, name, wallet_address, patterns, notes, created_at, updated_at";

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
        patterns: &[IxPattern],
    ) -> Result<IxPatternSet, sqlx::Error> {
        let row: Row = sqlx::query_as(&format!(
            "INSERT INTO ix_pattern_sets (id, name, wallet_address, patterns, notes)
             VALUES ($1, $2, $3, $4, $5) RETURNING {COLS}"
        ))
        .bind(Uuid::new_v4())
        .bind(draft.name.trim())
        .bind(draft.wallet_address.as_deref().map(str::trim))
        .bind(sqlx::types::Json(patterns))
        .bind(draft.notes.as_deref())
        .fetch_one(&self.pool)
        .await?;
        Ok(row.into())
    }

    /// Full replace of the writable half. `updated_at` is stamped here rather
    /// than by a trigger so the list's order tracks the last edit.
    pub async fn update(
        &self,
        id: Uuid,
        draft: &IxPatternSetDraft,
        patterns: &[IxPattern],
    ) -> Result<Option<IxPatternSet>, sqlx::Error> {
        let row: Option<Row> = sqlx::query_as(&format!(
            "UPDATE ix_pattern_sets
                SET name = $2, wallet_address = $3, patterns = $4, notes = $5,
                    updated_at = now()
              WHERE id = $1 RETURNING {COLS}"
        ))
        .bind(id)
        .bind(draft.name.trim())
        .bind(draft.wallet_address.as_deref().map(str::trim))
        .bind(sqlx::types::Json(patterns))
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
    fn validate_requires_a_name() {
        let draft = IxPatternSetDraft {
            name: "  ".into(),
            wallet_address: None,
            patterns: vec![],
            notes: None,
        };
        assert!(validate(&draft).is_err());
    }
}
