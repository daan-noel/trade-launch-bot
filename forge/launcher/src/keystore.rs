//! Envelope-encrypted wallet keystore (ADR D3).
//!
//! `managed_wallets.key_ref` is a path **relative to** `WALLET_KEYSTORE` pointing
//! at a JSON blob `{ wrapped_dek, dek_nonce, secret_nonce, ciphertext }`. The
//! ed25519 secret is decrypted in-process and wrapped as `Arc<dyn Signer>` for
//! pump-trader — never stored in Postgres.

use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use solana_sdk::signature::{Keypair, Signer};
use std::path::Path;
use std::sync::Arc;
use uuid::Uuid;
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

/// Resolve `key_ref` → the raw 64-byte ed25519 secret (scrubbed on drop).
///
/// DANGER: this returns unwrapped private-key material. Only the `wallet-export`
/// CLI path uses it; every runtime signing path must use [`resolve_signer`],
/// which keeps the secret wrapped in `Arc<dyn Signer>` and never exposes bytes.
pub fn resolve_secret_bytes(
    keystore_dir: &Path,
    key_ref: &str,
    kek: &dyn Kek,
) -> Result<Zeroizing<Vec<u8>>> {
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
    // Validate length + that it forms a real ed25519 keypair before handing back.
    Keypair::from_bytes(secret.as_ref()).context("invalid ed25519 secret length")?;
    Ok(Zeroizing::new(secret.to_vec()))
}

/// Read a 64-byte ed25519 secret from a Solana CLI JSON keypair file or a base58 string file.
pub fn read_keypair_bytes(path: &Path) -> Result<Zeroizing<Vec<u8>>> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("read keypair file {}", path.display()))?;
    let trimmed = raw.trim();
    if trimmed.starts_with('[') {
        let bytes: Vec<u8> = serde_json::from_str(trimmed).context("parse keypair JSON array")?;
        if bytes.len() != 64 {
            bail!("keypair JSON must be 64 bytes, got {}", bytes.len());
        }
        return Ok(Zeroizing::new(bytes));
    }
    let bytes = bs58::decode(trimmed)
        .into_vec()
        .context("decode keypair as base58")?;
    if bytes.len() != 64 {
        bail!("keypair base58 must decode to 64 bytes, got {}", bytes.len());
    }
    Ok(Zeroizing::new(bytes))
}

/// Write a v1 envelope blob. `key_ref` is relative to `keystore_dir` when used from
/// [`write_envelope_to_keystore`]; `path` may also be absolute.
pub fn write_envelope(path: &Path, secret: &[u8], kek: &dyn Kek) -> Result<()> {
    if secret.len() != 64 {
        bail!("ed25519 secret must be 64 bytes");
    }
    use base64::{engine::general_purpose::STANDARD, Engine};
    use rand::RngCore;

    let kek_key = kek.derive_aes_key();
    let dek_cipher = Aes256Gcm::new_from_slice(&kek_key).context("KEK cipher")?;
    let dek_nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let mut dek_raw = [0u8; 32];
    OsRng.fill_bytes(&mut dek_raw);
    let wrapped_dek = dek_cipher
        .encrypt(&dek_nonce, dek_raw.as_ref())
        .map_err(|e| anyhow::anyhow!("wrap DEK: {e}"))?;

    let secret_cipher = Aes256Gcm::new_from_slice(&dek_raw).context("DEK cipher")?;
    let secret_nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ciphertext = secret_cipher
        .encrypt(&secret_nonce, secret)
        .map_err(|e| anyhow::anyhow!("encrypt secret: {e}"))?;

    let blob = serde_json::json!({
        "v": 1,
        "wrapped_dek": STANDARD.encode(wrapped_dek),
        "dek_nonce": STANDARD.encode(dek_nonce),
        "secret_nonce": STANDARD.encode(secret_nonce),
        "ciphertext": STANDARD.encode(ciphertext),
    });
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(path, serde_json::to_vec_pretty(&blob)?)?;
    Ok(())
}

