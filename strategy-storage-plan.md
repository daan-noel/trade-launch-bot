# Strategy storage — table design (DB-only)

Shared tables keyed by `strategy_id` — **rules**, **runs** (+ a 1:1 **metrics**
child), **positions** — so a new strategy adds **rows, not tables**.

Scope: **table structure only**. This plan covers the strategy rules / runs /
positions tables and nothing else (no repos, Rust, API, frontend; param-sweep,
`trades`, `raw_transactions`, and `tokens*` tables are separate concerns).

---

## Design principles

1. **Typed lifecycle, JSONB brain.** Columns the runner needs for *any* strategy
   (sizing, mode, caps, status, entry/exit fills) are typed. The
   strategy-specific matching + entry/exit gates live in a `params` JSONB. Rules
   are read once at activation (cached), never per-event — so JSONB params cost
   nothing at runtime and make new strategies free.
2. **Every position belongs to a run — real and paper alike.** `run_seq` is
   monotonic per `(rule, mode)` → "run #N".
3. **Snapshot the rule into the run.** `params_snapshot` freezes the full rule at
   activation, so editing or deleting a rule never corrupts past results.
4. **Raw facts in positions, rollup in a 1:1 metrics child, derive the rest.**
   Positions hold granular fills; `strategy_run_metrics` caches the aggregate once
   finalized (kept typed for sort/filter/aggregate, split out so `strategy_runs`
   stays lean); a view derives per-position PnL on read. No stored `pnl_sol` on
   positions (stale risk).
5. **One `positions` / one `runs` table, `mode` column.** Real-only double-sell
   safety is a *partial* unique index `WHERE mode='real'`, not a separate table.

---

## Table 1 — `strategy_rules`

```sql
CREATE TABLE IF NOT EXISTS strategy_rules (
    id                      UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    strategy_id             TEXT        NOT NULL,          -- 'tpsl1' | 'tpsl2' | 'swing' | …
    rule_name               TEXT        NOT NULL,

    -- Universal, typed knobs the orchestrator needs for any strategy.
    buy_amount              DOUBLE PRECISION NOT NULL,     -- notional per entry (SOL)
    trade_mode              TEXT        NOT NULL DEFAULT 'paper'
                                CHECK (trade_mode IN ('paper', 'real')),
    is_active               BOOLEAN     NOT NULL DEFAULT TRUE,
    max_concurrent_tokens   BIGINT,                        -- run governance (NULL = unlimited)
    max_total_tokens        BIGINT,                        -- run governance (NULL = unlimited)

    -- The strategy's "brain": token fingerprint + entry gates + exit gates +
    -- tolerance. Shape is per-strategy, validated in app, not by the DB.
    --   tpsl1 → {token:{…}, exit:{take_profit, stop_loss, trailing_stop_pct, …}}
    --   tpsl2 → tpsl1 keys + {entry:{min_age_secs, pullback_pct, …}, exit:{cohort_ratio}}
    params                  JSONB       NOT NULL DEFAULT '{}',

    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_strategy_rules_strategy ON strategy_rules(strategy_id);
CREATE INDEX IF NOT EXISTS idx_strategy_rules_active   ON strategy_rules(strategy_id, is_active);
```

Notes

- `strategy_id` has **no CHECK** — extension is the point; the valid set is owned
  by the app registry.
- Token fingerprint, entry gates, exit gates, tolerance → all in `params`.

---

## Table 2 — `strategy_runs`

One activation session of a rule (paper or real).

```sql
CREATE TABLE IF NOT EXISTS strategy_runs (
    id               UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    strategy_id      TEXT        NOT NULL,
    rule_id          UUID        REFERENCES strategy_rules(id) ON DELETE SET NULL,
    mode             TEXT        NOT NULL CHECK (mode IN ('real', 'paper')),
    run_seq          BIGINT      NOT NULL,                 -- monotonic per (rule, mode): "run #N"
    status           TEXT        NOT NULL DEFAULT 'Running'
                         CHECK (status IN ('Running', 'Finished', 'Stopped', 'Cancelled')),

    -- Full frozen rule (typed knobs + params) at activation. Source of truth for
    -- analysis even after the rule is edited or deleted.
    params_snapshot  JSONB       NOT NULL,
    max_total_tokens BIGINT,                               -- snapshot of the stop cap (NULL = unlimited)

    started_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at      TIMESTAMPTZ,

    UNIQUE (rule_id, mode, run_seq)
);

CREATE INDEX IF NOT EXISTS idx_strategy_runs_rule
    ON strategy_runs(rule_id, mode, started_at DESC);
CREATE INDEX IF NOT EXISTS idx_strategy_runs_strategy
    ON strategy_runs(strategy_id, mode, status, started_at DESC);
```

