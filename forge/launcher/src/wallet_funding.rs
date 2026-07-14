//! Wallet funding orchestration (docs/wallet-funding-plan.md): the inverse of
//! `dust_sweep.rs`. Sends SOL treasury -> pool so `generated` wallets become
//! `funded` and claimable for launches. The pool could already generate wallets,
//! detect incoming SOL, and reclaim dust — this closes the loop by *sending* SOL
//! in.
//!
//! Deliberately a plain `solana-client` transfer (NOT pump-trader's Jito/
//! multi-sender/retry machinery) — funding has no landing urgency, unlike a snipe
//! buy. Same choice as [`crate::dust_sweep`].
//!
//! SAFETY: this spends real SOL autonomously. Every send is gated by
//! [`FundingConfig`]'s reserve floor + per-interval cap, and the whole subsystem
//! is off unless `FUND_ENABLED=true` (see `config.rs`). `FUND_DRY_RUN=true` plans
//! + logs transfers but sends nothing.
//!
//! Obfuscation = Tier 1: one transfer per tx, jittered amount + timing, direct
//! treasury->wallet. Tier 2 (multi-hop fan-out) is left as an unimplemented
//! [`FundingStrategy`] impl behind the same seam.

use std::str::FromStr;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use platform_core::models::{ManagedWallet, WalletRole, WalletStatus};
use platform_core::storage::repositories::{LaunchTemplateRepo, ManagedWalletRepo};
use rand::Rng;
use serde::Serialize;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Signature, Signer};
use sqlx::PgPool;
use tokio::task::JoinHandle;
use tracing::{info, warn};
use uuid::Uuid;

use crate::config::{FundingConfig, LauncherSettings};
use crate::events::EventSink;
use crate::funding_plan::FundPlan;
use crate::keystore::{self, EnvKek};
use crate::service::PumpfunTemplateParams;
use crate::wallet_pool::MIN_FUNDED_LAMPORTS;

/// How often the background funder tops the pool up. Cheap when idle — a single
/// `funded_count` per fundable role, no work when every role is already warm.
const FUND_INTERVAL: Duration = Duration::from_secs(60);

/// Serializes every funding pass in this process. The reserve floor + per-interval
/// spend cap are enforced against a pass-local treasury snapshot (`TreasuryPool`
/// captured at pass start + a running `spent`), so two passes running at once —
/// the background funder, a manual `POST /api/wallet_pool/fund`, and a JIT
/// `fund_for_launch` can all fire independently — each sees the other's spend as
/// zero and could together spend N× the cap or breach the reserve. Holding this
/// across a whole pass makes the snapshot authoritative: at most one pass reads
/// balances, spends, and writes them back at a time. Correctness over latency —
/// real SOL is moving, and a warm-pool pass is a fast no-op.
pub(crate) static FUNDING_LOCK: LazyLock<tokio::sync::Mutex<()>> =
    LazyLock::new(|| tokio::sync::Mutex::new(()));

/// One planned treasury->wallet transfer. Amount is already jittered.
#[derive(Debug, Clone)]
pub struct Transfer {
    pub managed_wallet_id: Uuid,
    pub target: Pubkey,
    pub lamports: u64,
}

/// Per-strategy amount inputs (base amount + jitter fraction).
#[derive(Debug, Clone, Copy)]
pub struct StrategyParams {
    pub amount_lamports: u64,
    pub jitter_pct: f64,
}

/// How treasury SOL reaches the pool wallets. The seam that keeps Tier-2
/// multi-hop out of the hot orchestration path (`fund_once` calls this once per
/// role, then just executes the returned transfers).
pub trait FundingStrategy {
    /// Plan the transfers for `targets`. Pure (no I/O) so it's unit-testable.
    fn plan_transfers(
        &self,
        treasury: Pubkey,
        targets: &[(Uuid, Pubkey)],
        params: &StrategyParams,
    ) -> Vec<Transfer>;
}

/// Tier 1: one direct transfer per target, amount jittered within
/// `[amount*(1-j), amount*(1+j)]`. Timing jitter (inter-send sleep) is applied by
/// `fund_once`, not here, so this stays pure.
pub struct DirectJittered;

impl FundingStrategy for DirectJittered {
    fn plan_transfers(
        &self,
        _treasury: Pubkey,
        targets: &[(Uuid, Pubkey)],
        params: &StrategyParams,
    ) -> Vec<Transfer> {
        let jitter = params.jitter_pct.clamp(0.0, 0.95);
        let mut rng = rand::thread_rng();
        targets
            .iter()
            .map(|(id, pk)| {
                let factor = 1.0 + rng.gen_range(-jitter..=jitter);
                let lamports = (params.amount_lamports as f64 * factor).round() as u64;
                Transfer { managed_wallet_id: *id, target: *pk, lamports }
            })
            .collect()
    }
}

