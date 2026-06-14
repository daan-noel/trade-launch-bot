# Cashback Claim (Option 2) — implementation plan

Sweep accrued pump.fun cashback (WSOL) back to the wallet. **Strictly off the
hot path** — never part of buy/sell. Run as a one-shot probe first; optionally a
low-frequency background sweep later.

## Source of truth (from on-chain IDLs)

Pulled from `pump-fun/pump-public-docs/idl/{pump,pump_amm}.json`.

### `UserVolumeAccumulator` account (claimable is readable directly)
```
user, needs_claim, total_unclaimed_tokens, total_claimed_tokens,
current_sol_volume, last_update_timestamp, has_total_claimed_tokens,
cashback_earned (u64), total_cashback_claimed (u64),
stable_cashback_earned (u64), total_stable_cashback_claimed (u64)  // pump (curve) only
```
- Anchor 8-byte discriminator prefix, then fields little-endian.
- **Claimable WSOL = `cashback_earned - total_cashback_claimed`** (lamports).
- Curve side also carries a `stable_*` pot (separate mint, likely USDC) — report it, claim path TBD.

### Two pots (different programs → different PDAs)
- Curve: `[b"user_volume_accumulator", wallet]` vs **pump_program** (`6EF8…`)
- AMM:   `[b"user_volume_accumulator", wallet]` vs **pump_swap_program** (`pAMM…`)

### Instructions (no args; `user` is NOT a signer — fee-payer signs; permissionless)
`sync_user_volume_accumulator`  disc `[86,31,192,87,163,87,79,238]`
```
user(r), global_volume_accumulator(r, [b"global_volume_accumulator"]),
user_volume_accumulator(w), event_authority(r, [b"__event_authority"]), program(r)
```
`claim_cashback` (AMM + pump; WSOL variant)  disc `[37,58,35,126,190,53,228,197]`
```
user(w),
user_volume_accumulator(w, [b"user_volume_accumulator", user]),
quote_mint(r)                 = WSOL,
quote_token_program(r)        = legacy SPL token,
user_volume_accumulator_wsol_token_account(w) = ATA(uva, token_prog, WSOL),
user_wsol_token_account(w)                     = ATA(user, token_prog, WSOL),
system_program(r),
event_authority(r, [b"__event_authority"]),
program(r)
```
`claim_cashback_v2` (pump only)  disc `[122,243,204,65,94,116,29,55]` — same idea, adds
`associated_token_program` + uses `associated_*` ATA names. Use AMM-style
`claim_cashback` for both programs first; fall back to v2 if the pump-program
claim sims revert.

> event_authority/program differ per program: derive `__event_authority` against
> the owning program and pass that program id as the trailing `program` account.

## Build steps

1. **constants.rs** — add the three discriminators + a comment block. WSOL mint
   already exists.
2. **New module `pump-trader/src/trader/claim.rs`:**
   - `read_uva(program, wallet) -> Option<UvaCashback>` — fetch+deserialize the
     UVA; return `{ claimable_wsol, stable_claimable }`. Read-only.
   - `cashback_status()` — read both pots, return per-pot claimable. Read-only.
   - `build_claim_ixs(program, event_authority)` — pure ix builder:
     `[sync, claim_cashback]`, parallel to `build_curve_sell_ixs`. No RPC/signing.
   - `claim_cashback()` — for each pot with `claimable > 0`: build ixs, wrap via
     `build_recent_tx` (recent blockhash, **no nonce, no Jito tip**), send via
     RPC `sendTransaction` (preflight on). Then unwrap: `close_account` on the
     user WSOL ATA → native SOL (reuse the close pattern in `sell.rs`). Skip
     empty pots so no reverting tx.
3. **Probe** — `probe.rs` + `main.rs`: `probe claim-cashback [--execute]`.
   - default = read status + **simulate** (build → `simulate_transaction` →
     report claimable per pot + CU + logs), mirroring `simulate-sell`.
   - `--execute` actually sends.
4. **Tests** (`-p pump-trader`): tx-size guard < 1232 B and account-layout
   assertion, like `curve_sell_with_nonce_fits`.
5. **Docs** — update `@docs/trade-execution.md` (new claim module + probe).

## Timing (answer to "every sell or periodic?")
Cashback is credited by the program automatically on every cashback trade — the
claim only *withdraws* the pile. Never per-sell (hot-path latency + per-claim
base fee on dust). Model = manual one-shot probe now; optional background sweep
later (hourly or when `claimable` crosses a threshold, e.g. 0.05 SOL).

## Open items
- Confirm pump-program (curve) claim uses `claim_cashback` vs `claim_cashback_v2`
  — resolve by simulating both, keep whichever passes.
- `stable_*` cashback (curve) — separate mint; report first, decide claim later.
- Whether the user WSOL ATA must pre-exist (AMM `claim_cashback` has no ATA-create
  account; pump v2 includes `associated_token_program`). If sim fails on a missing
  ATA, prepend an idempotent create-ATA ix.
