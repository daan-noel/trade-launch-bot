use std::collections::HashSet;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::config::constants::{lamports_to_sol, sol_to_lamports};
use crate::models::trade::{Trade, TradeType};
use crate::storage::repositories::wallet_dict_repo::WalletDictRepo;

/// Mints per round-trip for the startup cache-seed scans. Bounds each `= ANY($1)`
/// array so Postgres keeps using the per-mint indexes instead of falling back to a
/// full-table seq scan on a huge array (mirrors `sweep::corpus::DbSource` chunking).
const SEED_MINT_CHUNK: usize = 1000;

/// Rows per `insert_many` statement. A single Postgres statement is capped at
/// 65535 bind parameters (the wire protocol's int16 count); at 14 binds/row the
/// hard ceiling is 4681 rows, so this stays well under it. sqlx 0.6 silently
/// wraps `len() as i16` past the cap, corrupting the Parse/Bind message into a
/// Postgres parse error — exactly what the token_sync backfill hit on busy mints.
const TRADE_INSERT_CHUNK: usize = 3000;

pub struct TradeRepo {
    pool: PgPool,
}

impl Clone for TradeRepo {
    fn clone(&self) -> Self {
        Self { pool: self.pool.clone() }
    }
}

// ---------------------------------------------------------------------------
// DB row
//
// The NEW `trades` table stores integers (lamports / raw token units) and a raw
// 64-byte signature (BYTEA); the wallet address is resolved in-SQL by joining
// `wallet_dict`. The runtime `Trade` model is unchanged (f64 amounts, base58
// signature string), so every read path reconstructs the model from the integer
// columns via the conversion helpers at the bottom of this file. Columns the new
// table dropped (`id`, `price_per_token`, `received_at`, `ix_type`,
// `real_*_reserves`) are synthesized on read. `ix_labels` was re-added as a real
// JSONB column (migration 0002), written at ingest and read back where projected.
// ---------------------------------------------------------------------------

/// One row read from the new `trades` table LEFT-joined to `wallet_dict`. All
/// amounts are integers (lamports / raw token units); `tx_signature` is the raw
/// 64-byte signature; `wallet_address` is the joined-in base58 string, or a
/// synthetic `unknown:<wallet_id>` sentinel when the interned id has no
/// `wallet_dict` row (a LEFT join + `COALESCE` so a trade is never dropped just
/// because its wallet couldn't be resolved — see the read queries below).
#[derive(sqlx::FromRow)]
struct TradeDbRow {
    mint_address: String,
    wallet_address: String,
    trade_type: String,
    amount_lamports: i64,
    token_amount: i64,
    tx_signature: Vec<u8>,
    // Defaulted so the read paths that don't project `tx_index` (it's not consumed
    // downstream — ordering is resolved in SQL) still map cleanly to 0.
    #[sqlx(default)]
    tx_index: i32,
    leg_index: i16,
    slot: i64,
    block_time: DateTime<Utc>,
    reserve_lamports: Option<i64>,
    reserve_token: Option<i64>,
    venue: String,
    // Defaulted so read queries that don't project `ix_labels` still map cleanly
    // to `None` (only the trade-history reads select it). `None` = column absent
    // from the SELECT *or* a NULL row (pre-0002 trades — no raw_txs to backfill).
    #[sqlx(default)]
    ix_labels: Option<sqlx::types::Json<serde_json::Value>>,
}

impl TryFrom<TradeDbRow> for Trade {
    type Error = anyhow::Error;

    fn try_from(r: TradeDbRow) -> Result<Self, Self::Error> {
        let trade_type = match r.trade_type.as_str() {
            "buy" => TradeType::Buy,
            "sell" => TradeType::Sell,
            other => anyhow::bail!("Unknown trade_type in DB: {other}"),
        };

        // Reconstruct the model amounts from the integer columns. SOL is f64 (human
        // SOL from lamports); token_amount stays an exact integer (no f64 round-trip).
        let amount_sol = lamports_to_sol(r.amount_lamports);
        let token_amount = r.token_amount as u64;

        Ok(Self {
            // Synthesized: the new table has no `id` column.
            id: Uuid::new_v4(),
            mint_address: r.mint_address,
            wallet_address: r.wallet_address,
            trade_type,
            amount_sol,
            token_amount,
            // Derived: the new table has no `price_per_token` column. The ratio is
            // computed in f64 (token cast at the divide).
            price_per_token: price_of(amount_sol, token_amount as f64),
            tx_signature: sig_bytes_to_base58(&r.tx_signature),
            tx_index: r.tx_index as u32,
            leg_index: r.leg_index as u32,
            slot: r.slot as u64,
            block_time: r.block_time,
            // Synthesized: the new table has no `received_at`; reuse block_time.
            received_at: r.block_time,
            reserve_sol: r.reserve_lamports.map(lamports_to_sol),
            reserve_token: r.reserve_token.map(|v| v as u64),
            // The new table dropped the real_* reserve columns.
            real_reserve_sol: None,
            real_token_reserves: None,
            // Synthesized "Buy"/"Sell" instruction label from the trade side.
            instruction_type: ix_type_str(trade_type).to_string(),
            // Real per-tx instruction labels when the read projected `ix_labels`
            // (0002+ trades); `Null` when not selected or an unbackfilled old row.
            instruction_labels: r.ix_labels.map(|j| j.0).unwrap_or(serde_json::Value::Null),
            venue: r.venue,
        })
    }
}

fn trade_type_str(t: TradeType) -> &'static str {
    match t {
        TradeType::Buy => "buy",
        TradeType::Sell => "sell",
    }
}

/// "Buy"/"Sell" instruction-type label synthesized from the trade side (the new
/// table dropped the `ix_type` column).
fn ix_type_str(t: TradeType) -> &'static str {
    match t {
        TradeType::Buy => "Buy",
        TradeType::Sell => "Sell",
    }
}

// ---------------------------------------------------------------------------
// Repo
// ---------------------------------------------------------------------------

