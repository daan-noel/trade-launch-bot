# Simulate → Lake migration plan

## Status (2026-07-02) — ALL STAGES CODE-COMPLETE

All 7 stages implemented; `cargo check -p lab` + `-p live` clean, 76 lab tests
pass, clippy clean on touched code. Landed:

**Design note (revised after review):** the initial build added a separate
`SimTrade`/`SimToken`/`load_sim` — a near-clone of `SweepTrade` differing only by a
display-only `tx_signature`. That duplication was collapsed into **one type**:
`SweepTrade.tx_signature: Option<Box<str>>` (`None` on the sweep = 16 B, no heap;
`Some` on simulate) populated via a new `Selection::with_signatures` flag through the
**single** `LakeSource::load`. Simulate vs sweep pricing is now parity by construction.

- `export.rs` trades schema carries `tx_signature` (base58); DuckDB reads use
  `union_by_name=true`. `trade_repo::sig_bytes_to_base58` is now `pub`.
- `SweepTrade` gained `tx_signature: Option<Box<str>>`; `Selection` gained
  `with_signatures: bool`; `load_token_trades` conditionally projects the column.
  (No `SimTrade`/`load_sim`/`lab/src/lake/sim.rs` — deleted.)
- `lab/src/strategies/sim_fetch.rs::fetch_sim_histories(mints)` — shared lake read via
  `load` + `with_signatures:true`, uncapped per-mint, `curve_only:false`, stale-lake
  warn (`newest_lake_day`).
- tpsl1/tpsl2/swing1 `backtest.rs` all read `fetch_sim_histories`; tpsl2 switched to
  index-based entry (`find_scalp_entry_indexed` → `find_worst_case_paper_entry_at`).
  Backtest bodies unchanged by the type collapse (resolvers are `TradeRow`-generic).
- Dead PG plumbing deleted: `BacktestTradeCache` (module + `LocalState` field),
  `BACKTEST_FETCH_*`, per-file `TradeRepo`/`token_cache` freshness keying.
- Parity test `lake::duck::parity_tests::signature_flag_changes_only_the_signature`
  (`--ignored`); docs (@arch/database.md, sweep.md, strategies.md, @plans tpsl2-params,
  CLAUDE.md) + memory updated.

**Remaining (DB/lake-gated ops, not code):** run the one-time full `lake-export`
re-export (so every day file carries `tx_signature` uniformly, dropping the
`union_by_name` null-fill dependence) + a `--include-today` pass; then run the
`--ignored` parity test against a populated `$SWEEP_LAKE_DIR` to confirm green.

**Correction to Stage 4 item 4 (real_sol_reserves):** the plan assumed tpsl1's
`backtest.rs` reconstructs `real_sol_reserves` and that migration must drop it.
Verified: `real_sol_reserves`/`approx_real_sol_reserves` do **not** appear
anywhere in `lab/src/strategies/*`. The reconstruction (`approx_real_sol_reserves`)
only exists in the lake layer itself — `lab/src/lake/duck.rs:285` and
`export.rs:281` — where it already bakes the value into every lake row at
export/read time. So Stage 4 item 4 is a no-op: there is nothing to remove
from `backtest.rs`; pricing parity with the sweep is automatic once the
backtest reads lake rows, no extra cleanup step needed. Updated below.

## Cross-plan coordination — `token-first-slot-activity-plan.md`

That plan's §1–6 (streaming `first_slot_buy_sol`/`first_slot_sell_sol` write +
read path) are **done**. Its deferred §5 item — wiring those fields into
`TokenFingerprint`/`GroupField` — is promoted to its own Stage 7 in that plan,
and it **touches the same file** this plan's Stage 6 does:

- This plan (Stage 1 + Stage 6): `lab/src/lake/export.rs` `trades_schema()` /
  `TradeBuilders` — adds `tx_signature` to the **trades** Parquet file, then
  Stage 6 does a one-time **full re-export** so every day file is uniform.
- The other plan (its Stage 7): same file, `tokens_schema()` / `export_tokens`
  — adds `fp_first_slot_buy_sol`/`fp_first_slot_sell_sol` to the **tokens**
  dimension file, which also needs a re-export to backfill.

