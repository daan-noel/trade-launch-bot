//! Launch execution: template + dev wallet → create (+ dev-buy) → DB rows.

use anyhow::{bail, Context, Result};
use platform_core::models::{NewLaunch, NewToken};
use platform_core::storage::repositories::{
    LaunchRepo, LaunchTemplateRepo, ManagedWalletRepo, TokenMarketStateRepo, TokenRepo,
};
use platform_core::models::TokenMarketState;
use chrono::Utc;
use pump_trader::{
    CreateTokenArgs, CreateTokenV2Args, PumpFunTrader, TraderConfig,
};
use serde::Deserialize;
use serde_json::json;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signer};
use sqlx::PgPool;
use std::str::FromStr;
use std::sync::Arc;
use tracing::info;
use uuid::Uuid;

use crate::config::LauncherSettings;
use crate::keystore::{self, EnvKek};

/// Parsed `launch_templates.params` brain for pump.fun create_v2.
#[derive(Debug, Deserialize)]
pub struct PumpfunTemplateParams {
    pub name: String,
    pub symbol: String,
    pub uri: String,
    #[serde(default)]
    pub dev_buy_quote: Option<i64>,
    #[serde(default)]
    pub slippage_bps: Option<u64>,
    #[serde(default)]
    pub is_mayhem_mode: bool,
    #[serde(default)]
    pub cashback_enabled: bool,
}

