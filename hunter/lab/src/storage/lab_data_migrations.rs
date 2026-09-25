//! Lab data migrations — row rewrites SQL cannot express, run once per database right
//! after the lab schema migrations, each in one transaction and recorded in core's
//! `_data_migrations` ledger (the lab tables share that database).
//!
//! **`lab_metric_system_v2`** converts every grouped-sweep run row to v2 with the
//! engine's own converter ([`hunter_engine::v1`]): `tags` (was a volume-ix pattern
//! list), `stage_plans` (was a list of `scale_out` ladders) and `axes_spec`. A row
//! that fails to convert aborts the whole migration, naming the run. Combo `params`
//! stay as written — every reader converts them through `v1::parse_params_any`.

use serde_json::Value;
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// The name `_data_migrations` records.
pub const LAB_METRIC_SYSTEM_V2: &str = "lab_metric_system_v2";

/// Run every pending lab data migration. Called at boot after the lab schema migrations.
pub async fn run(pool: &PgPool) -> anyhow::Result<()> {
    let done: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM _data_migrations WHERE name = $1)")
        .bind(LAB_METRIC_SYSTEM_V2)
        .fetch_one(pool)
        .await?;
    if done {
        return Ok(());
    }
    let mut tx = pool.begin().await?;
    let rows = sqlx::query("SELECT id, axes_spec, tags, stage_plans FROM grouped_sweep_runs").fetch_all(&mut *tx).await?;
    let mut converted = 0usize;
    for row in rows {
        let id: Uuid = row.try_get("id")?;
        let axes: Value = row.try_get("axes_spec")?;
        let tags: Option<Value> = row.try_get("tags")?;
        let plans: Option<Value> = row.try_get("stage_plans")?;
        let (new_axes, new_tags, new_plans) = convert_run(&axes, tags.as_ref(), plans.as_ref())
            .map_err(|e| anyhow::anyhow!("grouped sweep run {id}: {e}"))?;
        if new_axes != axes || new_tags != tags || new_plans != plans {
            sqlx::query("UPDATE grouped_sweep_runs SET axes_spec = $2, tags = $3, stage_plans = $4 WHERE id = $1")
                .bind(id)
                .bind(&new_axes)
                .bind(&new_tags)
                .bind(&new_plans)
                .execute(&mut *tx)
                .await?;
            converted += 1;
        }
    }
    sqlx::query("INSERT INTO _data_migrations (name) VALUES ($1)").bind(LAB_METRIC_SYSTEM_V2).execute(&mut *tx).await?;
    tx.commit().await?;
    tracing::info!(runs = converted, "data migration {LAB_METRIC_SYSTEM_V2} applied");
    Ok(())
}

/// One run's `(axes_spec, tags, stage_plans)` in v2. Each part passes through when it
/// is already v2.
fn convert_run(axes: &Value, tags: Option<&Value>, plans: Option<&Value>) -> Result<(Value, Option<Value>, Option<Value>), String> {
    let mut axes = axes.clone();
    if let Some(list) = axes.get_mut("axes").and_then(Value::as_array_mut) {
        for a in list.iter_mut() {
            *a = hunter_engine::v1::convert_axis(a)?;
        }
    }
    // A v1 pattern list is an array; a v2 tags document is an object.
    let tags = match tags {
        Some(p @ Value::Array(_)) => Some(hunter_engine::v1::ix_patterns_to_tags(p)?),
        other => other.cloned(),
    };
    // A v1 ladder is an array of `{sell_bps, take_profit, conditions}` rungs; a v2 plan
    // is an array of named stages.
    let plans = match plans {
        Some(Value::Array(list)) => Some(Value::Array(
            list.iter()
                .map(|p| {
                    let is_v1 = p.as_array().is_some_and(|rungs| rungs.iter().all(|r| r.get("name").is_none()));
                    if is_v1 {
                        hunter_engine::v1::convert_ladder(p)
                    } else {
                        Ok(p.clone())
                    }
                })
                .collect::<Result<Vec<_>, String>>()?,
        )),
        other => other.cloned(),
    };
    Ok((axes, tags, plans))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_v1_run_converts_and_a_v2_run_passes_through() {
        let axes = json!({ "axes": [
            { "side": "entry", "group": "m_state", "metric": "time", "operator": "<=", "values": [20] },
            { "kind": "take_profit", "values": [50] }
        ] });
        let tags = json!([["Pump.Fun: Buy"]]);
        let plans = json!([[{ "sell_bps": 5000, "take_profit": 30 }]]);
        let (a, t, p) = convert_run(&axes, Some(&tags), Some(&plans)).unwrap();
        assert_eq!(a["axes"][0]["metric"], json!("m_state.age_sec"));
        assert!(t.as_ref().is_some_and(|t| t.get("volume").is_some()));
        assert!(p.as_ref().unwrap()[0][0].get("name").is_some());
        let again = convert_run(&a, t.as_ref(), p.as_ref()).unwrap();
        assert_eq!(again, (a, t, p), "converting twice changes nothing");
    }
}