**Do not run two separate full lake re-exports.** Land both schema changes
(this plan's `tx_signature` on `trades_schema()` + the other plan's
`fp_first_slot_*` on `tokens_schema()`) before triggering Stage 6's re-export,
so one `cargo run -p lab -- lake-export` pass produces uniform trades *and*
tokens files. If Stage 6 here runs before the fingerprint work lands, the
tokens dimension file will need its own follow-up re-export later — acceptable
but wasteful; prefer sequencing fingerprint's schema edit into the same
session as Stage 6.

**Second interaction — `SimToken` should carry `fp`.** The grouped sweep's
`TokenTrades` already carries `fp: TokenFingerprint` (`lab/src/sweep/corpus.rs:41`),
populated once per corpus load from the tokens dimension file
(`duck.rs::attach_fingerprints`). Stage 2 below (`LakeSource::load_sim` /
`SimToken`) should mirror that — carry `fp: TokenFingerprint` on `SimToken` the
same way. This costs nothing extra (same load, same file) and means once
first-slot-buy/sell lands in `TokenFingerprint`, it's automatically available
on simulate results too, not just the grouped sweep — no separate wiring pass
needed on the simulate side later. Added as Stage 2 item 4 below.

**Goal:** single-rule simulate (tpsl1 + tpsl2 + swing1 `.../simulate`) reads its
trade data from the **same source as the grouped sweep** — the Parquet lake via
`LakeSource::load` — instead of Postgres via `TradeRepo::find_by_mints_all`. This
removes the two-source divergence (sweep=lake, simulate=PG) so a rule prices
identically whether you sweep it or drill into it.

Decision locked with the user (2026-07-02): **backtest-fresh is acceptable** —
simulate is an analysis tool, staleness of an export interval is fine. So we
unify on the lake (Option A), not the parity-test-only Option B.

## Guiding principle — PG vs Lake roles (corrected 2026-07-02)

