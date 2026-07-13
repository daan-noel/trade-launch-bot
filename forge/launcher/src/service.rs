//! Launch execution: template + dev wallet → create (+ dev-buy) → DB rows.

use anyhow::{bail, Context, Result};
use platform_core::models::{BundleStatus, LaunchStatus, NewLaunch, NewToken, WalletRole};
use platform_core::storage::repositories::{
    BundleRepo, LaunchRepo, LaunchTemplateRepo, ManagedWalletRepo, MetadataTemplateRepo,
    TokenMarketStateRepo, TokenPositionRepo, TokenRepo,
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

use crate::bundle_execute::{execute_bundle_with_trader, BundleExecuteResult};
use crate::config::LauncherSettings;
use crate::keystore::{self, EnvKek};
use crate::trader_config::build_launch_trader_config;

/// pump.fun mints are always 6 decimals. Deliberately a launcher-local const:
/// `launcher` and `ingest-host` (which has its own `map::PUMP_TOKEN_DECIMALS`)
/// share only the venue-neutral `platform-core`, so this pump-specific fact can't
/// live in one shared home without coupling the two decoupled crates. Keep this
/// value equal to `ingest_host::map::PUMP_TOKEN_DECIMALS`.
const PUMP_TOKEN_DECIMALS: i16 = 6;

/// Create-side rent-exempt deposits + base/priority fees a launch locks up,
/// EXCLUDING the Jito tip and the dev-buy spend — keyed by launch **variant**,
/// because a legacy `create_v1` costs ~2× a `create_v2`.
///
/// These are ~all *recoverable rent* (mint, on-chain metadata, bonding-curve state,
/// the bonding-curve + dev ATAs) — measured on mainnet to be ~97% rent with no flat
/// protocol fee — so they're deterministic and carried as MEASURED constants rather
/// than a live RPC read, so the background funder (`FundPlan`) derives the SAME dev
/// target without a live trader (gate + funder can't drift). Re-measure if pump
/// changes the create account layout.
///
/// - `create_v2` (Token-2022, metadata embedded in the mint): measured **12,275,000**
///   on-chain (sig `454sgqt8…`). Floored to 13,000,000.
/// - `create_v1` (legacy SPL-Token + a SEPARATE ~0.006 SOL Metaplex metadata account,
///   and a larger overall account set): measured **~24,800,000** (sig `5ZNix…`, from
///   the balance at the reverted dev-buy). Floored to 26,000,000. **Under-budgeting
///   this is the exact bug that silently under-funded the dev-buy** — the create
///   succeeded but the fused buy ran the wallet negative → System
///   `ResultWithNegativeLamports` (`Custom(1)`) → opaque on-chain revert.
///
/// This floor varies by CREATE variant ONLY: the dev-buy contributes a fixed dev-ATA
/// (already in the measured set) plus the `dev_buy_quote` SPEND, which is budgeted as
/// a separate term — so a hand-authored dev-buy ix layout does not change it.
pub(crate) fn create_rent_and_fee_lamports(variant: &str) -> u64 {
    if variant.contains("create_v1") {
        26_000_000 // ~0.026 SOL — legacy SPL + Metaplex metadata account
    } else {
        13_000_000 // ~0.013 SOL — create_v2 (Token-2022)
    }
}

/// Minimum dev-wallet balance (lamports) to cover a create's rent + fees + the
/// live-configured Jito tip, on top of any dev-buy spend. `tip_ceiling_lamports`
/// is the MAX tip a launch bundle could draw (`JITO_MAX_TIP_SOL`), budgeted so a
/// re-bid that escalates the tip to the ceiling can't strand the launch
/// under-funded. The create floor is variant-aware (see
/// [`create_rent_and_fee_lamports`]). `pub(crate)` so the `probe` pre-launch check
/// reuses the same floor instead of hardcoding (and re-drifting) its own copy.
pub(crate) fn min_dev_launch_lamports(variant: &str, tip_ceiling_lamports: u64) -> u64 {
    create_rent_and_fee_lamports(variant) + tip_ceiling_lamports
}

/// Typed error for the pre-launch dev-wallet balance gate so the HTTP layer can
/// surface it as an actionable 400 ("needs 0.06 SOL, has 0.049") instead of the
/// blanket 500 every other `anyhow` error collapses to. Carries the
/// already-formatted, operator-facing message.
#[derive(Debug)]
pub struct InsufficientDevBalance(pub String);

impl std::fmt::Display for InsufficientDevBalance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for InsufficientDevBalance {}

/// Map the DB `launch_templates.variant` (`"pumpfun.create_v1"` / `_v2`) to the
/// venue **catalog** create variant name the orchestrator plan carries. This is
/// the one place the free-text launch-variant string is translated onto the
/// catalog SSOT (the create tx itself is still built by the matching `create_*`
/// trader method below — the plan documents + audits it).
fn create_variant_name(template_variant: &str) -> &'static str {
    match template_variant {
        "pumpfun.create_v2" => "create_v2",
        _ => "create",
    }
}

