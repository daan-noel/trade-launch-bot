//! Wallet funding orchestration (docs/wallet-funding-plan.md): the inverse of
//! `dust_sweep.rs`. Sends SOL treasury -> pool so `generated` wallets become
//! `funded` and claimable for launches. The pool could already generate wallets
//! and reclaim dust — this closes the loop by *sending* SOL in.
//!
//! **Operator-triggered only.** The autonomous background funder and the JIT
//! per-launch funder were removed; the sole entry point is [`fund_once`], driven
//! by the "Fund pool" button (`POST /api/wallet_pool/fund`). Nothing here spends
//! SOL unattended. Each send is confirmed and the wallet promoted to `funded`
//! in-place, so one button click leaves the pool claimable with no background poll.
//!
//! Deliberately a plain `solana-client` transfer (NOT pump-trader's Jito/
//! multi-sender/retry machinery) — funding has no landing urgency, unlike a snipe
//! buy. Same choice as [`crate::dust_sweep`].
//!
//! SAFETY: this spends real SOL. Every send is gated by [`FundingConfig`]'s
//! reserve floor + per-interval cap, and the whole subsystem is off unless
//! `FUND_ENABLED=true` (see `config.rs`). `FUND_DRY_RUN=true` plans + logs
//! transfers but sends nothing.
//!
//! Obfuscation = Tier 1: one transfer per tx, jittered amount, direct
//! treasury->wallet. Tier 2 (multi-hop fan-out) is left as an unimplemented
//! [`FundingStrategy`] impl behind the same seam.

use std::str::FromStr;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use platform_core::models::{ManagedWallet, WalletRole};
use platform_core::storage::repositories::ManagedWalletRepo;
use rand::Rng;
use serde::Serialize;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Signature, Signer};
use sqlx::PgPool;
use tracing::{info, warn};
use uuid::Uuid;

use crate::config::{FundingConfig, LauncherSettings};
use crate::events::EventSink;
use crate::keystore::{self, EnvKek};
use crate::wallet_pool::MIN_FUNDED_LAMPORTS;

/// Solana JSON-RPC `getMultipleAccounts` caps at 100 pubkeys per call — the
/// post-fund balance read-back chunks the sent wallets to this.
const RPC_BATCH_SIZE: usize = 100;

/// Refresh the reused funding blockhash after this many sends so a long pass can't
/// outlive it (a recent blockhash is valid ~150 slots / ~60 s; 20 fire-and-forget
/// sends complete well inside that).
const BLOCKHASH_REFRESH_EVERY: usize = 20;

/// Post-fund promotion read-back: fire-and-forget sends may not have landed at the
/// first `getMultipleAccounts`, so re-read the not-yet-funded wallets a few times
/// with a short pause before leaving them `funding` for the operator's Refresh.
const PROMOTE_MAX_ATTEMPTS: usize = 4;
const PROMOTE_RETRY_MS: u64 = 750;

/// Serializes every funding pass in this process. The reserve floor + per-interval
/// spend cap are enforced against a pass-local treasury snapshot (`TreasuryPool`
/// captured at pass start + a running `spent`), so two concurrent "Fund pool"
/// clicks each see the other's spend as zero and could together spend N× the cap
/// or breach the reserve. Holding this across a whole pass makes the snapshot
/// authoritative: at most one pass reads balances, spends, and writes them back at
/// a time. Correctness over latency — real SOL is moving, and a warm-pool pass is
/// a fast no-op.
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
/// `[amount*(1-j), amount*(1+j)]`.
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
        // Reuse the balance a recent "Refresh balances" wrote when it's still
        // fresh (audit Phase C2); else read on-chain.
        let balance = match crate::wallet_pool::fresh_cached_balance(&w) {
            Some(b) => b,
            None => rpc
                .get_balance(&pubkey)
                .await
                .with_context(|| format!("fetch treasury {} balance", w.address))?,
        };
        let signer = keystore::resolve_signer(&settings.keystore_dir, &w.key_ref, kek)
            .with_context(|| format!("resolve treasury {} signer", w.address))?;
        sources.push(TreasurySource { wallet: w, pubkey, signer, balance, spent: 0 });
    }
    Ok(TreasuryPool { sources })
}

/// Reflect each source treasury's spend on its cached balance immediately, so the
/// Wallet Pool page shows the drop right after the pass instead of a stale number.
/// Optimistic (ignores tx fees); the operator "Refresh balances" reconciles the
/// exact on-chain value. `record_balance`'s promotion CASE only fires for
/// `generated`/`funding`, so it never mutates a treasury's status.
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

