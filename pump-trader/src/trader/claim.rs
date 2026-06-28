// ============================================================
// Cashback claim — read-only status (NOT on the trade hot path).
//
// Accrued pump.fun cashback lives in the per-wallet `UserVolumeAccumulator`
// (UVA). There are TWO pots, one per program (different PDAs, same seeds):
//   curve : [b"user_volume_accumulator", wallet] vs pump_program
//   amm   : [b"user_volume_accumulator", wallet] vs pump_swap_program
//
// Claimable WSOL = cashback_earned - total_cashback_claimed (lamports). The
// curve-side account additionally carries a `stable_*` pot (separate mint).
// Layouts pulled from pump-fun/pump-public-docs IDLs.
//
// This module currently exposes only the read-only status read; the send path
// (sync + claim_cashback + unwrap) is built separately and stays off the hot
// path (recent blockhash, no nonce, no Jito tip).
// ============================================================

use super::PumpFunTrader;
use crate::error::{Context, Result, TradeError};
use crate::protocol::{self, CLAIM_CASHBACK_DISC, CLAIM_CASHBACK_V2_DISC, SYNC_UVA_DISC};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
};
use spl_associated_token_account::{
    get_associated_token_address_with_program_id,
    instruction::create_associated_token_account_idempotent,
};

/// Per-pot cashback status read from a `UserVolumeAccumulator` account.
#[derive(Debug, Clone, Copy)]
pub struct PotStatus {
    /// Human label for the owning program ("curve" / "amm").
    pub label: &'static str,
    pub pda: Pubkey,
    /// False if the UVA account doesn't exist yet (never traded a cashback coin
    /// on this venue) — treated as all-zero.
    pub exists: bool,
    pub cashback_earned: u64,
    pub total_cashback_claimed: u64,
    /// Curve-only "stable" cashback pot (separate mint). Zero on the AMM side.
    pub stable_earned: u64,
    pub stable_claimed: u64,
}

impl PotStatus {
    /// Claimable WSOL cashback (lamports).
    pub fn claimable(&self) -> u64 {
        self.cashback_earned.saturating_sub(self.total_cashback_claimed)
    }
    /// Claimable stable cashback (raw units of the stable mint).
    pub fn stable_claimable(&self) -> u64 {
        self.stable_earned.saturating_sub(self.stable_claimed)
    }
}

// Field offsets into UserVolumeAccumulator account data (8-byte Anchor
// discriminator + fields, little-endian). See IDL struct in cashback-claim-plan.md.
//   8  user(32) -> 40
//   40 needs_claim(1) -> 41
//   41 total_unclaimed_tokens(8) -> 49
//   49 total_claimed_tokens(8) -> 57
//   57 current_sol_volume(8) -> 65
//   65 last_update_timestamp(8) -> 73
//   73 has_total_claimed_tokens(1) -> 74
//   74 cashback_earned(8) -> 82
//   82 total_cashback_claimed(8) -> 90
//   90 stable_cashback_earned(8) -> 98   (curve only)
//   98 total_stable_cashback_claimed(8) -> 106 (curve only)
const OFF_CASHBACK_EARNED: usize = 74;
const OFF_CASHBACK_CLAIMED: usize = 82;
const OFF_STABLE_EARNED: usize = 90;
const OFF_STABLE_CLAIMED: usize = 98;

fn read_u64(data: &[u8], off: usize) -> u64 {
    data.get(off..off + 8)
        .and_then(|s| <[u8; 8]>::try_from(s).ok())
        .map(u64::from_le_bytes)
        .unwrap_or(0)
}

impl PumpFunTrader {
    /// Read both cashback pots (curve = pump_program UVA, amm = pump_swap UVA)
    /// and report claimable amounts. Read-only — two `getAccountInfo` calls, no
    /// transaction. A missing account (never traded a cashback coin on that
    /// venue) reports as all-zero, not an error.
    pub async fn cashback_status(&self) -> Result<Vec<PotStatus>> {
        let global = self
            .global_account
            .as_ref()
            .ok_or(TradeError::NotInitialized)?;

        let pots = [
            ("curve", global.user_volume_accumulator),
            ("amm", self.amm_user_volume_accumulator),
        ];

        let mut out = Vec::with_capacity(pots.len());
        for (label, pda) in pots {
            // get_account errors if the account doesn't exist; treat that as an
            // empty pot rather than failing the whole status read.
            let status = match self.rpc.get_account(&pda).await {
                Ok(acct) => PotStatus {
                    label,
                    pda,
                    exists: true,
                    cashback_earned: read_u64(&acct.data, OFF_CASHBACK_EARNED),
                    total_cashback_claimed: read_u64(&acct.data, OFF_CASHBACK_CLAIMED),
                    stable_earned: read_u64(&acct.data, OFF_STABLE_EARNED),
                    stable_claimed: read_u64(&acct.data, OFF_STABLE_CLAIMED),
                },
                Err(_) => PotStatus {
                    label,
                    pda,
                    exists: false,
                    cashback_earned: 0,
                    total_cashback_claimed: 0,
                    stable_earned: 0,
                    stable_claimed: 0,
                },
            };
            out.push(status);
        }
        Ok(out)
    }
}

