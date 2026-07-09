# Just-in-time, template-driven funding plan

> **Status: IMPLEMENTED (2026-07-08).** Sections A–D shipped. New:
> `launcher::funding_plan` (`FundPlan`, `dev_launch_required_lamports`,
> `leg_required_lamports`), `wallet_funding::fund_for_launch` +
> `TreasuryPool` multi-source spill (also fixes the flat `fund_once`/Fund button),
> `ManagedWalletRepo::claim_specific_for_funding`, `POST
> /api/wallet_pool/fund_for_launch`, and a "Fund for launch" step in the Launch
> Console. The `service.rs` launch gate now calls `dev_launch_required_lamports`
> (SSOT). `npm run build` green; Rust not compiled in this sandbox (no MSVC
> linker) — compile with `cargo check -p launcher -p live` before running.
> The background warm funder (Section D) was left wired but is now optional under
> pure JIT — set `FUND_TARGET_FUNDED_{DEV,BUNDLER}=0` to idle it without a code
> change.

Supersedes the flat warm-pool amounts in `wallet-funding-plan.md` P3. Two changes:

1. **Fund amounts are derived from the launch template per launch**, not fixed
   `FUND_AMOUNT_{DEV,BUNDLER}_LAMPORTS` env constants.
2. **All non-retired treasury wallets are a funding-source pool**, not just the
   oldest (`by_role(...).next()`).

## Why the current design is wrong for this use

- `role_plan` returns a flat env amount per role — but a launch's real need is
  `dev = MIN_DEV_LAUNCH_LAMPORTS + dev_buy_quote` and
  `leg = bundle_quote_per_leg + tip_share + fees`, both **template-specific**
  (`service.rs::PumpfunTemplateParams`). A flat amount over-funds (dust) or
  under-funds (launch `bail!`s the gate at `service.rs:149`).
- `fund_once` reads only the oldest treasury's balance + signs with its key
  (`wallet_funding.rs:177-186`). With SOL spread across several treasuries, the
  `under_reserve` rail trips at 0 spent even though funds exist — the observed
  "1 skipped (safety cap) — 0.0000 SOL spent".

## A. Template → FundPlan (single source of truth with the launch gate)

New in `launcher::service` (or a `funding_plan.rs`):

```rust
/// SSOT for the dev-wallet launch requirement. service.rs:149's gate MUST call
/// this instead of inlining `MIN_DEV_LAUNCH_LAMPORTS + dev_buy_quote`.
pub fn dev_launch_required_lamports(p: &PumpfunTemplateParams) -> u64 {
    MIN_DEV_LAUNCH_LAMPORTS + p.dev_buy_quote.unwrap_or(0).max(0) as u64
}

/// Per bundler leg: leg buy + its tip share + fee/rent headroom.
pub fn leg_required_lamports(quote_per_leg: i64, tip_quote: i64) -> u64 { ... }

pub struct FundPlan {
    pub dev_lamports: u64,      // dev_launch_required_lamports(+headroom)
    pub per_leg_lamports: u64,  // leg_required_lamports(+headroom)
    pub leg_count: u32,         // resolve_leg_count(bundler_count, params)
}
```

- Refactor `service.rs:149` to call `dev_launch_required_lamports` — kills the
  drift between the funder's target and the pre-launch gate (CLAUDE.md SSOT rule;
  guard with a unit test asserting `funded_amount >= gate`).
- `FUNDING_HEADROOM_LAMPORTS` const covers signature fees + the ±jitter downside
  so a topped-up wallet can't land under the gate. With JIT funding we can drop
  the amount jitter to 0 (obfuscation matters less than landing the exact need);
  keep only timing jitter if desired.
- Reuse `bundle::resolve_bundle_quote` for `(quote_per_leg, tip)` — no re-derivation.

## B. fund_once takes a plan, not env amounts

- Replace `role_plan`'s env amount with a caller-supplied amount. Add:

```rust
pub async fn fund_for_launch(
    pool, settings,
    template_id: Uuid,
    bundler_count: Option<u32>,
    dev_wallet_id: Uuid,      // the specific dev wallet this launch will use
    mode: FundMode,
) -> Result<FundReport>
```

  which: loads the template → `FundPlan`; **tops up the shortfall only** (read
  each target's current balance, transfer `need - balance`, skip if already ≥
  need); claims `leg_count` bundler wallets and funds each to `per_leg_lamports`;
  funds the named dev wallet to `dev_lamports`.
- Keep the pure `FundingStrategy::plan_transfers` seam; just feed it the
  per-target amount from the plan instead of a single role amount.
- The launch flow then funds → waits for `funded` promotion (balance poller) →
  `execute_launch` claims those same now-funded wallets. Since JIT correctness
  matters, use `FundMode::Background` semantics here (confirm each send) so a
  dropped tx doesn't leave a launch stuck.

## C. Treasury source pool (fixes the safety-cap bug)

Replace `.next()` with a multi-source spill in `fund_once`:

1. Load **all** non-retired treasuries + their live balances (one `get_balance`
   each, or `getMultipleAccounts`).
2. Build sources ordered by spendable (`balance - reserve`) desc; skip any at/below
   their reserve floor.
3. Per transfer, draw from the current source; when it hits its reserve floor,
   advance to the next. Sign each transfer with that source's key
   (`keystore::resolve_signer` per source, cached).
4. Safety check is now against **aggregate** spendable (`Σ(balance-reserve)`),
   and the per-pass `FUND_MAX_SPEND_PER_INTERVAL_LAMPORTS` cap still applies.
5. `record_balance` write-back applied per source treasury actually spent.

Reserve floor stays per-treasury (`FUND_TREASURY_RESERVE_LAMPORTS`, applied to
each). Treasury is no longer assumed singleton — update the `wallet_funding.rs`
module doc.

## D. Trigger / endpoint

- New `POST /api/wallet_pool/fund_for_launch { template_id, bundler_count?,
  dev_wallet_id }` → `fund_for_launch`, returns funded wallet ids + outcomes.
- Frontend Launch Console: a "Prepare / Fund for launch" step that runs this for
  the selected template before enabling "Launch", surfacing the per-wallet report.
- The old flat `POST /api/wallet_pool/fund` + background warm funder become
  redundant under pure JIT — disable the background `spawn_wallet_funding` flat
  top-up (or gate it off) so it doesn't fund wallets to a wrong flat amount.

## E. Left as-is / carve-outs

- `MIN_DEV_LAUNCH_LAMPORTS` (0.02) stays the create floor SSOT.
- Kill switch `FUND_ENABLED` + `FUND_DRY_RUN` unchanged.
- `claim_for_funding` / `revert_funding` atomicity unchanged (still per-wallet).

## Test / DoD

- Unit: `dev_launch_required_lamports` == gate; `FundPlan` from a template with
  dev-buy + N legs; funded amount ≥ gate after headroom.
- Unit: multi-treasury spill picks highest-spendable first, respects each reserve,
  aggregates the cap.
- `cargo check`/`clippy` on `launcher` + `live`; `npm run build`.
- Update `wallet-funding-plan.md` (mark P3 flat amounts superseded) + the
  `wallet_funding.rs` module doc (treasury no longer singleton).
