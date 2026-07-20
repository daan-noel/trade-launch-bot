# Trade Execution Workflow

End-to-end lifecycle of a real position across the live fingerprint+metrics engine
(`strategies/engine/`) and `pump-trader/` (on-chain execution). See
[@arch/trade-execution.md](@arch/trade-execution.md) for the pump-trader file map and
[@arch/strategies.md](@arch/strategies.md) for the strategy module map.

The pure fold (`hunter-engine::reduce`) owns *when* to buy/sell; the adapters below
own *how* — SOL guards, write-ahead submit, feed confirm, classify/heal, and crash
recovery. Path names from the pre-engine stack (`execution/real.rs`,
`runtime_cache.rs`) map to `exec_real.rs` / `InFlightGuards` / `reapers.rs`.

## A. Pre-buy guards (`decision_loop::dispatch_buy` — real mode)

Paper vs real is resolved from the position's `PositionMeta.trade_mode`
(snapshotted at `BuySubmitted` in Pass 1), matching sell routing — not from a
live rule reload — so an entry retry cannot flip executors. `trade_mode` is also
frozen post-create on the rule row (`apply_rule_update` ignores it).

Checked **inline** before spawning `run_entry`. If either fires the adapter emits
`FillFailed::Reverted` (engine may retry when free SOL returns) — no on-chain send.

| # | Guard | Formula | Source |
|---|---|---|---|
| 1 | SOL balance-floor | `wallet_balance − 0.02 SOL − committed ≥ buy_amount` | `trader.can_commit_buy(buy_lamports)` |
| 2 | `max_committed_sol` | `committed + buy_amount ≤ ceiling` (optional; `None` = disabled) | `settings.max_committed_sol` |

`committed_lamports` = sum of all open real position debits (atomic `u64`; DashMap
tracks per-position share). Both guards read the same counter. The wallet balance is
a background-refreshed cache (~30 s interval); guard 1 **fails open** when the cache
is cold (startup) so a stale cache never blocks all buys — the on-chain transaction
is the real backstop.

## B. Buy path — `exec_real::run_entry` (+ engine entry retries)

```
EntryGuard claimed                         ← recovery reaper skips guarded pg ids
commit_sol_for_position(buy_lamports)      ← idempotent per pg_id; released on Fatal /
                                             sink terminal / sell-start / reaper drop

adopt_existing_fill_if_present()           ← check THIS position's submitted sigs
                                             before sending again

reserves_fn() re-quotes min_out            ← fresh slippage floor from the live cache
register on_signed write-ahead hook        ← closure persists sig to DB on sign
send_snipe_buy() [confirm=false]           ← hook fires AFTER sign, BEFORE network submit
poll_feed_until_entry_fill() ~12 s         ← event-driven (TradeSignals.notify)
on timeout → classify_silent_send(sig) routes a Reverted status through the SSOT
             classify_swap_revert(custom, SwapRoute::Curve, SwapDirection::Buy):
  Reverted + buy slippage (6002/6042)  → FillFailed::Reverted (engine resubmits;
                                         fresh min_out on next run_entry)
  Reverted + ConstraintSeeds 2006      → refresh_curve_creator_vault(); changed →
                                         Reverted retry, unchanged / refresh-fail →
                                         FillFailed::Fatal
  Reverted + structural/unknown        → FillFailed::Fatal (blind resend only re-pays fees)
  Succeeded (landed+unindexed)         → wait extended poll (re-send = double-buy);
                                         still missing → Ambiguous (emit nothing)
  Pending / Err (status unknown)       → Ambiguous (never re-send; nonce tx may still land)

on fill:   FillConfirmed → sink records Holding; EntryGuard drops
on Fatal:  release_sol + FillFailed::Fatal → engine books ExitFailed (no retry)
on Ambiguous: leave BuySubmitted for the reaper; EntryGuard drops
```

Engine outer bound: `MAX_ENTRY_ATTEMPTS = 3` (`FillFailed::Reverted` only).
`FillFailed::Fatal` gives up immediately.

**Why write-ahead before submit:** the signature is fixed at signing time (durable
nonce), so it can be persisted to DB before any network round-trip. A crash between
sign and submit leaves a `BuySubmitted` row the recovery reaper can classify.

**Why per-signature attribution:** adopt-before-send only matches fills against *this
position's* submitted signatures — two concurrent positions in the same wallet on the
same token can never cross-adopt each other's fills.

## C. Sell path — `exec_real::run_exit` (+ engine exit retries)

