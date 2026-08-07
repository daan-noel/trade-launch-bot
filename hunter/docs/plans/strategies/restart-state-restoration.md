# Restart state restoration — what a warm start must rebuild, and what it must not replay

Permanent reference for the live engine's boot path. The rule it exists to protect:

> **A decision is only valid at the price it will be filled at.** Anything the engine
> learns about the past may shape its *state*; nothing about the past may drive a
> *decision*.

Everything below was found while explaining one real-money exit on 2026-08-06 and
auditing the rest of the boot path for the same shape.

## The incident (`247PRAda…`, rule `auto-fp-c10-best 3ix --- copy`)

| leg | time (UTC) | reason | price (SOL/raw) | vs entry |
| --- | --- | --- | --- | --- |
| buy | 15:26:51.85 | — | 5.2706e-14 | — |
| sell 50% | 15:26:59.33 | `bounce >= 50` | 8.461e-14 | +61% |
| sell 30% | 15:27:00.15 | `TakeProfit` (stage 1, tp 50) | 8.410e-14 | +60% |
| sell rest | **15:32:28.03** | **`StopLoss`** (`stop_loss: 28`) | 9.452e-14 | **+79%** |

The price never traded below entry after the buy — the worst print of the whole hold
was +20%. The stop could not fire on any price the position ever saw. What happened:

1. `15:32:25.448` — the ingest watchdog force-exited the process (no DB write for
   99.8 s). 8th boot that day.
2. `15:32:26.698` — `boot::adopt_holdings` rebuilt the arm from PG: correct
   `entry_price`, **empty** metric track.
3. The cold-start seed (`live/src/seed.rs`) processes held mints **first** and pushes
   up to `SEED_TRADES_PER_MINT = 500` rows (48 h window) into `TokenState.trades`
   with `trades_base` still 0. This token's whole life — 221 trades back to
   15:26:05 — landed in the cache.
4. `producers.rs::on_trade` read `cursor = trade_cursor.get(mint).unwrap_or(trades_base)`
   — and `trade_cursor` is an in-RAM `HashMap` that a restart empties. So on the first
   live ping it emitted **all 221 historical trades as fresh `Event::Trade`s**.
5. The oldest replayed tick (3.514e-14 ⇒ `pnl −33%`) satisfied the desugared
   `pnl <= -28` req — which `arm::CompiledRule::compile` *prepends* to `exit_reqs`,
   so it wins the walk — and the sell executed against the live book at +79%.
   15 of the 221 replayed trades sat below the stop.

`reduce` evaluates `Event::Trade` at `trade.at`, so the decision was taken 5.5 minutes
in the past and filled in the present. Nothing in the rule was wrong.

## The four failure modes, all from one gap

The producer's cursor is process-local; the cache is durable-ish (PG-seeded). Nothing
reconciled the two, so a warm start could not tell history from signal.

1. **Stale exit** — the one above. Any position-scoped exit (`pnl`/TP/SL,
   `retrace`, `bounce`) fires on a historical price and fills at the live one.
2. **Stale entry** — a token re-armed by `recover_armed` replays its cached flow, so
   `can_enter` can be satisfied by a burst that ended before the restart. Narrower
   (re-arm only covers `MAX_SNIPE_AGE_SECS` of log), but it spends real SOL.
3. **Spurious `Dead`** — the replay walks history with `now = trade.at`, so a quiet
   stretch mid-history reads as `DEAD_QUIET_SECS` of silence at depleted reserves and
   books a death-close on a token that is alive right now.
4. **Frozen bag (the mirror image)** — an adopted position whose token *never* trades
   again gets no ping at all, so its track keeps `current_price() = NaN` and
   `current_reserves() = NaN` forever. `NaN` satisfies no condition, so `pnl`/TP/SL,
   the trail, **and** the dead-token verdict can never fire; only a `held` time-stop
   still works. Consistent with three paper bags left `Holding` and untouched since
   entry (one for 24 h) on the box.

