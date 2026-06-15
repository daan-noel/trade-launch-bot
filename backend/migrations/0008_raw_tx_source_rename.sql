-- Rename `raw_transactions.source` values to name PROVENANCE, not transport.
-- The old 'grpc'/'rpc' labelled the wire protocol, which was misleading: the
-- token_sync "Fetch New" path arrives over LaserStream gRPC (replay) yet was
-- tagged 'rpc'. New vocabulary:
--     'grpc' -> 'live'  — real-time LaserStream ingest pipeline (DbWriter)
--     'rpc'  -> 'sync'  — token_sync backfill (both "Fetch All" via gTFA AND
--                         "Fetch New" via LaserStream replay)
-- Idempotent: the UPDATEs no-op once relabelled, and SET DEFAULT is repeatable.

-- Relabel existing rows (cascades to all partitions via the parent table).
UPDATE raw_transactions SET source = 'live' WHERE source = 'grpc';
UPDATE raw_transactions SET source = 'sync' WHERE source = 'rpc';

-- New rows always pass `source` explicitly, but keep the default meaningful.
ALTER TABLE raw_transactions ALTER COLUMN source SET DEFAULT 'live';
