-- ============================================================================
-- 0003 — backfill `position_fills` for pre-ledger positions.
--
-- Positions booked before mig 0018 (or whose sink write never appended a ledger
-- row) still carry entry/exit on `strategy_positions`, so the chart can mark
-- them while `GET …/fills` returns []. Reconstruct one buy (+ one sell for End)
-- from those snapshots. Idempotent: skips positions that already have any fill,
-- and skips sell sigs that already exist under the partial unique index.
--
-- Pure data rewrite — drop this file (keep the rationale as a comment) if the
-- chain is ever re-squashed into 0001_init.sql.
-- ============================================================================

-- Buy legs: any position with an entry snapshot and an empty ledger.
INSERT INTO position_fills (
    position_id, seq, side, price, sol_lamports, token_amount, at, reason, stage, tx_signature
)
SELECT
    p.id,
    1,
    'buy',
    p.entry_price,
    COALESCE(p.entry_lamports, 0),
    COALESCE(p.entry_token_amount, 0),
    p.entry_time,
    NULL,
    0,
    NULLIF(p.entry_tx_signatures->>0, '')
FROM strategy_positions p
WHERE p.entry_time IS NOT NULL
  AND p.entry_price IS NOT NULL
  AND NOT EXISTS (
      SELECT 1 FROM position_fills f WHERE f.position_id = p.id
  );

-- Sell legs: closed End rows that still have no sell in the ledger.
-- Prefer the scale-out aggregate when any sell was booked; else legacy exit_lamports.
INSERT INTO position_fills (
    position_id, seq, side, price, sol_lamports, token_amount, at, reason, stage, tx_signature
)
SELECT
    p.id,
    COALESCE((SELECT MAX(f.seq) FROM position_fills f WHERE f.position_id = p.id), 0) + 1,
    'sell',
    p.exit_price,
    CASE
        WHEN p.sold_token_amount > 0 THEN p.exit_sol_lamports_total
        ELSE COALESCE(p.exit_lamports, 0)
    END,
    COALESCE(NULLIF(p.exit_token_amount, 0), p.entry_token_amount, 0),
    p.exit_time,
    p.exit_reason,
    0,
    NULLIF(p.exit_tx_signatures->>0, '')
FROM strategy_positions p
WHERE p.status = 'End'
  AND p.exit_time IS NOT NULL
  AND p.exit_price IS NOT NULL
  AND NOT EXISTS (
      SELECT 1 FROM position_fills f WHERE f.position_id = p.id AND f.side = 'sell'
  )
  AND (
      NULLIF(p.exit_tx_signatures->>0, '') IS NULL
      OR NOT EXISTS (
          SELECT 1
          FROM position_fills f
          WHERE f.side = 'sell'
            AND f.tx_signature = p.exit_tx_signatures->>0
      )
  );
