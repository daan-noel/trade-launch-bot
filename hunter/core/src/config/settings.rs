use std::time::Duration;

/// Configuration shared by **both** bins (`hunter-live` + `hunter-lab`), loaded
/// from the environment once at startup. One constructor, one shape — a field a
/// bin doesn't use it simply leaves at its default (e.g. lab never sets the
/// Helius endpoints, disabling the token-sync replay path).
///
/// **Live-only trading credentials do NOT live here.** The trader wallet key,
/// nonce accounts, and Helius Sender endpoints are required to move real SOL, so
/// only the live bin loads them — see `live::config::TradingSecrets`. Tip / CU
/// knobs are shared ([`crate::config::FeeTuning`]) so lab's CostModel can price
/// the same floor live bids. Keeping secrets off this struct means lab can't
/// accidentally require (or hold) them. Mirrors forge's `Settings` (DB-only) +
/// `LauncherSettings` (live-only) split.
///
/// **HTTP host/port are read per-bin**, not here: live and lab bind different
/// ports (`LIVE_PORT` :8130 / `LAB_PORT` :8140 — matching the deploy
/// `*_API_PORT`s) so both can run at once. Each `main` resolves its own via
/// [`resolve_host`] / [`resolve_port`].
#[derive(Debug, Clone)]
pub struct Settings {
    // --- Helius endpoints (shared, OPTIONAL) ---
    // Used by the token-sync replay "Fetch New" fast path via `CoreState`. Empty
    // ⇒ replay disabled (RPC path only) — that's the lab default. The live bin
    // requires them and enforces it at boot with [`Settings::require_live_endpoints`].
    pub helius_api_key: String,
    pub helius_rpc_url: String,
    /// LaserStream (Yellowstone gRPC) ingest endpoint. Auth reuses
    /// `helius_api_key` via x-token.
    pub helius_laserstream_url: String,

    // --- Database ---
    pub database_url: String,
    /// **Hot-path** pool — ingest (`DbWriter`), `StrategyRunner`, maintenance,
    /// seeding and the background caches. One of three workload-isolated pools (see
    /// [`crate::storage::postgres::DbPools`]) so a write storm here can't starve the
    /// dashboard or the batch jobs.
    pub db_max_connections: u32,
    pub db_min_connections: u32,
    /// **API** pool — fast HTTP handlers (dashboard list/detail/count reads,
    /// settings, mutations). Isolated from the hot path so ingest/strategy traffic
    /// can't exhaust the connections the dashboard needs to respond.
    pub db_api_max_connections: u32,
    pub db_api_min_connections: u32,
    /// **Batch** pool — long, DB-heavy jobs (grouped sweeps' corpus load + per-group
    /// writer, tpsl backtests). Isolated so a sweep can't starve the dashboard
    /// reads - sharing one pool with them is the "pool timed out" regression.
    pub db_batch_max_connections: u32,
    pub db_batch_min_connections: u32,
    pub db_acquire_timeout: Duration,

    // --- Server ---
    pub http_enabled: bool,
    pub http_workers: usize,
    /// CORS allowed origin. `"*"` (default) keeps the permissive behaviour;
    /// set it to the frontend origin to lock cross-origin access down.
    pub cors_allowed_origin: String,
    /// Bearer token required on mutating (POST/PUT/DELETE/PATCH) API requests.
    /// **Required** at startup: the auth middleware is fail-closed, so without a
    /// token every mutating (real-SOL) route would be unreachable. Modelled as
    /// `Option` only so the middleware's `None` arm stays explicit; `from_env`
    /// rejects a missing/empty `API_AUTH_TOKEN`.
    pub api_auth_token: Option<String>,
}

impl Settings {
    /// Load the shared configuration from the environment. Call
    /// `dotenvy::dotenv()` before this. Used by **both** bins.
    ///
    /// Required: `DATABASE_URL` and `API_AUTH_TOKEN`. The Helius endpoints default
    /// to empty (lab runs with no keys / no gRPC); the live bin calls
    /// [`Self::require_live_endpoints`] after loading to fail fast if they're
    /// missing.
    pub fn from_env() -> anyhow::Result<Self> {
        let settings = Self {
            helius_api_key: env_or("HELIUS_API_KEY", ""),
            helius_rpc_url: env_or("HELIUS_RPC_URL", ""),
            helius_laserstream_url: env_or("HELIUS_LASERSTREAM_URL", ""),
            database_url: required("DATABASE_URL")?,
            db_max_connections: env_parse_min("DB_MAX_CONNECTIONS", 64u32, 1)?,
            db_min_connections: env_parse("DB_MIN_CONNECTIONS", 4u32)?,
            db_api_max_connections: env_parse_min("DB_API_MAX_CONNECTIONS", 32u32, 1)?,
            db_api_min_connections: env_parse("DB_API_MIN_CONNECTIONS", 2u32)?,
            db_batch_max_connections: env_parse_min("DB_BATCH_MAX_CONNECTIONS", 16u32, 1)?,
            db_batch_min_connections: env_parse("DB_BATCH_MIN_CONNECTIONS", 2u32)?,
            db_acquire_timeout: Duration::from_secs(env_parse("DB_ACQUIRE_TIMEOUT_SECS", 10u64)?),
            // Route through the erroring parse: a typo (e.g. `HTTP_ENABLED=ture`)
            // must fail loudly, not silently fall back to `true` and expose the API.
            http_enabled: env_parse("HTTP_ENABLED", true)?,
            http_workers: env_parse_min("HTTP_WORKERS", 2usize, 1)?,
            cors_allowed_origin: env_or("CORS_ALLOWED_ORIGIN", "*"),
            api_auth_token: Some(required_non_empty("API_AUTH_TOKEN")?),
        };

        Ok(settings)
    }

