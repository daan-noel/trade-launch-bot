# Strategy Param Sweep & Optimization — Plan

**Goal:** find the best param set *per token cohort*, for *any* strategy, efficiently.

- Group tokens by **fingerprint** (extensible feature vector) → optimize each cohort separately.
- One global "best" param set is misleading on a mixed population; a per-cohort best is not.
- Strategy-agnostic by design: TPSL2 is the first plug-in; new strategies with totally
  different entry/exit logic drop in without touching the sweep/scoring/fingerprint layers.

---

## Core idea

- A backtest is just `simulate(trades, params) -> outcome` — a pure function, no IO.
- So a sweep = load corpus **once** → call `simulate` many times in memory → aggregate.
- The DB is the only bottleneck; load once, never touch DB in the loop.

```
DB ──load once──► in-memory corpus ──► sweep (pure, no IO) ──► Parquet ──► Python analysis
```

- **Decision parity:** each strategy's `simulate` calls the *same* pure fns as live, so
  backtest and real trading resolve identical entry/exit *decisions* (when to buy/sell).
- **Economic parity (the hard part):** identical decisions ≠ identical PnL. Real fills lag
  the trigger, slip, pay fees/tips, and move thin markets. A sweep optimized on frictionless
  PnL picks params that evaporate live. The fill/cost model (below) + calibration step (Step 4b)
  make `simulate` PnL track realized PnL — this is what "results must match real trading" means.

---

## Fill & cost model — the bridge to real-trading parity

Frictionless backtest PnL is a lie the moment real SOL moves. Every simulated leg must apply
the same frictions the live path eats, or the sweep optimizes for a market that doesn't exist.
Model these explicitly (each a tunable param, defaults calibrated in Step 4b):

- **Entry fill = worst-case, not trigger price.** TPSL1 currently enters at the trigger
  trade's price (zero slip); only tpsl2 models worst-case fill. Generalize the tpsl2
  `find_worst_case_paper_entry` idea into the shared fill model: entry fills at the worst
  realistic price in the trigger's slot / slot+1 (latency window), never the optimistic tick.
- **Exit slip + latency.** A stop/trail/stall signal fires on trade *T*; the real sell lands a
  slot or more later, at the next available fill — usually worse on a falling token. Resolve the
  exit fill from the trade(s) **after** the signal within a latency window, not at the signal's
  own price.
- **Fees, tips, priority fees — a per-leg drag.** Each leg pays: base signature fee, priority
  fee (`cu_price × cu_limit`), Jito tip (escalating — see `pump-trader/jito_tip.rs`), and the
  AMM/pump fee on migrated tokens. Two legs per round-trip. For a high-frequency sniper on
  small size this drag can dominate gross PnL — ignoring it is the single biggest parity gap.
  Subtract real lamport costs per leg; source the actual tip/fee schedule from `pump-trader`.
- **Market impact / fillable size.** Your own order isn't in the replayed feed; on thin real
  liquidity your buy lifts and your sell craters the price. Bound fill by the trade's
  real-liquidity / size and apply slippage as a function of order size ÷ depth.
- **Fill probability.** Real buys sometimes never land (no position); sells retry. Model a
  miss/partial-fill rate rather than assuming 100% fill on every signal.
- **Where it lives:** one shared `FillModel` applied inside `simulate`, ideally the *same* code
  the live paper/real path uses to record fills — so a fix can't make backtest and live drift.
  Keep it a strategy-visible knob (different strategies trade different venues/sizes), but the
  default frictions are calibrated, not guessed.

## Performance & data scale (the corpus is huge — design for it)

The sweep touches each token's trades **once per combo** (combos × tokens reads). Memory
footprint and cache locality dominate, not the math. Decisions:

- **Compact columnar corpus, not `Vec<Trade>`.** Store only the fields `simulate` needs
  (ts, price, sol amount, is_buy, real-liquidity), struct-of-arrays per token, narrow
  numeric types (f32 / i32 / fixed-point). Cuts RAM several-fold and keeps each token's
  slice hot in L2/L3 across all combos. The full `Trade` struct never enters the sweep.
- **Cache-first source, DB fallback.** Load the recent/hot window straight from the live
  cache via `Arc<[Trade]>` (zero-copy clone, no DB) and only hit DB for the historical /
  cache-miss tail through `find_by_mints_paged`. Same `CorpusSource` trait behind both.
- **Loop order = parallelize over tokens, combos inner.** `rayon` over tokens; for each
  token run *all* combos before moving on, so its compact slice stays cache-resident.
