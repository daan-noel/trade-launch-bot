//! Envelope-encrypted wallet keystore (ADR D3).
//!
//! `managed_wallets.key_ref` is a path **relative to** `WALLET_KEYSTORE` pointing
//! at a JSON blob `{ wrapped_dek, dek_nonce, secret_nonce, ciphertext }`. The
//! ed25519 secret is decrypted in-process and wrapped as `Arc<dyn Signer>` for
//! pump-trader — never stored in Postgres.

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use solana_sdk::signature::{Keypair, Signer};
use std::path::Path;
use std::sync::Arc;
use zeroize::Zeroizing;

/// Key-encryption-key source (env/passphrase now → AWS KMS later).
pub trait Kek {
    fn derive_aes_key(&self) -> [u8; 32];
}

/// KEK from a passphrase env var (SHA-256 stretched to 32 bytes).
pub struct EnvKek {
    key: [u8; 32],
}

impl EnvKek {
    pub fn from_passphrase(passphrase: &str) -> Self {
        let digest = Sha256::digest(passphrase.as_bytes());
        let mut key = [0u8; 32];
        key.copy_from_slice(&digest);
        Self { key }
    }
}

impl Kek for EnvKek {
    fn derive_aes_key(&self) -> [u8; 32] {
        self.key
    }
}

#[derive(Debug, Deserialize)]
struct EnvelopeBlob {
    v: u8,
    wrapped_dek: String,
    dek_nonce: String,
    secret_nonce: String,
    ciphertext: String,
}

/// Resolve `key_ref` → signing handle. `key_ref` is relative to `keystore_dir`.
pub fn resolve_signer(
    keystore_dir: &Path,
    key_ref: &str,
    kek: &dyn Kek,
) -> Result<Arc<dyn Signer + Send + Sync>> {
    let path = keystore_dir.join(key_ref);
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("read keystore blob {}", path.display()))?;
    let blob: EnvelopeBlob =
        serde_json::from_str(&raw).context("parse envelope JSON keystore blob")?;
    if blob.v != 1 {
        bail!("unsupported keystore envelope version {}", blob.v);
    }
    let kek_key = kek.derive_aes_key();
    let dek = decrypt_aes(
        &kek_key,
        &decode_b64(&blob.dek_nonce)?,
        &decode_b64(&blob.wrapped_dek)?,
    )
    .context("unwrap DEK")?;
    let secret = Zeroizing::new(decrypt_aes(
        dek.as_ref(),
        &decode_b64(&blob.secret_nonce)?,
        &decode_b64(&blob.ciphertext)?,
    )?);
    let kp = Keypair::from_bytes(&secret).context("invalid ed25519 secret length")?;
    Ok(Arc::new(kp))
}

fn decrypt_aes(key: &[u8], nonce_bytes: &[u8], ciphertext: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    if nonce_bytes.len() != 12 {
        bail!("AES-GCM nonce must be 12 bytes");
    }
    let cipher = Aes256Gcm::new_from_slice(key).context("AES key length")?;
    let nonce = Nonce::from_slice(nonce_bytes);
    let plain = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| anyhow::anyhow!("AES-GCM decrypt: {e}"))?;
    Ok(Zeroizing::new(plain))
}

fn decode_b64(s: &str) -> Result<Vec<u8>> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    STANDARD.decode(s).context("base64 decode")
}

/// SPL token program id string for a launch variant.
pub fn token_program_for_variant(variant: &str) -> &'static str {
    if variant.contains("create_v1") {
        pump_trader::protocol::TOKEN_PROGRAM_ID
    } else {
        pump_trader::protocol::TOKEN_2022_PROGRAM_ID
    }
}