/// Parsed `launch_templates.params` brain for pump.fun create_v2. Token identity
/// (name/symbol/uri) is NOT here — it lives in the linked `metadata_templates`
/// row (`launch_templates.metadata_template_id`), the single source of truth.
#[derive(Debug, Deserialize)]
pub struct PumpfunTemplateParams {
    #[serde(default)]
    pub dev_buy_quote: Option<i64>,
    /// The dev-buy curve encoding (catalog name: `buy` / `buy_exact_sol_in` /
    /// `buy_v2` / `buy_exact_quote_in_v2`). Independent of the bundler legs' encoding
    /// so the dev buy can diverge from the co-buys. `None` ⇒ `buy_exact_sol_in` (the
    /// historical dev-buy encoding). Resolve via [`Self::dev_buy_variant_name`].
    #[serde(default)]
    pub dev_buy_variant: Option<String>,
    #[serde(default)]
    pub slippage_bps: Option<u64>,
    #[serde(default)]
    pub is_mayhem_mode: bool,
    #[serde(default)]
    pub cashback_enabled: bool,
    /// Optional post-create sniper bundle (composer draws from `leg_structures`).
    /// Default leg count when the launch request doesn't override it (see
    /// `LaunchRequest::bundler_count` / the Wallet Management "use N bundlers"
    /// control) — a template can still pre-configure a default.
    #[serde(default)]
    pub bundle_leg_count: Option<u32>,
    #[serde(default)]
    pub bundle_quote_per_leg: Option<i64>,
    #[serde(default)]
    pub bundle_tip_quote: Option<i64>,
    #[serde(default)]
    pub leg_structures: Option<Vec<crate::bundle::LegStructureRecipe>>,
    /// Hand-picked create-tx ix layout (orders CU/tip around the fused create
    /// `Core`; the dev-buy stays inside `Core`). `None` ⇒ `canonical_create`.
    #[serde(default)]
    pub create_layout: Option<Vec<pump_trader::DecoStep>>,
}

impl PumpfunTemplateParams {
    /// Author-time validation of a template's hand-picked pieces (mirrors the
    /// fail-closed checks `plan_pipeline::gate` re-runs before send), so a malformed
    /// template is rejected at save time — and, at launch, BEFORE any wallet is
    /// reserved or a pending row inserted — instead of only failing mid-plan with a
    /// silent 500. Validates (1) every authored bundler-leg buy variant is a buy
    /// encoding, (2) the dev-buy variant (a buy; tokens-out needs slippage), and
    /// (3) every authored ix layout. Returns the first defect.
    pub fn validate(&self) -> Result<(), String> {
        self.validate_leg_variants()?;
        self.validate_dev_buy()?;
        self.validate_layouts()?;
        Ok(())
    }

    /// The resolved dev-buy encoding — the authored `dev_buy_variant`, or the
    /// historical `buy_exact_sol_in` default. SSOT for the three read sites (macro
    /// wiring, persisted `create_args`, the bundler-less legacy create path).
    pub fn dev_buy_variant_name(&self) -> String {
        self.dev_buy_variant
            .clone()
            .unwrap_or_else(|| crate::plan_pipeline::LAUNCH_BUY_VARIANT.to_string())
    }

    /// The dev buy is a BUY fused into the create tx, so its encoding must be a
    /// catalog buy variant. A **tokens-out** encoding (`buy` / `buy_v2`,
    /// `Denom::ExactBaseOut`) buys an exact token amount capped by `max_sol_cost`, so
    /// it needs a `slippage_bps` for `min_out` to be a real reserve-derived token
    /// floor — without one it would request ~1 token and barely spend the budget. The
    /// SOL-in encodings spend the budget directly, so `None` slippage stays the
    /// historical unprotected launch behaviour. Only enforced when a dev buy exists.
    fn validate_dev_buy(&self) -> Result<(), String> {
        if self.dev_buy_quote.unwrap_or(0) <= 0 {
            return Ok(()); // no dev buy ⇒ the encoding is inert
        }
        let name = self.dev_buy_variant_name();
        let spec = pump_trader::spec(&name).ok_or_else(|| {
            format!(
                "dev_buy_variant {name:?} is not a known pump.fun variant \
                 (use buy, buy_v2, buy_exact_sol_in, or buy_exact_quote_in_v2)"
            )
        })?;
        if spec.kind != pump_trader::VariantKind::Buy {
            return Err(format!(
                "dev_buy_variant {name:?} is a {:?}, not a buy — the dev buy must use a \
                 buy encoding (buy, buy_v2, buy_exact_sol_in, buy_exact_quote_in_v2)",
                spec.kind
            ));
        }
        if spec.denom == pump_trader::Denom::ExactBaseOut && self.slippage_bps.is_none() {
            return Err(format!(
                "dev_buy_variant {name:?} is a tokens-out encoding — set slippage_bps so the \
                 dev buy spends its budget (a tokens-out buy with no slippage buys ~1 token)"
            ));
        }
        Ok(())
    }

