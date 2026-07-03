# First-slot buy/sell SOL — implementation plan

## Status (2026-07-02, verified against code)

**§1–6 are DONE.** Write path, metrics, and read/frontend surfacing all match
this plan and are live in the codebase:

- `Token.creation_slot` — `trading_core/src/models/token.rs:42`.
- Migration `trading_core/migrations/0004_add_creation_slot_and_first_slot_activity.sql:18-22`
  (columns were later renamed `first_slot_buy_lamports`/`first_slot_sell_lamports`
  by the separate `0009_sol_lamports_naming.sql` pass — see below).
- `token_repo.rs` inserts/upserts `creation_slot` (lines 215/250/301,
  `COALESCE(EXCLUDED.creation_slot, tokens.creation_slot)` at 282).
- `ingest-laserstream/src/decode/create.rs` captures `slot`; the consumer builds
  it into `Token` at `live/src/ingest/consumer.rs:444` (`creation_slot: Some(e.slot)`).
- `TokenState` fields + same-slot accumulation live in
  `trading_core/src/state/token_cache.rs:174/177/182` (fields),
  `apply_aggregates` **now at line ~345** (shifted from the plan's original 328),
  logic unchanged from what's described below. Unit tests at lines 886/911
  (`accumulates_first_slot_buy_and_sell_sol`, `first_slot_sums_zero_without_creation_slot`).
- `token_metrics.rs::TokenMetricsWrite` has both fields (lines 31/33), populated
  in `metrics_from_state` (78-79).
- `token_info_repo.rs` — `INFO_COLS`/`InfoRow`/`row_to_info`/`upsert_metrics` all
  wired (columns are `first_slot_buy_lamports`/`first_slot_sell_lamports` in SQL,
  converted to human-SOL `first_slot_buy_sol`/`first_slot_sell_sol` at the repo
  boundary via `lamports_to_sol`/`sol_to_lamports` — consistent with the SOL vs
  lamports naming rule).
- Read path/API (`token_repo.rs::TokenListRow`, `TokenSummary`/`TokenDetail` in
  `api/handlers/tokens/tokens.rs`, sort/filter keys `first_slot_buy`/`first_slot_sell`)
  and frontend (`shared/types/index.ts`, `sharedTokenColumns.tsx`, `tokenColumns.tsx`)
  are all confirmed present — §6 below is accurate as written.

**Stage 7 (§5's deferred fingerprint/grouping wiring) is now CODE-COMPLETE
(2026-07-02).** All backend + frontend code has landed and typechecks
(`trading_core`/`lab`/`live` clean, grouping tests green, touched frontend files
tsc-clean). Two items remain, both **externally gated, not code**: (1) the lake
re-export that backfills `fp_first_slot_*` into the tokens dimension file —
deliberately **not** run here, so it bundles into `simulate-lake-migration-plan.md`
Stage 6's single re-export (that plan's `tx_signature`/`trades_schema()` change
has already landed in the same file); (2) DB-gated spot-checks (`grouped()`
non-null keys, lake `DESCRIBE`) that need a populated local DB / the re-export.
Originally promoted from "explicitly out of scope" — see next section for the
cross-plan coordination.

## Cross-plan coordination — `simulate-lake-migration-plan.md`

That plan migrates single-rule simulate (tpsl1/tpsl2/swing1) off Postgres onto
the same Parquet lake the grouped sweep already uses. Two concrete links to
Stage 7 below:

1. **Same file, same re-export.** Stage 7 here adds `fp_first_slot_buy_sol`/
   `fp_first_slot_sell_sol` to `lab/src/lake/export.rs::tokens_schema()` (the
   lake's **tokens** dimension file). The other plan's Stage 1/Stage 6 adds
   `tx_signature` to `trades_schema()` (the **trades** file) in the same file
   and does a one-time **full lake re-export** to backfill it. Don't run two
   separate full re-exports — land this plan's Stage 7 schema change before or
   during that plan's Stage 6, so one `lake-export` pass produces uniform
   trades *and* tokens files.
2. **Fingerprint reaches simulate for free, if ordered right.** The other
   plan's Stage 2 (`SimToken`) is being written to carry `fp: TokenFingerprint`
   (mirroring `TokenTrades.fp` in the sweep corpus) specifically so that once
   `first_slot_buy_sol`/`first_slot_sell_sol` land in `TokenFingerprint` here,
   simulate results pick them up automatically — no separate simulate-side
   wiring needed. If Stage 7 here lands after that plan's Stage 2, no extra
   work; if it lands before, no extra work either — order doesn't block, only
   the re-export timing (point 1) benefits from coordination.

---

Adds two new `tokens_info` metrics: total buy SOL and total sell SOL across all
trades that land in the **same slot** as the token's creation transaction.
Computed **streaming, in-memory** (no backfill query, no extra DB round-trip) —
mirrors the existing `volume_sol_total`/`ath_price` accumulation pattern in
`TokenState`.

Scope decided: streaming compute path, stored on `tokens_info` (derived
hot-metric, not a `tokens` creation-fact), **no** fingerprint/grouping wiring
yet (`GroupField`/`TokenFingerprint`/lake export `fp_*` columns are a separate
follow-up once the raw data is validated) — **that follow-up is now Stage 7
below.**

## Why `tokens_info`, not `tokens`

`tokens` is write-once creation facts (`initial_buy_sol` reads the creation
tx's own bundled buy instruction — self-contained, known instantly).
This field requires scanning trades that arrive as **separate** gRPC events
after `TokenCreated`, so it's a derived-from-trades aggregate — same category
as `volume`/`trade_count`, which is exactly what `tokens_info` is for
(see `@plans/database/token-storage.md`).

## Why streaming, not a backfill query

The creation slot is known the instant `TokenCreated` fires. `on_trade()`
already folds every trade into `TokenState` before any DB write
(`apply_aggregates` in `trading_core/src/state/token_cache.rs:345`). Same-slot
buy/sell sums are one more `if` in that existing fold — no new query, no new
poll/backfill job, consistent with the CLAUDE.md hot-path budget ("Strategy
eval: read from runtime_cache.rs, never DB-per-event").

## Changes (all DONE — kept for reference)

### 1. `Token` model + `tokens` schema — capture `creation_slot`

Nothing today persists the creation transaction's `slot` (only `created_at`, a
timestamp). Add it as a plain creation fact:

- `trading_core/src/models/token.rs`: add `pub creation_slot: Option<u64>` to
  `Token`, threaded through `Token::new(...)`.
- `trading_core/migrations/000N_add_creation_slot_and_first_slot_activity.sql`
  (landed as `0004_...`):
  ```sql
  ALTER TABLE tokens ADD COLUMN IF NOT EXISTS creation_slot BIGINT;

  ALTER TABLE tokens_info
      ADD COLUMN IF NOT EXISTS first_slot_buy_sol  BIGINT,   -- lamports
      ADD COLUMN IF NOT EXISTS first_slot_sell_sol BIGINT;   -- lamports
  ```
  **Note:** the column names above were the original ones; `0009_sol_lamports_naming.sql`
  later renamed them to `first_slot_buy_lamports`/`first_slot_sell_lamports` to
  match the locked SOL-vs-lamports naming rule (a `BIGINT` holding lamports must
  not be named `_sol`). Current schema uses the `_lamports` names.
- `trading_core/src/storage/repositories/token_repo.rs`: add `creation_slot`
  to `insert`/`insert_many`/`upsert` column lists + binds (mirrors
  `initial_buy_sol`'s existing bind pattern, just `i64` not lamports-scaled —
  slot has no unit conversion).
- `ingest-laserstream/src/decode/create.rs`: the decoder already has `slot` in
  scope for the `TokenCreated` event (`create.rs:105-131`). Thread it into the
  `Token` constructed on the consumer side.

### 2. `TokenState` — accumulate same-slot sums

`trading_core/src/state/token_cache.rs`:

- Add two fields to `TokenState`: `first_slot_buy_sol: f64`,
  `first_slot_sell_sol: f64` (human-SOL `f64` in memory, same convention as
  `volume_sol_total` — lamports conversion happens at the repo boundary).
- Add a bool `first_slot_window_open: bool` (default `true`) — cheap latch so
  once a trade with `slot > creation_slot` is observed, accumulation stops
  permanently. Not strictly required for correctness (summing only same-slot
  trades is naturally idempotent/order-independent), but it avoids scanning
  the condition indefinitely on long-lived tokens — a one-line early return.
- In `apply_aggregates` (`token_cache.rs:345`), after the existing
  `volume_sol_total`/`trade_count` accumulation, add:
  ```rust
  if self.first_slot_window_open {
      match self.token.creation_slot {
          Some(creation_slot) if trade.slot() == creation_slot => {
              if trade.is_buy() {
                  self.first_slot_buy_sol += trade.sol_amount();
              } else {
                  self.first_slot_sell_sol += trade.sol_amount();
              }
          }
          Some(creation_slot) if trade.slot() > creation_slot => {
              self.first_slot_window_open = false;
          }
          _ => {}
      }
  }
  ```
  Order-independent by construction (sums, not "latest wins"), so no
  interaction with the existing `is_newest` gRPC-lag guard is needed — a
  same-slot trade delivered late still lands in the sum correctly as long as
  the window hasn't closed. A same-slot trade arriving *after* a later-slot
  trade already closed the window is the one edge case that under-counts;
  acceptable (matches how `is_dead`/`ath` already accept minor gRPC
  reordering imprecision) — note it in the column comment.

### 3. Metrics write path

- `trading_core/src/state/token_metrics.rs`: add `first_slot_buy_sol: f64`,
  `first_slot_sell_sol: f64` to `TokenMetricsWrite`; populate in
  `metrics_from_state`.
- `trading_core/src/storage/repositories/token_info_repo.rs`:
  - Add both columns to `INFO_COLS`, `InfoRow`, `row_to_info` (→
    `crate::models::token_info::TokenInfo`, which also needs the two fields).
  - Add two params to `upsert_metrics` (`#[allow(clippy::too_many_arguments)]`
    already present). Upsert semantics: plain `EXCLUDED` overwrite is correct
    here (unlike `ath_price`'s COALESCE-preserve) — the value only ever grows
    monotonically within the open window and freezes once closed, so the
    latest in-memory value is always the authoritative one.
- Whatever calls `upsert_metrics` today (the ingest `db_writer.rs` flush path,
  and the eviction sweep's final dead-token flush in `token_cache.rs:522`)
  needs the two new args threaded through — same call-site shape as the
  existing ones.

### 4. Seed/rebuild path parity

- `recompute_token_state` (`token_metrics.rs:32`) replays cached trades through
  `add_cached_trade` → `apply_aggregates`, so it gets the new accumulation for
  free — but only if `creation_slot` survived onto the seeded `Token`. Confirm
  the cold-start seed query (`token_repo.rs` / wherever `TokenState::new` is
  built from a DB row) selects `creation_slot`.
- `CachedTrade` already carries `slot` (`token_cache.rs:60`), so no new field
  needed there.

### 5. Fingerprint/grouping wiring — promoted to Stage 7 (was "out of scope")

~~`GroupField`/`TokenFingerprint` wiring (`grouping.rs`, `creation_stats_repo.rs`)
— add once the raw columns exist and are validated against real data.~~ The
raw columns are validated (live in prod, unit-tested). See Stage 7 below.

~~Lake export `fp_*` columns (`lab/src/lake/export.rs`, `duck.rs`) — same
reason, and lake fingerprint columns mirror `TokenFingerprint` 1:1, so they
should follow, not lead, the grouping-side decision.~~ Same — moved to Stage 7.

~~Frontend surfacing (token detail panel, table columns)~~ — **DONE** (see §6).

### 6. Read path + frontend surfacing (follow-up pass — DONE)

The streaming write path (§1–4) only populated the columns; nothing served them.
This pass wires the read path through to every token table.

**Backend read/API (`trading_core`):**

- `token_repo.rs`: `TokenListRow` gains `first_slot_buy_sol`/`first_slot_sell_sol`
  (`Option<f64>`, human SOL). All three list SELECTs (`find_list_rows`,
  `find_list_rows_for_mints`, `find_list_row_by_mint`) add
  `i.first_slot_buy_lamports::float8 / 1e9 AS …` (lamports → SOL at the boundary, like
  `initial_buy_sol`).
- `api/handlers/tokens/tokens.rs`:
  - `TokenSummary` (the `/api/tokens` + `/api/tokens/batch` list DTO) and
    `TokenDetail` (`/api/tokens/:mint`) both gain the two `Option<f64>` fields.
  - Both `From<&TokenState>` impls read the live-cache fields (always `Some`,
    since the cache carries `0.0` defaults); both `From<TokenListRow>` /
    slow-path JSON pass the DB `Option` through.
  - Server-side sort + per-column filter wired under keys `first_slot_buy` /
    `first_slot_sell`: added to `NUMERIC_COLS`, `SORTABLE_COLS`, `sort_key`,
    `col_filter_number`, `col_filter_text`, plus `f_first_slot_{buy,sell}_{min,max}`
    global-filter params in `PaginationParams`/`from_params`/`matches`.

Because both the live-cache path (`TokenListCache` builds `TokenSummary` from
`TokenState`) and the DB base path (`TokenSummary::from(TokenListRow)`) run
through the same DTO, every `/api/tokens*` consumer gets the fields with no
per-endpoint change.

**Frontend (`frontend-react`):**

- `shared/types/index.ts`: `TokenRecord` (+`first_slot_buy_sol`/`_sell_sol`
  non-optional), `TokenDetailRecord`, and the three enrichment-carrying types
  (`RulePositionRecord`, `MatchedTokenRecord`, `SimulatedTokenResult`) gain the
  optional fields.
- `shared/components/tokens/sharedTokenColumns.tsx`: two field names added to
  `TOKEN_ENRICH_FIELDS` (so `mergeTokenData` copies them into every strategy /
  wallet row) and two `ALL_TOKEN_COLS` entries (`first_slot_buy` /
  `first_slot_sell`, `market` group, `AmountCell`, `defaultVisible: false`) — so
  `appendedTokenColumns` surfaces them in every strategy/wallet table.
- `shared/components/tokens/tokenColumns.tsx`: two matching columns for the main
  Tokens page (with `filterNumber` for per-column numeric filtering).

Column keys are `first_slot_buy`/`first_slot_sell` (matching the backend sort/
filter keys); the DataTable's per-column filter + header sort round-trip to the
server via the existing `cf`/`sort` params — no new global filter-panel input
was added (deferred; the backend `f_*` params exist if it's wanted later).
`SimulatedTokenResult` already carries the fields — once
`simulate-lake-migration-plan.md` migrates the backtest data source, no
frontend change is needed here; the type contract is unchanged.

## Verification (§1–6, DONE)

- [x] `cargo check -p live` / `-p trading_core` clean.
- [x] Unit test in `token_cache.rs` mirroring the existing `is_dead` test style:
  construct a `TokenState` with a known `creation_slot`, feed
  same-slot buy + sell trades and one later-slot trade, assert
  `first_slot_buy_sol`/`first_slot_sell_sol` match and the window closes
  (a same-slot trade added afterward does NOT change the sums).
- [x] Zero-SOL verification per CLAUDE.md gotcha: no real trade needed — this is
  pure in-memory aggregation, fully covered by unit tests + a read-only
  Postgres check that `tokens_info.first_slot_buy_sol` populates after a real
  ingest run.

---

## Stage 7 — Fingerprint/grouping wiring (new, promoted from §5)

**Session kickoff:** "Execute Stage 7 of token-first-slot-activity-plan.md —
wire `first_slot_buy_sol`/`first_slot_sell_sol` into `TokenFingerprint`/
`GroupField` so it's usable as a sweep grouping dimension, plus the matching
lake `fp_*` columns. Coordinate the lake re-export with
`simulate-lake-migration-plan.md` Stage 6 (see that plan's Cross-plan
coordination section) — don't trigger two separate full re-exports."

**Context (verified 2026-07-02):**
- `TokenFingerprint` (`trading_core/src/grouping.rs:35-47`) and `GroupField`
  (`grouping.rs:84-93`) today hold only **creation-time metadata** (CU
  settings, cashback flag, initial-buy SOL, ix-label sequence) — all sourced
  from the `tokens` table directly. `first_slot_buy_sol`/`first_slot_sell_sol`
  would be the **first trade-derived, `tokens_info`-sourced** fingerprint
  field — architecturally new territory (not just "add another exact-match
  enum variant").
- `creation_stats_repo.rs::grouped()` (lines 242-317) builds its group-key CTE
  from `FROM tokens t` **with no `tokens_info` join** (confirmed: `grouped()`'s
  base CTE at line 302 has no `LEFT JOIN tokens_info`, unlike `heatmap`/`trend`
  which do join it at lines 104-105/147-148). Adding a `tokens_info`-sourced
  group field requires adding that join to `grouped()`.
- Lake fingerprint columns (`fp_*`, `lab/src/lake/export.rs:75-92`
  `tokens_schema()`) mirror `TokenFingerprint` 1:1 and are populated in
  `export_tokens` (`export.rs:337-414`) by streaming straight from Postgres
  `tokens` — also **no `tokens_info` join today**.
- No existing `GroupField` is trade-timing/early-activity derived — all are
  static creation facts. This is a new category, not a copy-paste of an
  existing field.

**Files:**
- `trading_core/src/grouping.rs`
- `trading_core/src/storage/repositories/creation_stats_repo.rs`
- `lab/src/lake/export.rs`
- `lab/src/lake/duck.rs`
- `frontend-react/src/lab/components/sweep/groupedTypes.ts`,
  `FingerprintGroupPicker.tsx`, `groupColumns.tsx`

**Work:**
- [x] `grouping.rs`: add `first_slot_buy_sol: Option<f64>`,
  `first_slot_sell_sol: Option<f64>` to `TokenFingerprint`; add
  `FirstSlotBuySol`/`FirstSlotSellSol` variants to `GroupField` (+ `as_str`/
  `from_tag`/`render_field`, following the existing `InitialBuySol` `f64`
  pattern — same "exact value, no binning yet" v1 semantics as the rest of the
  enum, per the module's own doc comment). Also updated the exhaustive
  `matches_field_filter` match in `lab/.../grouped_sweep.rs` (the field-filter
  path) to handle both new variants (f64 compare, mirrors `InitialBuySol`).
- [x] `creation_stats_repo.rs::group_field_sql()`: add SQL expressions for the
  two new fields, sourced from `tokens_info` (not `tokens`) —
  `COALESCE((ti.first_slot_buy_lamports::float8 / 1e9)::text, '∅')`.
- [x] `creation_stats_repo.rs::grouped()`: add `LEFT JOIN tokens_info ti ON
  ti.mint_address = t.mint_address` to the base CTE, matching the
  join `heatmap`/`trend` already use. One-to-one on `mint_address` → doesn't
  change group cardinality (noted in the fn doc).
- [x] `lab/src/lake/export.rs::tokens_schema()`: add
  `fp_first_slot_buy_sol`/`fp_first_slot_sell_sol` (`Float64`) fields.
- [x] `lab/src/lake/export.rs::export_tokens`: add a `LEFT JOIN tokens_info` to
  its Postgres SELECT (currently `tokens`-only) to source the two new columns.
- [x] `lab/src/lake/duck.rs::attach_fingerprints`: extend the SELECT / struct
  building to read the two new `fp_*` columns into `TokenFingerprint`.
- [x] Frontend: mirror the new `GroupField` variants in the grouping picker UI —
  `groupedTypes.ts` (`GROUP_FIELDS` + `GROUP_FIELD_LABELS`),
  `FingerprintGroupPicker.tsx` (unit hint), and the shared dashboard's
  `GroupedCreationSection.tsx` (`SCALAR_FILTER_FIELDS`). `groupColumns.tsx`
  needs no change — it renders group-key chips generically via
  `GROUP_FIELD_LABELS`.
- [ ] **Coordinate the lake re-export** with `simulate-lake-migration-plan.md`
  Stage 6: land this stage's `tokens_schema()` change before that plan's full
  `lake-export` re-export runs, so trades (`tx_signature`) and tokens
  (`fp_first_slot_*`) are backfilled in the same pass. If Stage 6 there has
  already run by the time this lands, do a standalone tokens-only re-export
  here instead (`export_tokens` is independent of `export_day`'s trades path).

**Done when:**
- [x] `cargo check -p trading_core` + `cargo check -p lab` + `cargo check -p live` clean.
- [x] `cargo test -p trading_core` covering `grouping.rs` — new
  `first_slot_fields_round_trip_and_render` test (as_str/from_tag round-trip +
  present-value render + `None`→sentinel). All 7 grouping tests pass.
- [ ] A `creation_stats_repo::grouped()` call with `FirstSlotBuySol` in `fields`
  returns non-null group keys for tokens with `tokens_info` rows — **DB-gated**,
  deferred (needs a populated local DB).
- [ ] Lake `DESCRIBE` on the tokens dimension file shows `fp_first_slot_buy_sol`/
  `fp_first_slot_sell_sol` populated (non-null) after a re-export — **gated on
  the coordinated re-export** (below).
- [x] Sweep grouping picker in the frontend lists the two new fields (code
  landed; `npm run build`'s only tsc error is a **pre-existing** unused-import in
  `src/live/App.tsx`, unrelated to this stage — my touched files typecheck clean).
- [ ] Only **one** full lake re-export was triggered across both this stage and
  `simulate-lake-migration-plan.md` Stage 6 — **NOT triggered here on purpose**:
  both `tokens_schema()` (`fp_first_slot_*`, this stage) and `trades_schema()`
  (`tx_signature`, the other plan's Stage 1 — already landed) now carry their new
  columns, so the other plan's Stage 6 single `lake-export` pass backfills both.

## Docs to update after implementation

- [x] `@arch/database.md` — `tokens`/`tokens_info` column lists (§1-6, done).
- [x] `@plans/database/token-storage.md` — `creation_slot`,
  `first_slot_buy_sol`, `first_slot_sell_sol` added to the table 1/2 schemas +
  rationale section (§1-6, done).
- [x] `@arch/sweep.md` — Stage 7: `grouping.rs` row now notes the two new
  `GroupField` variants and their `tokens_info`-sourced nature (the enum's first
  trade-derived fields) + the `LEFT JOIN tokens_info` in `grouped()`/`export_tokens`.
- [ ] `CLAUDE.md` — not needed: the `tokens_info` join in `grouped()`/`export_tokens`
  is one-to-one on `mint_address` over the already-bounded creation window, so it
  adds no new data-scale guardrail (heatmap/trend already join `tokens_info`). No
  standing-pattern callout warranted.