    /// Assert the Helius endpoints the **live** bin can't run without are present.
    /// Lab leaves them empty (replay disabled, no gRPC); live calls this right
    /// after [`Self::from_env`] so a missing endpoint fails at boot instead of at
    /// first RPC/gRPC use.
    pub fn require_live_endpoints(&self) -> anyhow::Result<()> {
        for (key, val) in [
            ("HELIUS_API_KEY", &self.helius_api_key),
            ("HELIUS_RPC_URL", &self.helius_rpc_url),
            ("HELIUS_LASERSTREAM_URL", &self.helius_laserstream_url),
        ] {
            if val.trim().is_empty() {
                anyhow::bail!("Missing required env var for the live bin: {key}");
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Resolve the HTTP bind host. Docker injects `HOST=0.0.0.0` (so the container is
/// reachable via its published port) and that wins; local dev falls back to the
/// bin-specific key (`LIVE_HOST` / `LAB_HOST`) so the two bins can bind
/// independently, then to loopback. Each `main` calls this. Mirrors forge.
pub fn resolve_host(specific_key: &str) -> String {
    std::env::var("HOST")
        .or_else(|_| std::env::var(specific_key))
        .unwrap_or_else(|_| "127.0.0.1".to_string())
}

/// Resolve the HTTP bind port. Docker injects `PORT` (the compose `*_API_PORT`)
/// and that wins; local dev falls back to the bin-specific key (`LIVE_PORT` /
/// `LAB_PORT`) so live and lab default to *different* ports (8130 / 8140 — the
/// same numbers as the deploy `*_API_PORT`s) and neither needs an inline
/// `PORT=…` override to avoid colliding. Mirrors forge.
pub fn resolve_port(specific_key: &str, default: u16) -> anyhow::Result<u16> {
    if let Ok(raw) = std::env::var("PORT") {
        return raw
            .parse()
            .map_err(|e| anyhow::anyhow!("Invalid value for PORT={raw:?}: {e}"));
    }
    env_parse(specific_key, default)
}

fn required(key: &str) -> anyhow::Result<String> {
    std::env::var(key).map_err(|_| anyhow::anyhow!("Missing required env var: {key}"))
}

/// Like `required`, but also rejects an empty/whitespace value. Used for secrets
/// where a blank string is as dangerous as a missing one (e.g. `API_AUTH_TOKEN`,
/// where an empty token would make the fail-closed auth middleware accept an
/// empty bearer).
fn required_non_empty(key: &str) -> anyhow::Result<String> {
    let val = required(key)?;
    if val.trim().is_empty() {
        anyhow::bail!("Required env var {key} must not be empty");
    }
    Ok(val)
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn env_parse<T>(key: &str, default: T) -> anyhow::Result<T>
where
    T: std::str::FromStr + Copy,
    T::Err: std::fmt::Display,
{
    match std::env::var(key) {
        Ok(val) => val
            .parse::<T>()
            .map_err(|e| anyhow::anyhow!("Invalid value for {key}={val:?}: {e}")),
        Err(_) => Ok(default),
    }
}

/// Like [`env_parse`] but rejects a parsed value below `min`. Used for sizing
/// knobs where a `0` is a silent footgun — `db_max_connections: 0` or
/// `http_workers: 0` wedges the pool / refuses every request rather than erroring.
fn env_parse_min<T>(key: &str, default: T, min: T) -> anyhow::Result<T>
where
    T: std::str::FromStr + Copy + PartialOrd + std::fmt::Display,
    T::Err: std::fmt::Display,
{
    let val = env_parse(key, default)?;
    if val < min {
        anyhow::bail!("{key} must be >= {min}, got {val}");
    }
    Ok(val)
}