    /// A bundler leg is a BUY, so its authored variant must be a catalog buy
    /// encoding — any of the four (`buy`, `buy_v2`, `buy_exact_sol_in`,
    /// `buy_exact_quote_in_v2`). The leg carries a SOL budget (`Amount::ExactQuote`),
    /// which every buy can consume: the SOL-in encodings spend it directly, the
    /// tokens-out encodings (`buy`/`buy_v2`) take it as `max_sol_cost` and derive the
    /// token amount from the curve reserves at build time (see `denom_accepts`). This
    /// only guards against a typo or a non-buy variant (a `sell`/`create`) slipping
    /// into `leg_structures` — the plan gate re-validates the full plan before send.
    fn validate_leg_variants(&self) -> Result<(), String> {
        for (i, recipe) in self.leg_structures.iter().flatten().enumerate() {
            let name = recipe.variant.as_str();
            match pump_trader::spec(name) {
                Some(s) if s.kind == pump_trader::VariantKind::Buy => {}
                Some(s) => {
                    return Err(format!(
                        "leg_structures[{i}] variant {name:?} is a {:?}, not a buy — a bundler \
                         leg must use a buy encoding (buy, buy_v2, buy_exact_sol_in, \
                         buy_exact_quote_in_v2)",
                        s.kind
                    ))
                }
                None => {
                    return Err(format!(
                        "leg_structures[{i}] variant {name:?} is not a known pump.fun variant \
                         (use buy, buy_v2, buy_exact_sol_in, or buy_exact_quote_in_v2)"
                    ))
                }
            }
        }
        Ok(())
    }

    /// Fail-closed validation of every hand-picked ix layout. A bundler co-buy is a
    /// snipe → its layout must carry `CuPrice` + `Tip`; the create layout is a
    /// non-snipe `Create`. Returns the first defect.
    pub fn validate_layouts(&self) -> Result<(), String> {
        use pump_trader::{IxLayout, LayoutKind};
        for (i, recipe) in self.leg_structures.iter().flatten().enumerate() {
            if let Some(steps) = &recipe.layout {
                IxLayout { steps: steps.clone() }
                    .validate(LayoutKind::Buy, true)
                    .map_err(|e| format!("leg_structures[{i}] ix layout invalid: {e}"))?;
            }
        }
        if let Some(steps) = &self.create_layout {
            IxLayout { steps: steps.clone() }
                .validate(LayoutKind::Create, false)
                .map_err(|e| format!("create_layout invalid: {e}"))?;
        }
        Ok(())
    }

    /// The authored create layout as an [`IxLayout`], if any — the launcher feeds
    /// this to `PumpFunTrader::set_create_layout` before building the create tx.
    pub fn create_ix_layout(&self) -> Option<pump_trader::IxLayout> {
        self.create_layout
            .clone()
            .map(|steps| pump_trader::IxLayout { steps })
    }
}

#[derive(Debug, Clone)]
pub struct LaunchRequest {
    pub template_id: Uuid,
    pub dev_wallet_id: Uuid,
    /// Per-launch metadata override — pick a different `metadata_templates` row
    /// for this one launch. Falls back to the template's own
    /// `metadata_template_id` when unset, so a template still works as a bare
    /// reusable preset.
    pub metadata_template_id: Option<Uuid>,
    /// Overrides the template's `bundle_leg_count` when set (the Launch
    /// Console's "use N bundlers" control) — bundler wallets are then a
    /// server-side atomic claim from the `funded` pool, never a client pick.
    pub bundler_count: Option<u32>,
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
    if dev_wallet.role != WalletRole::Dev.as_str() {
        bail!("wallet {} is role={}, expected dev", dev_wallet.id, dev_wallet.role);
    }

    let params: PumpfunTemplateParams =
        serde_json::from_value(template.params.clone()).context("parse template params")?;