/// One treasury wallet as a funding source: its live on-chain balance at pass
/// start, the running spend drawn from it this pass, and its signer. A funding
/// transfer is one system transfer signed by one key, so it draws wholly from a
/// single source (it can't be split across treasuries).
struct TreasurySource {
    wallet: ManagedWallet,
    pubkey: Pubkey,
    signer: Arc<dyn Signer + Send + Sync>,
    balance: u64,
    spent: u64,
}

impl TreasurySource {
    /// Lamports still drawable from this treasury while respecting its per-source
    /// reserve floor.
    fn spendable(&self, reserve: u64) -> u64 {
        self.balance.saturating_sub(self.spent).saturating_sub(reserve)
    }
}

/// The treasury source pool: EVERY non-retired `role=treasury` wallet, each a
/// spill source. Replaces the old single-oldest-treasury assumption
/// (`by_role(...).next()`) that tripped the `under_reserve` rail at "0 SOL spent"
/// whenever the oldest treasury happened to be the empty one while the operator's
/// SOL sat in the others.
struct TreasuryPool {
    sources: Vec<TreasurySource>,
}

impl TreasuryPool {
    /// Pick the source with the most spendable that can individually cover
    /// `amount`. `None` when no single treasury can cover this transfer while
    /// staying above its reserve floor (aggregate exhausted for a transfer of
    /// this size) — the caller treats that as a reserve breach and stops the pass.
    fn pick_source(&self, amount: u64, reserve: u64) -> Option<usize> {
        self.sources
            .iter()
            .enumerate()
            .filter(|(_, s)| s.spendable(reserve) >= amount)
            .max_by_key(|(_, s)| s.spendable(reserve))
            .map(|(i, _)| i)
    }
}

/// Build the treasury source pool: load every non-retired treasury, fetch each
/// live balance, and resolve each signer. A treasury with a bad address is
/// skipped (logged) rather than failing the whole pass. Returns an empty pool if
/// no treasury is configured — the caller no-ops, same posture as the dust sweep.
async fn build_treasury_pool(
    pool: &PgPool,
    settings: &LauncherSettings,
    rpc: &RpcClient,
    kek: &EnvKek,
) -> Result<TreasuryPool> {
    let treasuries = ManagedWalletRepo::by_role(pool, WalletRole::Treasury.as_str()).await?;
    let mut sources = Vec::with_capacity(treasuries.len());
    for w in treasuries {
        let pubkey = match Pubkey::from_str(&w.address) {
            Ok(pk) => pk,
            Err(e) => {
                warn!(managed_wallet_id = %w.id, %e, "wallet funding: bad treasury address — skipping source");
                continue;
            }
        };
        let balance = rpc
            .get_balance(&pubkey)
            .await
            .with_context(|| format!("fetch treasury {} balance", w.address))?;
        let signer = keystore::resolve_signer(&settings.keystore_dir, &w.key_ref, kek)
            .with_context(|| format!("resolve treasury {} signer", w.address))?;
        sources.push(TreasurySource { wallet: w, pubkey, signer, balance, spent: 0 });
    }
    Ok(TreasuryPool { sources })
}

/// Reflect each source treasury's spend on its cached balance immediately, so the
/// Wallet Pool page shows the drop right after the pass instead of a stale number
/// until the next poll. Optimistic (ignores tx fees); the balance poller
/// reconciles the exact on-chain value within one cycle. `record_balance`'s
/// promotion CASE only fires for `generated`/`funding`, so it never mutates a
/// treasury's status.
async fn writeback_treasury_balances(pool: &PgPool, sources: &TreasuryPool) {
    for s in &sources.sources {
        if s.spent == 0 {
            continue;
        }
        let projected = s.balance.saturating_sub(s.spent) as i64;
        if let Err(e) =
            ManagedWalletRepo::record_balance(pool, s.wallet.id, projected, MIN_FUNDED_LAMPORTS)
                .await
        {
            warn!(?e, treasury = %s.wallet.address, "wallet funding: failed to refresh treasury cached balance");
        }
    }
}

/// Scope for a single funding pass. Defaults (both `None`) = the background
/// behavior: top every fundable role up to its warm target. The manual endpoint
/// narrows it (a specific role and/or an explicit count).
#[derive(Debug, Clone, Default)]
pub struct FundScope {
    pub role: Option<WalletRole>,
    pub count: Option<i64>,
}

/// Delivery mode for a funding pass — how each transfer is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FundMode {
    /// Autonomous background funder: confirm each send and apply the timing
    /// jitter between them (de-correlates the transfers — Tier-1 obfuscation).
    Background,
    /// Operator-triggered manual pass (`POST /api/wallet_pool/fund`): submit each
    /// transfer without blocking on confirmation and skip the timing jitter, so
    /// the endpoint returns promptly instead of serially awaiting N confirmations
    /// (plus up to N jitter sleeps). The balance poller still promotes
    /// `funding -> funded` when the SOL lands; a rare accepted-but-dropped tx
    /// leaves the wallet in `funding` (its SOL never left the treasury), which is
    /// acceptable on this supervised path.
    Manual,
}

