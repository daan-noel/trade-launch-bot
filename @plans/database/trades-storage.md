# Trades storage — table design (DB-only)

The high-volume append-only feed (the LaserStream transport *is* this table), and
**both live ingest and sync/backfill write to it**. It is a **typed projection of
[raw_txs](raw-txs-storage.md)** — anything not promoted to a typed column here is
re-derived from the raw payload. At massive write volume on a RAM-constrained box,
every byte/row and every index is a cost — so the goal is the narrowest *exact* row
that still serves mint/time reads, exactly-once dedup, **deterministic chain-execution
ordering**, and **wall-clock candle rollups**.

Scope: **table structure only** (no repos / Rust / ingest pipeline). Partitioning is
delegated to **TimescaleDB** (see below). Consistent with
[strategy-storage.md](strategy-storage.md) and
[token-storage.md](token-storage.md) conventions (natural keys, integer base
units, derive-don't-store).

---

## Design principles

1. **Integer base units, not floats.** Solana amounts are native u64; `DOUBLE` only
   represents integers exactly to 2^53 (~9.0×10¹⁵), and token amounts / reserves sit
   right at that ceiling — sums (volume) and products (PnL) drift. `BIGINT` lamports /
   raw units are exact, same 8 bytes, match the chain.
2. **No surrogate key; the dedup key *is* the PK.** Nothing references a trade by `id`;
   the unique dedup key must exist anyway → make it the PK, drop the UUID.
3. **Derive-don't-store.** `price_per_token = sol_amount / token_amount` is derived in a
   read view; `ix_labels` is derived from [raw_txs](raw-txs-storage.md). Neither is
   stored per row.
4. **One fact, one column.** No two columns encode the same thing (`trade_type` already
   carried direction → `ix_type` removed).
5. **Two time axes, two jobs.** `slot` (+`tx_index`,`leg_index`) is the **execution
   order**; `block_time` is the **wall-clock bucket/partition axis**. They are different
   facts — keep both (see Ordering).
6. **Compression makes low-cardinality TEXT ~free; high-cardinality strings are the
   weight.** `trade_type`/`venue` repeat a handful of values → columnar compression
   crushes them to near-zero, so keep them readable. The real on-disk cost is the
   high-cardinality 44/64-byte strings (`wallet_address`, `tx_signature`) — that's where
   the optimization levers point (see Heaviness).

---

## Ordering — the core of this table

A Solana block (= one `slot`) holds an **ordered list of transactions**, and that order
*is* the execution order — which is what moves a bonding-curve price. The authoritative,
fork-stable, **live/sync-identical** ordering key is a 3-tuple:

```text
(slot,  tx_index,  leg_index)
   │        │          └─ instruction/leg order within one transaction
   │        └──────────── transaction's position within the block  (intra-block order)
   └───────────────────── block order across the chain
```

Why a timestamp can never be the *order* key:

- **`block_time` is per-block AND second-resolution** — every tx in a slot shares one
  value, and multiple *slots* can share the same second. It can't order within a block
  *or* reliably between adjacent blocks. (It's still the right **bucket** axis — see
  Partitioning.)
- **`received_at` is ingest order, not chain order** — only an approximation even live,
  and **meaningless for sync** (backfill fetches in loop order). Stays dropped.
- **`tx_index`** (delivered by Yellowstone/LaserStream as the tx `index`; read from the
  block structure on sync) is the missing authoritative intra-block key. **Kept.**

`slot` is reliably present on every live update *and* every backfilled tx → it is the
ordering backbone. `block_time` returns **only** as the partition/bucket dimension, not
as an order key.

---

## Column decisions vs the original row

Anchored to a real row (`sol=0.444288877`, `token=11758458159300`,
`price=3.7784620311686276e-14`):

| Old column | Decision |
| --- | --- |
| `id UUID` | **drop** — unreferenced; PK is the dedup key |
| `ix_type` (`'Sell'`) | **drop** — ≡ `trade_type` |
| `price_per_token` | **drop → derive** (`= sol_amount / token_amount`) |
| `ix_labels` | **drop → derive** from [raw_txs](raw-txs-storage.md) on `(block_time, tx_signature)` |
| `sol_amount` / `token_amount` / `*_reserves` | → `BIGINT` base units |
| `block_time` | **keep, re-scoped** — partition + candle-bucket dimension (NOT an order key) |
| `received_at` | **drop** — not an order key; was a dup of the ingest clock |
| `tx_index` | **keep** — transaction position within the block (intra-block order) |
| `slot`, `leg_index`, `tx_signature`, `trade_type`, `venue`, `mint`, `wallet` | keep |

---

## Target table

```sql
CREATE TABLE IF NOT EXISTS trades (
    mint_address           TEXT        NOT NULL,
    wallet_address         TEXT        NOT NULL,
    trade_type             TEXT        NOT NULL CHECK (trade_type IN ('buy','sell')),
    venue                  TEXT        NOT NULL DEFAULT 'curve'
                                            CHECK (venue IN ('curve','amm')),

    -- amounts as integer base units (exact, matches chain u64)
    sol_amount             BIGINT      NOT NULL,   -- lamports
    token_amount           BIGINT      NOT NULL,   -- raw token units
    virtual_sol_reserves   BIGINT,
    virtual_token_reserves BIGINT,
    real_sol_reserves      BIGINT,
    real_token_reserves    BIGINT,

    -- ordering key (slot, tx_index, leg_index); block_time = bucket/partition axis
    slot                   BIGINT      NOT NULL,   -- block ordinal (true execution order)
    tx_index               INTEGER     NOT NULL,   -- transaction position within the block
    leg_index              SMALLINT    NOT NULL DEFAULT 0,  -- instruction/leg order within the tx
    block_time             TIMESTAMPTZ NOT NULL,   -- wall-clock; partition + candle bucket

    tx_signature           TEXT        NOT NULL,   -- (BYTEA lever — see Heaviness)

    PRIMARY KEY (block_time, tx_signature, leg_index)  -- partition col first; IS the dedup key
);

-- Per-mint chronological reads with exact intra-block order (serves hot/recent chunks).
CREATE INDEX IF NOT EXISTS idx_trades_mint_order
    ON trades(mint_address, slot, tx_index, leg_index);
```

Notes

- PK `(block_time, tx_signature, leg_index)` doubles as exactly-once dedup
  (`ON CONFLICT DO NOTHING`): `block_time` is deterministic per confirmed tx and the
  signature is globally unique, so re-delivery collides exactly.
- **`block_time` NOT NULL is an ingest constraint** — it's the partition key, can't be
  NULL. If a live tx update lands before its block-meta, stamp `block_time` from a small
  in-memory `slot→block_time` map (you already consume block-meta for `tx_index`).
- **No BRIN** — Timescale chunk exclusion does the time-range pruning the BRIN used to.
- The mint index mainly serves **recent uncompressed chunks** (the live trading read
  path); historical chunks are served by compression's `segmentby`/`orderby` metadata.
- No `wallet_address` index (cohort logic reads the in-memory runtime cache).

---

## Partitioning — TimescaleDB hypertable on `block_time`

Replace the hand-rolled daily partitions (`ensure_/drop_trades_partition`, the pre-create
`DO` block, the maintenance task, the BRIN) with a hypertable chunked on `block_time` — a
**timestamptz** dimension, so retention/compression use native `now()` + `INTERVAL` (no
`set_integer_now_func`, no `current_chain_slot`), and **continuous aggregates can bucket
on it** (the reason for keeping the timestamp at all).

```sql
SELECT create_hypertable('trades', by_range('block_time', INTERVAL '1 day'));

-- Columnar compression — the big win on a RAM/IO-bound box (often 10–20×).
-- segment by mint (low-cardinality → stored once per segment); order by execution key.
ALTER TABLE trades SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'mint_address',
    timescaledb.compress_orderby   = 'slot, tx_index, leg_index'
);
-- compress_after MUST exceed the sync/backfill lookback (see Gotchas).
SELECT add_compression_policy('trades', compress_after => INTERVAL '7 days');

-- Declarative retention replaces drop_trades_partition.
SELECT add_retention_policy('trades', drop_after => INTERVAL '30 days');
```

Gotchas

- **`compress_after` > backfill horizon.** Sync writes historical trades; if it targets
  an already-compressed chunk, the insert is slow (and was disallowed on older Timescale).
  Keep the compression window wider than how far back sync reaches.
- **Unique/PK must include the partition column** (`block_time`) — it does.
- **Compressed chunks are append-tolerant but update/delete-hostile** — fine here
  (trades are immutable once written).
- Apply the same hypertable treatment to the sibling
  [raw_txs](raw-txs-storage.md) feed (also on `block_time`, shorter retention).

---

## Continuous aggregates — wall-clock OHLCV candles

The reason `block_time` is the dimension: a continuous aggregate's `time_bucket` **must**
be on the hypertable's partition column. Native, auto-refreshed per-mint candles:

```sql
CREATE MATERIALIZED VIEW trades_ohlcv_1m
WITH (timescaledb.continuous) AS
SELECT
    mint_address,
    time_bucket(INTERVAL '1 minute', block_time)                      AS bucket,
    -- open/close by execution order within the bucket → order by slot (not block_time,
    -- which is second-resolution); slot resolves all but same-slot boundary ties (immaterial).
    first(sol_amount::double precision / NULLIF(token_amount,0), slot) AS open_price,
    max(sol_amount::double precision / NULLIF(token_amount,0))         AS high_price,
    min(sol_amount::double precision / NULLIF(token_amount,0))         AS low_price,
    last(sol_amount::double precision / NULLIF(token_amount,0), slot)  AS close_price,
    sum(sol_amount)                                                    AS volume_lamports,
    count(*)                                                           AS trade_count
FROM trades
GROUP BY mint_address, bucket
WITH NO DATA;

SELECT add_continuous_aggregate_policy('trades_ohlcv_1m',
    start_offset      => INTERVAL '10 minutes',
    end_offset        => INTERVAL '1 minute',
    schedule_interval => INTERVAL '1 minute');
```

- Coarser candles (5m/1h) are cheap **hierarchical** CAggs built *on* `trades_ohlcv_1m`,
  not re-scans of `trades`.
- **Feeds `tokens_info`:** `volume`/`ath_price` can be maintained incrementally off this
  rollup instead of scanning raw trades — ties into
  [token-storage.md](token-storage.md).

---

## Read view — derived price

```sql
CREATE OR REPLACE VIEW trades_priced AS
SELECT
    t.*,
    (t.sol_amount::double precision / NULLIF(t.token_amount, 0)) AS price_per_token
FROM trades t;  -- lamports per raw unit; ×10^decimals at display for human price
```

---

## Heaviness — optimization levers (massive-volume)

Post-compression, the row cost is dominated by **high-cardinality strings**. Levers,
roughly highest payoff first:

1. **`tx_signature` → `BYTEA` (64 raw bytes vs 88 base58).** ~27% smaller on the single
   widest high-cardinality column, and it barely compresses (random bytes), so the saving
   is permanent. Cost: not eyeball-readable in `psql`. Recommended at this scale.
2. **Intern `wallet_address` → `wallet_id INTEGER`** (dict table `wallets(id, address)`.
   4 bytes vs 44, and high-cardinality wallets compress poorly as `orderby` (they're not
   the `segmentby`). Cost: a hot-path-ish join for wallet display. Biggest *structural*
   win; weigh against the join.
3. **`leg_index SMALLINT`** (done above) and confirm `tx_index` fits `INTEGER` (it does —
   per-block tx counts are well under 2³¹).
4. **Reserves audit.** The four `*_reserves` are 32 bytes/row. `virtual_*` drive price;
   `real_*` track migration progress only. If nothing analyzes migration from `trades`,
   drop `real_*` (re-derivable from [raw_txs](raw-txs-storage.md)). Keep `virtual_*`
   on the hot read path. Domain call.
5. **Keep `trade_type`/`venue` as TEXT** — low-cardinality, compression makes them ~free;
   an enum buys almost nothing and costs ergonomics.

---

## Open design questions

- **`ix_labels`** — derived from [raw_txs](raw-txs-storage.md) (chosen). Promote a
  specific label back to a typed `trades` column only if you `WHERE` on it at volume *and*
  the derive window (raw_txs retention) is too short for that query.
- **`real_*_reserves`** — keep vs drop-and-derive (lever 4).
- **`tx_signature` / `wallet_address` encoding** — `BYTEA` / interning (levers 1–2).
- **`mint_address` width** — 44-byte base58 per row; an INT token ref would add a hot-path
  JOIN. Leaning keep denormalized (it's the `compress_segmentby` key → near-free post-compression).
- **Candle granularity** — 1m base CAgg + hierarchical 5m/1h (chosen). Sub-minute candles
  would bucket multiple slots/second; fine, but rarely needed for meme-coin UIs.
