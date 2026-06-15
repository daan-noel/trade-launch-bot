# Analysis — strategy param-sweep engine

File-level map of `backend/src/analysis/`. Reference this instead of re-reading
source. Related: [strategies.md](strategies.md) (the tpsl pure fns the sweep
reuses), [database.md](database.md) (`sweep_runs`/`sweep_results`),
[frontend.md](frontend.md) (the per-strategy sweep page).

## Core idea
A backtest is a pure fn `simulate(trades, params) -> TokenOutcome`. A sweep loads
the corpus **once**, calls `simulate` over combos × tokens in memory (no DB in
the loop), then **folds** the per-(combo, token) outcomes into one ranked metrics
row per param-pair combo. Those rows persist to `sweep_results`; the dashboard's
per-strategy sweep page reads them and sorts/filters client-side.

- **Goal:** find the most profitable / successful param pairs for a strategy.
  One global ranked table (no cohort grouping). TPSL2 is the first plug-in.
- **Decision parity:** each `Strategy::simulate` calls the *same* pure fns the
  live path uses, so backtest and live resolve identical entry/exit decisions.
- **PnL:** frictionless (pure price-to-price `round_trip`). A fees/slippage
  cost layer is a future hook, not yet modelled.

```
corpus (cache|DB, once) ─► sweep (pure simulate) ─► fold to per-combo metrics
  ─► sweep_results (DB) ─► /api/strategies/{s}/sweeps[...] ─► React sweep page
```

## `backend/src/analysis/`
| File | Owns |
|---|---|
| `mod.rs` | Module map + the parity note. |
| `strategy.rs` | `Strategy` + `ParamSpace` traits (the entire new-strategy surface); `TokenOutcome` (`Copy`, no String — mint recovered at fold), `ExitCode`, `SweepMethod` (Grid/Random/LHS), and the frictionless `round_trip` helper. |
| `corpus.rs` | `CorpusSource` trait + `CacheSource` (live `TokenCache`, zero-copy) and `DbSource` (own chunked, per-mint-capped batch query — reuses `trade_repo::TradeSlimRow`). Compact columnar Parquet corpus cache keyed by corpus hash; `Selection` (cap + window + curve_only). |
| `tpsl2.rs` | First `Strategy` impl. `Tpsl2Params`/`Tpsl2Axes`/`Tpsl2Strategy` overlays params onto a base `Tpsl2Rule` and calls `entry::find_scalp_entry` + `find_worst_case_paper_entry` + `exit::find_trade_driven_exit`. |
| `tpsl1.rs` | Peer `Strategy` impl (clone-parity). Entry via `entry::find_entry_fill_in_trades` (no scalp gates), same `exit::find_trade_driven_exit`. |
| `sweep.rs` | `run_sweep` — `rayon` over tokens (combos inner, slice stays cache-hot); a single fold thread folds outcomes into one `ComboAgg` per combo. Returns `SweepStats` + `Vec<ComboMetrics>`. |
| `aggregate.rs` | `ComboAgg` (streaming accumulator) → `ComboMetrics` (one ranked row): win rate, total/expectancy/median/mean/p90/best/worst PnL, profit factor, holding stats, exit-reason mix. |
| `cli.rs` | `run(pool, token_cache, args)` — the `sweep` subcommand: parse opts → resolve base rule → load corpus → `run_sweep` (spawn_blocking) → persist run + rows via `SweepRepo`. |

## Entry point
`backend -- sweep --strategy tpsl1|tpsl2 [--source cache|db] [--tokens N] [--method grid|random:N|lhs:N] [--rule <uuid>] [--curve-only]`
— dispatched in `main.rs` before trader init (needs only DB; `--source cache`
seeds a `TokenCache`). A backend subcommand, not a `bin/` target, because the
crate has no lib target. Writes one `sweep_runs` row + its `sweep_results` rows.

## Persistence + API
- `models/sweep.rs` — `SweepRun` / `SweepResult` (serialize-only API models).
- `storage/repositories/sweep_repo.rs` — `SweepRepo::save_run` (run + bulk rows,
  one transaction), `list_runs(strategy, limit)`, `list_results(run_id)`.
- `api/handlers/strategies/sweep.rs` — `GET /api/strategies/{strategy}/sweeps`
  (runs) and `…/sweeps/{run_id}/results` (all ranked rows for a run).

## Invariants
- Adding a strategy = a new `Strategy` + `ParamSpace` impl only; corpus, sweep,
  aggregate, persistence, API, page stay untouched.
- Backtest and live share the entry/exit pure fns — a fix flows to both.
- Bound every load: `Selection` caps tokens + per-mint trades. A run's result set
  is bounded by combo count (hundreds–low thousands), so it is served whole and
  the table sorts/filters client-side.