/// What happened to one wallet in a funding pass — the manual endpoint's
/// per-wallet response, and the log detail for the background pass.
#[derive(Debug, Serialize)]
pub struct WalletFundOutcome {
    pub managed_wallet_id: Uuid,
    pub address: String,
    pub role: String,
    pub amount_lamports: u64,
    /// `sent` | `dry_run` | `failed` | `skipped_cap` | `skipped_bad_address` |
    /// `skipped_funded` (JIT: already at/above its target, no top-up needed).
    pub result: &'static str,
    pub signature: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Default, Serialize)]
pub struct FundReport {
    pub spent_lamports: u64,
    pub outcomes: Vec<WalletFundOutcome>,
}

/// The only roles this funder tops up: the launch buyers (dev + bundler legs).
/// Returns `(per-wallet amount, warm target)` or `None` for a non-fundable role
/// (treasury is the *source*; trading isn't part of the launch flow).
fn role_plan(role: WalletRole, cfg: &FundingConfig) -> Option<(u64, i64)> {
    match role {
        WalletRole::Dev => Some((cfg.amount_dev_lamports, cfg.target_funded_dev)),
        WalletRole::Bundler => Some((cfg.amount_bundler_lamports, cfg.target_funded_bundler)),
        WalletRole::Treasury | WalletRole::Trading => None,
    }
}

