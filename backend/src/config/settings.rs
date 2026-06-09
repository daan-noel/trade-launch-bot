use std::sync::OnceLock;
use std::time::Duration;

/// All configuration loaded from environment variables at startup.
/// Validated once — panics early if required values are missing.
#[derive(Debug, Clone)]
pub struct Settings {
    // --- Helius ---
    #[allow(dead_code)]
    pub helius_api_key: String,
    /// WSS URL from `HELIUS_WS_URL`, or built from the key:
    /// wss://atlas-mainnet.helius-rpc.com/?api-key=<key>
    pub helius_ws_url: String,
    pub helius_rpc_url: String,
    pub helius_sender_url: String,

    // --- Solana ---
    pub wallet_private_key: String,
    pub nonce_accounts: Vec<String>,

    // --- Trading ---
    pub compute_unit_limit: u64,
    pub compute_unit_price: u64,
    pub buy_seed_pool_size: usize,
    /// Jito tip per transaction, in SOL.
    pub jito_tip_sol: f64,
    /// How many times `sell_token` retries before giving up.
    pub max_sell_attempts: usize,
    /// How many times we poll for transaction confirmation.
    pub confirm_max_retries: usize,
    /// Delay between confirmation polls.
    pub confirm_poll: Duration,

    // --- Helius subscription ---
    pub subscription_method: String,

    // --- Price feeds ---
    /// Jupiter price API base (versioned; e.g. https://api.jup.ag/price/v3).
    pub jupiter_price_api_url: String,
    /// CoinGecko simple-price endpoint (query params appended in code).
    pub coingecko_price_url: String,
    /// How often the SOL/USD poller refreshes.
    pub sol_price_poll: Duration,

    // --- Outbound HTTP ---
    /// Request timeout for the shared third-party HTTP client.
    pub http_timeout: Duration,

    // --- Timing ---
    /// How often to send a WS ping (keepalive)
    pub ping_interval: Duration,
    /// How long to wait before reconnecting after a drop
    pub reconnect_interval: Duration,

    // --- Database ---
    pub database_url: String,

    // --- Server ---
    pub host: String,
    pub port: u16,
    pub http_enabled: bool,
    pub http_workers: usize,
    /// CORS allow-origin; "*" allows any origin.
    pub cors_allowed_origin: String,
}

impl Settings {
    /// Load from environment. Call `dotenvy::dotenv()` before this.
    pub fn from_env() -> anyhow::Result<Self> {
        let api_key = required("HELIUS_API_KEY")?;
        // Prefer an explicit HELIUS_WS_URL; otherwise build the default Atlas URL from the key.
        let default_ws_url = format!("wss://atlas-mainnet.helius-rpc.com/?api-key={}", api_key);
        let ws_url = env_or("HELIUS_WS_URL", &default_ws_url);

        Ok(Self {
            helius_ws_url: ws_url,
            helius_api_key: api_key,
            helius_rpc_url: required("HELIUS_RPC_URL")?,
            helius_sender_url: required("HELIUS_FAST_SENDER_URL")?,
            wallet_private_key: required("WALLET_PRIVATE_KEY")?,
            nonce_accounts: parse_required_list("NONCE_ACCOUNTS")?,
            compute_unit_limit: env_parse("COMPUTE_UNIT_LIMIT", 200_000)?,
            compute_unit_price: env_parse("COMPUTE_UNIT_PRICE", 1_000_000)?,
            buy_seed_pool_size: env_parse("BUY_SEED_POOL_SIZE", 16)?,
            jito_tip_sol: env_parse("JITO_TIP_SOL", 0.0002)?,
            max_sell_attempts: env_parse("MAX_SELL_ATTEMPTS", 5)?,
            confirm_max_retries: env_parse("CONFIRM_MAX_RETRIES", 5)?,
            confirm_poll: Duration::from_millis(env_parse("CONFIRM_POLL_MS", 1_000)?),
            subscription_method: env_or("SUBSCRIPTION_METHOD", "transactionSubscribe"),
            jupiter_price_api_url: env_or("JUPITER_PRICE_API_URL", "https://api.jup.ag/price/v3"),
            coingecko_price_url: env_or(
                "COINGECKO_PRICE_URL",
                "https://api.coingecko.com/api/v3/simple/price",
            ),
            sol_price_poll: Duration::from_secs(env_parse("SOL_PRICE_POLL_SECONDS", 60)?),
            http_timeout: Duration::from_secs(env_parse("HTTP_TIMEOUT_SECONDS", 10)?),
            ping_interval: Duration::from_millis(env_parse("PING_INTERVAL", 30_000)?),
            reconnect_interval: Duration::from_millis(env_parse("RECONNECT_INTERVAL", 10_000)?),
            database_url: required("DATABASE_URL")?,
            host: env_or("HOST", "127.0.0.1"),
            port: env_parse("PORT", 8081)?,
            http_enabled: env_or("HTTP_ENABLED", "true").parse().unwrap_or(true),
            http_workers: env_parse("HTTP_WORKERS", 2)?,
            cors_allowed_origin: env_or("CORS_ALLOWED_ORIGIN", "*"),
        })
    }
}

// ---------------------------------------------------------------------------
// Process-wide accessor
//
// Settings is loaded once at startup and never mutated. Most consumers receive
// it via explicit injection (TraderConfig, function params), but a few leaf
// helpers (price clients, the shared HTTP client) are reached from deep call
// stacks where threading would be noise — they read it here instead. Mirrors
// the OnceLock pattern already used for the shared HTTP client.
// ---------------------------------------------------------------------------

static GLOBAL: OnceLock<Settings> = OnceLock::new();

/// Install the process-wide settings. Call once at startup, right after `from_env`.
pub fn init_global(settings: Settings) {
    let _ = GLOBAL.set(settings);
}

/// Access the process-wide settings. Panics if `init_global` was not called.
pub fn get() -> &'static Settings {
    GLOBAL
        .get()
        .expect("Settings::init_global must be called before settings::get")
}

/// Like `get`, but returns `None` instead of panicking — for early/global
/// initializers that may run before `init_global`.
pub fn try_get() -> Option<&'static Settings> {
    GLOBAL.get()
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
