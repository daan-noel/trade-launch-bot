//! Config / settings. Loads `.env` (dotenvy) and exposes the DB connection
//! settings shared by both bins. Each bin reads its own HTTP host/port.
//!
//! Pool sizing mirrors meme-trading's workload-isolated hot/api/batch split (the
//! sum stays well under Postgres `max_connections`; connection counts are
//! load-bearing on the 2vCPU/4GB EC2 box).

use std::time::Duration;

/// DB connection settings, read from the environment once at boot.
#[derive(Debug, Clone)]
pub struct Settings {
    pub database_url: String,
    pub db_max_connections: u32,
    pub db_min_connections: u32,
    pub db_api_max_connections: u32,
    pub db_api_min_connections: u32,
    pub db_batch_max_connections: u32,
    pub db_batch_min_connections: u32,
    pub db_acquire_timeout: Duration,
}

impl Settings {
    /// Load `.env` (best-effort) then read settings. Panics only on a missing
    /// `DATABASE_URL` — the one value with no safe default.
    pub fn from_env() -> anyhow::Result<Self> {
        // Best-effort: absent .env is fine (docker/CI inject real env vars).
        let _ = dotenvy::dotenv();

        let database_url = std::env::var("DATABASE_URL")
            .map_err(|_| anyhow::anyhow!("DATABASE_URL is required (see .env.example)"))?;

        Ok(Self {
            database_url,
            db_max_connections: env_u32("DB_MAX_CONNECTIONS", 12),
            db_min_connections: env_u32("DB_MIN_CONNECTIONS", 1),
            db_api_max_connections: env_u32("DB_API_MAX_CONNECTIONS", 8),
            db_api_min_connections: env_u32("DB_API_MIN_CONNECTIONS", 1),
            db_batch_max_connections: env_u32("DB_BATCH_MAX_CONNECTIONS", 2),
            db_batch_min_connections: env_u32("DB_BATCH_MIN_CONNECTIONS", 0),
            db_acquire_timeout: Duration::from_secs(env_u64("DB_ACQUIRE_TIMEOUT_SECS", 10)),
        })
    }
}

fn env_u32(key: &str, default: u32) -> u32 {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}
