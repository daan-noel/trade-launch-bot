//! One-shot launch probe — balance check + create-only (no bundle) for debugging.

use anyhow::{bail, Context, Result};
use platform_core::config::Settings;
use platform_core::storage::connect;
use platform_core::storage::repositories::{
    LaunchTemplateRepo, ManagedWalletRepo, MetadataTemplateRepo,
};
use pump_trader::{CreateTokenV2Args, PumpFunTrader};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signer};
use std::str::FromStr;
use tracing::info;

use crate::config::LauncherSettings;
use crate::keystore::{self, EnvKek};
use crate::service::PumpfunTemplateParams;
use crate::trader_config::build_launch_trader_config;

/// `launch-probe [template_id] [dev_wallet_id]` — create-only smoke test.
pub async fn run_launch_probe(settings: &Settings, args: &[String]) -> Result<()> {
    let launcher = LauncherSettings::from_env()?;
    let pools = connect(settings).await?;

    let (template_id, dev_wallet_id) = if args.len() >= 2 {
        (
            uuid::Uuid::parse_str(&args[0]).context("parse template_id")?,
            uuid::Uuid::parse_str(&args[1]).context("parse dev_wallet_id")?,
        )
    } else {
        let templates = LaunchTemplateRepo::all(&pools.hot).await?;
        let devs = ManagedWalletRepo::by_role(&pools.hot, "dev").await?;
        let template = templates
            .first()
            .context("no launch_templates — run scripts/seed-dev-launch.sql")?;
        let dev = devs.first().context("no dev managed_wallets — run seed")?;
        (template.id, dev.id)
    };

    let template = LaunchTemplateRepo::get(&pools.hot, template_id)
        .await?
        .context("template not found")?;
    let dev_wallet = ManagedWalletRepo::get(&pools.hot, dev_wallet_id)
        .await?
        .context("dev wallet not found")?;

    let params: PumpfunTemplateParams =
        serde_json::from_value(template.params.clone()).context("parse template params")?;

    // Token identity comes from the linked metadata template (single source of
    // truth), same as launcher::service::execute_launch.
    let metadata_template_id = template
        .metadata_template_id
        .context("launch template has no metadata_template_id — link one before probing")?;
    let metadata = MetadataTemplateRepo::get(&pools.hot, metadata_template_id)
        .await?
        .context("metadata template not found")?;

    let kek = EnvKek::from_passphrase(&launcher.kek_passphrase);
    let signer = keystore::resolve_signer(
        &launcher.keystore_dir,
        &dev_wallet.key_ref,
        &kek,
    )?;
    let creator = signer.pubkey();

    let nonce_accounts: Vec<Pubkey> = launcher
        .nonce_accounts
        .iter()
        .map(|s| Pubkey::from_str(s).with_context(|| format!("parse nonce pubkey {s}")))
        .collect::<Result<_>>()?;

    let trader_config = build_launch_trader_config(&launcher, signer.clone(), nonce_accounts);
    let mut trader = PumpFunTrader::new(trader_config);
    trader.initialize().await.context("initialize pump-trader")?;

    let balance = trader.get_sol_balance().await.context("fetch dev wallet balance")?;
    info!(
        dev = %creator,
        balance_sol = balance as f64 / pump_trader::protocol::LAMPORTS_PER_SOL as f64,
        "dev wallet balance"
    );
    if balance < 20_000_000 {
        bail!(
            "dev wallet {creator} has only {:.4} SOL — fund with at least 0.05 SOL before launching",
            balance as f64 / pump_trader::protocol::LAMPORTS_PER_SOL as f64
        );
    }

    let mint = Keypair::new();
    let args = CreateTokenV2Args {
        name: metadata.name.clone(),
        symbol: metadata.symbol.clone(),
        uri: metadata.uri.clone(),
        creator,
        is_mayhem_mode: params.is_mayhem_mode,
        cashback_enabled: params.cashback_enabled,
    };

    info!(mint = %mint.pubkey(), "submitting create_v2 (probe — no DB row, no bundle)");
    let sig = trader.create_token_v2(&mint, &args, true).await?;
    info!(
        mint = %mint.pubkey(),
        sig = %sig,
        solscan = "https://solscan.io/tx/{}",
        sig
    );
    println!("CREATE OK");
    println!("mint: {}", mint.pubkey());
    println!("signature: {sig}");
    println!("https://solscan.io/tx/{sig}");
    Ok(())
}
