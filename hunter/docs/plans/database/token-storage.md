# Token storage — table design (DB-only)

Three tables, one row per token, **keyed by `mint_address` (natural key)** —
split by **write pattern**, not just by concept:

| Table | Write profile | Holds |
| --- | --- | --- |
| `tokens` | **write-once** (insert at creation) | static creation facts |
| `tokens_info` | **hot-updated** (every metric recompute) | live market metrics |
| `token_sync_state` | hot-updated (every sync) | per-venue ingest watermarks |

Scope: **table structure only** (no repos / Rust / API / frontend). Designed for
a clean rebuild — consistent with [strategy-storage.md](strategy-storage.md)
conventions (natural keys, typed-when-sorted, JSONB-when-variable, derive-don't-store,
child table for repeated groups).

---

## Design principles

1. **Split by write pattern.** `tokens` is immutable after creation; `tokens_info`
   is rewritten constantly. In Postgres MVCC every UPDATE writes a dead tuple, so
   keeping the immutable facts in their own table means a metrics update never
   rewrites (or vacuum-churns) the wide static row.
2. **Three concerns, not two.** Static creation facts, live market metrics, and
   sync watermarks change on different cadences and for different reasons — three
   tables.
3. **Natural key + FK, no surrogate UUID.** Everything joins on `mint_address`;
   nothing references a token `id`. `mint_address` is the PK; the metric/sync rows
   FK back to it (enforces 1:1, kills orphans, drops a redundant index per table).
4. **Cache scan-requiring aggregates; derive cheap arithmetic.** `ath_price`,
   `current_price`, `volume`, `trade_count` need a trade scan → cache them.
   `age`, `market_cap` are arithmetic over cached values → derive in a view, never
   store (stale the instant written).
5. **Typed metrics; JSONB only for variable shape.** Token lists sort/filter by
   metrics → typed columns. JSONB stays for variable creation data (`ix_labels`,
   `initial_buy_instruction`) + a `meta` escape hatch.
6. **Index `tokens_info` sparingly.** It is hot-updated; every index is write
   amplification on the metrics path. Index only what the token list actually
   sorts/filters by.

---

## Table 1 — `tokens` (static creation facts)

```sql
CREATE TABLE IF NOT EXISTS tokens (
    mint_address            TEXT        PRIMARY KEY,        -- natural, immutable, universal join key
    creator_wallet          TEXT        NOT NULL,

    -- metadata (fixed at creation)
    name                    TEXT        NOT NULL,
    symbol                  TEXT        NOT NULL,
    bonding_curve_address   TEXT,
    token_program_id        TEXT,

    -- launch economics (amounts are exact integers: supply = raw token units,
    -- initial_buy_sol = lamports; model keeps SOL as human f64, ÷1e9 on read.
    -- See @arch/database.md "Amount typing".)
    initial_supply_token    BIGINT,
    initial_buy_sol         BIGINT,                 -- lamports

    -- creation-tx compute budget
    cu_limit                BIGINT,
    cu_price                BIGINT,

    -- launch flags
    is_mayhem_mode          BOOLEAN     NOT NULL DEFAULT FALSE,
    is_cashback_enabled     BOOLEAN     NOT NULL DEFAULT FALSE,

    -- creation provenance (variable-shape → JSONB)
    creation_tx_signature   TEXT        NOT NULL,
    creation_slot           BIGINT,                 -- Solana slot of the create tx
    ix_labels               JSONB       NOT NULL DEFAULT '[]',
    initial_buy_instruction JSONB,

    meta                    JSONB       NOT NULL DEFAULT '{}',  -- future static attrs w/o a migration
    created_at              TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_tokens_creator_wallet      ON tokens(creator_wallet);
CREATE INDEX IF NOT EXISTS idx_tokens_created_at          ON tokens(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_tokens_token_program_id    ON tokens(token_program_id);
CREATE INDEX IF NOT EXISTS idx_tokens_is_mayhem_mode      ON tokens(is_mayhem_mode);
CREATE INDEX IF NOT EXISTS idx_tokens_is_cashback_enabled ON tokens(is_cashback_enabled);
```

Notes

- `mint_address` is the PK — the old surrogate `id UUID` + separate `UNIQUE` are gone.
- `name`/`symbol` treated as static (fixed at launch). If on-chain metadata renames
  become relevant, they move to `tokens_info` (dynamic) — see open questions.
- `creation_slot` is a write-once creation fact (known at `TokenCreated`). It is the
  slot key for the same-slot activity sums on `tokens_info` (`first_slot_*_sol`), so
  it lives on `tokens` (creation fact), while the derived-from-trades sums live on
  `tokens_info`.

---

## Table 2 — `tokens_info` (live market metrics)

