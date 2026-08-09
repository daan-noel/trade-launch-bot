# Raw transactions storage — table design (DB-only)

The **source-of-truth feed**: the full, unparsed transaction payload. `trades` is a
*typed projection* of this table — `ix_labels`, and anything else you didn't promote
to a typed column, is **re-derived from here** by joining on `(block_time,
tx_signature)`. One raw tx : many trade legs.

This is the **heaviest write table in the system** (every tx, full payload), so the
whole design is "store the least possible, parse on demand, drop it soonest."

Scope: **table structure only** (no repos / Rust / ingest pipeline). Partitioning via
**TimescaleDB**, aligned with [trades-storage.md](trades-storage.md) so the
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
   weight. Drop it **well before** `trades` (3 days vs 30 — see *Measured cost*).
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
SELECT add_compression_policy('raw_txs', compress_after => INTERVAL '1 day');

-- Heaviest table → shortest retention.
SELECT add_retention_policy('raw_txs', drop_after => INTERVAL '3 days');
```

Notes / gotchas

- **Don't double-compress.** Store *uncompressed* protobuf `BYTEA` and let Timescale's
  columnar compression do the work — app-side gzip + Timescale compression wastes CPU
  for little gain and defeats chunk-level compression metadata.
- **`compress_after` > sync backfill horizon** — if sync writes deep history into an
  already-compressed chunk the insert is slow/blocked. `db-incremental-sync.ps1` only
  *reads* the server, and reads work fine against compressed chunks, so the window is
  bounded by nothing here in practice.
- **Both policies compare against a chunk's `range_end`, not its start.** With 1-day
  chunks, `compress_after => 1 day` still leaves ~2 uncompressed days resident: the
  chunk covering `[Aug 8, Aug 9)` is only eligible once `now() > Aug 10`. Shortening
  `compress_after` therefore buys much less than it looks — **`drop_after` is the
  effective lever.**
- **Retention coupling:** raw_txs is the only place full payload lives. Anything you
  might want to re-derive *must* be derived inside its `drop_after` window. If a derive
  need outlives 3 days, either widen retention or promote that field to a typed column
  on `trades` — which is exactly why `trades.ix_labels` and `trades.fee_lamports` are
  columns written at decode time rather than derived on demand.

### Measured cost (live box, 2026-08-09)

The reason retention here is aggressive, and why compression is not the answer:

| Table | Uncompressed | Compressed | Total | Span | Compression |
| --- | --- | --- | --- | --- | --- |
| `raw_txs` | 4.5 GB (1 chunk) | 16.9 GB (5 chunks) | **21.5 GB** | 7 days | **~18%** |
| `trades` | 11.2 GB (8 chunks) | ~2 GB (9 chunks) | 13 GB | 17 days | **~84%** |

`raw_txs` was **58% of the whole database** for a feed nothing routinely reads, and
it barely compresses because `payload` is a single opaque `BYTEA`: it TOASTs (hence
the outsized `pg_toast_*` relations beside each chunk) and is already row-compressed
before columnar compression ever sees it, and there is no low-cardinality column to
`segmentby`. `trades` compresses ~6x precisely because it has repeated values to
segment and order on. **More compression cannot fix this table; less retention can.**

That is affordable because `raw_txs` is opt-in on the sync path
(`db-incremental-sync.ps1 -IncludeRawTxs`, **off by default**), so it is not the
workstation's source for anything, and `persist_raw` (an `app_settings` bool read in
`live/src/ingest/consumer.rs`) can switch the writes off entirely. At 1d/3d the table
settles near ~12 GB instead of 21.5 GB.

> Note when reading `pg_total_relation_size` output: it **already includes** TOAST,
> so the `pg_toast_*` rows that appear alongside chunk rows in a size ranking are the
> same bytes listed twice. Don't sum them.

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
- **Retention split** — 3 days (raw) vs 30 (trades), tightened from 7/30 on 2026-08-09
  after the sizing above. The real driver is "how far back does any re-derive ever
  reach"; set raw retention to that horizon + margin. If nothing re-derives at all,
  `persist_raw = false` is strictly better than any retention window.
- **`payload` codec** — protobuf (LaserStream wire form, zero re-encode) vs a tighter
  app bincode. Prefer storing the wire form verbatim: no re-encode cost on the hot path,
  and it's the exact bytes for faithful replay.
