# Token Management Plan (post-launch)

Manage tokens **after** they launch: read holdings + PnL per wallet, sell (all / partial /
by group), buy more, consolidate back to treasury. Built as a thin orchestrator over the
**existing** SLP wallet pool, keystore, funding, and confirm machinery — and over
`pump-trader`'s ready-made buy/sell executor. No second trade engine.

## Decisions locked (2026-07-08)

- **Manual-first.** Every action is operator-driven: `plan → preview → execute`. No background
  schedulers fire trades in phase 1. Automation (armed ladders, volume loops) is a later phase
  built on the same primitives.
- **Holdings = dedicated table** (`token_positions`, migration `0010`), seeded from launch/bundle
  fills, reconciled against chain. Carries cost basis → real per-wallet PnL. "Can't manage what
  you can't see" comes first.
- **Ladders = simple thresholds authored in SLP** (sell X% at a price / market-cap milestone).
  Lightweight rules, **no** dependency on meme-trading's `tpsl_rules_core`. Deferred to Phase 4;
  phase 1–3 ship manual partial sells only.
- **Volume-making = deferred.** Design a clean seam now (`ManageAction::Buy` primitive + fresh-wallet
  selection), implement the loop scheduler later. Out of phase-1 scope.

## Domain model

Three primitives over a set of controlled wallets. Everything the operator wants is a **policy**
that produces an `ActionPlan` (a list of per-wallet legs) from one primitive + a wallet selection +
a sizing mode:

| Primitive | Sizing modes | Wallet scope |
| --- | --- | --- |
| **Sell** | `pct_of_holdings`, `to_sol_target`, `fixed_base` | dev · bundlers · subset · all |
| **Buy** | `fixed_sol`, `fixed_base` | dev · bundlers · fresh (pool) |
| **Consolidate** | full-balance sweep (tokens and/or SOL) → treasury | used/any → treasury |

`ActionPlan` = `Vec<PlanLeg { wallet_id, side, size_base_or_lamports, est_out }>`, computed from
current `token_positions` + live price. **Always previewed before execution** (free dry-run + audit
trail). Execution fans the legs out with bounded concurrency, each leg feed-confirmed against
`trades` exactly like `confirm.rs` does for buys.

## What we reuse (do NOT rebuild)

| Need | Existing asset |
| --- | --- |
| Sign a per-wallet tx | `launcher::keystore::resolve_signer` → `Arc<dyn Signer>` |
| Buy/sell executor | `pump_trader::PumpFunTrader::{buy_token, sell_token_once}` (meme-trading path-dep) |
| Trader construction/config | `launcher::trader_config::build_launch_trader_config` (+ a manage-tuned variant) |
| Confirm fills without RPC | `launcher::confirm` pattern + `TradeRepo::find_signatures_present` |
| SOL balance polling | `wallet_pool::spawn_balance_poller` / `fetch_balances` / `record_balance` |
| SOL consolidation pattern | `dust_sweep.rs` (fee-aware sweep → treasury → retire) |
| Fresh wallets + funding | `ManagedWalletRepo` (`claim_funded`) + `wallet_funding` |
| Cost basis | `launches.dev_buy_quote` + `bundles.legs` JSONB + actual `trades` by signature/wallet_id |
| Enum+CHECK+roundtrip pattern | `models/status.rs` |

## Data model — migration `0010`

New table `token_positions` (per wallet × token). One row per (wallet, mint) we hold or held.

```
token_positions
  id              uuid pk
  mint_address    text  -> tokens(mint_address)
  wallet_id       uuid  -> managed_wallets(id)
  role            text  CHECK(dev|bundler|treasury|trading)   -- denormalized for group selects
  token_account   text                                        -- canonical ATA, restart-safe reuse
  balance_base    bigint  not null default 0                  -- current tokens held (exact base units)
  cost_quote      bigint  not null default 0                  -- lamports spent acquiring (cost basis)
  realized_quote  bigint  not null default 0                  -- lamports recovered from sells
  balance_checked_at  timestamptz
  status          text  CHECK(open|closed)                    -- closed when balance_base -> 0
  created_at / updated_at
  UNIQUE(mint_address, wallet_id)
```

- **Seed** on launch/bundle confirm: dev row from `launches.dev_buy_quote`; bundler rows from
  `bundles.legs`. `cost_quote` = planned/actual quote spent; `balance_base` = tokens received
  (from the matching `trades` rows by leg signature).
- **Reconcile** via a balance poller (RPC `getTokenAccountsByOwner` / batched account reads),
  writing `balance_base` + `balance_checked_at` — mirrors the SOL poller.
- **PnL** (derived, never stored): `unrealized = balance_base * price_quote`,
  `pnl = realized_quote + unrealized - cost_quote`. Prices come from `token_market_state` /
  `trades_priced` — decimals/USD applied in views only, per the SSOT rule.

New action-log table `manage_actions` (audit + idempotency of every executed plan):

```
manage_actions
  id              uuid pk
  mint_address    text
  kind            text  CHECK(sell|buy|consolidate)
  sizing          text  CHECK(pct_of_holdings|to_sol_target|fixed_base|fixed_sol|sweep)
  selection       jsonb                       -- {role?, wallet_ids?}
  plan            jsonb                        -- the previewed PlanLeg[]
  status          text  CHECK(planned|executing|completed|partial|failed)
  legs_confirmed  int   default 0
  created_at / completed_at
```

Add both `status`/`kind`/`sizing`/`role` vocabularies as Rust enums in `models/status.rs`,
each `as_str()` matching the SQL CHECK, with a roundtrip test (project convention).