    // Resolve token identity from the linked metadata template (single source of
    // truth). A per-launch `metadata_template_id` overrides the template's own for
    // this one launch; the template stays a reusable preset.
    let metadata_template_id = req
        .metadata_template_id
        .or(template.metadata_template_id)
        .context("launch template has no metadata_template_id and none was provided")?;
    let metadata = MetadataTemplateRepo::get(pool, metadata_template_id)
        .await?
        .context("metadata template not found")?;
    let (meta_name, meta_symbol, meta_uri) =
        (metadata.name, metadata.symbol, metadata.uri);

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
    // Fail-closed on any malformed template piece (bundler-leg variant denom + any
    // hand-picked layout) BEFORE a single ix is built, a wallet is reserved, or a
    // pending row is inserted — so a bad template fails fast + clean, never as a
    // silent mid-plan 500 that strands a `pending` launch with `reserved` wallets.
    params
        .validate()
        .map_err(|e| anyhow::anyhow!("template invalid: {e}"))?;
    trader.set_create_layout(params.create_ix_layout());

    // Fail fast before building anything on-chain: the dev wallet must cover the
    // create's rent + fees + the configured Jito tip on top of the dev-buy spend,
    // if any.
    let dev_buy_quote = params.dev_buy_quote.unwrap_or(0);
    let balance = trader.get_sol_balance().await.context("fetch dev wallet balance")?;
    // SSOT: the exact figure `funding_plan::FundPlan` funds the dev wallet to, so
    // the JIT funder's target and this gate can never drift (they were inlined
    // separately before). The create floor tracks the live tip ceiling.
    let tip_ceiling = settings.launch_tip_ceiling_lamports();
    let create_floor = min_dev_launch_lamports(&template.variant, tip_ceiling);
    let required_lamports =
        crate::funding_plan::dev_launch_required_lamports(&template.variant, &params, tip_ceiling);
    if balance < required_lamports {
        let per_sol = pump_trader::protocol::LAMPORTS_PER_SOL as f64;
        // Typed so the HTTP layer maps it to a 400 with this exact message,
        // instead of the opaque 500 that hid the shortfall before.
        return Err(InsufficientDevBalance(format!(
            "dev wallet {creator} has {:.4} SOL — needs at least {:.4} SOL \
             (create floor {:.4} + dev-buy {:.4}) before launching",
            balance as f64 / per_sol,
            required_lamports as f64 / per_sol,
            create_floor as f64 / per_sol,
            dev_buy_quote.max(0) as f64 / per_sol,
        ))
        .into());
    }

