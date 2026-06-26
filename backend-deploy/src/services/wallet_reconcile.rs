//! Boot wallet-balance sweep — buy-in-flight recovery, **Phase 3** (backstop).
//!
//! The durable `BuySubmitted` marker (Phases 1–2) makes the *bot's own* buy path
//! crash-safe: every signed buy is recorded before it can land, so a restart can
//! always reconcile a position against its submitted signatures. This sweep is the
//! ground-truth backstop for everything that path can't see — a manual transfer
//! into the wallet, a marker that failed to persist, or any future bug — by
//! checking the wallet's actual on-chain token accounts against the set of mints
//! any open position could own.
//!
//! It runs **once at boot, off the critical path** (a spawned task): list the
//! wallet's non-zero token accounts (one RPC scan, two concurrent calls inside
//! [`PumpFunTrader::get_all_token_accounts`]) and flag any balance whose mint is
//! attributed to no open position across **both** strategy clones. Read-only and
//! advisory — it never sells or deletes (the bot can't safely attribute or exit a
//! bag it has no rule/position context for); it only surfaces the discrepancy for
//! manual review (rent reclaim / manual sell).

use std::collections::HashSet;

use tracing::{info, warn};

use backend_core::storage::repositories::{
    tpsl1_position_repo::Tpsl1PositionRepo, tpsl2_position_repo::Tpsl2PositionRepo,
};
use crate::trader::{PumpFunTrader, WalletHolding};

/// The native wrapped-SOL mint. The trade paths wrap/unwrap WSOL per AMM swap and
/// close the account on completion, but a transient WSOL balance at boot is
/// expected plumbing — never an orphaned token bag — so it is excluded from the
/// unattributed report rather than flagged every restart.
const WSOL_MINT: &str = pump_trader::constants::WSOL_MINT;

/// One on-chain token balance the wallet holds that no open position accounts for
/// — surfaced for manual review, never auto-acted-on.
#[derive(Debug, Clone)]
pub struct UnattributedHolding {
    pub mint: String,
    pub amount: u64,
    pub ui_amount: f64,
    pub token_account: String,
}

/// Pure reconciliation: the wallet holdings whose mint is neither tracked by an
/// open position nor an expected non-position balance (WSOL). Split from the
/// RPC/DB I/O so the attribution rule is unit-tested directly.
fn unattributed<'a>(
    holdings: &'a [WalletHolding],
    tracked_mints: &HashSet<String>,
) -> Vec<&'a WalletHolding> {
    holdings
        .iter()
        .filter(|h| h.mint != WSOL_MINT && !tracked_mints.contains(&h.mint))
        .collect()
}

/// List the wallet's on-chain token accounts and reconcile them against every
/// open (non-`End`) position across both strategy clones. Logs a summary, flags
/// each unattributable balance for review, and returns them. Best-effort: a failed
/// RPC/DB read returns an `Err` the caller logs — the sweep is never fatal.
pub async fn reconcile_wallet_holdings(
    trader: &PumpFunTrader,
    tpsl1_repo: &Tpsl1PositionRepo,
    tpsl2_repo: &Tpsl2PositionRepo,
) -> anyhow::Result<Vec<UnattributedHolding>> {
    // On-chain ground truth: every non-zero token account the wallet holds.
    let holdings = trader.get_all_token_accounts().await?;

    // The set of mints any open position could own tokens for, across BOTH clones
    // — a single-clone view would mis-flag the other clone's bags as orphans.
    // `distinct_unsettled_mints` covers Holding / Arming / BuySubmitted /
    // ExitPending / ExitFailed (the last can still hold a bag whose sell failed),
    // i.e. exactly the states where tokens may legitimately sit in the wallet.
    let mut tracked: HashSet<String> = HashSet::new();
    for m in tpsl1_repo.distinct_unsettled_mints().await? {
        tracked.insert(m);
    }
    for m in tpsl2_repo.distinct_unsettled_mints().await? {
        tracked.insert(m);
    }

    let flagged: Vec<UnattributedHolding> = unattributed(&holdings, &tracked)
        .into_iter()
        .map(|h| UnattributedHolding {
            mint: h.mint.clone(),
            amount: h.amount,
            ui_amount: h.ui_amount,
            token_account: h.token_account.clone(),
        })
        .collect();

    if flagged.is_empty() {
        info!(
            held = holdings.len(),
            tracked = tracked.len(),
            "Wallet reconcile: every on-chain token balance is attributed to an open position"
        );
    } else {
        warn!(
            held = holdings.len(),
            tracked = tracked.len(),
            unattributed = flagged.len(),
            "Wallet reconcile: on-chain token balances with NO owning position — needs manual \
             review (rent reclaim / manual sell); the bot will NOT auto-sell an unattributable bag"
        );
        for h in &flagged {
            warn!(
                mint = %h.mint,
                amount = h.amount,
                ui_amount = h.ui_amount,
                token_account = %h.token_account,
                "Wallet reconcile: unattributed holding"
            );
        }
    }

    Ok(flagged)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn holding(mint: &str) -> WalletHolding {
        WalletHolding {
            mint: mint.to_string(),
            amount: 1_000,
            ui_amount: 1.0,
            decimals: 6,
            token_account: format!("acct-{mint}"),
            token_program_id: pump_trader::constants::TOKEN_PROGRAM_ID.to_string(),
        }
    }

    #[test]
    fn flags_only_untracked_non_wsol_mints() {
        let holdings = vec![holding("AAA"), holding("BBB"), holding(WSOL_MINT)];
        let tracked: HashSet<String> = ["AAA".to_string()].into_iter().collect();

        let flagged = unattributed(&holdings, &tracked);
        // AAA is tracked, WSOL is excluded plumbing → only BBB is unattributed.
        assert_eq!(flagged.len(), 1);
        assert_eq!(flagged[0].mint, "BBB");
    }

    #[test]
    fn no_flags_when_everything_is_tracked() {
        let holdings = vec![holding("AAA"), holding("BBB")];
        let tracked: HashSet<String> =
            ["AAA".to_string(), "BBB".to_string()].into_iter().collect();
        assert!(unattributed(&holdings, &tracked).is_empty());
    }

    #[test]
    fn wsol_alone_is_never_flagged() {
        let holdings = vec![holding(WSOL_MINT)];
        let tracked: HashSet<String> = HashSet::new();
        assert!(unattributed(&holdings, &tracked).is_empty());
    }
}
