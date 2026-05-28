use std::time::Duration;

/// All configuration loaded from environment variables at startup.
/// Validated once — panics early if required values are missing.
#[derive(Debug, Clone)]
pub struct Settings {
    // --- Helius ---
    #[allow(dead_code)]
    pub helius_api_key: String,
    /// Fully constructed WSS URL: wss://atlas-mainnet.helius-rpc.com/?api-key=<key>
    pub helius_ws_url: String,
    pub helius_rpc_url: String,
    pub helius_sender_url: String,

    // --- Solana ---
    pub pump_program_id: String,
    pub wallet_private_key: String,
    pub nonce_accounts: Vec<String>,

    // --- Trading ---
    pub compute_unit_limit: u64,
    pub compute_unit_price: u64,
    pub buy_seed_pool_size: usize,

    // --- Helius subscription ---
    pub subscription_method: String,

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
}

impl Settings {
    /// Load from environment. Call `dotenvy::dotenv()` before this.
    pub fn from_env() -> anyhow::Result<Self> {
        let api_key = required("HELIUS_API_KEY")?;
        let ws_url = format!("wss://atlas-mainnet.helius-rpc.com/?api-key={}", api_key);

        Ok(Self {
            helius_ws_url: ws_url,
            helius_api_key: api_key,
            helius_rpc_url: required("HELIUS_RPC_URL")?,
            helius_sender_url: required("HELIUS_FAST_SENDER_URL")?,
            pump_program_id: required("PUMP_PROGRAM_ID")?,
            wallet_private_key: required("WALLET_PRIVATE_KEY")?,
            nonce_accounts: parse_required_list("NONCE_ACCOUNTS")?,
            compute_unit_limit: env_parse("COMPUTE_UNIT_LIMIT", 250_000)?,
            compute_unit_price: env_parse("COMPUTE_UNIT_PRICE", 1_000_000)?,
            buy_seed_pool_size: env_parse("BUY_SEED_POOL_SIZE", 16)?,
            subscription_method: env_or("SUBSCRIPTION_METHOD", "transactionSubscribe"),
            ping_interval: Duration::from_millis(env_parse("PING_INTERVAL", 30_000)?),
            reconnect_interval: Duration::from_millis(env_parse("RECONNECT_INTERVAL", 10_000)?),
            database_url: required("DATABASE_URL")?,
            host: env_or("HOST", "127.0.0.1"),
            port: env_parse("PORT", 8081)?,
        })
    }
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
