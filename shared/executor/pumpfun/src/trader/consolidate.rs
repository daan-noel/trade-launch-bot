// ============================================================
// Token-account consolidation — OFF the hot path, operator-invoked.
//
// Solana can leave a wallet holding several token accounts for one mint: the
// snipe buy path mints non-canonical accounts via the create-with-seed
// template pool, so a wallet that both snipes and buys into the real ATA
// accumulates one account per source. A "sell all" that drained only the
// first account silently orphaned funds in the rest.
//
// `consolidate_token_accounts` sweeps every OTHER account's balance into one
// destination and closes each drained orphan (rent reclaim), so a subsequent
// buy/sell deals with exactly one account. The happy path — the wallet
// already holds only the destination — is a single enumeration RPC and zero
// writes.
//
// **The destination is the caller's choice and it matters.** `into: None`
// means the canonical ATA, which is right for a wallet whose buys target the
// ATA (forge's `buy_token`). It is WRONG for hunter, whose every buy — bot and
// manual alike — goes through `buy_token_snipe_write_ahead` into a seeded
// template account: consolidating there would move a live bag into an account
// no position references, and the position's exit would then find nothing to
// sell. A hunter caller must pass `into: Some(account_the_position_holds)`.
// ============================================================

use executor_core::SimOutcome;
use super::PumpFunTrader;
use crate::error::{Context, Result};
use crate::types::TokenProgram;
use solana_sdk::{instruction::Instruction, pubkey::Pubkey};
use spl_associated_token_account::{
    get_associated_token_address_with_program_id,
    instruction::create_associated_token_account_idempotent,
};
use std::str::FromStr;
use tracing::{info, warn};

/// One orphan account slated for consolidation: its pubkey, current balance, and
/// the `[create_dest_ata_idempotent?, transfer_checked?, close_orphan]` ix set that
/// folds it into the destination account and reclaims its rent.
pub struct OrphanPlan {
    pub orphan: Pubkey,
    pub balance: u64,
    pub ixs: Vec<Instruction>,
}

impl PumpFunTrader {
    /// Consolidate every token account for `mint` OTHER than `into` — balance
    /// transferred, account closed for rent. `into: None` = the wallet's canonical
    /// ATA (created idempotently if missing); `into: Some(pk)` = that exact
    /// **already-existing** account. See the module header for why the choice is
    /// load-bearing, not cosmetic.
    ///
    /// Happy path (nothing but the destination, or no accounts at all): a single
    /// enumeration RPC and an early return — zero writes, zero cost.
    ///
    /// Otherwise, for each orphan: a recent-blockhash tx of
    /// `[create_dest_ata_idempotent?, transfer_checked(full)?, close_orphan]` is
    /// sent and confirmed before moving on. One tx per orphan keeps each
    /// transfer+close atomic — a failure can never strand a half-moved balance —
    /// and the idempotent create makes re-runs safe. An error on any orphan aborts
    /// the run and is surfaced; already-consolidated orphans stay consolidated.
    ///
    /// `token_program` is the mint's SPL program (from `resolve_buy_routing`), so
    /// the right transfer/close/ATA convention is used for classic Token vs
    /// Token-2022.
    pub async fn consolidate_token_accounts(
        &self,
        mint: &str,
        token_program: TokenProgram,
        into: Option<Pubkey>,
    ) -> Result<()> {
        let plans = self.plan_consolidation(mint, token_program, into).await?;
        if plans.is_empty() {
            // Happy path: nothing but the destination account (or nothing at all).
            return Ok(());
        }
        info!(
            mint = %mint, orphans = plans.len(), dest = ?into,
            "consolidating orphan token accounts into the destination account"
        );

        for plan in plans {
            let orphan_str = plan.orphan.to_string();
            // Recent-blockhash, preflight on (default) — a still-funded close fails
            // cheaply in simulation. Confirm before the next orphan / the buy so the
            // wallet state is settled.
            let tx = self
                .build_recent_tx(plan.ixs, self.config.signer.as_ref())
                .await?;
            let sig = self
                .rpc
                .send_transaction(&tx)
                .await
                .context("consolidation sendTransaction")?
                .to_string();
            if let Err(e) =
                self.confirm_transaction(&sig, self.config.retry.confirm_max_retries).await
            {
                warn!(
                    mint = %mint, account = %orphan_str,
                    "consolidation tx sent but not confirmed: {e}"
                );
                return Err(e);
            }
            info!(mint = %mint, account = %orphan_str, balance = plan.balance, sig = %sig,
                "orphan account consolidated + closed");
        }

        Ok(())
    }

