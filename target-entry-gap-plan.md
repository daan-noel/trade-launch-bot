# Plan: Target (trigger-trade) vs. real Entry gap — TPSL2 only

## Goal
When a TPSL2 rule's scalp entry signal fires, persist the **trigger trade** as the
"target" point (`target_price`, `target_time`, `target_amount`, `target_tx`).
Later, when the real (or trusted candidate) fill lands, the existing `entry_*`
columns are filled as today. Storing both lets us derive the gap (price slippage /
time latency / size delta) between the targeted point and the actual entry.

- **"target" = the scalp-entry trigger trade tx** (the trade where `find_scalp_entry` first holds).
- `target_amount` = that trigger trade's SOL amount (`Trade.sol_amount`).
- `target_tx` = that trigger trade's signature.
- The gap itself is **not** stored now — it will be derived later from the stored columns.

## Scope
- **TPSL2 only.** Two position tables: `tpsl2_real_positions` and `tpsl2_paper_positions`.
- Backtest (in-memory, no table) and `tpsl2_paper_test_run` are **out of scope**.
- In **real** mode the target trade and the entry fill are different txs (a true gap).
  In **paper** mode the entry is **no longer** the trigger trade: it is the worst-case
  (highest-priced) trade in the trigger's block and the next block — see Step 7. So
  `target_*` and `entry_*` differ in paper too, except in the fallback case where the
  trigger is the only candidate.

---

## Step 1 — DB migration (new nullable columns)
- New migration file `backend/migrations/000X_tpsl2_target_columns.sql`.
- Add to **both** `tpsl2_real_positions` and `tpsl2_paper_positions`:
  - `target_price   DOUBLE PRECISION`
  - `target_amount  DOUBLE PRECISION`
  - `target_time    TIMESTAMPTZ`
  - `target_tx      TEXT`
- All **nullable** (no backfill): existing rows and not-yet-armed positions stay NULL.
- No UNIQUE constraint on `target_tx` (the trigger trade is someone else's trade and
  can repeat across positions).

## Step 2 — `Position` model ([backend/src/models/position.rs](backend/src/models/position.rs))
- Add fields: `target_price: Option<f64>`, `target_amount: Option<f64>`,
  `target_time: Option<DateTime<Utc>>`, `target_tx: Option<String>`.
- `Position::new(...)` keeps its current signature — initialize all four to `None`
  (target is set later, not at construction). This avoids churn at the other
  `Position::new` call sites (`exit/mod.rs`, tests).
- Add helper `set_target(&mut self, price, amount, time, tx)` that sets the four
  fields and bumps `updated_at`.

## Step 3 — `EntryFill` carries the trigger amount ([entry/mod.rs](backend/src/strategies/tpsl_sniper_2/entry/mod.rs), [entry/scalp.rs](backend/src/strategies/tpsl_sniper_2/entry/scalp.rs))
- Add `amount_sol: f64` to `EntryFill`.
- In `find_scalp_entry`, populate it from the candidate trade: `amount_sol: cand.sol_amount`
  (alongside the existing `price`/`tx_signature`/`block_time`).
- Update the `EntryFill { .. }` literal and any test constructors.