- **No per-call allocation.** `simulate` returns a small `Copy` `TokenOutcome`; reuse
  scratch buffers, no heap churn inside the hot loop.
- **Streaming emit, bounded RAM.** combos × tokens rows can reach 1e9 — never hold them.
  Write Parquet row-group-by-row-group as each token finishes; two-stage coarse→refine
  keeps combo count sane. Log the projected row count and refuse to silently truncate.

---

## Layout

```
backend/src/analysis/
  corpus.rs       # CorpusSource (cache | DB) → compact columnar TokenTrades, cached to Parquet by corpus-hash
  fingerprint.rs  # per-token feature vector + cohort bucket (computed once, extensible)
  strategy.rs     # Strategy + ParamSpace traits — the only thing a new strategy implements
  sweep.rs        # rayon over combos × tokens → Vec<TokenOutcome>
  emit.rs         # TokenOutcome rows → Parquet
backend/src/bin/
  sweep.rs        # CLI: load corpus → sample params → sweep → write Parquet
analysis/         # Python: Polars/DuckDB scoring notebook + charts
```

---

## Step 1 — Corpus loader (`corpus.rs` + `bin/sweep` skeleton)

- Expose a `CorpusSource` trait with two impls behind one interface:
  - **cache** — pull the hot/recent window from the live `Arc<[Trade]>` cache, zero-copy, no DB.
  - **DB** — the historical / cache-miss tail via the **batch query** (shared primitive
    below): all selected tokens in one round-trip, replacing the per-token N+1 loop.
- Project each source's `Trade` into the **compact columnar `TokenTrades`** (sweep fields
  only); cache to `corpus.parquet` keyed by a corpus hash → re-runs load instantly (mmap), no DB.
- Reuse the existing Mayhem + token-criteria filters so the corpus matches live policy.
- **Scope the population — don't "load everything."** Loader takes an explicit selection
  (filter **+** token cap and/or `created` window). Parquet is the streaming boundary if
  it overflows RAM (write per-mint as fetched). Log population size + which bound clipped it;
  never silently truncate.

### Shared primitive — `trade_repo::find_by_mints_paged`

The sweep loader and the swing `/swings/batch` DB fallback want the same thing: *given a
set of mints, fetch each mint's trades, bounded per mint, in one query.* Build it once in
[trade_repo.rs](backend/src/storage/repositories/trade_repo.rs), reuse in both — fixes the
N+1 in `run_backtest` and in swing's DB fallback.

```rust
/// Trades for many mints in one round-trip, each mint capped to its newest
/// `per_mint_cap` trades, returned chronological per mint. The per-mint
/// ROW_NUMBER window stops one high-volume mint from blowing the result size.
pub async fn find_by_mints_paged(
    &self,
    mints: &[String],
    per_mint_cap: i64,
) -> anyhow::Result<Vec<Trade>>   // caller groups by mint_address
```

```sql
WITH ranked AS (
    SELECT <cols>,
           ROW_NUMBER() OVER (
             PARTITION BY mint_address
             ORDER BY slot DESC, block_time DESC, tx_signature DESC, leg_index DESC
           ) AS rn
    FROM trades
    WHERE mint_address = ANY($1)
)
SELECT <cols> FROM ranked
WHERE rn <= $2
ORDER BY mint_address, slot ASC, block_time ASC, tx_signature ASC, leg_index ASC
```

