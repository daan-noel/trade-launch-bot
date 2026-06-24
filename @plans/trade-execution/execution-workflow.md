# Trade Execution Workflow

End-to-end lifecycle of a real position across `strategies/` (orchestration) and `pump-trader/` (on-chain execution). See [@arch/trade-execution.md](@arch/trade-execution.md) for the pump-trader file map and [@arch/strategies.md](@arch/strategies.md) for the strategy module map.

## A. Pre-buy guards (service.rs — both TPSL1 and TPSL2)

Checked **inline** in `on_token_created`, before `sync_position`. If either fires the loop continues immediately — no position is created, no runtime cleanup needed.

| # | Guard | Formula | Source |
|---|---|---|---|
| 1 | SOL balance-floor | `wallet_balance − 0.02 SOL − committed ≥ buy_amount` | `trader.can_commit_buy(buy_lamports)` |
| 2 | `max_committed_sol` | `committed + buy_amount ≤ ceiling` (optional; `None` = disabled) | `settings.max_committed_sol` |

`committed_lamports` = sum of all open real position debits (atomic `u64`; DashMap tracks per-position share). Both guards read the same counter. The wallet balance is a background-refreshed cache (~30 s interval); guard 1 **fails open** when the cache is cold (startup) so a stale cache never blocks all buys — the on-chain transaction is the real backstop.

## B. Buy path — `buy_until_filled_or_give_up` (execution/real.rs)

```
commit_sol_for_position(buy_lamports)      ← SOL earmarked; released on any failure
EntryGuard claimed                         ← recovery reaper skips guarded positions

per-attempt loop (max 3):
  adopt_existing_fill_if_present()         ← check OUR sent sigs before sending again
  register on_signed write-ahead hook      ← closure persists sig to DB on sign
  send_snipe_buy() [confirm=false]         ← hook fires AFTER sign, BEFORE network submit
  poll_feed_until_entry_fill() 12 × 1 s   ← event-driven (TradeSignals.notify)
  on timeout → classify_silent_send(sig):
    Ok(Some(false)) proven revert  → retry (safe: tx is confirmed gone)
    Ok(Some(true))  landed+unindexed → wait extra poll window (re-send = double-buy)
    Ok(None) / Err  pending/unknown → give up (never re-send; nonce tx may still land)

on fill:   update_entry → sync runtime cache → EntryGuard drops
on fail:   release_sol_for_position() + delete position + EntryGuard drops
```

**Why write-ahead before submit:** the signature is fixed at signing time (durable nonce), so it can be persisted to DB before any network round-trip. A crash between sign and submit leaves a `BuySubmitted` row the recovery reaper can classify.

**Why per-signature attribution:** `adopt_existing_fill_if_present` only matches fills against *this position's* submitted signatures — two concurrent positions in the same wallet on the same token can never cross-adopt each other's fills.

## C. Sell path — `sell_and_close_position` / `sell_until_balance_cleared` (execution/real.rs)

```
release_sol_for_position()                 ← idempotent; done FIRST, before any tx
if entry_token_amount == 0: close directly (no tx, no poll)

per-attempt loop (max 6, Jito tip escalates per level):
  re-read is_migrated from ReserveCache    ← route can flip mid-exit (curve → AMM)
  send sell (15 s hard cap for RPC ops)    ← Ok(Some(sig)) | Ok(None) | Err
  register wakeup BEFORE each balance query (prevent miss-in-gap)
  poll_feed event-driven, rate-limited ≥ 250 ms between queries:
    sum_legs_by_signatures(sell_sigs)      ← per-sig; never shared net balance
    remaining ≤ 0.0001 → cleared ✓
  if deadline without clear → classify_sell_revert(error_code, route_changed):
    slippage revert OR route changed       → retry (new reserves / new route next attempt)
    structural revert (empty acct, etc.)   → StopFeeBurn (blind retry only wastes fees)
    no-land / pending / status error       → retry with escalated Jito tip

on cleared:  spawn rent-reclaim (fire-and-forget), record_exit, log PnL%, ExitGuard drops
on failed:
  net_token_amount_by_wallet_and_mint() ≤ threshold
    → close_externally_cleared_position (ManualSell)
  else → mark ExitFailed at trigger_price/time; ExitGuard drops
```

**Why SOL is released first:** `release_sol_for_position` must fire regardless of whether the sell succeeds or the process crashes mid-exit. Releasing after a confirmed sell would leave committed SOL stranded if the process crashes between sell and release.

**Why route is re-read per attempt:** a token can migrate from curve → AMM between sell attempts. Re-reading `is_migrated` lets the next attempt automatically switch venue without manual intervention.

**Why rate-limit balance queries:** during a rapid sell dump the `trades` feed can fire many times per 250 ms window. Querying `sum_legs_by_signatures` on every notification would run a DB aggregate in a tight loop; the rate-limit batches notifications into at most one query per 250 ms, with a bypass at the poll deadline to ensure a final check always runs.

## D. RAII interlocks (runtime_cache.rs)

Both guards are DashSet operations — zero allocation, panic-safe (Drop always fires).

**`EntryGuard`** — claims `(rule_id, mint)` slot during buy:
- Prevents two concurrent entry tasks for the same rule+token
- Recovery reaper (`redrive_orphaned_buy_submitted`) skips positions with a live guard
- Dropped when the spawned task ends (fill recorded or buy failed)

**`ExitGuard`** — claims `position_id` slot during sell:
- `try_begin_exit()` returns `None` if already claimed; all exit paths (trade-driven, clock-driven, manual sell detection) go through this gate
- Spawned sell task holds the guard for its full lifetime
- Dropped when sell completes or the task panics — no wedged state possible

## E. Crash recovery (service.rs background tasks)

Both reapers fire once at boot (immediate tick) then every 60 s.

**`redrive_orphaned_buy_submitted`** — classifies in-flight buys; never re-sends:
- Per `BuySubmitted` row, query on-chain status of each submitted signature:
  - **Adopt:** any sig found in the `trades` feed → `update_entry` → transition to Holding
  - **Drop:** all sigs confirmed reverted on-chain → safe to delete position
  - **Wait:** any sig pending / unknown / age < 10 min → leave row, try again next tick
  - **Flag:** any sig still pending > 10 min → log for manual review

**`redrive_orphaned_exit_pending`** — re-drives stalled sells:
- Finds `ExitPending` rows with no live `ExitGuard` (guard not held = sell task is gone)
- Runs **before** the stale-fail sweep so recoverable bags get a retry before being marked failed
- Re-spawns `spawn_real_sell()` for each orphan
- Stale-fail sweep (`fail_stale_exit_pending`) marks ExitFailed after 5 min of unresolved ExitPending
