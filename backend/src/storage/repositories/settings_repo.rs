use std::collections::HashMap;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;

/// A single typed setting: its persisted key and its default. This is the
/// registry entry that ties a strongly-typed value to a row in `app_settings`.
///
/// Adding a setting = add a `Setting` const to [`keys`] and a field to
/// [`AppSettings`]; no migration, no SQL. The default is used whenever the key
/// is absent (never set) or stored as an undecodable value.
pub struct Setting<T> {
    pub key: &'static str,
    default: fn() -> T,
}

impl<T> Setting<T> {
    const fn new(key: &'static str, default: fn() -> T) -> Self {
        Self { key, default }
    }

    pub fn default_value(&self) -> T {
        (self.default)()
    }
}

/// The setting registry — one entry per persisted setting. The dotted keys are
/// the on-disk names (namespaced by area); the struct field names are the
/// in-memory / API names. Keep these in sync with [`AppSettings`].
pub mod keys {
    use super::Setting;

    pub const TRACK_MAYHEM: Setting<bool> = Setting::new("ingest.track_mayhem", || false);
    pub const TRACK_POST_MIGRATION: Setting<bool> =
        Setting::new("ingest.track_post_migration", || false);
    pub const TIMEZONE: Setting<Option<String>> = Setting::new("ui.timezone", || None);
    pub const PRICE_UNIT: Setting<Option<String>> = Setting::new("ui.price_unit", || None);
    pub const SLIPPAGE_BPS: Setting<Option<u64>> = Setting::new("trade.slippage_bps", || None);
    pub const LIVE: Setting<bool> = Setting::new("ingest.live", || false);
    pub const PERSIST_RAW: Setting<bool> = Setting::new("ingest.persist_raw", || false);
    /// Master switch for the ingest liveness watchdog. When off, the watchdog
    /// holds fire (never restarts the process) regardless of stall. Default on.
    pub const WATCHDOG_ENABLED: Setting<bool> = Setting::new("ingest.watchdog_enabled", || true);
    /// Stall window (seconds): no ingest forward progress for this long while
    /// live trips the watchdog. Floored server-side (see `ingest_health`). Seeded
    /// from `INGEST_STALL_TIMEOUT_SECS` on a fresh DB; the UI value wins after.
    pub const WATCHDOG_STALL_TIMEOUT_SECS: Setting<u64> =
        Setting::new("ingest.watchdog_stall_timeout_secs", || 120);
    /// How often (seconds) the watchdog wakes to check the stall window.
    pub const WATCHDOG_CHECK_INTERVAL_SECS: Setting<u64> =
        Setting::new("ingest.watchdog_check_interval_secs", || 15);
}

/// Global, server-wide settings — the assembled, strongly-typed view of the
/// `app_settings` key-value rows. Held in memory (a `watch` channel) as the
/// runtime source of truth; serialized as-is for the `/api/settings` response,
/// so its field names are the stable API contract (the frontend mirrors them).
///
/// Persistence is per-key (see [`keys`] / [`SettingsRepo`]), not this whole
/// struct: a write touches only the changed key's row.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// Live-mode toggle for the LaserStream ingest (live = connect, dead = paused).
    /// Persisted so a restart restores the operator's last on/off choice instead
    /// of always booting paused. Set via `PUT /api/system/live`.
    pub live: bool,
    /// Persist raw transaction blobs to `raw_transactions`. When off, the ingest
    /// pipeline skips the raw-blob enqueue (trades/metrics are still recorded) to
    /// curb DB growth. Default off.
    pub persist_raw: bool,
    /// Master switch for the ingest liveness watchdog. When off, the watchdog
    /// never force-exits the process on a stall. Default on.
    pub watchdog_enabled: bool,
    /// Stall window in seconds: no ingest forward progress for this long while
    /// live trips the watchdog. Clamped server-side to a safe floor on write.
    pub watchdog_stall_timeout_secs: u64,
    /// How often the watchdog wakes (seconds) to check the stall window.
    pub watchdog_check_interval_secs: u64,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self::from_map(&HashMap::new())
    }
}

impl AppSettings {
    /// Assemble the typed view from a `key -> JSONB value` map (the rows of
    /// `app_settings`). Each field reads its registry key; a missing or
    /// undecodable value falls back to that setting's default, so a row written
    /// by an older binary — or a key never set — always deserializes cleanly.
    fn from_map(map: &HashMap<String, Value>) -> Self {
        Self {
            track_mayhem: pick(map, &keys::TRACK_MAYHEM),
            track_post_migration: pick(map, &keys::TRACK_POST_MIGRATION),
            timezone: pick(map, &keys::TIMEZONE),
            price_unit: pick(map, &keys::PRICE_UNIT),
            slippage_bps: pick(map, &keys::SLIPPAGE_BPS),
            live: pick(map, &keys::LIVE),
            persist_raw: pick(map, &keys::PERSIST_RAW),
            watchdog_enabled: pick(map, &keys::WATCHDOG_ENABLED),
            watchdog_stall_timeout_secs: pick(map, &keys::WATCHDOG_STALL_TIMEOUT_SECS),
            watchdog_check_interval_secs: pick(map, &keys::WATCHDOG_CHECK_INTERVAL_SECS),
        }
    }
}

