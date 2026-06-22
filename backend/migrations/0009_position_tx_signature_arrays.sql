-- 1C: per-signature attribution (enables concurrent same-token positions safely).
--
-- Replace the single entry_tx / exit_tx TEXT columns on all FOUR position tables
-- (2 real + 2 paper — the `Position` model is shared, so the columns must exist on
-- every table even though only the real path populates them per-leg) with JSONB
-- signature *arrays*. A fill can be multi-leg (the exit already is across retries;
-- the entry will be once we scale into a position), and a single TEXT cannot hold
-- several legs. Single-leg today is just an array of length 1, so multi-leg later
-- needs no second migration.
--
-- Backfill: the old single value becomes a 1-element array; NULL/'' becomes '[]'
-- (an empty array falls back to the legacy net-balance confirm — recovery only).
--
-- Backstop: moving to an array drops the DB-level UNIQUE guards that the old
-- columns carried on the two REAL tables (entry_tx NOT NULL UNIQUE, exit_tx
-- UNIQUE) — the last line against double-recording the same buy/sell on the
-- real-money path. Restore them as PARTIAL UNIQUE expression indexes on the first
-- array element (the single-leg case), skipping the empty-array rows (unentered
-- Holding / un-sold positions). The paper tables keep no uniqueness (a paper run
-- re-trades the same token), matching the old schema.
--
-- Hot-path index: find_fill_by_signature and the per-signature sell-confirm both
-- filter the large, continuously-growing partitioned `trades` table by
-- (wallet, mint, tx_signature) on the entry/exit-confirm hot path — without a
-- supporting index that is a seq scan (data-scale guardrail). Non-unique parent
-- index ⇒ cascades to all (incl. future) partitions, like the other trades indexes.

CREATE INDEX IF NOT EXISTS idx_trades_wallet_mint_sig
    ON trades (wallet_address, mint_address, tx_signature);

-- ---------------------------------------------------------------------------
-- tpsl1_real_positions  (unique backstops on entry+exit first leg)
-- ---------------------------------------------------------------------------
ALTER TABLE tpsl1_real_positions
    ADD COLUMN IF NOT EXISTS entry_tx_signatures JSONB NOT NULL DEFAULT '[]',
    ADD COLUMN IF NOT EXISTS exit_tx_signatures  JSONB NOT NULL DEFAULT '[]';
UPDATE tpsl1_real_positions SET
    entry_tx_signatures = CASE WHEN entry_tx IS NOT NULL AND entry_tx <> ''
                               THEN jsonb_build_array(entry_tx) ELSE '[]'::jsonb END,
    exit_tx_signatures  = CASE WHEN exit_tx  IS NOT NULL AND exit_tx  <> ''
                               THEN jsonb_build_array(exit_tx)  ELSE '[]'::jsonb END;
ALTER TABLE tpsl1_real_positions DROP COLUMN entry_tx, DROP COLUMN exit_tx;
CREATE UNIQUE INDEX IF NOT EXISTS uq_tpsl1_real_positions_entry_sig0
    ON tpsl1_real_positions ((entry_tx_signatures->>0))
    WHERE jsonb_array_length(entry_tx_signatures) > 0;
CREATE UNIQUE INDEX IF NOT EXISTS uq_tpsl1_real_positions_exit_sig0
    ON tpsl1_real_positions ((exit_tx_signatures->>0))
    WHERE jsonb_array_length(exit_tx_signatures) > 0;

-- ---------------------------------------------------------------------------
-- tpsl2_real_positions  (unique backstops on entry+exit first leg)
-- ---------------------------------------------------------------------------
ALTER TABLE tpsl2_real_positions
    ADD COLUMN IF NOT EXISTS entry_tx_signatures JSONB NOT NULL DEFAULT '[]',
    ADD COLUMN IF NOT EXISTS exit_tx_signatures  JSONB NOT NULL DEFAULT '[]';
UPDATE tpsl2_real_positions SET
    entry_tx_signatures = CASE WHEN entry_tx IS NOT NULL AND entry_tx <> ''
                               THEN jsonb_build_array(entry_tx) ELSE '[]'::jsonb END,
    exit_tx_signatures  = CASE WHEN exit_tx  IS NOT NULL AND exit_tx  <> ''
                               THEN jsonb_build_array(exit_tx)  ELSE '[]'::jsonb END;
ALTER TABLE tpsl2_real_positions DROP COLUMN entry_tx, DROP COLUMN exit_tx;
CREATE UNIQUE INDEX IF NOT EXISTS uq_tpsl2_real_positions_entry_sig0
    ON tpsl2_real_positions ((entry_tx_signatures->>0))
    WHERE jsonb_array_length(entry_tx_signatures) > 0;
CREATE UNIQUE INDEX IF NOT EXISTS uq_tpsl2_real_positions_exit_sig0
    ON tpsl2_real_positions ((exit_tx_signatures->>0))
    WHERE jsonb_array_length(exit_tx_signatures) > 0;

-- ---------------------------------------------------------------------------
-- tpsl1_paper_positions  (no uniqueness — a run re-trades the same token)
-- ---------------------------------------------------------------------------
ALTER TABLE tpsl1_paper_positions
    ADD COLUMN IF NOT EXISTS entry_tx_signatures JSONB NOT NULL DEFAULT '[]',
    ADD COLUMN IF NOT EXISTS exit_tx_signatures  JSONB NOT NULL DEFAULT '[]';
UPDATE tpsl1_paper_positions SET
    entry_tx_signatures = CASE WHEN entry_tx IS NOT NULL AND entry_tx <> ''
                               THEN jsonb_build_array(entry_tx) ELSE '[]'::jsonb END,
    exit_tx_signatures  = CASE WHEN exit_tx  IS NOT NULL AND exit_tx  <> ''
                               THEN jsonb_build_array(exit_tx)  ELSE '[]'::jsonb END;
ALTER TABLE tpsl1_paper_positions DROP COLUMN entry_tx, DROP COLUMN exit_tx;

-- ---------------------------------------------------------------------------
-- tpsl2_paper_positions  (no uniqueness — a run re-trades the same token)
-- ---------------------------------------------------------------------------
ALTER TABLE tpsl2_paper_positions
    ADD COLUMN IF NOT EXISTS entry_tx_signatures JSONB NOT NULL DEFAULT '[]',
    ADD COLUMN IF NOT EXISTS exit_tx_signatures  JSONB NOT NULL DEFAULT '[]';
UPDATE tpsl2_paper_positions SET
    entry_tx_signatures = CASE WHEN entry_tx IS NOT NULL AND entry_tx <> ''
                               THEN jsonb_build_array(entry_tx) ELSE '[]'::jsonb END,
    exit_tx_signatures  = CASE WHEN exit_tx  IS NOT NULL AND exit_tx  <> ''
                               THEN jsonb_build_array(exit_tx)  ELSE '[]'::jsonb END;
ALTER TABLE tpsl2_paper_positions DROP COLUMN entry_tx, DROP COLUMN exit_tx;
