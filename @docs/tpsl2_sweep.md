# TPSL2 Sweep — param-sweep engine

File-level map of `backend/src/tpsl2_sweep/`. Reference this instead of
re-reading source. Related: [strategies.md](strategies.md) (the tpsl2 pure fns
the sweep reuses), [database.md](database.md)
(`tpsl2_sweep_runs`/`tpsl2_sweep_results`), [frontend.md](frontend.md) (the
TPSL2 sweep page).

**TPSL2-specific.** Other strategies get their own *separate* sweep module,
tables, endpoints, and page — clone-and-tweak this one rather than plugging a
second impl into a shared engine.

## Core idea
A backtest is a pure fn `simulate(trades, params) -> TokenOutcome`. A sweep loads
the corpus **once**, calls `simulate` over combos × tokens in memory (no DB in
the loop), then **folds** the per-(combo, token) outcomes into one ranked metrics
row per param-pair combo. Those rows persist to `tpsl2_sweep_results`; the
dashboard's TPSL2 sweep page reads them and sorts/filters client-side.

- **Goal:** find the most profitable / successful TPSL2 param pairs. One global
  ranked table (no cohort grouping).
- **Ranking:** each combo gets a `score` = `μ − 1.64·σ/√n` — the one-sided
  lower-confidence bound on **realized** (closed-trade) per-trade pnl%. It rewards
  a high mean while the `σ` term penalises dispersion and `√n` shrinks small
  samples, so a lucky few-token combo can't out-rank a steady edge. `score` is
  `NULL` when `n_closed < 2` (no evidence) and is the TPSL2 sweep page's default
  sort (desc; nulls sink). `std_pnl_pct` (the σ) is persisted alongside so `z`
  can be retuned client-side without re-sweeping.
- **Decision parity:** `Tpsl2Strategy::simulate` calls the *same* pure fns the
  live path uses, so backtest and live resolve identical entry/exit decisions.
- **PnL:** frictionless (pure price-to-price `round_trip`). A fees/slippage
  cost layer is a future hook, not yet modelled.

```
corpus (cache|DB, once) ─► sweep (pure simulate) ─► fold to per-combo metrics
  ─► tpsl2_sweep_results (DB) ─► /api/strategies/tpsl2/sweeps[...] ─► React page
```

**Two ways to run a sweep:** the offline `tpsl2-sweep` CLI (full rayon pool, DB
or DB-seeded cache), or `POST /api/strategies/tpsl2/sweeps` — runs **in the live
backend against the live `TokenCache`** (zero DB load for the corpus, exact
live-window parity), single-flight (`AppState.tpsl2_sweep_running`, 409 if busy),
on a bounded rayon pool so it can't starve the trading hot path.

## `backend/src/tpsl2_sweep/`

| File | Owns |
|---|---|
| `mod.rs` | Module map + the parity note. |
| `strategy.rs` | `Strategy` + `ParamSpace` traits; `TokenOutcome` (`Copy`, no String — mint recovered at fold), `ExitCode`, `SweepMethod` (Grid/Random/LHS), and the frictionless `round_trip` helper. |
| `corpus.rs` | `CorpusSource` trait + `CacheSource` (live `TokenCache`, zero-copy) and `DbSource` (own chunked, per-mint-capped batch query — reuses `trade_repo::TradeSlimRow`). Compact columnar Parquet corpus cache keyed by corpus hash; `Selection` (cap + window + curve_only). |
| `engine.rs` | `run_sweep` — `rayon` over tokens (combos inner, slice stays cache-hot); a single fold thread folds outcomes into one `ComboAgg` per combo. Returns `SweepStats` + `Vec<ComboMetrics>`. |
| `aggregate.rs` | `ComboAgg` (streaming accumulator) → `ComboMetrics` (one ranked row): win rate, total/expectancy/median/mean/p90/best/worst PnL, `std_pnl_pct`, profit factor, the robust `score` (`robust_score`: `μ−Z·σ/√n` on closed trades, `Z=SCORE_Z`), holding stats, exit-reason mix. |
| `tpsl2_strategy.rs` | The TPSL2 `Strategy` impl. `Tpsl2Params`/`Tpsl2Axes`/`Tpsl2Strategy` overlays params onto a base `Tpsl2Rule` and calls `entry::find_scalp_entry` + `find_worst_case_paper_entry` + `exit::find_trade_driven_exit`. |
| `cli.rs` | `run(pool, token_cache, args)` — the `tpsl2-sweep` subcommand. Also `run_cache_sweep(pool, token_cache, CacheSweepConfig)` — the **in-process, live-cache** sweep the HTTP trigger calls (loads corpus straight from the live `TokenCache`, bounded rayon pool, returns the persisted run). Shared `sweep_engine` (spawn_blocking → persist via `Tpsl2SweepRepo`) backs both; `max_threads: Some(n)` bounds the pool for the in-process path, `None` = full pool for the CLI. |

## Entry point
`backend -- tpsl2-sweep [--source cache|db] [--tokens N] [--method grid|random:N|lhs:N] [--rule <uuid>] [--curve-only]`
— dispatched in `main.rs` before trader init (needs only DB; `--source cache`
seeds a `TokenCache`). A backend subcommand, not a `bin/` target, because the
crate has no lib target. Writes one `tpsl2_sweep_runs` row + its
`tpsl2_sweep_results` rows.

## Persistence + API
- `models/tpsl2_sweep.rs` — `Tpsl2SweepRun` / `Tpsl2SweepResult` (serialize-only API models).
- `storage/repositories/tpsl2_sweep_repo.rs` — `Tpsl2SweepRepo::save_run` (run +
  bulk rows, one transaction), `list_runs(limit)`, `list_results(run_id)`.
- `api/handlers/strategies/tpsl2_sweep.rs` — `GET /api/strategies/tpsl2/sweeps`
  (runs), `…/sweeps/{run_id}/results` (all ranked rows for a run), and
  `POST …/sweeps` (`start_sweep` — single-flight in-process live-cache run; body
  `{rule_id?, tokens?, method?, curve_only?}`; returns the run, 409 if busy).

## Invariants
- Backtest and live share the entry/exit pure fns — a fix flows to both.
- Bound every load: `Selection` caps tokens + per-mint trades. A run's result set
  is bounded by combo count (hundreds–low thousands), so it is served whole and
  the table sorts/filters client-side.
- A new strategy's sweep is a *separate* clone of this stack (module + tables +
  endpoints + page), not a plug-in here.
