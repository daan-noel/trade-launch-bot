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

    // --- Timing ---
    /// How long to wait before reconnecting after a stream drop.
    pub reconnect_interval: Duration,
    /// Liveness watchdog: if ingest makes no forward progress for this long while
    /// live mode is on, the process force-exits so the supervisor restarts a wedged
    /// ingest (a downstream `.await` deadlock the in-stream watchdog can't see). The
    /// pump.fun firehose never goes this quiet, so only a real stall trips it.
    pub ingest_stall_timeout: Duration,

    // --- Database ---
    pub database_url: String,
    /// **Hot-path** Postgres pool sizing — backs ingest (`DbWriter`),
    /// `StrategyRunner`, maintenance, seeding and the background caches. Kept
    /// separate from the API pool so an ingest/strategy write storm can't starve
    /// dashboard reads (and vice-versa). Defaults suit a busy ingest workload;
    /// override via env when the database has more or less headroom.
    pub db_max_connections: u32,
    pub db_min_connections: u32,
    /// **API** Postgres pool sizing — backs only the HTTP request handlers
    /// (list/detail endpoints, sweeps). Isolated from `db_max_connections` so the
    /// relentless ingest/strategy traffic can't exhaust the connections the
    /// dashboard needs to respond.
    pub db_api_max_connections: u32,
    pub db_api_min_connections: u32,
    /// The Postgres server's `max_connections` (mirrors `POSTGRES_MAX_CONNECTIONS`
    /// in docker-compose). Used only to fail-fast at startup if the two app pools
    /// plus `db_connection_reserve` would overcommit the server — a misconfig that
    /// otherwise surfaces as opaque "too many clients already" errors under load.
    pub db_server_max_connections: u32,
    /// Connections to leave free on the server for the superuser reserve
    /// (`superuser_reserved_connections`, default 3) and external clients (Navicat,
    /// psql). The startup check requires `db_max + db_api_max + reserve <= server`.
    pub db_connection_reserve: u32,
    pub db_acquire_timeout: Duration,
    /// `idle_in_transaction_session_timeout` applied to every pooled connection.
    /// Bounds the one failure mode that actually drains a pool — a leaked/stuck
    /// *open* transaction pinning its slot forever. `0` disables it. Safe for
    /// long-running *active* queries (e.g. sweep corpus scans): it only fires on a
    /// transaction left idle, never on one doing work.
    pub db_idle_tx_timeout: Duration,

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
            reconnect_interval: Duration::from_millis(env_parse("RECONNECT_INTERVAL", 10_000)?),
            // Floor at 30s so a typo can't set a trigger-happy timeout that restarts
            // the process on a normal burst lull.
            ingest_stall_timeout: Duration::from_secs(env_parse_min(
                "INGEST_STALL_TIMEOUT_SECS",
                120u64,
                30,
            )?),
            database_url: required("DATABASE_URL")?,
            db_max_connections: env_parse_min("DB_MAX_CONNECTIONS", 64u32, 1)?,
            db_min_connections: env_parse("DB_MIN_CONNECTIONS", 4u32)?,
            db_api_max_connections: env_parse_min("DB_API_MAX_CONNECTIONS", 32u32, 1)?,
            db_api_min_connections: env_parse("DB_API_MIN_CONNECTIONS", 2u32)?,
            db_server_max_connections: env_parse_min("POSTGRES_MAX_CONNECTIONS", 300u32, 1)?,
            db_connection_reserve: env_parse("DB_CONNECTION_RESERVE", 20u32)?,
            db_acquire_timeout: Duration::from_secs(env_parse("DB_ACQUIRE_TIMEOUT_SECS", 10u64)?),
            db_idle_tx_timeout: Duration::from_secs(env_parse("DB_IDLE_TX_TIMEOUT_SECS", 30u64)?),
            host: env_or("HOST", "127.0.0.1"),
            port: env_parse("PORT", 8081)?,
            // Route through the erroring parse: a typo (e.g. `HTTP_ENABLED=ture`)
            // must fail loudly, not silently fall back to `true` and expose the API.
            http_enabled: env_parse("HTTP_ENABLED", true)?,
            http_workers: env_parse_min("HTTP_WORKERS", 2usize, 1)?,
            cors_allowed_origin: env_or("CORS_ALLOWED_ORIGIN", "*"),
            api_auth_token: Some(required_non_empty("API_AUTH_TOKEN")?),
        };

        settings.validate_pool_sizing()?;
        Ok(settings)
    }

    /// Fail fast if the two app pools (plus the superuser/external reserve) would
    /// overcommit the Postgres server's `max_connections`. Without this, an env
    /// edit that pushes `DB_MAX_CONNECTIONS + DB_API_MAX_CONNECTIONS` past the
    /// server ceiling surfaces only later, as opaque "too many clients already"
    /// errors once load drives both pools toward their max. `POSTGRES_MAX_CONNECTIONS`
    /// here mirrors the value docker-compose passes to the postgres container, so the
    /// check is meaningful under compose; tune `DB_CONNECTION_RESERVE` (or set
    /// `POSTGRES_MAX_CONNECTIONS`) when running against a differently-sized server.
    fn validate_pool_sizing(&self) -> anyhow::Result<()> {
        check_pool_budget(
            self.db_max_connections,
            self.db_api_max_connections,
            self.db_server_max_connections,
            self.db_connection_reserve,
        )
    }
}

/// Pure pool-budget check (testable without building a full `Settings`): the two
/// app pools plus the reserve must fit inside the server's `max_connections`.
fn check_pool_budget(
    db_max: u32,
    db_api_max: u32,
    server_max: u32,
    reserve: u32,
) -> anyhow::Result<()> {
    let requested = db_max + db_api_max;
    let budget = server_max.saturating_sub(reserve);
    anyhow::ensure!(
        requested <= budget,
        "DB pool sizing overcommits Postgres: DB_MAX_CONNECTIONS ({db_max}) + \
         DB_API_MAX_CONNECTIONS ({db_api_max}) = {requested} exceeds the available \
         budget of {budget} (POSTGRES_MAX_CONNECTIONS {server_max} − \
         DB_CONNECTION_RESERVE {reserve}). Lower the pool sizes or raise \
         POSTGRES_MAX_CONNECTIONS.",
    );
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::check_pool_budget;

    #[test]
    fn pool_budget_accepts_sizing_within_server_ceiling() {
        // 150 + 80 + 20 reserve = 250 <= 300 → ok.
        assert!(check_pool_budget(150, 80, 300, 20).is_ok());
        // Exactly fills the budget (230 == 300 - 70) → ok (boundary inclusive).
        assert!(check_pool_budget(150, 80, 300, 70).is_ok());
    }

    #[test]
    fn pool_budget_rejects_overcommit() {
        // 150 + 80 = 230 > 300 - 80 = 220 → rejected.
        assert!(check_pool_budget(150, 80, 300, 80).is_err());
        // Classic footgun: defaults left at 150/80 against a native Postgres
        // still at max_connections=100.
        assert!(check_pool_budget(150, 80, 100, 20).is_err());
    }

    #[test]
    fn pool_budget_reserve_larger_than_server_saturates_not_panics() {
        // reserve > server_max must not underflow; budget floors at 0 → rejected.
        assert!(check_pool_budget(1, 1, 10, 50).is_err());
    }
}
