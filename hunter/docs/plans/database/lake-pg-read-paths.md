# Trade-history read paths: Parquet lake vs Postgres

Deep-dive for the data-scale rule in [CLAUDE.md](../../../CLAUDE.md) *"bound every query;
single-rule simulate reads the lake, not PG."* Explains which trade-history reads go to
the sealed Parquet lake, which stay on Postgres, and why. Related: `@arch/database.md`,
`@arch/sweep.md`, `@plans/token-analysis/swing-detection-logic.md`.

## The one deliberate full-history carve-out on PG

`GET /api/tokens/:mint/trades` (`get_trades` → `TradeRepo::find_by_mint_paged`,
`limit <= 0` ⇒ no `LIMIT`) returns a token's **full** trade history **on purpose**. The
inspect charts (Positions / Sim / grouped-sweep) resolve their entry/exit markers + swing
legs against this exact trade set, so a first-N cap mis-snapped the exit / later swing legs
off a high-volume token. It is still mint-scoped (never the whole table) and a cold,
deliberately-opened path — **don't re-add a row cap**. A positive `limit` still pages.

## Single-rule simulate reads the lake, not PG

tpsl1/tpsl2/swing1 `.../simulate` → `strategies::sim_fetch::fetch_sim_histories` →
`LakeSource::load` with `Selection::with_signatures = true` — the **same corpus + same
`SweepTrade`** the grouped sweep uses. There is no separate `SimTrade`; the flag only
populates `tx_signature` for Solscan links.

The grouped sweep loads it `false` and instead resolves its token-results table's
entry/exit signatures via a narrow indexed `(mint, slot, side)` PG lookup
(`grouped_sweep.rs`'s `resolve_fill_signatures` → `TradeRepo`) rather than carrying the
extra bytes through every sweep row. **That lookup plus the candidate-token scan are the
only remaining PG reads of the `trades` table anywhere in `lab`** — every other
trade-history path (grouped sweep, simulate, backtests) is lake-only.

## Sealed-days-only + the stale-lake warning

The lake is **sealed-days-only**, so keep
`cargo run -p hunter-lab -- lake-export --include-today` on a cadence or simulate on recent
(today's) tokens returns truncated histories (the loader logs a stale-lake warn). Parity
with the sweep is guarded by `lake::duck::parity_tests` (not `--ignored`: auto-runs
when `$SWEEP_LAKE_DIR` points at a populated lake, self-skips otherwise), and the
writer/reader lake column names are single-sourced in `lab/src/lake/schema.rs`.

## swing1-detect reads lake ∪ PG fresh tail

The per-token **`swing1-detect`** endpoint reads the token's **full history = sealed lake
∪ PG fresh tail** (`fetch_full_history_one`: lake for the deep past, plus every PG row on a
slot the lake never reached) via the shared `swing_1::funnel::build_swing1_funnel`.
Lake-only blanked the overlay for a token created after the last `lake-export` (entry/exit
markers still showed — they come from the PG fills, not the lake); the PG-tail union closes
that, and the lake still covers tokens older than PG's 30-day `trades` retention.

The swing1 backtest still **carries its legs** in the result row — so the *sim/sweep*
inspect chart's legs are the sim's own (no re-detect); only the *position* overlay
re-detects (and now sees the fresh tail).

### PG-tail rows must reconstruct real reserves (liquidity parity)

The program-emitted **`real_sol_reserves` is never persisted** — only the live decoder
sets it (`Trade::real_reserve_sol`); the DB keeps only the virtual reserve pair. The lake
already reconstructs it at load (`lab/src/lake/duck.rs` → `approx_real_sol_reserves(vsol,
venue)` = `vsol − 30` on curve, `== vsol` on amm). The **PG fresh tail must do the same**,
or a PG-read row's `real_reserve_sol` is `None` → the engine's `liquidity` metric is `NaN`
for every event (`liquidity` = last real reserve). That blanks the metric-panes liquidity
chart **and** makes any `liquidity >= X` entry gate unsatisfiable, so a profitable token
created *after* the last lake export (i.e. PG-tail-only) is silently never entered in
simulate/paper — even though live entered it fine (live has the decoder's real value).

`sweep/projection.rs::project_pg_tail` (used by both `fetch_full_history_one_opts` branches)
is `Trade`-concrete and derives `real_reserve_sol` via the SSOT `approx_real_sol_reserves`,
so a PG-tail row and its eventual sealed-lake copy compute identical liquidity/deadness. The
generic `project_trades` (lake corpus + tests) is unaffected — the lake source builds
`CorpusTrade` directly with the reconstruction already applied.

## Bounds are analysis-agnostic

`MAX_TRADES_RETAINED` is the **live in-RAM cache trim, never an analysis read bound** —
analysis reads full history. A batch analysis path resolves its whole mint list in **one**
`fetch_sim_histories` call rather than per-mint PG round trips.
