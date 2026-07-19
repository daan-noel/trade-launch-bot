# Helius RPC-usage reduction — plan (round 2, 2026-07-19)

Project-wide audit of **billed Helius JSON-RPC usage** across hunter + forge + shared
crates, with a prioritized, stability-first reduction plan. Round 1 (the
`getTransaction` prewarm burst + steady-state blockhash/nonce polling) already landed
(`3911d070`, `63a455df`); this is the follow-on covering what remains.

> **Status:** PLAN ONLY — awaiting go-ahead per item. Nothing here is implemented yet.
> Scope decision pending; balance-poll approach chosen = **push-feed via LaserStream**.

---

## 0. Billing model — what is and isn't a credit

Only reads on the **credited `HELIUS_RPC_URL`** cost credits. These do **not**:

| Traffic | Endpoint | Billing |
| --- | --- | --- |
| `sendTransaction` (trades) | `HELIUS_FAST_SENDER_URL` (`/fast`) | **0 credits** |
| `sendBundle` (launches) | Jito block engine | not Helius |
| Jito tip floor | `bundles.jito.wtf` REST | not Helius |
| SOL/USD price | CoinGecko / Jupiter | not Helius |
| Live ingest stream | LaserStream gRPC (Yellowstone) | flat plan, not per-call |

**Consequence:** the reduction target is **read RPC** (`getAccountInfo`, `getBalance`,
`getMultipleAccounts`, `getTransaction`, `getSignaturesForAddress`,
`getTokenAccountsByOwner`, `getSlotLeaders`, `getSignatureStatuses`) and
`simulateTransaction`. The `sendTransaction` sender fan-out (multiple `/fast` URLs) is a
**land-rate/latency dial, not a credit cost** — do not "reduce" it for billing reasons.

## 1. Baseline — what round 1 already achieved (DO NOT re-touch)

Verified intact in current code:

- **Automated trade hot path is push/cache-fed end-to-end**: blockhash (push `blocks_meta`
  → `BlockhashCache::store_pushed`, watchdog only RPCs if the tick was missed), durable
  nonce (`on_nonce_account_update` push re-arm), AMM pool facts (feed-harvested
  `observe_amm_swap_accounts`), reserves (WS `reserve_cache`), routing (cache-served for
  migrated tokens). Steady-state automated trade ≈ **1 `sendTransaction` (0 credits)**.
- **Sell-confirm is feed-based** (strategy path never RPC-polls a confirm) — MUST stay so.
- **forge `confirm.rs`** bundle-landing watcher is feed-derived (zero RPC).
- **forge read paths** (`manage/positions.rs` page reads, token holdings, manage preview)
  are feed-derived; all balance batches already use `getMultipleAccounts` (≤100/call).
- **hunter `token_sync` fetch paths**: full backfill uses gTFA (~0.1 credit/tx);
  incremental prefers LaserStream replay (0 credits) before any RPC fallback.

**Idle steady-state RPC today:** hunter-live = **one** `getBalance` every 60s;
forge-live = **zero**. Round 1 did the heavy lifting; round 2 is about the remaining
event-driven bursts + the last steady-state poll + dashboard-poll amplification.

---

## 2. Tier 1 — highest impact, zero hot-path risk

### T1.1 — Forge restore/backfill: `getTransaction` → `getTransactionsForAddress` (gTFA) + DB-dedup
- **Where:** `shared/ingest/core/src/backfill/pager.rs:137` (batch `getTransaction`),
  driven by `forge/live/src/restore/backfill.rs:92`; signature gather at
  `pager.rs:59` / `backfill.rs:148`.
