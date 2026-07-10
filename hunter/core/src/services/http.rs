use std::sync::OnceLock;

pub const USER_AGENT: &str =
    "Mozilla/5.0 (compatible; Hunter/1.0; +https://github.com/your-org/hunter)";

static HTTP: OnceLock<reqwest::Client> = OnceLock::new();

/// Shared outbound HTTP client for third-party REST APIs.
pub fn client() -> &'static reqwest::Client {
    HTTP.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .user_agent(USER_AGENT)
            .build()
            .expect("failed to build HTTP client")
    })
}