impl TradeRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Insert a trade. `ON CONFLICT DO NOTHING` on the natural dedup key
    /// `(block_time, tx_signature, leg_index)` — the table's PRIMARY KEY. This was
    /// `DO UPDATE` under the old schema; per the TimescaleDB plan we switch to
    /// DO NOTHING because compressed chunks are update-hostile and the first write
    /// already carries the correct reserves, so a replay has nothing to refresh.
    ///
    /// The single wallet is interned into `wallet_dict` first so the row references
    /// it by a compact `wallet_id` (INTEGER) instead of the base58 string.
    pub async fn insert(&self, trade: &Trade) -> anyhow::Result<()> {
        let wallet_id = WalletDictRepo::new(self.pool.clone())
            .intern(&trade.wallet_address)
            .await?;

        sqlx::query(
            r#"
            INSERT INTO trades
                (mint_address, wallet_id, trade_type, venue,
                 amount_lamports, token_amount,
                 reserve_lamports, reserve_token,
                 slot, tx_index, leg_index, block_time, tx_signature, ix_labels)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
            ON CONFLICT (block_time, tx_signature, leg_index) DO NOTHING
            "#,
        )
        .bind(&trade.mint_address)
        .bind(wallet_id)
        .bind(trade_type_str(trade.trade_type))
        .bind(&trade.venue)
        .bind(sol_to_lamports(trade.amount_sol))
        .bind(trade.token_amount as i64)
        .bind(trade.reserve_sol.map(sol_to_lamports))
        .bind(trade.reserve_token.map(|v| v as i64))
        .bind(trade.slot as i64)
        .bind(trade.tx_index as i32)
        .bind(trade.leg_index as i16)
        .bind(trade.block_time)
        .bind(sig_base58_to_bytes(&trade.tx_signature)?)
        .bind(sqlx::types::Json(&trade.instruction_labels))
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Bulk version of [`insert`] — one multi-row statement per chunk, same
    /// `ON CONFLICT DO NOTHING` dedup on the `(block_time, tx_signature, leg_index)`
    /// primary key. Callers SHOULD dedup by that key first; DO NOTHING tolerates
    /// duplicates within a flush regardless (the first write wins). Used by the live
    /// ingest DB-writer to collapse a flush into a single round-trip, and by the
    /// token_sync backfill.
    ///
    /// All wallets are interned in one batch first (`intern_many`), then each row
    /// binds its `wallet_id` from the resulting map.
    ///
    /// Chunked at [`TRADE_INSERT_CHUNK`]: a single statement is capped at 65535
    /// bind parameters (the wire protocol's int16 count), and sqlx 0.6 has no
    /// guard — it writes `len() as i16`, so past the ceiling the count silently
    /// wraps and Postgres rejects the malformed Parse/Bind ("DB parse error"). At
    /// 14 binds/row the chunk stays well under the ceiling. Each chunk is safe to
    /// retry (DO NOTHING is idempotent).
    pub async fn insert_many(&self, trades: &[Trade]) -> anyhow::Result<()> {
        if trades.is_empty() {
            return Ok(());
        }

        // Intern every distinct wallet up front, then look each row's id up from
        // the map. One batched round-trip instead of one intern per row.
        let unique: Vec<String> = {
            let mut seen: HashSet<&str> = HashSet::new();
            let mut out: Vec<String> = Vec::new();
            for t in trades {
                if seen.insert(t.wallet_address.as_str()) {
                    out.push(t.wallet_address.clone());
                }
            }
            out
        };
        let wallet_ids = WalletDictRepo::new(self.pool.clone())
            .intern_many(&unique)
            .await?;

        for chunk in trades.chunks(TRADE_INSERT_CHUNK) {
            let mut qb: sqlx::QueryBuilder<sqlx::Postgres> = sqlx::QueryBuilder::new(
                "INSERT INTO trades \
                 (mint_address, wallet_id, trade_type, venue, amount_lamports, token_amount, \
                  reserve_lamports, reserve_token, slot, tx_index, leg_index, \
                  block_time, tx_signature, ix_labels) ",
            );
            // `push_values` cannot bubble a Result, so pre-resolve the fallible
            // signature decode into the loop's error path is not possible here;
            // instead we decode eagerly and bind the bytes (decode errors surface
            // as an empty-vec bind — but the model's signatures come from the chain
            // and are valid, so this is the live path). For robustness the decode
            // falls back to an empty Vec rather than panicking.
            qb.push_values(chunk, |mut b, t| {
                let wallet_id = wallet_ids.get(&t.wallet_address).copied().unwrap_or_default();
                b.push_bind(&t.mint_address)
                    .push_bind(wallet_id)
                    .push_bind(trade_type_str(t.trade_type))
                    .push_bind(&t.venue)
                    .push_bind(sol_to_lamports(t.amount_sol))
                    .push_bind(t.token_amount as i64)
                    .push_bind(t.reserve_sol.map(sol_to_lamports))
                    .push_bind(t.reserve_token.map(|v| v as i64))
                    .push_bind(t.slot as i64)
                    .push_bind(t.tx_index as i32)
                    .push_bind(t.leg_index as i16)
                    .push_bind(t.block_time)
                    .push_bind(sig_base58_to_bytes(&t.tx_signature).unwrap_or_default())
                    .push_bind(sqlx::types::Json(&t.instruction_labels));
            });
            qb.push(" ON CONFLICT (block_time, tx_signature, leg_index) DO NOTHING");
            qb.build().execute(&self.pool).await?;
        }

        Ok(())
    }

    /// Signature of the most recently saved trade for a token on a specific
    /// venue (`"curve"` or `"amm"`), if any. Used as the `until` boundary for
    /// incremental syncs so each venue resumes from its own last saved trade.
    /// Ordered by the execution-order key (slot, tx_index, leg_index) and the raw
    /// signature bytes are decoded back to base58 for the caller.
    pub async fn latest_signature(
        &self,
        mint: &str,
        venue: &str,
    ) -> anyhow::Result<Option<String>> {
        let bytes: Option<Vec<u8>> = sqlx::query_scalar(
            r#"
            SELECT tx_signature
            FROM trades
            WHERE mint_address = $1 AND venue = $2
            ORDER BY slot DESC, tx_index DESC, leg_index DESC
            LIMIT 1
            "#,
        )
        .bind(mint)
        .bind(venue)
        .fetch_optional(&self.pool)
        .await?;

        Ok(bytes.map(|b| sig_bytes_to_base58(&b)))
    }

    /// Resolve the real base58 `tx_signature` for a batch of fill rows, each keyed
    /// by `(mint, slot, side)`. The sweep walks a slim `CorpusTrade` that carries no
    /// signature, so its entry/exit fills only know the slot; the grouped-sweep
    /// drill-in calls this to recover the actual signature for chart/table linking.
    ///
    /// `keys` is `(mint, slot, is_buy)`. The query over-selects by `(mint, slot)`
    /// set membership (Postgres has no ergonomic tuple-array bind) and the exact
    /// `(mint, slot, side)` match is done in Rust — the drill-in set is small
    /// (one group's tokens × at most 2 slots each), so the over-fetch is bounded.
    /// Returns a map keyed by `(mint, slot, is_buy)` → base58 signature. A fill
    /// whose slot has no trade at all is simply absent from the map.
    ///
    /// Side is a **preference, not a filter**: tpsl fills are real buys/sells so the
    /// side always matches, but a generic-engine fill's slot is the trade that
    /// *priced* the fill — which may be the opposite side. When the requested side
    /// isn't present at the slot, the other side's signature is returned so the
    /// chart still marks the candle the fill executed against.
    pub async fn resolve_fill_signatures(
        &self,
        keys: &[(String, u64, bool)],
    ) -> anyhow::Result<std::collections::HashMap<(String, u64, bool), String>> {
        use std::collections::HashMap;
        if keys.is_empty() {
            return Ok(HashMap::new());
        }
        let mints: Vec<String> = keys.iter().map(|(m, _, _)| m.clone()).collect();
        let slots: Vec<i64> = keys.iter().map(|(_, s, _)| *s as i64).collect();

        let rows: Vec<(String, i64, String, Vec<u8>)> = sqlx::query_as(
            r#"
            SELECT t.mint_address, t.slot, t.trade_type, t.tx_signature
            FROM trades t
            WHERE t.mint_address = ANY($1)
              AND t.slot = ANY($2)
            "#,
        )
        .bind(&mints)
        .bind(&slots)
        .fetch_all(&self.pool)
        .await?;

        // Per `(mint, slot)`, keep the first signature seen for each side. A fill maps
        // to one trade; if a slot+side has several rows (multi-leg or several buys in
        // the fill slot) the first wins — any of them links the bar to the right candle.
        let mut by_slot: HashMap<(String, u64), (Option<String>, Option<String>)> = HashMap::new();
        for (mint, slot, trade_type, sig_bytes) in rows {
            let entry = by_slot.entry((mint, slot as u64)).or_default();
            let slot_side = if trade_type == "buy" { &mut entry.0 } else { &mut entry.1 };
            if slot_side.is_none() {
                *slot_side = Some(sig_bytes_to_base58(&sig_bytes));
            }
        }

        let mut out: HashMap<(String, u64, bool), String> = HashMap::new();
        for (mint, slot, want_buy) in keys.iter().cloned() {
            if let Some((buy_sig, sell_sig)) = by_slot.get(&(mint.clone(), slot)) {
                // Prefer the requested side; fall back to the other side's trade.
                let (preferred, fallback) =
                    if want_buy { (buy_sig, sell_sig) } else { (sell_sig, buy_sig) };
                if let Some(sig) = preferred.clone().or_else(|| fallback.clone()) {
                    out.insert((mint, slot, want_buy), sig);
                }
            }
        }
        Ok(out)
    }

    /// All transaction signatures already saved for a token on a venue
    /// (`"curve"` or `"amm"`). The incremental sync uses this to skip
    /// `getTransaction` for trades it already has, so it doesn't re-spend Helius
    /// RPC credits re-downloading them. Returned as a set of base58 strings for
    /// O(1) membership tests.
    ///
    /// `candidates` is the list of signatures the sync is about to fetch; the query
    /// intersects against it (`tx_signature = ANY($3)`) so Postgres returns only the
    /// already-saved sigs among that page. The slot floor now reads the venue's
    /// watermark from `token_sync_state.last_slot` (was `tokens_info.last_synced_*`),
    /// COALESCEd to 0 before the first sync stamps a watermark. An empty `candidates`
    /// short-circuits to an empty set.
    pub async fn saved_signatures(
        &self,
        mint: &str,
        venue: &str,
        candidates: &[String],
    ) -> anyhow::Result<HashSet<String>> {
        if candidates.is_empty() {
            return Ok(HashSet::new());
        }
        // Translate candidate base58 signatures to raw bytes for the BYTEA `= ANY`.
        let candidate_bytes: Vec<Vec<u8>> = candidates
            .iter()
            .map(|s| sig_base58_to_bytes(s))
            .collect::<anyhow::Result<_>>()?;

        let rows: Vec<(Vec<u8>,)> = sqlx::query_as(
            r#"
            SELECT DISTINCT t.tx_signature
            FROM trades t
            WHERE t.mint_address = $1
              AND t.venue = $2
              AND t.tx_signature = ANY($3)
              AND t.slot >= COALESCE(
                  (SELECT s.last_slot
                   FROM token_sync_state s
                   WHERE s.mint_address = $1 AND s.venue = $2),
                  0)
            "#,
        )
        .bind(mint)
        .bind(venue)
        .bind(&candidate_bytes)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|(b,)| sig_bytes_to_base58(&b)).collect())
    }

    /// Count of distinct transaction signatures already saved for a token on a
    /// venue (`"curve"` / `"amm"`). The sync **preview** derives its "Fetch All"
    /// total from this DB count plus the cheap "new" count, instead of re-paging
    /// full history over `getSignaturesForAddress` (an advisory UI figure — an
    /// estimate is fine, it isn't the real sync).
    pub async fn distinct_signature_count(&self, mint: &str, venue: &str) -> anyhow::Result<u64> {
        let n: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(DISTINCT tx_signature)
            FROM trades
            WHERE mint_address = $1 AND venue = $2
            "#,
        )
        .bind(mint)
        .bind(venue)
        .fetch_one(&self.pool)
        .await?;
        Ok(n.max(0) as u64)
    }

    /// Most-recent trade by `wallet` on `mint` of a given side, or `None`.
    /// Filters in SQL and fetches a single row instead of pulling N rows and
    /// scanning them in Rust. The wallet is first translated to its interned id; if
    /// it has no id it has no trades, so we return `None` without touching `trades`.
    ///
    /// No longer on the entry/exit-confirm path (1C replaced "latest buy/sell for
    /// the pair" with per-signature attribution — see [`Self::find_fill_by_signature`]
    /// / [`Self::sum_legs_by_signatures`]); used for ad-hoc lookups and the
    /// ManualSell external-clear detection path.
    pub async fn find_latest_by_wallet_mint_type(
        &self,
        wallet: &str,
        mint: &str,
        trade_type: TradeType,
    ) -> anyhow::Result<Option<Trade>> {
        let Some(wallet_id) = WalletDictRepo::new(self.pool.clone()).id_for(wallet).await? else {
            return Ok(None);
        };
        let row = sqlx::query_as::<_, TradeDbRow>(
            r#"
            SELECT t.mint_address, COALESCE(w.address, 'unknown:' || t.wallet_id::text) AS wallet_address, t.trade_type, t.venue,
                   t.amount_lamports, t.token_amount,
                   t.reserve_lamports, t.reserve_token,
                   t.slot, t.tx_index, t.leg_index, t.block_time, t.tx_signature
            FROM trades t
            LEFT JOIN wallet_dict w ON w.id = t.wallet_id
            WHERE t.wallet_id = $1 AND t.mint_address = $2 AND t.trade_type = $3
            ORDER BY t.slot DESC, t.tx_index DESC, t.leg_index DESC
            LIMIT 1
            "#,
        )
        .bind(wallet_id)
        .bind(mint)
        .bind(trade_type_str(trade_type))
        .fetch_optional(&self.pool)
        .await?;

        row.map(Trade::try_from).transpose()
    }

    /// Average manual-buy cost basis per mint for `wallet` over a bounded mint set.
    /// Rolls up `trade_type='buy'` legs — `SUM(amount_lamports)` / `SUM(token_amount)`
    /// grouped by mint — into an [`AvgEntry`] each. This is the **manual-buy
    /// cost-basis SSOT** (bot buys already carry `strategy_positions.entry_*`).
    ///
    /// `avg_entry_price` is human SOL per raw token unit — the SAME price convention
    /// as [`crate::models::strategy::StrategyPosition::entry_price`] and
    /// [`SigLegs::price_per_token`] (Σsol / Σtokens) — so a manually-bought bag and a
    /// bot bag price identically.
    ///
    /// Bounded by the caller's `mints` slice (the held-mint set is tiny). An unknown
    /// wallet has no trades, so returns an empty map without touching `trades`; mints
    /// the wallet never bought are simply absent.
    pub async fn avg_entry_by_wallet_and_mints(
        &self,
        wallet: &str,
        mints: &[String],
    ) -> anyhow::Result<std::collections::HashMap<String, AvgEntry>> {
        if mints.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        let Some(wallet_id) = WalletDictRepo::new(self.pool.clone()).id_for(wallet).await? else {
            return Ok(std::collections::HashMap::new());
        };
        // Σ over the wallet's buy legs, grouped per mint. Kept integer in SQL
        // (exact lamports / raw units); the SOL conversion happens once in Rust.
        let rows: Vec<(String, i64, i64)> = sqlx::query_as(
            r#"
            SELECT mint_address,
                   COALESCE(SUM(amount_lamports), 0)::bigint,
                   COALESCE(SUM(token_amount), 0)::bigint
            FROM trades
            WHERE wallet_id = $1
              AND trade_type = 'buy'
              AND mint_address = ANY($2)
            GROUP BY mint_address
            "#,
        )
        .bind(wallet_id)
        .bind(mints)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|(mint, total_cost_lamports, total_token_amount)| {
                let avg_entry_price = if total_token_amount > 0 {
                    lamports_to_sol(total_cost_lamports) / total_token_amount as f64
                } else {
                    0.0
                };
                (
                    mint,
                    AvgEntry {
                        avg_entry_price,
                        total_token_amount: total_token_amount as u64,
                        total_cost_lamports,
                    },
                )
            })
            .collect())
    }

    /// Distinct token mints this wallet traded in the `since..now` window,
    /// ordered by the wallet's most-recent trade on each mint (recent first) and
    /// capped at `limit`. Powers the Trader Analysis page's per-wallet token list.
    ///
    /// Counts **both** buys and sells, so a mint the wallet only *exited* in the
    /// window (its buy predates `since`) still appears. An unknown wallet has no
    /// trades, so returns an empty vec without touching `trades`. Bounded by
    /// `limit` + the `block_time >= since` window, which rides the hypertable's
    /// `block_time` partitioning.
    pub async fn wallet_traded_mints(
        &self,
        wallet: &str,
        since: DateTime<Utc>,
        limit: i64,
    ) -> anyhow::Result<Vec<WalletTradedMint>> {
        let Some(wallet_id) = WalletDictRepo::new(self.pool.clone()).id_for(wallet).await? else {
            return Ok(Vec::new());
        };
        let rows: Vec<(String, DateTime<Utc>, i64, i64)> = sqlx::query_as(
            r#"
            SELECT mint_address,
                   MAX(block_time) AS last_trade_at,
                   COUNT(*) FILTER (WHERE trade_type = 'buy')  AS buy_count,
                   COUNT(*) FILTER (WHERE trade_type = 'sell') AS sell_count
            FROM trades
            WHERE wallet_id = $1
              AND block_time >= $2
            GROUP BY mint_address
            ORDER BY last_trade_at DESC
            LIMIT $3
            "#,
        )
        .bind(wallet_id)
        .bind(since)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|(mint_address, last_trade_at, buy_count, sell_count)| WalletTradedMint {
                mint_address,
                last_trade_at,
                buy_count,
                sell_count,
            })
            .collect())
    }

    /// Find all trades for a token in execution order (slot, tx_index, leg_index).
    /// LEFT-joins `wallet_dict` to recover each trade's wallet address (orphaned
    /// wallet ids fall back to the `unknown:<id>` sentinel, never dropping a row).
    pub async fn find_by_mint_all(&self, mint: &str) -> anyhow::Result<Vec<Trade>> {
        let rows = sqlx::query_as::<_, TradeDbRow>(
            r#"
            SELECT t.mint_address, COALESCE(w.address, 'unknown:' || t.wallet_id::text) AS wallet_address, t.trade_type, t.venue,
                   t.amount_lamports, t.token_amount,
                   t.reserve_lamports, t.reserve_token,
                   t.slot, t.tx_index, t.leg_index, t.block_time, t.tx_signature, t.ix_labels
            FROM trades t
            LEFT JOIN wallet_dict w ON w.id = t.wallet_id
            WHERE t.mint_address = $1
            ORDER BY t.slot ASC, t.tx_index ASC, t.leg_index ASC
            "#,
        )
        .bind(mint)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(Trade::try_from).collect()
    }

    /// Find all trades for a *batch* of tokens in one round-trip, grouped per
    /// mint and each group in the same execution order as [`find_by_mint_all`].
    /// The backtest uses this to fetch a chunk of candidate mints with a single
    /// query instead of one query per token: same total rows, but ~`mints.len()`×
    /// fewer round-trips and PgPool connections held.
    ///
    /// Bounded by the caller's chunk size (never the full `trades` table). Mints
    /// with no trades are simply absent from the returned map.
    pub async fn find_by_mints_all(
        &self,
        mints: &[String],
    ) -> anyhow::Result<std::collections::HashMap<String, Vec<Trade>>> {
        // `mint_address` leads the ORDER BY so each mint's rows arrive as one
        // contiguous run already in execution order; grouping is then a single
        // linear pass with no per-mint sort.
        let rows = sqlx::query_as::<_, TradeDbRow>(
            r#"
            SELECT t.mint_address, COALESCE(w.address, 'unknown:' || t.wallet_id::text) AS wallet_address, t.trade_type, t.venue,
                   t.amount_lamports, t.token_amount,
                   t.reserve_lamports, t.reserve_token,
                   t.slot, t.tx_index, t.leg_index, t.block_time, t.tx_signature
            FROM trades t
            LEFT JOIN wallet_dict w ON w.id = t.wallet_id
            WHERE t.mint_address = ANY($1)
            ORDER BY t.mint_address ASC, t.slot ASC, t.tx_index ASC, t.leg_index ASC
            "#,
        )
        .bind(mints)
        .fetch_all(&self.pool)
        .await?;

        let mut grouped: std::collections::HashMap<String, Vec<Trade>> =
            std::collections::HashMap::with_capacity(mints.len());
        for row in rows {
            let mut trade = Trade::try_from(row)?;
            // BACKTEST-ONLY approximation of `real_reserve_sol`. The `trades` table
            // dropped the program-emitted real-reserve column, so `Trade::try_from`
            // leaves it `None`. This method feeds the offline backtest/sim ONLY (the
            // live/paper decision path uses `CachedTrade` from the decoder, which
            // carries the exact emitted value), so it's safe to reconstruct the
            // approximate real SOL here from the priced reserve pair + venue so the
            // sim's real-reserve gates (tpsl2 `min_liq_sol`/organic-liq, dead-token)
            // resolve instead of always seeing 0. Same formula as the lake corpus
            // (`approx_real_sol_reserves`); an approximation, not lamport-identical.
            trade.real_reserve_sol = trade
                .reserve_sol
                .map(|s| crate::config::constants::approx_real_sol_reserves(s, &trade.venue));
            grouped
                .entry(trade.mint_address.clone())
                .or_default()
                .push(trade);
        }
        Ok(grouped)
    }

    /// Find trades for a token in execution order, paged by `limit`/`offset`.
    /// Same ordering as `find_by_mint_all`. `limit <= 0` returns the FULL history
    /// (unbounded) — the inspect charts (Positions / Sim / grouped-sweep) resolve
    /// their entry/exit markers and swing legs against this trade set, so a first-N
    /// cap left the tail of a high-volume token off the chart. A positive `limit`
    /// still bounds the response.
    pub async fn find_by_mint_paged(
        &self,
        mint: &str,
        limit: i64,
        offset: i64,
    ) -> anyhow::Result<Vec<Trade>> {
        // `LIMIT NULL` = all rows; a positive cap passes through unchanged. Binding
        // an `Option<i64>` lets one SQL string serve both the capped and full-history
        // callers without string-building the query.
        let limit_opt: Option<i64> = if limit <= 0 { None } else { Some(limit) };
        let rows = sqlx::query_as::<_, TradeDbRow>(
            r#"
            SELECT t.mint_address, COALESCE(w.address, 'unknown:' || t.wallet_id::text) AS wallet_address, t.trade_type, t.venue,
                   t.amount_lamports, t.token_amount,
                   t.reserve_lamports, t.reserve_token,
                   t.slot, t.tx_index, t.leg_index, t.block_time, t.tx_signature, t.ix_labels
            FROM trades t
            LEFT JOIN wallet_dict w ON w.id = t.wallet_id
            WHERE t.mint_address = $1
            ORDER BY t.slot ASC, t.tx_index ASC, t.leg_index ASC
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(mint)
        .bind(limit_opt)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(Trade::try_from).collect()
    }

    /// Sum the legs of one transaction `signature` for `(wallet, mint, side)`,
    /// rolled up into a [`SigLegs`] (Σtokens, Σsol, first/last leg time). `None`
    /// when the signature has no matching trade indexed yet.
    ///
    /// This is the **per-signature entry attribution** primitive: the snipe buy
    /// already returns its own submitted signature, so the entry fill is recovered
    /// by *that* signature instead of `find_latest_by_wallet_mint_type` (the latest
    /// buy for the pair) — which, with two concurrent positions on the same token,
    /// would adopt the same fill twice.
    pub async fn find_fill_by_signature(
        &self,
        wallet: &str,
        mint: &str,
        signature: &str,
    ) -> anyhow::Result<Option<SigLegs>> {
        self.sum_legs_by_signatures(wallet, mint, std::slice::from_ref(&signature.to_string()), TradeType::Buy)
            .await
    }

    /// Sum the legs of a *set* of this position's own transaction `signatures` for
    /// `(wallet, mint, side)` into a single [`SigLegs`]. Used to confirm an exit by
    /// summing the position's OWN sell signatures' token legs against its
    /// `entry_token_amount` — so concurrent same-token positions never confirm
    /// against each other's sells (unlike the shared net `(wallet, mint)` balance).
    /// `None` when none of the signatures are indexed yet (empty `signatures` or an
    /// unknown wallet short-circuits). The integer SOL/token sums are converted back
    /// to f64 (SOL from lamports, tokens from raw units) for [`SigLegs`].
    pub async fn sum_legs_by_signatures(
        &self,
        wallet: &str,
        mint: &str,
        signatures: &[String],
        trade_type: TradeType,
    ) -> anyhow::Result<Option<SigLegs>> {
        if signatures.is_empty() {
            return Ok(None);
        }
        let Some(wallet_id) = WalletDictRepo::new(self.pool.clone()).id_for(wallet).await? else {
            return Ok(None);
        };
        // Translate the position's base58 signatures to raw bytes for the BYTEA filter.
        let sig_bytes: Vec<Vec<u8>> = signatures
            .iter()
            .map(|s| sig_base58_to_bytes(s))
            .collect::<anyhow::Result<_>>()?;

        let row: (i64, i64, i64, Option<DateTime<Utc>>, Option<DateTime<Utc>>) = sqlx::query_as(
            r#"
            SELECT COUNT(*)::bigint,
                   COALESCE(SUM(token_amount), 0)::bigint,
                   COALESCE(SUM(amount_lamports), 0)::bigint,
                   MIN(block_time),
                   MAX(block_time)
            FROM trades
            WHERE wallet_id = $1
              AND mint_address = $2
              AND trade_type = $3
              AND tx_signature = ANY($4)
            "#,
        )
        .bind(wallet_id)
        .bind(mint)
        .bind(trade_type_str(trade_type))
        .bind(&sig_bytes)
        .fetch_one(&self.pool)
        .await?;

        let (leg_count, token_sum, lamports_sum, first, last) = row;
        if leg_count == 0 {
            return Ok(None);
        }
        Ok(Some(SigLegs {
            // token_amount stays an exact integer (raw units); SOL → human f64.
            token_amount: token_sum as u64,
            amount_sol: lamports_to_sol(lamports_sum),
            first_block_time: first.unwrap_or_else(Utc::now),
            last_block_time: last.unwrap_or_else(Utc::now),
        }))
    }

    /// Net token balance for `(wallet, mint)` (Σbuys − Σsells), as **signed raw
    /// integer units** (`i64` — a partially-cleared bag can be negative mid-state).
    /// No longer on the sell-confirm hot path (replaced by per-signature
    /// attribution); used for external-clear detection (ManualSell path) and ad-hoc
    /// balance lookups. An unknown wallet has no trades, so returns 0 without
    /// touching `trades`.
    pub async fn net_token_amount_by_wallet_and_mint(
        &self,
        wallet: &str,
        mint: &str,
    ) -> anyhow::Result<i64> {
        let Some(wallet_id) = WalletDictRepo::new(self.pool.clone()).id_for(wallet).await? else {
            return Ok(0);
        };
        // Sum the integer token_amount column with a buy/sell sign — exact raw units.
        let balance: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(CASE WHEN trade_type = 'buy' THEN token_amount WHEN trade_type = 'sell' THEN -token_amount ELSE 0 END), 0)::bigint FROM trades WHERE wallet_id = $1 AND mint_address = $2",
        )
        .bind(wallet_id)
        .bind(mint)
        .fetch_one(&self.pool)
        .await?;

        Ok(balance)
    }

    /// Stream the cache-seed trade history for the given mints, grouped per mint,
    /// invoking `f(mint, trades, agg)` once per mint as its run completes. A single
    /// scan does the work the old two-pass seed needed (full aggregate scan +
    /// full chronological stream):
    ///
    /// - **Capped** — only the newest `per_mint_cap` trades per mint land in
    ///   `trades` (a per-mint `ROW_NUMBER` window), so a high-volume token reads its
    ///   recent window instead of its full unbounded history.
    /// - **Single pass** — lifetime `count`/`volume` and the newest trade's
    ///   `block_time`/`price`/`reserves` ride along as window aggregates computed
    ///   over the *full* partition in the same scan (`SeedAgg`), so the caller never
    ///   needs a second aggregate query.
    ///
    /// `trades` arrives oldest-first per mint (ready for `push_trade_capped`). The
    /// seed path doesn't filter by wallet, so it simply JOINs `wallet_dict` for the
    /// address. Scoped to the seeded set (`mint = ANY($1)`, chunked) and grouped
    /// while streaming so peak memory is one mint's capped run.
    pub async fn for_each_seed_mint<F>(
        &self,
        mints: &[String],
        per_mint_cap: i64,
        mut f: F,
    ) -> anyhow::Result<()>
    where
        F: FnMut(String, Vec<Trade>, SeedAgg),
    {
        use futures_util::TryStreamExt;

        /// One seed row: the trade columns plus the per-mint (partition-constant)
        /// aggregates carried by the window functions. `lifetime_volume` is in
        /// lamports (SUM of the integer column) and converted to f64 SOL on read;
        /// `newest_price` is already a float (sol/token ratio computed in SQL);
        /// `newest_reserves` is the raw integer reserve_token as f64.
        #[derive(sqlx::FromRow)]
        struct SeedTradeRow {
            #[sqlx(flatten)]
            trade: TradeDbRow,
            lifetime_count: i64,
            lifetime_volume: i64,
            newest_block_time: DateTime<Utc>,
            newest_price: Option<f64>,
            newest_reserves: Option<f64>,
        }

        if mints.is_empty() {
            return Ok(());
        }
        for chunk in mints.chunks(SEED_MINT_CHUNK) {
            let mut stream = sqlx::query_as::<_, SeedTradeRow>(
                r#"
                WITH ranked AS (
                    SELECT t.mint_address, COALESCE(w.address, 'unknown:' || t.wallet_id::text) AS wallet_address, t.trade_type, t.venue,
                           t.amount_lamports, t.token_amount,
                           t.reserve_lamports, t.reserve_token,
                           t.slot, t.tx_index, t.leg_index, t.block_time, t.tx_signature,
                           ROW_NUMBER()                          OVER w  AS rn,
                           COUNT(*)                              OVER wp AS lifetime_count,
                           COALESCE(SUM(t.amount_lamports) OVER wp, 0)::bigint AS lifetime_volume,
                           FIRST_VALUE(t.block_time)             OVER w  AS newest_block_time,
                           FIRST_VALUE(t.amount_lamports::float8 / NULLIF(t.token_amount, 0)) OVER w AS newest_price,
                           FIRST_VALUE(t.reserve_token::float8) OVER w AS newest_reserves
                    FROM trades t
                    LEFT JOIN wallet_dict w ON w.id = t.wallet_id
                    WHERE t.mint_address = ANY($1)
                    WINDOW
                        w  AS (PARTITION BY t.mint_address
                               ORDER BY t.slot DESC, t.tx_index DESC, t.leg_index DESC),
                        wp AS (PARTITION BY t.mint_address)
                )
                SELECT mint_address, wallet_address, trade_type, venue,
                       amount_lamports, token_amount,
                       reserve_lamports, reserve_token,
                       slot, tx_index, leg_index, block_time, tx_signature,
                       lifetime_count, lifetime_volume, newest_block_time, newest_price, newest_reserves
                FROM ranked
                WHERE rn <= $2
                ORDER BY mint_address ASC, slot ASC, tx_index ASC, leg_index ASC
                "#,
            )
            .bind(chunk)
            .bind(per_mint_cap)
            .fetch(&self.pool);

            // Rows are mint-contiguous (ORDER BY mint_address) and each mint lives
            // entirely within this chunk, so group on the mint boundary and flush.
            let mut cur_mint: Option<String> = None;
            let mut buf: Vec<Trade> = Vec::new();
            let mut agg: Option<SeedAgg> = None;
            while let Some(row) = stream.try_next().await? {
                if cur_mint.as_deref() != Some(row.trade.mint_address.as_str()) {
                    if let (Some(m), Some(a)) = (cur_mint.take(), agg.take()) {
                        f(m, std::mem::take(&mut buf), a);
                    }
                    cur_mint = Some(row.trade.mint_address.clone());
                    agg = Some(SeedAgg {
                        lifetime_count: row.lifetime_count.max(0) as u64,
                        // lifetime_volume is a lamports SUM → convert to f64 SOL.
                        lifetime_volume: lamports_to_sol(row.lifetime_volume),
                        last_trade_at: row.newest_block_time,
                        current_reserves: row.newest_reserves,
                        newest_price: row.newest_price,
                    });
                }
                buf.push(Trade::try_from(row.trade)?);
            }
            if let (Some(m), Some(a)) = (cur_mint.take(), agg.take()) {
                f(m, std::mem::take(&mut buf), a);
            }
        }
        Ok(())
    }
}