/// Run one funding pass. Resolves the treasury, tops up each in-scope fundable
/// role, and returns a per-wallet report. Best-effort: a single wallet's failure
/// reverts just that wallet (`funding` -> `generated`) and the pass continues; a
/// safety-rail breach (reserve floor / per-interval cap) reverts every
/// still-unsent claim and stops.
pub async fn fund_once(
    pool: &PgPool,
    settings: &LauncherSettings,
    scope: FundScope,
    mode: FundMode,
    sink: Option<&dyn EventSink>,
) -> Result<FundReport> {
    let Some(cfg) = settings.funding.as_ref() else {
        bail!("wallet funding not configured (FUND_ENABLED not set)");
    };

    // Serialize against every other funding pass — see `FUNDING_LOCK`. Held for the
    // whole pass so the treasury snapshot + spend accounting stay authoritative.
    let _funding_guard = FUNDING_LOCK.lock().await;

    let mut report = FundReport::default();

    // Cheap indexed shortfall gate BEFORE paying for the treasury RPC snapshot
    // (audit §5): a periodic top-up pass over a fully warm pool otherwise builds
    // the treasury pool — N `get_balance` RPCs — every 60s only to fund nothing.
    // An explicit `count` (a manual fund request) always proceeds; the automatic
    // pass (count = None) bails when `funded_count` already meets every target.
    if scope.count.is_none() {
        let roles: Vec<WalletRole> = match scope.role {
            Some(r) => vec![r],
            None => vec![WalletRole::Dev, WalletRole::Bundler],
        };
        let mut shortfall = 0i64;
        for role in &roles {
            if let Some((_amount, target)) = role_plan(*role, cfg) {
                let funded = ManagedWalletRepo::funded_count(pool, role.as_str()).await?;
                shortfall += (target - funded).max(0);
            }
        }
        if shortfall <= 0 {
            return Ok(report); // fully warm — nothing to fund, no RPC paid
        }
    }

    // Resolve the treasury source pool (ALL non-retired treasuries). No-op if
    // none — same posture as the dust sweep's missing-treasury case.
    let rpc =
        RpcClient::new_with_commitment(settings.rpc_url.clone(), CommitmentConfig::confirmed());
    let kek = EnvKek::from_passphrase(&settings.kek_passphrase);
    let mut sources = build_treasury_pool(pool, settings, &rpc, &kek).await?;
    if sources.sources.is_empty() {
        warn!("wallet funding: no treasury wallet configured (role=treasury) — skipping");
        return Ok(report);
    }
    let funding_source = "auto-fund (treasury pool)".to_string();
    let strategy = DirectJittered;
    let plan_from = sources.sources[0].pubkey; // representative; DirectJittered ignores it

    let roles: Vec<WalletRole> = match scope.role {
        Some(r) => vec![r],
        None => vec![WalletRole::Dev, WalletRole::Bundler],
    };

    'roles: for role in roles {
        let Some((amount, target)) = role_plan(role, cfg) else {
            warn!(role = role.as_str(), "wallet funding: role is not fundable — skipping");
            continue;
        };

        // How many to claim: an explicit count, else the top-up shortfall.
        let n = match scope.count {
            Some(c) => c,
            None => target - ManagedWalletRepo::funded_count(pool, role.as_str()).await?,
        };
        if n <= 0 {
            continue;
        }

        let claimed =
            ManagedWalletRepo::claim_for_funding(pool, role.as_str(), n, &funding_source).await?;
        if claimed.is_empty() {
            info!(role = role.as_str(), want = n, "wallet funding: no `generated` wallets to fund");
            continue;
        }

        // Parse addresses; a bad address can't be funded — revert its claim.
        let mut targets: Vec<(Uuid, Pubkey)> = Vec::with_capacity(claimed.len());
        for w in &claimed {
            match Pubkey::from_str(&w.address) {
                Ok(pk) => targets.push((w.id, pk)),
                Err(e) => {
                    let _ = ManagedWalletRepo::revert_funding(pool, &[w.id]).await;
                    warn!(managed_wallet_id = %w.id, %e, "wallet funding: bad address — reverted");
                    report.outcomes.push(bad_address_outcome(w));
                }
            }
        }

        let transfers = strategy.plan_transfers(
            plan_from,
            &targets,
            &StrategyParams { amount_lamports: amount, jitter_pct: cfg.amount_jitter_pct },
        );

        for (i, t) in transfers.iter().enumerate() {
            // Safety rails — breach reverts every still-unsent claim and stops.
            // `over_cap`: aggregate per-interval spend across all sources.
            // `pick_source == None`: no single treasury can cover this transfer
            // above its reserve floor (the pool is exhausted for a transfer this
            // size) — the multi-treasury analogue of the old `under_reserve`.
            let over_cap = report.spent_lamports + t.lamports > cfg.max_spend_per_interval_lamports;
            let source_idx = sources.pick_source(t.lamports, cfg.treasury_reserve_lamports);
            if over_cap || source_idx.is_none() {
                let unsent: Vec<Uuid> = transfers[i..].iter().map(|t| t.managed_wallet_id).collect();
                let _ = ManagedWalletRepo::revert_funding(pool, &unsent).await;
                warn!(
                    role = role.as_str(),
                    over_cap,
                    under_reserve = source_idx.is_none(),
                    spent = report.spent_lamports,
                    reverted = unsent.len(),
                    "wallet funding: safety rail hit — reverted remaining claims, stopping pass"
                );
                for t in &transfers[i..] {
                    report.outcomes.push(WalletFundOutcome {
                        managed_wallet_id: t.managed_wallet_id,
                        address: t.target.to_string(),
                        role: role.as_str().to_string(),
                        amount_lamports: t.lamports,
                        result: "skipped_cap",
                        signature: None,
                        error: None,
                    });
                }
                break 'roles;
            }
            let source_idx = source_idx.expect("pick_source is Some (checked above)");

            if cfg.dry_run {
                // Plan + log, send nothing, release the claim so it isn't stranded.
                let _ = ManagedWalletRepo::revert_funding(pool, &[t.managed_wallet_id]).await;
                info!(
                    managed_wallet_id = %t.managed_wallet_id,
                    address = %t.target,
                    lamports = t.lamports,
                    "wallet funding (DRY RUN): would transfer"
                );
                report.outcomes.push(WalletFundOutcome {
                    managed_wallet_id: t.managed_wallet_id,
                    address: t.target.to_string(),
                    role: role.as_str().to_string(),
                    amount_lamports: t.lamports,
                    result: "dry_run",
                    signature: None,
                    error: None,
                });
                continue;
            }

            // Draw this transfer from the chosen source treasury. Clone the Arc
            // signer + copy the pubkey out first so the source isn't borrowed
            // across the `.await` (lets the Ok arm below take `&mut` to book the
            // spend).
            let (src_signer, src_pubkey) = {
                let src = &sources.sources[source_idx];
                (src.signer.clone(), src.pubkey)
            };
            // Background pass confirms each send (so a failed send reverts the
            // claim); the manual pass fires and returns (the poller confirms via
            // balance arrival) — see `FundMode`.
            let send_result = match mode {
                FundMode::Background => {
                    send_transfer(&rpc, &src_signer, src_pubkey, t.target, t.lamports).await
                }
                FundMode::Manual => {
                    submit_transfer(&rpc, &src_signer, src_pubkey, t.target, t.lamports).await
                }
            };
            match send_result {
                Ok(sig) => {
                    sources.sources[source_idx].spent += t.lamports;
                    report.spent_lamports += t.lamports;
                    info!(
                        managed_wallet_id = %t.managed_wallet_id,
                        address = %t.target,
                        lamports = t.lamports,
                        sig = %sig,
                        "wallet funding: transfer sent (poller will promote funding -> funded)"
                    );
                    report.outcomes.push(WalletFundOutcome {
                        managed_wallet_id: t.managed_wallet_id,
                        address: t.target.to_string(),
                        role: role.as_str().to_string(),
                        amount_lamports: t.lamports,
                        result: "sent",
                        signature: Some(sig.to_string()),
                        error: None,
                    });
                }
                Err(e) => {
                    let _ = ManagedWalletRepo::revert_funding(pool, &[t.managed_wallet_id]).await;
                    warn!(managed_wallet_id = %t.managed_wallet_id, %e, "wallet funding: transfer failed — reverted");
                    report.outcomes.push(WalletFundOutcome {
                        managed_wallet_id: t.managed_wallet_id,
                        address: t.target.to_string(),
                        role: role.as_str().to_string(),
                        amount_lamports: t.lamports,
                        result: "failed",
                        signature: None,
                        error: Some(format!("{e}")),
                    });
                }
            }

            // Timing jitter — de-correlate the sends in time (background funder
            // only; the manual pass skips it for a snappy response). Compute the
            // delay in a tight scope so the (non-Send) RNG never crosses the await.
            if mode == FundMode::Background && cfg.max_delay_ms > 0 {
                let delay = rand::thread_rng().gen_range(0..=cfg.max_delay_ms);
                tokio::time::sleep(Duration::from_millis(delay)).await;
            }
        }
    }

    // Reflect each source treasury's spend on its cached balance immediately (see
    // `writeback_treasury_balances`).
    if !cfg.dry_run {
        writeback_treasury_balances(pool, &sources).await;
    }

    // Push a coarse "pool changed" signal if this pass actually touched wallets, so
    // the Wallet Pool page refetches instead of polling (audit Phase A2).
    notify_pool_changed(sink, &report);
    Ok(report)
}

