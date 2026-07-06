//! Launcher runtime settings (Helius RPC/sender, nonce accounts, keystore path).

use anyhow::{bail, Context, Result};
use std::path::PathBuf;

/// Env-backed settings for the launch executor.
#[derive(Debug, Clone)]
pub struct LauncherSettings {
    pub rpc_url: String,
    pub sender_urls: Vec<String>,
    pub nonce_accounts: Vec<String>,
    /// Directory containing envelope-encrypted wallet blobs (`key_ref` is relative).
    pub keystore_dir: PathBuf,
    /// Passphrase for [`super::keystore::EnvKek`] (wraps ed25519 secrets at rest).
    pub kek_passphrase: String,
}

impl LauncherSettings {
    pub fn from_env() -> Result<Self> {
        let rpc_url = std::env::var("HELIUS_RPC_URL")
            .or_else(|_| std::env::var("RPC_URL"))
            .context("HELIUS_RPC_URL (or RPC_URL) required for launcher")?;
        let sender_urls = sender_urls_from_env()?;
        if sender_urls.is_empty() {
            bail!("at least one sender URL required (HELIUS_FAST_SENDER_URL or HELIUS_SENDER_URLS)");
        }
        let nonce_raw = std::env::var("NONCE_ACCOUNTS")
            .context("NONCE_ACCOUNTS required for launcher (comma-separated pubkeys)")?;
        let nonce_accounts: Vec<String> = nonce_raw
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        if nonce_accounts.is_empty() {
            bail!("NONCE_ACCOUNTS parsed empty");
        }
        let keystore_dir = std::env::var("WALLET_KEYSTORE")
            .map(PathBuf::from)
            .context("WALLET_KEYSTORE required (directory of envelope-encrypted wallet blobs)")?;
        let kek_passphrase = std::env::var("LAUNCHER_KEK_PASSPHRASE")
            .or_else(|_| std::env::var("WALLET_KEK_PASSPHRASE"))
            .context("LAUNCHER_KEK_PASSPHRASE (or WALLET_KEK_PASSPHRASE) required")?;
        Ok(Self {
            rpc_url,
            sender_urls,
            nonce_accounts,
            keystore_dir,
            kek_passphrase,
        })
    }
}

fn sender_urls_from_env() -> Result<Vec<String>> {
    if let Ok(list) = std::env::var("HELIUS_SENDER_URLS") {
        let urls: Vec<String> = list
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        if !urls.is_empty() {
            return Ok(urls);
        }
    }
    if let Ok(one) = std::env::var("HELIUS_FAST_SENDER_URL") {
        if !one.is_empty() {
            return Ok(vec![one]);
        }
    }
    Ok(Vec::new())
}
