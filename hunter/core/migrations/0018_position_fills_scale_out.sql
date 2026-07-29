-- 0018_position_fills_scale_out.sql — per-position fills ledger + scale-out aggregates.
--
-- WHY. Partial exits (tranched scale-out) need N sell legs inside one episode.
-- Durable truth is an append-only `position_fills` ledger; `strategy_positions`
-- keeps running aggregates (`sold_token_amount`, `exit_sol_lamports_total`,
-- `scale_stage`) so list/PnL queries stay JOIN-free. On `End`, existing exit_*
-- columns still stamp a SOL-weighted average so CLOSED_PRED / realized PnL keep
-- working. See `docs/roadmap/partial-exits-plan.md` §1b / §3.
--
-- INDEX. `uq_strategy_positions_exit_sig0` unique-indexed only
-- `(exit_tx_signatures->>0)`. With N exit legs that is no longer enough (a later
-- leg's sig could collide with another position's first). Uniqueness moves to
-- `position_fills.tx_signature` for non-empty sell sigs. Entry-side
-- `uq_strategy_positions_entry_sig0` is unchanged (still one buy).

ALTER TABLE strategy_positions
    ADD COLUMN IF NOT EXISTS sold_token_amount BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS exit_sol_lamports_total BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS scale_stage SMALLINT NOT NULL DEFAULT 0;

COMMENT ON COLUMN strategy_positions.sold_token_amount IS
    'Running sum of confirmed sell-leg raw token units (cache of position_fills).';
COMMENT ON COLUMN strategy_positions.exit_sol_lamports_total IS
    'Running sum of confirmed sell-leg SOL in lamports (cache of position_fills).';
COMMENT ON COLUMN strategy_positions.scale_stage IS
    'Next scale-out stage index (0 = pre-first partial / legacy full-bag).';

CREATE TABLE IF NOT EXISTS position_fills (
    position_id   UUID        NOT NULL REFERENCES strategy_positions(id) ON DELETE CASCADE,
    seq           INTEGER     NOT NULL,
    side          TEXT        NOT NULL CHECK (side IN ('buy', 'sell')),
    price         DOUBLE PRECISION NOT NULL,
    sol_lamports  BIGINT      NOT NULL,
    token_amount  BIGINT      NOT NULL,
    at            TIMESTAMPTZ NOT NULL,
    reason        TEXT,
    stage         SMALLINT,
    tx_signature  TEXT,
    PRIMARY KEY (position_id, seq)
);

CREATE INDEX IF NOT EXISTS idx_position_fills_position
    ON position_fills (position_id, seq);

-- Real (and any non-empty) sell sigs must be unique across positions — replaces
-- uq_strategy_positions_exit_sig0. Empty/NULL sigs (paper) are excluded.
CREATE UNIQUE INDEX IF NOT EXISTS uq_position_fills_sell_tx
    ON position_fills (tx_signature)
    WHERE side = 'sell' AND tx_signature IS NOT NULL AND tx_signature <> '';

DROP INDEX IF EXISTS uq_strategy_positions_exit_sig0;

-- Prefer the running SOL aggregate for closed realized PnL when set; fall back to
-- the stamped exit_lamports for legacy rows that predate the ledger.
DROP VIEW IF EXISTS strategy_position_pnl;
CREATE VIEW strategy_position_pnl AS
SELECT
    p.*,
    -- Prefer the running aggregate once any sell leg has landed (scale-out or
    -- single-leg); legacy End rows keep sold_token_amount = 0 and use exit_lamports.
    ((CASE WHEN p.sold_token_amount > 0
           THEN p.exit_sol_lamports_total
           ELSE p.exit_lamports
      END - p.entry_lamports)::float8 / 1e9) AS realized_pnl_sol,
    CASE WHEN p.entry_price > 0
         THEN (p.exit_price - p.entry_price) / p.entry_price * 100.0 END AS pnl_pct,
    CASE WHEN p.entry_time IS NOT NULL AND p.exit_time IS NOT NULL
         THEN EXTRACT(EPOCH FROM (p.exit_time - p.entry_time)) END     AS holding_secs,
    (p.status = 'End' AND p.exit_time IS NOT NULL)                     AS is_closed
FROM strategy_positions p;