/// Look up the on-chain `tx_signature` for a real trade matching a paper fill.
/// Paper execution finds fills in the in-memory cache (which strips `tx_signature`
/// for Phase B), then calls this to recover the real signature so paper positions
/// store the same tx as sim positions and the frontend highlight works identically.
/// Returns `None` for time-driven exits where no real trade occurred.
///
/// Price is derived now (the table has no `price_per_token` column), so the match
/// converts the stored lamports to SOL (`amount_lamports::float8 / 1e9`) before dividing
/// by `token_amount`, matching the `price_of()`/`lamports_to_sol()` convention every
/// caller uses to compute the requested `price_per_token` (SOL, not lamports, per
/// token). Best-effort lookup; the raw signature bytes are decoded back to base58.
pub async fn find_tx_by_fill(
    pool: &PgPool,
    mint: &str,
    block_time: DateTime<Utc>,
    price_per_token: f64,
) -> sqlx::Result<Option<String>> {
    let bytes: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT tx_signature FROM trades \
         WHERE mint_address = $1 AND block_time = $2 \
           AND (amount_lamports::float8 / 1e9 / NULLIF(token_amount, 0)) = $3 \
         LIMIT 1",
    )
    .bind(mint)
    .bind(block_time)
    .bind(price_per_token)
    .fetch_optional(pool)
    .await?;

    Ok(bytes.map(|b| sig_bytes_to_base58(&b)))
}