    /// Build the per-orphan consolidation plans for `mint` WITHOUT sending anything:
    /// enumerate the wallet's accounts, resolve the destination, and for each orphan
    /// assemble the exact `[create_ata_idempotent?, transfer_checked?, close]` ix set
    /// [`consolidate_token_accounts`] would send. Empty = happy path (only the
    /// destination, or no accounts). Shared by the live consolidation and the
    /// zero-SOL dry-run so what's simulated is what production sends.
    pub async fn plan_consolidation(
        &self,
        mint: &str,
        token_program: TokenProgram,
        into: Option<Pubkey>,
    ) -> Result<Vec<OrphanPlan>> {
        let mint_pk = Pubkey::from_str(mint)?;
        let owner = self.config.signer.pubkey();
        let token_program_pk = token_program.pubkey();
        // A caller-named destination already exists (it is holding a bag); only the
        // canonical ATA may need the idempotent create prefix.
        let ensure_dest = into.is_none();
        let dest = into.unwrap_or_else(|| {
            get_associated_token_address_with_program_id(&owner, &mint_pk, &token_program_pk)
        });

        // Enumerate every account the wallet holds for this mint.
        let accounts = self.get_all_token_accounts_for_mint(mint).await?;

        // Orphans = accounts that are NOT the destination. (A zero-balance orphan is
        // still closed below to reclaim its rent.)
        let orphans: Vec<(Pubkey, u64)> =
            accounts.into_iter().filter(|(pk, _)| *pk != dest).collect();
        if orphans.is_empty() {
            return Ok(Vec::new());
        }

        // `transfer_checked` (both programs — the modern, Token-2022-safe convention)
        // needs the mint decimals. Read them once from the mint account.
        let decimals = self.mint_decimals(&mint_pk).await?;

        let mut plans = Vec::with_capacity(orphans.len());
        for (orphan, balance) in orphans {
            let ixs = build_orphan_ixs(
                &owner,
                &mint_pk,
                &token_program_pk,
                token_program,
                &dest,
                ensure_dest,
                &orphan,
                balance,
                decimals,
            )?;
            plans.push(OrphanPlan { orphan, balance, ixs });
        }
        Ok(plans)
    }

    /// Zero-SOL dry-run of consolidation: build the same per-orphan plans as the live
    /// path and run each through `simulate_ixs` against live chain state instead of
    /// sending. Returns `(orphan, balance, SimOutcome)` per orphan so a caller can
    /// confirm every transfer+close would succeed before risking a real run. No
    /// signing key spend, no tx submitted.
    pub async fn simulate_consolidation(
        &self,
        mint: &str,
        token_program: TokenProgram,
        into: Option<Pubkey>,
    ) -> Result<Vec<(Pubkey, u64, SimOutcome)>> {
        let plans = self.plan_consolidation(mint, token_program, into).await?;
        let payer = self.config.signer.pubkey();
        let mut out = Vec::with_capacity(plans.len());
        for plan in plans {
            // Track the orphan + payer so the sim reports the token drain + rent return.
            let outcome = self
                .simulate_ixs(plan.ixs, &payer, &[plan.orphan, payer])
                .await?;
            out.push((plan.orphan, plan.balance, outcome));
        }
        Ok(out)
    }

    /// Read a mint's `decimals` from its on-chain account. Both classic Token and
    /// Token-2022 store `decimals` at byte offset 44 of the base mint layout
    /// (`mint_authority(36) + supply(8)`), so a single account read covers both —
    /// no program-specific unpack. Off the hot path (consolidation only).
    async fn mint_decimals(&self, mint: &Pubkey) -> Result<u8> {
        let acct = self.rpc.get_account(mint).await?;
        const DECIMALS_OFFSET: usize = 44;
        acct.data
            .get(DECIMALS_OFFSET)
            .copied()
            .context("mint account too short to read decimals")
    }
}

