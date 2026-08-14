//! `strategy_arms` — the durable arm ledger's reads and batched writes.
//!
//! One row per `(rule, mint)` arming episode. The in-RAM `ArmedRegistry` answers
//! "what is armed right now" for the Console's Waiting lane; this answers "what
//! was armed over a window" for the Console's Arms section. The two overlap on
//! live episodes on purpose — see `docs/plans/strategies/arm-ledger.md`.
//!
//! **Writes are batched.** The engine sink is on the decision fold's effect
//! drain, so it queues and returns; the writer task calls [`ArmRepo::insert_arms`]
//! / [`ArmRepo::end_arms`] with whatever a flush drained. Both take slices and
//! issue ONE statement — a per-episode round trip would put the arm rate straight
//! onto the connection pool.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::api::table_query::{FilterSpec, TableRequest};
use crate::models::strategy_arm::{ArmBlockedBy, ArmFunnel, StrategyArm, BLOCKED_BY_LIMIT};
use crate::storage::repositories::strategy_repo::{like_escape, push_filter_predicate};
use crate::storage::token_enrichment::FilterKind;

/// Projection for [`StrategyArm`]. `waited_sec` is computed, never stored — a
/// stored copy would go stale on every live episode the moment it is read.
const ARM_COLS: &str = "a.rule_id, a.mint_address, a.mode, a.armed_at, a.ended_at, \
    a.end_reason, a.position_id, a.end_detail, t.symbol";

/// The representative blocker, lifted out of `end_detail`. THE definition — the
/// sort whitelist, the filter whitelist and the summary's `GROUP BY` all
/// reference this const, so a breakdown bar and the rows the column filters to
/// can never describe different populations.
const BLOCKED_BY_SQL: &str = "(a.end_detail ->> 'blocked_by')";

/// Seconds this episode has waited: to its end, or to now while it is live. THE
/// definition — the projection, the sort whitelist and the filter whitelist all
/// reference this const, so the column cannot sort by one fact and filter by
/// another. `EXTRACT` yields `NUMERIC`, so the cast is part of the definition:
/// `waited_sec` decodes as `f64` here and in `PERCENTILE_CONT` over the same
/// expression.
const WAITED_SEC_SQL: &str =
    "EXTRACT(EPOCH FROM (COALESCE(a.ended_at, now()) - a.armed_at))::float8";

/// The episode's "when" instant for a time-range read. `armed_at`, not
/// `ended_at`: the range picker asks "what did the bot look at during this
/// window", and keying on the end would drop every episode still waiting.
const ARM_WHEN_SQL: &str = "a.armed_at";

/// A sort/filter/search request for the arms list, built from the frontend
/// `DataTable`'s emitted view-state. Only whitelisted keys are honored (see
/// [`arm_sort_sql`] / [`arm_filter_sql`]); anything else is dropped, never
/// interpolated, so no user text reaches a SQL identifier. Values bind as
/// parameters.
#[derive(Debug, Clone, Default)]
pub struct ArmQuery {
    /// Free-text over mint / symbol (ILIKE). Empty = no search.
    pub search: String,
    pub filters: Vec<(String, FilterSpec)>,
    pub sort: Vec<(String, bool)>,
    /// `armed_at` window — `from` inclusive, `to` exclusive (mirrors `AnalysisRange`).
    pub time_from: Option<DateTime<Utc>>,
    pub time_to: Option<DateTime<Utc>>,
}

impl From<TableRequest> for ArmQuery {
    fn from(req: TableRequest) -> Self {
        let range = req.range.unwrap_or_default();
        ArmQuery {
            search: req.search,
            filters: req.filters.into_iter().collect(),
            sort: req.sorting.into_iter().map(|s| (s.col, s.dir.is_desc())).collect(),
            time_from: range.from,
            time_to: range.to,
        }
    }
}

/// Frontend column key → trusted SQL sort expression. `None` = not sortable.
/// Aliases: `a` = strategy_arms, `t` = LEFT-JOINed `tokens`.
fn arm_sort_sql(key: &str) -> Option<&'static str> {
    Some(match key {
        "mint_address" => "a.mint_address",
        "symbol" => "t.symbol",
        "rule_id" => "a.rule_id::text",
        "mode" => "a.mode",
        "armed_at" => "a.armed_at",
        "ended_at" => "a.ended_at",
        "end_reason" => "a.end_reason",
        "blocked_by" => BLOCKED_BY_SQL,
        "waited_sec" => WAITED_SEC_SQL,
        _ => return None,
    })
}

