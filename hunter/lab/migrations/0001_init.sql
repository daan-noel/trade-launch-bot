-- ============================================================================
-- Consolidated lab-only schema (single-file init).
--
-- Squash of the entire lab migration chain into one end-state file — it creates
-- the schema exactly as running the full chain on a fresh database would leave it.
-- Two squash generations are absorbed:
--
--   * the legacy chain already folded into the previous `0001_grouped_sweep.sql`:
--       - 0001 grouped_sweep       tpsl1/tpsl2 four-table sets
--       - 0002 swing1 + next_kill  swing_1 set + shared n_exit_next_kill column
--       - 0003 drop cohort         n_exit_cohort removed from every _results table
--       + 0002/0003 trades_wallet_index (+ covering variant) and 0004 n_exit_dead
--
--   * the 0002..0013 chain layered on top of it:
--       0002 bucket_width_sol            (legacy tables only — dropped by 0005)
--       0003 generic_grouped_sweep       the ONE unprefixed grouped_sweep_* set
--       0004 open_pnl_sol                grouped_sweep_results.open_pnl_sol
--       0005 retire_legacy_sweep_tables  tpsl1 / tpsl2 / swing_1 sets dropped
--       0006 volume_ix_patterns          grouped_sweep_runs.volume_ix_patterns
--       0007 bucket_width_double         bucket_width_sol / buy_amount_sol -> f64
--       0009 sweep_fingerprint_id        grouped_sweep_runs.fingerprint_id
--       0010 sweep_fill_cost_model       fill_model / cost_model
--       0011 sweep_corpus_last_trade     corpus_last_trade_at
--       0012 exit_metric_slots           n_exit_metrics_by_slot
--       0013 sweep_scale_out_overlay     scale_out / scale_out_top_k
--
--   * the 0002 chain layered on top of THAT init:
--       0002 drop_n_exit_next_kill       grouped_sweep_results.n_exit_next_kill gone
--
-- 0008 (metric-group renames inside axes_spec / best_params / params) was a pure
-- data backfill over pre-existing rows — a no-op on a fresh database, so it is
-- intentionally not reproduced here.
--
-- NOTE (ledger): as with the core chain, collapsing this one changes the file's
-- checksum and removes version 2 from `_lab_migrations`. Reconcile an existing lab
-- database once with `scripts/consolidate-migration-ledgers.ps1`, and run
-- `scripts/squash-catchup.sql` FIRST — reconciling rewrites the ledger, never the
-- schema, so a folded-in migration that had not yet run would silently never run.
--
-- Written **only by `lab`** (the workstation analysis box); `live`/EC2 never
-- touches these, which is why they live in the lab-owned migration set (applied
-- via the lab-private `_lab_migrations` ledger, NOT the shared `_sqlx_migrations`).
--
-- The generic four-table set, names per `crate::sweep::registry`'s
-- `GroupedSweepTables`:
--   grouped_sweep_runs     one row per sweep run (header + lifecycle status)
--   grouped_sweep_groups   one row per surviving fingerprint group
--   grouped_sweep_results  one row per (group, ranked combo)
--   grouped_sweep_combos   per-run combo->params dictionary
--
-- Storage types mirror the repo's read/write code (RunDbRow / GroupDbRow /
-- ResultDbRow + the append/insert binds): results PnL/score floats are REAL (f32)
-- and the count columns INTEGER (i32); group best_score/expectancy and the run SOL
-- knobs are DOUBLE PRECISION.
--
-- NOTE (ledger): collapsing the chain changes this file's checksum and removes
-- versions 2..13 from `_lab_migrations`. An already-migrated database must be
-- reconciled once with `scripts/consolidate-migration-ledgers.ps1` before `lab`
-- will boot against it.
-- ============================================================================