    let mint = Keypair::new();
    let mint_address = mint.pubkey().to_string();
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
            status: Some(LaunchStatus::Pending.as_str().into()),
        },
    )
    .await?;

    let launch_id = pending.id;
    let finish = async {
        // Reserve the dev wallet to this launch before it's spent from — closes
        // the race where two concurrent launches both pick the same `funded`
        // dev wallet from the dropdown and both build/submit a create+dev-buy
        // from it. Also what makes the `mark_used` transition below a real
        // reserved->used move instead of a permanent no-op, so dev wallets
        // reach `used` and are picked up by the dust sweep like bundler legs.
        ManagedWalletRepo::claim_specific(pool, dev_wallet.id, launch_id)
            .await?
            .context("dev wallet is no longer funded (claimed by another launch)")?;

        let dev_buy_sol = dev_buy_quote as f64 / pump_trader::protocol::LAMPORTS_PER_SOL as f64;
        // ATOMIC LAUNCH: when the launch has bundler legs, the create (+ dev-buy) is
        // NOT sent here — it becomes `tx0` of the atomic Jito bundle built + submitted
        // below (`execute_bundle_with_trader`), so create and every co-buy land in one
        // slot (no front-run gap, no separate-submission drop). The launch stays
        // `pending`; the confirm watcher flips it to `created` once the bundle lands.
        // A bundler-less launch keeps the legacy create-then-confirm path (nothing to
        // bundle atomically, so no drop risk).
        let wants_bundle = crate::bundle::resolve_leg_count(req.bundler_count, &params).is_some();

        // The dev-buy is NOT encoded in the variant — it's `dev_buy_quote > 0`.
        // `create_v2`/`create_v1` are the only variants.
        let ix_label = match template.variant.as_str() {
            "pumpfun.create_v2" => "Pump.Fun: Create_v2",
            "pumpfun.create_v1" => "Pump.Fun: Create",
            other => bail!("unsupported launch variant: {other}"),
        };

        // `signature` is empty until known: for the atomic path the create leg's sig
        // is stamped on the launch by `execute_bundle` at submit; for the legacy path
        // it's the standalone create tx below.
        let signature = if wants_bundle {
            String::new()
        } else {
            let sig = match template.variant.as_str() {
                "pumpfun.create_v2" => {
                    let args = CreateTokenV2Args {
                        name: meta_name.clone(),
                        symbol: meta_symbol.clone(),
                        uri: meta_uri.clone(),
                        creator,
                        is_mayhem_mode: params.is_mayhem_mode,
                        cashback_enabled: params.cashback_enabled,
                    };
                    if dev_buy_quote > 0 {
                        // The dev-buy encoding (validated above); same catalog→executor
                        // map the co-buy legs use so it also accepts `buy_exact_quote_in_v2`.
                        let dev_variant =
                            crate::plan_pipeline::bundle_buy_variant(&params.dev_buy_variant_name())?;
                        trader
                            .create_token_v2_and_dev_buy(
                                &mint, &args, dev_buy_sol, params.slippage_bps, dev_variant, true,
                            )
                            .await?
                    } else {
                        trader.create_token_v2(&mint, &args, true).await?
                    }
                }
                "pumpfun.create_v1" => {
                    let args = CreateTokenArgs {
                        name: meta_name.clone(),
                        symbol: meta_symbol.clone(),
                        uri: meta_uri.clone(),
                        creator,
                    };
                    if dev_buy_quote > 0 {
                        let dev_variant =
                            crate::plan_pipeline::bundle_buy_variant(&params.dev_buy_variant_name())?;
                        trader
                            .create_token_and_dev_buy(
                                &mint, &args, dev_buy_sol, params.slippage_bps, dev_variant, true,
                            )
                            .await?
                    } else {
                        trader.create_token(&mint, &args, true).await?
                    }
                }
                other => bail!("unsupported launch variant: {other}"),
            };

            LaunchRepo::set_created(pool, launch_id, &sig, LaunchStatus::Created.as_str())
                .await?;

            // Wallet-pool lifecycle (legacy path): the dev wallet is now consumed
            // (terminal — never re-claimable, eligible for the dust sweep). In the
            // atomic path the dev wallet stays `reserved` until the bundle lands
            // (confirm watcher marks it `used`) — a dropped atomic bundle spent
            // nothing, so it must not be prematurely retired.
            if let Err(e) = ManagedWalletRepo::mark_used(pool, &[dev_wallet.id]).await {
                tracing::warn!(%launch_id, dev_wallet_id = %dev_wallet.id, %e, "failed to mark dev wallet used");
            }

            // Seed the dev cost-basis position eagerly (idempotent) the moment a
            // dev-buy lands, so the dust sweep's open-position guard protects this
            // wallet's gas SOL immediately. (Atomic path: the confirm watcher seeds
            // it on landing instead — the dev-buy hasn't landed yet here.)
            if dev_buy_quote > 0 {
                if let Err(e) = TokenPositionRepo::seed(
                    pool,
                    &mint_address,
                    dev_wallet.id,
                    WalletRole::Dev.as_str(),
                    dev_buy_quote,
                )
                .await
                {
                    tracing::warn!(%launch_id, dev_wallet_id = %dev_wallet.id, %e, "failed to seed dev position");
                }
            }
            sig
        };

        let token_program = keystore::token_program_for_variant(&template.variant);
        TokenRepo::insert(
            pool,
            &NewToken {
                mint_address: mint_address.clone(),
                launchpad_id: template.launchpad_id,
                quote_asset_id: template.quote_asset_id,
                creator_wallet: creator.to_string(),
                is_own_launch: true,
                name: meta_name.clone(),
                symbol: meta_symbol.clone(),
                decimals: PUMP_TOKEN_DECIMALS,
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

        // Force `is_own_launch = true` even if the ingest feed already inserted this
        // mint's row (its `INSERT … DO NOTHING` above would be a no-op, leaving the
        // feed's `is_own_launch = false`). The create event lands on the feed almost
        // immediately after the tx confirms — often before we reach the insert.
        TokenRepo::mark_own_launch(pool, &mint_address).await?;

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
            mint_address: mint_address.clone(),
            create_signature: signature,
            bundle: None,
        })
    }
    .await;

    if let Err(e) = finish {
        // Hand the dev wallet (and any wallet this launch reserved) back to `funded`
        // — a launch that failed before/at build spent nothing, so leaving it
        // `reserved` would strand real SOL and block every retry at `claim_specific`.
        // Scoped by launch id, so a concurrent launch's reservation is never touched.
        if let Err(rel) = ManagedWalletRepo::release_by_launch(pool, launch_id).await {
            tracing::warn!(%launch_id, %rel, "failed to release reservations after chain error");
        }
        if let Err(mark_err) =
            LaunchRepo::set_failed(pool, launch_id, LaunchStatus::Failed.as_str()).await
        {
            tracing::warn!(%launch_id, %mark_err, "failed to mark launch failed after chain error");
        } else {
            tracing::warn!(%launch_id, error = %e, "launch failed after pending row inserted");
        }
        return Err(e);
    }

    let result = finish?;

    // Bundle planning + atomic submit. In the atomic launch the create leg lives
    // INSIDE the bundle, so — unlike the old flow where create had already landed —
    // a problem ANYWHERE here means the token was never created. The whole phase runs
    // inside one `bundle_phase` block whose single error handler is the SSOT cleanup:
    // delete the persisted mint key, release every wallet this launch reserved (dev +
    // bundler legs) back to `funded`, and mark the launch `failed`. Every `?` inside
    // therefore fails LOUD + CLEAN — never a silent 500 that strands a `pending`
    // launch with `reserved` wallets (the bug where a bad leg variant slipped a bare
    // `?` here left three launches pending + wallets stuck reserved).
    let bundle_phase = async {
        let mut result = result; // owned by this phase; mutated with the bundle result
        if let Some(leg_count) = crate::bundle::resolve_leg_count(req.bundler_count, &params) {
            let (quote_per_leg, tip_quote) = crate::bundle::resolve_bundle_quote(&params)?;

            // Server-side atomic claim from the `funded` bundler pool (wallet-pool
            // Phase 3) — never a client-side wallet pick. May return fewer than
            // `leg_count` if the pool is short; plan exactly that many legs rather
            // than erroring or reusing a wallet across legs.
            let claimed = ManagedWalletRepo::claim_funded(
                pool,
                WalletRole::Bundler.as_str(),
                leg_count as i64,
                launch_id,
            )
            .await?;
            if claimed.is_empty() {
                // No bundlers available: create never got sent (it's tx0 of the bundle
                // we can't build). Bail → the shared handler releases the dev wallet +
                // marks the launch failed, so the operator can fund bundlers (or
                // relaunch with 0 bundlers) and retry.
                bail!(
                    "no funded bundler wallets available for an atomic launch \
                     (requested {leg_count}) — fund the bundler pool or launch with 0 bundlers"
                );
            }
            if claimed.len() < leg_count as usize {
                tracing::warn!(
                    launch_id = %launch_id,
                    requested = leg_count,
                    claimed = claimed.len(),
                    "bundler pool short — planning a smaller bundle"
                );
            }
            // Phase 2.F: build the full launch as an orchestrator `Plan`
            // (create + dev-buy + N bundler co-buys), all catalog-validated, then
            // pass the mandatory fingerprint-audit gate before it's persisted or a
            // single leg is built. The create + dev-buy is now `tx0` of the atomic
            // bundle (built + submitted by `execute_bundle`, NOT sent above); its op
            // both documents the launch and drives the create leg build. Bundler buys
            // are `ExactQuote` (SOL-in, min_out-floored) — never the overflow-prone
            // tokens-out encoding.
            let bundle_slippage = params
                .slippage_bps
                .or(Some(crate::plan_pipeline::DEFAULT_BUNDLE_SLIPPAGE_BPS));
            // Hand-picked recipes cycle across the claimed legs (`recipe = leg_structures
            // [i % len]`) — the claimed count is dynamic (a short pool yields fewer
            // legs), so a handful of authored shapes distribute over however many
            // wallets are claimed. Empty `leg_structures` ⇒ no authored variant/layout
            // (each leg falls back to the default buy encoding + persona disguise).
            let recipes = params.leg_structures.as_deref().unwrap_or(&[]);
            let bundlers: Vec<orchestrator::BundlerLeg> = claimed
                .iter()
                .enumerate()
                .map(|(i, w)| {
                    let recipe = (!recipes.is_empty()).then(|| &recipes[i % recipes.len()]);
                    orchestrator::BundlerLeg {
                        wallet: orchestrator::WalletRef::managed(w.id, w.address.clone()),
                        sol: quote_per_leg.max(0) as u64,
                        slippage_bps: recipe
                            .and_then(|r| r.slippage_bps_min)
                            .or(bundle_slippage),
                        variant: recipe.map(|r| r.variant.as_str().to_string()),
                        layout: recipe.and_then(|r| r.layout.clone()),
                    }
                })
                .collect();
            let mut seq = orchestrator::IdSeq::new(0);
            let ops = orchestrator::bundle_launch(
                &mut seq,
                &orchestrator::BundleLaunch {
                    mint: mint_address.clone(),
                    dev: orchestrator::WalletRef::managed(dev_wallet.id, creator.to_string()),
                    create_variant: create_variant_name(&template.variant).to_string(),
                    buy_variant: crate::plan_pipeline::LAUNCH_BUY_VARIANT.to_string(),
                    dev_buy_variant: params.dev_buy_variant_name(),
                    dev_buy_sol: dev_buy_quote.max(0) as u64,
                    dev_slippage_bps: params.slippage_bps,
                    bundlers,
                },
            );
            let plan = orchestrator::Plan::for_mint(mint_address.clone(), ops);
            let gated = crate::plan_pipeline::gate(plan, settings.allow_fingerprint)
                .context("launch bundle failed the mandatory plan/audit gate")?;

            // Persist the ephemeral mint keypair (envelope-encrypted) BEFORE the
            // atomic submit — `execute_bundle` loads it to sign the create leg, and a
            // background re-bid after a drop reloads it to re-sign create. Deleted on
            // any terminal outcome (confirm watcher / failure cleanup).
            keystore::write_mint_key(&settings.keystore_dir, launch_id, &mint, &kek)
                .context("persist launch mint key")?;

            // The create leg's build inputs (SSOT: bundle_execute rebuilds tx0 from
            // this on every submit + re-bid, so it never needs the launch request).
            let create_args = crate::bundle_execute::CreateLegArgs {
                variant: template.variant.clone(),
                name: meta_name.clone(),
                symbol: meta_symbol.clone(),
                uri: meta_uri.clone(),
                is_mayhem_mode: params.is_mayhem_mode,
                cashback_enabled: if template.variant.contains("create_v1") {
                    false
                } else {
                    params.cashback_enabled
                },
                dev_buy_quote: dev_buy_quote.max(0) as u64,
                dev_buy_variant: params.dev_buy_variant_name(),
                slippage_bps: params.slippage_bps,
                creator: creator.to_string(),
            };
            let create_args_json =
                serde_json::to_value(&create_args).context("serialize create_args")?;

            let legs_json = crate::plan_pipeline::display_legs_json(&gated);
            let bundle = BundleRepo::insert(
                pool,
                launch_id,
                tip_quote,
                legs_json,
                gated.plan_json(),
                gated.audit_json(),
                Some(create_args_json),
            )
            .await?;
            LaunchRepo::set_bundle_id(pool, launch_id, bundle.id).await?;
            info!(
                launch_id = %launch_id, bundle_id = %bundle.id, legs = claimed.len(),
                audit_score = gated.audit.score, "bundle planned + audited"
            );
        }

        // Auto-submit the planned atomic bundle (create tx0 + co-buy legs) — no second
        // HTTP call. This IS the create for a bundled launch, so a submit failure is a
        // launch failure: the `?` drops into the shared cleanup below.
        if let Some(bundle_id) = LaunchRepo::get(pool, launch_id)
            .await?
            .and_then(|l| l.bundle_id)
        {
            if BundleRepo::get(pool, bundle_id)
                .await?
                .is_some_and(|b| b.status == BundleStatus::Planned.as_str())
            {
                // Reuse the dev trader built above — the auto-submit no longer stands up
                // (and initialize()s) a second trader in the same request.
                let bundle_result = execute_bundle_with_trader(pool, settings, &trader, bundle_id)
                    .await
                    .context("atomic launch bundle auto-submit failed")?;
                info!(
                    launch_id = %launch_id,
                    bundle_id = %bundle_id,
                    jito_bundle_id = %bundle_result.jito_bundle_id,
                    "atomic launch bundle auto-submitted"
                );
                result.bundle = Some(bundle_result);
                // The create leg's signature was stamped on the launch by
                // `execute_bundle`; surface it in the response (status is still
                // `pending` until the confirm watcher sees the bundle land).
                if let Some(sig) =
                    LaunchRepo::get(pool, launch_id).await?.and_then(|l| l.create_signature)
                {
                    result.create_signature = sig;
                }
            }
        }

        Ok::<_, anyhow::Error>(result)
    }
    .await;

    match bundle_phase {
        Ok(result) => Ok(result),
        Err(e) => {
            tracing::warn!(%launch_id, error = %format!("{e:#}"), "launch bundle phase failed — cleaning up");
            // Delete the persisted mint key if one was written (idempotent — a no-op
            // when the failure happened before `write_mint_key`).
            let mint_ref = keystore::mint_key_ref(launch_id);
            if let Err(del) = keystore::delete_mint_key(&settings.keystore_dir, &mint_ref) {
                tracing::warn!(%launch_id, %del, "failed to delete mint key during launch cleanup");
            }
            // Release every wallet this launch reserved (dev + bundler legs) back to
            // `funded` — a failed atomic launch spent nothing (a bundle that never
            // submitted/landed executes no txs), so they're fully reusable, not `used`,
            // and the next retry won't stall at `claim_specific`.
            if let Err(rel) = ManagedWalletRepo::release_by_launch(pool, launch_id).await {
                tracing::warn!(%launch_id, %rel, "failed to release reservations during launch cleanup");
            }
            if let Err(mark) =
                LaunchRepo::set_failed(pool, launch_id, LaunchStatus::Failed.as_str()).await
            {
                tracing::warn!(%launch_id, %mark, "failed to mark launch failed during launch cleanup");
            }
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params_with_leg_variants(variants: &[&str]) -> PumpfunTemplateParams {
        let legs: Vec<serde_json::Value> =
            variants.iter().map(|v| json!({ "variant": v })).collect();
        serde_json::from_value(json!({
            "dev_buy_quote": 20_000_000,
            "bundle_leg_count": variants.len(),
            "bundle_quote_per_leg": 10_000_000,
            "leg_structures": legs,
        }))
        .expect("params parse")
    }

    /// All four buy encodings are valid bundler-leg variants (each consumes the leg's
    /// SOL budget — tokens-out via `max_sol_cost`, SOL-in via `spendable`).
    #[test]
    fn validate_accepts_all_buy_variants() {
        // The exact original template (`["buy_exact_sol_in", "buy"]`) now validates.
        assert!(params_with_leg_variants(&["buy_exact_sol_in", "buy"]).validate().is_ok());
        // Every buy encoding is accepted, tokens-out included.
        for v in ["buy", "buy_v2", "buy_exact_sol_in", "buy_exact_quote_in_v2"] {
            assert!(params_with_leg_variants(&[v]).validate().is_ok(), "{v} should validate");
        }
        // The pre-alignment string still parses (serde alias) and is accepted.
        assert!(params_with_leg_variants(&["buy_exact_quote_in"]).validate().is_ok());
        // No leg_structures at all ⇒ nothing to validate.
        let empty: PumpfunTemplateParams = serde_json::from_value(json!({})).unwrap();
        assert!(empty.validate().is_ok());
    }

    /// A non-buy variant can't even be authored into `leg_structures` — the
    /// `BuyVariant` enum rejects it at JSON parse (the first line of defense; the
    /// catalog-SSOT check in `validate_leg_variants` is the second, against drift).
    #[test]
    fn non_buy_leg_variant_fails_to_parse() {
        let bad = json!({ "leg_structures": [{ "variant": "sell" }] });
        assert!(
            serde_json::from_value::<PumpfunTemplateParams>(bad).is_err(),
            "a `sell` leg variant must not deserialize"
        );
    }

    fn params_with_dev_buy(variant: Option<&str>, slippage: Option<u64>, quote: i64) -> PumpfunTemplateParams {
        let mut obj = json!({ "dev_buy_quote": quote });
        if let Some(v) = variant {
            obj["dev_buy_variant"] = json!(v);
        }
        if let Some(s) = slippage {
            obj["slippage_bps"] = json!(s);
        }
        serde_json::from_value(obj).expect("params parse")
    }

    /// The dev-buy variant defaults to `buy_exact_sol_in` and every buy encoding is
    /// accepted; a tokens-out encoding additionally REQUIRES a slippage so its
    /// `min_out` is a real token floor. The check is inert without a dev buy.
    #[test]
    fn validate_dev_buy_variant_and_tokens_out_slippage() {
        // Default (None ⇒ buy_exact_sol_in), SOL-in, no slippage → ok.
        assert!(params_with_dev_buy(None, None, 20_000_000).validate().is_ok());
        // Every SOL-in encoding validates with no slippage.
        for v in ["buy_exact_sol_in", "buy_exact_quote_in_v2"] {
            assert!(params_with_dev_buy(Some(v), None, 20_000_000).validate().is_ok(), "{v} SOL-in");
        }
        // A tokens-out dev variant with NO slippage is rejected...
        for v in ["buy", "buy_v2"] {
            assert!(params_with_dev_buy(Some(v), None, 20_000_000).validate().is_err(), "{v} needs slippage");
            // ...and accepted once a slippage is set.
            assert!(params_with_dev_buy(Some(v), Some(500), 20_000_000).validate().is_ok(), "{v} + slippage");
        }
        // An unknown / non-buy dev variant is rejected.
        assert!(params_with_dev_buy(Some("sell"), Some(500), 20_000_000).validate().is_err());
        assert!(params_with_dev_buy(Some("nonsense"), Some(500), 20_000_000).validate().is_err());
        // No dev buy (quote 0) ⇒ the encoding is inert, even a tokens-out one w/o slippage.
        assert!(params_with_dev_buy(Some("buy"), None, 0).validate().is_ok());
    }
}