/// Read one setting's value out of the row map, falling back to its default.
fn pick<T: DeserializeOwned>(map: &HashMap<String, Value>, setting: &Setting<T>) -> T {
    map.get(setting.key)
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_else(|| setting.default_value())
}

pub struct SettingsRepo {
    pool: PgPool,
}

impl SettingsRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Load the full assembled settings view (all rows in one `SELECT`).
    /// Absent keys are filled from [`AppSettings::default`].
    pub async fn load_all(&self) -> anyhow::Result<AppSettings> {
        let rows = sqlx::query_as::<_, (String, Value)>("SELECT key, value FROM app_settings")
            .fetch_all(&self.pool)
            .await?;
        let map: HashMap<String, Value> = rows.into_iter().collect();
        Ok(AppSettings::from_map(&map))
    }

    /// Read one typed setting, falling back to its default if unset/undecodable.
    /// Part of the typed key-value API; callers today load the whole view via
    /// [`Self::load_all`], so this is unused until a single-key read is needed.
    #[allow(dead_code)]
    pub async fn get_one<T: DeserializeOwned>(&self, setting: &Setting<T>) -> anyhow::Result<T> {
        let row = sqlx::query_as::<_, (Value,)>("SELECT value FROM app_settings WHERE key = $1")
            .bind(setting.key)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row
            .and_then(|(v,)| serde_json::from_value(v).ok())
            .unwrap_or_else(|| setting.default_value()))
    }

    /// Atomically upsert one typed setting's row. Touches only this key.
    pub async fn set_one<T: Serialize>(
        &self,
        setting: &Setting<T>,
        value: &T,
    ) -> anyhow::Result<()> {
        let value = serde_json::to_value(value)?;
        self.set_many(&[(setting.key, value)]).await
    }

    /// Insert setting rows only if their key is absent (`ON CONFLICT DO NOTHING`).
    /// Used at startup to seed a default into a fresh DB without ever clobbering a
    /// value an operator has already set via the API.
    pub async fn seed_if_absent(&self, entries: &[(&str, Value)]) -> anyhow::Result<()> {
        if entries.is_empty() {
            return Ok(());
        }
        let mut tx = self.pool.begin().await?;
        for (key, value) in entries {
            sqlx::query(
                r#"
                INSERT INTO app_settings (key, value, updated_at)
                VALUES ($1, $2, now())
                ON CONFLICT (key) DO NOTHING
                "#,
            )
            .bind(key)
            .bind(value)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Atomically upsert several setting rows in one transaction. Used by partial
    /// updates that touch multiple keys at once; each key is its own row, so this
    /// never clobbers settings the request didn't mention.
    pub async fn set_many(&self, entries: &[(&str, Value)]) -> anyhow::Result<()> {
        if entries.is_empty() {
            return Ok(());
        }
        let mut tx = self.pool.begin().await?;
        for (key, value) in entries {
            sqlx::query(
                r#"
                INSERT INTO app_settings (key, value, updated_at)
                VALUES ($1, $2, now())
                ON CONFLICT (key) DO UPDATE
                    SET value = EXCLUDED.value,
                        updated_at = EXCLUDED.updated_at
                "#,
            )
            .bind(key)
            .bind(value)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn from_map_fills_defaults_for_absent_keys() {
        let settings = AppSettings::from_map(&HashMap::new());
        // Defaults: tracking off, live off, optional prefs unset.
        assert!(!settings.track_mayhem);
        assert!(!settings.track_post_migration);
        assert!(!settings.live);
        assert!(!settings.persist_raw);
        assert_eq!(settings.timezone, None);
        assert_eq!(settings.price_unit, None);
        assert_eq!(settings.slippage_bps, None);
        // Watchdog on by default, with the standard window/cadence.
        assert!(settings.watchdog_enabled);
        assert_eq!(settings.watchdog_stall_timeout_secs, 120);
        assert_eq!(settings.watchdog_check_interval_secs, 15);
    }

    #[test]
    fn from_map_applies_present_keys_over_defaults() {
        let mut map = HashMap::new();
        map.insert("ingest.track_mayhem".to_string(), json!(false));
        map.insert("ingest.live".to_string(), json!(true));
        map.insert("ingest.persist_raw".to_string(), json!(false));
        map.insert("ui.price_unit".to_string(), json!("USD"));
        map.insert("trade.slippage_bps".to_string(), json!(250));

        let settings = AppSettings::from_map(&map);
        assert!(!settings.track_mayhem); // overridden
        assert!(settings.live); // overridden
        assert!(!settings.persist_raw); // overridden
        assert!(!settings.track_post_migration); // still default
        assert_eq!(settings.price_unit.as_deref(), Some("USD"));
        assert_eq!(settings.slippage_bps, Some(250));
        assert_eq!(settings.timezone, None); // still default
    }

    #[test]
    fn pick_falls_back_when_value_is_wrong_type() {
        // A value that can't decode into the field type must not panic — it
        // falls back to the setting's default.
        let mut map = HashMap::new();
        map.insert("ingest.track_mayhem".to_string(), json!("not a bool"));
        let settings = AppSettings::from_map(&map);
        // Falls back to the setting's default (`TRACK_MAYHEM` = false) instead of a
        // decode error — the point is no panic, whatever the default value is.
        assert!(!settings.track_mayhem);
    }
}
