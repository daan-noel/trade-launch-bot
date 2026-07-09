//! Wallet-pool backup (wallet-pool Phase 4): copy the encrypted keystore +
//! export the current `managed_wallets` table, run after each generation batch.
//!
//! SECURITY: this backup contains ENCRYPTED keystore blobs + `key_ref`s, never
//! raw key material and never the KEK passphrase — `LAUNCHER_KEK_PASSPHRASE`
//! must be stored SEPARATELY from these backups (never bundled together). See
//! `docs/wallet-pool-plan.md` Phase 4 for the restore runbook.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;
use platform_core::models::{ManagedWallet, WalletStatus};
use platform_core::storage::repositories::ManagedWalletRepo;
use sqlx::PgPool;
use tracing::{info, warn};

use crate::config::LauncherSettings;

const BACKUP_README: &str = "This directory holds ENCRYPTED wallet keystore blobs (keystore/) and a \
managed_wallets export (managed_wallets.json). It does NOT contain the KEK \
passphrase (LAUNCHER_KEK_PASSPHRASE) -- that must be stored SEPARATELY, never \
alongside this backup. See docs/wallet-pool-plan.md Phase 4 for the restore \
runbook (decrypt one known wallet and confirm the derived address before \
trusting the pool).\n";

/// Run one backup pass into a fresh timestamped subdirectory of `backup_root`:
/// - `managed_wallets.json` — every wallet row (`ManagedWallet::to_backup_json`,
///   full fidelity including `key_ref`), all statuses, for audit/restore.
/// - `keystore/` — the encrypted blob for every **non-`retired`** wallet only
///   (backup retention: a retired wallet has no live value, so it's dropped
///   from new backups to shrink the exposure surface — its row still exists in
///   the JSON export for history).
///
/// Best-effort by design: the caller (wallet generation) should log and
/// continue on `Err`, never fail the generation itself over a backup problem.
pub async fn run_backup(
    pool: &PgPool,
    settings: &LauncherSettings,
    backup_root: &Path,
) -> Result<PathBuf> {
    let stamp = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let dir = backup_root.join(format!("wallet-pool-{stamp}"));
    let keystore_dir = dir.join("keystore");
    std::fs::create_dir_all(&keystore_dir).context("create backup keystore dir")?;
    std::fs::write(dir.join("README.txt"), BACKUP_README).context("write backup README")?;

    let wallets = ManagedWalletRepo::list_all(pool, None).await?;
    let export: Vec<_> = wallets.iter().map(ManagedWallet::to_backup_json).collect();
    let export_bytes = serde_json::to_vec_pretty(&export).context("serialize managed_wallets export")?;
    std::fs::write(dir.join("managed_wallets.json"), export_bytes)
        .context("write managed_wallets export")?;

    let mut copied = 0usize;
    let mut skipped_retired = 0usize;
    for wallet in &wallets {
        if wallet.status == WalletStatus::Retired.as_str() {
            skipped_retired += 1;
            continue;
        }
        let src = settings.keystore_dir.join(&wallet.key_ref);
        let dest = keystore_dir.join(&wallet.key_ref);
        if let Some(parent) = dest.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                warn!(wallet_id = %wallet.id, %e, "backup: failed to create nested keystore dir");
                continue;
            }
        }
        match std::fs::copy(&src, &dest) {
            Ok(_) => copied += 1,
            Err(e) => warn!(
                wallet_id = %wallet.id,
                key_ref = %wallet.key_ref,
                %e,
                "backup: failed to copy keystore blob"
            ),
        }
    }

    info!(
        dir = %dir.display(),
        copied,
        skipped_retired,
        total = wallets.len(),
        "wallet-pool backup complete"
    );
    Ok(dir)
}