- **Now:** `POST /api/wallet_pool/restore` pages **every managed wallet's full history**
  via `getSignaturesForAddress`, then fetches **every** signature via `N×getTransaction`
  (batched only for round-trips — still 1 credit/tx). No dedup vs DB, so every re-run
  re-spends the full cost. Cost ≈ **U credits** (U = union of successful sigs across all
  wallets' entire history) + `Σ ceil(H_w/1000)` sig-page calls.
- **Change:**
  1. Port `get_transactions_for_address_full_page_enc` (already at
     `hunter/core/src/services/helius_rpc.rs:414`, used by hunter `token_sync`) into
     `ingest-core`'s pager; page gTFA per wallet instead of gather-sigs-then-batch-get.
     Output is already `getTransaction`-shaped, so `wrap_transaction_result` /
     `rpc_to_protobuf` are unchanged.
  2. Before fetching, dedup the signature set against existing `trades`/`raw_txs`
     (hunter already does this — `trade_repo.rs:365`), and persist a per-wallet
     newest-restored-signature checkpoint passed as `until` (pager already accepts `until`
     at `pager.rs:63`) so re-runs page only the delta.
- **Win:** **~10×** on the dominant `getTransaction` spend, and re-runs/incremental
  restores go from full-re-spend to near-free.
- **Tradeoff / stability:** gTFA is a Helius archival extension (feed is already Helius);
  same data, honors base64 incl. `meta.loadedAddresses`. Dedup is write-idempotent already,
  so no correctness change — only fewer fetches. **No hot-path exposure** (restore is a
  restart-time op).
- **Optional add-on:** bound the restart window (max age / pages / slot floor) instead of
  `before=None, until=None` (`backfill.rs:148`) so a high-history wallet doesn't page its
  whole life; the live feed covers everything after restore. Defer unless a wallet has
  huge history.
- **Verify:** restore a keystore with a warm DB → observe near-zero `getTransaction`;
  cold DB → observe ~1/10th credits vs before on the Helius method dashboard.

### T1.2 — hunter dashboard: cache the two uncached read endpoints
- **T1.2a `get_wallet_tokens`** — `hunter/live/src/api/handlers/trading/solana.rs:494` →
  `services/wallet_tokens.rs:29` `list_enriched` → `trader.get_all_token_accounts()`
  (**2× `getTokenAccountsByOwner`** per request) + `resolve_curve_facts_batch`.
  - **Now:** uncached — every dashboard poll does a fresh two-program wallet scan. If the
    UI polls this every few seconds while open, this is plausibly the **largest 24/7
    credit source** (e.g. 3s poll ≈ 28.8k req/day × 2 = 57.6k `getTokenAccountsByOwner`/day).
  - **Change:** route `list_enriched`/`enrich_one` through the existing 8s `HoldingsCache`
    (`services/portfolio.rs:92`, `state/deploy_state.rs:63`) that the sibling portfolio
    path already uses — or fold the endpoint onto the portfolio SSOT.
  - **Tradeoff:** ≤8s staleness (already the accepted portfolio contract); post-trade
    freshness handled by the existing `?fresh=1`/`invalidate()` bust.
- **T1.2b `cashback_status`** — `hunter/live/src/api/handlers/trading/cashback.rs:37` →
  `claim.rs:104` (**2× `getAccountInfo`**, curve + AMM pots), no server-side cache.
  - **Change:** add a 30–60s server-side TTL cache; bust it after a successful
    `claim_cashback`.
  - **Tradeoff:** slightly stale cashback display; off the trade path, negligible risk.

### T1.3 — hunter `token_sync`: remove the double `preflight`
- **Where:** `hunter/live/src/api/handlers/tokens/sync.rs:53` runs `preflight` (to return a
  synchronous 400), then the spawned `run_token_sync` runs it **again** at
  `services/token_sync.rs:235`. Each `preflight` = `account_exists` (1 `getAccountInfo`) +
  `has_any_signature` (≥1 `getSignaturesForAddress`).
- **Change:** split `preflight` into "derive bonding curve (pure — `derive_bonding_curve`,
  `token_sync.rs:167`)" + "verify on-chain (RPC)"; run the RPC half once. Pass a
  `skip_preflight`/pre-derived value from the handler to `run_token_sync`.
- **Win:** 1 `getAccountInfo` + ≥1 `getSignaturesForAddress` saved on **every** sync.
- **Tradeoff:** `run_token_sync` loses a standalone on-chain guard, but the handler always
  guards first. Trivial, low risk.

---

## 3. Tier 2 — meaningful, low risk

