//! CLI: encrypt a Solana keypair into the managed-wallet keystore.

use anyhow::{bail, Context, Result};
use std::path::Path;

use crate::config::LauncherSettings;
use crate::keystore::{EnvKek, write_envelope_to_keystore};

/// `wallet-encrypt <source_keypair.json> <key_ref>` — writes an envelope blob under
/// `WALLET_KEYSTORE` / `key_ref` using `LAUNCHER_KEK_PASSPHRASE`.
pub fn run_wallet_encrypt(args: &[String]) -> Result<()> {
    let source = args
        .first()
        .context("usage: wallet-encrypt <source_keypair> <key_ref>")?;
    let key_ref = args
        .get(1)
        .context("usage: wallet-encrypt <source_keypair> <key_ref>")?;
    if args.len() > 2 {
        bail!("usage: wallet-encrypt <source_keypair> <key_ref>");
    }

    let settings = LauncherSettings::from_env()?;
    let kek = EnvKek::from_passphrase(&settings.kek_passphrase);
    let address = write_envelope_to_keystore(
        &settings.keystore_dir,
        key_ref,
        Path::new(source),
        &kek,
    )?;

    println!(
        "encrypted {} -> {}/{} (pubkey {})",
        source,
        settings.keystore_dir.display(),
        key_ref,
        address
    );
    Ok(())
}