/// Emit the coarse wallet-pool push (no-op without a sink) when a funding pass
/// moved SOL or claimed/reverted wallets — i.e. produced any per-wallet outcome.
/// A fully warm no-op pass (empty report) emits nothing.
fn notify_pool_changed(sink: Option<&dyn EventSink>, report: &FundReport) {
    if let Some(sink) = sink {
        if !report.outcomes.is_empty() {
            sink.wallet_pool_changed();
        }
    }
}

/// Whether a single-wallet funding step wants the pass to keep going or stop
/// (a safety-rail breach stops the whole pass — a partially funded launch must
/// not proceed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FundStep {
    Continue,
    Stop,
}

/// Shared context for a JIT per-launch funding pass — the boot config + the one
/// RPC client + the delivery mode, threaded to each per-wallet step.
struct LaunchFundCtx<'a> {
    pool: &'a PgPool,
    rpc: &'a RpcClient,
    cfg: &'a FundingConfig,
    mode: FundMode,
}

/// Just-in-time, template-driven funding for ONE launch (docs/jit-funding-plan.md).
/// Derives the exact per-wallet need from the launch template ([`FundPlan`]) and
/// tops up **only the shortfall** so the dev wallet + `leg_count` bundler wallets
/// are `funded` to their template amounts before `execute_launch` claims them.
///
/// Unlike the flat warm-pool [`fund_once`], amounts are per-launch (dev = create
/// floor + dev-buy; leg = leg buy + tip + fees), not `FUND_AMOUNT_*` constants,
/// and there's no ±amount jitter (JIT funds the exact need). Draws from the whole
/// treasury source pool, same as `fund_once`. Runs in [`FundMode::Background`]
/// (confirm each send) so a wallet is promoted to `funded` only after its SOL has
/// actually landed — a launch claiming a not-yet-landed wallet would fail its
/// balance gate.
pub async fn fund_for_launch(
    pool: &PgPool,
    settings: &LauncherSettings,
    template_id: Uuid,
    bundler_count: Option<u32>,
    dev_wallet_id: Uuid,
    mode: FundMode,
    sink: Option<&dyn EventSink>,
) -> Result<FundReport> {
    let Some(cfg) = settings.funding.as_ref() else {
        bail!("wallet funding not configured (FUND_ENABLED not set)");
    };

    // Serialize against every other funding pass — see `FUNDING_LOCK`. A JIT launch
    // fund and the background funder must not race on the treasury snapshot/cap.
    let _funding_guard = FUNDING_LOCK.lock().await;

    let mut report = FundReport::default();

    // Template → per-launch funding requirement (single source of truth with the
    // pre-launch gate — see `funding_plan`).
    let template = LaunchTemplateRepo::get(pool, template_id)
        .await?
        .context("launch template not found")?;
    let params: PumpfunTemplateParams =
        serde_json::from_value(template.params.clone()).context("parse template params")?;
    let plan = FundPlan::from_params(
        &template.variant,
        &params,
        bundler_count,
        settings.launch_tip_ceiling_lamports(),
    )?;

    let rpc =
        RpcClient::new_with_commitment(settings.rpc_url.clone(), CommitmentConfig::confirmed());
    let kek = EnvKek::from_passphrase(&settings.kek_passphrase);
    let mut sources = build_treasury_pool(pool, settings, &rpc, &kek).await?;
    if sources.sources.is_empty() {
        warn!("wallet funding: no treasury wallet configured (role=treasury) — skipping");
        return Ok(report);
    }

    let ctx = LaunchFundCtx { pool, rpc: &rpc, cfg, mode };

    // 1) Dev wallet — the SPECIFIC wallet this launch will use, funded to the gate.
    let dev = ManagedWalletRepo::get(pool, dev_wallet_id)
        .await?
        .context("dev wallet not found")?;
    if dev.role != WalletRole::Dev.as_str() {
        bail!("wallet {} is role={}, expected dev", dev.id, dev.role);
    }
    if dev.status != WalletStatus::Generated.as_str()
        && dev.status != WalletStatus::Funded.as_str()
    {
        bail!(
            "dev wallet {} is status={}, expected `generated` or `funded`",
            dev.id,
            dev.status
        );
    }
    // A `generated` dev wallet must be claimed to `funding` first (so a concurrent
    // funder can't double-fund it); a `funded` one is topped up in place.
    let dev = if dev.status == WalletStatus::Generated.as_str() {
        match ManagedWalletRepo::claim_specific_for_funding(pool, dev.id, "jit-fund (launch)")
            .await?
        {
            Some(claimed) => claimed,
            None => bail!("dev wallet {} is no longer `generated` (claimed elsewhere)", dev.id),
        }
    } else {
        dev
    };
    let step = fund_wallet_to_target(&ctx, &mut sources, &mut report, dev, plan.dev_lamports).await?;

    // 2) Bundler legs — ensure `leg_count` bundler wallets are `funded` to
    //    `per_leg`. `execute_launch` claims its legs at RANDOM from the funded
    //    bundler pool, so EVERY funded bundler must meet `per_leg` (not just N of
    //    them) or a randomly-drawn leg could be under-funded. Top all funded
    //    bundlers up first (counting them toward the target), then claim
    //    `generated` ones to reach `leg_count`.
    if step == FundStep::Continue && plan.leg_count > 0 {
        let mut have: u32 = 0;
        let funded = ManagedWalletRepo::find_by_status(
            pool,
            WalletStatus::Funded.as_str(),
            Some(WalletRole::Bundler.as_str()),
        )
        .await?;
        let mut stopped = false;
        for w in funded {
            match fund_wallet_to_target(&ctx, &mut sources, &mut report, w, plan.per_leg_lamports)
                .await?
            {
                FundStep::Continue => have += 1,
                FundStep::Stop => {
                    stopped = true;
                    break;
                }
            }
        }

        let short = plan.leg_count.saturating_sub(have);
        if !stopped && short > 0 {
            let claimed = ManagedWalletRepo::claim_for_funding(
                pool,
                WalletRole::Bundler.as_str(),
                short as i64,
                "jit-fund (launch)",
            )
            .await?;
            if (claimed.len() as u32) < short {
                warn!(
                    launch_template = %template.id,
                    want = short,
                    got = claimed.len(),
                    "wallet funding: bundler `generated` pool short for launch — generate more bundlers"
                );
            }
            for w in claimed {
                if fund_wallet_to_target(&ctx, &mut sources, &mut report, w, plan.per_leg_lamports)
                    .await?
                    == FundStep::Stop
                {
                    break;
                }
            }
        }
    }

    if !cfg.dry_run {
        writeback_treasury_balances(pool, &sources).await;
    }

    notify_pool_changed(sink, &report);
    Ok(report)
}