/// Static per-pot facts needed to build one pot's claim transaction.
#[derive(Clone)]
struct PotClaim {
    label: &'static str,
    /// Owning program (pump for curve, pump_swap for AMM).
    program: Pubkey,
    user_volume_accumulator: Pubkey,
    /// `global_volume_accumulator` for that program (sync reads it).
    global_volume_accumulator: Pubkey,
    /// `__event_authority` PDA of the owning program.
    event_authority: Pubkey,
    claim_disc: [u8; 8],
    /// True for the pump `claim_cashback_v2` layout (adds the
    /// associated-token-program account that the AMM `claim_cashback` omits).
    include_atoken_program: bool,
    /// Quote the claim pays out: WSOL for the two SOL pots, the stable mint for
    /// the curve stable pot. `claim_cashback_v2` selects which on-chain pot
    /// (`cashback_earned` vs `stable_cashback_earned`) to drain from this mint.
    quote_mint: Pubkey,
    /// Token program owning `quote_mint` (classic SPL for WSOL; read on-chain for
    /// the stable mint, which may be SPL or Token-2022).
    quote_token_program: Pubkey,
    /// True only for WSOL: append a `close_account` so the claimed WSOL unwraps
    /// to native SOL. The stable mint is a real token — left as an SPL balance.
    unwrap: bool,
    /// True for the stable pot (its claimable is `stable_*`, in raw mint units,
    /// not lamports) so callers can report it apart from the SOL pots.
    is_stable: bool,
}

/// Outcome of one pot's claim — simulation result or, with `execute`, the sent
/// signature.
#[derive(Debug, Clone)]
pub struct ClaimOutcome {
    pub label: &'static str,
    pub claimable: u64,
    /// True for the stable pot — `claimable` is then raw stable-mint units, not
    /// lamports, and the claim lands as an SPL balance (no unwrap to SOL).
    pub is_stable: bool,
    /// True if this was simulate-only (no tx sent).
    pub simulated: bool,
    pub units_consumed: Option<u64>,
    /// `Some(msg)` if the (simulated or sent) tx failed; `None` on success.
    pub err: Option<String>,
    /// Sent signature when `execute` and the send succeeded.
    pub signature: Option<String>,
    pub logs: Vec<String>,
}

