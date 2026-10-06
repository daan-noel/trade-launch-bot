//! Stranded token-account rent on the trading wallet.
//!
//! A buy funds a fresh account. The exit closes the one that position recorded.
//! Anything left empty (a close that did not land, a second account on the same
//! mint) stays on the wallet at ~0.002 SOL and is invisible to the holdings
//! scan, which drops a zero balance. This scan is the mop: it classifies every
//! account and closes the ones that are safe to close.

use std::collections::{HashMap, HashSet};

use pump_trader::protocol::WSOL_MINT;
use pump_trader::{OwnedTokenAccount, ReclaimSendReport};
use serde::Serialize;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

use crate::state::deploy_state::DeployState;

/// Raw units at or under which a non-empty account is dust. A full sell leaves
/// a handful of units; a live bag is orders of magnitude larger. 10_000 raw is
/// 0.01 token at pump's 6 decimals, and $0.01 of USDC — USDC is excluded from
/// the burn anyway.
pub const DUST_RAW_MAX: u64 = 10_000;

#[derive(Debug, Clone, Default, Serialize)]
pub struct RentBucket {
    pub accounts: u32,
    pub lamports: u64,
}

impl RentBucket {
    fn add(&mut self, lamports: u64) {
        self.accounts = self.accounts.saturating_add(1);
        self.lamports = self.lamports.saturating_add(lamports);
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RentStatus {
    pub empty: RentBucket,
    pub empty_legacy: u32,
    pub empty_token2022: u32,
    pub dust: RentBucket,
    pub wrapped: RentBucket,
    pub blocked: RentBucket,
    /// Empty accounts, and dust on a mint, that an open position still needs.
    pub open_position: RentBucket,
    /// Non-dust balances. They stay on the holdings table.
    pub held: u32,
    /// Mints that own more than one token account.
    pub multi_account_mints: u32,
    pub dust_raw_max: u64,
}

pub struct ClassifiedRent {
    pub status: RentStatus,
    /// Empty accounts, plus wrapped SOL (closing it unwraps).
    pub close: Vec<OwnedTokenAccount>,
    /// Dust the burn action may destroy.
    pub burn: Vec<OwnedTokenAccount>,
}

/// Split `accounts` into what a sweep may touch and what it must leave.
pub fn classify_rent(
    accounts: &[OwnedTokenAccount],
    open_mints: &HashSet<String>,
    open_accounts: &HashSet<String>,
) -> ClassifiedRent {
    let usdc =
        Pubkey::from_str(trading_core::config::constants::USDC_MINT).expect("USDC mint constant");
    let mut status = RentStatus {
        empty: RentBucket::default(),
        empty_legacy: 0,
        empty_token2022: 0,
        dust: RentBucket::default(),
        wrapped: RentBucket::default(),
        blocked: RentBucket::default(),
        open_position: RentBucket::default(),
        held: 0,
        multi_account_mints: 0,
        dust_raw_max: DUST_RAW_MAX,
    };
    let mut close = Vec::new();
    let mut burn = Vec::new();
    let mut per_mint: HashMap<Pubkey, u32> = HashMap::new();

    for account in accounts {
        *per_mint.entry(account.mint).or_insert(0) += 1;
        if account.frozen || account.withheld > 0 {
            status.blocked.add(account.lamports);
            continue;
        }
        let protected = open_accounts.contains(&account.pubkey.to_string());
        if account.mint == WSOL_MINT && account.amount > 0 {
            if protected {
                status.open_position.add(account.lamports);
            } else {
                status.wrapped.add(account.lamports);
                close.push(account.clone());
            }
            continue;
        }
        if account.amount == 0 {
            if protected {
                status.open_position.add(account.lamports);
            } else {
                status.empty.add(account.lamports);
                if account.program_id == pump_trader::protocol::TOKEN {
                    status.empty_legacy = status.empty_legacy.saturating_add(1);
                } else {
                    status.empty_token2022 = status.empty_token2022.saturating_add(1);
                }
                close.push(account.clone());
            }
            continue;
        }
        // Cash is never burned. An empty USDC account falls through the
        // amount == 0 arm above and is closed like any other empty account.
        if account.mint == usdc {
            status.held = status.held.saturating_add(1);
            continue;
        }
        if account.amount <= DUST_RAW_MAX {
            if protected || open_mints.contains(&account.mint.to_string()) {
                status.open_position.add(account.lamports);
            } else {
                status.dust.add(account.lamports);
                burn.push(account.clone());
            }
            continue;
        }
        status.held = status.held.saturating_add(1);
    }
    status.multi_account_mints = per_mint.values().filter(|n| **n > 1).count() as u32;
    ClassifiedRent {
        status,
        close,
        burn,
    }
}

#[derive(Debug, Serialize)]
pub struct RentRecoverResult {
    pub closed: u32,
    pub still_open: u32,
    pub lamports_returned: u64,
    pub skipped_inflight: u32,
    pub error_count: u32,
    pub errors: Vec<String>,
}

const ERRORS_SHOWN: usize = 5;

/// Re-read the wallet, close empty accounts and unwrap WSOL, and when
/// `burn_dust` is set also burn dust and close those accounts. Confirms each
/// batch, then re-reads so the counts are what is left on chain.
pub async fn recover(state: &DeployState, burn_dust: bool) -> anyhow::Result<RentRecoverResult> {
    let accounts = state.trader.list_owned_token_accounts().await?;
    let wallet = state.trader.wallet_pubkey();
    let (open_mints, open_accounts) = state.strategy_repo.open_real_rent_guards(&wallet).await?;
    let classified = classify_rent(&accounts, &open_mints, &open_accounts);

    let (close, skipped_close) = skip_inflight(state, classified.close);
    let (burn, skipped_burn) = if burn_dust {
        skip_inflight(state, classified.burn)
    } else {
        (Vec::new(), 0)
    };
    let skipped_inflight = skipped_close.saturating_add(skipped_burn);

    let attempted: Vec<OwnedTokenAccount> =
        close.iter().cloned().chain(burn.iter().cloned()).collect();
    if attempted.is_empty() {
        return Ok(RentRecoverResult {
            closed: 0,
            still_open: 0,
            lamports_returned: 0,
            skipped_inflight,
            error_count: 0,
            errors: Vec::new(),
        });
    }

    let mut report = state.trader.close_token_accounts(&close).await?;
    if burn_dust {
        merge_report(
            &mut report,
            state.trader.burn_and_close_token_accounts(&burn).await?,
        );
    }

    match state.trader.list_owned_token_accounts().await {
        Ok(left) => {
            let still: HashSet<String> = left.into_iter().map(|a| a.pubkey.to_string()).collect();
            let mut closed = 0u32;
            let mut still_open = 0u32;
            let mut lamports_returned = 0u64;
            for account in &attempted {
                if still.contains(&account.pubkey.to_string()) {
                    still_open = still_open.saturating_add(1);
                } else {
                    closed = closed.saturating_add(1);
                    lamports_returned = lamports_returned.saturating_add(account.lamports);
                }
            }
            let error_count = report.errors.len() as u32;
            Ok(RentRecoverResult {
                closed,
                still_open,
                lamports_returned,
                skipped_inflight,
                error_count,
                errors: report.errors.into_iter().take(ERRORS_SHOWN).collect(),
            })
        }
        Err(err) => {
            let error_count = (report.errors.len() + 1) as u32;
            let mut errors = vec![format!("re-read failed: {err}")];
            errors.extend(
                report
                    .errors
                    .into_iter()
                    .take(ERRORS_SHOWN.saturating_sub(1)),
            );
            Ok(RentRecoverResult {
                closed: report.closed as u32,
                still_open: attempted.len().saturating_sub(report.closed) as u32,
                lamports_returned: report.closed_lamports,
                skipped_inflight,
                error_count,
                errors,
            })
        }
    }
}

pub async fn status(state: &DeployState) -> anyhow::Result<RentStatus> {
    let accounts = state.trader.list_owned_token_accounts().await?;
    let wallet = state.trader.wallet_pubkey();
    let (open_mints, open_accounts) = state.strategy_repo.open_real_rent_guards(&wallet).await?;
    Ok(classify_rent(&accounts, &open_mints, &open_accounts).status)
}

fn skip_inflight(
    state: &DeployState,
    accounts: Vec<OwnedTokenAccount>,
) -> (Vec<OwnedTokenAccount>, u32) {
    let mut kept = Vec::with_capacity(accounts.len());
    let mut skipped = 0u32;
    for account in accounts {
        if state.inflight.exit_mint_held(&account.mint.to_string()) {
            skipped = skipped.saturating_add(1);
        } else {
            kept.push(account);
        }
    }
    (kept, skipped)
}

fn merge_report(into: &mut ReclaimSendReport, extra: ReclaimSendReport) {
    into.closed = into.closed.saturating_add(extra.closed);
    into.closed_lamports = into.closed_lamports.saturating_add(extra.closed_lamports);
    into.errors.extend(extra.errors);
}

#[cfg(test)]
mod tests {
    use super::*;
    use pump_trader::protocol::{TOKEN, TOKEN_2022};
    use solana_sdk::pubkey::Pubkey;

    fn account(
        pubkey: &str,
        mint: Pubkey,
        program: Pubkey,
        amount: u64,
        lamports: u64,
    ) -> OwnedTokenAccount {
        OwnedTokenAccount {
            pubkey: Pubkey::from_str(pubkey).unwrap(),
            mint,
            program_id: program,
            amount,
            lamports,
            frozen: false,
            withheld: 0,
        }
    }

    const A1: &str = "bByaS6oD5FaWJGQuH467VuhUqiCq6iuBXqaxBQMuryY";
    const A2: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
    const A3: &str = "Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1";

    fn mint(byte: u8) -> Pubkey {
        let mut b = [0u8; 32];
        b[0] = byte;
        Pubkey::new_from_array(b)
    }

    #[test]
    fn empty_accounts_split_by_program_and_sum_lamports() {
        let accounts = vec![
            account(A1, mint(1), TOKEN_2022, 0, 2_100_000),
            account(A3, mint(2), TOKEN, 0, 2_039_280),
        ];
        let c = classify_rent(&accounts, &HashSet::new(), &HashSet::new());
        assert_eq!(c.status.empty.accounts, 2);
        assert_eq!(c.status.empty.lamports, 4_139_280);
        assert_eq!(c.status.empty_token2022, 1);
        assert_eq!(c.status.empty_legacy, 1);
        assert_eq!(c.close.len(), 2);
        assert!(c.burn.is_empty());
    }

    #[test]
    fn an_open_position_account_stays_and_an_orphan_on_that_mint_closes() {
        let m = mint(3);
        let accounts = vec![
            account(A1, m, TOKEN_2022, 0, 2_000_000),
            account(A3, m, TOKEN_2022, 0, 2_000_000),
        ];
        let mut open_accounts = HashSet::new();
        open_accounts.insert(A1.to_string());
        let mut open_mints = HashSet::new();
        open_mints.insert(m.to_string());
        let c = classify_rent(&accounts, &open_mints, &open_accounts);
        assert_eq!(c.close.len(), 1);
        assert_eq!(c.close[0].pubkey.to_string(), A3);
        assert_eq!(c.status.open_position.accounts, 1);
        assert_eq!(c.status.multi_account_mints, 1);
    }

    #[test]
    fn dust_burns_unless_the_mint_is_open_or_the_balance_is_a_real_bag() {
        let accounts = vec![
            account(A1, mint(4), TOKEN, DUST_RAW_MAX, 2_000_000),
            account(A3, mint(5), TOKEN, DUST_RAW_MAX, 2_000_000),
            account(A2, mint(6), TOKEN, DUST_RAW_MAX + 1, 2_000_000),
        ];
        let mut open_mints = HashSet::new();
        open_mints.insert(mint(5).to_string());
        let c = classify_rent(&accounts, &open_mints, &HashSet::new());
        assert_eq!(c.burn.len(), 1);
        assert_eq!(c.burn[0].mint, mint(4));
        assert_eq!(c.status.dust.accounts, 1);
        assert_eq!(c.status.open_position.accounts, 1);
        assert_eq!(c.status.held, 1);
    }

    #[test]
    fn frozen_withheld_usdc_and_wsol_go_to_their_own_buckets() {
        let mut frozen = account(A1, mint(7), TOKEN, 0, 2_000_000);
        frozen.frozen = true;
        let mut withheld = account(A3, mint(8), TOKEN_2022, 0, 2_000_000);
        withheld.withheld = 3;
        let usdc = Pubkey::from_str(trading_core::config::constants::USDC_MINT).unwrap();
        let accounts = vec![
            frozen,
            withheld,
            account(A2, usdc, TOKEN, 1, 2_000_000),
            account(
                "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
                WSOL_MINT,
                TOKEN,
                50_000,
                2_050_000,
            ),
        ];
        let c = classify_rent(&accounts, &HashSet::new(), &HashSet::new());
        assert_eq!(c.status.blocked.accounts, 2);
        assert_eq!(c.status.held, 1, "USDC is not dust");
        assert_eq!(c.status.wrapped.accounts, 1);
        assert_eq!(c.status.wrapped.lamports, 2_050_000);
        assert_eq!(c.close.len(), 1);
        assert!(c.close[0].mint == WSOL_MINT);
        assert!(c.burn.is_empty());
    }
}
