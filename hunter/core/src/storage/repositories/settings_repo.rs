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
    // Slippage is ONE key per side — the legacy combined `trade.slippage_bps` is
    // retired (deleted from `app_settings` by the slippage-reset migration, now
    // folded into `0001_init.sql`'s notes) so a blank buy field can't fall through
    // to a stale legacy number instead of the default.
    pub const BUY_SLIPPAGE_BPS: Setting<Option<u64>> = Setting::new("trade.buy_slippage_bps", || None);
    pub const SELL_SLIPPAGE_BPS: Setting<Option<u64>> = Setting::new("trade.sell_slippage_bps", || None);
    pub const LIVE: Setting<bool> = Setting::new("ingest.live", || false);
    pub const PERSIST_RAW: Setting<bool> = Setting::new("ingest.persist_raw", || false);
    /// Master switch for the ingest liveness watchdog. When off, the watchdog
    /// holds fire (never restarts the process) regardless of stall. Default on.
    pub const WATCHDOG_ENABLED: Setting<bool> = Setting::new("ingest.watchdog_enabled", || true);
    /// Stall window (seconds): no ingest forward progress for this long while
    /// live trips the watchdog. Floored server-side (see `ingest_health`). Default
    /// 180s; adjustable live via the Settings page without a restart.
    pub const WATCHDOG_STALL_TIMEOUT_SECS: Setting<u64> =
        Setting::new("ingest.watchdog_stall_timeout_secs", || 90);
    /// How often (seconds) the watchdog wakes to check the stall window.
    pub const WATCHDOG_CHECK_INTERVAL_SECS: Setting<u64> =
        Setting::new("ingest.watchdog_check_interval_secs", || 10);
    /// Hard ceiling on total SOL committed to open real positions at any moment
    /// (in SOL). When set, a new real buy is blocked if it would push the running
    /// committed total over this value. `None` = no explicit ceiling (the wallet
    /// balance-floor guard still applies).
    pub const MAX_COMMITTED_SOL: Setting<Option<f64>> =
        Setting::new("trade.max_committed_sol", || None);
    /// Enable gap-replay on LaserStream reconnect: send `from_slot` so the server
    /// replays missed transactions since the last seen slot. Default OFF — replayed
    /// TokenCreated events have stale block_time until the SlotAnchor is pinned
    /// (A3), and any replay is filtered by A4's 30 s freshness gate anyway.
    pub const GAP_REPLAY_ON_RECONNECT: Setting<bool> =
        Setting::new("ingest.gap_replay_on_reconnect", || false);
    /// Maximum gap-replay window (seconds). If the gap since last progress exceeds
    /// this, reconnect without `from_slot` (full re-subscribe) instead of replaying
    /// a huge backlog. Default 300 s (5 min).
    pub const GAP_REPLAY_MAX_WINDOW_SECS: Setting<u64> =
        Setting::new("ingest.gap_replay_max_window_secs", || 300);
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
    /// Buy-side slippage tolerance in bps (100 = 1%), used **exactly as typed**.
    /// `None` (blank) = `DEFAULT_SLIPPAGE_BPS`. `Some(0)` is rejected with a 400
    /// on write, so it never reaches storage.
    pub buy_slippage_bps: Option<u64>,
    /// Sell-side slippage tolerance in bps, used **exactly as typed**. `None`
    /// (blank) = no floor (min_out = 1, sell all) so bot exits never stall on a
    /// rapidly dumping token. `Some(0)` is rejected with a 400 on write.
    pub sell_slippage_bps: Option<u64>,
    /// Live-mode toggle for the LaserStream ingest (live = connect, dead = paused).
    /// Persisted so a restart restores the operator's last on/off choice instead
    /// of always booting paused. Set via `PUT /api/system/live`.
    pub live: bool,
    /// Persist raw transaction payloads to `raw_txs`. When off, the ingest
    /// pipeline skips the raw-payload enqueue (trades/metrics are still recorded)
    /// to curb DB growth. Default off.
    pub persist_raw: bool,
    /// Master switch for the ingest liveness watchdog. When off, the watchdog
    /// never force-exits the process on a stall. Default on.
    pub watchdog_enabled: bool,
    /// Stall window in seconds: no ingest forward progress for this long while
    /// live trips the watchdog. Clamped server-side to a safe floor on write.
    pub watchdog_stall_timeout_secs: u64,
    /// How often the watchdog wakes (seconds) to check the stall window.
    pub watchdog_check_interval_secs: u64,
    /// Hard ceiling (SOL) on total SOL committed to open real positions. When set,
    /// a new real buy that would push committed total over this is blocked. `None`
    /// = no explicit ceiling.
    pub max_committed_sol: Option<f64>,
    /// Enable gap-replay on LaserStream reconnect. Default false (safe default:
    /// replayed TokenCreated events use stale block_time and are filtered by A4).
    pub gap_replay_on_reconnect: bool,
    /// Maximum gap-replay window in seconds. Gaps beyond this trigger a clean
    /// re-subscribe instead of replaying a large backlog. Default 300 s.
    pub gap_replay_max_window_secs: u64,
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
            buy_slippage_bps: pick(map, &keys::BUY_SLIPPAGE_BPS),
            sell_slippage_bps: pick(map, &keys::SELL_SLIPPAGE_BPS),
            live: pick(map, &keys::LIVE),
            persist_raw: pick(map, &keys::PERSIST_RAW),
            watchdog_enabled: pick(map, &keys::WATCHDOG_ENABLED),
            watchdog_stall_timeout_secs: pick(map, &keys::WATCHDOG_STALL_TIMEOUT_SECS),
            watchdog_check_interval_secs: pick(map, &keys::WATCHDOG_CHECK_INTERVAL_SECS),
            max_committed_sol: pick(map, &keys::MAX_COMMITTED_SOL),
            gap_replay_on_reconnect: pick(map, &keys::GAP_REPLAY_ON_RECONNECT),
            gap_replay_max_window_secs: pick(map, &keys::GAP_REPLAY_MAX_WINDOW_SECS),
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

    /// Atomically upsert one typed setting's row. Touches only this key.
    pub async fn set_one<T: Serialize>(
        &self,
        setting: &Setting<T>,
        value: &T,
    ) -> anyhow::Result<()> {
        let value = serde_json::to_value(value)?;
        self.set_many(&[(setting.key, value)]).await
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
        assert_eq!(settings.buy_slippage_bps, None);
        assert_eq!(settings.sell_slippage_bps, None);
        // Watchdog on by default, with the standard window/cadence.
        assert!(settings.watchdog_enabled);
        assert_eq!(settings.watchdog_stall_timeout_secs, 90);
        assert_eq!(settings.watchdog_check_interval_secs, 10);
    }

    #[test]
    fn from_map_applies_present_keys_over_defaults() {
        let mut map = HashMap::new();
        map.insert("ingest.track_mayhem".to_string(), json!(false));
        map.insert("ingest.live".to_string(), json!(true));
        map.insert("ingest.persist_raw".to_string(), json!(false));
        map.insert("ui.price_unit".to_string(), json!("USD"));
        map.insert("trade.buy_slippage_bps".to_string(), json!(250));

        let settings = AppSettings::from_map(&map);
        assert!(!settings.track_mayhem); // overridden
        assert!(settings.live); // overridden
        assert!(!settings.persist_raw); // overridden
        assert!(!settings.track_post_migration); // still default
        assert_eq!(settings.price_unit.as_deref(), Some("USD"));
        assert_eq!(settings.buy_slippage_bps, Some(250));
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
