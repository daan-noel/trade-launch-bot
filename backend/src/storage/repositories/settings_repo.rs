use serde::{Deserialize, Serialize};
use sqlx::PgPool;

/// Global, server-wide tracking policy backing the `app_settings` singleton row.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TrackingSettings {
    pub track_mayhem: bool,
    pub track_post_migration: bool,
}

impl Default for TrackingSettings {
    fn default() -> Self {
        // Mirrors the migration's column defaults: track everything.
        Self {
            track_mayhem: true,
            track_post_migration: true,
        }
    }
}

pub struct SettingsRepo {
    pool: PgPool,
}

impl SettingsRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Read the tracking policy. Falls back to [`TrackingSettings::default`] if the
    /// singleton row is somehow absent (the migration seeds it, so this is just a
    /// safety net).
    pub async fn get(&self) -> anyhow::Result<TrackingSettings> {
        let row = sqlx::query_as::<_, (bool, bool)>(
            "SELECT track_mayhem, track_post_migration FROM app_settings WHERE id = TRUE",
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row
            .map(|(track_mayhem, track_post_migration)| TrackingSettings {
                track_mayhem,
                track_post_migration,
            })
            .unwrap_or_default())
    }

    /// Persist the tracking policy, upserting the singleton row.
    pub async fn set(&self, settings: TrackingSettings) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO app_settings (id, track_mayhem, track_post_migration, updated_at)
            VALUES (TRUE, $1, $2, now())
            ON CONFLICT (id) DO UPDATE
                SET track_mayhem = EXCLUDED.track_mayhem,
                    track_post_migration = EXCLUDED.track_post_migration,
                    updated_at = EXCLUDED.updated_at
            "#,
        )
        .bind(settings.track_mayhem)
        .bind(settings.track_post_migration)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}