impl PumpFunTrader {
    /// Build the off-hot-path claim instruction set for one pot:
    ///   [create_idempotent(user WSOL ATA), sync, claim_cashback, close(unwrap)]
    /// The claim deposits WSOL into the user's WSOL ATA; the trailing
    /// `close_account` unwraps it to native SOL (and returns the just-paid ATA
    /// rent, so the create/close pair is rent-neutral). WSOL is a legacy-SPL
    /// mint, so `quote_token_program` is the classic token program throughout.
    /// Pure construction — no RPC, no signing — so the probe simulates the exact
    /// bytes a real claim sends.
    fn build_claim_ixs(&self, p: &PotClaim) -> Result<Vec<Instruction>> {
        let user = self.config.signer.pubkey();
        let token_prog = p.quote_token_program; // WSOL = legacy SPL; stable = read on-chain
        let mint = p.quote_mint;
        let atoken_program = protocol::ASSOCIATED_TOKEN_PROGRAM;

        // Both layouts move the quote from ATA(uva) -> ATA(user).
        let uva_quote_ata =
            get_associated_token_address_with_program_id(&p.user_volume_accumulator, &mint, &token_prog);
        let user_quote_ata = get_associated_token_address_with_program_id(&user, &mint, &token_prog);

        let mut ixs = Vec::with_capacity(4);

        // 1. Ensure the user's quote ATA exists (idempotent — no-op if present).
        ixs.push(create_associated_token_account_idempotent(
            &user, &user, &mint, &token_prog,
        ));

        // 2. sync_user_volume_accumulator — recompute the rolling window.
        ixs.push(Instruction {
            program_id: p.program,
            accounts: vec![
                AccountMeta::new_readonly(user, false),
                AccountMeta::new_readonly(p.global_volume_accumulator, false),
                AccountMeta::new(p.user_volume_accumulator, false),
                AccountMeta::new_readonly(p.event_authority, false),
                AccountMeta::new_readonly(p.program, false),
            ],
            data: SYNC_UVA_DISC.to_vec(),
        });

        // 3. claim_cashback — `user` is not a signer (permissionless); the
        // fee-payer signs the tx. The pump (v2) layout inserts the
        // associated-token-program account that the AMM layout omits.
        let mut claim_accounts = vec![
            AccountMeta::new(user, false),
            AccountMeta::new(p.user_volume_accumulator, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(token_prog, false),
        ];
        if p.include_atoken_program {
            claim_accounts.push(AccountMeta::new_readonly(atoken_program, false));
        }
        claim_accounts.push(AccountMeta::new(uva_quote_ata, false));
        claim_accounts.push(AccountMeta::new(user_quote_ata, false));
        claim_accounts.push(AccountMeta::new_readonly(self.system_program, false));
        claim_accounts.push(AccountMeta::new_readonly(p.event_authority, false));
        claim_accounts.push(AccountMeta::new_readonly(p.program, false));
        ixs.push(Instruction {
            program_id: p.program,
            accounts: claim_accounts,
            data: p.claim_disc.to_vec(),
        });

        // 4. WSOL only: close the user's WSOL ATA, returning WSOL (now native) +
        // the ATA rent to the wallet. The stable mint is a real token — its claim
        // stays as an SPL balance in the user's ATA, so no close here.
        if p.unwrap {
            ixs.push(spl_token::instruction::close_account(
                &token_prog,
                &user_quote_ata,
                &user,
                &user,
                &[],
            )?);
        }

        Ok(ixs)
    }

    /// The two WSOL pots, with their per-program PDAs and claim variant.
    fn claim_pots(&self) -> Result<[PotClaim; 2]> {
        let global = self
            .global_account
            .as_ref()
            .context("Not initialized")?;
        Ok([
            PotClaim {
                label: "curve",
                program: self.pump_program,
                user_volume_accumulator: global.user_volume_accumulator,
                global_volume_accumulator: global.global_volume_accumulator,
                event_authority: self.event_authority,
                // pump (curve) uses the v2 WSOL layout.
                claim_disc: CLAIM_CASHBACK_V2_DISC,
                include_atoken_program: true,
                quote_mint: self.wsol_mint,
                quote_token_program: self.token_program,
                unwrap: true,
                is_stable: false,
            },
            PotClaim {
                label: "amm",
                program: self.pump_swap_program,
                user_volume_accumulator: self.amm_user_volume_accumulator,
                global_volume_accumulator: self.amm_global_volume_accumulator,
                event_authority: self.amm_event_authority,
                claim_disc: CLAIM_CASHBACK_DISC,
                include_atoken_program: false,
                quote_mint: self.wsol_mint,
                quote_token_program: self.token_program,
                unwrap: true,
                is_stable: false,
            },
        ])
    }

    /// The curve-side stable cashback pot, if the program has a stable quote mint
    /// configured. Curve-only (the AMM UVA carries no stable pot) and built off
    /// the hot path — reads the stable mint's owning token program on-chain
    /// (`getAccount`), since a stable quote may be classic SPL or Token-2022.
    /// `Ok(None)` when no stable mint is configured or the read fails (the claim
    /// is then simply skipped, never an error).
    async fn stable_claim_pot(&self) -> Result<Option<PotClaim>> {
        let global = self.global_account.as_ref().context("Not initialized")?;
        let Some(stable_mint) = global.stable_quote_mint else {
            return Ok(None);
        };
        let Ok(mint_account) = self.rpc.get_account(&stable_mint).await else {
            return Ok(None);
        };
        Ok(Some(PotClaim {
            label: "curve-stable",
            program: self.pump_program,
            user_volume_accumulator: global.user_volume_accumulator,
            global_volume_accumulator: global.global_volume_accumulator,
            event_authority: self.event_authority,
            // Same v2 instruction as the curve WSOL claim — the quote_mint selects
            // the stable pot on-chain.
            claim_disc: CLAIM_CASHBACK_V2_DISC,
            include_atoken_program: true,
            quote_mint: stable_mint,
            quote_token_program: mint_account.owner,
            unwrap: false,
            is_stable: true,
        }))
    }

    /// Sweep accrued cashback from both pots back to the wallet as native SOL.
    /// OFF the trade hot path — recent blockhash, NO durable nonce, NO Jito tip,
    /// one tx per pot. Pots with zero claimable are skipped (no empty/reverting
    /// tx). `execute == false` simulates each tx and reports (claimable, CU,
    /// logs) without sending; `true` sends via RPC `sendTransaction` (preflight
    /// on) so a bad claim is rejected in simulation, not paid as a landed revert.
    pub async fn claim_cashback(&self, execute: bool) -> Result<Vec<ClaimOutcome>> {
        let status = self.cashback_status().await?;

        // Two WSOL pots (curve + amm), plus the curve stable pot when configured.
        let mut pots: Vec<PotClaim> = self.claim_pots()?.to_vec();
        if let Some(stable) = self.stable_claim_pot().await? {
            pots.push(stable);
        }

        let mut outcomes = Vec::new();
        for pot in &pots {
            // WSOL pots match their venue status by label; the stable pot draws
            // from the curve UVA's separate `stable_*` counters.
            let claimable = if pot.is_stable {
                status
                    .iter()
                    .find(|s| s.label == "curve")
                    .map(|s| s.stable_claimable())
                    .unwrap_or(0)
            } else {
                status
                    .iter()
                    .find(|s| s.label == pot.label)
                    .map(|s| s.claimable())
                    .unwrap_or(0)
            };
            if claimable == 0 {
                continue; // nothing to sweep on this pot
            }

            let ixs = self.build_claim_ixs(pot)?;
            let tx = self.build_recent_tx(ixs, self.config.signer.as_ref()).await?;

            let outcome = if execute {
                match self.rpc.send_transaction(&tx).await {
                    Ok(sig) => ClaimOutcome {
                        label: pot.label,
                        claimable,
                        is_stable: pot.is_stable,
                        simulated: false,
                        units_consumed: None,
                        err: None,
                        signature: Some(sig.to_string()),
                        logs: Vec::new(),
                    },
                    Err(e) => ClaimOutcome {
                        label: pot.label,
                        claimable,
                        is_stable: pot.is_stable,
                        simulated: false,
                        units_consumed: None,
                        err: Some(e.to_string()),
                        signature: None,
                        logs: Vec::new(),
                    },
                }
            } else {
                let (units_consumed, err, logs) = self.simulate_transaction(&tx).await?;
                ClaimOutcome {
                    label: pot.label,
                    claimable,
                    is_stable: pot.is_stable,
                    simulated: true,
                    units_consumed,
                    err,
                    signature: None,
                    logs,
                }
            };
            outcomes.push(outcome);
        }
        Ok(outcomes)
    }
}

#[cfg(test)]
mod tests {
    use crate::trader::{GlobalAccount, PumpFunTrader, TraderConfig};
    use solana_sdk::{message::Message, pubkey::Pubkey, signature::Keypair};
    use std::sync::Arc;