/// Rolled-up result of one or more trade legs sharing a `(wallet, mint, side)`,
/// summed by transaction signature ([`TradeRepo::find_fill_by_signature`] /
/// [`TradeRepo::sum_legs_by_signatures`]). For an entry the summary is the adopted
/// buy fill (single-leg today); for an exit it's the running total of the
/// position's own sell legs, compared against `entry_token_amount` to confirm the
/// clear.
#[derive(Debug, Clone)]
pub struct SigLegs {
    /// Σ token_amount across the legs — exact raw integer units.
    pub token_amount: u64,
    /// Σ amount_sol across the legs.
    pub amount_sol: f64,
    /// Earliest leg's block time (the fill's entry time).
    pub first_block_time: DateTime<Utc>,
    /// Latest leg's block time (the fill's exit time).
    pub last_block_time: DateTime<Utc>,
}

/// Rolled-up manual-buy cost basis for one `(wallet, mint)` — the Σ of the
/// wallet's `trade_type='buy'` legs on the mint ([`TradeRepo::avg_entry_by_wallet_and_mints`]).
/// The cost-basis SSOT for manually-bought bags (bot bags carry
/// `strategy_positions.entry_*`).
#[derive(Debug, Clone)]
pub struct AvgEntry {
    /// Weighted-average entry price — human SOL per raw token unit (Σsol / Σtokens),
    /// 0 when no tokens. Same convention as `StrategyPosition::entry_price`.
    pub avg_entry_price: f64,
    /// Σ token_amount across the wallet's buy legs — exact raw integer units.
    pub total_token_amount: u64,
    /// Σ amount_lamports across the wallet's buy legs — exact integer lamports.
    pub total_cost_lamports: i64,
}

