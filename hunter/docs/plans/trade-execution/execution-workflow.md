# Trade Execution Workflow

End-to-end lifecycle of a real position across the live fingerprint+metrics engine
(`strategies/engine/`) and `pump-trader/` (on-chain execution). See
[@arch/trade-execution.md](@arch/trade-execution.md) for the pump-trader file map and
[@arch/strategies.md](@arch/strategies.md) for the strategy module map.

The pure fold (`hunter-engine::reduce`) owns *when* to buy/sell; the adapters below
own *how* — SOL guards, write-ahead submit, feed confirm, classify/heal, and crash
recovery. Those adapters are `exec_real.rs`, `InFlightGuards`, and `reapers.rs`, all
under `live/src/strategies/engine/`.

## A0. Decision → spawn (latency)

`dispatch` runs state effects before submit effects, and Pass 1 does not await
durable PG for the hot transitions:

| Transition | Pass 1 | Submit spawn |
|---|---|---|
| `BuySubmitted` | registry upsert + background `insert_position` (handle kept) | immediate once registry has `pg_id` |
| `Holding` | registry fill economics sync; background `record_entry_fill` (chains after insert) | n/a (sell sizes from registry) |
| `ExitPending` | fire-and-forget status update (chains after pending write inside spawn) | not gated on the status write |
| later fills / terminal | await pending PG chain, then write | n/a |

`Sink::warm_runs` on rule reload pre-caches `strategy_runs` so the first buy of a
rule rarely awaits `insert_run`. Cold miss reuses the latest still-`Running` row
(and deletes empty leading shells from older always-insert warm behavior) so a
restart does not push paper scoreboard N/PnL onto a new empty `run_seq`.

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

adopt_existing_fill_if_present()           ← process-local SubmittedBuyJournal only
                                             (empty ⇒ first attempt, skip PG)

reserves_fn() re-quotes min_out            ← fresh slippage floor from the live cache
register on_signed write-ahead hook        ← sync journal + fire-and-forget
                                             mark_buy_submitted (≤250 ms bound);
                                             never holds the nonce slot on PG
send_snipe_buy() [confirm=false]           ← hook returns immediately, then submit
                                             (send_transaction_rebroadcast: detached
                                             front-loaded re-post of the IDENTICAL
                                             signed tx — schedule
                                             [60,120,250,400,700,1000] ms then 1 s
                                             tail, 5 s window; gated on
                                             TxAnchor::Entry — see below)
poll_feed_until_entry_fill() ~12 s         ← event-driven (TradeSignals.notify)
on timeout → classify_silent_send(sig) routes a Reverted status through the SSOT
             classify_swap_revert(custom, SwapRoute::Curve, SwapDirection::Buy):
  Reverted + buy slippage (6002/6042)  → FillFailed::Reverted (engine resubmits;
                                         fresh min_out on next run_entry)
  Reverted + ConstraintSeeds 2006      → recheck_curve_creator(): chain creator ≠
                                         the order's → token cache + Reverted retry;
                                         equal / read-fail → FillFailed::Fatal
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
nonce). The process-local `SubmittedBuyJournal` is set synchronously on sign; PG
`mark_buy_submitted` is fire-and-forget (250 ms bound) so a slow DB never delays
fan-out. A crash between sign and submit still leaves a `BuySubmitted` row (once
the insert lands) for the recovery reaper; same-process retries adopt from the
journal without a PG read.

**Why per-signature attribution:** adopt-before-send only matches fills against *this
position's* submitted signatures — two concurrent positions in the same wallet on the
same token can never cross-adopt each other's fills.

