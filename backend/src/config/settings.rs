use std::time::Duration;

/// All configuration loaded from environment variables at startup.
/// Validated once — panics early if required values are missing.
#[derive(Debug, Clone)]
pub struct Settings {
    // --- Helius ---
    #[allow(dead_code)]
    pub helius_api_key: String,
    pub helius_rpc_url: String,
    /// One or more Helius Sender endpoints. The signed tx is fanned out to all of
    /// them concurrently (same signature → on-chain dedup, tip paid once), so a
    /// slow/down endpoint can't gate the send. A single entry behaves exactly
    /// like the legacy single-endpoint path.
    pub helius_sender_urls: Vec<String>,
    /// LaserStream (Yellowstone gRPC) ingest endpoint. Auth reuses
    /// `helius_api_key` via x-token. Required — the live transport.
    pub helius_laserstream_url: String,

    // --- Solana ---
    pub wallet_private_key: String,
    pub nonce_accounts: Vec<String>,

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
    /// writer, tpsl backtests). Isolated so a sweep can't starve the dashboard reads
    /// it used to share a pool with (the "pool timed out" regression).
    pub db_batch_max_connections: u32,
    pub db_batch_min_connections: u32,
    pub db_acquire_timeout: Duration,

    // --- Server ---
    pub host: String,
    pub port: u16,
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
    /// Load from environment. Call `dotenvy::dotenv()` before this.
    pub fn from_env() -> anyhow::Result<Self> {
        let api_key = required("HELIUS_API_KEY")?;

        let settings = Self {
            helius_api_key: api_key,
            helius_rpc_url: required("HELIUS_RPC_URL")?,
            helius_sender_urls: sender_urls()?,
            helius_laserstream_url: required("HELIUS_LASERSTREAM_URL")?,
            wallet_private_key: required("WALLET_PRIVATE_KEY")?,
            nonce_accounts: parse_required_list("NONCE_ACCOUNTS")?,
            database_url: required("DATABASE_URL")?,
            db_max_connections: env_parse_min("DB_MAX_CONNECTIONS", 64u32, 1)?,
            db_min_connections: env_parse("DB_MIN_CONNECTIONS", 4u32)?,
            db_api_max_connections: env_parse_min("DB_API_MAX_CONNECTIONS", 32u32, 1)?,
            db_api_min_connections: env_parse("DB_API_MIN_CONNECTIONS", 2u32)?,
            db_batch_max_connections: env_parse_min("DB_BATCH_MAX_CONNECTIONS", 16u32, 1)?,
            db_batch_min_connections: env_parse("DB_BATCH_MIN_CONNECTIONS", 2u32)?,
            db_acquire_timeout: Duration::from_secs(env_parse("DB_ACQUIRE_TIMEOUT_SECS", 10u64)?),
            host: env_or("HOST", "127.0.0.1"),
            port: env_parse("PORT", 8081)?,
            // Route through the erroring parse: a typo (e.g. `HTTP_ENABLED=ture`)
            // must fail loudly, not silently fall back to `true` and expose the API.
            http_enabled: env_parse("HTTP_ENABLED", true)?,
            http_workers: env_parse_min("HTTP_WORKERS", 2usize, 1)?,
            cors_allowed_origin: env_or("CORS_ALLOWED_ORIGIN", "*"),
            api_auth_token: Some(required_non_empty("API_AUTH_TOKEN")?),
        };

        Ok(settings)
    }
}

/// Helius Sender endpoints, newest-form first. Prefer the plural
/// `HELIUS_FAST_SENDER_URLS` (comma-separated, fanned out concurrently); fall
/// back to the legacy singular `HELIUS_FAST_SENDER_URL` so existing single-
/// endpoint deployments keep working unchanged. At least one is required.
fn sender_urls() -> anyhow::Result<Vec<String>> {
    if let Ok(list) = std::env::var("HELIUS_FAST_SENDER_URLS") {
        let items: Vec<String> = list
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if !items.is_empty() {
            return Ok(items);
        }
    }
    Ok(vec![required("HELIUS_FAST_SENDER_URL")?])
}

fn parse_required_list(key: &str) -> anyhow::Result<Vec<String>> {
    let value = required(key)?;
    let items = value
        .split(',')
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect::<Vec<_>>();

    if items.is_empty() {
        anyhow::bail!("{key} must contain at least one value");
    }

    Ok(items)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

