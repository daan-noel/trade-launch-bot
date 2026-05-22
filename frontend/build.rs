// build.rs — reads POLL_INTERVAL_MS from .env (or env) and emits a Rust source
// file so the value is available as a proper `const u32` at compile time.
use std::{env, fs, path::Path};

fn main() {
    // Re-run whenever .env or the shell env variable changes.
    println!("cargo:rerun-if-changed=.env");
    println!("cargo:rerun-if-env-changed=POLL_INTERVAL_MS");
    println!("cargo:rerun-if-env-changed=API_BASE");

    // Priority: .env file first, then shell environment, then fallback.
    let raw_ms = read_dotenv_var("../.env", "POLL_INTERVAL_MS")
        .or_else(|| read_dotenv_var(".env", "POLL_INTERVAL_MS"))
        .or_else(|| env::var("POLL_INTERVAL_MS").ok())
        .unwrap_or_else(|| "5000".to_string());
    let api_base = read_dotenv_var("../.env", "API_BASE")
        .or_else(|| read_dotenv_var(".env", "API_BASE"))
        .or_else(|| env::var("API_BASE").ok())
        .unwrap_or_else(|| "http://127.0.0.1:8081".to_string());

    let ms: u32 = raw_ms.trim().parse().unwrap_or(5_000);

    let out_dir = env::var("OUT_DIR").expect("OUT_DIR not set");
    let dest = Path::new(&out_dir).join("env_config.rs");
    fs::write(
        &dest,
        format!(
            "/// Injected build-time frontend config.\npub const POLL_INTERVAL_MS: u32 = {};\npub const API_BASE: &str = \"{}\";\n",
            ms,
            api_base,
        ),
    )
    .expect("failed to write env_config.rs");
}

/// Minimal `.env` parser — returns the value for `key` if found.
fn read_dotenv_var(filename: &str, key: &str) -> Option<String> {
    let content = fs::read_to_string(filename).ok()?;
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('#') || !line.contains('=') {
            continue;
        }
        let mut parts = line.splitn(2, '=');
        let k = parts.next()?.trim();
        if k == key {
            return Some(parts.next()?.trim().to_string());
        }
    }
    None
}
