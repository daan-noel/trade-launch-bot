# Plan: End-to-end SOL/lamports naming + typing clarity

## Goal (locked rule)

Every field/column/variable that denotes an amount of SOL must make its unit
unambiguous from its name alone, with **zero exceptions**, front-to-back:

1. **Lamports fields** end in **`_lamports`** and are always an exact integer
   (`BIGINT` in Postgres, `u64`/`i64` in Rust, `number` in TS — always a whole
   lamport count, never a fraction). If the old name contained the word `sol`,
   that word is removed (e.g. `reserve_sol` → `reserve_lamports`, not
   `reserve_sol_lamports`).
2. **SOL fields** end in **`_sol`** and are always a float (`DOUBLE
   PRECISION`/`REAL` in Postgres, `f64` in Rust, `number` in TS).
3. Same base concept, same base name, unit-only suffix differs across layers —
   e.g. the DB stores `entry_lamports` (exact), the Rust model exposes
   `entry_sol` (human-readable) converted from it. This is already the
   established pattern for `MIN_TRADE_LAMPORTS`/`MIN_TRADE_SOL`
   (`trading_core/src/config/constants/tuning.rs:97-98`) and for
   `PositionsSummaryRow`'s `total_pnl_lamports` → `PositionsSummary.total_pnl_sol`
   (`strategy_repo.rs:266-284` vs `models/strategy.rs:111-120`) — we're
   generalizing a pattern that already exists in the best parts of the
   codebase, not inventing a new one.