/// Scope for a single funding pass. Defaults (both `None`) = top every fundable
/// role up to its warm target. The endpoint narrows it (a specific role and/or an
/// explicit count).
#[derive(Debug, Clone, Default)]
pub struct FundScope {
    pub role: Option<WalletRole>,
    pub count: Option<i64>,
}

/// What happened to one wallet in a funding pass — the endpoint's per-wallet
/// response, and the log detail.
#[derive(Debug, Serialize)]
pub struct WalletFundOutcome {
    pub managed_wallet_id: Uuid,
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

/// Run one operator-triggered funding pass ("Fund pool"). Resolves the treasury,
/// tops up each in-scope fundable role, CONFIRMS each send, then promotes the
/// funded wallets to `funded` off a fresh on-chain read — so the pool is claimable
/// the moment this returns, with no background poll. Best-effort: a single
/// wallet's failure reverts just that wallet (`funding` -> `generated`) and the
/// pass continues; a safety-rail breach (reserve floor / per-interval cap) reverts
/// every still-unsent claim and stops.
pub async fn fund_once(
    pool: &PgPool,
    settings: &LauncherSettings,
    scope: FundScope,
    sink: Option<&dyn EventSink>,
) -> Result<FundReport> {
    let Some(cfg) = settings.funding.as_ref() else {
        bail!("wallet funding not configured (FUND_ENABLED not set)");
    };

    // Serialize against every other funding pass — see `FUNDING_LOCK`. Held for the
    // whole pass so the treasury snapshot + spend accounting stay authoritative.
    let _funding_guard = FUNDING_LOCK.lock().await;

    let mut report = FundReport::default();
    // (id, pubkey) of every wallet a send confirmed for — promoted after the pass.
    let mut sent: Vec<(Uuid, Pubkey)> = Vec::new();
    // One recent blockhash reused across the whole pass (funding is not landing-
    // urgent), fetched lazily on the first send and refreshed every
    // `BLOCKHASH_REFRESH_EVERY` sends — saves N−1 `getLatestBlockhash` per pass.
    let mut blockhash: Option<Hash> = None;
    let mut sends_since_refresh = 0usize;

    // Cheap indexed shortfall gate BEFORE paying for the treasury RPC snapshot
    // (audit §5): a top-up over a fully warm pool otherwise builds the treasury
    // pool — N `get_balance` RPCs — only to fund nothing. An explicit `count`
    // always proceeds; a top-up (count = None) bails when `funded_count` already
    // meets every target.
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
            ManagedWalletRepo::claim_for_funding(pool, role.as_str(), n, "fund pool (manual)").await?;
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

            // Reuse one recent blockhash across the pass; refresh it lazily/periodically.
            // A blockhash fetch failure can't fund this wallet — revert its claim and
            // continue (same posture as a failed send).
            if blockhash.is_none() || sends_since_refresh >= BLOCKHASH_REFRESH_EVERY {
                match rpc.get_latest_blockhash().await {
                    Ok(bh) => {
                        blockhash = Some(bh);
                        sends_since_refresh = 0;
                    }
                    // A stale prior blockhash may still be within its validity window, so
                    // reuse it rather than reverting; only revert if we have none at all.
                    Err(e) if blockhash.is_some() => {
                        warn!(%e, "wallet funding: blockhash refresh failed — reusing prior");
                    }
                    Err(e) => {
                        let _ = ManagedWalletRepo::revert_funding(pool, &[t.managed_wallet_id]).await;
                        warn!(managed_wallet_id = %t.managed_wallet_id, %e, "wallet funding: blockhash fetch failed — reverted");
                        report.outcomes.push(WalletFundOutcome {
                            managed_wallet_id: t.managed_wallet_id,
                            address: t.target.to_string(),
                            role: role.as_str().to_string(),
                            amount_lamports: t.lamports,
                            result: "failed",
                            signature: None,
                            error: Some(format!("blockhash fetch failed: {e}")),
                        });
                        continue;
                    }
                }
            }
            let bh = blockhash.expect("blockhash set above");

            // Fire-and-forget: a failed *submit* reverts the claim; landing is confirmed
            // by the batched `promote_funded` read-back after the pass (no per-wallet
            // `getSignatureStatuses` confirm loop). Funding has no landing urgency.
            match send_transfer(&rpc, &src_signer, src_pubkey, t.target, t.lamports, bh).await {
                Ok(sig) => {
                    sources.sources[source_idx].spent += t.lamports;
                    report.spent_lamports += t.lamports;
                    sends_since_refresh += 1;
                    sent.push((t.managed_wallet_id, t.target));
                    info!(
                        managed_wallet_id = %t.managed_wallet_id,
                        address = %t.target,
                        lamports = t.lamports,
                        sig = %sig,
                        "wallet funding: transfer sent"
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
        }
    }

    // Promote every wallet whose send confirmed to `funded`, off a fresh on-chain
    // read — the manual counterpart of the old balance poll, done once at the end
    // of the pass so a single click leaves the pool claimable.
    if !cfg.dry_run {
        promote_funded(pool, &rpc, &sent).await;
        writeback_treasury_balances(pool, &sources).await;
    }

    // Push a coarse "pool changed" signal if this pass actually touched wallets, so
    // the Wallet Pool page refetches instead of polling (audit Phase A2).
    notify_pool_changed(sink, &report);
    Ok(report)
}

/// Read the on-chain balance of every wallet a send submitted for and stamp it via
/// `record_balance`, which promotes `funding` -> `funded` once the balance clears
/// `MIN_FUNDED_LAMPORTS`. Because sends are now fire-and-forget, a wallet may not
/// have landed at the first read — so re-read the still-below-threshold wallets a
/// few times ([`PROMOTE_MAX_ATTEMPTS`], [`PROMOTE_RETRY_MS`]) before giving up. Any
/// wallet still short after the retries stays `funding` — the operator's next
/// "Refresh balances" click reconciles it (the SOL likely landed; only the DB lags).
async fn promote_funded(pool: &PgPool, rpc: &RpcClient, sent: &[(Uuid, Pubkey)]) {
    let mut pending: Vec<(Uuid, Pubkey)> = sent.to_vec();
    for attempt in 0..PROMOTE_MAX_ATTEMPTS {
        if pending.is_empty() {
            break;
        }
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(PROMOTE_RETRY_MS)).await;
        }
        let mut still_pending: Vec<(Uuid, Pubkey)> = Vec::new();
        for chunk in pending.chunks(RPC_BATCH_SIZE) {
            let pubkeys: Vec<Pubkey> = chunk.iter().map(|(_, pk)| *pk).collect();
            let accounts = match rpc.get_multiple_accounts(&pubkeys).await {
                Ok(a) => a,
                Err(e) => {
                    warn!(?e, "wallet funding: post-fund balance read failed — will retry");
                    still_pending.extend_from_slice(chunk);
                    continue;
                }
            };
            for ((id, pk), acct) in chunk.iter().zip(accounts) {
                let lamports = acct.map(|a| a.lamports).unwrap_or(0) as i64;
                // Stamp every read; `record_balance` promotes once it clears the floor.
                if let Err(e) =
                    ManagedWalletRepo::record_balance(pool, *id, lamports, MIN_FUNDED_LAMPORTS).await
                {
                    warn!(?e, managed_wallet_id = %id, "wallet funding: promote after fund failed");
                }
                if lamports < MIN_FUNDED_LAMPORTS {
                    still_pending.push((*id, *pk));
                }
            }
        }
        pending = still_pending;
    }
    if !pending.is_empty() {
        warn!(
            count = pending.len(),
            "wallet funding: some wallets not confirmed funded after retries — left `funding` for Refresh"
        );
    }
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

/// Submit a funding transfer over a **caller-supplied** blockhash, fire-and-forget.
/// No retry and no per-send confirmation — the caller reverts the claim on a submit
/// error, the batched [`promote_funded`] read-back confirms landing, and the operator
/// re-clicks "Fund pool" to re-attempt a miss. Phase 2.F: the treasury->pool move is
/// a typed `TransferSol` executed through the SSOT
/// [`crate::plan_exec::execute_transfer_with_blockhash`] (exact lamports, treasury pays the fee).
async fn send_transfer(
    rpc: &RpcClient,
    treasury_signer: &Arc<dyn Signer + Send + Sync>,
    from: Pubkey,
    to: Pubkey,
    lamports: u64,
    blockhash: Hash,
) -> Result<Signature> {
    let (sig, _) = crate::plan_exec::execute_transfer_with_blockhash(
        rpc,
        treasury_signer.as_ref(),
        from,
        to,
        crate::plan_exec::TransferMode::Exact(lamports),
        false, // fire-and-forget; promote_funded confirms landing
        blockhash,
    )
    .await?
    .context("funding transfer produced no signature")?;
    Ok(sig)
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