/// Encrypt `source_keypair` into `keystore_dir` / `key_ref` and return the public address.
pub fn write_envelope_to_keystore(
    keystore_dir: &Path,
    key_ref: &str,
    source_keypair: &Path,
    kek: &dyn Kek,
) -> Result<String> {
    let secret = read_keypair_bytes(source_keypair)?;
    let address = Keypair::from_bytes(&secret)
        .context("invalid keypair bytes")?
        .pubkey()
        .to_string();
    let out = keystore_dir.join(key_ref);
    write_envelope(&out, &secret, kek)?;
    Ok(address)
}

#[derive(Debug, Deserialize)]
struct EnvelopeBlob {
    v: u8,
    wrapped_dek: String,
    dek_nonce: String,
    secret_nonce: String,
    ciphertext: String,
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

/// The keystore-relative path a launch's ephemeral mint key is stored at. Derived
/// deterministically from the launch id, so the atomic-bundle submit path and the
/// confirm watcher's re-bid both find it without a DB column.
pub fn mint_key_ref(launch_id: Uuid) -> String {
    format!("mints/{launch_id}.enc")
}

/// Persist the launch **mint** keypair, envelope-encrypted (same KEK path as
/// wallets), so a dropped atomic-launch bundle can be re-signed on a re-bid — the
/// create leg lives INSIDE the bundle, so an all-or-nothing drop means create never
/// happened and the whole bundle (create included) must be rebuilt. Deleted on any
/// terminal outcome via [`delete_mint_key`]. Returns the key_ref (relative to the
/// keystore dir); it is also re-derivable from the launch id via [`mint_key_ref`].
pub fn write_mint_key(
    keystore_dir: &Path,
    launch_id: Uuid,
    mint: &Keypair,
    kek: &dyn Kek,
) -> Result<String> {
    let key_ref = mint_key_ref(launch_id);
    let out = keystore_dir.join(&key_ref);
    write_envelope(&out, &mint.to_bytes(), kek)?;
    Ok(key_ref)
}

/// Delete a persisted launch mint key (terminal-outcome cleanup). Idempotent — a
/// missing file is not an error (a re-bid path may run this more than once, or none
/// was ever written for a bundle-less launch).
pub fn delete_mint_key(keystore_dir: &Path, key_ref: &str) -> Result<()> {
    let path = keystore_dir.join(key_ref);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("delete mint key {}", path.display())),
    }
}

/// SPL token program id string for a launch variant.
pub fn token_program_for_variant(variant: &str) -> &'static str {
    if variant.contains("create_v1") {
        pump_trader::protocol::TOKEN_PROGRAM_ID
    } else {
        pump_trader::protocol::TOKEN_2022_PROGRAM_ID
    }
}

/// Inverse of [`token_program_for_variant`]: the launch `variant` string for a
/// token's on-chain program id. The legacy SPL Token program means a `create_v1`
/// launch; anything else (Token-2022, or an unknown/absent program) maps to
/// `create_v2`, the current pump.fun default — matching the `else` branch of
/// [`token_program_for_variant`]. Used by the keystore-restore backfill to label a
/// discovered own-launch's `launches.variant` from its decoded create tx.
pub fn variant_for_token_program(token_program_id: Option<&str>) -> &'static str {
    if token_program_id == Some(pump_trader::protocol::TOKEN_PROGRAM_ID) {
        "pumpfun.create_v1"
    } else {
        "pumpfun.create_v2"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::signature::Keypair;
    use std::fs;

    #[test]
    fn envelope_roundtrip_resolves_same_pubkey() {
        let dir = std::env::temp_dir().join(format!("keystore-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let kp = Keypair::new();
        let src = dir.join("src.json");
        fs::write(&src, serde_json::to_string(&kp.to_bytes().to_vec()).unwrap()).unwrap();
        let kek = EnvKek::from_passphrase("test-passphrase");
        let key_ref = "dev-test.enc";
        write_envelope_to_keystore(&dir, key_ref, &src, &kek).unwrap();
        let signer = resolve_signer(&dir, key_ref, &kek).unwrap();
        assert_eq!(signer.pubkey(), kp.pubkey());
        let _ = fs::remove_dir_all(&dir);
    }
}