/// Top ONE wallet up to `target` lamports, drawing the shortfall from the
/// treasury pool. Reads the wallet's live balance, sends only `target - balance`
/// (nothing if already ≥ target), and — in [`FundMode::Background`] — promotes it
/// to `funded` once the send confirms. A `funding` claim is reverted on any
/// failure/dry-run/rail-hit; a `funded` wallet is left funded (`revert_funding`
/// only touches `funding`, so it's a safe no-op there). Returns [`FundStep::Stop`]
/// when a safety rail breaks, so the caller aborts the rest of the launch's
/// funding.
async fn fund_wallet_to_target(
    ctx: &LaunchFundCtx<'_>,
    sources: &mut TreasuryPool,
    report: &mut FundReport,
    wallet: ManagedWallet,
    target: u64,
) -> Result<FundStep> {
    let is_funding = wallet.status == WalletStatus::Funding.as_str();

    let target_pk = match Pubkey::from_str(&wallet.address) {
        Ok(pk) => pk,
        Err(e) => {
            if is_funding {
                let _ = ManagedWalletRepo::revert_funding(ctx.pool, &[wallet.id]).await;
            }
            warn!(managed_wallet_id = %wallet.id, %e, "wallet funding: bad address — skipped");
            report.outcomes.push(bad_address_outcome(&wallet));
            return Ok(FundStep::Continue);
        }
    };

    let balance = ctx
        .rpc
        .get_balance(&target_pk)
        .await
        .with_context(|| format!("fetch target {} balance", wallet.address))?;
    let shortfall = target.saturating_sub(balance);

    // Already at/above target — no send. Promote a `generated`/`funding` wallet
    // to `funded` off its real observed balance so a launch can claim it.
    if shortfall == 0 {
        if let Err(e) =
            ManagedWalletRepo::record_balance(ctx.pool, wallet.id, balance as i64, MIN_FUNDED_LAMPORTS)
                .await
        {
            warn!(?e, managed_wallet_id = %wallet.id, "wallet funding: failed to record target balance");
        }
        report.outcomes.push(WalletFundOutcome {
            managed_wallet_id: wallet.id,
            address: wallet.address.clone(),
            role: wallet.role.clone(),
            amount_lamports: 0,
            result: "skipped_funded",
            signature: None,
            error: None,
        });
        return Ok(FundStep::Continue);
    }

    // Safety rails — same basis as `fund_once`: aggregate per-interval cap +
    // single-source reserve coverage.
    let over_cap = report.spent_lamports + shortfall > ctx.cfg.max_spend_per_interval_lamports;
    let source_idx = sources.pick_source(shortfall, ctx.cfg.treasury_reserve_lamports);
    if over_cap || source_idx.is_none() {
        if is_funding {
            let _ = ManagedWalletRepo::revert_funding(ctx.pool, &[wallet.id]).await;
        }
        warn!(
            managed_wallet_id = %wallet.id,
            over_cap,
            under_reserve = source_idx.is_none(),
            spent = report.spent_lamports,
            "wallet funding: safety rail hit funding a launch — stopping"
        );
        report.outcomes.push(WalletFundOutcome {
            managed_wallet_id: wallet.id,
            address: wallet.address.clone(),
            role: wallet.role.clone(),
            amount_lamports: shortfall,
            result: "skipped_cap",
            signature: None,
            error: None,
        });
        return Ok(FundStep::Stop);
    }
    let source_idx = source_idx.expect("pick_source is Some (checked above)");

    if ctx.cfg.dry_run {
        if is_funding {
            let _ = ManagedWalletRepo::revert_funding(ctx.pool, &[wallet.id]).await;
        }
        info!(
            managed_wallet_id = %wallet.id,
            address = %wallet.address,
            lamports = shortfall,
            "wallet funding (DRY RUN): would top up for launch"
        );
        report.outcomes.push(WalletFundOutcome {
            managed_wallet_id: wallet.id,
            address: wallet.address.clone(),
            role: wallet.role.clone(),
            amount_lamports: shortfall,
            result: "dry_run",
            signature: None,
            error: None,
        });
        return Ok(FundStep::Continue);
    }

    let (src_signer, src_pubkey) = {
        let src = &sources.sources[source_idx];
        (src.signer.clone(), src.pubkey)
    };
    let send_result = match ctx.mode {
        FundMode::Background => {
            send_transfer(ctx.rpc, &src_signer, src_pubkey, target_pk, shortfall).await
        }
        FundMode::Manual => {
            submit_transfer(ctx.rpc, &src_signer, src_pubkey, target_pk, shortfall).await
        }
    };
    match send_result {
        Ok(sig) => {
            sources.sources[source_idx].spent += shortfall;
            report.spent_lamports += shortfall;
            // Background confirmed the send, so the SOL has landed — promote to
            // `funded` now (off the projected balance) rather than waiting a poll
            // cycle, so the launch can claim it immediately. Manual leaves the
            // promotion to the balance poller (the tx may not have landed yet).
            if ctx.mode == FundMode::Background {
                let projected = balance.saturating_add(shortfall) as i64;
                if let Err(e) = ManagedWalletRepo::record_balance(
                    ctx.pool,
                    wallet.id,
                    projected,
                    MIN_FUNDED_LAMPORTS,
                )
                .await
                {
                    warn!(?e, managed_wallet_id = %wallet.id, "wallet funding: send ok but promote failed");
                }
            }
            info!(
                managed_wallet_id = %wallet.id,
                address = %wallet.address,
                lamports = shortfall,
                sig = %sig,
                "wallet funding: launch top-up sent"
            );
            report.outcomes.push(WalletFundOutcome {
                managed_wallet_id: wallet.id,
                address: wallet.address.clone(),
                role: wallet.role.clone(),
                amount_lamports: shortfall,
                result: "sent",
                signature: Some(sig.to_string()),
                error: None,
            });
        }
        Err(e) => {
            if is_funding {
                let _ = ManagedWalletRepo::revert_funding(ctx.pool, &[wallet.id]).await;
            }
            warn!(managed_wallet_id = %wallet.id, %e, "wallet funding: launch top-up failed");
            report.outcomes.push(WalletFundOutcome {
                managed_wallet_id: wallet.id,
                address: wallet.address.clone(),
                role: wallet.role.clone(),
                amount_lamports: shortfall,
                result: "failed",
                signature: None,
                error: Some(format!("{e}")),
            });
        }
    }

    if ctx.mode == FundMode::Background && ctx.cfg.max_delay_ms > 0 {
        let delay = rand::thread_rng().gen_range(0..=ctx.cfg.max_delay_ms);
        tokio::time::sleep(Duration::from_millis(delay)).await;
    }

    Ok(FundStep::Continue)
}