/// Frontend column key → trusted SQL expression + type. `None` = not filterable.
fn arm_filter_sql(key: &str) -> Option<(&'static str, FilterKind)> {
    use FilterKind::{Numeric, Text};
    Some(match key {
        "mint_address" => ("a.mint_address", Text),
        "symbol" => ("t.symbol", Text),
        "rule_id" => ("a.rule_id::text", Text),
        "mode" => ("a.mode", Text),
        // NULL while the episode is live, and the UI badges that as "Waiting" —
        // COALESCE or filtering for it never matches a live row (the same trap
        // `exit_reason` has on the positions whitelist).
        "end_reason" => ("COALESCE(a.end_reason, 'waiting')", Text),
        // NULL on every ending but `unsatisfiable` — and deliberately NOT
        // COALESCEd: "no blocker recorded" is absence, not a bucket, and giving
        // it a name would file every `dead` episode under it.
        "blocked_by" => (BLOCKED_BY_SQL, Text),
        "position_id" => ("a.position_id::text", Text),
        "waited_sec" => (WAITED_SEC_SQL, Numeric),
        _ => return None,
    })
}

fn push_arm_where(qb: &mut sqlx::QueryBuilder<sqlx::Postgres>, query: &ArmQuery) {
    let search = query.search.trim();
    if !search.is_empty() {
        let needle = format!("%{}%", like_escape(search));
        qb.push(" AND (a.mint_address ILIKE ")
            .push_bind(needle.clone())
            .push(" OR t.symbol ILIKE ")
            .push_bind(needle)
            .push(")");
    }
    if let Some(from) = query.time_from {
        qb.push(" AND ").push(ARM_WHEN_SQL).push(" >= ").push_bind(from);
    }
    if let Some(to) = query.time_to {
        qb.push(" AND ").push(ARM_WHEN_SQL).push(" < ").push_bind(to);
    }
    for (key, spec) in &query.filters {
        if let Some((col, kind)) = arm_filter_sql(key) {
            push_filter_predicate(qb, col, kind, spec);
        }
    }
}

