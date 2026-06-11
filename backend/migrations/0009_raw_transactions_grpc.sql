-- -------------------------------------------------------------------------
-- raw_transactions_grpc
--   Raw LaserStream (Yellowstone gRPC) transaction results, kept SEPARATE from
--   the WS `raw_transactions` table so the two raw formats never co-mingle while
--   both transports can run (toggled via INGEST_TRANSPORT). Stored as the adapted
--   jsonParsed-shaped JSON. Write-only — later-analysis source.
-- -------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS raw_transactions_grpc (
    id              UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    signature       TEXT        UNIQUE NOT NULL,
    slot            BIGINT      NOT NULL,
    block_time      TIMESTAMPTZ NOT NULL,
    raw_data        JSONB       NOT NULL,
    received_at     TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_raw_tx_grpc_signature ON raw_transactions_grpc(signature);
CREATE INDEX IF NOT EXISTS idx_raw_tx_grpc_slot      ON raw_transactions_grpc(slot DESC);
