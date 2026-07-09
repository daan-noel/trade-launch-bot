//! Domain C feed repos: wallet interning, raw_txs, trades.
//!
//! Batch inserts use a single `UNNEST(...)` multi-row statement (one round-trip,
//! one prepared statement) — the hot-path pattern. Dedup is the table's own PK
//! via `ON CONFLICT DO NOTHING`, so a replayed feed is idempotent.

use sqlx::PgPool;

use crate::models::{NewTrade, RawTx, TradePriced};

/// `wallet_dict` — interning map (4-byte `wallet_id` ↔ 44-byte address).
pub struct WalletDictRepo;

impl WalletDictRepo {
    /// Intern an address → its stable `wallet_id`, inserting on first sight. The
    /// `DO UPDATE` is a no-op touch so a pre-existing row still `RETURNING`s.
    pub async fn intern(pool: &PgPool, address: &str) -> anyhow::Result<i32> {
        let id: i32 = sqlx::query_scalar(
            "INSERT INTO wallet_dict (address) VALUES ($1) \
             ON CONFLICT (address) DO UPDATE SET address = EXCLUDED.address \
             RETURNING id",
        )
        .bind(address)
        .fetch_one(pool)
        .await?;
        Ok(id)
    }
}

/// `raw_txs` — source-of-truth unparsed feed.
pub struct RawTxRepo;

impl RawTxRepo {
    /// Idempotent batch insert (PK `(block_time, tx_signature)`).
    pub async fn insert_batch(pool: &PgPool, rows: &[RawTx]) -> anyhow::Result<u64> {
        if rows.is_empty() {
            return Ok(0);
        }
        let tx_signature: Vec<Vec<u8>> = rows.iter().map(|r| r.tx_signature.clone()).collect();
        let slot: Vec<i64> = rows.iter().map(|r| r.slot).collect();
        let block_time: Vec<_> = rows.iter().map(|r| r.block_time).collect();
        let tx_index: Vec<i32> = rows.iter().map(|r| r.tx_index).collect();
        let payload: Vec<Vec<u8>> = rows.iter().map(|r| r.payload.clone()).collect();
        let source: Vec<i16> = rows.iter().map(|r| r.source).collect();

        let res = sqlx::query(
            "INSERT INTO raw_txs (tx_signature, slot, block_time, tx_index, payload, source) \
             SELECT * FROM UNNEST($1::bytea[], $2::int8[], $3::timestamptz[], $4::int4[], \
                                  $5::bytea[], $6::int2[]) \
             ON CONFLICT (block_time, tx_signature) DO NOTHING",
        )
        .bind(&tx_signature)
        .bind(&slot)
        .bind(&block_time)
        .bind(&tx_index)
        .bind(&payload)
        .bind(&source)
        .execute(pool)
        .await?;
        Ok(res.rows_affected())
    }
}

/// `trades` — typed projection (high-volume hypertable).
pub struct TradeRepo;

impl TradeRepo {
    /// Idempotent batch insert (dedup key = PK `(block_time, tx_signature, leg_index)`).
    pub async fn insert_batch(pool: &PgPool, rows: &[NewTrade]) -> anyhow::Result<u64> {
        if rows.is_empty() {
            return Ok(0);
        }
        let mint_address: Vec<String> = rows.iter().map(|r| r.mint_address.clone()).collect();
        let wallet_id: Vec<i32> = rows.iter().map(|r| r.wallet_id).collect();
        let launchpad_id: Vec<i16> = rows.iter().map(|r| r.launchpad_id).collect();
        let market_kind: Vec<String> = rows.iter().map(|r| r.market_kind.clone()).collect();
        let quote_asset_id: Vec<i16> = rows.iter().map(|r| r.quote_asset_id).collect();
        let trade_type: Vec<String> = rows.iter().map(|r| r.trade_type.clone()).collect();
        let amount_quote: Vec<i64> = rows.iter().map(|r| r.amount_quote).collect();
        let amount_base: Vec<i64> = rows.iter().map(|r| r.amount_base).collect();
        let reserve_quote: Vec<Option<i64>> = rows.iter().map(|r| r.reserve_quote).collect();
        let reserve_base: Vec<Option<i64>> = rows.iter().map(|r| r.reserve_base).collect();
        let slot: Vec<i64> = rows.iter().map(|r| r.slot).collect();
        let tx_index: Vec<i32> = rows.iter().map(|r| r.tx_index).collect();
        let leg_index: Vec<i16> = rows.iter().map(|r| r.leg_index).collect();
        let block_time: Vec<_> = rows.iter().map(|r| r.block_time).collect();
        let tx_signature: Vec<Vec<u8>> = rows.iter().map(|r| r.tx_signature.clone()).collect();

        let res = sqlx::query(
            "INSERT INTO trades \
                (mint_address, wallet_id, launchpad_id, market_kind, quote_asset_id, trade_type, \
                 amount_quote, amount_base, reserve_quote, reserve_base, \
                 slot, tx_index, leg_index, block_time, tx_signature) \
             SELECT * FROM UNNEST($1::text[], $2::int4[], $3::int2[], $4::text[], $5::int2[], $6::text[], \
                                  $7::int8[], $8::int8[], $9::int8[], $10::int8[], \
                                  $11::int8[], $12::int4[], $13::int2[], $14::timestamptz[], $15::bytea[]) \
             ON CONFLICT (block_time, tx_signature, leg_index) DO NOTHING",
        )
        .bind(&mint_address)
        .bind(&wallet_id)
        .bind(&launchpad_id)
        .bind(&market_kind)
        .bind(&quote_asset_id)
        .bind(&trade_type)
        .bind(&amount_quote)
        .bind(&amount_base)
        .bind(&reserve_quote)
        .bind(&reserve_base)
        .bind(&slot)
        .bind(&tx_index)
        .bind(&leg_index)
        .bind(&block_time)
        .bind(&tx_signature)
        .execute(pool)
        .await?;
        Ok(res.rows_affected())
    }

