//! CLI: decrypt a keystore blob and confirm its derived pubkey — the wallet-pool
//! Phase 4 restore runbook's "decrypt one known wallet and confirm the derived
//! address matches before trusting the pool" step.

use anyhow::{bail, Context, Result};

use crate::config::LauncherSettings;
use crate::keystore::{self, EnvKek};

/// `wallet-verify <key_ref> <expected_address>` — resolves `key_ref` through the
/// keystore (using `WALLET_KEYSTORE` + `LAUNCHER_KEK_PASSPHRASE`) and checks the
/// derived pubkey against `expected_address`. Exits non-zero on mismatch/error
/// so it's usable as a restore-runbook gate, not just a manual eyeball check.
pub fn run_wallet_verify(args: &[String]) -> Result<()> {
    let key_ref = args
        .first()
        .context("usage: wallet-verify <key_ref> <expected_address>")?;
    let expected_address = args
        .get(1)
        .context("usage: wallet-verify <key_ref> <expected_address>")?;
    if args.len() > 2 {
        bail!("usage: wallet-verify <key_ref> <expected_address>");
    }

    let settings = LauncherSettings::from_env()?;
    let kek = EnvKek::from_passphrase(&settings.kek_passphrase);
    let signer = keystore::resolve_signer(&settings.keystore_dir, key_ref, &kek)?;
    let derived = signer.pubkey().to_string();

    if &derived == expected_address {
        println!("PASS: {} -> {} (matches expected)", key_ref, derived);
        Ok(())
    } else {
        bail!("FAIL: {key_ref} decrypts to {derived}, expected {expected_address}");
    }
}