### T2.1 — hunter SOL-balance poll: **push-feed via LaserStream** (user-chosen)
- **Where:** `hunter/live/src/main.rs:579` `loop { … sleep(60s); trader.get_sol_balance() }`
  → `query.rs:38` `getBalance`. ~1,440 calls/day, forever. Consumer:
  `can_commit_buy` affordability gate (`engine.rs:458`), cache at `engine.rs:477`.
- **Change (push-feed):**
  1. Add the **wallet pubkey** to `PushHooks.watch_accounts` (`main.rs:660-676`, the same
     hook already used for nonce accounts).
  2. **Extend the `on_account` callback to carry `lamports`.** The current signature passes
     `(slot, pubkey, data)`; the wallet is a system account whose balance is in `lamports`,
     not `data`. Yellowstone `SubscribeUpdateAccount` includes `lamports` — thread it
     through the ingest `PushHooks` contract (`shared/ingest/core`) and the executor
     `Engine` bridge, then call `update_balance_lamports_cache` from the pushed value.
  3. Keep a **slow safety poll (~10 min)** as a fallback if the feed doesn't deliver
     account updates on the current plan.
- **Win:** steady-state `getBalance` → ~0; balance tracks at feed speed.
- **Tradeoff:** modest plumbing across the shared ingest `PushHooks` contract (a shared
  crate — verify both consumers: hunter opts in, forge must get a byte-identical
  subscription and ignore the new field). No speed cost (gate still reads a cache).
- **Note:** this touches a **shared-crate public API** (`PushHooks`); treat the signature
  as a contract and keep forge's no-opt-in path unchanged.

### T2.2 — Forge funding pass: batch the confirm + reuse blockhash
- **Where:** `forge/launcher/src/wallet_funding.rs:359` loop → `send_transfer` →
  `plan_exec.rs:137` (`getLatestBlockhash` per transfer) + `:169`
  (`send_and_confirm` = 1 send + a `getSignatureStatuses` poll loop per wallet).
- **Change:** switch the loop to `execute_transfer(confirm=false)` (fire-and-forget), then
  confirm all N in **one** batched `getSignatureStatuses` loop (≤256 sigs/call) — or rely
  on the existing batched `promote_funded` `getMultipleAccounts` read-back with a short
  bounded retry. Fetch **one** blockhash per pass (refresh every ~20 sends / ~30s).
- **Win:** N per-wallet confirm loops → ~1; N−1 `getLatestBlockhash` saved.
- **Tradeoff:** funding is plain SOL transfers with no landing urgency and near-100% land;
  on a rare miss the wallet stays `funding` and the operator re-clicks (already the failure
  posture). Low risk, operator-triggered path.

### T2.3 — Forge jito-leader: cache `getLeaderSchedule` per epoch
- **Where:** `forge/launcher/src/jito_leader.rs:85/92/171` — `wait_for_jito_leader`
  (`bundle_execute.rs:256`) polls `getSlot` + `getSlotLeaders` on **every** launch submit
  and re-bid (≥2 Helius RPC/submit, more while waiting for a Jito slot).
- **Change:** fetch `getLeaderSchedule` once per epoch (whole-epoch leader→slots map),
  then each poll needs only `getSlot` + a local lookup; refetch on epoch rollover. Also
  cache the StakeNet Jito-identity set with a TTL (non-Helius, but latency on the launch
  path).
- **Win:** per-submit leader-gate RPC from ~2/iteration → ~1/iteration; schedule fetch
  amortizes to ~free across an epoch (~2 days).
- **Tradeoff / speed:** on the **live launch path** — but the gate is already fail-open, so
  the only risk from staleness is an occasional non-Jito-slot submit. Moderate complexity
  (epoch-boundary handling).

### T2.4 — hunter `preview_sync`: cap the total-count walk
- **Where:** `services/token_sync.rs:667` — `preview_sync` pages up to
  `PREVIEW_MAX_PAGES=10` × 1000 sigs for a "new" count **and** a "total" count, for both
  curve and AMM → up to ~40 `getSignaturesForAddress` per preview of a high-volume migrated
  mint.
- **Change:** derive the "total" estimate from the **DB** trade count + the cheap "new"
  count instead of re-paging full history over RPC; and/or lower `PREVIEW_MAX_PAGES` to
  3–5; and/or cache total per mint (history only grows).