## Backend structure (mirror handler→service→repo)

```
crates/launcher/src/manage/
  mod.rs
  model.rs         ManageAction, Sizing, WalletSelection, ActionPlan, PlanLeg
  selection.rs     resolve a WalletSelection -> Vec<ManagedWallet> (role | ids | fresh-from-pool)
  positions.rs     seed_from_launch/bundle, reconcile balances, pnl()
  plan.rs          build ActionPlan from (primitive + selection + sizing + live positions)
  execute.rs       fan legs out over pump-trader, feed-confirm, write positions + manage_actions
  sell.rs          sell primitive (pct / to_sol_target / fixed_base)
  buy.rs           buy primitive (fixed_sol / fixed_base)  -- fresh-wallet capable
  consolidate.rs   generalize dust_sweep to tokens+SOL -> treasury
  poller.rs        spawn_position_poller (token-balance reconcile; RAM/IO budgeted)

crates/platform-core/src/storage/repositories/
  positions.rs     TokenPositionRepo (upsert, by_mint, by_role, close)
  manage.rs        ManageActionRepo (insert planned, mark_executing/completed/partial)
```

Execution detail: reuse **one** `PumpFunTrader` per request where possible (`initialize()` is
~8–12 RPC round trips). For each leg: `resolve_signer(wallet)` → `sell_token_once(confirm=false)` /
`buy_token(...)` → collect the returned signature → poll the **full** `trades` window before retry
(the meme-trading double-sell guard; preserve it). Update `token_positions` from the confirmed fill,
not from the intended amount.

## HTTP endpoints (`crates/live/src/http.rs`)

```
GET  /api/tokens/:mint/positions            -> per-wallet holdings + PnL (reads token_positions)
POST /api/tokens/:mint/manage/preview       -> ActionPlan (no execution) from a ManageRequest
POST /api/tokens/:mint/manage/execute       -> execute a previously previewed plan (by plan id)
POST /api/tokens/:mint/consolidate          -> sweep tokens/SOL -> treasury
GET  /api/tokens/:mint/manage/actions       -> manage_actions history
```

`ManageRequest { kind, sizing, size, selection }`. Preview and execute are split so the UI can
show the plan and the operator confirms. Guard destructive execution behind an explicit
`confirm: true` + the existing settings gate (a `MANAGE_ENABLED` kill switch, mirroring `FUND_ENABLED`).

## Frontend (`frontend-launch/`)

- Extend `src/features/launches/TokenDetailPage.tsx` with a **Manage** panel:
  - Per-wallet holdings + PnL table (role, balance, cost, value, PnL%) — reads `/positions`.
  - Action form: kind (sell/buy/consolidate) · wallet group (dev/bundlers/all/pick) · sizing
    (slider for %, input for SOL target) → **Preview** shows the `ActionPlan` legs → **Execute**.
  - "Sell all" prominent + double-confirm; actions history list.
- Endpoints in `src/shared/store/endpoints.ts` (RTK-Query mutations + `Positions` tag for cache
  invalidation, invalidated on execute). Types in `src/shared/types.ts`.

## Phasing

1. **Read model** ✅ **DONE** (2026-07-08) — migration `0010_token_positions.sql`, `PositionStatus`
   enum + roundtrip test, `TokenPosition` model, `TokenPositionRepo` (`seed`/`by_mint`/`set_balance`),
   `LaunchRepo::find_by_mint`, `ManagedWalletRepo::get_many`, `launcher::manage::load_positions`
   (seed-from-launch/bundle + best-effort on-chain reconcile via `getTokenAccountsByOwner`),
   `GET /api/tokens/{mint}/positions`, and the "Our holdings" holdings+PnL table on
   `TokenDetailPage`. Reconcile is on-read (no background poller yet — cold path). *Manage nothing
   yet; just see everything.* Verified: `cargo check -p live` + `cargo test -p platform-core` +
   `vite build` all clean. **Operator step:** migration `0010` applies on next `live` boot (`connect`
   runs migrations) — no manual step, but the DB must be reachable.
2. **Sell primitive** — `plan/preview/execute` split, wallet-group selection, sizing
   (pct / to_sol_target / fixed_base), feed-confirm, `manage_actions` audit. Ship "sell all" +
   "sell %" with double-confirm. **Safety-critical — do first.**
3. **Buy + consolidate** — fixed-size buys from dev/bundlers; generalize `dust_sweep` to tokens+SOL.
4. **Simple-threshold ladders** *(later)* — arm a rule (sell X% at price/mcap milestone), evaluated
   by a background task on the ingested `trades` feed; reuses the phase-2 sell primitive.
5. **Volume-making** *(later)* — fresh-wallet rotation + buy/sell loop scheduler over the buy/sell
   primitives + wallet pool/funding.

## SSOT / budget cautions

- Amounts carry unit suffixes, exact `bigint` base units (`_base` tokens / `_quote` lamports);
  prices are raw ratios, decimals+USD only in views. Don't store computed PnL.
- CHECK vocabularies → Rust enums in `models/status.rs` + roundtrip test.
- EC2 is 2vCPU/4GB, IO-bound: the position poller must be batched + budgeted (reuse the SOL
  poller's `getMultipleAccounts` batching); don't add unbounded per-token RPC. New pools require
  shrinking something else.
- Reuse `config::constants::{sol_to_lamports, lamports_to_sol}` (meme-trading) / SLP equivalent —
  no private lamport conversions.
- `token_account` persisted on the position row so bot sell/buy reuse the canonical ATA across
  restarts (multi-account-token lesson from meme-trading).
```
