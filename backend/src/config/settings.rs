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

    // --- Database ---
    pub database_url: String,

    // --- Server ---
    pub host: String,
    pub port: u16,
    pub http_enabled: bool,
    pub http_workers: usize,
}

impl Settings {
    /// Load from environment. Call `dotenvy::dotenv()` before this.
    pub fn from_env() -> anyhow::Result<Self> {
        let api_key = required("HELIUS_API_KEY")?;

        Ok(Self {
            helius_api_key: api_key,
            helius_rpc_url: required("HELIUS_RPC_URL")?,
            helius_sender_urls: sender_urls()?,
            helius_laserstream_url: required("HELIUS_LASERSTREAM_URL")?,
            wallet_private_key: required("WALLET_PRIVATE_KEY")?,
            nonce_accounts: parse_required_list("NONCE_ACCOUNTS")?,
            reconnect_interval: Duration::from_millis(env_parse("RECONNECT_INTERVAL", 10_000)?),
            database_url: required("DATABASE_URL")?,
            host: env_or("HOST", "127.0.0.1"),
            port: env_parse("PORT", 8081)?,
            http_enabled: env_or("HTTP_ENABLED", "true").parse().unwrap_or(true),
            http_workers: env_parse("HTTP_WORKERS", 2)?,
        })
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