fn bad_address_outcome(w: &ManagedWallet) -> WalletFundOutcome {
    WalletFundOutcome {
        managed_wallet_id: w.id,
        address: w.address.clone(),
        role: w.role.clone(),
        amount_lamports: 0,
        result: "skipped_bad_address",
        signature: None,
        error: None,
    }
}

/// Send + confirm a funding transfer (background pass). No retry — the caller
/// reverts the claim on error and the next pass re-attempts. Phase 2.F: the
/// treasury→pool move is a typed `TransferSol` executed through the SSOT
/// [`crate::plan_exec::execute_transfer`] (exact lamports, treasury pays the fee).
async fn send_transfer(
    rpc: &RpcClient,
    treasury_signer: &Arc<dyn Signer + Send + Sync>,
    from: Pubkey,
    to: Pubkey,
    lamports: u64,
) -> Result<Signature> {
    let (sig, _) = crate::plan_exec::execute_transfer(
        rpc,
        treasury_signer.as_ref(),
        from,
        to,
        crate::plan_exec::TransferMode::Exact(lamports),
        true, // confirm
    )
    .await?
    .context("funding transfer produced no signature")?;
    Ok(sig)
}

/// Submit a funding transfer WITHOUT waiting for confirmation (manual pass):
/// returns as soon as the RPC accepts the tx. The balance poller promotes
/// `funding -> funded` when the SOL lands — see [`FundMode::Manual`].
async fn submit_transfer(
    rpc: &RpcClient,
    treasury_signer: &Arc<dyn Signer + Send + Sync>,
    from: Pubkey,
    to: Pubkey,
    lamports: u64,
) -> Result<Signature> {
    let (sig, _) = crate::plan_exec::execute_transfer(
        rpc,
        treasury_signer.as_ref(),
        from,
        to,
        crate::plan_exec::TransferMode::Exact(lamports),
        false, // submit only
    )
    .await?
    .context("funding transfer produced no signature")?;
    Ok(sig)
}

