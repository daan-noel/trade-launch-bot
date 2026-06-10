use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;

/// Global, server-wide settings — the typed view of the `app_settings` singleton
/// row's JSONB document. This struct is the source of truth for the document
/// shape: add a field (with a default) to add a setting; no migration needed.
///
/// `#[serde(default)]` makes every missing JSON key fall back to [`Default`], so
/// rows written by an older binary still deserialize cleanly.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    /// Track Mayhem-mode tokens in the ingest pipeline.
    pub track_mayhem: bool,
    /// Record AMM trade histories for migrated tokens.
    pub track_post_migration: bool,
    /// Header timezone preference (IANA name). `None` = never set by a client.
    pub timezone: Option<String>,
    /// Header price-unit preference ("SOL" | "USD"). `None` = never set.
    pub price_unit: Option<String>,
    /// Default trade slippage tolerance in basis points (100 = 1%). Used when a
    /// buy/sell request doesn't specify its own. `None` = fall back to the
    /// server's built-in default (`DEFAULT_SLIPPAGE_BPS`).
    pub slippage_bps: Option<u64>,
    /// Live-mode toggle for the Helius WS ingest (live = connect, dead = paused).
    /// Persisted here so a restart restores the operator's last on/off choice
    /// instead of always booting paused. Set via `PUT /api/system/live`.
    pub live: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            track_mayhem: true,
            track_post_migration: true,
            timezone: None,
            price_unit: None,
            slippage_bps: None,
            live: false,
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

    /// Read the settings document. Unknown/missing keys are filled from
    /// [`AppSettings::default`]; an absent row also yields defaults.
    pub async fn get(&self) -> anyhow::Result<AppSettings> {
        let row = sqlx::query_as::<_, (Value,)>("SELECT data FROM app_settings WHERE id = 1")
            .fetch_optional(&self.pool)
            .await?;

        let settings = match row {
            Some((data,)) => serde_json::from_value(data).unwrap_or_default(),
            None => AppSettings::default(),
        };
        Ok(settings)
    }

    /// Persist the full settings document, upserting the singleton row.
    pub async fn set(&self, settings: &AppSettings) -> anyhow::Result<()> {
        let data = serde_json::to_value(settings)?;
        sqlx::query(
            r#"
            INSERT INTO app_settings (id, data, updated_at)
            VALUES (1, $1, now())
            ON CONFLICT (id) DO UPDATE
                SET data = EXCLUDED.data,
                    updated_at = EXCLUDED.updated_at
            "#,
        )
        .bind(data)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}
