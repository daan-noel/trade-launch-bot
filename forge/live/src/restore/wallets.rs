//! Restore step 1 — rebuild `managed_wallets` from the keystore folder.
//!
//! On a fresh box the DB is empty but the operator copied the keystore's `.enc`
//! blobs across. Each blob holds ONLY the encrypted ed25519 secret — no address,
//! role, or lifecycle metadata (all of that lived in Postgres). So we enumerate
//! the top-level `{role}-{uuid}.enc` files, decrypt each to recover its pubkey,
//! and insert a fresh `managed_wallets` row keyed by that address. Fresh DB ids
//! are assigned (the keystore filename UUID is NOT the old row id — it was
//! discarded at generation time); on-chain data joins by address, so this is
//! functionally lossless. Idempotent (`insert_if_absent`), so a re-run is a no-op.

use std::collections::HashMap;
use std::path::Path;
use std::str::FromStr;

use anyhow::{Context, Result};
use platform_core::models::{NewManagedWallet, WalletRole};
use platform_core::storage::repositories::ManagedWalletRepo;
use sqlx::PgPool;
use tracing::{info, warn};
use uuid::Uuid;

use launcher::{resolve_signer, EnvKek, LauncherSettings};

/// The outcome of the wallet-restore step: how many NEW rows landed, plus the
/// lookup maps the later backfill step needs (all resolved wallets by address, and
/// the dev-wallet subset used to link discovered launches back to their creator).
pub struct WalletRestore {
    /// Count of rows that were freshly inserted this run (a re-run reports 0).
    pub inserted: usize,
    /// address → (managed_wallet_id, role) for EVERY restored wallet — the probe
    /// set backfill interns trades against and reconcile probes for balances. The
    /// backfill step derives its dev-wallet subset (own-launch detection) from the
    /// `role` here, so no separate map is stored.
    pub by_address: HashMap<String, (Uuid, String)>,
}

/// Enumerate the keystore's top-level wallet blobs, decrypt each to its address,
/// and insert idempotently. Returns the lookup maps for the backfill step.
///
/// Skips the `mints/` subdirectory (ephemeral launch mint keys, not wallets) and
/// any non-`.enc` file (e.g. `export-*.base58` operator dumps). A blob whose role
/// prefix isn't a known [`WalletRole`], or that fails to decrypt, is logged and
/// skipped — one bad file never aborts the whole restore.
pub async fn restore_wallets(pool: &PgPool, settings: &LauncherSettings) -> Result<WalletRestore> {
    let dir = settings.keystore_dir.as_path();
    let kek = EnvKek::from_passphrase(&settings.kek_passphrase);

    let mut inserted = 0usize;
    let read = std::fs::read_dir(dir)
        .with_context(|| format!("read keystore dir {}", dir.display()))?;
    for entry in read {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                warn!(?e, "skip unreadable keystore entry");
                continue;
            }
        };
        let path = entry.path();
        // Top-level `.enc` files only — skip the `mints/` subdir and any base58 dumps.
        if !path.is_file() {
            continue;
        }
        let Some((role, key_ref)) = parse_wallet_file(&path) else {
            continue;
        };

        let address = match resolve_signer(dir, &key_ref, &kek) {
            Ok(signer) => signer.pubkey().to_string(),
            Err(e) => {
                warn!(%key_ref, ?e, "skip wallet blob — decrypt/resolve failed (wrong KEK?)");
                continue;
            }
        };

        let landed = ManagedWalletRepo::insert_if_absent(
            pool,
            &NewManagedWallet {
                address: address.clone(),
                label: None,
                role: role.as_str().to_string(),
                key_ref,
                derivation_index: None,
            },
        )
        .await
        .with_context(|| format!("insert restored wallet {address}"))?;
        if landed {
            inserted += 1;
        }
    }

    // Re-read the full set so the map carries the DB-assigned ids (both freshly
    // inserted and any that pre-existed a re-run). One query, small table.
    let all = ManagedWalletRepo::list_all(pool, None).await?;
    let mut by_address = HashMap::with_capacity(all.len());
    for w in all {
        by_address.insert(w.address, (w.id, w.role));
    }

    info!(inserted, total = by_address.len(), "restored wallets from keystore");
    Ok(WalletRestore { inserted, by_address })
}

/// Parse a keystore path into `(role, key_ref)` when it's a top-level wallet blob
/// named `{role}-{uuid}.enc` with a recognized role. `key_ref` is the filename
/// (keystore-relative, as [`resolve_signer`] expects). Returns `None` for the
/// `mints/` subdir contents, non-`.enc` files, or an unknown role prefix.
fn parse_wallet_file(path: &Path) -> Option<(WalletRole, String)> {
    let name = path.file_name()?.to_str()?;
    if !name.ends_with(".enc") {
        return None;
    }
    // Role is the prefix before the first '-' (`dev-<uuid>.enc`, `bundler-<uuid>.enc`).
    let role_str = name.split('-').next()?;
    let role = WalletRole::from_str(role_str).ok()?;
    Some((role, name.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parses_known_roles_and_skips_others() {
        let dev = PathBuf::from("dev-1e5b8c4a-0000-0000-0000-000000000000.enc");
        assert_eq!(
            parse_wallet_file(&dev),
            Some((WalletRole::Dev, dev.file_name().unwrap().to_str().unwrap().to_string()))
        );
        let bundler = PathBuf::from("bundler-abc.enc");
        assert!(matches!(parse_wallet_file(&bundler), Some((WalletRole::Bundler, _))));

        // A launch mint key (lives under mints/, but even by name it's not a role).
        assert_eq!(parse_wallet_file(&PathBuf::from("mints/deadbeef.enc")), None);
        // Operator base58 export dumps.
        assert_eq!(parse_wallet_file(&PathBuf::from("export-somewallet.base58")), None);
        // Unknown role prefix.
        assert_eq!(parse_wallet_file(&PathBuf::from("hotwallet-xyz.enc")), None);
    }
}
