# A real partial sell in flight at a restart

A restart while a **real** partial sell (a line's `sell_pct`) is in flight leaves the row
`ExitPending` with `extra.exit_pending_partial = true`. Boot adopts only `Holding` rows, so
the engine never sees it, and the reaper's `redrive_orphaned_exit_pending` re-drives it as
an orphan sell of the **whole remainder** (`spawn_orphan_sell`, reason `Recovery`). The
position exits in full instead of banking its share and walking the rest of its stages.
Money is not left unmanaged, but the rule's plan is cut short.

Paper is exact already: a paper sell in flight died with the process, so boot puts the row
back to `Holding` ([restart-state-restoration](../plans/strategies/restart-state-restoration.md)).

## Why real is not the same fix

Whether the partial landed is a fact on the chain. PG `trades` misses whatever printed
while the process (and its ingest) was down, so a PG-net check can read "not landed" for a
sell that landed during the downtime; reopening the row then lets the line fire again and
sell a second share.

## The exact recovery (needs a decision)

1. At boot, for each real `ExitPending` row with `exit_pending_partial`: read the token
   account balance once (`getTokenAccountBalance`, one Helius call per such row; these rows
   exist only when a restart hits a partial in flight).
2. Balance equals `entry_token_amount - sold_token_amount`: the sell did not land. Put the
   row back to `Holding`, adopt it at its stage; the line may fire again, which is the
   decision it was making.
3. Balance lower by about the partial's share: it landed. Book the leg from the chain
   (the `sell_backfill` path) with the stage it moves to (the line's `go`, which then also
   has to be stored with the pending flag), then adopt at that stage.
4. Anything else (a sell still pending on the chain at boot): leave it to the existing
   stale-`ExitPending` bag-check.

Helius spend needs explicit approval. Until then the full-remainder re-drive stands. No
active rule uses a partial sell today (local DB 2026-09-25: one paper rule, inactive).
