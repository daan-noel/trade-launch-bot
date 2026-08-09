//! Boot wallet-balance sweep — buy-in-flight recovery (backstop).
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

use trading_core::models::is_expected_non_position;
use trading_core::storage::repositories::strategy_repo::StrategyRepo;
use crate::trader::{PumpFunTrader, WalletHolding};

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
/// open position nor an expected non-position balance (WSOL plumbing + USDC cash).
/// Split from the RPC/DB I/O so the attribution rule is unit-tested directly.
fn unattributed<'a>(
    holdings: &'a [WalletHolding],
    tracked_mints: &HashSet<String>,
) -> Vec<&'a WalletHolding> {
    holdings
        .iter()
        .filter(|h| !is_expected_non_position(&h.mint) && !tracked_mints.contains(&h.mint))
        .collect()
}

/// List the wallet's on-chain token accounts and reconcile them against every
/// open (non-`End`) real position. Logs a summary, flags each unattributable
/// balance for review, and returns them. Best-effort: a failed RPC/DB read returns
/// an `Err` the caller logs — the sweep is never fatal.
pub async fn reconcile_wallet_holdings(
    trader: &PumpFunTrader,
    strategy_repo: &StrategyRepo,
) -> anyhow::Result<Vec<UnattributedHolding>> {
    // On-chain ground truth: every non-zero token account the wallet holds.
    let holdings = trader.get_all_token_accounts().await?;

    // The set of mints any open position could own tokens for, across ALL
    // strategies. `distinct_unsettled_real_mints` covers Holding /
    // BuySubmitted / ExitPending / ExitStuck / ExitUnconfirmed (the last two can
    // still hold a bag whose
    // sell failed), i.e. exactly the states where tokens may legitimately sit in
    // the wallet. The unified `strategy_positions` table makes this one query.
    let tracked: HashSet<String> =
        strategy_repo.distinct_unsettled_real_mints().await?.into_iter().collect();

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
    fn flags_only_untracked_non_expected_mints() {
        let wsol = trading_core::config::constants::WSOL_MINT;
        let usdc = trading_core::config::constants::USDC_MINT;
        let holdings = vec![
            holding("AAA"),
            holding("BBB"),
            holding(wsol),
            holding(usdc),
        ];
        let tracked: HashSet<String> = ["AAA".to_string()].into_iter().collect();

        let flagged = unattributed(&holdings, &tracked);
        // AAA tracked; WSOL + USDC expected non-position → only BBB is unattributed.
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
    fn wsol_and_usdc_alone_are_never_flagged() {
        let holdings = vec![
            holding(trading_core::config::constants::WSOL_MINT),
            holding(trading_core::config::constants::USDC_MINT),
        ];
        let tracked: HashSet<String> = HashSet::new();
        assert!(unattributed(&holdings, &tracked).is_empty());
    }
}