Notes

- `rule_id ON DELETE SET NULL`: deleting a rule preserves its runs (the truth is
  in `params_snapshot`). NULL `rule_id` is harmless in the unique (Postgres
  treats NULLs as distinct).
- Lifecycle + snapshot only; the finalize-time rollup lives in the 1:1
  `strategy_run_metrics` child below.

---

## Table 3 — `strategy_run_metrics`

Finalize-time rollup, one row per run — written once the run reaches a terminal
status. Kept fully **typed** (sort / filter / aggregate), split out so
`strategy_runs` stays lean. Same metric set as the param-sweep results so
live / paper / sweep analysis stay comparable. A missing row = run not yet
finalized.

```sql
CREATE TABLE IF NOT EXISTS strategy_run_metrics (
    run_id              UUID        PRIMARY KEY REFERENCES strategy_runs(id) ON DELETE CASCADE,
    rolled_up_at        TIMESTAMPTZ NOT NULL DEFAULT now(),

    -- counts
    n_fired             INTEGER     NOT NULL,
    n_open              INTEGER     NOT NULL,
    n_closed            INTEGER     NOT NULL,

    -- profitability / success
    win_rate            REAL        NOT NULL,
    total_pnl_sol       REAL        NOT NULL,
    expectancy_sol      REAL        NOT NULL,
    mean_pnl_pct        REAL        NOT NULL,
    median_pnl_pct      REAL        NOT NULL,
    p90_pnl_pct         REAL        NOT NULL,
    best_pnl_pct        REAL        NOT NULL,
    worst_pnl_pct       REAL        NOT NULL,
    std_pnl_pct         REAL        NOT NULL DEFAULT 0,
    profit_factor       REAL,                              -- NULL = no losing trades (∞)
    avg_holding_secs    REAL        NOT NULL,
    median_holding_secs REAL        NOT NULL,

    -- exit-reason mix (how closed trades terminated)
    n_exit_take_profit  INTEGER     NOT NULL,
    n_exit_stop_loss    INTEGER     NOT NULL,
    n_exit_trailing     INTEGER     NOT NULL,
    n_exit_stall        INTEGER     NOT NULL,
    n_exit_time         INTEGER     NOT NULL,
    n_exit_liquidity    INTEGER     NOT NULL,
    n_exit_cohort       INTEGER     NOT NULL,              -- 0 for strategies w/o cohort exit
    n_exit_open         INTEGER     NOT NULL
);

-- Leaderboards / "best runs" scans without touching strategy_runs.
CREATE INDEX IF NOT EXISTS idx_strategy_run_metrics_pnl ON strategy_run_metrics(total_pnl_sol DESC);
```

Notes

- 1:1 with `strategy_runs` (`run_id` is both PK and FK). NOT NULL within the row:
  if a metrics row exists, every aggregate is present — no half-finalized state.
- JOIN `strategy_runs` for the common "list runs with their PnL" view; the
  `idx_..._pnl` index serves "top runs" without the JOIN.

---

## Table 4 — `strategy_positions`

One position the bot opened within a run.

