# Execution latency — `target_slot` / `entry_slot` / `exit_slot`

How long it takes to go from *deciding* to *filled*, measured in the only unit a
bonding curve moves in. Columns land in `strategy_positions` via core migration
`0004_position_slots.sql`.

## Why slots and not milliseconds

A curve price changes when a trade **lands**, and trades land in slots. Wall clock
cannot express that: `trades.block_time` is the ingest clock, so a timestamp delta
measures how fast this process saw the feed, never how many slots of price movement
happened in between. `entry_slot - target_slot` is the number that matters, and
nothing else stored today produces it.

The three columns pair with the snapshots already on the row:

| | what it is | source |
| --- | --- | --- |
| `target_*` | the trigger trade that armed the entry — what we knew | `TargetSnapshot`, from `CachedTrade` |
| `entry_*` | the buy that landed — what we got | `SigLegs::first_slot` |
| `exit_*` | the sell that landed | `SigLegs::last_slot` |

## NULL is a real answer

A slot is a **measured** quantity, so absence is `NULL`, never a `0` sentinel — `0`
is a valid slot number. Three cases produce it, and each is honest:

* **A paper fill has no `entry_slot`/`exit_slot`.** The fill is simulated; it never
  lands in a slot. Borrowing the trigger's slot would fabricate a zero-latency
  reading, which is worse than having none. `target_slot` IS recorded in paper —
  the trigger is a real print off the feed.
* **An externally-cleared bag has no `exit_slot`.** It is reconstructed from wallet
  net, not observed landing, so no leg of ours carries a slot.
* **A leg with no slot** stays `None` rather than defaulting, unlike the block-time
  fields which fall back to `now()`.

Consequence: **paper cannot validate latency.** It measures the paper fill model
against its own input. Only real fills answer the question.

## Both modes record the target

Paper resolves its trigger inside the fill loop — it has to, because the worst-case
fill is chosen relative to it. Real submits immediately and learns its fill slot
only on confirm, so `dispatch_buy` captures the trigger before the submit; that is
the last moment the deciding trade is identifiable. Both paths write
`PositionMeta::target_snapshot`, and one sink persists it.

The struct is `TargetSnapshot`, not `PaperTarget`: the old name read as "real does
not do this", which is exactly the wrong inference to invite.

## The slot rides the side-channel, not the event

`FillSigs` carries the fill slot from executor to sink, alongside the signatures and
token account. It does **not** go on `Event::FillConfirmed`'s `Fill`.

`hunter_engine::reduce` is the pure decision fold, and no decision reads a slot.
Putting one on the event would widen the kernel's input for a bookkeeping value —
the same reason `tx_signature` and `token_account` already travel this way.

## The preview path carries slots too

`ObservedLegs` (the process-local own-leg preview, `trade_signals.rs`) carries
`first_slot`/`last_slot`, not just the PG path. That preview is the **fast** confirm
— a snipe usually resolves from it before PG commits. Without slots there, exactly
the fastest fills would record `entry_slot = NULL` and the histogram would describe
only the slow ones, biasing the measurement toward the answer it is meant to test.

## Reading it

```sql
SELECT entry_slot - target_slot AS slots_late, COUNT(*)
FROM strategy_positions
WHERE mode = 'real' AND target_slot IS NOT NULL AND entry_slot IS NOT NULL
GROUP BY 1 ORDER BY 1;
```

`idx_strategy_positions_latency` is partial on exactly this predicate, so rows that
can never answer stay out of the index.

Two uses:

1. **Calibrate a backtest.** A sim that fills at the signal trade's own price is
   look-ahead — not optimism, an impossibility. This histogram says which fill lag
   to charge instead.
2. **Falsify the paper fill model.** The paper worst-case adverse slippage is
   currently unfalsifiable. Real slot deltas make it checkable, and every backtest
   inherits that model — so this is worth more than the latency answer alone.

A rule whose edge is flat across `slots_late` is a thesis; one whose edge decays
steeply is partly a latency bet. Rank accordingly when the distribution is unknown.