- **Sweep caller** passes a large cap, chunks the mint list (one `ANY($1)` per chunk so a
  statement's result stays bounded), groups the flat `Vec<Trade>` into `TokenTrades`.
- **Swing caller** ([swing.rs](backend/src/api/handlers/tokens/swing.rs#L223)) makes **one**
  call for the cache-miss mints instead of `find_by_mint_paged`-per-mint, reusing the
  `SWING_DB_TRADE_CAP` window as the cap. Merges with cache hits.
- Offline/DB path only — does **not** touch the live DashMap (that's a separate fix).

## Step 2 — Fingerprints (`fingerprint.rs`)

- Compute **once per token**, cheap and explainable; the feature set is **extensible** —
  add a field without changing any other layer.
  - lifespan, trade count, ATH multiple, secs-to-peak, max drawdown
  - peak real liquidity, cohort dump behaviour, organic-flow share
  - coarse shape label: spike-and-die / continuation / flat / slow-bleed
- Assign each token a **cohort bucket** (rule-based first; ML clustering later, same interface).
- Write `fingerprints.parquet` (mint → features + cohort).
- *Why:* cohorts are the unit of optimization — every score in Step 4 is per cohort.

## Step 3 — Strategy traits + sweep (`strategy.rs`, `sweep.rs`, `emit.rs`)

- Two small interfaces — **the entire surface a new strategy implements:**
  - `Strategy { fn simulate(&self, trades, params) -> TokenOutcome }`
  - `ParamSpace { fn sample(&self, method) -> Vec<Params> }`
- `simulate` is a black box: it owns its *own* entry/exit logic. A strategy with completely
  different mechanics (e.g. momentum, mean-reversion, time-based) just returns a `TokenOutcome`
  — the sweep/scoring/fingerprint layers never know the difference.
- TPSL2 = first impl, wrapping its existing pure entry/exit fns.
- Sweep method (pluggable behind `SweepMethod`): coarse grid on high-leverage params,
  random/Latin-hypercube on the rest; two-stage coarse → refine.
- `rayon`-parallel **over tokens** (combos inner, slice stays cache-hot); `simulate`
  returns a `Copy` `TokenOutcome`, no per-call heap. Stream rows to Parquet as each token
  finishes — never buffer all combos × tokens. Emit per-(combo, token) rows:
  - `outcomes.parquet` — combo_id, mint, pnl_percent, pnl_sol, exit_reason, holding_secs, entry_time
  - `combos.parquet`   — combo_id → strategy id + full param set + sweep method

## Step 4 — Scoring notebook (`analysis/`, Python + Polars/DuckDB)

- Join `outcomes × combos × fingerprints` — no re-simulation.
- Score each combo **per cohort** with a robust vector, never raw total PnL:
  - median pnl%, win rate, mean/σ (Sharpe-like), max drawdown
  - **n firing tokens** — drop `n < ~30` as hypothesis-only (anti curve-fit)
  - **OOS − IS gap** — optimize on early time-half, report on late half; big gap = overfit
- Output a ranked per-(strategy, cohort) table + the "why": PnL, exit-reason mix, equity
  curve, and the tokens that drove the result.

## Step 4b — Calibration & parity validation (gates the whole pipeline)

A sweep is only trustworthy once `simulate` reproduces trades you've **actually executed**.
This step is the empirical proof that backtest ≈ real — run it before believing any sweep output,
and re-run whenever the fill/cost model or the live execution path changes.

- **Replay the real book.** Take every closed `tpsl{1,2}_real_positions` row (it stores
  entry/exit `{price,amount,time,tx}` + `exit_reason`), re-simulate those exact tokens with the
  rule's params + the fill/cost model, and join simulated vs realized **per position**.
- **Report the parity error**, not a vibe: distribution of `sim_pnl − real_pnl` (median, p90,
  σ), exit-reason agreement (did sim exit for the same reason at ~the same time?), and entry/exit
  fill-price error. A systematic bias (sim always rosier) means the cost model is too generous.
- **Tune the fill/cost params to close the gap** — slippage bps, latency slots, tip/fee schedule,
  fill-miss rate — until the per-position error is small and unbiased. These calibrated defaults
  then feed every sweep, so cohort optima inherit real-world realism.
- **Make it a standing check, not a one-off.** A cheap CI/manual harness (`bin/sweep --calibrate`)
  that prints the parity error on the recent real book; a regression here = the model drifted from
  live and sweep results are no longer trustworthy. Decision parity is already guarded by shared
  pure fns; this guards economic parity.

## Step 5 — Router (later)

- At entry time, map a token's fingerprint → its cohort's best param set (and best strategy).
- Swap rule-based buckets for ML clusters behind the same bucket interface, no engine change.

---

## Guardrails

- Anything chosen against thin samples (`n < ~30`) is **paper-only**, never real SOL.
- Always report **in-sample vs out-of-sample** side by side — in-sample-only wins are overfit.
- **No params go to real SOL until Step 4b passes.** A combo that wins only on frictionless
  PnL is unproven; require the calibrated fill/cost model and a small, unbiased sim-vs-real
  parity error first. Frictionless backtest PnL is an upper bound, never the expected result.
- **Backtest and live must share the fill/cost code.** If `simulate` and the paper/real path
  compute fills differently, parity rots silently — same leak rule as the entry/exit fns.
- **Adding a strategy = a new `Strategy` + `ParamSpace` impl only.** Corpus, fingerprint,
  sweep, scoring, and router layers stay untouched. If a new strategy forces a change in any
  other layer, the abstraction leaked — fix the trait, not the layer.