/// One token a wallet traded in the window, with the wallet's interaction stats
/// on that mint — the recent-first ordering key + the wallet-specific columns for
/// the Trader Analysis token table ([`TradeRepo::wallet_traded_mints`]).
///
/// `buy_count`/`sell_count` are scoped to the same `block_time >= since` window,
/// so a mint the wallet only *exited* in the window can show `buy_count = 0` (its
/// buys predate the window) — matches the "both buys and sells count" semantics.
#[derive(Debug, Clone, serde::Serialize)]
pub struct WalletTradedMint {
    pub mint_address: String,
    pub last_trade_at: DateTime<Utc>,
    pub buy_count: i64,
    pub sell_count: i64,
}

impl SigLegs {
    /// Weighted-average execution price (Σsol / Σtokens), or 0 when no tokens.
    pub fn price_per_token(&self) -> f64 {
        if self.token_amount > 0 {
            self.amount_sol / self.token_amount as f64
        } else {
            0.0
        }
    }
}

/// Per-mint, lifetime-scoped aggregates the cache seed needs alongside a mint's
/// (capped) recent trade run — computed in the same single scan as the trades
/// (see [`TradeRepo::for_each_seed_mint`]). `lifetime_count`/`lifetime_volume`
/// cover the *full* history; the `newest_*` fields are the most recent trade's.
pub struct SeedAgg {
    pub lifetime_count: u64,
    pub lifetime_volume: f64,
    pub last_trade_at: DateTime<Utc>,
    pub current_reserves: Option<f64>,
    pub newest_price: Option<f64>,
}