## Step 4 — Real positions repo ([tpsl2_position_repo.rs](backend/src/storage/repositories/tpsl2_position_repo.rs))
- Add the four `target_*` fields to `PositionDbRow` + its `TryFrom` mapping.
- Extend `insert` column list + binds (write current `Position.target_*`, normally NULL at insert).
- Extend `update` to carry `target_*` (so a full update doesn't wipe them), **or**
  leave `update` untouched and rely solely on a dedicated writer (preferred — see next).
- Add `update_target(position_id, price, amount, time, tx) -> Position` mirroring
  `update_entry` (writes the four columns + `updated_at`, `RETURNING ...` the row).
- Add `target_price, target_amount, target_time, target_tx` to **every** `SELECT`
  column list in this file (find_by_id, find_by_rule, find_holding_*, find_by_strategy,
  find_all_holding, update_entry's RETURNING, etc.).

## Step 5 — Paper positions repo ([tpsl2_paper_trading_repo.rs](backend/src/storage/repositories/tpsl2_paper_trading_repo.rs))
- Add the four `target_*` fields to `PaperPositionDbRow` + its `TryFrom`.
- Add columns to the shared `POSITION_COLS` constant and to `insert`.
- Add `update_target(...)` mirroring the real repo (or fold target into `update_entry` —
  see Step 7, since paper sets target and entry from the same trade).

## Step 6 — Real execution: capture & persist the target ([execution/real.rs](backend/src/strategies/tpsl_sniper_2/execution/real.rs))
- Change `await_scalp_entry_signal` to return `Option<EntryFill>` (the trigger trade)
  instead of `bool` — stop discarding the qualifying fill.
- In the spawned entry task in [service.rs](backend/src/strategies/tpsl_sniper_2/service.rs)
  (around the `await_scalp_entry_signal` call): when a signal is returned, call
  `position_repo.update_target(position_id, fill.price, fill.amount_sol, fill.block_time, fill.tx_signature)`
  **before** sending the buy, then `runtime.sync_position(...)` with the returned row.
- `None` (window elapsed) → drop the unentered position exactly as today (target stays NULL).
- `buy_until_filled_or_give_up` / `adopt_existing_fill_if_present` are unchanged — they
  still fill `entry_*` from the on-chain fill, leaving `target_*` intact.

## Step 7 — Paper execution: target = trigger trade, entry = worst-case fill ([execution/scalp.rs](backend/src/strategies/tpsl_sniper_2/entry/scalp.rs) + [execution/paper.rs](backend/src/strategies/tpsl_sniper_2/execution/paper.rs))

In paper mode the entry tx must **differ** from the target tx. The trigger trade is
the target; the entry is the worst-case adverse fill in the trigger's block and the
next block.

### 7a — Worst-case entry resolver (new pure fn, e.g. in `entry/scalp.rs`)

```rust
/// Paper worst-case entry. Given the mint's chronological trades and the trigger
/// trade `target` (slot S, leg_index L), choose the entry fill:
///   • candidate pool = trades in slot S or S+1, strictly after `target` by
///     (slot, leg_index), excluding the target tx, ANY trade type;
///   • drop dust (`Trade::is_dust(sol_amount)`) and `price_per_token <= 0`;
///   • pick the highest `price_per_token` (worst case); tie → latest by (slot, leg_index);
///   • empty pool → fall back to `target` itself (entry == target).
/// `amount_sol` of the returned entry = the TRIGGER trade's sol_amount.
fn find_worst_case_paper_entry(trades: &[Trade], target_tx: &str) -> EntryFill
```

- Look the target `Trade` up in `trades` by `tx_signature` to read its `slot`/`leg_index`
  and `sol_amount`.
- Note caveat: `leg_index` is index-within-tx, so `(slot, leg_index)` does not fully
  order two distinct txs in the same slot — implement the key as specified and accept
  ties resolved by the highest-price/latest rule.
- Unit-test: same-block-only, spanning S→S+1, dust/zero filtered out, tie→latest,
  empty→fallback equals target.

### 7b — Wire it into `spawn_entry_fill_poll`

- Where `find_scalp_entry` returns `target_fill`:
  - `paper_repo.update_target(position_id, target_fill.price, target_fill.amount_sol,
    target_fill.block_time, target_fill.tx_signature)` — the trigger trade.
  - `let entry = find_worst_case_paper_entry(&trades, &target_fill.tx_signature);`
  - `paper_repo.update_entry(position_id, &entry.tx_signature, entry.amount_sol,
    entry.price, entry.block_time)` — note `entry.amount_sol` is the trigger trade's SOL.
- `target_*` and `entry_*` are written from **different** trades (except the fallback).
- The existing "no scalp signal in the window → drop the position" path is unchanged
  (a resolved signal now always yields an entry via the fallback).

## Step 8 — Runtime cache ([runtime_cache.rs](backend/src/strategies/tpsl_sniper_2/runtime_cache.rs))
- `sync_position` takes a `&Position` snapshot, so the new fields ride along
  automatically. Verify it stores the whole `Position` (no field-by-field copy that
  would drop `target_*`); if it does copy fields, add the four.

## Step 9 — API response ([tpsl2_positions.rs](backend/src/api/handlers/strategies/tpsl2_positions.rs))
- Add `target_price`, `target_amount`, `target_time`, `target_tx` to `PositionResponse`
  and map them in `impl From<Position>`.
- (Optional, deferred) the derived gap can be computed here later; for now expose raw columns only.

## Step 10 — Frontend ([types/index.ts](frontend-react/src/types/index.ts), [tpsl2/tableColumns.tsx](frontend-react/src/components/tpsl2/tableColumns.tsx))
- Add `target_price`, `target_amount`, `target_time`, `target_tx` (nullable) to
  `RulePositionRecord` (and any other Position-shaped interface used by the TPSL2 tables).
- Add columns to the TPSL2 position table(s) in `tableColumns.tsx` rendering the four
  target values next to the existing entry columns (reuse `price.displayPrice` / `fmtTime`).
- Gap columns are derived later — not added now.

## Step 11 — Verify
- `cargo build` / `cargo clippy -p backend` (catch every `SELECT`/row-struct field list).
- Run the ignored DB integration tests in `execution/real.rs` against a local Postgres
  to confirm target is written on arming and entry still fills independently.
- Frontend `tsc` / build; visually confirm the new target columns render with real +
  paper TPSL2 rules.

---

## Notes / open choices
- Whether to fold `target_*` into the paper `update_entry` or add a separate
  `update_target` is an implementation detail (Step 5/7) — both are fine.
- `target_tx` is intentionally **not** unique.
- If "all 4 tables" actually meant additional tables beyond the two TPSL2 position
  tables, revisit Steps 1/4/5 before implementing.
