# First-slot fingerprint — deferred live entry gate

## Problem

`first_slot_buy_sol` / `first_slot_sell_sol` are streaming sums over same-creation-slot trades. At `TokenCreated` they are indistinguishable from "really zero" vs "not measured yet." Evaluating them inline in `on_token_created` would reject every live token while backtest/simulate (full history already collected) would work — a silent live/backtest parity bug.

## Design

Two-phase entry gate, strategy-agnostic (tpsl1 / tpsl2 / swing1):

1. **Instant pass** — `StrategyImpl::matches_instant_entry` runs creation-time fingerprint axes only.
2. **Queue** — if `StrategyParams::requires_first_slot_data()` and instant pass, register `(mint, rule_id)` in `pending_first_slot` instead of buying.
3. **Resolve** — on window close (`on_trade_executed`, `!first_slot_window_open`) or 5s backstop (`sweep_first_slot_pending` on the 1s runner tick), enrich `Token` with accumulated totals and run full `matches_entry` → normal entry path.

## Pending set

Mirrors `until_dead_armers`:

- `DashMap<(String, Uuid), PendingFirstSlotEntry>` on `StrategyRuntimeCache`
- Cap `MAX_FIRST_SLOT_PENDING = 32`, evict oldest on overflow
- One-shot: `take_first_slot_pending` / `expire_first_slot_pending` remove entries; no retry

## Analysis path

No deferral. `TokenRepo::find_page_before` LEFT JOINs `tokens_info`; `Token.first_slot_buy_sol` / `first_slot_sell_sol` populated at scan time. Shared SSOT matcher: `token_matches_buy_rule`.

## Constants

- `FIRST_SLOT_GATE_TIMEOUT_SECS = 5` — generous backstop vs sub-second typical same-slot trade cadence
- `MAX_FIRST_SLOT_PENDING = 32` — same order of magnitude as `MAX_UNTIL_DEAD_ARMERS`