This migration is one instance of a broader division of responsibility. Framing it
right explains *why* it stops where it does (trades move, token selection doesn't):

- **Postgres = authoritative write sink + source of truth + fresh/unsealed tail +
  indexed/point queries.** The `trades` table *is* the live gRPC feed (ingest can
  NOT target the lake). PG also serves the tokens-table candidate scan
  (`collect_matching_tokens`), rules, positions, and 100K+ token-list pagination —
  those are *queries*, not "persist for display".
- **Lake = derived-from-PG, immutable, day-partitioned, sealed-days-only (no
  today/unsealed), no mutability.** A columnar mirror of sealed history for
  repeated full-scan analysis. Anything needing "now" can't come from it.
- **Live hot path = in-RAM cache** (latency budget), never DB-per-event.

**Corrected end-state:** bulk trade reads (sweep + simulate) → lake, loaded once
into an in-memory corpus and reused across evaluations; **candidate-token
selection + the fresh/today tail stay on Postgres** (indexed query / unsealed data
the lake can't serve). The user's original "PG = persist/display only" model
undersold PG's role as write-sink/source-of-truth/query-engine; this migration
moves exactly what *should* move and no more. (Scope confirmed: trades→Lake, keep
token scan on PG.) See memory `pg-vs-lake-roles`.

## Current topology (verified)

| Path | Trade source | Trade type | Entry resolver |
| --- | --- | --- | --- |
| Grouped sweep | lake (`LakeSource::load`) | `SweepTrade` (slim, wallet-interned, **no tx_sig**) | index-based (`find_worst_case_paper_entry_at`) |
| tpsl1 simulate | PG (`find_by_mints_all`) | `Trade` (full) | `find_entry_fill_in_trades` (positional — no fork) |
| tpsl2 simulate | PG (`find_by_mints_all`) | `Trade` (full) | `find_worst_case_paper_entry` (**by tx_sig** — Fork A) |
| swing1 simulate | PG (`find_by_mints_all`) | `Trade` (full) | `find_phase_entry` → returns `(trigger_idx, fill)` (already index-based — no fork) |

Key enabling fact: the shared entry/exit fns are **already generic over
`TradeRow`** (`find_entry_fill_in_trades<T: TradeRow>`,
`find_trade_driven_exit<T: TradeRow>`), and `SweepTrade: TradeRow`. So the
backtest's core resolution logic consumes `SweepTrade` **unchanged** — no
re-projection back to `Trade`.

## The two real forks (why this isn't a one-line swap)

### Fork A — tpsl2 resolves entry by `tx_signature`, which `SweepTrade` drops
`lab/src/strategies/tpsl_sniper_2/backtest.rs:278` calls
`find_worst_case_paper_entry(&trades, &target.tx_signature)`. `SweepTrade::
tx_signature()` returns `""` (Phase 1.2 dropped the 88 B base58 per row; the
sweep resolves the trigger by **index** instead). The sweep already has the
index path: `find_scalp_entry` → `trigger_idx` →
`find_worst_case_paper_entry_at(trades, trigger_idx)`
(see `lab/src/sweep/strategies/tpsl2.rs:661`, `registry.rs:565`).

→ **tpsl2 backtest must switch to the index-based entry resolution.** `find_scalp_entry`
already returns enough to recover the trigger index (it walks the slice); confirm
whether it exposes the index or whether we call the `_indexed` variant the sweep
uses. tpsl1 has **no** such issue — `find_entry_fill_in_trades` is already
positional.

### Fork B — result rows carry tx-signature strings the frontend renders
`BacktestTokenResult` has `entry_tx` / `exit_tx` / `target_tx` (tpsl2). The
frontend renders them as Solscan links (`Tpsl1Page.tsx`, `Tpsl2Page.tsx`,
`shared/components/tpsl2/tableColumns.tsx`). Lake `SweepTrade` has no signatures
→ these would go **blank** = user-visible regression.

**BLOCKING FINDING (2026-07-02):** the lake **trades Parquet schema has NO
`tx_signature` column** — it was deliberately dropped at export
(`lab/src/lake/export.rs:46` "no `tx_signature`"). So "keep tx links" is NOT a
projection-widening on the read side; the column simply isn't in the files. The
chosen decision (wider simulate projection) therefore **requires re-adding
`tx_signature` to the export schema** and re-exporting, OR falling back to a PG
lookup. The 88 B/row cost lands on **every** lake trade file (sweep reads them
too), not just simulate — this is a real storage decision, re-surfaced to the user.

Options for Fork B (pick in decisions below):
- **B1 — add `tx_signature` back to the lake trade schema.** Widen
  `trades_schema()` + the export builders + the DuckDB read. Costs ~88 B/row on
  **every** lake file and forces a **full re-export** (existing immutable day
  files lack the column → DuckDB read must tolerate its absence or all days must
  be rewritten). Heaviest option; makes the lake self-sufficient for simulate.
- **B2 — accept blank tx links in simulate.** Simplest; regresses the UI
  (Solscan links go blank on simulate/matched tables).
- **B3 — resolve tx_sig by (slot, leg_index, mint) lookup** against a cheap PG
  point-query for just the resolved entry/exit/target rows (a few per token, not
  the full history). Keeps the lake slim + unchanged, no re-export. Reintroduces
  a *tiny* PG read on the simulate path — but only for display strings, never for
  pricing/decisions, so the "single source of truth for metrics" goal still holds.

Recommendation shifted to **B3**: the lake stays slim and un-re-exported, the
sweep is untouched, and the PG dependency is display-only (signatures for a
handful of rows), not a data-source split for the actual simulation. B1's
full-re-export + every-file cost is disproportionate for cosmetic links.

## Freshness — the lake is sealed-days-only

`lake-export` writes **sealed** UTC days (`< date_trunc('day', now())`); today's
in-progress trades stay in PG until tomorrow's export. Simulate routinely matches
tokens created **today** (its `find_by_mints_all` has no time bound). If we point
it at a default lake, those tokens come back with **truncated/empty** trades — a
correctness regression worse than staleness.

→ **Require `lake-export --include-today`** on a recurring cadence so simulate on
recent tokens isn't truncated. `export_lake(include_today=true)` already exists
and force-overwrites today's non-immutable snapshot. Cadence is an ops choice
(cron on the workstation); simulate then trails "now" by at most one export
interval, which the user accepted.

Open sub-question: should simulate **detect** a stale lake (newest dt < today)
and warn, rather than silently returning a short history? Cheap guard worth adding.

## Implementation sketch

1. **tpsl1 backtest** (`lab/src/strategies/tpsl_sniper_1/backtest.rs`)
   - Keep the candidate-token scan on PG (`collect_matching_tokens` — tokens
     table, not trades).
   - Replace the chunked `find_by_mints_all` fetch + `backtest_trade_cache` layer
     with **one** `LakeSource::load(&Selection { mints: Some(matched_mints),
     created_after: None, created_before: None, per_mint_cap: <full>, window:
     LaunchWindow, .. })`.
   - Iterate `corpus.tokens` (`TokenTrades.trades: Arc<Vec<SweepTrade>>`); run
     `find_entry_fill_in_trades` + `find_trade_driven_exit` on `&SweepTrade`
     (generic — compiles unchanged).
   - Progress ticks: one per matched token (load is a single call now, so ticking
     shifts from per-chunk to per-token-resolve).
   - ~~Drop `real_sol_reserves` reconstruction here~~ — **N/A, verified not
     present in `backtest.rs` today** (see Status note above); pricing already
     matches the sweep by construction once lake rows are read.

2. **tpsl2 backtest** (`lab/src/strategies/tpsl_sniper_2/backtest.rs`)
   - Same lake swap, **plus** Fork A: switch to index-based
     `find_worst_case_paper_entry_at(trades, trigger_idx)`.
   - Fork B resolution (tx links) per decision.

3. **`per_mint_cap` for simulate.** Sweep caps at `SWEEP_PER_MINT_CAP` (launch
   window). Simulate today reads **full** history (`find_by_mints_all` uncapped)
   and computes ATH over all trades. Decide: keep simulate uncapped (pass a very
   high / sentinel cap to `Selection`) or accept the sweep cap. Uncapped is the
   current simulate contract; capping would silently change ATH/exit for
   high-volume tokens. **Lean: uncapped for simulate** (`per_mint_cap = i64::MAX`
   or a large const), documented.

4. **Remove now-dead PG plumbing** on the simulate path: `TradeRepo` import,
   `backtest_trade_cache`, `token_cache` freshness keying, `BACKTEST_FETCH_CHUNK`
   /`_CONCURRENCY`. Confirm `backtest_trade_cache` has no other consumer before
   deleting the field from `LocalState`.

5. **`LakeSource` reuse.** Load once per simulate (like the grouped-sweep handler
   at `grouped_sweep.rs:424`), share `SWEEP_LAKE_DIR` root resolution.

## Parity test (replaces two-path divergence with a guarantee)

Add a test (`--ignored`, needs a lake) that runs the **same rule** through
simulate and through a 1-rule grouped sweep over the **same mint set** and
asserts identical per-token entry/exit/pnl. This is the analogue of
`token_repo::parity_tests` for the token-list engines. Until it's green, the
"identical pricing" claim is unproven.

## Docs to update on completion

- `@arch/database.md` — drop the "backtest-only `find_by_mints_all` reconstructs
  real_sol_reserves" note for the simulate path (no longer PG-fed).
- `@arch/sweep.md` + `@arch/strategies.md` — simulate now shares the lake corpus source.
- `@plans/tpsl-strategy/tpsl2-entry-exit-params.md` — the "simulate reads PG /
  sweep reads lake" split description is now stale.
- `CLAUDE.md` data-scale guardrails — if the include-today export becomes a
  standing requirement for simulate.
- Memory: update `real-sol-reserves-offline` / `canonical-price-gmgn` notes
  (simulate no longer reconstructs; it reads the lake's baked value).

## Decisions (locked with user 2026-07-02)

1. **Fork B (tx links): B1** — re-add `tx_signature` to the lake trade schema so
   the lake is self-sufficient (no PG on the simulate path).
2. **Scope: all three** — tpsl1, tpsl2, **and swing1** migrate now.
3. **per_mint_cap: uncapped** — simulate keeps full per-token history (ATH/exit
   unchanged); pass a sentinel high cap to `Selection`.

## B1 schema-migration detail (the re-export concern)

Existing day files were written **without** `tx_signature`. Options for the mixed
old/new schema when DuckDB globs all days:

- **Re-export everything.** Delete the lake trades dir and re-run `lake-export`;
  every file then carries the column. Simplest, but throws away the immutability
  win for one migration (acceptable — it's a one-time cost).
- **`union_by_name=true`** on the DuckDB `read_parquet` so old files null-fill the
  missing column. Keeps existing files; simulate just gets `NULL` tx on
  pre-migration days (blank link on old rows only). Lower churn.

→ Lean **re-export** on the workstation lake (small, local, one command) so every
day is uniform and no simulate row has a surprise-null signature. Confirm lake
size before committing to the wipe. **Coordinate this re-export with
`token-first-slot-activity-plan.md` Stage 7** (see Cross-plan coordination
above) so `tokens_schema()`'s `fp_first_slot_*` columns land in the same pass.

### Exact export touch points (`lab/src/lake/export.rs`)
- `trades_schema()` — add `Field::new("tx_signature", Utf8, false)`.
- `LakeTradeRow` — add `tx_signature: String`; add `t.tx_signature` to the SELECT.
- `TradeBuilders` — add `tx_signature: StringBuilder`; append in `push`; add the
  finished column to the `RecordBatch` vec (position must match schema order).

### Exact read touch points (`lab/src/lake/duck.rs`)
- `load_token_trades` SQL — add `t.tx_signature` to `ranked` + outer SELECT.
- Row read — pull the new column; store it on the simulate trade row.
- **Projection fork:** the sweep's `SweepTrade` stays slim (no signature). Simulate
  needs a row type that carries it. Two sub-options:
  - Add `tx_signature: Option<Box<str>>` to `SweepTrade` (simplest; sweep ignores
    it but pays ~16 B/row for the Option+ptr even when None). Rejected — taxes the
    sweep hot loop the projection was built to avoid.
  - **New `SimTrade`** in the lab strategies layer = the `SweepTrade` fields +
    `tx_signature: String`, impl `TradeRow` with a real `tx_signature()`. A
    dedicated `LakeSource::load_sim(sel) -> Vec<SimToken>` (or a generic load
    param) builds these. → **chosen**: keeps the sweep projection untouched.

## Shared-helper refactor (all three backtests are structurally identical)

tpsl1/tpsl2/swing1 `run_backtest` share the same skeleton: rule load → candidate
scan (`collect_matching_tokens`) → chunked PG fetch + `backtest_trade_cache` →
per-token resolve closure → progress ticks → `select_simulated_tokens` → sort.
Only the resolve closure differs. Extract the lake fetch into one helper
(`fetch_sim_histories(app_state, &mints) -> HashMap<String, Arc<Vec<SimTrade>>>`)
used by all three; each keeps its own resolve closure (generic over `TradeRow`,
so `SimTrade` slots in). Delete the PG chunk/cache/concurrency consts + the
`token_cache` freshness keying from all three.

---

## Execution stages (run each in a separate chat session)

Each stage below is **self-contained**: it lists the files to touch, the exact
work, and a **Done when** gate you can verify before moving on. Stages are
ordered by dependency — do not start a stage until the prior one's gate is green.
Every stage ends on a compiling tree (`cargo check -p lab` clean) so a session
can stop cleanly. Check off items as you go — that's the todo list.

At the **start of each session**, paste the "Session kickoff" line for that stage
so the assistant has the entry context without re-deriving it.

## Stage 1 — Lake schema: add `tx_signature` (B1)

**Session kickoff:** "Execute Stage 1 of simulate-lake-migration-plan.md — add
`tx_signature` to the lake trade export + DuckDB read. Don't touch the backtests yet."

**Files:**
- `lab/src/lake/export.rs`
- `lab/src/lake/duck.rs`

**Work:**
- [ ] `export.rs::trades_schema()` — add `Field::new("tx_signature", Utf8, false)`
  (append at end so column order is explicit; note the position).
- [ ] `export.rs::LakeTradeRow` — add `tx_signature: String`; add `t.tx_signature`
  to the `export_day` SELECT.
- [ ] `export.rs::TradeBuilders` — add a `tx_signature: StringBuilder`, append in
  `push`, and add the finished array to the `RecordBatch` column vec **in the
  same position** as the schema field.
- [ ] `duck.rs::load_token_trades` SQL — add `t.tx_signature` to the `ranked` CTE and
  outer SELECT. Read the column into the row (store on the sim row in Stage 2;
  for now just make the read compile — you may leave it read-but-unused behind an
  `#[allow(dead_code)]` or fold it into Stage 2. Prefer folding: **do Stage 2's
  `SimTrade` in this same session only if time allows**, else stub).
- [ ] Use `union_by_name=true` on the DuckDB `read_parquet` glob so old (pre-column)
  day files null-fill instead of erroring, **then** re-export (Stage 6) makes it
  uniform. This lets the tree stay green before the re-export.

**Done when:**
- [ ] `cargo check -p lab` clean.
- [ ] A fresh `cargo run -p lab -- lake-export --include-today` over a small
  window writes files that include the column (verify with a DuckDB `DESCRIBE`
  or a one-off `SELECT tx_signature ... LIMIT 1`).
- [ ] No backtest behavior changed yet.

## Stage 2 — `SimTrade` type + `LakeSource::load_sim`

**Session kickoff:** "Execute Stage 2 — add the `SimTrade` row type (SweepTrade
fields + `tx_signature`) and a lake loader that returns it, `TradeRow`-impl'd.
Sweep's `SweepTrade` stays untouched. Also carry `fp: TokenFingerprint` on
`SimToken` so simulate results get fingerprint data for free once
token-first-slot-activity-plan.md Stage 7 lands."

**Files:**
- `lab/src/sweep/projection.rs` (or a new `lab/src/strategies/sim_trade.rs` — keep
  `SimTrade` in the strategies layer, not the sweep projection)
- `lab/src/lake/duck.rs`
- `lab/src/sweep/corpus.rs` (if a `SimToken`/`SimCorpus` analogue is needed)

**Work:**
- [ ] Define `SimTrade` = all `SweepTrade` fields + `tx_signature: String`. Impl
  `TradeRow` with a **real** `tx_signature()` returning the stored string (all
  other accessors identical to `SweepTrade`'s impl).
- [ ] Add `LakeSource::load_sim(&self, sel: &Selection) -> Result<Vec<SimToken>>`
  (or `Corpus<SimTrade>` if `Corpus` is made generic — simpler to add a parallel
  `load_sim` that builds `SimTrade` rows and returns `Vec<(mint, symbol,
  Arc<Vec<SimTrade>>)>`). Reuse the same SQL as `load_token_trades`, now reading
  the `tx_signature` column added in Stage 1.
- [ ] **Do not** widen `SweepTrade` — the sweep hot loop stays slim.
- [ ] `SimToken` carries `fp: TokenFingerprint`, populated from the same
  `attach_fingerprints` path `TokenTrades` already uses (`duck.rs`) — mirrors the
  sweep's corpus shape, no separate fetch. (Cross-plan: this is what lets
  `first_slot_buy_sol`/`first_slot_sell_sol` reach simulate once
  `token-first-slot-activity-plan.md` Stage 7 adds them to `TokenFingerprint`.)

**Done when:**
- [ ] `cargo check -p lab` clean.
- [ ] A unit/`--ignored` test (or a scratch `probe`) can call `load_sim` over a
  known mint and get back rows whose `tx_signature()` is non-empty and whose
  `SimToken.fp` is populated.
- [ ] Backtests still use PG (unchanged).

## Stage 3 — `fetch_sim_histories` shared helper

**Session kickoff:** "Execute Stage 3 — extract the shared lake fetch helper
`fetch_sim_histories` used by all three backtests. Backtests still call PG until
Stage 4/5/6 swap each one."

**Files:**
- new `lab/src/strategies/sim_fetch.rs` (or a shared mod in `lab/src/strategies/mod.rs`)

**Work:**
- [ ] `fetch_sim_histories(app_state, mints: &[String]) -> Result<HashMap<String,
  Arc<Vec<SimTrade>>>>`: build a `Selection { mints: Some(mints), per_mint_cap:
  <uncapped sentinel, e.g. i64::MAX or a large const>, window: LaunchWindow,
  created_after/before: None, curve_only: <match current simulate>, .. }`, call
  `LakeSource::load_sim`, collapse to the map. Resolve the lake root via the same
  `crate::lake::lake_root()` the grouped-sweep handler uses.
- [ ] Add a **stale-lake guard**: if the newest lake day `< today (UTC)`, log a warn
  (cheap; the open sub-question from the freshness section). Non-fatal.

**Done when:**
- [ ] `cargo check -p lab` clean.
- [ ] Helper compiles and is unit-testable in isolation.
- [ ] No backtest wired to it yet.

## Stage 4 — Migrate **tpsl1** backtest to lake

**Session kickoff:** "Execute Stage 4 — swap tpsl1 `run_backtest` to lake via
`fetch_sim_histories`. tpsl1 entry is positional (no Fork A). Keep candidate scan
on PG. Note: the plan's original item 4 (drop real_sol_reserves reconstruction)
is a no-op — verified not present in backtest.rs; skip it."

**Files:**
- `lab/src/strategies/tpsl_sniper_1/backtest.rs`

**Work:**
- [ ] Keep `collect_matching_tokens` (PG tokens scan) as-is.
- [ ] Replace the chunked `find_by_mints_all` fetch + `backtest_trade_cache` with one
  `fetch_sim_histories(app_state, &matched_mints)`.
- [ ] Per-token resolve: `find_entry_fill_in_trades(&trades, 1)` +
  `find_trade_driven_exit` on `&SimTrade` (generic over `TradeRow` — compiles
  unchanged). `entry_tx`/`exit_tx` now come from `SimTrade::tx_signature()`.
- [ ] ~~Drop `real_sol_reserves` reconstruction~~ — N/A, confirmed not present.
- [ ] Progress ticks → per-token-resolve (load is one call now).
- [ ] Delete tpsl1-local `BACKTEST_FETCH_CHUNK`/`_CONCURRENCY` if unused elsewhere.

**Done when:**
- [ ] `cargo check -p lab` clean.
- [ ] A tpsl1 simulate run returns non-empty results with populated
  `entry_tx`/`exit_tx` on a rule you know matches recent tokens (requires a
  fresh `--include-today` lake).

## Stage 5 — Migrate **tpsl2** backtest to lake (Fork A)

**Session kickoff:** "Execute Stage 5 — swap tpsl2 `run_backtest` to lake AND
switch entry from `find_worst_case_paper_entry(by tx_sig)` to the index-based
`find_scalp_entry_with_cohort_indexed` + `find_worst_case_paper_entry_at`."

**Files:**
- `lab/src/strategies/tpsl_sniper_2/backtest.rs`
- reference: `trading_core/src/strategies/tpsl_sniper_2/entry/scalp.rs`
  (`find_scalp_entry_with_cohort_indexed`, `find_worst_case_paper_entry_at`),
  `lab/src/sweep/strategies/tpsl2.rs:661` for the sweep's index path.

**Work:**
- [ ] Same lake swap via `fetch_sim_histories`.
- [ ] **Fork A:** replace `find_worst_case_paper_entry(&trades, &target.tx_signature)`
  (currently at `backtest.rs:278`) with the sweep's index-based resolution — get
  `trigger_idx` from `find_scalp_entry_with_cohort_indexed`, then
  `find_worst_case_paper_entry_at(&trades, trigger_idx)`.
- [ ] `target_tx`/`entry_tx`/`exit_tx` come from `SimTrade::tx_signature()` (Fork B
  satisfied by B1 — the lake now carries signatures).
- [ ] Keep `round_trip_with_costs`.

**Done when:**
- [ ] `cargo check -p lab` clean.
- [ ] tpsl2 simulate returns results whose `target_tx`/`entry_tx`/`exit_tx` are
  populated and whose entry matches what a 1-rule sweep of the same mint set
  picks (spot-check a couple tokens; formal parity is Stage 7).

## Stage 6 — Migrate **swing1** backtest + re-export lake

**Session kickoff:** "Execute Stage 6 — swap swing1 `run_backtest` to lake
(`find_phase_entry` is already index-based, no fork), then do the one-time full
lake re-export so every day carries `tx_signature`. Before re-exporting, check
whether token-first-slot-activity-plan.md Stage 7 (fp_first_slot_* columns) has
landed — if so, bundle both schema changes into this same re-export pass."

**Files:**
- `lab/src/strategies/swing_1/backtest.rs`
- ops: workstation lake dir (`$SWEEP_LAKE_DIR`)

**Work:**
- [ ] swing1 lake swap via `fetch_sim_histories`. `find_phase_entry` already returns
  `(trigger_idx, fill)` — no Fork A. Reuses tpsl1's `BacktestTokenResult`.
- [ ] Remove the now-dead PG plumbing shared across all three: `TradeRepo` import on
  the simulate path, `backtest_trade_cache` field (confirm no other consumer
  before deleting from `LocalState`), `token_cache` freshness keying, remaining
  `BACKTEST_FETCH_*` consts.
- [ ] Check whether `token-first-slot-activity-plan.md` Stage 7's `tokens_schema()`
  changes have landed; if so, they're included automatically. If not, proceed —
  that plan can do its own follow-up re-export later (not blocking).
- [ ] **One-time re-export:** `cargo run -p lab -- lake-export` (full) after
  confirming lake size, so all day files carry `tx_signature` uniformly (removes
  the `union_by_name` null-fill dependence). Then a `--include-today` pass.

**Done when:**
- [ ] `cargo check -p lab` clean.
- [ ] All three simulate paths lake-fed.
- [ ] The lake `DESCRIBE` shows `tx_signature` non-null across all days.
- [ ] No `find_by_mints_all` call remains on any simulate path (grep to confirm).

## Stage 7 — Parity test + docs + memory

**Session kickoff:** "Execute Stage 7 — add the simulate-vs-sweep parity test and
update all docs/memory per the plan's 'Docs to update' section."

**Files:**
- new test (`--ignored`) alongside `token_repo::parity_tests` style
- `@arch/database.md`, `@arch/sweep.md`, `@arch/strategies.md`,
  `@plans/tpsl-strategy/tpsl2-entry-exit-params.md`, `CLAUDE.md`
- memory: `real-sol-reserves-offline`, `canonical-price-gmgn`, `pg-vs-lake-roles`

**Work:**
- [ ] Parity test: run the same rule through simulate and through a 1-rule grouped
  sweep over the **same mint set**; assert identical per-token entry/exit/pnl.
  `--ignored` (needs a lake).
- [ ] Docs: drop the "simulate reads PG / sweep reads lake" split everywhere; note
  the include-today export requirement in CLAUDE.md data-scale guardrails if it's
  now standing.
- [ ] Memory: simulate no longer reconstructs `real_sol_reserves` (reads lake's baked
  value); update the two pricing notes.

**Done when:**
- [ ] Parity test green (`--ignored`).
- [ ] `cargo check -p lab` + `cargo check -p live` clean.
- [ ] Docs + memory updated.
- [ ] Grep confirms no stale "simulate=PG" prose.

## Stage dependency graph

```
Stage 1 (lake tx_signature)
    └─> Stage 2 (SimTrade + load_sim, carries fp)
            └─> Stage 3 (fetch_sim_histories helper)
                    ├─> Stage 4 (tpsl1)  ┐
                    ├─> Stage 5 (tpsl2)  ├─ (4/5/6 independent, any order)
                    └─> Stage 6 (swing1 + re-export — coordinate with
                        token-first-slot-activity-plan.md Stage 7)
                            └─> Stage 7 (parity + docs)
```

Stages 4/5/6 depend only on Stage 3 and can run in any order (or parallel
sessions), but Stage 6 owns the shared PG-plumbing cleanup + re-export, so run it
**last** among the three. Stage 7 needs all three migrated.
