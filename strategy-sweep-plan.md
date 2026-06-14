# Strategy Param Sweep & Optimization — Plan

Find the most profitable TPSL2 param sets from massive token+trade data, prove
*why* they win, and stay ready for more strategies later.

- **Compute:** Rust offline binary (reuses production pure fns) → Parquet → Python analysis.
- **Objective:** robust metric vector, per fingerprint cohort, with a train/test gap.
- **Parity:** the sweep calls the same `find_scalp_entry` / `find_trade_driven_exit`
  as live, so backtest and real trading resolve identical entries/exits.

---

## Core idea

- Entry/exit are already **pure functions of `(&[Trade], &Rule)`**.
- So a sweep is just: load data once → call those fns many times in memory → aggregate.
- The DB is the bottleneck, not the math — load the corpus **once**, never touch DB in the loop.

```
DB ──load once──► in-memory corpus ──► sweep (pure, no IO) ──► Parquet ──► Python analysis
```

---

## Layout

```
backend/src/analysis/
  corpus.rs       # DB → Vec<TokenTrades>, ONE batch query, cached to Parquet by corpus-hash
  fingerprint.rs  # per-token feature vector + coarse cohort bucket (computed once)
  strategy.rs     # Strategy + ParamSpace traits (TPSL2 = first impl, wraps existing pure fns)
  sweep.rs        # rayon over combos × tokens → Vec<TokenOutcome>
  emit.rs         # TokenOutcome rows → Parquet
backend/src/bin/
  sweep.rs        # CLI: load corpus → sample params → sweep → write Parquet
analysis/         # Python: Polars/DuckDB scoring notebook + charts
```

---

## Step 1 — Corpus loader (`corpus.rs` + `bin/sweep` skeleton)

- Add a **batch trade query** (the shared primitive below): fetch all selected
  tokens' trades in one round-trip (`WHERE mint_address = ANY($1)`), not the
  current per-token N+1 loop.
- Build `Vec<TokenTrades { token, trades: Vec<Trade> }>` in memory.
- Cache to `corpus.parquet`, keyed by a corpus hash → re-runs load instantly, no DB.
- Reuse the existing Mayhem + token-criteria filters so the corpus matches live policy.
- **Scope the population, don't "load everything."** At our scale the full
  corpus may not fit in RAM, so the loader takes an explicit selection — the
  Mayhem/criteria filter **plus** a token cap and/or `created` window — and
  Parquet is the streaming boundary if it overflows (write per-mint as fetched,
  never hold the whole `trades` table live). Log the population size + which
  bound clipped it; never silently truncate.
- _Bonus:_ this fixes the N+1 in today's `run_backtest` **and** gives swing
  detection's DB fallback a batch path (see below).

### Shared primitive — `trade_repo::find_by_mints_paged`

Both the sweep corpus loader and the swing `/swings/batch` DB fallback want the
same thing: *given a set of mints, fetch each mint's trades, bounded per mint,
in one query.* Build it once in [trade_repo.rs](backend/src/storage/repositories/trade_repo.rs)
and reuse it in both places.

```rust
/// Trades for many mints in one round-trip, each mint capped to its newest
/// `per_mint_cap` trades, all returned chronological per mint. Replaces the
/// per-mint `find_by_mint_paged` loop (N+1) on the batch paths. The per-mint
/// `ROW_NUMBER` window bounds the response so one high-volume mint in the set
/// can't blow the result size — same guard `find_by_mint_paged` gives per call.
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

Notes:

- **Per-mint cap, newest-kept, chronological-returned** — mirrors the existing
  `SWING_DB_TRADE_CAP` / `MAX_TRADES_RETAINED` window so the batch fallback
  serves the *same* launch→recent slice the live cache would have. Reuse that
  constant as the cap on the swing side.
- **Sweep caller** passes its own (larger) cap and groups the flat `Vec<Trade>`
  by `mint_address` into `TokenTrades`. One `ANY($1)` per corpus chunk; chunk
  the mint list so a single statement's result stays bounded.
- **Swing caller** ([swing.rs](backend/src/api/handlers/tokens/swing.rs#L223))
  collects the batch's cache-*miss* mints, makes **one** call instead of the
  current `find_by_mint_paged`-per-mint inside `buffer_unordered`, and merges
  with the cache hits. Kills that N+1 too.
- This is the offline/DB path only — it does **not** touch the live DashMap.
  The hot-cache lock-contention fix is separate (M5, `Arc<[Trade]>`); the two
  are independent and both wanted.

## Step 2 — Fingerprints (`fingerprint.rs`)

- Compute **once per token** (cheap, explainable features):
  - lifespan, trade count, ATH multiple, secs-to-peak, max drawdown
  - peak real liquidity, cohort dump behaviour, organic-flow share
  - a coarse shape label: spike-and-die / continuation / flat / slow-bleed
- Assign each token a **cohort bucket** (rule-based first; ML clustering later, same interface).
- Write `fingerprints.parquet` (mint → features + cohort).
- _Why:_ one global "best" param set is misleading on a mixed population — optimize **per cohort**.

## Step 3 — Strategy traits + sweep (`strategy.rs`, `sweep.rs`, `emit.rs`)

- Define two small interfaces so the engine is strategy-agnostic:
  - `Strategy { fn simulate(&self, trades, params) -> TokenOutcome }`
  - `ParamSpace { fn sample(&self, method) -> Vec<Params> }`
- TPSL2 = first impl, wrapping the existing `find_scalp_entry` / `find_trade_driven_exit`.
- Sweep method: coarse grid on the high-leverage params
  (entry liquidity, take-profit, stop-loss, min-age), random/Latin-hypercube on the rest;
  two-stage coarse → refine. Pluggable behind `SweepMethod`.
- `rayon`-parallel over combos × tokens.
- Emit per-(combo, token) rows to Parquet:
  - `outcomes.parquet` — combo_id, mint, pnl_percent, pnl_sol, exit_reason, holding_secs, entry_time
  - `combos.parquet`   — combo_id → full param set + sweep method

## Step 4 — Scoring notebook (`analysis/`, Python + Polars/DuckDB)

- Join `outcomes` × `combos` × `fingerprints` — no re-simulation needed.
- Score each combo **per cohort** with a robust vector, never raw total PnL:
  - median pnl%, win rate, mean/σ (Sharpe-like), max drawdown
  - **n firing tokens** — drop `n < ~30` as hypothesis-only (anti curve-fit)
  - **OOS − IS gap** — optimize on early time-half, report on late half; big gap = overfit
- Output a ranked per-cohort table + the "why": per-cohort PnL, exit-reason mix,
  equity curve, and the specific tokens that drove the result.

## Step 5 — Router (later)

- Map each token's fingerprint → its cohort's best param set **at entry time**.
- Swap rule-based buckets for ML clusters behind the same bucket interface, no engine change.

---

## Guardrails

- Anything chosen against thin samples (`n < ~30`) is **paper-only**, never real SOL.
- Always report **in-sample vs out-of-sample** side by side — a combo good only in-sample is overfit.
- Adding a new strategy = new `Strategy` + `ParamSpace` impl only; sweep / scoring /
  fingerprint / router layers are untouched.