    /// Which of `signatures` already appear in `trades` for `mint_address` —
    /// the feed-based landing check (never a fresh RPC poll). Bounded to a
    /// handful of signatures (one bundle's legs), mint-scoped.
    pub async fn find_signatures_present(
        pool: &PgPool,
        mint_address: &str,
        signatures: &[Vec<u8>],
    ) -> anyhow::Result<std::collections::HashSet<Vec<u8>>> {
        if signatures.is_empty() {
            return Ok(std::collections::HashSet::new());
        }
        let rows: Vec<(Vec<u8>,)> = sqlx::query_as(
            "SELECT DISTINCT tx_signature FROM trades \
             WHERE mint_address = $1 AND tx_signature = ANY($2::bytea[])",
        )
        .bind(mint_address)
        .bind(signatures)
        .fetch_all(pool)
        .await?;
        Ok(rows.into_iter().map(|(sig,)| sig).collect())
    }

    /// Sum a wallet's fills of one side (`buy`/`sell`) for a mint, in quote base
    /// units — the feed-accurate realized-proceeds / actual-cost figure for a
    /// managed wallet's position. Joins `wallet_dict` by address (the interned
    /// `wallet_id` is keyed there, never stored on the managed wallet). Mint- and
    /// wallet-scoped — never a table scan. Returns 0 when nothing's ingested yet.
    pub async fn sum_side_quote_by_address(
        pool: &PgPool,
        mint_address: &str,
        address: &str,
        trade_type: &str,
    ) -> anyhow::Result<i64> {
        let (sum,): (i64,) = sqlx::query_as(
            "SELECT COALESCE(SUM(t.amount_quote), 0) FROM trades t \
             JOIN wallet_dict w ON w.id = t.wallet_id \
             WHERE t.mint_address = $1 AND w.address = $2 AND t.trade_type = $3",
        )
        .bind(mint_address)
        .bind(address)
        .bind(trade_type)
        .fetch_one(pool)
        .await?;
        Ok(sum)
    }

    /// Priced read for a mint (newest-first). `limit <= 0` ⇒ no LIMIT (the full
    /// mint-scoped history the inspect charts need). Still mint-scoped, never the
    /// whole table.
    pub async fn find_priced_by_mint(
        pool: &PgPool,
        mint_address: &str,
        limit: i64,
    ) -> anyhow::Result<Vec<TradePriced>> {
        let base = "SELECT * FROM trades_priced WHERE mint_address = $1 \
                    ORDER BY slot DESC, tx_index DESC, leg_index DESC";
        let rows = if limit > 0 {
            sqlx::query_as::<_, TradePriced>(&format!("{base} LIMIT $2"))
                .bind(mint_address)
                .bind(limit)
                .fetch_all(pool)
                .await?
        } else {
            sqlx::query_as::<_, TradePriced>(base)
                .bind(mint_address)
                .fetch_all(pool)
                .await?
        };
        Ok(rows)
    }

    /// Newest-first priced page for a mint PLUS the full mint-scoped trade count,
    /// in ONE query via `count(*) OVER()` (the count is over all matching rows,
    /// before `LIMIT`). Serves the launch-status poll, which needs both — folds
    /// what were a separate count query + page fetch into one round trip. `limit`
    /// is clamped to `>= 1` (the status view is always paged).
    pub async fn find_priced_page_with_count(
        pool: &PgPool,
        mint_address: &str,
        limit: i64,
    ) -> anyhow::Result<(Vec<TradePriced>, i64)> {
        use sqlx::{FromRow, Row};
        let rows = sqlx::query(
            "SELECT *, count(*) OVER() AS total_count FROM trades_priced \
             WHERE mint_address = $1 \
             ORDER BY slot DESC, tx_index DESC, leg_index DESC LIMIT $2",
        )
        .bind(mint_address)
        .bind(limit.max(1))
        .fetch_all(pool)
        .await?;
        let total = match rows.first() {
            Some(r) => r.try_get::<i64, _>("total_count")?,
            None => 0,
        };
        let trades = rows
            .iter()
            .map(TradePriced::from_row)
            .collect::<Result<Vec<_>, _>>()?;
        Ok((trades, total))
    }
}
