use std::sync::OnceLock;

pub const USER_AGENT: &str =
    "Mozilla/5.0 (compatible; MemeTrading/1.0; +https://github.com/your-org/meme-trading)";

static HTTP: OnceLock<reqwest::Client> = OnceLock::new();

/// Shared outbound HTTP client for third-party REST APIs.
pub fn client() -> &'static reqwest::Client {
    HTTP.get_or_init(|| {
        // Read the configured timeout; fall back to 10s if settings aren't
        // installed yet (this client is lazily built on first use).
        let timeout = crate::config::settings::try_get()
            .map(|s| s.http_timeout)
            .unwrap_or_else(|| std::time::Duration::from_secs(10));
        reqwest::Client::builder()
            .timeout(timeout)
            .user_agent(USER_AGENT)
            .build()
            .expect("failed to build HTTP client")
    })
}