```sql
CREATE TABLE IF NOT EXISTS strategy_positions (
    id                      UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    run_id                  UUID        NOT NULL REFERENCES strategy_runs(id) ON DELETE CASCADE,
    strategy_id             TEXT        NOT NULL,           -- denormalized for cross-strategy scans
    rule_id                 UUID,                           -- denormalized for direct rule filter
    mode                    TEXT        NOT NULL CHECK (mode IN ('real', 'paper')),

    mint                    TEXT        NOT NULL,
    wallet                  TEXT        NOT NULL,
    token_program_id        TEXT,

    -- Amount type-by-meaning (see @arch/database.md): price = SOL per RAW token
    -- unit (ratio → float); *_token_amount = raw token units (BIGINT, exact integer);
    -- *_sol = lamports (BIGINT, exact). Models keep SOL as human f64 and convert at the
    -- repo boundary; *_token_amount is u64 end-to-end. Frontend scales for display.

    -- Optional trigger trade (someone else's trade that armed entry; scalp-style).
    target_price            DOUBLE PRECISION,
    target_token_amount     BIGINT,                        -- raw token units
    target_time             TIMESTAMPTZ,
    target_tx               TEXT,

    -- Entry fill (NULL until the buy lands).
    entry_price             DOUBLE PRECISION,
    entry_token_amount      BIGINT,                        -- raw token units (not SOL)
    entry_sol               BIGINT,                        -- lamports spent (true cost; incl. fees/slippage)
    entry_time              TIMESTAMPTZ,
    entry_tx_signatures     JSONB       NOT NULL DEFAULT '[]',

    -- Exit fill.
    exit_price              DOUBLE PRECISION,
    exit_token_amount       BIGINT,                        -- raw token units
    exit_sol                BIGINT,                        -- lamports received
    exit_time               TIMESTAMPTZ,
    exit_tx_signatures      JSONB       NOT NULL DEFAULT '[]',

    submitted_buy_signatures TEXT[]     NOT NULL DEFAULT '{}',  -- real-mode in-flight recovery
    status                  TEXT        NOT NULL
                                CHECK (status IN ('Arming','BuySubmitted','Holding',
                                                  'ExitPending','End','ExitFailed')),
    exit_reason             TEXT,                           -- TakeProfit | StopLoss | TrailingStop |
                                                            -- Stall | TimeStop | LiquidityExit | CohortExit
    extra                   JSONB       NOT NULL DEFAULT '{}',  -- strategy-specific overflow beyond target_*

    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Analysis / list indexes.
CREATE INDEX IF NOT EXISTS idx_strategy_positions_run            ON strategy_positions(run_id);
CREATE INDEX IF NOT EXISTS idx_strategy_positions_strategy_created
    ON strategy_positions(strategy_id, status, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_strategy_positions_rule_created   ON strategy_positions(rule_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_strategy_positions_mint_status    ON strategy_positions(mint, status);
CREATE INDEX IF NOT EXISTS idx_strategy_positions_token_program  ON strategy_positions(token_program_id);

-- In-flight recovery sweep (real mode).
CREATE INDEX IF NOT EXISTS idx_strategy_positions_buy_submitted
    ON strategy_positions(updated_at) WHERE status = 'BuySubmitted';

-- Double-sell / double-buy safety: unique on the first tx leg, REAL MODE ONLY.
-- Paper may replay the same token across runs, so it is excluded.
CREATE UNIQUE INDEX IF NOT EXISTS uq_strategy_positions_entry_sig0
    ON strategy_positions ((entry_tx_signatures->>0))
    WHERE mode = 'real' AND jsonb_array_length(entry_tx_signatures) > 0;
CREATE UNIQUE INDEX IF NOT EXISTS uq_strategy_positions_exit_sig0
    ON strategy_positions ((exit_tx_signatures->>0))
    WHERE mode = 'real' AND jsonb_array_length(exit_tx_signatures) > 0;
```

Notes

- `run_id ON DELETE CASCADE`: a run's positions die with the run. Deleting a
  *rule* does **not** delete positions (rule→run is SET NULL); purge a run to
  drop its positions.
- `target_*` kept as typed nullable columns (a "trigger trade" generalizes and
  stays queryable); anything more bespoke goes in `extra`.
- Volume is bounded — one row per **bot** trade, not per market trade — so **no
  partitioning** (unlike `trades` / `raw_transactions`).

---

## Analysis view — derived per-position PnL

PnL stays derived, never stored:

```sql
CREATE OR REPLACE VIEW strategy_position_pnl AS
SELECT
    p.*,
    -- exit_sol/entry_sol are lamports (BIGINT); ÷1e9 back to human SOL so the view
    -- matches StrategyPosition::realized_pnl_sol() (f64 SOL).
    ((p.exit_sol - p.entry_sol)::float8 / 1e9)                        AS realized_pnl_sol,
    CASE WHEN p.entry_price > 0
         THEN (p.exit_price - p.entry_price) / p.entry_price * 100.0 END AS pnl_pct,
    CASE WHEN p.entry_time IS NOT NULL AND p.exit_time IS NOT NULL
         THEN EXTRACT(EPOCH FROM (p.exit_time - p.entry_time)) END     AS holding_secs,
    (p.status = 'End' AND p.exit_time IS NOT NULL)                     AS is_closed
FROM strategy_positions p;
```

Run-level aggregates read straight off `strategy_run_metrics` (the cached rollup,
JOINed to `strategy_runs` for lifecycle context); ad-hoc / deeper cuts query this
view filtered by `run_id` / `rule_id` / `strategy_id`.

---

## Open design questions

- **`tolerance_pct`** — keep in `params`, or promote to a typed column? (Leaning
  `params`: strategy-specific, not orchestrator-needed.)
- **`exit_reason` vocabulary** — free TEXT (extensible) vs CHECK set. Leaning free
  TEXT so a new strategy's exit reason needs no DDL.