// ---------------------------------------------------------------------------
// Conversion helpers — the I/O boundary between the runtime `Trade` model and the
// integer/BYTEA `trades` schema.
//
// SOL: the model carries `amount_sol` / `reserve_sol` as human SOL (f64),
// so the SOL side round-trips through `sol_to_lamports`/`lamports_to_sol` (exact
// lamport precision in the BIGINT column). Token amounts and token reserves are
// now exact integers (`u64`) in the model too, so they bind/read as `i64` directly
// — no float helper, no precision loss above 2^53.
// ---------------------------------------------------------------------------

/// Derived execution price (`sol / token`), or 0 when no tokens.
fn price_of(sol: f64, token: f64) -> f64 {
    if token > 0.0 {
        sol / token
    } else {
        0.0
    }
}

/// Base58 signature string → raw 64-byte signature. Parse errors map into anyhow.
fn sig_base58_to_bytes(s: &str) -> anyhow::Result<Vec<u8>> {
    let sig = solana_sdk::signature::Signature::from_str(s)
        .map_err(|e| anyhow::anyhow!("invalid base58 signature {s:?}: {e}"))?;
    Ok(sig.as_ref().to_vec())
}

/// Raw signature bytes → base58 string. Best-effort: a malformed length yields an
/// empty string rather than erroring (read paths shouldn't fail on a stray row).
/// `pub` so the `lab` lake export can convert the `trades.tx_signature` BYTEA
/// column to the base58 string the lake carries (Stage 1 of the simulate→lake
/// migration) with the exact same encoding this repo uses everywhere else.
pub fn sig_bytes_to_base58(bytes: &[u8]) -> String {
    solana_sdk::signature::Signature::try_from(bytes)
        .map(|s| s.to_string())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Per-signature attribution primitives (`find_fill_by_signature` /
// `sum_legs_by_signatures`) — the heart of decision #2 (two positions on the
// same token must each confirm against their OWN fills, never the shared
// `(wallet, mint)` balance). DB-integration, so `#[ignore]`d like the other
// DB tests; run against a local Postgres:
//   $env:DATABASE_URL = "postgres://postgres:1220@localhost:5432/hunter_bot"
//   cargo test -p trading_core trade_repo:: -- --ignored --nocapture
// Each test uses unique mint/wallet ids and deletes the rows it created.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    /// `insert_many` binds 13 params/row (mint_address, wallet_id, trade_type,
    /// venue, amount_lamports, token_amount, reserve_lamports, reserve_token,
    /// slot, tx_index, leg_index, block_time, tx_signature); one Postgres statement
    /// is capped at 65535 (sqlx 0.6 wraps `len() as i16` past it → a Postgres parse
    /// error). Pin the chunk so adding a bound column re-checks the ceiling here
    /// instead of surfacing as a runtime parse error on the backfill path.
    #[test]
    fn trade_insert_chunk_stays_under_param_ceiling() {
        const BINDS_PER_ROW: usize = 13;
        assert!(
            TRADE_INSERT_CHUNK * BINDS_PER_ROW <= 65_535,
            "TRADE_INSERT_CHUNK ({TRADE_INSERT_CHUNK}) × {BINDS_PER_ROW} binds exceeds the 65535 ceiling"
        );
    }

    /// `reserve_sol` is human SOL in the model but `reserve_lamports` in the BIGINT column —
    /// the SOL↔lamports round-trip must preserve fractional SOL (the old
    /// `f64_opt_to_raw` path rounded 30.5 SOL to the integer 31).
    #[test]
    fn reserve_sol_round_trips_through_lamports() {
        let sol = 30.123_456_789_f64;
        let stored = sol_to_lamports(sol);
        assert_eq!(stored, 30_123_456_789, "SOL → lamports keeps 9-decimal precision");
        let back = lamports_to_sol(stored);
        assert!((back - sol).abs() < 1e-9, "lamports → SOL recovers the value");
    }

    async fn test_pool() -> Option<PgPool> {
        let url = std::env::var("DATABASE_URL").ok()?;
        PgPoolOptions::new().max_connections(2).connect(&url).await.ok()
    }

    fn unique(prefix: &str) -> String {
        format!("{prefix}{}", Uuid::new_v4().simple())
    }

    /// Insert one trade leg under `sig`/`leg_index` for `(wallet, mint, side)`.
    /// `(block_time, tx_signature, leg_index)` is the conflict target, so distinct
    /// legs of one tx differ only by `leg_index`. The model is unchanged, so this
    /// builds a `Trade` exactly as before; the repo handles the schema conversion.
    async fn insert_leg(
        repo: &TradeRepo,
        wallet: &str,
        mint: &str,
        side: TradeType,
        sig: &str,
        leg_index: u32,
        sol: f64,
        tokens: u64,
    ) {
        let mut trade = Trade::new(
            mint.to_string(),
            wallet.to_string(),
            side,
            sol,
            tokens,
            sig.to_string(),
            100,
            Utc::now(),
        );
        trade.leg_index = leg_index;
        repo.insert(&trade).await.expect("insert leg");
    }

    async fn cleanup(pool: &PgPool, mint: &str) {
        let _ = sqlx::query("DELETE FROM trades WHERE mint_address = $1")
            .bind(mint)
            .execute(pool)
            .await;
    }

    #[tokio::test]
    #[ignore = "requires a local Postgres (DATABASE_URL); run with --ignored"]
    async fn find_fill_by_signature_sums_multi_leg() {
        let Some(pool) = test_pool().await else { return };
        let repo = TradeRepo::new(pool.clone());
        let (wallet, mint, sig) = (unique("W"), unique("M"), unique("buysig-"));

        // One buy that landed as two legs (e.g. a split route) under one signature.
        insert_leg(&repo, &wallet, &mint, TradeType::Buy, &sig, 0, 0.6, 600).await;
        insert_leg(&repo, &wallet, &mint, TradeType::Buy, &sig, 1, 0.4, 400).await;
        // A foreign buy on the SAME (wallet, mint) under a different signature —
        // a concurrent same-token position's fill (decision #2). Must NOT leak in.
        insert_leg(&repo, &wallet, &mint, TradeType::Buy, &unique("foreign-"), 0, 9.9, 9999).await;

        let legs = repo
            .find_fill_by_signature(&wallet, &mint, &sig)
            .await
            .expect("query")
            .expect("the signature's legs are summed, not None");
        assert_eq!(legs.token_amount, 1000, "Σtokens across both legs");
        assert!((legs.amount_sol - 1.0).abs() < 1e-6, "Σsol across both legs");
        // Weighted-average price = Σsol / Σtokens, not a per-leg price.
        assert!((legs.price_per_token() - 0.001).abs() < 1e-9, "weighted-avg fill price");

        cleanup(&pool, &mint).await;
    }

    #[tokio::test]
    #[ignore = "requires a local Postgres (DATABASE_URL); run with --ignored"]
    async fn sum_legs_by_signatures_isolates_own_sells_and_short_circuits_empty() {
        let Some(pool) = test_pool().await else { return };
        let repo = TradeRepo::new(pool.clone());
        let (wallet, mint) = (unique("W"), unique("M"));
        let (mine_a, mine_b, theirs) = (unique("sellA-"), unique("sellB-"), unique("sellX-"));

        // This position's exit landed across two sell signatures…
        insert_leg(&repo, &wallet, &mint, TradeType::Sell, &mine_a, 0, 0.3, 300).await;
        insert_leg(&repo, &wallet, &mint, TradeType::Sell, &mine_b, 0, 0.2, 200).await;
        // …while a concurrent same-token position sold under its own signature.
        insert_leg(&repo, &wallet, &mint, TradeType::Sell, &theirs, 0, 5.0, 5000).await;

        // Empty signature set short-circuits to None (never a full-table scan).
        assert!(
            repo.sum_legs_by_signatures(&wallet, &mint, &[], TradeType::Sell)
                .await
                .expect("query")
                .is_none(),
            "empty signatures ⇒ None"
        );

        let legs = repo
            .sum_legs_by_signatures(
                &wallet,
                &mint,
                &[mine_a.clone(), mine_b.clone()],
                TradeType::Sell,
            )
            .await
            .expect("query")
            .expect("own sell legs summed");
        assert_eq!(legs.token_amount, 500, "only THIS position's sells summed");
        assert!((legs.amount_sol - 0.5).abs() < 1e-6, "concurrent position's sell excluded");

        // Side filter holds: no Buy legs exist, so a Buy query over the sell sigs is None.
        assert!(
            repo.sum_legs_by_signatures(&wallet, &mint, &[mine_a, mine_b], TradeType::Buy)
                .await
                .expect("query")
                .is_none(),
            "trade_type filter excludes the sell legs"
        );

        cleanup(&pool, &mint).await;
    }

    /// A trade whose `wallet_id` has NO `wallet_dict` row (e.g. a desynced lab
    /// mirror) must STILL be returned by the read paths — the address join is a
    /// LEFT join with a `unknown:<id>` fallback, not an INNER join that silently
    /// drops the row. Regression for the "ingest missed transactions" report that
    /// was actually the wallet_dict INNER join hiding ~58% of the lab mirror's trades.
    #[tokio::test]
    #[ignore = "requires a local Postgres (DATABASE_URL); run with --ignored"]
    async fn orphaned_wallet_id_trade_is_still_returned() {
        let Some(pool) = test_pool().await else { return };
        let repo = TradeRepo::new(pool.clone());
        let mint = unique("M");

        // A wallet_id guaranteed absent from wallet_dict (max + 100k), inserted
        // straight into `trades` to simulate an orphaned/desynced interned id.
        let orphan_id: i32 =
            sqlx::query_scalar("SELECT COALESCE(MAX(id), 0) + 100000 FROM wallet_dict")
                .fetch_one(&pool)
                .await
                .expect("max id");
        sqlx::query(
            "INSERT INTO trades \
             (mint_address, wallet_id, trade_type, venue, amount_lamports, token_amount, \
              reserve_lamports, reserve_token, slot, tx_index, leg_index, block_time, tx_signature) \
             VALUES ($1, $2, 'buy', 'curve', 1000000000, 1000, NULL, NULL, 1, 0, 0, now(), $3) \
             ON CONFLICT DO NOTHING",
        )
        .bind(&mint)
        .bind(orphan_id)
        .bind(vec![7u8; 64])
        .execute(&pool)
        .await
        .expect("insert orphan trade");

        let trades = repo.find_by_mint_all(&mint).await.expect("query");
        assert_eq!(trades.len(), 1, "orphaned-wallet trade must NOT be dropped by the join");
        assert_eq!(
            trades[0].wallet_address,
            format!("unknown:{orphan_id}"),
            "unresolved wallet_id falls back to the sentinel, not an empty/dropped row"
        );

        cleanup(&pool, &mint).await;
    }

    /// `find_by_mint_paged(limit <= 0)` returns the token's FULL history (no cap),
    /// while a positive `limit` still bounds the page. Regression for the inspect
    /// charts' entry/exit markers + swing legs mis-snapping when a first-N cap left
    /// the tail of a high-volume token off the chart.
    #[tokio::test]
    #[ignore = "requires a local Postgres (DATABASE_URL); run with --ignored"]
    async fn find_by_mint_paged_zero_limit_is_unbounded() {
        let Some(pool) = test_pool().await else { return };
        let repo = TradeRepo::new(pool.clone());
        let (wallet, mint) = (unique("W"), unique("M"));

        // More rows than the old 5000 first-N cap would have returned in one page —
        // keep it modest here (distinct legs of one tx differ only by leg_index).
        const ROWS: u32 = 12;
        let sig = unique("bulk-");
        for i in 0..ROWS {
            insert_leg(&repo, &wallet, &mint, TradeType::Buy, &sig, i, 0.1, 100).await;
        }

        // `0` (and any non-positive limit) ⇒ every row, no LIMIT clause.
        let all = repo.find_by_mint_paged(&mint, 0, 0).await.expect("query");
        assert_eq!(all.len() as u32, ROWS, "limit <= 0 returns the full history");
        let neg = repo.find_by_mint_paged(&mint, -1, 0).await.expect("query");
        assert_eq!(neg.len() as u32, ROWS, "a negative limit is also unbounded");

        // A positive limit still caps the page (paging contract preserved).
        let capped = repo.find_by_mint_paged(&mint, 5, 0).await.expect("query");
        assert_eq!(capped.len(), 5, "positive limit bounds the response");

        cleanup(&pool, &mint).await;
    }
}