/// The `[create_dest_ata_idempotent?, transfer_checked?, close_orphan]` ix set for
/// one orphan. Pure (no RPC, no signing) so it's unit-testable and shared by the
/// live consolidation and the dry-run. A zero-balance orphan skips the transfer (it's
/// only closed for rent). Routed legacy vs Token-2022 by `token_program`.
///
/// `ensure_dest` prefixes the idempotent create-ATA — correct **only** when `dest`
/// IS the canonical ATA. For a caller-named destination it is skipped: the create ix
/// derives the ATA from `owner`+`mint` and would mint an unrelated third account
/// (paying its rent) rather than the destination actually being transferred into.
#[allow(clippy::too_many_arguments)]
fn build_orphan_ixs(
    owner: &Pubkey,
    mint: &Pubkey,
    token_program_pk: &Pubkey,
    token_program: TokenProgram,
    dest: &Pubkey,
    ensure_dest: bool,
    orphan: &Pubkey,
    balance: u64,
    decimals: u8,
) -> Result<Vec<Instruction>> {
    let mut ixs: Vec<Instruction> = Vec::with_capacity(3);

    if ensure_dest {
        // Ensure the canonical ATA exists as the transfer destination. Idempotent so
        // re-runs (and the common case where it already exists) are no-ops.
        ixs.push(create_associated_token_account_idempotent(
            owner,
            owner,
            mint,
            token_program_pk,
        ));
    }

    if balance > 0 {
        let transfer_ix = match token_program {
            TokenProgram::Legacy => spl_token::instruction::transfer_checked(
                token_program_pk, orphan, mint, dest, owner, &[], balance, decimals,
            )?,
            TokenProgram::Token2022 => spl_token_2022::instruction::transfer_checked(
                token_program_pk, orphan, mint, dest, owner, &[], balance, decimals,
            )?,
        };
        ixs.push(transfer_ix);
    }

    let close_ix = match token_program {
        TokenProgram::Legacy => {
            spl_token::instruction::close_account(token_program_pk, orphan, owner, owner, &[])?
        }
        TokenProgram::Token2022 => {
            spl_token_2022::instruction::close_account(token_program_pk, orphan, owner, owner, &[])?
        }
    };
    ixs.push(close_ix);

    Ok(ixs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pk(b: u8) -> Pubkey {
        Pubkey::new_from_array([b; 32])
    }

    #[test]
    fn funded_orphan_builds_create_transfer_close() {
        let owner = pk(1);
        let mint = pk(2);
        let canonical = pk(3);
        let orphan = pk(4);
        let tp = spl_token::id();
        let ixs = build_orphan_ixs(
            &owner, &mint, &tp, TokenProgram::Legacy, &canonical, true, &orphan, 1_000, 6,
        )
        .unwrap();
        // create-ATA (ATA program) + transfer_checked (token program) + close (token program).
        assert_eq!(ixs.len(), 3, "funded orphan = create + transfer + close");
        assert_eq!(ixs[0].program_id, spl_associated_token_account::id());
        assert_eq!(ixs[1].program_id, tp, "transfer runs on the token program");
        assert_eq!(ixs[2].program_id, tp, "close runs on the token program");
        // transfer_checked: source=orphan, dest=canonical present in account metas.
        let metas: Vec<Pubkey> = ixs[1].accounts.iter().map(|m| m.pubkey).collect();
        assert!(metas.contains(&orphan), "transfer source is the orphan");
        assert!(metas.contains(&canonical), "transfer dest is the canonical ATA");
    }

    #[test]
    fn empty_orphan_skips_transfer() {
        let ixs = build_orphan_ixs(
            &pk(1), &pk(2), &spl_token::id(), TokenProgram::Legacy, &pk(3), true, &pk(4), 0, 6,
        )
        .unwrap();
        // Zero balance: only create-ATA + close (no transfer of 0).
        assert_eq!(ixs.len(), 2, "empty orphan = create + close, no transfer");
        assert_eq!(ixs[1].program_id, spl_token::id(), "second ix is the close");
    }

    #[test]
    fn token_2022_routes_to_2022_program() {
        let tp = spl_token_2022::id();
        let ixs = build_orphan_ixs(
            &pk(1), &pk(2), &tp, TokenProgram::Token2022, &pk(3), true, &pk(4), 500, 9,
        )
        .unwrap();
        assert_eq!(ixs.len(), 3);
        assert_eq!(ixs[1].program_id, tp, "Token-2022 transfer routes to the 2022 program");
        assert_eq!(ixs[2].program_id, tp, "Token-2022 close routes to the 2022 program");
    }

    #[test]
    fn caller_named_dest_emits_no_create_ata() {
        let owner = pk(1);
        let mint = pk(2);
        // A seeded template account a hunter position holds — NOT the canonical ATA.
        let seeded_dest = pk(7);
        let orphan = pk(4);
        let tp = spl_token::id();
        let ixs = build_orphan_ixs(
            &owner, &mint, &tp, TokenProgram::Legacy, &seeded_dest, false, &orphan, 1_000, 6,
        )
        .unwrap();
        // transfer + close only: creating an ATA here would mint a THIRD account
        // (derived from owner+mint, not `seeded_dest`) and pay its rent for nothing.
        assert_eq!(ixs.len(), 2, "named destination = transfer + close, no create");
        assert!(
            ixs.iter().all(|i| i.program_id != spl_associated_token_account::id()),
            "no ATA-program ix when the destination was named by the caller"
        );
        let metas: Vec<Pubkey> = ixs[0].accounts.iter().map(|m| m.pubkey).collect();
        assert!(metas.contains(&seeded_dest), "transfer dest is the named account");
    }
}