- **Tradeoff:** coarser "Fetch All" number — preview is advisory UI garnish, stability risk
  nil. User-triggered, not automated.

---

## 4. Tier 3 — cleanups (batch N→1, low absolute volume)

- **Forge sweep/consolidate** (`wallet_sweep.rs:291`/`:241`): batch the post-drain
  `get_balance` (`:421`) into one `getMultipleAccounts` after the loop (like
  `promote_funded`); reuse one blockhash across the pass. The 2× `getTokenAccountsByOwner`
  per wallet is necessary (unknown-mint enumeration across both token programs).
- **Forge manage** (`manage/execute.rs:306`/`:486`): batch `ensure_sell_gas` /
  `consolidate_leg` balances into one `getMultipleAccounts` over the leg wallets.
  Per-leg `trader.initialize()` (`:371`) is an N-call loop into the executor — a shared
  cached global account across per-wallet traders would remove N−1 init round-trips
  (executor-crate change; see T3-exec below).
- **Forge misc:** `build_treasury_pool` per-treasury `get_balance` (`wallet_funding.rs:181`)
  → batch; `wallet_transfer.rs:131` 2× `get_balance` → one `getMultipleAccounts([from,to])`.
- **Executor (`shared/executor`):**
  - Dedup the **manual-buy double bonding-curve read**: `read_curve_routing`
    (`query.rs:496`, holds full curve `account.data`) then `curve_virtual_reserves`
    (`query.rs:313`, a second `get_account` on the same account). Have the routing read also
    extract virtual reserves (offsets 8/16) and seed `reserve_cache` → saves 1 `get_account`
    per manual slippage buy, zero downside. (Snipe path unaffected — reserves come from the
    trigger event.)
  - Batch the **startup nonce prefetch** (`engine.rs:292` → `nonce.rs:340`, N sequential
    `get_account`) into `getMultipleAccounts` (≤100/req). Startup-only; scales with
    nonce-pool size.

---

## 5. Explicit "do NOT touch" (already optimal / necessary)

Automated trade hot path (push-fed blockhash/nonce, feed-harvested AMM pool, WS reserves,
cache-served routing); strategy sell-confirm (feed-based — regressing to an RPC poll
re-introduces latency + double-sell risk); forge `confirm.rs` (feed-based); all existing
`getMultipleAccounts` batch sites; gTFA full backfill + LaserStream-replay incremental in
hunter `token_sync`; boot wallet reconcile (2× `getTokenAccountsByOwner`, once, ground-truth
backstop); SlotAnchor pin (getSlot + getBlockTime, once); the manage `ladder`/`volume`/
lifecycle interval loops (DB/feed-only); SOL/USD + Jito-tip + sender traffic (not Helius
credits).

---

## 6. Suggested implementation order & verification

Order (each independently shippable, verify on the Helius per-method dashboard):
1. **T1.3** (double-preflight) — trivial, isolated, no API change.
2. **T1.2** (dashboard caches) — hunter-only, low risk, plausibly the biggest 24/7 saver.
3. **T1.1** (forge gTFA + dedup) — biggest single-burst saver; port + reuse hunter's impl.
4. **T2.1** (balance poll push-feed) — touches the shared `PushHooks` contract; verify both
   hunter (opt-in) and forge (unchanged) after the signature change.
5. **T2.2 / T2.3 / T2.4**, then **Tier 3** as polish.

Definition of done per item (per repo rules): `cargo check` clean on touched bins
(`hunter-live`, `forge-live`, and the shared crate if `PushHooks` changes), clippy on
touched code, tests where logic changed, docs tier updated. Runtime verification: paper /
operator smoke that exercises the path + a before/after read on the Helius per-method usage
dashboard confirming the expected drop. For T2.1, confirm the current Helius plan actually
delivers `accounts` push updates (fallback: the slow safety poll keeps the gate correct).

---

## 7. Source audits (2026-07-19)

Four parallel subagent audits underpin this plan: `shared/executor` (core+pumpfun),
hunter-live + hunter-core, forge/launcher, and forge-live + shared/ingest. Round-1 context:
[[helius-rpc-reduction]] memory. All file:line references above were reported by those
audits against the current tree — re-verify before editing (the code moves).
