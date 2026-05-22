use std::time::Duration;

/// All configuration loaded from environment variables at startup.
/// Validated once — panics early if required values are missing.
#[derive(Debug, Clone)]
pub struct Settings {
    // --- Helius ---
    #[allow(dead_code)]
    pub helius_api_key: String,
    /// Fully constructed WSS URL: wss://atlas-mainnet.helius-rpc.com?api-key=<key>
    pub helius_ws_url: String,

    // --- Solana ---
    pub pump_program_id: String,

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
            pump_program_id: required("PUMP_PROGRAM_ID")?,
            subscription_method: env_or("SUBSCRIPTION_METHOD", "transactionSubscribe"),
            ping_interval: Duration::from_millis(env_parse("PING_INTERVAL", 30_000)?),
            reconnect_interval: Duration::from_millis(env_parse("RECONNECT_INTERVAL", 10_000)?),
            database_url: required("DATABASE_URL")?,
            host: env_or("HOST", "127.0.0.1"),
            port: env_parse("PORT", 8081)?,
        })
    }
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
