//! Execute a planned bundle: build signed buy txs → Jito `sendBundle`.

use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use platform_core::storage::repositories::{
    BundleRepo, LaunchRepo, ManagedWalletRepo, TokenRepo,
};
use pump_trader::types::TokenProgram;
use pump_trader::{PumpFunTrader};
use serde::Serialize;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::transaction::Transaction;
use sqlx::PgPool;
use std::str::FromStr;
use tracing::info;
use uuid::Uuid;

use crate::bundle::{leg_params, legs_from_json};
use crate::config::LauncherSettings;
use crate::keystore::{self, EnvKek};
use crate::trader_config::build_launch_trader_config;

#[derive(Debug, Clone, Serialize)]
pub struct BundleExecuteResult {
    pub bundle_id: Uuid,
    pub jito_bundle_id: String,
    pub leg_signatures: Vec<String>,
}

/// Build + submit a `planned` bundle's buy legs to Jito.
pub async fn execute_bundle(
    pool: &PgPool,
    settings: &LauncherSettings,
    bundle_id: Uuid,
) -> Result<BundleExecuteResult> {
    let bundle = BundleRepo::get(pool, bundle_id)
        .await?
        .context("bundle not found")?;
    if bundle.status != "planned" {
        bail!(
            "bundle {} is status={}, expected planned",
            bundle_id,
            bundle.status
        );
    }

    let launch = LaunchRepo::get(pool, bundle.launch_id)
        .await?
        .context("launch not found for bundle")?;
    if launch.create_signature.is_none() {
        bail!("launch must be created on-chain before bundle execute");
    }

    let token = TokenRepo::get(pool, &launch.mint_address)
        .await?
        .context("token row not found")?;

    let legs = legs_from_json(&bundle.legs)?;
    if legs.is_empty() {
        bail!("bundle has no legs");
    }

    let kek = EnvKek::from_passphrase(&settings.kek_passphrase);
    let first_wallet = ManagedWalletRepo::get(pool, legs[0].wallet_id)
        .await?
        .context("bundler wallet not found")?;
    if first_wallet.role != "bundler" {
        bail!(
            "wallet {} is role={}, expected bundler",
            first_wallet.id,
            first_wallet.role
        );
    }

    let primary_signer = keystore::resolve_signer(
        &settings.keystore_dir,
        &first_wallet.key_ref,
        &kek,
    )?;

    let nonce_accounts: Vec<Pubkey> = settings
        .nonce_accounts
        .iter()
        .map(|s| Pubkey::from_str(s).with_context(|| format!("parse nonce pubkey {s}")))
        .collect::<Result<_>>()?;

    let trader_config = build_launch_trader_config(settings, primary_signer, nonce_accounts);
    let mut trader = PumpFunTrader::new(trader_config);
    trader.initialize().await.context("initialize pump-trader")?;

    let mint = Pubkey::from_str(&launch.mint_address).context("parse mint")?;
    let creator = Pubkey::from_str(&token.creator_wallet).context("parse creator")?;
    let token_program_id = token
        .token_program_id
        .as_deref()
        .unwrap_or_else(|| keystore::token_program_for_variant(&launch.variant));
    let token_program = TokenProgram::from_id(token_program_id);
    let cashback_enabled = token
        .meta
        .get("cashback_enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let blockhash = trader.fresh_blockhash().await?;

    BundleRepo::set_status(pool, bundle_id, "submitting").await?;

    let finish = async {
        let mut txs = Vec::with_capacity(legs.len());
        let mut leg_signatures = Vec::with_capacity(legs.len());

        for leg in &legs {
            let variant = pump_trader::BundleBuyVariant::parse(&leg.structure.variant)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let wallet = ManagedWalletRepo::get(pool, leg.wallet_id)
                .await?
                .context("bundler wallet not found")?;
            if wallet.role != "bundler" {
                bail!(
                    "wallet {} is role={}, expected bundler",
                    wallet.id,
                    wallet.role
                );
            }
            let signer =
                keystore::resolve_signer(&settings.keystore_dir, &wallet.key_ref, &kek)?;
            let buy_lamports = leg.quote_amount.max(0) as u64;
            let params = leg_params(&leg.structure);
            let tx = trader
                .build_bundle_leg_tx(
                    signer.as_ref(),
                    blockhash,
                    &mint,
                    &creator,
                    token_program,
                    buy_lamports,
                    cashback_enabled,
                    variant,
                    &params,
                )
                .await?;
            leg_signatures.push(tx.signatures[0].to_string());
            txs.push(tx);
        }

        let jito_bundle_id = submit_jito_bundle(&settings.jito_block_engine_url, &txs).await?;
        BundleRepo::set_submitted(pool, bundle_id, &jito_bundle_id, &leg_signatures).await?;

        // Wallet-pool lifecycle: bundler wallets stay `reserved` here — submit
        // isn't completion. `launcher::confirm` transitions them to `used` once
        // the bundle reaches a terminal landed/dropped/partial outcome.

        info!(
            bundle_id = %bundle_id,
            launch_id = %bundle.launch_id,
            legs = legs.len(),
            jito_bundle_id = %jito_bundle_id,
            "bundle submitted to Jito"
        );

        Ok(BundleExecuteResult {
            bundle_id,
            jito_bundle_id,
            leg_signatures,
        })
    }
    .await;

    if finish.is_err() {
        if let Err(mark_err) = BundleRepo::set_status(pool, bundle_id, "failed").await {
            tracing::warn!(%bundle_id, %mark_err, "failed to mark bundle failed");
        }
    }

    finish
}

async fn submit_jito_bundle(url: &str, txs: &[Transaction]) -> Result<String> {
    let mut encoded = Vec::with_capacity(txs.len());
    for tx in txs {
        encoded.push(STANDARD.encode(bincode::serialize(tx)?));
    }

    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "sendBundle",
        "params": [encoded]
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(url)
        .json(&body)
        .send()
        .await
        .context("Jito sendBundle HTTP")?;
    let status = resp.status();
    let text = resp.text().await.context("Jito sendBundle body")?;
    if !status.is_success() {
        bail!("Jito sendBundle HTTP {status}: {text}");
    }
    let v: serde_json::Value = serde_json::from_str(&text).context("parse Jito response")?;
    if let Some(err) = v.get("error") {
        bail!("Jito sendBundle error: {err}");
    }
    v.get("result")
        .context("Jito response missing result")?
        .as_str()
        .context("Jito bundle id not a string")
        .map(str::to_string)
}
