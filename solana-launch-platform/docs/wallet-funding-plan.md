# Wallet Funding Orchestration Plan

> **Status: IMPLEMENTED (2026-07-07).** Tier 1 shipped across P1–P4. Migration is
> `0008_wallet_funding.sql` (the `0005` referenced below was already taken by
> `metadata_templates`). `cargo check -p live` + `cargo test -p launcher -p
> platform-core` green (incl. the DB-gated claim-atomicity guard); `npm run build`
> green. Deferred Tier-2 `MultiHop` remains an unimplemented `FundingStrategy`.

Closes the gap left by the wallet-pool work (`wallet-pool-plan.md`, retired): the pool can
*generate* wallets, *detect* incoming SOL (balance poller → `generated→funded`), and *reclaim*
leftover SOL (dust sweep, pool→treasury), but nothing *sends* SOL **into** the pool. Funding is a
manual operator step today. This adds automated treasury→pool funding.

Decisions locked with operator (2026-07-07):
- **Obfuscation = Tier 1**: one transfer per tx, jittered amounts + timing, direct treasury→wallet.
  Multi-hop fan-out (Tier 2 / ADR D5) deferred behind a `FundingStrategy` seam.
- **Trigger = proactive top-up**: background task keeps ≥N `funded` wallets per role warm, plus a
  manual `POST /api/wallet_pool/fund` endpoint.

## Why "how you fund" matters (threat model)

The funded wallets (dev + bundler legs) all appear as buyers in the same launch, on-chain, within
seconds. A single tx funding many wallets, or uniform round amounts from one treasury in one slot,
clusters them into one entity for any rug-checker/sniper-detector. Tier 1 defeats *naive* clustering
(per-tx + jitter). Tier 2 hops defeat one more hop of correlation but not temporal/amount analysis;
strongest obfuscation (fresh CEX-withdrawal sources) stays a manual practice of seeding multiple
`role=treasury` wallets.

## State machine change

```
generated → funding → funded → reserved → used → retired
            ^^^^^^^ new
```

`funding` = SOL send in flight; the wallet is atomically claimed out of `generated` so a concurrent
funder / post-restart run can't double-fund it (real SOL loss). The existing balance poller flips
`funding→funded` when SOL lands (extend its promotion `CASE`). On send failure we revert
`funding→generated` (retryable).

## Work items

### P1 — DB + repo (migration `0005_wallet_funding.sql`)
- Extend `managed_wallets.status` CHECK to include `funding`.
- Extend the `idx_managed_wallets_generated` / poller partial indexes to also cover `funding` (poller
  must observe funding wallets to promote them).
- `ManagedWalletRepo::claim_for_funding(pool, role, count) -> Vec<ManagedWallet>`:
  `SELECT ... WHERE status='generated' AND role=$1 FOR UPDATE SKIP LOCKED LIMIT $2`,
  `UPDATE ... SET status='funding', funding_source=$src`. Mirrors `claim_funded` (own_launch.rs:145).
- `ManagedWalletRepo::revert_funding(pool, ids)`: `funding→generated`, guarded no-op otherwise
  (mirror `mark_used` shape).
- `record_balance` promotion `CASE`: promote to `funded` from **both** `generated` and `funding`.
- `funded_count(pool, role)` helper for the top-up target check.
- Guard test: `claim_for_funding` is atomic under concurrency; revert only touches `funding`.

### P2 — `launcher::wallet_funding` (invert `dust_sweep.rs`)
- Reuse plain `solana-client` `RpcClient` + `system_instruction::transfer` + `keystore::resolve_signer`
  (NOT pump-trader/Jito — funding isn't latency-critical).
- `FundingStrategy` trait seam: `plan_transfers(treasury, targets, cfg) -> Vec<Transfer>`. Tier-1 impl
  `DirectJittered` = one `Transfer` per target, amount jittered within `[amount*(1-j), amount*(1+j)]`,
  timing jittered by sleeping a random 0..max_delay between sends. Tier-2 `MultiHop` left unimplemented.
- `fund_once(pool, settings)`:
  1. Resolve the `role=treasury` wallet (`by_role`); no-op + log if absent (mirror dust_sweep:51).
  2. For each role that needs topping up: `n = target - funded_count(role)`; if `n<=0` skip.
  3. `claim_for_funding(role, n)` → targets.
  4. Safety gate (below). If fail, `revert_funding` and stop.
  5. Send per-strategy transfers, one tx each, plain `RpcClient::send_and_confirm_transaction`.
     On per-wallet failure: `revert_funding([id])`, continue.
  6. Poller promotes `funding→funded` on arrival; provenance already in `funding_source`.
- `spawn_wallet_funding(pool, settings)`: background loop, `FUND_INTERVAL`, gated on config presence
  (mirror `spawn_dust_sweep`). Wire in `crates/live/src/main.rs:81-89`.

### P3 — Safety rails (non-negotiable — autonomous real-SOL spend)
> **Superseded (2026-07-08) by `jit-funding-plan.md` for amounts + treasury sourcing:**
> the flat `FUND_AMOUNT_*` per-wallet amounts below are now the **warm-pool
> default only** — the JIT `fund_for_launch` path derives amounts per launch from
> the template (`FundPlan`), and both paths now draw from the **whole** treasury
> pool (`TreasuryPool`), not just the oldest `role=treasury` wallet. The reserve
> floor is applied **per treasury**.
- **Treasury reserve floor** `FUND_TREASURY_RESERVE_LAMPORTS`: never spend any one treasury below it.
- **Per-interval spend cap** `FUND_MAX_SPEND_PER_INTERVAL_LAMPORTS`: hard stop mid-batch when hit
  (checked against the **aggregate** spend across all source treasuries).
- **Per-wallet amount** `FUND_AMOUNT_DEV_LAMPORTS` / `FUND_AMOUNT_BUNDLER_LAMPORTS` (warm-pool flat
  defaults: dev 0.05 SOL — funds the 0.02 SOL launch gate `service.rs::MIN_DEV_LAUNCH_LAMPORTS` plus
  dev-buy headroom; bundler 0.03 SOL = leg buy + Jito tip + fees). JIT overrides these per launch.
- **Jitter** `FUND_AMOUNT_JITTER_PCT`, `FUND_MAX_DELAY_MS`.
- **Top-up target** `FUND_TARGET_FUNDED_{DEV,BUNDLER}`.
- **Kill switch** `FUND_ENABLED` (default false) + **dry-run** `FUND_DRY_RUN` (log intended transfers,
  send nothing). All in `LauncherSettings::from_env` + `.env.example`.

### P4 — API + UI
- `POST /api/wallet_pool/fund { role?, count? }` (http.rs:180 pattern) → on-demand `fund_once` scoped
  to the request. Best-effort, returns per-wallet outcome.
- `WalletPool.tsx`: "Fund pool" button + wire the low-pool banner to it; show `funding` status count.

## Verify
- `cargo check --workspace` + `cargo test -p platform-core` (repo guard tests) in a real dev env
  (this sandbox has no MSVC linker — cannot compile here; build against proven patterns).
- Devnet end-to-end: generate → fund (dry-run first, then live) → confirm `funding→funded` promotion +
  `funding_source` populated → launch claims from warm pool.

## Deferred (Tier 2 / later)
- `MultiHop` FundingStrategy: treasury → ephemeral hop wallets (retired after) → pool wallets.
- Fingerprint/timing picker UI, KMS KEK (carried over from wallet-pool Phase 5+).