```
ExitGuard claimed                          ← recovery reaper skips guarded pg ids
release_sol_for_position()                 ← idempotent; done FIRST, before any tx
if entry_token_amount == 0: FillConfirmed at zero (no tx)

per-attempt loop (max 6, Jito tip escalates per level):
  re-read is_migrated from TokenCache      ← route can flip mid-exit (curve → AMM)
  send sell (15 s hard cap for RPC ops)    ← Ok(Some(sig)) | Ok(None) | Err
  register wakeup BEFORE each balance query (prevent miss-in-gap)
  poll_feed event-driven, rate-limited ≥ 250 ms between queries:
    sum_legs_by_signatures(sell_sigs)      ← per-sig; never shared net balance
    remaining ≤ dust → cleared ✓
  if deadline without clear → classify_sell_confirm(error_code, route_changed):
    (thin wrapper over pump-trader's shared pump_trader::classify_swap_revert(
     custom, SwapRoute, SwapDirection::Sell) — the SAME classifier pump-trader's
     own confirm=true sell/amm_sell retry uses; see @arch/trade-execution.md)
    slippage revert OR route changed       → retry (new reserves / new route next attempt)
    curve ConstraintSeeds 2006             → RefreshCreator: refresh_curve_creator_vault()
                                             changed → retry, unchanged → FillFailed::Fatal
    AMM   ConstraintSeeds 2006             → RefreshCoinCreator: refresh_amm_pool_info()
                                             changed → retry, unchanged → FillFailed::Fatal
    6024 / 6005                            → refresh cashback / re-route migrated
    structural revert                      → FillFailed::Fatal
    Succeeded / Pending / status error     → extended feed poll; still unclear →
                                             FillFailed::Unconfirmed (never re-sell)

on cleared:  spawn rent-reclaim (M1: only if no sibling open on mint), FillConfirmed,
             ExitGuard drops
on Fatal:    FillFailed::Fatal → engine books ExitFailed
on Unconfirmed: FillFailed::Unconfirmed → ExitUnconfirmed (never re-sold)
```

Engine outer bound: `MAX_EXIT_ATTEMPTS = 5` for safe `Reverted` (e.g. never-submitted).
`Fatal` / `Unconfirmed` are terminal in the fold.

**Real-SOL smoke:** ops checklist in [sell-close-smoke.md](./sell-close-smoke.md)
(classifier unit-tested; chain smoke is manual).

**Why SOL is released first:** `release_sol_for_position` must fire regardless of
whether the sell succeeds or the process crashes mid-exit. Releasing after a confirmed
sell would leave committed SOL stranded if the process crashes between sell and release.

**Why route is re-read per attempt:** a token can migrate from curve → AMM between sell
attempts. Re-reading `is_migrated` lets the next attempt automatically switch venue.

**Why a 2006 (ConstraintSeeds) is recoverable, not Fatal:** the snipe buy caches
`TokenPDAs.creator_vault` derived from the create-event creator. pump.fun can change
`bonding_curve.creator` (via `set_creator`) *after* that buy — both `buy` and `sell`
seed `creator_vault` from `["creator-vault", bonding_curve.creator]`, so the stale
cached vault then reverts every sell (or resend of a buy) with Anchor
`ConstraintSeeds (2006)`. A 2006 is therefore *not* structural on either route:
`refresh_curve_creator_vault()` / `refresh_amm_pool_info()` re-read the current
creator/pool (one off-path RPC, only after a failed poll window) and report
changed-vs-unchanged; changed → overwrite the cache and retry, unchanged (or the
refresh RPC itself fails) → Fatal. Both the sell loop here and the curve-buy retry in
**B** funnel through the one `pump_trader::classify_swap_revert` decision table.

**Why rate-limit balance queries:** during a rapid sell dump the `trades` feed can fire
many times per 250 ms window. Querying `sum_legs_by_signatures` on every notification
would run a DB aggregate in a tight loop; the rate-limit batches notifications into at
most one query per 250 ms, with a bypass at the poll deadline to ensure a final check
always runs.

## D. RAII interlocks (`InFlightGuards` in `engine/mod.rs`)

Both guards are DashSet operations — zero allocation, panic-safe (Drop always fires).

**`EntryGuard`** — claims `strategy_positions.id` during buy:
- Prevents two concurrent entry tasks for the same PG row
- Recovery reaper (`redrive_orphaned_buy_submitted`) skips positions with a live guard
- Dropped when the spawned task ends (fill recorded, Fatal, or Ambiguous)

**`ExitGuard`** — claims `strategy_positions.id` during sell:
- `try_begin_exit()` returns `None` if already claimed; live sell + reaper redrive
  go through this gate
- Spawned sell task holds the guard for its full lifetime
- Dropped when sell completes or the task panics — no wedged state possible

## E. Crash recovery (`reapers.rs` — spawned from the engine loop)

Reaper fires **immediately at boot**, then every 60 s. Runs **before** the stale-fail
sweep so recoverable bags get a retry before being marked failed.

**`redrive_orphaned_buy_submitted`** — classifies in-flight buys; never re-sends:
- Per `BuySubmitted` row (guard not held), query feed/chain of each submitted signature:
  - **Adopt:** any sig found in the `trades` feed → `record_entry_fill` → Holding;
    if the engine still tracks the row, also emit `FillConfirmed` so TP/SL resumes
  - **Drop:** all sigs confirmed reverted on-chain → delete position + `release_sol`;
    nudge engine with `FillFailed::Fatal` when tracked
  - **Wait:** any sig pending / unknown / age < 10 min → leave row, try again next tick
  - **Flag:** any sig still pending > 10 min → log for manual review

**`redrive_orphaned_exit_pending`** — re-drives stalled sells:
- Finds `ExitPending` rows with no live `ExitGuard`
- If the engine still has `inflight_intent` → emit `FillFailed::Reverted` so the fold
  re-`SubmitSell`s (opaque intents aren't reconstructible from PG alone)
- Else (post-restart orphan) → spawn a direct `run_exit` and persist the outcome onto
  the PG row
- Stale-fail sweep (`fail_stale_exit_pending`) marks ExitFailed after 5 min of
  unresolved ExitPending
- `delete_stale_unentered` only deletes `Arming` rows (never `BuySubmitted`)