4. **Out of scope, deliberately**: ratio/rate fields that aren't "an amount of
   SOL" at all — `entry_price`/`exit_price`/`target_price`/`current_price`/
   `ath_price`/`price_per_token` (SOL **per raw token unit**), `pnl_percent`/
   `pnl_pct` (a ratio), `cu_price` (micro-lamports **per compute unit**). Forcing
   `_sol`/`_lamports` onto these would actively mislead (they're not amounts),
   so they keep their current `_price`/`_pct` naming. Raw token-unit fields
   (`token_amount`, `reserve_token`, `*_token_amount`) are untouched — not a
   SOL/lamports concern at all.

This is a rename-heavy, mostly non-semantic change: **~95% of the work is
renaming an already-correctly-typed field**, not converting units. The DB
audit found only two fields with no unit suffix at all today (`tokens_info.
volume`, `strategy_rules.buy_amount`) — everything else already has the right
*type*, just an inconsistent or absent unit *suffix*, or a `_sol`-looking name
on a lamports column (the exact class of bug that caused the `find_tx_by_fill`
mismatch fixed earlier this session).

## Why this is worth the effort (context for future readers of this file)

The two full-workspace audits behind this plan found the two-tier design
(DB = lamports for ledger columns, Rust/wire = human SOL) is *already mostly
correct* — but a handful of fields keep an `_sol`-shaped name while actually
holding lamports (`trades.reserve_sol`, `strategy_positions.entry_sol`/
`exit_sol`, `tokens.initial_buy_sol`, `tokens_info.first_slot_buy_sol`/
`first_slot_sell_sol`, `tokens.initial_buy_instruction->>'max_sol_cost'`/
`'spendable_sol_in'`), and this exact ambiguity is what caused the very bug
fixed earlier this session (`trade_repo::find_tx_by_fill` compared a
lamports-scale SQL expression against a human-SOL-scale Rust value). A strict,
literal, no-exceptions naming rule makes that class of bug impossible to write
without it being visually obvious in a diff.

## Non-goals

- **Not** converting the DB's exact-integer ledger columns to floats, or vice
  versa — see rule 1/2 above, this is a naming/consistency pass, not a
  precision redesign.
- **Not** unifying `max_cost_lamports`/`spendable_lamports_in` (token wire,
  lamports — verbatim on-chain instruction args) with `p_token_max_sol_cost`/
  `p_token_spendable_sol_in` (rule params, human SOL) into one unit. They stay
  dual-unit by design (one is a literal on-chain arg snapshot, the other a
  user-facing filter threshold) — the rename just makes that split
  self-documenting instead of a silent footgun.
- **Not** touching USD fields (`usd_rate`, `value_usd`, `price_usd`) — separate
  currency, not in scope.

---

## Master rename table

Legend: **L** = DB/Postgres, **R** = Rust, **W** = wire (HTTP/SSE JSON —
inherits the Rust field name via serde, no separate rename), **F** = frontend
TS. "—" = no change needed at that layer (already compliant or not present).

### `trades` (+ `trades_priced` view)

| Concept | Layer | Current | Type | → New | Type |
|---|---|---|---|---|---|
| Trade's SOL leg | L | `trades.sol_amount` | `BIGINT` | `trades.amount_lamports` | `BIGINT` |
| Trade's SOL leg | R | `Trade.sol_amount`, `TradeRow::sol_amount()` | `f64` | `Trade.amount_sol`, `TradeRow::amount_sol()` | `f64` |
| Curve/pool SOL reserve | L | `trades.reserve_sol` | `BIGINT` | `trades.reserve_lamports` | `BIGINT` |
| Curve/pool SOL reserve | R | `Trade.reserve_sol`, `TradeRow::reserve_sol()` | `f64` | — (already compliant) | — |
| Real (non-virtual) SOL reserve | R only (not persisted) | `Trade.real_sol_reserves`, `TradeRow::real_sol_reserves()` | `f64` | `Trade.real_reserve_sol`, `TradeRow::real_reserve_sol()` | `f64` |
| View's derived ratio | L (view) | `trades_priced.price_per_token` (= lamports/token, **mislabeled** — see Decision D1) | `double precision` | `trades_priced.price_per_token` (fixed to = SOL/token) | `double precision` |

`CachedTrade` (`trading_core/src/state/token_cache.rs`) and `SweepTrade`
(`lab/src/sweep/projection.rs`) both implement `TradeRow` and carry their own
`price_per_token`/reserve fields mirroring `Trade` — same renames apply
(`price_per_token` stays, it's a ratio; any `sol_amount`-named field renames to
`amount_sol`).

### `tokens` / `tokens_info`

| Concept | Layer | Current | Type | → New | Type |
|---|---|---|---|---|---|
| Creator's first buy | L | `tokens.initial_buy_sol` | `BIGINT` | `tokens.initial_buy_lamports` | `BIGINT` |
| Creator's first buy | R/W | `Token.initial_buy_sol` | `f64` | — (already compliant) | — |
| Creation-ix ceiling | L (JSONB key) | `initial_buy_instruction->>'max_sol_cost'` | lamports | `->>'max_cost_lamports'` | lamports |
| Creation-ix ceiling | R/W | `TokenFingerprint.max_sol_cost`, `TokenSummary.max_sol_cost` | `i64`/`u64` | `max_cost_lamports` | `i64`/`u64` |
| Creation-ix spendable | L (JSONB key) | `initial_buy_instruction->>'spendable_sol_in'` | lamports | `->>'spendable_lamports_in'` | lamports |
| Creation-ix spendable | R/W | `TokenFingerprint.spendable_sol_in`, `TokenSummary.spendable_sol_in` | `i64`/`u64` | `spendable_lamports_in` | `i64`/`u64` |
| Cumulative trade volume | L | `tokens_info.volume` (no suffix at all today) | `DOUBLE PRECISION` | `tokens_info.volume_sol` | `DOUBLE PRECISION` |
| Cumulative trade volume | R/W | `TokenInfo.volume`, `TokenSummary.volume_sol_total` | `f64` | `TokenInfo.volume_sol` (wire field `volume_sol_total` already compliant, unchanged) | `f64` |
| First-slot buy total | L | `tokens_info.first_slot_buy_sol` | `BIGINT` | `tokens_info.first_slot_buy_lamports` | `BIGINT` |
| First-slot buy total | R/W | `TokenInfo.first_slot_buy_sol` | `f64` | — (already compliant) | — |
| First-slot sell total | L | `tokens_info.first_slot_sell_sol` | `BIGINT` | `tokens_info.first_slot_sell_lamports` | `BIGINT` |
| First-slot sell total | R/W | `TokenInfo.first_slot_sell_sol` | `f64` | — (already compliant) | — |

### `strategy_rules` / `strategy_run_metrics` / `strategy_positions`

| Concept | Layer | Current | Type | → New | Type |
|---|---|---|---|---|---|
| Rule's buy size | L | `strategy_rules.buy_amount` (no suffix) | `DOUBLE PRECISION` | `strategy_rules.buy_amount_sol` | `DOUBLE PRECISION` |
| Rule's buy size | R/W | `StrategyRule.buy_amount` | `f64` | `StrategyRule.buy_amount_sol` | `f64` |
| Rule params (JSONB) | L/R/W | `p_token_initial_buy_sol`, `p_token_max_sol_cost` *(human SOL, not the on-chain arg — different concept from `max_cost_lamports` above)*, `p_token_spendable_sol_in`, `p_entry_min_alive_sol`, `p_entry_min_organic_sol`, `p_entry_min_liquidity_sol`, `p_swing_high_to_low_sol`, `p_swing_low_to_high_sol`, `high_to_low_threshold_sol`, `low_to_high_threshold_sol`, `big_tx_sol` | `f64` | — (already compliant, no change) | — |
| Run PnL rollup | L/R/W | `strategy_run_metrics.total_pnl_sol`, `.expectancy_sol` | `REAL`/`f32` | — (already compliant) | — |
| Position entry spend | L | `strategy_positions.entry_sol` | `BIGINT` | `strategy_positions.entry_lamports` | `BIGINT` |
| Position entry spend | R/W | `StrategyPosition.entry_sol` | `f64` | — (already compliant) | — |
| Position exit proceeds | L | `strategy_positions.exit_sol` | `BIGINT` | `strategy_positions.exit_lamports` | `BIGINT` |
| Position exit proceeds | R/W | `StrategyPosition.exit_sol` | `f64` | — (already compliant) | — |
| Summary aggregates | R/W | `PositionsSummary.total_pnl_sol/total_entry_sol/total_holding_sol/total_gains_sol/total_losses_sol` | `f64` | — (already compliant) | — |
| Summary raw sums (private) | R | `PositionsSummaryRow.total_pnl_lamports/total_entry_lamports/total_holding_lamports/total_gains_lamports/total_losses_lamports`, `RuleCountersRow.total_pnl_lamports` | `i64` | — (already compliant — this struct is the model to copy) | — |
| PnL view | L (view) | `strategy_position_pnl.realized_pnl_sol` | `float8` (derived) | — (already compliant) | — |

### `lab` grouped-sweep tables (`tpsl1`/`tpsl2`/`swing1` variants)

All already compliant — **zero DB changes needed**: `buy_amount_sol` (`REAL`),
`total_pnl_sol` (`REAL`), `expectancy_sol` (`REAL`), `best_expectancy_sol`
(`DOUBLE PRECISION`). Verify `grouped_sweep_repo.rs` / `lab/src/sweep/**` don't
independently reference the *old* `tokens`/`tokens_info` column names when
joining for fingerprint enrichment (they will, per the rename above, and need
updating in step with `trading_core`).

### Cashback (not DB-backed — computed live from on-chain WSOL balance)

Already fully compliant, **zero changes**: `claimable_lamports`,
`total_claimable_lamports`, `claimed_lamports` (all `u64`, correct suffix).
`stable_claimable` is a raw SPL-token-unit count for a non-SOL mint, not a
lamports/SOL concern — left as is.

### `pump-trader` (real-money execution — lowest priority, see Decision D3)

Already ~95% compliant: `max_buy_sol`, `min_sol`, `max_sol`,
`MIN_JITO_TIP_SOL`, `MAX_JITO_TIP_SOL`, `buy_lamports`, `committed_lamports`
all already follow the rule. One straggler:

| Concept | Current | Type | → New | Type |
|---|---|---|---|---|
| Cached wallet balance | `PumpFunTrader.sol_balance_cache` | `Arc<Mutex<Option<(u64, Instant)>>>` | `PumpFunTrader.balance_lamports_cache` | same |

### Frontend (`frontend-react/src`) — mirrors every wire rename above 1:1

Every backend rename that reaches the wire (i.e. every row above marked
**W**) needs the identical rename in:

- `shared/types/index.ts` (`TokenRecord`, `TradeRecord`/`LiveTrade`,
  `RuleRecord`, `RulePositionRecord`, `PositionsSummary`, `SimulatedTokenResult`)
- `lab/components/sweep/groupedTypes.ts`, `lab/components/sweep/types.ts`
- `shared/components/token-price-chart/types.ts`
- Every display component currently reading the old field name — the
  representative ones both audits already found: `sharedTokenColumns.tsx`,
  `tokenColumns.tsx`, `filters.ts`, `FingerprintGroupPicker.tsx` (all four for
  `max_sol_cost`/`spendable_sol_in` → `max_cost_lamports`/`spendable_lamports_in`,
  **keep the existing `÷1e9` display conversion**, it's already correct, only
  the field name changes), `tokenTradeColumns.tsx`, `live/components/
  transactions/tradeColumns.tsx`, `token-price-chart/chartBars.ts`,
  `token-price-chart/WalletMarkersTooltip.tsx` (all for `sol_amount` →
  `amount_sol`), `tpsl1/tableColumns.tsx`, `tpsl2/tableColumns.tsx`,
  `tpsl2/ruleColumns.tsx`, `tpsl1/ruleColumns.tsx`, `SimSummaryCard.tsx`
- Form specs binding to the renamed top-level rule column:
  `shared/lib/params/specs/tpsl1.ts`/`tpsl2.ts`/`swing1.ts` — the
  `column: 'buy_amount'` field spec → `column: 'buy_amount_sol'` (the `p_*`
  spec entries are unaffected, already compliant)
- `lab/components/sweep/SweepConfigForm.tsx`, `SelectedSweepHistory.tsx` —
  `buy_amount_sol` is **already** the sweep's name (no change there); double
  check no sweep code still special-cases the live-rule's old `buy_amount`
  key when mapping between the two.

No `transformResponse` unit-scaling exists anywhere in `frontend-react` today
— renames are pure identifier changes on both sides of the wire, not new
conversion logic.

---

## Migrations

New, additive migrations (never edit `0001_init.sql`/`0001_grouped_sweep.sql`
in place — same locked convention as `cohort-removal-plan.md`).

### `trading_core/migrations/0009_sol_lamports_naming.sql`

```sql
-- trades
ALTER TABLE trades RENAME COLUMN sol_amount  TO amount_lamports;
ALTER TABLE trades RENAME COLUMN reserve_sol TO reserve_lamports;

-- tokens
ALTER TABLE tokens RENAME COLUMN initial_buy_sol TO initial_buy_lamports;

-- tokens.initial_buy_instruction JSONB keys (mirrors migration 0006's approach)
UPDATE tokens
SET initial_buy_instruction =
    (initial_buy_instruction - 'max_sol_cost' - 'spendable_sol_in')
    || jsonb_strip_nulls(jsonb_build_object(
        'max_cost_lamports',    initial_buy_instruction->'max_sol_cost',
        'spendable_lamports_in', initial_buy_instruction->'spendable_sol_in'
    ))
WHERE initial_buy_instruction ?| array['max_sol_cost', 'spendable_sol_in'];

-- tokens_info
ALTER TABLE tokens_info RENAME COLUMN volume            TO volume_sol;
ALTER TABLE tokens_info RENAME COLUMN first_slot_buy_sol  TO first_slot_buy_lamports;
ALTER TABLE tokens_info RENAME COLUMN first_slot_sell_sol TO first_slot_sell_lamports;

-- strategy_rules
ALTER TABLE strategy_rules RENAME COLUMN buy_amount TO buy_amount_sol;

-- strategy_positions
ALTER TABLE strategy_positions RENAME COLUMN entry_sol TO entry_lamports;
ALTER TABLE strategy_positions RENAME COLUMN exit_sol  TO exit_lamports;

-- Rebuild views that reference renamed columns (CREATE OR REPLACE — same
-- physical view, updated column refs). Also fixes trades_priced's
-- price_per_token to be SOL/token (was lamports/token, see Decision D1).
CREATE OR REPLACE VIEW trades_priced AS
SELECT
    t.*,
    (t.amount_lamports::double precision / 1e9 / NULLIF(t.token_amount, 0)) AS price_per_token
FROM trades t;

CREATE OR REPLACE VIEW strategy_position_pnl AS
SELECT
    p.*,
    ((p.exit_lamports - p.entry_lamports)::float8 / 1e9) AS realized_pnl_sol
FROM strategy_positions p
WHERE p.exit_lamports IS NOT NULL AND p.entry_lamports IS NOT NULL;
```

(Confirm the exact current `strategy_position_pnl` view predicate/column list
against `0001_init.sql:365-372` before writing the real migration — the
snippet above is the shape, not a verbatim copy.)

No `lab/migrations` changes needed (schema already compliant).

Indexes named after old columns (e.g. `idx_strategy_run_metrics_pnl` — already
fine, references `total_pnl_sol` which isn't renamed; check for any index
literally named `..._sol_amount_...` etc. and rename the index too for
consistency, though this is cosmetic and won't break anything if skipped).

---

## Backend code changes (by crate)

### `trading_core`

- **Models**: `models/trade.rs` (`Trade.sol_amount`→`amount_sol`,
  `Trade.real_sol_reserves`→`real_reserve_sol`, `TradeRow` trait methods to
  match), `models/token_info.rs` (`TokenInfo.volume`→`volume_sol`),
  `models/strategy.rs` (`StrategyRule.buy_amount`→`buy_amount_sol`),
  `models/tpsl1_strategy_rule.rs`/`tpsl2_strategy_rule.rs` (no field renames,
  but constructors/call sites that pass `rule.buy_amount` need updating),
  `grouping.rs` (`TokenFingerprint.max_sol_cost`→`max_cost_lamports`,
  `.spendable_sol_in`→`spendable_lamports_in`, `GroupField` enum variants +
  their `as_str()`/`from_str()` string mappings + `extract_lamports` call sites).
- **Repos**: `storage/repositories/trade_repo.rs` (`TradeDbRow` fields, all
  raw SQL column lists/aggregates, `find_tx_by_fill`, `price_of`/
  `lamports_to_sol`/`sol_to_lamports` call sites), `token_repo.rs` (SQL
  `SELECT`/`WHERE`/sort-key maps for `initial_buy_lamports`, `volume_sol`,
  `first_slot_*_lamports`, JSONB key refs for `max_cost_lamports`/
  `spendable_lamports_in`), `token_info_repo.rs` (same), `strategy_repo.rs`
  (`StrategyPositionDbRow`, `RULE_COLS`/`POSITION_COLS`/`POSITION_COLS_SP`
  consts, `position_sort_sql`/`position_filter_sql` maps, aggregate SQL text
  in `positions_summary`/`rule_counters_for_latest_paper_runs`).
- **Consolidate the four duplicated `lamports_to_sol`/`sol_to_lamports`
  copies** (`trade_repo.rs:844-855`, `token_repo.rs:158-165`,
  `token_info_repo.rs:89-96`, `strategy_repo.rs:17-24`) into one shared
  `trading_core::storage::units` module. Not strictly required for the rename,
  but this is the natural moment to kill the drift risk the audit flagged —
  do it in the same pass.
- **API handlers**: `api/handlers/tokens/tokens.rs` (`TokenSummary` DTO fields
  + `sql.rs`'s sort/filter maps + the in-RAM filter that currently divides
  `max_sol_cost`/`spendable_sol_in` by 1e9 — keep the ÷1e9, rename the field),
  `api/handlers/system/stream.rs` (`SseEvent` JSON field emission),
  `models/ingest.rs` (`SseEvent::TradeExecuted`/`LiquidityAdded`/`LiquidityRemoved`
  `.sol_amount`→`.amount_sol`; `RuleNotifSnapshot` unaffected, already
  compliant).
- **Ingest decode**: `live/src/ingest/consumer.rs` (`sol_amount`
  local/field→`amount_sol`), `ingest-laserstream/src/decode/trade.rs`
  (`DecodedTradeEvent` field if named `sol`/`sol_amount`).

### `live`

- `live/src/strategies/execution/{real,paper}.rs`, `service.rs`: every
  `.sol_amount`/`.entry_sol`/`.exit_sol`/`.buy_amount` field access on `Trade`/
  `StrategyRule` updated to the new names (`cargo check` will find every one —
  see Verification below).
- `live/src/api/handlers/strategies/{positions,rules}.rs`,
  `live/src/api/handlers/trading/solana.rs`: DTO field renames matching the
  model renames above.
- `live/src/api/handlers/trading/cashback.rs`: no change (already compliant).

### `lab`

- `lab/src/sweep/**` (`aggregate.rs`, `corpus.rs`, `engine.rs`,
  `grouped_engine.rs`, `projection.rs`, `strategies/{tpsl1,tpsl2,swing1}.rs`):
  `SweepTrade.sol_amount`→`amount_sol` (or whatever `TradeRow` renames to),
  every backtest P&L calc reading `.sol_amount`.
  `lab/src/lake/duck.rs` (`price_per_token: price` — ratio, unaffected).
- `lab/src/api/handlers/strategies/{tpsl1,tpsl2,swing1,grouped_sweep}.rs`: DTO
  renames.
- `lab/src/storage/repositories/grouped_sweep_repo.rs`: update any joined
  `tokens`/`tokens_info` column references.

### `pump-trader` (Decision D3 — do last, lowest priority)

- `trader/mod.rs`: `sol_balance_cache`→`balance_lamports_cache` + its
  accessor `update_sol_balance_cache`.

### `ingest-laserstream` / `ingest-websocket`

- Grep for `sol_amount`/`sol:` fields in decode structs; rename to match
  `trading_core`'s `Trade.amount_sol` so the ingest→core boundary uses
  identical field names (currently already true structurally, just needs the
  same rename applied).

---

## Frontend changes

1. Rename every TS interface field per the Master rename table above.
2. For the two lamports-wire fields (`max_cost_lamports`/
   `spendable_lamports_in`), **keep the existing `÷1e9` conversions** in
   `sharedTokenColumns.tsx`/`tokenColumns.tsx`/`filters.ts` — only the
   property name read changes, not the math.
3. For every `sol_amount`→`amount_sol` rename, it's a pure identifier swap —
   no formatting logic changes (`AmountCell`/`PriceCell` already treat the
   value as human SOL).
4. `shared/lib/params/specs/tpsl1.ts`/`tpsl2.ts`/`swing1.ts`: update the
   `column: 'buy_amount'` → `'buy_amount_sol'` field spec entry (the label
   `"Buy Amount (SOL)"` stays).
5. Run `npm run build` (checks **both** trees per `CLAUDE.md`) — `tsc` will
   fail on every stale reference, same safety net as `cargo check` on the
   backend.

---

## Sequencing (each step must compile/typecheck before the next)

1. **DB migration** (`0009_sol_lamports_naming.sql`) — apply locally, confirm
   `sqlx migrate run` succeeds and manually spot-check a few rows/views.
2. **`trading_core`**: models → repos → grouping.rs → API handlers → ingest
   decode structs, in that order (inner-to-outer, so each step's compile
   errors point only at the next outer layer). `cargo check -p trading_core`
   clean.
3. **`live`**: strategies/execution/service → API handlers. `cargo check -p
   live` clean.
4. **`lab`**: sweep engine → API handlers → repo. `cargo check -p lab` clean.
5. **`pump-trader`**: the one internal rename. `cargo check -p pump-trader`
   clean.
6. **`ingest-laserstream`/`ingest-websocket`**: align decode field names.
   `cargo check` on both clean.
7. **Frontend**: types → RTK endpoints (no change expected, verify) →
   components → form specs. `npm run build` clean.
8. **Full workspace**: `cargo check` on every crate + `cargo test -p
   trading_core -p live -p lab -p pump-trader` (unit tests reference old field
   names too — the compiler will find them) + `npm run build`.
9. **Docs**: update `@arch/database.md` (column names), `@arch/frontend.md`
   (wire field names) if either enumerates the renamed fields, and add a short
   "SOL vs lamports naming rule" paragraph to `CLAUDE.md` so this doesn't
   regress on the next new column.

## Verification strategy

- **Rust**: renaming a struct field makes every stale reference a compile
  error — `cargo check -p trading_core -p live -p lab -p pump-trader
  -p ingest-laserstream -p ingest-websocket` after each phase is the primary
  safety net, not optional grepping.
- **TypeScript**: same story via `npm run build` (checks both `live`/`lab`
  trees).
- **What the compiler *won't* catch** — grep explicitly for these after the
  rename, since they're string literals, not typed references:
  - Raw SQL text (column names inside `sqlx::query`/`query_scalar` string
    literals) — `rg -i "sol_amount|reserve_sol\b|initial_buy_sol\b|first_slot_(buy|sell)_sol|entry_sol\b|exit_sol\b|\bvolume\b.*tokens_info|buy_amount\b|max_sol_cost|spendable_sol_in" --type rust`
  - JSONB key string literals (`'max_sol_cost'`, `'spendable_sol_in'`) in both
    Rust and the migration.
  - Frontend string-keyed column maps (`sort`/`filter` key tables, `DataTable`
    column `key:` props) that route through a generic `Record<string, ...>`
    rather than a typed interface field access.
  - Test fixtures/JSON fixtures (`lab/**/*.json`, integration test literals)
    hardcoding old field names.
- **Manual smoke test** after everything compiles: create a paper `tpsl1`
  rule, let it fire and close a position, confirm `entry_lamports`/
  `exit_lamports` land correctly in the DB and the frontend positions table +
  `SimSummaryCard` render the right PnL; load the tokens list and confirm
  `max_cost_lamports`/`spendable_lamports_in` filter/sort/display still work
  post-rename.

## Risk / rollback

- Every DB change is a `RENAME COLUMN` (or a `CREATE OR REPLACE VIEW`) — no
  data is transformed except the one `initial_buy_instruction` JSONB key
  rewrite, which follows the exact, already-proven pattern of migration
  `0006_snake_case_buy_ix_keys.sql`. Trivially reversible pre-deploy
  (`RENAME COLUMN ... TO ...` back) if something is missed.
- This is a **wire-breaking change** (JSON field names change), but both bins
  (`live`, `lab`) and the frontend are built and deployed together from one
  repo with no third-party API consumers — ship as one atomic
  commit/deploy, no versioning/compat shim needed.
- Main residual risk is a missed **string-literal** reference (raw SQL, JSONB
  keys, frontend string-keyed column maps) that the compiler can't catch —
  mitigated by the explicit grep pass in Verification above, run *after*
  `cargo check`/`npm run build` are both clean.

## Locked decisions

- **D1 — `trades_priced.price_per_token` math bug**: fixed as part of this
  pass (divide by `1e9` so the view's `price_per_token` actually means
  SOL/token, matching every other `price_per_token` in the codebase), since
  leaving a "SOL per token" -named column silently computing lamports/token
  is exactly the ambiguity this whole effort exists to kill. No code currently
  queries this view directly, so this is zero-risk.
- **D2 — dual units for the same concept** (`max_cost_lamports` wire vs
  `p_token_max_sol_cost` rule param): kept as two units by design (see
  Non-goals). The rename alone removes the silent-footgun risk; unifying them
  is a separate, bigger product decision not part of this effort.
- **D3 — `pump-trader` polish is lowest priority**: it's already ~95%
  compliant and touches no DB/wire contract, so it's sequenced last and can be
  dropped from scope entirely without weakening the rest of this plan.
- **D4 — `cu_price` and all `_price`/`_pct` ratio fields are out of scope**:
  see Non-goals; forcing a `_sol`/`_lamports` suffix onto a rate/ratio field
  would be actively misleading, not clarifying.
- **D5 — naming direction is suffix, not prefix**: `sol_amount`/`entry_sol`
  read naturally as prefix-style already, but the locked rule is a strict
  trailing suffix per the request, so `sol_amount` → `amount_sol` (not
  `sol_amount` kept as-is) despite reading slightly less naturally next to
  `token_amount` (which is untouched — it contains neither `sol` nor
  `lamport`, so the rule doesn't apply to it, and the resulting asymmetry is
  cosmetic, not ambiguous).

## Effort estimate (rough, for sequencing/scheduling only)

| Phase | Size | Why |
|---|---|---|
| DB migration | Small | ~10 `RENAME COLUMN` + 1 JSONB rewrite + 2 view rebuilds |
| `trading_core` models+repos | **Large** | `sol_amount`→`amount_sol` alone touches `Trade`/`TradeRow`/`CachedTrade` and is read across nearly every strategy/entry/exit module |
| `trading_core` API/grouping | Medium | Concentrated in `tokens.rs`, `sql.rs`, `grouping.rs`, `stream.rs` |
| `live` | Medium | Mechanical follow-through once `trading_core` compiles |
| `lab` | **Large** | Sweep/backtest engine reads `sol_amount`/`price_per_token` pervasively over hot loops |
| `pump-trader` | Trivial | One field |
| `ingest-laserstream`/`ingest-websocket` | Small | Decode struct field names only |
| Frontend | **Large** | Every trade table, chart, position table, PnL card touches at least one renamed field |

The `sol_amount`→`amount_sol` rename is the single biggest line-count item in
this plan by a wide margin (it's the most-read field in the entire codebase);
everything else is comparatively contained.