Plus a silent state loss that is not a replay bug but shares the boot: an adopted
`EnteredCtx` re-seeds `peak_price = trough_price = entry_price`, so a **trailing stop
re-anchors to entry on every restart** and a bag can round-trip its entire run-up
before `retrace` reads anything again.

## The fix — one rail, both directions

`Producer` carries `started_at` (the loop's start instant) and splits every unseen
cached trade on it:

* `block_time < started_at` ⇒ **prime**: `hunter_engine::prime_trade` folds the trade
  into the track, ratchets each `Entered` arm's peak/trough **that the trade is not
  older than**, and advances the deadness clock. It returns nothing — no effects to
  discard, so no decision can leak.
* otherwise ⇒ **decide**: an `Event::Trade` through `reduce`, exactly as before.

Priming is *deferral*, not suppression: the 200 ms `Tick` re-evaluates every token
against the warm track and the wall clock, so a stop that is still true fires within
one tick — at the price it will actually fill at. That single rail closes all four
failure modes and restores the trailing peak (fix 5) as a side effect.

**The peak restore is bounded by `entered_at`, and must stay that way.** The seed
reaches back `SEED_TRADES_MAX_AGE_HOURS` — far past the fill of an adopted position —
so priming replays trades from *before* it entered. `peak`/`trough` are
**position-scoped** (they define `retrace` and `bounce`), so `fold_entered_extremes`
skips any arm whose `entered_at` is newer than the trade being folded. Without that
guard the restore over-corrects into the mirror bug: a dip-entry bag inherits the
run-up it deliberately did not buy, wakes up already deep in `retrace`, and stops out
on a high it never held — the fix-5 failure inverted. The guard is a no-op on the live
path (every event is newer than the fill by construction), so it costs one compare
per held arm and only ever changes the restart path. Locked by
`golden::primed_history_before_entry_never_inflates_the_peak`, the mirror of
`primed_history_restores_the_trailing_peak`.

`Producer::prime_tracked` runs the same path from the tick for every tracked mint with
no cursor yet — the case where no ping will ever arrive. It writes no cursor when the
mint is not in the cache, so it retries until the async seed lands (closing the boot
race the incident timeline shows: adopt at `26.698`, seed still running at `38.8`).

Two smaller rails came with it:

* `FirstSlotSettled` is now gated on the same snipe-freshness check as `TokenCreated`
  — after a restart, the first trade ping on a seeded token used to "settle" a
  creation slot that closed hours earlier.
* Primed trades are **not** written to the event log. The log is the decision stream
  the lab replays; logging an observation would make a replay re-decide precisely what
  priming exists to prevent (and the trades are already in PG).

## Invariants a future change must not break

* `prime_trade` returns `()`. Do not give it an `Effects` return "for symmetry" — the
  absence of a return value is what makes the no-decision guarantee structural.
* `fold_trade` is shared by the primed and the decided path. A primed trade and a live
  trade must leave the track in the same shape, or a warm start diverges from a cold one.
* The cursor is only written once the trades behind it have been primed **or** emitted.
  Writing it early (e.g. "skip the backlog") re-creates failure mode 4.
* `started_at` is the loop's start, not the process's: it must cover everything the seed
  can backfill.

Locked by `engine/tests/golden.rs::{primed_history_never_fires_an_exit,
primed_history_restores_the_trailing_peak}` and the `producers::tests` module
(`seeded_history_is_primed_not_decided`, `post_boot_trade_still_decides`,
`mixed_backlog_splits_at_started_at`, `prime_tracked_seeds_a_quiet_token_once`,
`prime_tracked_retries_until_the_cache_has_the_mint`,
`stale_token_does_not_emit_first_slot_settled`).

## Still open

The restarts themselves. The box took **8 boots on 2026-08-06**, most from
`Ingest watchdog: no successful DB write for ~90-100 s`. This fix makes a restart
harmless to open positions; it does not stop the restarts.
