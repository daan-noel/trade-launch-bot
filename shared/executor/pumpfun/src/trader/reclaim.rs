// Batch rent reclaim. Off the trade hot path: a recent blockhash, no durable
// nonce, no Jito tip, preflight left on. One bad account must not sink the
// rest of its batch, so a failed chunk is retried one account at a time.

use super::PumpFunTrader;
use crate::error::{Context, Result};
use crate::types::{OwnedTokenAccount, ReclaimSendReport, TokenProgram};
use solana_sdk::pubkey::Pubkey;
use tracing::warn;

/// Closes per transaction. Each close adds one account key; 12 stays under the
/// 1232-byte limit with room for both token programs.
const CLOSE_BATCH: usize = 12;
/// Burn and close are two instructions, and the burn names the mint.
const BURN_BATCH: usize = 6;

impl PumpFunTrader {
    /// Close these accounts and confirm each batch. For a non-native mint the
    /// token balance has to be zero or the program reverts the whole transaction.
    /// Wrapped SOL is the exception: closing it unwraps the balance into the wallet.
    pub async fn close_token_accounts(
        &self,
        accounts: &[OwnedTokenAccount],
    ) -> Result<ReclaimSendReport> {
        self.reclaim_chunks(accounts, false, CLOSE_BATCH).await
    }

    /// Burn each account's token balance, then close it, in the same transaction.
    /// The caller decides which balances are small enough to destroy.
    pub async fn burn_and_close_token_accounts(
        &self,
        accounts: &[OwnedTokenAccount],
    ) -> Result<ReclaimSendReport> {
        self.reclaim_chunks(accounts, true, BURN_BATCH).await
    }

    async fn reclaim_chunks(
        &self,
        accounts: &[OwnedTokenAccount],
        burn: bool,
        batch: usize,
    ) -> Result<ReclaimSendReport> {
        let mut report = ReclaimSendReport::default();
        if accounts.is_empty() {
            return Ok(report);
        }
        let owner = self.config.signer.pubkey();
        for chunk in accounts.chunks(batch) {
            match self.send_reclaim_tx(chunk, burn, &owner).await {
                Ok(()) => {
                    report.closed += chunk.len();
                    report.closed_lamports =
                        report.closed_lamports.saturating_add(chunk_lamports(chunk));
                }
                Err(err) if chunk.len() == 1 => {
                    report.errors.push(format!("{}: {err}", chunk[0].pubkey));
                }
                Err(err) => {
                    warn!(
                        n = chunk.len(),
                        "rent batch failed, retrying one account at a time: {err}"
                    );
                    for account in chunk {
                        match self
                            .send_reclaim_tx(std::slice::from_ref(account), burn, &owner)
                            .await
                        {
                            Ok(()) => {
                                report.closed += 1;
                                report.closed_lamports =
                                    report.closed_lamports.saturating_add(account.lamports);
                            }
                            Err(err) => report.errors.push(format!("{}: {err}", account.pubkey)),
                        }
                    }
                }
            }
        }
        Ok(report)
    }

    async fn send_reclaim_tx(
        &self,
        accounts: &[OwnedTokenAccount],
        burn: bool,
        owner: &Pubkey,
    ) -> Result<()> {
        let mut ixs = Vec::with_capacity(accounts.len() * if burn { 2 } else { 1 });
        for account in accounts {
            if burn && account.amount > 0 {
                ixs.push(burn_ix(account, owner)?);
            }
            ixs.push(close_ix(account, owner)?);
        }
        let tx = self
            .build_recent_tx(ixs, self.config.signer.as_ref())
            .await?;
        self.rpc
            .send_and_confirm_transaction(&tx)
            .await
            .context("rent reclaim send")?;
        Ok(())
    }
}

fn chunk_lamports(accounts: &[OwnedTokenAccount]) -> u64 {
    accounts
        .iter()
        .fold(0u64, |sum, a| sum.saturating_add(a.lamports))
}

fn close_ix(
    account: &OwnedTokenAccount,
    owner: &Pubkey,
) -> Result<solana_sdk::instruction::Instruction> {
    let ix = match TokenProgram::from_pubkey(&account.program_id) {
        TokenProgram::Token2022 => spl_token_2022::instruction::close_account(
            &account.program_id,
            &account.pubkey,
            owner,
            owner,
            &[],
        )?,
        TokenProgram::Legacy => spl_token::instruction::close_account(
            &account.program_id,
            &account.pubkey,
            owner,
            owner,
            &[],
        )?,
    };
    Ok(ix)
}

fn burn_ix(
    account: &OwnedTokenAccount,
    owner: &Pubkey,
) -> Result<solana_sdk::instruction::Instruction> {
    let ix = match TokenProgram::from_pubkey(&account.program_id) {
        TokenProgram::Token2022 => spl_token_2022::instruction::burn(
            &account.program_id,
            &account.pubkey,
            &account.mint,
            owner,
            &[],
            account.amount,
        )?,
        TokenProgram::Legacy => spl_token::instruction::burn(
            &account.program_id,
            &account.pubkey,
            &account.mint,
            owner,
            &[],
            account.amount,
        )?,
    };
    Ok(ix)
}
