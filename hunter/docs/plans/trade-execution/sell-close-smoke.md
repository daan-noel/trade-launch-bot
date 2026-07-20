# Sell / close real-SOL smoke checklist (Part 1)

Ops procedure for the Part 1 real-money paths shipped 2026-07-14
(`classify_sell_confirm` never re-sends on Succeeded/Pending; rent reclaim gated
by sibling open positions). Unit tests pin the classifier; this checklist is the
manual chain smoke — do **not** put real SOL in `cargo test`.

See also [execution-workflow.md](./execution-workflow.md) §C.

## Preflight (zero SOL)

```powershell
cargo run -p hunter-live -- probe simulate-sell <mint> …
cargo run -p hunter-live -- probe sweep-sell-dryrun …
cargo run -p hunter-live -- probe consolidate-dryrun …
cargo run -p hunter-live -- probe holdings
```

Confirm RPC + keys + feed look healthy before any real exit.

## Engine exit smoke (~0.01 SOL buy first, or use an existing tiny bag)

Prefer a **paper** exit through the Monitor / Live Trading UI first, then a
tiny **real** exit through the same engine path (row "Sell ALL" or Stop & close
— both fold `ManualClose` → `exec_real::run_exit`).

| # | Assert | Pass? |
|---|---|---|
| 1 | Sell lands; status → `End` (or `ExitUnconfirmed` if feed lag — **never** a second sell signature) | |
| 2 | On `Succeeded` / `Pending` mid-confirm, logs show extended feed-poll / `WaitConfirm`, not a re-submit | |
| 3 | With a **sibling** open position on the same mint, rent reclaim (`close_token_account`) is **skipped** | |
| 4 | After the **last** position on that mint clears, rent reclaim runs | |
| 5 | Toast / SSE: `strategy_position_update` frames arrive (`ExitPending` → terminal) | |

## Fail / escalate

- Second sell signature for the same position → treat as C1 regression; stop live trading.
- Reclaim on a mint that still has an open sibling → M1 gate regression.
- No SSE / toast for real exits → check `StrategyPositionUpdate` sink + notification prefs.

## Note on same-mint exit serialization

The 2026-07-14 audit table mentioned `runtime.mint_exit_lock`. That mutex is
**not** present today — serialization is per-`pg_id` via `InFlightGuards`. Two
positions on the same mint can exit concurrently; reclaim is still DB-gated.
Flag a follow-up only if concurrent same-mint exits prove unsafe in practice.