CREATE TABLE IF NOT EXISTS grouped_sweep_runs (
    id                   UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    strategy_id          TEXT        NOT NULL,
    source               TEXT        NOT NULL,
    method               TEXT        NOT NULL,
    created_after        TIMESTAMPTZ,
    created_before       TIMESTAMPTZ,
    curve_only           BOOLEAN     NOT NULL,
    grouping_spec        JSONB       NOT NULL,
    axes_spec            JSONB       NOT NULL,
    min_tokens           INTEGER     NOT NULL,
    token_count          INTEGER     NOT NULL,
    group_count          INTEGER     NOT NULL,
    combo_count          INTEGER     NOT NULL,
    corpus_hash          TEXT,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    status               TEXT        NOT NULL DEFAULT 'running',
    groups_done          INTEGER     NOT NULL DEFAULT 0,
    ix_labels_filter     JSONB,
    field_filters        JSONB,
    token_cap            INTEGER,
    max_combos           INTEGER,
    label                TEXT,

    -- SOL knobs are DOUBLE PRECISION, not REAL (0007): as f32 a stored 0.1 widened
    -- back to f64 as 0.10000000149011612, and Promote / flow-discovery copied that
    -- noise straight into `fingerprints.bucket_size_amount`.
    buy_amount_sol       DOUBLE PRECISION,
    -- Per-run bucket width for the continuous SOL grouping axes (initial buy, max
    -- cost, spendable-in, first-slot buy/sell). It is the same width the created
    -- rule's live matcher and the creation-stats dashboard bucket by ("what you
    -- swept = what you run"), so it must persist for display, re-run and promotion.
    -- NULL on legacy rows => callers fall back to `grouping::SOL_BUCKET_WIDTH` (0.1),
    -- exactly the width those rows were swept at.
    bucket_width_sol     DOUBLE PRECISION,

    -- Optional volume-ix pattern set (0006, V2.2): when the axes reference
    -- `m_flow_split` / `m_flow_window`, the run carries an array-of-arrays of
    -- ordered label sequences. The fold compiles them corpus-wide into FlowPatterns;
    -- Promote writes the same set into the created fingerprint's `metric_config`.
    volume_ix_patterns   JSONB,

    -- Saved-fingerprint corpus scope (0009). The run keeps only tokens the engine
    -- MATCHES against that fingerprint (`hunter_engine::fingerprint::matches` — the
    -- SSOT the live entry gate uses). Not expressible as `field_filters` (those
    -- compare exact values, the engine matches by bucket), so without its own column
    -- a re-run / token-results reload would silently sweep the UNSCOPED corpus.
    -- Deliberately NO FK: deleting a fingerprint must not delete — or block
    -- deleting — the sweep history scoped by it; the UI falls back to the raw id.
    fingerprint_id       UUID,

    -- Pricing models (0010). The sweep used to hardcode the pessimistic
    -- worst-in-window fill AND charge slippage_bps on top of a fill model that
    -- already prices slippage — not a harmless constant pessimism: worst-in-slot
    -- penalises short holds hardest and the fixed per-leg cost scales with how often
    -- a combo fires, so the pair distorted the very comparison a sweep exists to
    -- make. Both are request inputs now, which makes them part of a run's IDENTITY.
    -- NULL on legacy rows, read back as the old behaviour (`worst_case` +
    -- `pumpfun_default`). Text (the serde tag), not an enum: these are wire values
    -- owned by the Rust types, and a CHECK would need migrating with every new model.
    fill_model           TEXT,
    cost_model           TEXT,

    -- Corpus freshness (0011): the newest trade the run actually saw, i.e. the
    -- corpus-wide `max(block_time)` captured once at load — the same instant the
    -- frozen-tail resolve anchors its horizon on. The sweep reads the sealed Parquet
    -- lake ONLY while `simulate` splices the fresh PG tail on top, so a stale export
    -- silently freezes sweep positions as `Open (est)` at hours-old prices while a
    -- simulate over the same rule watches those tokens die. NULL on rows written
    -- before this column existed (and on a trade-less corpus) — the UI shows
    -- "unknown" rather than inventing a time.
    corpus_last_trade_at TIMESTAMPTZ,

    -- Pass-2 fixed scale_out ladder overlay (0013). When set, each group's top-K
    -- combos are re-scored with this ladder after the cheap axes pass; Promote
    -- merges it into the draft params. NULL on legacy / no-overlay runs.
    scale_out            JSONB,
    scale_out_top_k      INT
);
CREATE INDEX IF NOT EXISTS idx_grouped_sweep_runs_created
    ON grouped_sweep_runs(created_at DESC);

CREATE TABLE IF NOT EXISTS grouped_sweep_groups (
    id                  UUID  PRIMARY KEY DEFAULT uuid_generate_v4(),
    run_id              UUID  NOT NULL REFERENCES grouped_sweep_runs(id) ON DELETE CASCADE,
    group_index         INTEGER          NOT NULL,
    group_key           JSONB            NOT NULL,
    token_count         INTEGER          NOT NULL,
    fired_count         BIGINT           NOT NULL,
    best_combo_id       INTEGER          NOT NULL,
    best_score          DOUBLE PRECISION,
    best_expectancy_sol DOUBLE PRECISION NOT NULL,
    best_params         JSONB            NOT NULL,
    mints               TEXT[]
);
CREATE INDEX IF NOT EXISTS idx_grouped_sweep_groups_run
    ON grouped_sweep_groups(run_id, best_score DESC NULLS LAST, group_index ASC);

CREATE TABLE IF NOT EXISTS grouped_sweep_combos (
    run_id   UUID    NOT NULL REFERENCES grouped_sweep_runs(id) ON DELETE CASCADE,
    combo_id INTEGER NOT NULL,
    params   JSONB   NOT NULL,
    PRIMARY KEY (run_id, combo_id)
);