#[derive(Debug, Clone)]
pub struct LaunchRequest {
    pub template_id: Uuid,
    pub dev_wallet_id: Uuid,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LaunchResult {
    pub launch_id: Uuid,
    pub mint_address: String,
    pub create_signature: String,
}

/// Execute one launch from a stored template + dev wallet.
pub async fn execute_launch(
    pool: &PgPool,
    settings: &LauncherSettings,
    req: LaunchRequest,
) -> Result<LaunchResult> {
    let template = LaunchTemplateRepo::get(pool, req.template_id)
        .await?
        .context("launch template not found")?;
    let dev_wallet = ManagedWalletRepo::get(pool, req.dev_wallet_id)
        .await?
        .context("dev wallet not found")?;
    if dev_wallet.role != "dev" {
        bail!("wallet {} is role={}, expected dev", dev_wallet.id, dev_wallet.role);
    }

    let params: PumpfunTemplateParams =
        serde_json::from_value(template.params.clone()).context("parse template params")?;

    let kek = EnvKek::from_passphrase(&settings.kek_passphrase);
    let signer = keystore::resolve_signer(
        &settings.keystore_dir,
        &dev_wallet.key_ref,
        &kek,
    )?;
    let creator = signer.pubkey();

    let nonce_accounts: Vec<Pubkey> = settings
        .nonce_accounts
        .iter()
        .map(|s| Pubkey::from_str(s).with_context(|| format!("parse nonce pubkey {s}")))
        .collect::<Result<_>>()?;

    let trader_config = Arc::new(TraderConfig::new(
        settings.rpc_url.clone(),
        settings.sender_urls.clone(),
        signer,
        nonce_accounts,
    ));
    let mut trader = PumpFunTrader::new(trader_config);
    trader.initialize().await.context("initialize pump-trader")?;

    let mint = Keypair::new();
    let mint_address = mint.pubkey().to_string();

    let dev_buy_quote = params.dev_buy_quote.unwrap_or(0);
    let pending = LaunchRepo::insert(
        pool,
        &NewLaunch {
            template_id: Some(template.id),
            mint_address: mint_address.clone(),
            launchpad_id: template.launchpad_id,
            variant: template.variant.clone(),
            quote_asset_id: template.quote_asset_id,
            dev_wallet_id: Some(dev_wallet.id),
            dev_buy_quote: if dev_buy_quote > 0 {
                Some(dev_buy_quote)
            } else {
                None
            },
            status: Some("pending".into()),
        },
    )
    .await?;

    let launch_id = pending.id;
    let finish = async {
        let dev_buy_sol = dev_buy_quote as f64 / pump_trader::protocol::LAMPORTS_PER_SOL as f64;
        let (signature, ix_label) = match template.variant.as_str() {
            "pumpfun.create_v2" | "pumpfun.create_v2_devbuy" => {
                let args = CreateTokenV2Args {
                    name: params.name.clone(),
                    symbol: params.symbol.clone(),
                    uri: params.uri.clone(),
                    creator,
                    is_mayhem_mode: params.is_mayhem_mode,
                    cashback_enabled: params.cashback_enabled,
                };
                let sig = if dev_buy_quote > 0 {
                    trader
                        .create_token_v2_and_dev_buy(
                            &mint,
                            &args,
                            dev_buy_sol,
                            params.slippage_bps,
                            true,
                        )
                        .await?
                } else {
                    trader.create_token_v2(&mint, &args, true).await?
                };
                (sig, "Pump.Fun: Create_v2")
            }
            "pumpfun.create_v1" | "pumpfun.create_v1_devbuy" => {
                let args = CreateTokenArgs {
                    name: params.name.clone(),
                    symbol: params.symbol.clone(),
                    uri: params.uri.clone(),
                    creator,
                };
                let sig = if dev_buy_quote > 0 {
                    trader
                        .create_token_and_dev_buy(
                            &mint,
                            &args,
                            dev_buy_sol,
                            params.slippage_bps,
                            true,
                        )
                        .await?
                } else {
                    trader.create_token(&mint, &args, true).await?
                };
                (sig, "Pump.Fun: Create")
            }
            other => bail!("unsupported launch variant: {other}"),
        };

        LaunchRepo::set_created(pool, launch_id, &signature, "created").await?;

        let token_program = keystore::token_program_for_variant(&template.variant);
        TokenRepo::insert(
            pool,
            &NewToken {
                mint_address: mint_address.clone(),
                launchpad_id: template.launchpad_id,
                quote_asset_id: template.quote_asset_id,
                creator_wallet: creator.to_string(),
                is_own_launch: true,
                name: params.name.clone(),
                symbol: params.symbol.clone(),
                decimals: 6,
                token_program_id: Some(token_program.to_string()),
                initial_supply_base: None,
                initial_buy_quote: if dev_buy_quote > 0 {
                    Some(dev_buy_quote)
                } else {
                    None
                },
                creation_slot: None,
                creation_tx_signature: signature.clone(),
                ix_labels: Some(json!([ix_label])),
                meta: Some(json!({
                    "is_mayhem_mode": params.is_mayhem_mode,
                    "cashback_enabled": if template.variant.contains("create_v1") {
                        false
                    } else {
                        params.cashback_enabled
                    },
                    "template_id": template.id,
                    "launch_id": launch_id,
                })),
                created_at: None,
            },
        )
        .await?;

        TokenMarketStateRepo::upsert(
            pool,
            &TokenMarketState {
                mint_address: mint_address.clone(),
                current_price_quote: None,
                ath_price_quote: None,
                ath_at: None,
                volume_quote: dev_buy_quote.max(0),
                trade_count: if dev_buy_quote > 0 { 1 } else { 0 },
                last_trade_at: None,
                is_dead: false,
                is_migrated: false,
                updated_at: Utc::now(),
            },
        )
        .await?;

        info!(
            launch_id = %launch_id,
            mint = %mint_address,
            sig = %signature,
            "launch completed"
        );

        Ok(LaunchResult {
            launch_id,
            mint_address,
            create_signature: signature,
        })
    }
    .await;

    if let Err(e) = finish {
        if let Err(mark_err) = LaunchRepo::set_failed(pool, launch_id, "failed").await {
            tracing::warn!(%launch_id, %mark_err, "failed to mark launch failed after chain error");
        } else {
            tracing::warn!(%launch_id, error = %e, "launch failed after pending row inserted");
        }
        return Err(e);
    }

    finish
}