**Why rebroadcast the snipe buy** (`Engine::send_transaction_rebroadcast`, curve snipe
only, gated on `TxAnchor::Entry` — not on `durable_nonce`, because Entry may fall back
to a recent blockhash under nonce contention): the Helius Sender submits once with
`maxRetries: 0` and does NOT rebroadcast. On the cheap sub-`0.001 SOL` tip tier
(Sender's best-effort band — "fewer pathways, no priority buffer") that single
un-prioritized shot frequently misses every leader slot, so the tx never lands and the
row sits at `BuySubmitted` forever (observed 0/4 landing on the deployed box,
2026-07-24). The fix keeps the cheap tier and re-posts the **identical** signed tx on a
front-loaded schedule (`[60,120,250,400,700,1000]` ms, then 1 s tail, 5 s window) —
snipe value decays inside a slot or two, so a flat 500 ms cadence wasted its first
re-posts. Safe by construction: the signature is fixed, so the bank dedups every
re-post — the tx executes at most once and the Jito tip is paid at most once; once it
lands a nonce tx consumes the nonce and later re-posts are rejected as stale
(harmless). A blockhash is valid ~60 s (12× the window), so the Entry fallback keeps
rebroadcasting too. Sender POSTs are 0-credit, so the extra broadcasts add landing
paths at no cost. Sells already retry (6-attempt loop), so only the buy needed this.

## C. Sell path — `exec_real::run_exit` (+ engine exit retries)

```
ExitGuard claimed                          ← recovery reaper skips guarded pg ids
mint lock claimed, or queued behind the    ← FIFO, bounded by the holder's worst case;
  sibling exit that holds it                 timeout → FillFailed::Reverted (nothing sent);
                                             after a wait, drop if the intent moved on
release_sol_for_position()                 ← idempotent; done FIRST, before any tx
if entry_token_amount == 0: FillConfirmed at zero (no tx)

per-attempt loop (max 6, tip escalates per level — max(percentile ladder, min×1.5^level)):
  re-read is_migrated from TokenCache      ← route can flip mid-exit (curve → AMM)
  send sell (15 s hard cap for RPC ops)    ← Ok(Some(sig)) | Ok(None) | Err
  await_own_legs (the wait both confirms share):
    wake future created BEFORE the checks  ← a notify during a check is not lost
    own-leg preview on every wake          ← ingest records it before the DB write
    sum_legs_by_signatures(sell_sigs,      ← fallback only: every 250 ms when a trade
      since = exit start)                    landed for the key, and at the deadline
    remaining ≤ dust → cleared ✓
  if deadline without clear → classify_sell_confirm(error_code, route_changed):
    (thin wrapper over pump-trader's shared pump_trader::classify_swap_revert(
     custom, SwapRoute, SwapDirection::Sell) — the SAME classifier pump-trader's
     own confirm=true sell/amm_sell retry uses; see @arch/trade-execution.md)
    slippage revert OR route changed       → retry (new reserves / new route next attempt)
    curve ConstraintSeeds 2006             → RefreshCreator: recheck_curve_creator()
                                             chain creator ≠ the attempt's → next attempt
                                             uses it; equal / read-fail → FillFailed::Fatal
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

**Which creator an order uses:** pump.fun can reassign `bonding_curve.creator` (via
`set_creator`) at any time on the curve, and both `buy` and `sell` seed `creator_vault`
from `["creator-vault", bonding_curve.creator]`. The launch creator
(`token.creator_wallet`) is therefore NOT the vault seed. Every curve `TradeEvent`
carries the creator the venue validated that swap against (`Trade::curve_creator`);
ingest writes it to `TokenState::curve_creator` before the strategy ping, and
`dispatch_buy` / `dispatch_sell` take `TokenState::trade_creator()` (that creator, else
the launch creator). The trigger print is itself a landed swap on the mint, so a buy
decided on it derives the vault the chain just accepted, with no RPC.

**Why a 2006 (ConstraintSeeds) is recoverable, not Fatal:** it still happens when the
creator changes between the order's print and its transaction, or on an order built
before any print carried the creator (after a restart). `recheck_curve_creator()`
re-reads the curve's creator (one off-path RPC, only after a confirmed revert) and
compares it with the creator the reverted transaction used: different → write it to the
token cache and retry (the buy's retry is a new engine decision that reads it; the sell
loop's next attempt carries it); equal, or the read fails → Fatal, so a resend never
re-pays a fee on a vault it cannot fix. The AMM route's `refresh_amm_pool_info()` keeps
its changed-vs-unchanged rule. Both loops funnel through the one
`pump_trader::classify_swap_revert` decision table.

**Why the preview, and Postgres only as a fallback:** ingest records our own leg in
`TradeSignals` and wakes the waiter before it queues the DB write, so the preview holds a
landed transaction one feed hop after it arrives. A Postgres lookup can only be slower:
it waits for the batched commit, and on a busy disk it costs a read per chunk it
probes. The fallback covers a leg the preview does not hold (healed from RPC, or past
its TTL). It never runs at t=0, re-runs only once `WaitGuard::seq` shows a trade landed
for the key, and its `since` anchor bounds `block_time` so it prunes to the recent
chunks.

## D. RAII interlocks (`InFlightGuards` in `engine/mod.rs`)

The pg-id guards are DashSet operations: zero allocation, panic-safe (Drop always
fires). The mint guard is a FIFO `tokio::sync::Mutex` per mint, pruned once no guard or
waiter holds it.

**`EntryGuard`** — claims `strategy_positions.id` during buy:
- Prevents two concurrent entry tasks for the same PG row
- Recovery reaper (`redrive_orphaned_buy_submitted`) skips positions with a live guard
- Dropped when the spawned task ends (fill recorded, Fatal, or Ambiguous)

**`ExitGuard`** — claims `strategy_positions.id` during sell:
- `try_begin_exit()` returns `None` if already claimed; live sell + reaper redrive
  go through this gate
- Spawned sell task holds the guard for its full lifetime
- Dropped when sell completes or the task panics — no wedged state possible

**`ExitMintGuard`**: claims the mint during sell (positions on one mint share a token
account):
- A bot exit queues behind the holder (`begin_exit_mint`) and sends the moment it
  finishes. The wait is bounded by the holder's own worst case (`EXIT_MINT_WAIT`); a
  timeout hands the exit back as `FillFailed::Reverted`.
- Orphan sweeps and the wallet Sell All try-lock (`try_begin_exit_mint`) and report busy.

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