CREATE TABLE IF NOT EXISTS grouped_sweep_results (
    run_id              UUID    NOT NULL REFERENCES grouped_sweep_runs(id) ON DELETE CASCADE,
    group_id            UUID    NOT NULL REFERENCES grouped_sweep_groups(id) ON DELETE CASCADE,
    combo_id            INTEGER NOT NULL,
    n_fired             INTEGER NOT NULL,
    n_open              INTEGER NOT NULL,
    n_closed            INTEGER NOT NULL,
    win_rate            REAL    NOT NULL,
    total_pnl_sol       REAL    NOT NULL,
    mean_pnl_pct        REAL    NOT NULL,
    median_pnl_pct      REAL    NOT NULL,
    p90_pnl_pct         REAL    NOT NULL,
    best_pnl_pct        REAL    NOT NULL,
    worst_pnl_pct       REAL    NOT NULL,
    std_pnl_pct         REAL    NOT NULL,
    profit_factor       REAL,
    score               REAL,
    expectancy_sol      REAL    NOT NULL,
    avg_holding_secs    REAL    NOT NULL,
    median_holding_secs REAL    NOT NULL,

    -- Unrealized (still-`Open`) PnL alongside the realized `total_pnl_sol` (0004).
    -- Every headline sweep stat is realized-only: `RunAgg` folds a still-Open
    -- position into n_fired/n_open but keeps its mark-to-last-price PnL out of
    -- total_pnl_sol/win_rate/score (an unrealized mark isn't a trade outcome, and
    -- folding it in made headline numbers depend on when the corpus window happened
    -- to end). That invariant stands; this column carries the excluded mark so the
    -- UI can show `total_pnl_sol + open_pnl_sol` beside the realized figure. It is
    -- NEVER summed into total_pnl_sol. Display only — `best_combo` still ranks on
    -- the realized `score`, so a combo that leaves its losers open still ranks as if
    -- they didn't exist; changing the ranking is a separate, deliberate decision.
    open_pnl_sol        REAL    NOT NULL DEFAULT 0,

    n_exit_take_profit  INTEGER NOT NULL,
    n_exit_stop_loss    INTEGER NOT NULL,
    n_exit_trailing     INTEGER NOT NULL,
    n_exit_stall        INTEGER NOT NULL,
    n_exit_time         INTEGER NOT NULL,
    n_exit_liquidity    INTEGER NOT NULL,
    -- No n_exit_next_kill: the swing1-only NextKill counter was retired with the
    -- named tpsl/swing stack (0002) and nothing emits that exit reason. Historical
    -- `strategy_positions.exit_reason = 'NextKill'` strings survive as opaque
    -- labels and roll into the UI's "Other" bucket.
    n_exit_dead         INTEGER NOT NULL DEFAULT 0,
    n_exit_metrics      INTEGER NOT NULL DEFAULT 0,

    -- Per-combo breakdown of `ExitCode::Metrics` by WHICH authored exit condition
    -- fired (0012). `n_exit_metrics` collapses every authored condition (`stall > 3`,
    -- `retrace >= 5`, `held >= 10`, …) into one bucket, because the per-combo
    -- aggregate is a fixed-size streaming accumulator held combos-wide in RAM
    -- (hundreds of thousands per run) — it can't afford a counter per distinct metric
    -- label without losing the O(1)-per-combo memory bound the sweep's RAM budget
    -- rests on. This keeps that bound: a FIXED-SIZE array (see
    -- `hunter_lab::sweep::strategy::N_EXIT_METRIC_SLOTS`, currently 8) indexed by the
    -- 0-based position of the rule's OWN authored exit reqs — not a global metric id —
    -- resolved once per combo at bind time (`BoundCombo::exit_metric_label`), zero
    -- extra per-token work. A rule with more than 8 conditions folds the overflow
    -- into the last slot, never worse than the old single bucket. ONE `INTEGER[]`
    -- column, not 8 scalars: `append_group`'s bulk insert already sits close to the
    -- 65535 bind-parameter ceiling on its 2000-row chunks; an array costs ONE bind.
    n_exit_metrics_by_slot INTEGER[] NOT NULL DEFAULT '{}',

    n_exit_open         INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_grouped_sweep_results_group
    ON grouped_sweep_results(run_id, group_id);

-- ---------------------------------------------------------------------------
-- Lab-only supporting index for the Trader Analysis query
-- (`TradeRepo::wallet_traded_mints` -> GET /api/wallets/:wallet/tokens).
--
-- Covering variant: seek by `wallet_id`, range-scan `block_time DESC`
-- (recent-first), with `mint_address` + `trade_type` trailing so the GROUP BY +
-- `COUNT(*) FILTER (WHERE trade_type=…)` are satisfied index-only (no heap fetch).
-- Lives in the lab-only migration set on purpose: only `lab` runs this analysis
-- query, so the EC2 `live` box's ingest hot path pays NO extra per-insert
-- index-maintenance cost.
-- ---------------------------------------------------------------------------
CREATE INDEX IF NOT EXISTS idx_trades_wallet_time
    ON trades (wallet_id, block_time DESC, mint_address, trade_type);