/// `ORDER BY` from the whitelisted sort keys, falling back to newest-armed. The
/// `(armed_at, rule_id, mint_address)` tiebreaker is the natural key, so paging
/// is stable when the sort column ties.
fn push_arm_order(qb: &mut sqlx::QueryBuilder<sqlx::Postgres>, query: &ArmQuery) {
    let resolved: Vec<(&'static str, bool)> =
        query.sort.iter().filter_map(|(k, desc)| arm_sort_sql(k).map(|s| (s, *desc))).collect();
    qb.push(" ORDER BY ");
    for (sql, desc) in &resolved {
        qb.push(*sql).push(if *desc { " DESC NULLS LAST, " } else { " ASC NULLS LAST, " });
    }
    qb.push("a.armed_at DESC, a.rule_id, a.mint_address");
}

/// One row for [`ArmRepo::insert_arms`]: `(rule_id, mint_address, mode, armed_at)`.
/// A named alias rather than a struct — the writer builds these by draining an
/// [`ArmLedgerWrite`](crate::models::strategy_arm::ArmLedgerWrite) queue, and a
/// second owned type between the two would be pure ceremony.
pub type ArmInsertRow = (Uuid, String, String, DateTime<Utc>);

/// One row for [`ArmRepo::end_arms`]: the episode key
/// (`rule_id`, `mint_address`, `armed_at`) plus how it ended.
pub type ArmEndRow = (
    Uuid,
    String,
    DateTime<Utc>,
    DateTime<Utc>,
    String,
    Option<Uuid>,
    Option<serde_json::Value>,
);

#[derive(Clone)]
pub struct ArmRepo {
    pool: PgPool,
}

impl ArmRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    // -- Writes (batched; called only by the ledger writer task) --------------

    /// Insert one flush's worth of new episodes in a single statement.
    ///
    /// `ON CONFLICT DO NOTHING` on the natural key: a re-arm at the same instant
    /// for the same pair is the same episode, and the engine can legitimately
    /// re-emit `Armed` after a rules reload.
    pub async fn insert_arms(&self, rows: &[ArmInsertRow]) -> anyhow::Result<u64> {
        if rows.is_empty() {
            return Ok(0);
        }
        let mut qb: sqlx::QueryBuilder<sqlx::Postgres> = sqlx::QueryBuilder::new(
            "INSERT INTO strategy_arms (rule_id, mint_address, mode, armed_at) ",
        );
        qb.push_values(rows, |mut b, (rule_id, mint, mode, armed_at)| {
            b.push_bind(*rule_id).push_bind(mint).push_bind(mode).push_bind(*armed_at);
        });
        qb.push(" ON CONFLICT (armed_at, rule_id, mint_address) DO NOTHING");
        Ok(qb.build().execute(&self.pool).await?.rows_affected())
    }

    /// Close one flush's worth of episodes in a single statement.
    ///
    /// `WHERE ended_at IS NULL` makes the write idempotent and keeps the FIRST
    /// ending: a token can go dead in the same drain as the rule being paused,
    /// and the reason that actually ended the episode is the earlier one.
    pub async fn end_arms(&self, rows: &[ArmEndRow]) -> anyhow::Result<u64> {
        if rows.is_empty() {
            return Ok(0);
        }
        let mut qb: sqlx::QueryBuilder<sqlx::Postgres> = sqlx::QueryBuilder::new(
            "UPDATE strategy_arms a SET ended_at = v.ended_at, \
             end_reason = v.end_reason, position_id = v.position_id, \
             end_detail = v.end_detail FROM (",
        );
        qb.push_values(rows, |mut b, (rule_id, mint, armed_at, ended_at, reason, pos, detail)| {
            b.push_bind(*rule_id)
                .push_bind(mint)
                .push_bind(*armed_at)
                .push_bind(*ended_at)
                .push_bind(reason)
                .push_bind(*pos)
                // `VALUES` gives an all-NULL column no type to infer, and every
                // ending but `unsatisfiable` is NULL — so the cast is load-bearing,
                // not decoration.
                .push_bind(detail)
                .push_unseparated("::jsonb");
        });
        qb.push(
            ") AS v(rule_id, mint_address, armed_at, ended_at, end_reason, position_id, end_detail) \
             WHERE a.rule_id = v.rule_id AND a.mint_address = v.mint_address \
             AND a.armed_at = v.armed_at AND a.ended_at IS NULL",
        );
        Ok(qb.build().execute(&self.pool).await?.rows_affected())
    }

    // -- Reads ----------------------------------------------------------------

    /// One page of arming episodes, newest-armed first by default. LEFT-JOINs
    /// `tokens` for the symbol column and the symbol half of the search — the
    /// join is outer because the ledger and `tokens` expire on different clocks.
    pub async fn arms_paged(
        &self,
        limit: i64,
        offset: i64,
        query: &ArmQuery,
    ) -> anyhow::Result<Vec<StrategyArm>> {
        let mut qb: sqlx::QueryBuilder<sqlx::Postgres> = sqlx::QueryBuilder::new(format!(
            "SELECT {ARM_COLS}, {WAITED_SEC_SQL} AS waited_sec FROM strategy_arms a \
             LEFT JOIN tokens t ON t.mint_address = a.mint_address WHERE TRUE"
        ));
        push_arm_where(&mut qb, query);
        push_arm_order(&mut qb, query);
        qb.push(" LIMIT ").push_bind(limit).push(" OFFSET ").push_bind(offset);
        Ok(qb.build_query_as::<StrategyArm>().fetch_all(&self.pool).await?)
    }

    /// Filtered count — same JOIN + WHERE as [`ArmRepo::arms_paged`], so the
    /// pager total tracks the page exactly.
    pub async fn count_arms(&self, query: &ArmQuery) -> anyhow::Result<i64> {
        let mut qb: sqlx::QueryBuilder<sqlx::Postgres> = sqlx::QueryBuilder::new(
            "SELECT COUNT(*) FROM strategy_arms a \
             LEFT JOIN tokens t ON t.mint_address = a.mint_address WHERE TRUE",
        );
        push_arm_where(&mut qb, query);
        let (n,): (i64,) = qb.build_query_as().fetch_one(&self.pool).await?;
        Ok(n)
    }

    /// The funnel over the same cohort — aggregated in Postgres, no rows shipped.
    /// Every count comes from ONE scan so the parts can never disagree with the
    /// total the pager shows.
    pub async fn arm_funnel(&self, query: &ArmQuery) -> anyhow::Result<ArmFunnel> {
        let mut qb: sqlx::QueryBuilder<sqlx::Postgres> = sqlx::QueryBuilder::new(format!(
            "SELECT COUNT(*) AS armed, \
             COUNT(*) FILTER (WHERE a.end_reason = 'entered') AS entered, \
             COUNT(*) FILTER (WHERE a.ended_at IS NULL) AS live, \
             COUNT(*) FILTER (WHERE a.end_reason = 'dead') AS dead, \
             COUNT(*) FILTER (WHERE a.end_reason = 'migrated') AS migrated, \
             COUNT(*) FILTER (WHERE a.end_reason = 'unsatisfiable') AS unsatisfiable, \
             COUNT(*) FILTER (WHERE a.end_reason = 'paused') AS paused, \
             COUNT(*) FILTER (WHERE a.end_reason = 'duplicate_identity') AS duplicate_identity, \
             0::float8 AS entry_rate_pct, \
             PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY {WAITED_SEC_SQL}) \
               FILTER (WHERE a.ended_at IS NOT NULL) AS median_waited_sec \
             FROM strategy_arms a \
             LEFT JOIN tokens t ON t.mint_address = a.mint_address WHERE TRUE"
        ));
        push_arm_where(&mut qb, query);
        let funnel = qb.build_query_as::<ArmFunnel>().fetch_one(&self.pool).await?;
        Ok(funnel.with_entry_rate())
    }

    /// Which entry condition held the cohort's `unsatisfiable` episodes out, by
    /// count — the breakdown behind that tile.
    ///
    /// Its own statement rather than another `COUNT(*) FILTER` on
    /// [`ArmRepo::arm_funnel`]: this one groups by a value only the rows know, so
    /// it cannot be a column of a fixed-shape aggregate. Same JOIN and same WHERE,
    /// so it counts exactly the population that funnel describes.
    ///
    /// `WHERE <blocked_by> IS NOT NULL` also drops the episodes that ran out of
    /// clock with **everything else satisfied** — they have a detail but no
    /// blocker, and they are not a bucket (see `entry_blockers_json`).
    pub async fn arm_blocked_by(&self, query: &ArmQuery) -> anyhow::Result<Vec<ArmBlockedBy>> {
        let mut qb: sqlx::QueryBuilder<sqlx::Postgres> = sqlx::QueryBuilder::new(format!(
            "SELECT {BLOCKED_BY_SQL} AS blocked_by, COUNT(*) AS n FROM strategy_arms a \
             LEFT JOIN tokens t ON t.mint_address = a.mint_address \
             WHERE {BLOCKED_BY_SQL} IS NOT NULL"
        ));
        push_arm_where(&mut qb, query);
        qb.push(format!(
            " GROUP BY {BLOCKED_BY_SQL} ORDER BY n DESC, blocked_by LIMIT {BLOCKED_BY_LIMIT}"
        ));
        Ok(qb.build_query_as::<ArmBlockedBy>().fetch_all(&self.pool).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The waited-seconds column must resolve to the SAME expression on both
    /// whitelists — a sort and a filter that disagree would page rows the filter
    /// then hides.
    #[test]
    fn waited_sec_sorts_and_filters_on_one_expression() {
        assert_eq!(arm_sort_sql("waited_sec"), Some(WAITED_SEC_SQL));
        assert_eq!(arm_filter_sql("waited_sec").map(|(sql, _)| sql), Some(WAITED_SEC_SQL));
    }

    /// Same contract as `waited_sec`, and it matters more here: the summary's
    /// breakdown groups by this expression and the column filters by it, so a
    /// divergence would show a bar whose rows the filter then can't produce.
    #[test]
    fn blocked_by_sorts_filters_and_groups_on_one_expression() {
        assert_eq!(arm_sort_sql("blocked_by"), Some(BLOCKED_BY_SQL));
        assert_eq!(arm_filter_sql("blocked_by").map(|(sql, _)| sql), Some(BLOCKED_BY_SQL));
    }

    /// A missing blocker is absence, not a bucket. COALESCEing it would file
    /// every `dead` / `migrated` / `paused` episode under one invented name.
    #[test]
    fn blocked_by_filter_does_not_coalesce() {
        let (sql, _) = arm_filter_sql("blocked_by").expect("whitelisted");
        assert!(!sql.contains("COALESCE"), "absence would become a bucket: {sql}");
    }

    /// Unknown keys are dropped, never interpolated — the injection contract.
    #[test]
    fn unknown_keys_resolve_to_nothing() {
        assert_eq!(arm_sort_sql("a.mint_address; DROP TABLE tokens"), None);
        assert!(arm_filter_sql("'; DELETE FROM strategy_arms --").is_none());
    }

    /// A live episode has no `end_reason`, so the filter column must COALESCE or
    /// the "Waiting" chip matches nothing.
    #[test]
    fn end_reason_filter_covers_live_episodes() {
        let (sql, _) = arm_filter_sql("end_reason").expect("whitelisted");
        assert!(sql.contains("COALESCE"), "live episodes would be unfilterable: {sql}");
    }
}
