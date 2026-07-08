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
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use platform_core::models::{ManagedWallet, WalletRole};
use platform_core::storage::repositories::ManagedWalletRepo;
use rand::Rng;
use serde::Serialize;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::message::Message;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Signature, Signer};
use solana_sdk::system_instruction;
use solana_sdk::transaction::Transaction;
use sqlx::PgPool;
use tokio::task::JoinHandle;
use tracing::{info, warn};
use uuid::Uuid;

use crate::config::{FundingConfig, LauncherSettings};
use crate::keystore::{self, EnvKek};

/// How often the background funder tops the pool up. Cheap when idle — a single
/// `funded_count` per fundable role, no work when every role is already warm.
const FUND_INTERVAL: Duration = Duration::from_secs(60);

/// One planned treasury->wallet transfer. Amount is already jittered.
#[derive(Debug, Clone)]
pub struct Transfer {
    pub wallet_id: Uuid,
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
                Transfer { wallet_id: *id, target: *pk, lamports }
            })
            .collect()
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

/// What happened to one wallet in a funding pass — the manual endpoint's
/// per-wallet response, and the log detail for the background pass.
#[derive(Debug, Serialize)]
pub struct WalletFundOutcome {
    pub wallet_id: Uuid,
    pub address: String,
    pub role: String,
    pub amount_lamports: u64,
    /// `sent` | `dry_run` | `failed` | `skipped_cap` | `skipped_bad_address`.
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
) -> Result<FundReport> {
    let Some(cfg) = settings.funding.as_ref() else {
        bail!("wallet funding not configured (FUND_ENABLED not set)");
    };

    let mut report = FundReport::default();

    // Resolve the treasury (the SOL source). No-op if absent — same posture as
    // the dust sweep's missing-treasury case.
    let Some(treasury) = ManagedWalletRepo::by_role(pool, WalletRole::Treasury.as_str())
        .await?
        .into_iter()
        .next()
    else {
        warn!("wallet funding: no treasury wallet configured (role=treasury) — skipping");
        return Ok(report);
    };
    let treasury_pk =
        Pubkey::from_str(&treasury.address).context("parse treasury wallet address")?;

    let rpc =
        RpcClient::new_with_commitment(settings.rpc_url.clone(), CommitmentConfig::confirmed());
    let treasury_balance = rpc
        .get_balance(&treasury_pk)
        .await
        .context("fetch treasury balance")?;

    let kek = EnvKek::from_passphrase(&settings.kek_passphrase);
    let treasury_signer = keystore::resolve_signer(&settings.keystore_dir, &treasury.key_ref, &kek)
        .context("resolve treasury signer")?;
    let funding_source = format!("treasury {} (auto-fund)", treasury.address);
    let strategy = DirectJittered;

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
                    warn!(wallet_id = %w.id, %e, "wallet funding: bad address — reverted");
                    report.outcomes.push(bad_address_outcome(w));
                }
            }
        }

        let transfers = strategy.plan_transfers(
            treasury_pk,
            &targets,
            &StrategyParams { amount_lamports: amount, jitter_pct: cfg.amount_jitter_pct },
        );

        for (i, t) in transfers.iter().enumerate() {
            // Safety rails — breach reverts every still-unsent claim and stops.
            let over_cap = report.spent_lamports + t.lamports > cfg.max_spend_per_interval_lamports;
            let under_reserve = treasury_balance
                .saturating_sub(report.spent_lamports)
                .saturating_sub(t.lamports)
                < cfg.treasury_reserve_lamports;
            if over_cap || under_reserve {
                let unsent: Vec<Uuid> = transfers[i..].iter().map(|t| t.wallet_id).collect();
                let _ = ManagedWalletRepo::revert_funding(pool, &unsent).await;
                warn!(
                    role = role.as_str(),
                    over_cap,
                    under_reserve,
                    spent = report.spent_lamports,
                    reverted = unsent.len(),
                    "wallet funding: safety rail hit — reverted remaining claims, stopping pass"
                );
                for t in &transfers[i..] {
                    report.outcomes.push(WalletFundOutcome {
                        wallet_id: t.wallet_id,
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

            if cfg.dry_run {
                // Plan + log, send nothing, release the claim so it isn't stranded.
                let _ = ManagedWalletRepo::revert_funding(pool, &[t.wallet_id]).await;
                info!(
                    wallet_id = %t.wallet_id,
                    address = %t.target,
                    lamports = t.lamports,
                    "wallet funding (DRY RUN): would transfer"
                );
                report.outcomes.push(WalletFundOutcome {
                    wallet_id: t.wallet_id,
                    address: t.target.to_string(),
                    role: role.as_str().to_string(),
                    amount_lamports: t.lamports,
                    result: "dry_run",
                    signature: None,
                    error: None,
                });
                continue;
            }

            match send_transfer(&rpc, &treasury_signer, treasury_pk, t.target, t.lamports).await {
                Ok(sig) => {
                    report.spent_lamports += t.lamports;
                    info!(
                        wallet_id = %t.wallet_id,
                        address = %t.target,
                        lamports = t.lamports,
                        sig = %sig,
                        "wallet funding: transfer sent (poller will promote funding -> funded)"
                    );
                    report.outcomes.push(WalletFundOutcome {
                        wallet_id: t.wallet_id,
                        address: t.target.to_string(),
                        role: role.as_str().to_string(),
                        amount_lamports: t.lamports,
                        result: "sent",
                        signature: Some(sig.to_string()),
                        error: None,
                    });
                }
                Err(e) => {
                    let _ = ManagedWalletRepo::revert_funding(pool, &[t.wallet_id]).await;
                    warn!(wallet_id = %t.wallet_id, %e, "wallet funding: transfer failed — reverted");
                    report.outcomes.push(WalletFundOutcome {
                        wallet_id: t.wallet_id,
                        address: t.target.to_string(),
                        role: role.as_str().to_string(),
                        amount_lamports: t.lamports,
                        result: "failed",
                        signature: None,
                        error: Some(format!("{e}")),
                    });
                }
            }

            // Timing jitter — de-correlate the sends in time. Compute the delay in
            // a tight scope so the (non-Send) RNG never crosses the await.
            if cfg.max_delay_ms > 0 {
                let delay = rand::thread_rng().gen_range(0..=cfg.max_delay_ms);
                tokio::time::sleep(Duration::from_millis(delay)).await;
            }
        }
    }

    Ok(report)
}

fn bad_address_outcome(w: &ManagedWallet) -> WalletFundOutcome {
    WalletFundOutcome {
        wallet_id: w.id,
        address: w.address.clone(),
        role: w.role.clone(),
        amount_lamports: 0,
        result: "skipped_bad_address",
        signature: None,
        error: None,
    }
}

/// Plain treasury-signed SOL transfer (fee paid by the treasury). No retry — the
/// caller reverts the claim on error and the next pass re-attempts.
async fn send_transfer(
    rpc: &RpcClient,
    treasury_signer: &Arc<dyn Signer + Send + Sync>,
    from: Pubkey,
    to: Pubkey,
    lamports: u64,
) -> Result<Signature> {
    let blockhash = rpc.get_latest_blockhash().await.context("fetch blockhash")?;
    let ix = system_instruction::transfer(&from, &to, lamports);
    let msg = Message::new_with_blockhash(&[ix], Some(&from), &blockhash);
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[treasury_signer.as_ref() as &dyn Signer], blockhash)
        .context("sign funding transfer")?;
    rpc.send_and_confirm_transaction(&tx)
        .await
        .context("send funding transfer")
}

/// Spawn the background funder. Long-lived; keeps every fundable role warm. Gate
/// the *spawn* on `settings.funding.is_some()` at the call site (mirror the
/// dust-sweep wiring) — this loop assumes funding is configured.
pub fn spawn_wallet_funding(pool: PgPool, settings: LauncherSettings) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(FUND_INTERVAL);
        loop {
            tick.tick().await;
            match fund_once(&pool, &settings, FundScope::default()).await {
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
            assert_eq!(t.wallet_id, *id);
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
