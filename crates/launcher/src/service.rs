//! Launch execution: template + dev wallet → create (+ dev-buy) → DB rows.

use anyhow::{bail, Context, Result};
use platform_core::models::{NewLaunch, NewToken};
use platform_core::storage::repositories::{
    BundleRepo, LaunchRepo, LaunchTemplateRepo, ManagedWalletRepo, TokenMarketStateRepo,
    TokenRepo,
};
use platform_core::models::TokenMarketState;
use chrono::Utc;
use pump_trader::{
    CreateTokenArgs, CreateTokenV2Args, PumpFunTrader,
};
use serde::Deserialize;
use serde_json::json;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signer};
use sqlx::PgPool;
use std::str::FromStr;
use tracing::info;
use uuid::Uuid;

use crate::bundle_execute::{execute_bundle, BundleExecuteResult};
use crate::config::LauncherSettings;
use crate::keystore::{self, EnvKek};
use crate::trader_config::build_launch_trader_config;

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
    /// Optional post-create sniper bundle (composer draws from `leg_structures`).
    #[serde(default)]
    pub bundle_leg_count: Option<u32>,
    #[serde(default)]
    pub bundle_wallet_ids: Option<Vec<Uuid>>,
    #[serde(default)]
    pub bundle_quote_per_leg: Option<i64>,
    #[serde(default)]
    pub bundle_tip_quote: Option<i64>,
    #[serde(default)]
    pub leg_structures: Option<Vec<crate::bundle::LegStructureRecipe>>,
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
    /// Present when the template planned a bundle and auto-submit succeeded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bundle: Option<BundleExecuteResult>,
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

    let trader_config = build_launch_trader_config(settings, signer, nonce_accounts);
    let mut trader = PumpFunTrader::new(trader_config);
    trader.initialize().await.context("initialize pump-trader")?;

    let balance = trader.get_sol_balance().await.context("fetch dev wallet balance")?;
    if balance < 20_000_000 {
        bail!(
            "dev wallet {creator} has only {:.4} SOL — fund with at least 0.05 SOL before launching",
            balance as f64 / pump_trader::protocol::LAMPORTS_PER_SOL as f64
        );
    }

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

        // Wallet-pool lifecycle: a reserved dev wallet is now consumed (terminal —
        // never re-claimable). A no-op for wallets not currently `reserved` (e.g.
        // today's free-form-selected wallets, until Phase 3 wires pool claiming
        // into wallet selection).
        if let Err(e) = ManagedWalletRepo::mark_used(pool, &[dev_wallet.id]).await {
            tracing::warn!(%launch_id, dev_wallet_id = %dev_wallet.id, %e, "failed to mark dev wallet used");
        }

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

        if let Some((leg_count, wallets, quote_per_leg, tip_quote)) =
            crate::bundle::parse_bundle_plan(&params)?
        {
            let recipes = params
                .leg_structures
                .as_deref()
                .filter(|p| !p.is_empty())
                .context("bundle_leg_count requires leg_structures pool")?;
            let composed =
                crate::bundle::compose_bundle_legs(recipes, leg_count, &wallets, quote_per_leg)?;
            let bundle = BundleRepo::insert(
                pool,
                launch_id,
                tip_quote,
                crate::bundle::legs_to_json(&composed),
            )
            .await?;
            LaunchRepo::set_bundle_id(pool, launch_id, bundle.id).await?;
            info!(launch_id = %launch_id, bundle_id = %bundle.id, legs = leg_count, "bundle planned");
        }

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
            bundle: None,
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

    let mut result = finish?;

    // Auto-submit a planned sniper bundle — no second HTTP call when the template
    // has `bundle_leg_count`. Launch is already on-chain; bundle failure does not
    // roll back the create.
    if let Some(bundle_id) = LaunchRepo::get(pool, launch_id)
        .await?
        .and_then(|l| l.bundle_id)
    {
        if BundleRepo::get(pool, bundle_id)
            .await?
            .is_some_and(|b| b.status == "planned")
        {
            let bundle_result = execute_bundle(pool, settings, bundle_id)
                .await
                .context("launch created on-chain but bundle auto-submit failed")?;
            info!(
                launch_id = %launch_id,
                bundle_id = %bundle_id,
                jito_bundle_id = %bundle_result.jito_bundle_id,
                "bundle auto-submitted after launch"
            );
            result.bundle = Some(bundle_result);
        }
    }

    Ok(result)
}