```sql
CREATE TABLE IF NOT EXISTS tokens_info (
    mint_address   TEXT        PRIMARY KEY REFERENCES tokens(mint_address) ON DELETE CASCADE,

    -- cached aggregates (each requires a trade scan to recompute)
    current_price  DOUBLE PRECISION,
    ath_price      DOUBLE PRECISION,
    ath_timestamp  TIMESTAMPTZ,
    volume         DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    trade_count    BIGINT      NOT NULL DEFAULT 0,
    last_trade_at  TIMESTAMPTZ,

    -- same-creation-slot activity (derived from trades, streamed in TokenState;
    -- lamports, like initial_buy_sol — model keeps human f64, ÷1e9 on read).
    -- Sums buy/sell SOL over trades whose slot == tokens.creation_slot. Grows
    -- monotonically within the open window, then freezes → plain EXCLUDED overwrite
    -- on upsert (unlike ath's COALESCE-preserve).
    first_slot_buy_sol   BIGINT,                  -- lamports
    first_slot_sell_sol  BIGINT,                  -- lamports

    -- lifecycle (orthogonal axes; a token can be both)
    is_dead        BOOLEAN     NOT NULL DEFAULT FALSE,
    is_migrated    BOOLEAN     NOT NULL DEFAULT FALSE,
    -- seconds from creation to last meaningful trade; set only once is_dead.
    -- A cached point-in-time value (NOT live age) → kept, not derived.
    lifetime_secs  BIGINT,

    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now()      -- last metrics recompute
);

-- Hot-updated table → index only what the token list sorts/filters by.
CREATE INDEX IF NOT EXISTS idx_tokens_info_is_dead       ON tokens_info(is_dead);
CREATE INDEX IF NOT EXISTS idx_tokens_info_volume        ON tokens_info(volume DESC);
CREATE INDEX IF NOT EXISTS idx_tokens_info_last_trade_at ON tokens_info(last_trade_at DESC);
```

Notes

- PK = FK = `mint_address` → 1:1 with `tokens`, no orphans, no surrogate id, no
  separate mint index.
- **Dropped vs the old shape:** `age` (derive: `now() - tokens.created_at`),
  `market_cap` (derive: `current_price × initial_supply_token`), `created_at`
  (redundant with `tokens.created_at`), and all `last_synced_*` (→ `token_sync_state`).
- Created lazily: a token has its `tokens` row at creation and gets its
  `tokens_info` row on first sync → `LEFT JOIN` until then.

---

## Table 3 — `token_sync_state` (per-venue ingest watermarks)

Replaces the `last_synced_curve_*` / `last_synced_amm_*` repeated group. One row
per `(mint, venue)`, so a new venue is a new **row**, not two new columns.

```sql
CREATE TABLE IF NOT EXISTS token_sync_state (
    mint_address   TEXT        NOT NULL REFERENCES tokens(mint_address) ON DELETE CASCADE,
    venue          TEXT        NOT NULL CHECK (venue IN ('curve', 'amm')),
    last_sig       TEXT,                                  -- newest signature seen (the `until` boundary)
    last_slot      BIGINT,                                -- newest slot (resume-by-slot for replay)
    last_synced_at TIMESTAMPTZ,                           -- wall-clock of last successful sync
    PRIMARY KEY (mint_address, venue)
);

-- Resync scheduler: "which tokens are stalest?"
CREATE INDEX IF NOT EXISTS idx_token_sync_state_synced ON token_sync_state(last_synced_at);
```

Notes

- This is **ingest bookkeeping**, deliberately separate from market metrics — it
  changes on the sync cadence, for a different reason than price/volume.
- The display "last synced" for a token is `MAX(last_synced_at)` over its venue
  rows (derive it; don't denormalize back onto `tokens_info`).

---

## Analysis view — full token picture (derived age / market_cap)

```sql
CREATE OR REPLACE VIEW token_overview AS
SELECT
    t.*,
    i.current_price, i.ath_price, i.ath_timestamp, i.volume, i.trade_count,
    i.last_trade_at, i.is_dead, i.is_migrated, i.lifetime_secs,
    i.updated_at AS metrics_updated_at,
    EXTRACT(EPOCH FROM (now() - t.created_at))::bigint    AS age_secs,        -- derived, never stored
    (i.current_price * t.initial_supply_token)            AS market_cap       -- derived
FROM tokens t
LEFT JOIN tokens_info i USING (mint_address);
```

`LEFT JOIN` so freshly-created (pre-sync) tokens still appear with NULL metrics.

---

## Open design questions

- **`market_cap`** — derived in the view (chosen). Promote to a cached, indexed
  column **only if** you need keyset pagination ordered by market cap at scale.
  *SSOT:* the SQL derivation lives once in `storage::token_enrichment::MARKET_CAP_SQL`
  (`current_price × initial_supply_token`), spliced into every projection/sort/filter;
  the live in-RAM path uses `config::constants::market_cap_sol` with the same per-token
  supply (mayhem-aware constant only as fallback), so both formulas agree. A guard test
  pins `ENRICH_SELECT` to the const.
- **`name` / `symbol`** — static (chosen). Move to `tokens_info` only if on-chain
  metadata renames must be tracked over time.
- **`tokens` retention** — unbounded growth (one row per token ever created). On a
  RAM-constrained box, decide whether long-dead tokens get archived/pruned, given
  `trades` is already retention-bounded (old tokens can't be recomputed anyway).
- **`is_dead` / `is_migrated`** — two booleans (chosen; they're orthogonal). Revisit
  as a single `lifecycle` enum only if the stages become mutually exclusive.
