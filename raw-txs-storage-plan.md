# Raw transactions storage — table design (DB-only)

> **STATUS: IMPLEMENTED.** The `raw_txs` hypertable, compression/retention policies,
> and `BYTEA` payload design described here are shipped as part of live-lab-remake
> Phase 1 (`d62111f`). This file is the canonical schema reference. Crate paths use
> `ingest-laserstream/src/` and `trading_core/src/storage/repositories/`.

The **source-of-truth feed**: the full, unparsed transaction payload. `trades` is a
*typed projection* of this table — `ix_labels`, and anything else you didn't promote
to a typed column, is **re-derived from here** by joining on `(block_time,
tx_signature)`. One raw tx : many trade legs.

This is the **heaviest write table in the system** (every tx, full payload), so the
whole design is "store the least possible, parse on demand, drop it soonest."

Scope: **table structure only** (no repos / Rust / ingest pipeline). Partitioning via
**TimescaleDB**, aligned with [trades-storage-plan.md](trades-storage-plan.md) so the
two feeds share a `block_time` axis (chunk-aligned joins, parallel retention).
Replaces the old `raw_transactions` table.

---

## Design principles

1. **Bytes, not JSONB.** The payload is only ever parsed in Rust — never queried
   *into* with SQL — so store the original wire bytes (`BYTEA`: protobuf / bincode),
   not `JSONB`. JSONB re-stores every key name on every row and expands the encoding;
   at this volume that is the single largest avoidable cost. Parse on read.
2. **One fact per row = one transaction.** PK is the tx identity. Legs are *not* rows
   here (they're rows in `trades`); a raw tx is the whole transaction.
3. **Shortest retention in the system.** Raw payload is replay/audit/derive-only —
   once `trades` is projected and the recent derive window has passed, it's dead
   weight. Drop it **well before** `trades` (e.g. 7 days vs 30).
4. **PK-only indexing.** Derive paths arrive carrying the trade's `(block_time,
   tx_signature)` → straight PK hit. No signature-only secondary index (pure write
   amplification on the heaviest table).
5. **Raw bytes for the signature too.** `BYTEA` 64-byte signature, not 88-byte base58
   `TEXT` — ~27% smaller, still globally unique, and high-cardinality so it barely
   compresses (the raw-bytes saving is permanent, not reclaimed by columnar compression).

---

## Target table

```sql
CREATE TABLE IF NOT EXISTS raw_txs (
    tx_signature  BYTEA       NOT NULL,             -- raw 64-byte sig (not base58 TEXT)
    slot          BIGINT      NOT NULL,             -- block ordinal
    block_time    TIMESTAMPTZ NOT NULL,             -- partition dimension (aligned w/ trades)
    tx_index      INTEGER     NOT NULL,             -- transaction position within the block
    payload       BYTEA       NOT NULL,             -- original encoded tx; parse in Rust, never in SQL
    source        SMALLINT    NOT NULL DEFAULT 0,   -- provenance only: 0=live 1=sync
    PRIMARY KEY (block_time, tx_signature)          -- partition col first; IS the dedup key
);
```

`PRIMARY KEY (block_time, tx_signature)` doubles as exactly-once dedup
(`ON CONFLICT DO NOTHING`): `block_time` is deterministic per confirmed tx and the
signature is globally unique, so re-delivery collides exactly. No other index.

---

## Partitioning — TimescaleDB hypertable on `block_time`

Same dimension as `trades` so chunks line up (joins stay chunk-local, both retention
policies sweep together).

```sql
SELECT create_hypertable('raw_txs', by_range('block_time', INTERVAL '1 day'));

-- Columnar compression. No natural low-cardinality segment key; order by chain position.
ALTER TABLE raw_txs SET (
    timescaledb.compress,
    timescaledb.compress_orderby = 'slot, tx_index'
);
-- Keep compress_after > how far back sync inserts raw payload (same gotcha as trades).
SELECT add_compression_policy('raw_txs', compress_after => INTERVAL '2 days');

-- Heaviest table → shortest retention.
SELECT add_retention_policy('raw_txs', drop_after => INTERVAL '7 days');
```

Notes / gotchas

- **Don't double-compress.** Store *uncompressed* protobuf `BYTEA` and let Timescale's
  columnar compression do the work — app-side gzip + Timescale compression wastes CPU
  for little gain and defeats chunk-level compression metadata.
- **`compress_after` > sync backfill horizon** — if sync writes deep history into an
  already-compressed chunk the insert is slow/blocked. With a 2-day window, sync that
  only backfills the last hour or two is safe; widen if it reaches further back.
- **Retention coupling:** raw_txs is the only place full payload lives. Anything you
  might want to re-derive *must* be derived inside its `drop_after` window. If a derive
  need outlives 7 days, either widen retention or promote that field to a typed column
  on `trades`.

---

## Relationship to `trades`

```text
raw_txs (1 row / tx)  ──<  trades (1 row / leg)
   PK (block_time, tx_signature)        FK-ish join key (block_time, tx_signature)
```

- `trades` carries `(block_time, tx_signature, leg_index)` — the join to `raw_txs` is
  a PK lookup; parse `payload`, pick the instruction at `leg_index` to rebuild
  `ix_labels` (or any other on-demand detail).
- No declared FK: `raw_txs` is retention-dropped earlier than `trades`, so a hard FK
  would either block retention or orphan-cascade live trades. Keep it a soft join.

---

## Open design questions

- **`source`** — kept for provenance/debugging (1 row-byte). Drop entirely if you never
  branch on live-vs-sync after ingest.
- **Retention split** — 7 days (raw) vs 30 (trades) is a starting point; the real driver
  is "how far back does any re-derive ever reach." Set raw retention to that horizon + margin.
- **`payload` codec** — protobuf (LaserStream wire form, zero re-encode) vs a tighter
  app bincode. Prefer storing the wire form verbatim: no re-encode cost on the hot path,
  and it's the exact bytes for faithful replay.