    const TX_LIMIT: usize = 1232;

    fn dummy_trader() -> PumpFunTrader {
        let mut t = PumpFunTrader::new(Arc::new(TraderConfig::new(
            "http://localhost".into(),
            vec!["http://localhost".into()],
            Arc::new(Keypair::new()),
            vec![Pubkey::new_unique()],
        )));
        t.global_account = Some(GlobalAccount {
            global_pda: Pubkey::new_unique(),
            fee_recipient: Pubkey::new_unique(),
            global_volume_accumulator: Pubkey::new_unique(),
            user_volume_accumulator: Pubkey::new_unique(),
            fee_config: Pubkey::new_unique(),
            stable_quote_mint: Some(Pubkey::new_unique()),
        });
        t
    }

    fn wire_size(msg: &Message) -> usize {
        1 + 64 * msg.header.num_required_signatures as usize + msg.serialize().len()
    }

    /// All claim variants — curve v2 (largest account list), AMM, and the curve
    /// stable pot (v2 layout, no unwrap) — build and, on a recent-blockhash
    /// message, stay under the 1232 B wire limit.
    #[test]
    fn claim_txs_fit() {
        let t = dummy_trader();
        let payer = t.config.signer.pubkey();
        // The two WSOL pots plus a synthetic stable pot (mirrors what
        // `stable_claim_pot` builds, minus the on-chain token-program read).
        let global = t.global_account.as_ref().expect("global");
        let mut pots: Vec<super::PotClaim> = t.claim_pots().expect("claim pots").to_vec();
        pots.push(super::PotClaim {
            label: "curve-stable",
            program: t.pump_program,
            user_volume_accumulator: global.user_volume_accumulator,
            global_volume_accumulator: global.global_volume_accumulator,
            event_authority: t.event_authority,
            claim_disc: crate::protocol::CLAIM_CASHBACK_V2_DISC,
            include_atoken_program: true,
            quote_mint: global.stable_quote_mint.expect("stable mint set in dummy"),
            quote_token_program: t.token_program,
            unwrap: false,
            is_stable: true,
        });
        for pot in pots {
            let ixs = t.build_claim_ixs(&pot).expect("build claim ixs");
            let msg = Message::new(&ixs, Some(&payer));
            let size = wire_size(&msg);
            eprintln!("claim [{}] = {size} B (limit {TX_LIMIT})", pot.label);
            assert!(size <= TX_LIMIT, "claim [{}] = {size} B over limit", pot.label);
        }
    }
}
