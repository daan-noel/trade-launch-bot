//! Data migrations — row rewrites SQL cannot express, run once per database right after
//! the schema migrations, each in one transaction and recorded in `_data_migrations`.
//!
//! **`metric_system_v2`** converts every stored v1 document to v2 with the engine's own
//! converter ([`hunter_engine::v1`]): fingerprint `tags` (was `metric_config`) and
//! criteria, rule `params`, and each run's `params_snapshot`. A fingerprint or rule that
//! fails to convert — or converts to a tags document the v2 validator refuses — aborts
//! the whole migration, naming the row: nothing is half-converted. A run snapshot that
//! fails is left as written (it is history, and the readout falls back to the rule's
//! current params). Every `Running` run's `config_hash` is cleared: it digests the v1
//! JSON, so a resumed run would otherwise record the format change as an operator edit;
//! a run with no hash takes its current config as the baseline.
//!
//! Dry run: [`convert_metric_system_v2`] with `commit = false` converts and validates
//! every row inside a transaction that is then rolled back, and reports what it would
//! change. It reads either schema — `tags`, or `metric_config` on a database the schema
//! migration has not reached — so `hunter-lab migrate-v2 --dry-run` can check the
//! server's rows before the deploy that migrates them.

use serde_json::Value;
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// The name `_data_migrations` records.
pub const METRIC_SYSTEM_V2: &str = "metric_system_v2";

/// What a conversion changed (or, dry, would change).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct V2Report {
    pub fingerprints: usize,
    pub rules: usize,
    pub run_snapshots: usize,
    /// Run snapshots left as written because they would not convert.
    pub run_snapshots_kept: Vec<String>,
    /// Running runs whose `config_hash` was cleared.
    pub run_hashes_cleared: u64,
}

/// Run every pending data migration. Called at boot after the schema migrations.
pub async fn run(pool: &PgPool) -> anyhow::Result<()> {
    let done: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM _data_migrations WHERE name = $1)")
        .bind(METRIC_SYSTEM_V2)
        .fetch_one(pool)
        .await?;
    if !done {
        let report = convert_metric_system_v2(pool, true).await?;
        tracing::info!(
            fingerprints = report.fingerprints,
            rules = report.rules,
            run_snapshots = report.run_snapshots,
            kept = report.run_snapshots_kept.len(),
            hashes_cleared = report.run_hashes_cleared,
            "data migration {METRIC_SYSTEM_V2} applied"
        );
    }
    Ok(())
}

/// The fingerprint column holding the tags document: `tags`, or `metric_config` before
/// core `0021`. Only ever one of two literals, so it can be formatted into SQL.
async fn fingerprint_doc_column(pool: &PgPool) -> anyhow::Result<&'static str> {
    let has_tags: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM information_schema.columns \
         WHERE table_schema = current_schema() AND table_name = 'fingerprints' AND column_name = 'tags')",
    )
    .fetch_one(pool)
    .await?;
    Ok(if has_tags { "tags" } else { "metric_config" })
}

/// Convert every v1 row to v2. `commit = false` rolls the transaction back: a dry run.
pub async fn convert_metric_system_v2(pool: &PgPool, commit: bool) -> anyhow::Result<V2Report> {
    let col = fingerprint_doc_column(pool).await?;
    if commit && col != "tags" {
        anyhow::bail!("{METRIC_SYSTEM_V2}: the schema migration (core 0021) has not run");
    }
    let mut tx = pool.begin().await?;
    let mut report = V2Report::default();

    let fps = sqlx::query(&format!("SELECT id, name, criteria, {col} AS doc FROM fingerprints"))
        .fetch_all(&mut *tx)
        .await?;
    for row in fps {
        let (id, name): (Uuid, String) = (row.try_get("id")?, row.try_get("name")?);
        let criteria: Value = row.try_get("criteria")?;
        let doc: Value = row.try_get("doc")?;
        let new_criteria = hunter_engine::v1::convert_criteria(&criteria);
        let new_doc = if hunter_engine::v1::is_v1_metric_config(&doc) {
            hunter_engine::v1::convert_metric_config(&doc).map_err(|e| anyhow::anyhow!("fingerprint {name} ({id}): {e}"))?
        } else {
            doc.clone()
        };
        hunter_engine::metrics::tags::config::validate_tags(&new_doc)
            .map_err(|e| anyhow::anyhow!("fingerprint {name} ({id}): the converted tags do not validate: {e}"))?;
        if new_criteria != criteria || new_doc != doc {
            sqlx::query(&format!("UPDATE fingerprints SET criteria = $2, {col} = $3 WHERE id = $1"))
                .bind(id)
                .bind(&new_criteria)
                .bind(&new_doc)
                .execute(&mut *tx)
                .await
                .map_err(|e| anyhow::anyhow!("fingerprint {name} ({id}): {e}"))?;
            report.fingerprints += 1;
        }
    }

    let rules = sqlx::query("SELECT id, rule_name, params FROM strategy_rules").fetch_all(&mut *tx).await?;
    for row in rules {
        let (id, name): (Uuid, String) = (row.try_get("id")?, row.try_get("rule_name")?);
        let params: Value = row.try_get("params")?;
        if !hunter_engine::v1::is_v1_params(&params) {
            continue;
        }
        let v2 = hunter_engine::v1::parse_params_any(&params)
            .map_err(|e| anyhow::anyhow!("rule {name} ({id}): {e}"))?
            .to_value();
        sqlx::query("UPDATE strategy_rules SET params = $2 WHERE id = $1").bind(id).bind(&v2).execute(&mut *tx).await?;
        report.rules += 1;
    }

    let runs = sqlx::query("SELECT id, params_snapshot FROM strategy_runs").fetch_all(&mut *tx).await?;
    for row in runs {
        let id: Uuid = row.try_get("id")?;
        let snap: Value = row.try_get("params_snapshot")?;
        if !hunter_engine::v1::is_v1_params(&snap) {
            continue;
        }
        match hunter_engine::v1::parse_params_any(&snap) {
            Ok(p) => {
                sqlx::query("UPDATE strategy_runs SET params_snapshot = $2 WHERE id = $1")
                    .bind(id)
                    .bind(p.to_value())
                    .execute(&mut *tx)
                    .await?;
                report.run_snapshots += 1;
            }
            Err(e) => report.run_snapshots_kept.push(format!("run {id}: {e}")),
        }
    }

    report.run_hashes_cleared = sqlx::query(
        "UPDATE strategy_runs SET config_hash = NULL WHERE status = 'Running' AND config_hash IS NOT NULL",
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

    if commit {
        sqlx::query("INSERT INTO _data_migrations (name) VALUES ($1)").bind(METRIC_SYSTEM_V2).execute(&mut *tx).await?;
        tx.commit().await?;
    } else {
        tx.rollback().await?;
    }
    Ok(report)
}
