//! CLI: decrypt one managed wallet and print its base58 private key.
//!
//! DANGER: this is the ONLY path that emits raw private-key material. It is
//! gated behind an explicit `--i-understand` flag and writes to a 0600 file by
//! default (stdout only with `--stdout`, which lands in shell history). Every
//! runtime signing path uses `keystore::resolve_signer` instead, which never
//! exposes bytes. Needs `LAUNCHER_KEK_PASSPHRASE` + the encrypted keystore.

use anyhow::{bail, Context, Result};
use platform_core::config::Settings;
use platform_core::storage::connect;
use platform_core::storage::repositories::ManagedWalletRepo;
use solana_sdk::signature::{Keypair, Signer};

use crate::config::LauncherSettings;
use crate::keystore::{resolve_secret_bytes, EnvKek};

/// `wallet-export <wallet_id> --i-understand [--stdout]` — decrypt a managed
/// wallet and emit its base58 (Phantom-style) private key.
pub async fn run_wallet_export(settings: &Settings, args: &[String]) -> Result<()> {
    let wallet_id_arg = args
        .first()
        .context("usage: wallet-export <wallet_id> --i-understand [--stdout]")?;
    let confirmed = args.iter().any(|a| a == "--i-understand");
    let to_stdout = args.iter().any(|a| a == "--stdout");

    if !confirmed {
        bail!(
            "refusing to export raw private key without --i-understand \
             (usage: wallet-export <wallet_id> --i-understand [--stdout])"
        );
    }

    let wallet_id = uuid::Uuid::parse_str(wallet_id_arg).context("parse wallet_id")?;
    let launcher = LauncherSettings::from_env()?;
    let pools = connect(settings).await?;

    let wallet = ManagedWalletRepo::get(&pools.hot, wallet_id)
        .await?
        .context("managed wallet not found")?;

    let kek = EnvKek::from_passphrase(&launcher.kek_passphrase);
    let secret = resolve_secret_bytes(&launcher.keystore_dir, &wallet.key_ref, &kek)?;

    // Confirm the decrypted secret derives the address on file before emitting it.
    let kp = Keypair::from_bytes(secret.as_ref()).context("invalid keypair bytes")?;
    let derived = kp.pubkey().to_string();
    if derived != wallet.address {
        bail!(
            "decrypted key derives {derived} but managed_wallets row says {} — refusing to export",
            wallet.address
        );
    }

    let base58 = bs58::encode(secret.as_ref()).into_string();

    if to_stdout {
        eprintln!("WARNING: printing private key to stdout — it will land in shell history / logs");
        println!("{base58}");
        return Ok(());
    }

    let out = launcher
        .keystore_dir
        .join(format!("export-{wallet_id}.base58"));
    write_secret_file(&out, base58.as_bytes())
        .with_context(|| format!("write private key to {}", out.display()))?;
    println!("wrote base58 private key for {derived} -> {}", out.display());
    println!("DELETE this file after importing — it is unencrypted key material.");
    Ok(())
}

/// Write `bytes` to `path` with owner-only permissions (0600 on unix; on Windows
/// the keystore dir ACL is the boundary).
fn write_secret_file(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    std::fs::write(path, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}
