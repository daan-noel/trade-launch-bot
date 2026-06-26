# A1 — Concurrency caps ignored in real mode (Error 1)

> Workstream A (tpsl-realtime). Run **first** — it also reduces the trigger for
> [A2](02-buy-adoption-orphan-cleanup.md) and [A4](04-snipe-freshness-gate.md).
> Apply to **both** TPSL1 (`tpsl_sniper_1`) and TPSL2 (`tpsl_sniper_2`) — intentional clones.
> Paths are pre-crate-split — see [../README.md](../README.md#-path-caveat--line-refs-predate-the-crate-split).

## Report

Set `Max Concurrent Tokens = 2` and `Max Total Tokens = 2` on a TPSL1 rule, switched to
real mode. It bought 10+ tokens, ignoring both caps.

## Root cause — cap check and cap counter are out of sync across the real-buy latency window

- **Check:** [service.rs:237-257](../../backend/src/strategies/tpsl_sniper_1/service.rs#L237-L257)
  reads `holding_count_by_rule` / `total_count_by_rule`.
- **Counters only bump on fill:** in `sync_position`, the cap counters increment only when
  `entry_price.is_some()` —
  [runtime_cache.rs:722-741](../../backend/src/strategies/tpsl_sniper_1/runtime_cache.rs#L722-L741).
- **The "inline claim" doesn't claim a cap slot:** `sync_position(None, &position)` at
  [service.rs:308](../../backend/src/strategies/tpsl_sniper_1/service.rs#L308) runs on a fresh
  position with `entry_price = None`, so it only inserts into the *holding index* (exit-gating)
  and does **not** move the cap counters. The comment at service.rs:298-308 claims it reserves
  the slot — it does not.

Result: real buys take seconds to fill; until they do, every ping in a launch wave reads
count = 0, passes the cap, and submits a buy → far more than the cap.

**Why real-only:** paper mode fills the entry from the in-memory cache almost instantly, so
the counter bumps before the next ping. Real on-chain fill latency leaves the counter stale
and the cap wide open.

## Fix (recommended) — count in-flight positions against the cap

Add a reserved/in-flight counter:

1. Bump it **inline** at the claim ([service.rs:308](../../backend/src/strategies/tpsl_sniper_1/service.rs#L308)).
2. Release it on buy-fail rollback (service.rs:332-336 and 373-385).
3. Cap check sums `reserved + holding` (and `reserved + total` for the total cap).

**Alternative:** gate the cap check on the per-rule holding-index size, which already includes
`Arming` / `BuySubmitted` states ([position.rs:256-261](../../backend/src/models/position.rs#L256-L261)).

## Scope & done

- Mirror in **TPSL1 + TPSL2** (identical counter logic).
- `cargo check -p backend-deploy` clean; clippy on touched code; unit-test the cap arithmetic.
