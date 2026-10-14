//! Envelope-encrypted wallet files. Same blob shape as forge: AES-256-GCM, a
//! random data key wrapped by the passphrase. The database stores the filename.

use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm,
};
use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use rand::RngCore;
use sha2::{Digest, Sha256};
use solana_sdk::signature::Keypair;
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

const ENVELOPE_V: u32 = 1;

#[derive(serde::Deserialize, serde::Serialize)]
struct EnvelopeBlob {
    v: u32,
    wrapped_dek: String,
    dek_nonce: String,
    secret_nonce: String,
    ciphertext: String,
}

pub struct Kek {
    key: [u8; 32],
}

impl Kek {
    pub fn from_passphrase(passphrase: &str) -> Self {
        let digest = Sha256::digest(passphrase.as_bytes());
        let mut key = [0u8; 32];
        key.copy_from_slice(&digest);
        Self { key }
    }
}

/// A wallet name is the filename stem. It cannot leave the keystore directory.
pub fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return false;
    }
    let rest_ok = chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    rest_ok && name.len() <= 32
}

pub fn key_ref_for(name: &str) -> String {
    format!("{name}.enc")
}

fn blob_path(dir: &Path, key_ref: &str) -> Result<PathBuf> {
    if key_ref.contains('/') || key_ref.contains('\\') || key_ref.contains("..") {
        bail!("keystore filename is not a single file");
    }
    Ok(dir.join(key_ref))
}

pub fn write_keypair(dir: &Path, key_ref: &str, secret: &[u8], kek: &Kek) -> Result<()> {
    if secret.len() != 64 {
        bail!("ed25519 secret must be 64 bytes");
    }
    let kek_key = kek.key;
    let dek_cipher = Aes256Gcm::new_from_slice(&kek_key).context("KEK cipher")?;
    let dek_nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let mut dek_raw = [0u8; 32];
    OsRng.fill_bytes(&mut dek_raw);
    let wrapped_dek = dek_cipher
        .encrypt(&dek_nonce, dek_raw.as_ref())
        .map_err(|e| anyhow::anyhow!("wrap data key: {e}"))?;

    let secret_cipher = Aes256Gcm::new_from_slice(&dek_raw).context("data-key cipher")?;
    let secret_nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ciphertext = secret_cipher
        .encrypt(&secret_nonce, secret)
        .map_err(|e| anyhow::anyhow!("encrypt secret: {e}"))?;

    let blob = EnvelopeBlob {
        v: ENVELOPE_V,
        wrapped_dek: STANDARD.encode(wrapped_dek),
        dek_nonce: STANDARD.encode(dek_nonce),
        secret_nonce: STANDARD.encode(secret_nonce),
        ciphertext: STANDARD.encode(ciphertext),
    };
    std::fs::create_dir_all(dir)?;
    let path = blob_path(dir, key_ref)?;
    // Never replace a file: it may be the only copy of another wallet's key.
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::AlreadyExists => anyhow::anyhow!("keystore file {key_ref} already exists"),
            _ => anyhow::Error::new(e).context(format!("create keystore file {}", path.display())),
        })?;
    std::io::Write::write_all(&mut file, &serde_json::to_vec_pretty(&blob)?)?;
    Ok(())
}

pub fn read_keypair(dir: &Path, key_ref: &str, kek: &Kek) -> Result<Keypair> {
    let path = blob_path(dir, key_ref)?;
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("read keystore file {}", path.display()))?;
    let blob: EnvelopeBlob = serde_json::from_str(&raw).context("parse keystore envelope")?;
    if blob.v != ENVELOPE_V {
        bail!("unsupported keystore envelope version {}", blob.v);
    }
    let dek = decrypt(&kek.key, &decode(&blob.dek_nonce)?, &decode(&blob.wrapped_dek)?)
        .context("unwrap data key")?;
    let secret = Zeroizing::new(
        decrypt(dek.as_ref(), &decode(&blob.secret_nonce)?, &decode(&blob.ciphertext)?)
            .context("decrypt secret")?,
    );
    Keypair::from_bytes(&secret).context("invalid ed25519 secret")
}

pub fn parse_secret(raw: &str) -> Result<Zeroizing<Vec<u8>>> {
    let trimmed = raw.trim();
    if trimmed.starts_with('[') {
        let bytes: Vec<u8> = serde_json::from_str(trimmed).context("parse keypair JSON array")?;
        if bytes.len() != 64 {
            bail!("keypair JSON must be 64 bytes");
        }
        return Ok(Zeroizing::new(bytes));
    }
    let bytes = bs58::decode(trimmed)
        .into_vec()
        .context("decode keypair as base58")?;
    if bytes.len() != 64 {
        bail!("keypair base58 must decode to 64 bytes");
    }
    Ok(Zeroizing::new(bytes))
}

fn decrypt(key: &[u8], nonce: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>> {
    let cipher = Aes256Gcm::new_from_slice(key).context("cipher")?;
    let nonce = aes_gcm::Nonce::from_slice(nonce);
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| anyhow::anyhow!("decrypt failed"))
}

fn decode(value: &str) -> Result<Vec<u8>> {
    STANDARD.decode(value).context("base64")
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::signature::Signer;

    #[test]
    fn a_name_is_a_single_stem() {
        assert!(valid_name("main"));
        assert!(valid_name("scale-2"));
        assert!(!valid_name(""));
        assert!(!valid_name("../x"));
        assert!(!valid_name("Main"));
    }

    #[test]
    fn a_roundtrip_returns_the_same_pubkey() {
        let dir = std::env::temp_dir().join(format!("hunter-keystore-{}", uuid::Uuid::new_v4()));
        let kp = Keypair::new();
        let secret = kp.to_bytes();
        let kek = Kek::from_passphrase("test-passphrase");
        write_keypair(&dir, "main.enc", &secret, &kek).unwrap();
        let back = read_keypair(&dir, "main.enc", &kek).unwrap();
        assert_eq!(kp.pubkey(), back.pubkey());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_second_write_to_the_same_name_keeps_the_first_key() {
        let dir = std::env::temp_dir().join(format!("hunter-keystore-{}", uuid::Uuid::new_v4()));
        let kek = Kek::from_passphrase("test-passphrase");
        let first = Keypair::new();
        write_keypair(&dir, "main.enc", &first.to_bytes(), &kek).unwrap();
        let err = write_keypair(&dir, "main.enc", &Keypair::new().to_bytes(), &kek).unwrap_err();
        assert!(err.to_string().contains("already exists"));
        assert_eq!(read_keypair(&dir, "main.enc", &kek).unwrap().pubkey(), first.pubkey());
        let _ = std::fs::remove_dir_all(dir);
    }
}