/// Spawn the background funder. Long-lived; keeps every fundable role warm. Gate
/// the *spawn* on `settings.funding.is_some()` at the call site (mirror the
/// dust-sweep wiring) — this loop assumes funding is configured.
pub fn spawn_wallet_funding(
    pool: PgPool,
    settings: LauncherSettings,
    sink: Option<Arc<dyn EventSink>>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(FUND_INTERVAL);
        loop {
            tick.tick().await;
            match fund_once(
                &pool,
                &settings,
                FundScope::default(),
                FundMode::Background,
                sink.as_deref(),
            )
            .await
            {
                Ok(report) if report.outcomes.is_empty() => {}
                Ok(report) => info!(
                    spent_lamports = report.spent_lamports,
                    wallets = report.outcomes.len(),
                    "wallet funding pass complete"
                ),
                Err(e) => warn!(?e, "wallet funding pass failed"),
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_jittered_one_transfer_per_target_within_bounds() {
        let amount = 50_000_000u64;
        let jitter = 0.15;
        let targets: Vec<(Uuid, Pubkey)> =
            (0..32).map(|_| (Uuid::new_v4(), Pubkey::new_unique())).collect();
        let transfers = DirectJittered.plan_transfers(
            Pubkey::new_unique(),
            &targets,
            &StrategyParams { amount_lamports: amount, jitter_pct: jitter },
        );
        assert_eq!(transfers.len(), targets.len(), "one transfer per target");
        let lo = (amount as f64 * (1.0 - jitter)).round() as u64;
        let hi = (amount as f64 * (1.0 + jitter)).round() as u64;
        for (t, (id, pk)) in transfers.iter().zip(&targets) {
            assert_eq!(t.managed_wallet_id, *id);
            assert_eq!(t.target, *pk);
            assert!(
                t.lamports >= lo && t.lamports <= hi,
                "amount {} outside [{lo}, {hi}]",
                t.lamports
            );
        }
    }

    #[test]
    fn only_dev_and_bundler_are_fundable() {
        let cfg = FundingConfig {
            treasury_reserve_lamports: 0,
            max_spend_per_interval_lamports: 0,
            amount_dev_lamports: 5,
            amount_bundler_lamports: 3,
            amount_jitter_pct: 0.0,
            max_delay_ms: 0,
            target_funded_dev: 2,
            target_funded_bundler: 4,
            dry_run: true,
        };
        assert_eq!(role_plan(WalletRole::Dev, &cfg), Some((5, 2)));
        assert_eq!(role_plan(WalletRole::Bundler, &cfg), Some((3, 4)));
        assert_eq!(role_plan(WalletRole::Treasury, &cfg), None);
        assert_eq!(role_plan(WalletRole::Trading, &cfg), None);
    }
}
