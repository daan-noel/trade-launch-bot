use std::collections::{HashMap, HashSet};
use std::str::FromStr;

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::config::constants::{lamports_to_sol, sol_to_lamports};
use crate::models::trade::{Trade, TradeType};
use crate::models::MarkQuote;
use crate::storage::repositories::wallet_dict_repo::WalletDictRepo;
use crate::strategies::wallet_ledger::WalletTx;

/// Mints per round-trip for the startup cache-seed scans. Bounds each `= ANY($1)`
/// array so Postgres keeps using the per-mint indexes instead of falling back to a
/// full-table seq scan on a huge array (mirrors `sweep::corpus::DbSource` chunking).
const SEED_MINT_CHUNK: usize = 1000;

/// Bind parameters `insert_many` pushes per row — one per column in its INSERT
/// list. The ONE place that number is written down: the chunk size below and the
/// ceiling guard in `tests` both read it, so adding a bound column cannot leave a
/// stale copy behind (it did once — the doc said 18 while the guard still asserted
/// 15, and neither was the truth).
const TRADE_INSERT_BINDS_PER_ROW: usize = 21;

/// Rows per `insert_many` statement. A single Postgres statement is capped at
/// 65535 bind parameters (the wire protocol's int16 count), and sqlx 0.6 silently
/// wraps `len() as i16` past the cap, corrupting the Parse/Bind message into a
/// Postgres parse error — exactly what the token_sync backfill hit on busy mints.
///
/// DERIVED, not chosen: the budget below divided by the binds a row costs. That
/// makes the chunk shrink on its own when a column is added, instead of relying on
/// whoever adds it to notice — the failure mode being a wrapped param count and a
/// "DB parse error" on the backfill path, far from the edit that caused it.
const TRADE_INSERT_CHUNK: usize = TRADE_INSERT_PARAM_BUDGET / TRADE_INSERT_BINDS_PER_ROW;

/// Bind parameters one statement is allowed to spend. The wire cap is 65,535;
/// this stops well short so several more columns fit without a re-think.
const TRADE_INSERT_PARAM_BUDGET: usize = 50_000;

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

/// The one per-mint trade-history projection: every column a `Trade` carries,
/// labels and fee trio included, with the payer and the proxy flag resolved, read
/// `FROM` [`TRADE_HISTORY_FROM`]. The history reads (`find_by_mint_all`,
/// `find_by_mint_until`, `find_by_mint_before`, `find_by_mint_paged`) append their
/// own `WHERE` and [`TRADE_HISTORY_ORDER`]; the cache seed (`for_each_seed_mint`)
/// ranks the same rows, so a seeded cache row is built from what a live one is.
///
/// A row is proxied when the decoder said so, OR when the wallet is a dictionary
/// entry no keypair can sign for. The second half is what reaches HISTORY: rows
/// written before 0014 carry a NULL `is_proxied`, and the flag on `wallet_dict` is
/// the only thing that can still classify them.
const TRADE_HISTORY_COLUMNS: &str = "t.mint_address, \
    COALESCE(w.address, 'unknown:' || t.wallet_id::text) AS wallet_address, t.trade_type, t.venue, \
    t.amount_lamports, t.token_amount, t.reserve_lamports, t.reserve_token, \
    t.slot, t.tx_index, t.leg_index, t.block_time, t.tx_signature, t.ix_labels, \
    t.fee_lamports, t.cu_limit, t.cu_price, t.tip_lamports, \
    p.address AS payer_address, \
    COALESCE(t.is_proxied, w.is_proxy) AS is_proxied";

/// The tables [`TRADE_HISTORY_COLUMNS`] reads.
const TRADE_HISTORY_FROM: &str = "FROM trades t \
    LEFT JOIN wallet_dict w ON w.id = t.wallet_id \
    LEFT JOIN wallet_dict p ON p.id = t.payer_id";

/// Execution order: the chain's own (slot, transaction, leg).
const TRADE_HISTORY_ORDER: &str = "ORDER BY t.slot ASC, t.tx_index ASC, t.leg_index ASC";

/// A history row as a replay folds it: `real_reserve_sol` rebuilt from the priced
/// reserve pair ([`approx_real_sol_reserves`]), because the column was dropped from
/// `trades`. The lake corpus applies the same formula.
///
/// [`approx_real_sol_reserves`]: crate::config::constants::approx_real_sol_reserves
fn replayable_trade(row: TradeDbRow) -> anyhow::Result<Trade> {
    let mut trade = Trade::try_from(row)?;
    trade.real_reserve_sol = trade
        .reserve_sol
        .map(|s| crate::config::constants::approx_real_sol_reserves(s, &trade.venue));
    Ok(trade)
}


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
    // Defaulted for the same reason as `ix_labels` — only the trade-history reads
    // project it. `None` = column absent from the SELECT *or* a pre-0005 row (no
    // raw_txs to re-decode, so those stay unknown forever). Never coalesce to 0:
    // a landed tx always paid the base fee, so 0 would be a false reading.
    #[sqlx(default)]
    fee_lamports: Option<i64>,
    // The 0013 fee-budget trio. Defaulted for the same reason as `fee_lamports`:
    // only the trade-history reads project them, and `None` is both "not selected"
    // and "written before 0013". Never coalesce to 0 — for `tip_lamports` a real 0
    // is a distinct, meaningful state (see the migration).
    #[sqlx(default)]
    cu_limit: Option<i64>,
    #[sqlx(default)]
    cu_price: Option<i64>,
    #[sqlx(default)]
    tip_lamports: Option<i64>,
    // The 0014 attribution pair. Defaulted like the rest: absent from most SELECTs,
    // and NULL on every row written before 0014 (unbackfillable — `raw_txs` keeps
    // 3 days). `is_proxied` must never be coalesced to `false`: that would assert
    // the wallet signed, which is exactly the claim these columns exist to stop
    // being made for free.
    #[sqlx(default)]
    payer_address: Option<String>,
    #[sqlx(default)]
    is_proxied: Option<bool>,
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
            payer_address: r.payer_address.unwrap_or_default(),
            is_proxied: r.is_proxied,
            trade_type,
            amount_sol,
            token_amount,
            // Derived: the new table has no `price_per_token` column. The ratio is
            // computed in f64 (token cast at the divide).
            price_per_token: price_of(amount_sol, token_amount as f64),
            fee_sol: r.fee_lamports.map(lamports_to_sol),
            cu_limit: r.cu_limit.map(|v| v as u64),
            cu_price: r.cu_price.map(|v| v as u64),
            tip_lamports: r.tip_lamports.map(|v| v as u64),
            // Not projected by the history reads: only a position's own fills read
            // it, through `sum_legs_by_signatures`.
            payer_net_lamports: None,
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
        let dict = WalletDictRepo::new(self.pool.clone());
        let wallet_id = dict.intern(&trade.wallet_address).await?;
        // The payer is interned into the same dictionary as the wallet — one name
        // space for account addresses, so a router's customer and that customer's
        // own direct trades share an id.
        let payer_id = match trade.payer_address.is_empty() {
            true => None,
            false => Some(dict.intern(&trade.payer_address).await?),
        };
        if trade.is_proxied == Some(true) {
            dict.mark_proxy(wallet_id).await?;
        }

        sqlx::query(
            r#"
            INSERT INTO trades
                (mint_address, wallet_id, trade_type, venue,
                 amount_lamports, token_amount,
                 reserve_lamports, reserve_token,
                 slot, tx_index, leg_index, block_time, tx_signature, ix_labels,
                 fee_lamports, cu_limit, cu_price, tip_lamports,
                 payer_id, is_proxied, payer_net_lamports)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15,
                    $16, $17, $18, $19, $20, $21)
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
        .bind(trade.fee_sol.map(sol_to_lamports))
        .bind(trade.cu_limit.map(|v| v as i64))
        .bind(trade.cu_price.map(|v| v as i64))
        .bind(trade.tip_lamports.map(|v| v as i64))
        .bind(payer_id)
        .bind(trade.is_proxied)
        .bind(trade.payer_net_lamports)
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
                // Wallets AND payers share one dictionary and one intern
                // round-trip: they are the same kind of thing (an account
                // address), and a router's customer appears as a payer here and
                // as a wallet on its own direct trades.
                if !t.payer_address.is_empty() && seen.insert(t.payer_address.as_str()) {
                    out.push(t.payer_address.clone());
                }
            }
            out
        };
        let dict = WalletDictRepo::new(self.pool.clone());
        let wallet_ids = dict.intern_many(&unique).await?;

        // A wallet the decoder saw sign nothing is a program, and that is a fact
        // about the ADDRESS, not about this trade - so it belongs on the
        // dictionary, where history can be excluded by it too. One statement per
        // flush, and only when a flush actually carries a proxied leg.
        let proxies: Vec<i32> = {
            let mut seen: HashSet<i32> = HashSet::new();
            trades
                .iter()
                .filter(|t| t.is_proxied == Some(true))
                .filter_map(|t| wallet_ids.get(&t.wallet_address).copied())
                .filter(|id| seen.insert(*id))
                .collect()
        };
        if !proxies.is_empty() {
            dict.mark_proxies(&proxies).await?;
        }

        // Pre-decode every signature before building the query. `push_values`
        // cannot bubble a Result; binding `unwrap_or_default()` empty BYTEA let
        // malformed rows collide on the dedup key and silently disappear.
        let signatures: Vec<Vec<u8>> = trades
            .iter()
            .map(|t| sig_base58_to_bytes(&t.tx_signature))
            .collect::<anyhow::Result<_>>()?;

        for (chunk, sig_chunk) in trades
            .chunks(TRADE_INSERT_CHUNK)
            .zip(signatures.chunks(TRADE_INSERT_CHUNK))
        {
            let mut qb: sqlx::QueryBuilder<sqlx::Postgres> = sqlx::QueryBuilder::new(
                "INSERT INTO trades \
                 (mint_address, wallet_id, trade_type, venue, amount_lamports, token_amount, \
                  reserve_lamports, reserve_token, slot, tx_index, leg_index, \
                  block_time, tx_signature, ix_labels, fee_lamports, \
                  cu_limit, cu_price, tip_lamports, payer_id, is_proxied, payer_net_lamports) ",
            );
            qb.push_values(chunk.iter().zip(sig_chunk), |mut b, (t, sig)| {
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
                    .push_bind(sig.as_slice())
                    .push_bind(sqlx::types::Json(&t.instruction_labels))
                    .push_bind(t.fee_sol.map(sol_to_lamports))
                    .push_bind(t.cu_limit.map(|v| v as i64))
                    .push_bind(t.cu_price.map(|v| v as i64))
                    .push_bind(t.tip_lamports.map(|v| v as i64))
                    .push_bind(
                        (!t.payer_address.is_empty())
                            .then(|| wallet_ids.get(&t.payer_address).copied())
                            .flatten(),
                    )
                    .push_bind(t.is_proxied)
                    .push_bind(t.payer_net_lamports);
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

    /// Base58 signature of ONE print, addressed by the canonical trade order key
    /// `(mint, slot, tx_index, leg_index)`: what the signature-free token cache
    /// knows about a print it holds. The live paper fill and the trigger snapshot
    /// are priced off cached prints, so this is how their rows get the signature
    /// the chart and the trades table key on.
    ///
    /// `block_time` bounds the scan to the print's own hypertable chunk (+/- 1 s, so
    /// sub-microsecond rounding of the stored timestamp cannot miss it); the order key
    /// alone is unique and rides `idx_trades_mint_order`. `None` = not written yet
    /// (the ingest writer batches) or already pruned.
    pub async fn print_signature(
        &self,
        mint: &str,
        slot: u64,
        tx_index: u32,
        leg_index: u32,
        block_time: DateTime<Utc>,
    ) -> anyhow::Result<Option<String>> {
        let bytes: Option<Vec<u8>> = sqlx::query_scalar(
            r#"
            SELECT tx_signature
            FROM trades
            WHERE mint_address = $1 AND slot = $2 AND tx_index = $3 AND leg_index = $4
              AND block_time BETWEEN $5 - interval '1 second' AND $5 + interval '1 second'
            LIMIT 1
            "#,
        )
        .bind(mint)
        .bind(slot as i64)
        .bind(tx_index as i32)
        .bind(leg_index as i16)
        .bind(block_time)
        .fetch_optional(&self.pool)
        .await?;
        Ok(bytes.map(|b| sig_bytes_to_base58(&b)))
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

    /// Last trade on `mint` inside `[since, until]`, any wallet — the token's
    /// observed price *as of* a past moment. Used by the reaper's paper
    /// `ExitStuck` heal to close at the price the exit would have filled at
    /// instead of a fabricated breakeven.
    ///
    /// **Both bounds matter.** `since` is what keeps this cheap: `trades` is a
    /// `block_time` hypertable, so a bare `mint_address` lookup probes every
    /// chunk, while bounding it to the position's own entry prunes to the chunks
    /// that can hold the token's history. `until` is what keeps it *honest*: the
    /// newest print overall can be days after the exit fired, on a token that went
    /// on trading — pricing a stranded close at it would book a PnL that never
    /// existed. Ordering is the canonical execution order
    /// (`slot, tx_index, leg_index` — `idx_trades_mint_order`), never
    /// `block_time`, which is a partition axis and not an order key.
    pub async fn find_latest_by_mint(
        &self,
        mint: &str,
        since: DateTime<Utc>,
        until: DateTime<Utc>,
    ) -> anyhow::Result<Option<Trade>> {
        let row = sqlx::query_as::<_, TradeDbRow>(
            r#"
            SELECT t.mint_address, COALESCE(w.address, 'unknown:' || t.wallet_id::text) AS wallet_address, t.trade_type, t.venue,
                   t.amount_lamports, t.token_amount,
                   t.reserve_lamports, t.reserve_token,
                   t.slot, t.tx_index, t.leg_index, t.block_time, t.tx_signature
            FROM trades t
            LEFT JOIN wallet_dict w ON w.id = t.wallet_id
            WHERE t.mint_address = $1 AND t.block_time >= $2 AND t.block_time <= $3
            ORDER BY t.slot DESC, t.tx_index DESC, t.leg_index DESC
            LIMIT 1
            "#,
        )
        .bind(mint)
        .bind(since)
        .bind(until)
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
        // The wallet flow is per TRANSACTION, repeated on each of its legs: one
        // value per signature, and one unknown makes the mint's sum unknown.
        let rows: Vec<(String, i64, i64, Option<i64>)> = sqlx::query_as(
            r#"
            WITH legs AS (
                SELECT mint_address, tx_signature, amount_lamports, token_amount,
                       payer_net_lamports
                FROM trades
                WHERE wallet_id = $1
                  AND trade_type = 'buy'
                  AND mint_address = ANY($2)
            ),
            per_tx AS (
                SELECT mint_address, MAX(payer_net_lamports) AS flow
                FROM legs GROUP BY mint_address, tx_signature
            ),
            paid AS (
                SELECT mint_address,
                       CASE WHEN bool_or(flow IS NULL) THEN NULL ELSE -SUM(flow) END AS paid
                FROM per_tx GROUP BY mint_address
            )
            SELECT l.mint_address,
                   COALESCE(SUM(l.amount_lamports), 0)::bigint,
                   COALESCE(SUM(l.token_amount), 0)::bigint,
                   MAX(p.paid)::bigint
            FROM legs l JOIN paid p USING (mint_address)
            GROUP BY l.mint_address
            "#,
        )
        .bind(wallet_id)
        .bind(mints)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|(mint, total_cost_lamports, total_token_amount, wallet_paid_lamports)| {
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
                        wallet_paid_lamports,
                    },
                )
            })
            .collect())
    }

    /// Distinct token mints this wallet traded in the `since..=until` window
    /// (`until = None` ⇒ open-ended, i.e. up to now), ordered by the wallet's
    /// most-recent trade on each mint (recent first). `limit <= 0` returns every
    /// mint in the window (unbounded); a positive `limit` caps the response.
    /// Powers the Trader Analysis page's per-wallet token list.
    ///
    /// Every aggregate is scoped to the SAME window, so a closed upper bound
    /// reads the wallet exactly as it looked at that instant — no leg after
    /// `until` leaks into the PnL.
    ///
    /// Counts **both** buys and sells, so a mint the wallet only *exited* in the
    /// window (its buy predates `since`) still appears. An unknown wallet has no
    /// trades, so returns an empty vec without touching `trades`. Bounded by
    /// the `block_time` window (and optionally `limit`), which rides the
    /// hypertable's `block_time` partitioning.
    pub async fn wallet_traded_mints(
        &self,
        wallet: &str,
        since: DateTime<Utc>,
        until: Option<DateTime<Utc>>,
        limit: i64,
    ) -> anyhow::Result<Vec<WalletTradedMint>> {
        let Some(wallet_id) = WalletDictRepo::new(self.pool.clone()).id_for(wallet).await? else {
            return Ok(Vec::new());
        };
        let by_id = HashMap::from([(wallet_id, wallet.to_string())]);
        self.traded_mints_agg(&by_id, None, since, until, limit).await
    }

    /// The same per-`(wallet, mint)` rollup as [`wallet_traded_mints`](Self::wallet_traded_mints),
    /// for a SET of wallets, restricted to an explicit `mints` slice — the Trader
    /// Analysis page's **co-trade** read: which of these other wallets were also
    /// on the mints already on screen, and where in the tape did each enter.
    ///
    /// Scoping to the caller's mint set (rather than reading each wallet's whole
    /// window and intersecting in Rust) is what keeps this cheap: the primary
    /// wallet's mints are already known, and a comparison wallet's activity
    /// *outside* them cannot answer a co-trade question. Addresses absent from
    /// `wallet_dict` and `(wallet, mint)` pairs with no leg in the window drop
    /// out — never a zero row.
    ///
    /// Deliberately unbounded (no `limit`): the caller already bounded the mints.
    pub async fn wallets_traded_mints_on(
        &self,
        wallets: &[String],
        mints: &[String],
        since: DateTime<Utc>,
        until: Option<DateTime<Utc>>,
    ) -> anyhow::Result<Vec<WalletTradedMint>> {
        if wallets.is_empty() || mints.is_empty() {
            return Ok(Vec::new());
        }
        let dict = WalletDictRepo::new(self.pool.clone());
        let mut by_id: HashMap<i32, String> = HashMap::new();
        for w in wallets {
            // Cache-first per address; an untracked one resolves to nothing and
            // drops out rather than failing the whole read.
            if let Some(id) = dict.id_for(w).await? {
                by_id.insert(id, w.clone());
            }
        }
        if by_id.is_empty() {
            return Ok(Vec::new());
        }
        self.traded_mints_agg(&by_id, Some(mints), since, until, 0).await
    }

    /// The shared `(wallet_id, mint)` aggregate behind both readers above — ONE
    /// SQL string, so the single-wallet and co-trade paths can never drift on
    /// what an entry/exit leg or a per-side sum means.
    ///
    /// `by_id` is the resolved `wallet_dict` id → address map: its keys are the
    /// `= ANY` filter, its values re-attach the address to each row. `mints =
    /// None` ⇒ every mint in the window. `limit` is only meaningful for a SINGLE
    /// wallet — across several, `ORDER BY last_trade_at DESC LIMIT n` would cut
    /// through the union rather than per wallet, so multi-wallet callers pass 0.
    async fn traded_mints_agg(
        &self,
        by_id: &HashMap<i32, String>,
        mints: Option<&[String]>,
        since: DateTime<Utc>,
        until: Option<DateTime<Utc>>,
        limit: i64,
    ) -> anyhow::Result<Vec<WalletTradedMint>> {
        // `LIMIT NULL` = all rows (same trick as `find_by_mint_paged`). Binding
        // an `Option<i64>` lets one SQL string serve both the capped and full-
        // window callers without string-building the query.
        let limit_opt: Option<i64> = if limit <= 0 { None } else { Some(limit) };
        let wallet_ids: Vec<i32> = by_id.keys().copied().collect();
        // `amount_lamports`/`token_amount` sums per side feed the Trader Analysis
        // page's volume and average-price columns (curve-side, not PnL) — both are
        // exact-integer `SUM(...)::BIGINT` (never NULL: `COALESCE` guards the
        // FILTER'd sum when a mint has only one side in the window). A named
        // `FromRow` (rather than a wide tuple) keeps every field labelled at the
        // call site and avoids a positional mapping mistake as the count grows.
        #[derive(sqlx::FromRow)]
        struct WalletTradedMintRow {
            wallet_id: i32,
            mint_address: String,
            first_trade_at: DateTime<Utc>,
            last_trade_at: DateTime<Utc>,
            buy_count: i64,
            sell_count: i64,
            buy_lamports: i64,
            sell_lamports: i64,
            buy_token_amount: i64,
            sell_token_amount: i64,
            // Entry = the FIRST buy leg, exit = the LAST sell leg (execution
            // order), each with the reserve snapshot and own size needed to
            // reconstruct the curve depth it traded into, PLUS the leg's
            // `(slot, tx_index)` tape position — the only ordering key fine
            // enough to say who entered first (see the ARRAY_AGG note below).
            // All `Option`: a mint the wallet only exited in the window has no
            // buy side, and `reserve_lamports` is nullable for rows ingested
            // without a reserve snapshot.
            entry_at: Option<DateTime<Utc>>,
            entry_slot: Option<i64>,
            entry_tx_index: Option<i32>,
            entry_reserve_lamports: Option<i64>,
            entry_leg_lamports: Option<i64>,
            entry_venue: Option<String>,
            exit_at: Option<DateTime<Utc>>,
            exit_slot: Option<i64>,
            exit_tx_index: Option<i32>,
            exit_reserve_lamports: Option<i64>,
            exit_leg_lamports: Option<i64>,
            exit_venue: Option<String>,
        }

        let rows: Vec<WalletTradedMintRow> = sqlx::query_as(
            r#"
            SELECT wallet_id,
                   mint_address,
                   MIN(block_time) AS first_trade_at,
                   MAX(block_time) AS last_trade_at,
                   COUNT(*) FILTER (WHERE trade_type = 'buy')  AS buy_count,
                   COUNT(*) FILTER (WHERE trade_type = 'sell') AS sell_count,
                   COALESCE(SUM(amount_lamports) FILTER (WHERE trade_type = 'buy'), 0)::BIGINT  AS buy_lamports,
                   COALESCE(SUM(amount_lamports) FILTER (WHERE trade_type = 'sell'), 0)::BIGINT AS sell_lamports,
                   COALESCE(SUM(token_amount) FILTER (WHERE trade_type = 'buy'), 0)::BIGINT  AS buy_token_amount,
                   COALESCE(SUM(token_amount) FILTER (WHERE trade_type = 'sell'), 0)::BIGINT AS sell_token_amount,
                   -- Entry/exit leg picks. `(ARRAY_AGG(x ORDER BY …) FILTER (…))[1]`
                   -- is the per-side first/last row without a second scan or a
                   -- correlated subquery; the arrays are per (wallet, mint) so they
                   -- stay tiny. Execution order is (slot, tx_index, leg_index) — the
                   -- same key `find_by_mint_all` orders by, NOT `block_time`, which
                   -- is only second-precision and ties across a whole slot.
                   (ARRAY_AGG(block_time       ORDER BY slot, tx_index, leg_index) FILTER (WHERE trade_type = 'buy'))[1]  AS entry_at,
                   (ARRAY_AGG(slot             ORDER BY slot, tx_index, leg_index) FILTER (WHERE trade_type = 'buy'))[1]  AS entry_slot,
                   (ARRAY_AGG(tx_index         ORDER BY slot, tx_index, leg_index) FILTER (WHERE trade_type = 'buy'))[1]  AS entry_tx_index,
                   (ARRAY_AGG(reserve_lamports ORDER BY slot, tx_index, leg_index) FILTER (WHERE trade_type = 'buy'))[1]  AS entry_reserve_lamports,
                   (ARRAY_AGG(amount_lamports  ORDER BY slot, tx_index, leg_index) FILTER (WHERE trade_type = 'buy'))[1]  AS entry_leg_lamports,
                   (ARRAY_AGG(venue            ORDER BY slot, tx_index, leg_index) FILTER (WHERE trade_type = 'buy'))[1]  AS entry_venue,
                   (ARRAY_AGG(block_time       ORDER BY slot DESC, tx_index DESC, leg_index DESC) FILTER (WHERE trade_type = 'sell'))[1] AS exit_at,
                   (ARRAY_AGG(slot             ORDER BY slot DESC, tx_index DESC, leg_index DESC) FILTER (WHERE trade_type = 'sell'))[1] AS exit_slot,
                   (ARRAY_AGG(tx_index         ORDER BY slot DESC, tx_index DESC, leg_index DESC) FILTER (WHERE trade_type = 'sell'))[1] AS exit_tx_index,
                   (ARRAY_AGG(reserve_lamports ORDER BY slot DESC, tx_index DESC, leg_index DESC) FILTER (WHERE trade_type = 'sell'))[1] AS exit_reserve_lamports,
                   (ARRAY_AGG(amount_lamports  ORDER BY slot DESC, tx_index DESC, leg_index DESC) FILTER (WHERE trade_type = 'sell'))[1] AS exit_leg_lamports,
                   (ARRAY_AGG(venue            ORDER BY slot DESC, tx_index DESC, leg_index DESC) FILTER (WHERE trade_type = 'sell'))[1] AS exit_venue
            FROM trades
            WHERE wallet_id = ANY($1)
              AND block_time >= $2
              -- Open upper bound stays a plain NULL bind (no second SQL string);
              -- the cast is what lets Postgres type the parameter. Same for the
              -- optional mint scope: NULL ⇒ every mint in the window.
              AND ($3::timestamptz IS NULL OR block_time <= $3)
              AND ($4::text[] IS NULL OR mint_address = ANY($4))
            GROUP BY wallet_id, mint_address
            ORDER BY last_trade_at DESC
            LIMIT $5
            "#,
        )
        .bind(&wallet_ids)
        .bind(since)
        .bind(until)
        .bind(mints)
        .bind(limit_opt)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .filter_map(|r| {
                // The id came from `by_id`'s own keys, so this lookup always hits;
                // `filter_map` just avoids an unwrap on that invariant.
                let wallet_address = by_id.get(&r.wallet_id)?.clone();
                Some(WalletTradedMint {
                    wallet_address,
                    mint_address: r.mint_address,
                    first_trade_at: r.first_trade_at,
                    last_trade_at: r.last_trade_at,
                    buy_count: r.buy_count,
                    sell_count: r.sell_count,
                    buy_sol: lamports_to_sol(r.buy_lamports),
                    sell_sol: lamports_to_sol(r.sell_lamports),
                    buy_token_amount: r.buy_token_amount,
                    sell_token_amount: r.sell_token_amount,
                    entry_at: r.entry_at,
                    entry_slot: r.entry_slot,
                    entry_tx_index: r.entry_tx_index,
                    exit_at: r.exit_at,
                    exit_slot: r.exit_slot,
                    exit_tx_index: r.exit_tx_index,
                    entry_curve_sol: pre_trade_real_sol(
                        r.entry_reserve_lamports,
                        r.entry_leg_lamports,
                        r.entry_venue.as_deref(),
                        TradeType::Buy,
                    ),
                    exit_curve_sol: pre_trade_real_sol(
                        r.exit_reserve_lamports,
                        r.exit_leg_lamports,
                        r.exit_venue.as_deref(),
                        TradeType::Sell,
                    ),
                })
            })
            .collect())
    }

    /// The pool each of `mints` trades on now: the spot (SOL per raw unit) and
    /// priced SOL depth of its newest trade that carries a reserve pair — the pool
    /// an open bag would sell into. A mint with no such trade in the retained
    /// window is absent. No per-swap PumpSwap fee is stored, so `venue_fee_bps` is
    /// `None` and a migrated pool marks at the curve fee.
    ///
    /// One `idx_trades_mint_order` backward scan per mint (~7 ms each locally).
    pub async fn latest_pools(&self, mints: &[String]) -> anyhow::Result<HashMap<String, MarkQuote>> {
        if mints.is_empty() {
            return Ok(HashMap::new());
        }
        let rows: Vec<(String, i64, i64)> = sqlx::query_as(
            r#"
            SELECT m.mint, x.reserve_lamports, x.reserve_token
            FROM unnest($1::text[]) AS m(mint)
            JOIN LATERAL (
                SELECT t.reserve_lamports, t.reserve_token
                FROM trades t
                WHERE t.mint_address = m.mint
                  AND t.reserve_lamports IS NOT NULL AND t.reserve_token > 0
                ORDER BY t.slot DESC, t.tx_index DESC, t.leg_index DESC
                LIMIT 1
            ) x ON true
            "#,
        )
        .bind(mints)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(mint, reserve_lamports, reserve_token)| {
                let reserve_sol = lamports_to_sol(reserve_lamports);
                let quote = MarkQuote {
                    price: reserve_sol / reserve_token as f64,
                    reserve_sol: Some(reserve_sol),
                    venue_fee_bps: None,
                };
                (mint, quote)
            })
            .collect())
    }

    /// One wallet's transactions on `mints` in `since..=until`, legs collapsed per
    /// transaction, in tape order per mint: the input of
    /// [`wallet_episodes`](crate::strategies::wallet_ledger::wallet_episodes).
    ///
    /// A transaction's flow is its `payer_net_lamports`, and only when it is
    /// exactly this wallet's trade on this mint: the wallet paid for it, and no
    /// leg of another mint or wallet shares it. Otherwise the flow is `None`, never
    /// a curve-side substitute.
    ///
    /// Mint-scoped, so it rides `idx_trades_mint_order`. The shared-transaction
    /// test is a lookup on `(block_time, slot, tx_index)`, run only where a flow
    /// exists: ~0.15 ms a transaction on an open chunk, ~3 ms on a compressed one.
    pub async fn wallet_txs_on(
        &self,
        wallet: &str,
        mints: &[String],
        since: DateTime<Utc>,
        until: Option<DateTime<Utc>>,
    ) -> anyhow::Result<Vec<(String, WalletTx)>> {
        if mints.is_empty() {
            return Ok(Vec::new());
        }
        let Some(wallet_id) = WalletDictRepo::new(self.pool.clone()).id_for(wallet).await? else {
            return Ok(Vec::new());
        };
        #[derive(sqlx::FromRow)]
        struct WalletTxRow {
            mint_address: String,
            slot: i64,
            tx_index: i32,
            block_time: DateTime<Utc>,
            token_delta: i64,
            flow_lamports: Option<i64>,
        }
        let rows: Vec<WalletTxRow> = sqlx::query_as(
            r#"
            WITH tx AS (
                SELECT mint_address, slot, tx_index,
                       MIN(block_time) AS block_time,
                       SUM(CASE WHEN trade_type = 'buy' THEN token_amount ELSE -token_amount END)::BIGINT AS token_delta,
                       MAX(payer_net_lamports) AS payer_net_lamports,
                       COALESCE(BOOL_AND(payer_id = wallet_id), FALSE) AS wallet_pays
                FROM trades
                WHERE wallet_id = $1
                  AND mint_address = ANY($2)
                  AND block_time >= $3
                  AND ($4::timestamptz IS NULL OR block_time <= $4)
                GROUP BY mint_address, slot, tx_index
            )
            SELECT mint_address, slot, tx_index, block_time, token_delta,
                   -- CASE keeps the lookup off every transaction without a flow.
                   CASE
                       WHEN payer_net_lamports IS NULL OR NOT wallet_pays THEN NULL
                       WHEN EXISTS (
                           SELECT 1 FROM trades o
                           WHERE o.block_time = tx.block_time
                             AND o.slot = tx.slot
                             AND o.tx_index = tx.tx_index
                             AND (o.mint_address <> tx.mint_address OR o.wallet_id <> $1)
                       ) THEN NULL
                       ELSE payer_net_lamports
                   END AS flow_lamports
            FROM tx
            ORDER BY mint_address, slot, tx_index
            "#,
        )
        .bind(wallet_id)
        .bind(mints)
        .bind(since)
        .bind(until)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| {
                let tx = WalletTx {
                    slot: r.slot,
                    tx_index: r.tx_index,
                    time_ms: r.block_time.timestamp_millis(),
                    token_delta: r.token_delta,
                    flow_lamports: r.flow_lamports,
                };
                (r.mint_address, tx)
            })
            .collect())
    }

    /// Every leg on each mint inside its own `(lo_slot..=hi_slot)` window, with the
    /// columns the flow classifiers read and nothing else.
    ///
    /// One nested-loop over `idx_trades_mint_order` per window — the index is
    /// `(mint_address, slot, tx_index, leg_index)`, so each window is one index
    /// range. The `block_time` bounds are the SPAN of every window (not per-mint):
    /// slot already filters precisely, and a constant time range is what lets the
    /// planner exclude chunks before the loop starts instead of per row.
    ///
    /// `exclude_wallet` drops one address in SQL rather than in the caller, so its
    /// legs never reach a count or a sum. Unknown addresses resolve to nothing and
    /// exclude nothing.
    ///
    /// Legs, not transactions: one tx emits several, and callers that mean "one
    /// print" collapse on `(slot, tx_index)`. Filtering `leg_index = 0` here would
    /// drop the later-leg buys entirely.
    pub async fn prints_in_slot_windows(
        &self,
        windows: &[SlotWindow],
        exclude_wallet: Option<&str>,
    ) -> anyhow::Result<Vec<TapePrint>> {
        if windows.is_empty() {
            return Ok(Vec::new());
        }
        let exclude_id = match exclude_wallet {
            Some(w) => WalletDictRepo::new(self.pool.clone()).id_for(w).await?,
            None => None,
        };
        let mints: Vec<String> = windows.iter().map(|w| w.mint_address.clone()).collect();
        let lo_slots: Vec<i64> = windows.iter().map(|w| w.lo_slot).collect();
        let hi_slots: Vec<i64> = windows.iter().map(|w| w.hi_slot).collect();
        // `min`/`max` over a non-empty slice — the early return above guarantees it.
        let lo_time = windows.iter().map(|w| w.lo_time).min().expect("non-empty");
        let hi_time = windows.iter().map(|w| w.hi_time).max().expect("non-empty");

        #[derive(sqlx::FromRow)]
        struct PrintRow {
            mint_address: String,
            slot: i64,
            tx_index: i32,
            wallet_address: String,
            trade_type: String,
            amount_lamports: i64,
            ix_labels: Option<sqlx::types::Json<serde_json::Value>>,
            cu_limit: Option<i64>,
            cu_price: Option<i64>,
            tip_lamports: Option<i64>,
        }

        let rows: Vec<PrintRow> = sqlx::query_as(
            r#"
            SELECT t.mint_address, t.slot, t.tx_index,
                   COALESCE(w.address, 'unknown:' || t.wallet_id::text) AS wallet_address,
                   t.trade_type, t.amount_lamports, t.ix_labels,
                   t.cu_limit, t.cu_price, t.tip_lamports
            FROM UNNEST($1::text[], $2::bigint[], $3::bigint[])
                 AS win(mint_address, lo_slot, hi_slot)
            JOIN trades t
              ON t.mint_address = win.mint_address
             AND t.slot BETWEEN win.lo_slot AND win.hi_slot
             AND t.block_time BETWEEN $4 AND $5
            LEFT JOIN wallet_dict w ON w.id = t.wallet_id
            WHERE ($6::int IS NULL OR t.wallet_id <> $6)
            ORDER BY t.mint_address, t.slot, t.tx_index, t.leg_index
            "#,
        )
        .bind(&mints)
        .bind(&lo_slots)
        .bind(&hi_slots)
        .bind(lo_time)
        .bind(hi_time)
        .bind(exclude_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| TapePrint {
                mint_address: r.mint_address,
                slot: r.slot,
                tx_index: r.tx_index,
                wallet_address: r.wallet_address,
                is_buy: r.trade_type == "buy",
                amount_lamports: r.amount_lamports,
                ix_labels: r.ix_labels.map(|j| j.0),
                cu_limit: r.cu_limit,
                cu_price: r.cu_price,
                tip_lamports: r.tip_lamports,
            })
            .collect())
    }

    /// The oldest instant `trades` can still answer for: the start of the oldest
    /// chunk the retention policy has not dropped.
    ///
    /// Read from the chunk catalog (milliseconds) rather than as `MIN(block_time)`
    /// (seconds, and it grows with the table). Exact for this question because
    /// retention drops WHOLE chunks: everything from that boundary forward is
    /// present, and a hole above it is an ingest gap, not retention.
    ///
    /// `None` when the hypertable has no chunks at all.
    pub async fn tape_floor(&self) -> anyhow::Result<Option<DateTime<Utc>>> {
        let floor: Option<DateTime<Utc>> = sqlx::query_scalar(
            r#"
            SELECT MIN(range_start)
            FROM timescaledb_information.chunks
            WHERE hypertable_name = 'trades'
            "#,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(floor)
    }

    /// Find all trades for a token in execution order (slot, tx_index, leg_index).
    /// LEFT-joins `wallet_dict` to recover each trade's wallet address (orphaned
    /// wallet ids fall back to the `unknown:<id>` sentinel, never dropping a row).
    pub async fn find_by_mint_all(&self, mint: &str) -> anyhow::Result<Vec<Trade>> {
        let rows = sqlx::query_as::<_, TradeDbRow>(
            &format!(
            "SELECT {TRADE_HISTORY_COLUMNS} {TRADE_HISTORY_FROM} WHERE t.mint_address = $1 \
             {TRADE_HISTORY_ORDER}"
        ))
        .bind(mint)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(Trade::try_from).collect()
    }

    /// Find a token's trades up to and including `until`, in execution order, with
    /// `real_reserve_sol` reconstructed.
    ///
    /// The **replay** read (`live`'s closed-position rule readout): a fold to one past
    /// instant needs the history *up to* it and nothing after, and a memecoin that keeps
    /// trading for hours past a position's exit would otherwise transfer and walk rows
    /// that cannot affect the answer. Bounding in SQL keeps that off the deploy box.
    ///
    /// `real_reserve_sol` is the same [`approx_real_sol_reserves`] reconstruction
    /// [`Self::find_by_mints_all`] applies and for the same reason — the column was
    /// dropped from `trades`, so every offline reader rebuilds it from the priced
    /// reserve pair. It is an approximation, not the emitted value: a caller
    /// presenting these numbers must say so.
    ///
    /// [`approx_real_sol_reserves`]: crate::config::constants::approx_real_sol_reserves
    pub async fn find_by_mint_until(
        &self,
        mint: &str,
        until: chrono::DateTime<chrono::Utc>,
    ) -> anyhow::Result<Vec<Trade>> {
        let rows = sqlx::query_as::<_, TradeDbRow>(
            &format!(
            "SELECT {TRADE_HISTORY_COLUMNS} {TRADE_HISTORY_FROM} WHERE t.mint_address = $1 \
             AND t.block_time <= $2 {TRADE_HISTORY_ORDER}"
        ))
        .bind(mint)
        .bind(until)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(replayable_trade).collect()
    }

    /// A token's trades strictly before the chain position `(slot, tx_index, leg)`,
    /// in execution order, with `real_reserve_sol` reconstructed as
    /// [`Self::find_by_mint_until`] does.
    ///
    /// The **rebuild** read (`live`'s rule-activation hydration): the in-RAM cache
    /// keeps the newest trades of a token, and this returns everything before the
    /// oldest one it still holds, so the two splice into the whole history. `since`
    /// bounds the scan to the token's own chunks (its creation, less a margin).
    pub async fn find_by_mint_before(
        &self,
        mint: &str,
        since: chrono::DateTime<chrono::Utc>,
        before: (u64, u32, u32),
    ) -> anyhow::Result<Vec<Trade>> {
        let rows = sqlx::query_as::<_, TradeDbRow>(&format!(
            "SELECT {TRADE_HISTORY_COLUMNS} {TRADE_HISTORY_FROM} WHERE t.mint_address = $1 \
             AND t.block_time >= $2 AND (t.slot, t.tx_index, t.leg_index) < ($3, $4, $5) {TRADE_HISTORY_ORDER}"
        ))
        .bind(mint)
        .bind(since)
        .bind(before.0 as i64)
        .bind(before.1 as i32)
        .bind(before.2 as i16)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(replayable_trade).collect()
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
            &format!(
            "SELECT {TRADE_HISTORY_COLUMNS} {TRADE_HISTORY_FROM} WHERE t.mint_address = $1 \
             {TRADE_HISTORY_ORDER} LIMIT $2 OFFSET $3"
        ))
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

        let row: SigLegsRow = sqlx::query_as(
            r#"
            WITH legs AS (
                SELECT tx_signature, token_amount, amount_lamports, block_time, slot,
                       tx_index, leg_index, payer_net_lamports, reserve_lamports, reserve_token
                FROM trades
                WHERE wallet_id = $1
                  AND mint_address = $2
                  AND trade_type = $3
                  AND tx_signature = ANY($4)
            ),
            -- The wallet flow is per TRANSACTION, repeated on each of its legs:
            -- one value per signature, and one unknown makes the sum unknown.
            per_tx AS (
                SELECT MAX(payer_net_lamports) AS flow FROM legs GROUP BY tx_signature
            )
            SELECT COUNT(*)::bigint,
                   COALESCE(SUM(token_amount), 0)::bigint,
                   COALESCE(SUM(amount_lamports), 0)::bigint,
                   MIN(block_time),
                   MAX(block_time),
                   MIN(slot),
                   MAX(slot),
                   (SELECT CASE WHEN bool_or(flow IS NULL) THEN NULL ELSE SUM(flow) END
                    FROM per_tx)::bigint,
                   -- The newest leg's post-trade spot, SOL per raw unit.
                   (ARRAY_AGG(reserve_lamports::float8 / 1e9 / reserve_token::float8
                              ORDER BY slot DESC, tx_index DESC, leg_index DESC)
                        FILTER (WHERE reserve_lamports IS NOT NULL AND reserve_token > 0))[1]
            FROM legs
            "#,
        )
        .bind(wallet_id)
        .bind(mint)
        .bind(trade_type_str(trade_type))
        .bind(&sig_bytes)
        .fetch_one(&self.pool)
        .await?;

        let (
            leg_count,
            token_sum,
            lamports_sum,
            first,
            last,
            first_slot,
            last_slot,
            wallet_lamports,
            post_spot,
        ) = row;
        if leg_count == 0 {
            return Ok(None);
        }
        Ok(Some(SigLegs {
            // token_amount stays an exact integer (raw units); SOL → human f64.
            token_amount: token_sum as u64,
            amount_sol: lamports_to_sol(lamports_sum),
            wallet_lamports,
            first_block_time: first.unwrap_or_else(Utc::now),
            last_block_time: last.unwrap_or_else(Utc::now),
            // Unlike the block times there is no `now()` fallback: a missing slot
            // stays None. Substituting anything here would fabricate a latency
            // reading, which is worse than not having one.
            first_slot: first_slot.map(|v| v as u64),
            last_slot: last_slot.map(|v| v as u64),
            post_spot,
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
    /// - **Capped**: only the newest `per_mint_cap` trades per mint land in
    ///   `trades` (a per-mint `ROW_NUMBER` window), so a high-volume token reads its
    ///   recent window instead of its full unbounded history.
    /// - **Single pass**: the in-window `count`/`volume` ride along as window
    ///   aggregates computed over the *full* partition in the same scan (`SeedAgg`),
    ///   so the caller never needs a second aggregate query. The newest trade is
    ///   always in the capped run (its last row), so nothing else is aggregated.
    ///
    /// `trades` arrives oldest-first per mint (ready for `push_trade_capped`), each
    /// row the history projection ([`TRADE_HISTORY_COLUMNS`]) converted like a
    /// history read ([`replayable_trade`]): labels, fee trio and real reserve
    /// included, so a seeded cache row hashes and reads exactly like a live one.
    /// Scoped to the seeded set (`mint = ANY($1)`, chunked) and grouped while
    /// streaming so peak memory is one mint's capped run.
    ///
    /// `since` bounds the scan to trades newer than the cutoff
    /// (`SEED_TRADES_MAX_AGE_HOURS` at the caller) so TimescaleDB prunes older
    /// chunks — without it the window scan read the entire retained history and
    /// starved ingest writes for minutes per boot (2026-07-29 incident). A mint
    /// with no in-window trades never reaches `f`; the caller's no-trades path
    /// covers it. The per-mint aggregates are therefore in-window, not lifetime.
    ///
    /// **Degrades per chunk instead of aborting:** one failed chunk (statement
    /// timeout under load, a bad row) is logged and SKIPPED — the seed is a
    /// warm-start, not a correctness gate, and aborting the whole task meant
    /// every restart re-ran the identical doomed scan. Mints already flushed
    /// stay seeded; the failed chunk's mints fill from live events instead.
    pub async fn for_each_seed_mint<F>(
        &self,
        mints: &[String],
        per_mint_cap: i64,
        since: DateTime<Utc>,
        mut f: F,
    ) -> anyhow::Result<()>
    where
        F: FnMut(String, Vec<Trade>, SeedAgg),
    {
        use futures_util::TryStreamExt;

        /// One seed row: the trade columns plus the per-mint (partition-constant)
        /// aggregates carried by the window functions. `lifetime_volume` is in
        /// lamports (SUM of the integer column) and converted to f64 SOL on read.
        #[derive(sqlx::FromRow)]
        struct SeedTradeRow {
            #[sqlx(flatten)]
            trade: TradeDbRow,
            lifetime_count: i64,
            lifetime_volume: i64,
        }

        if mints.is_empty() {
            return Ok(());
        }
        for chunk in mints.chunks(SEED_MINT_CHUNK) {
            let sql = format!(
                r#"
                WITH ranked AS (
                    SELECT {TRADE_HISTORY_COLUMNS},
                           ROW_NUMBER()                          OVER w  AS rn,
                           COUNT(*)                              OVER wp AS lifetime_count,
                           COALESCE(SUM(t.amount_lamports) OVER wp, 0)::bigint AS lifetime_volume
                    {TRADE_HISTORY_FROM}
                    WHERE t.mint_address = ANY($1) AND t.block_time >= $3
                    WINDOW
                        w  AS (PARTITION BY t.mint_address
                               ORDER BY t.slot DESC, t.tx_index DESC, t.leg_index DESC),
                        wp AS (PARTITION BY t.mint_address)
                )
                SELECT * FROM ranked
                WHERE rn <= $2
                ORDER BY mint_address ASC, slot ASC, tx_index ASC, leg_index ASC
                "#
            );
            let mut stream = sqlx::query_as::<_, SeedTradeRow>(&sql)
            .bind(chunk)
            .bind(per_mint_cap)
            .bind(since)
            .fetch(&self.pool);

            // Rows are mint-contiguous (ORDER BY mint_address) and each mint lives
            // entirely within this chunk, so group on the mint boundary and flush.
            let mut cur_mint: Option<String> = None;
            let mut buf: Vec<Trade> = Vec::new();
            let mut agg: Option<SeedAgg> = None;
            let chunk_result: anyhow::Result<()> = loop {
                match stream.try_next().await {
                    Ok(Some(row)) => {
                        if cur_mint.as_deref() != Some(row.trade.mint_address.as_str()) {
                            if let (Some(m), Some(a)) = (cur_mint.take(), agg.take()) {
                                f(m, std::mem::take(&mut buf), a);
                            }
                            cur_mint = Some(row.trade.mint_address.clone());
                            agg = Some(SeedAgg {
                                lifetime_count: row.lifetime_count.max(0) as u64,
                                // lifetime_volume is a lamports SUM → convert to f64 SOL.
                                lifetime_volume: lamports_to_sol(row.lifetime_volume),
                            });
                        }
                        match replayable_trade(row.trade) {
                            Ok(t) => buf.push(t),
                            Err(e) => break Err(e),
                        }
                    }
                    Ok(None) => break Ok(()),
                    Err(e) => break Err(e.into()),
                }
            };
            match chunk_result {
                Ok(()) => {
                    // Clean end of chunk — flush the trailing mint.
                    if let (Some(m), Some(a)) = (cur_mint.take(), agg.take()) {
                        f(m, std::mem::take(&mut buf), a);
                    }
                }
                Err(e) => {
                    // The in-flight mint's buffer is dropped, NOT flushed: it holds
                    // the oldest rows of a capped run, so flushing it would seed a
                    // stale newest-price. Already-flushed mints stay.
                    tracing::warn!(
                        chunk_mints = chunk.len(),
                        error = %e,
                        "seed scan: chunk failed — skipping it (its mints will fill from live events)"
                    );
                }
            }
        }
        Ok(())
    }
}

/// One `sum_legs_by_signatures` row: leg count, Σ tokens, Σ lamports, first / last
/// block time, first / last slot, the wallet flow, the newest leg's post spot.
type SigLegsRow = (
    i64,
    i64,
    i64,
    Option<DateTime<Utc>>,
    Option<DateTime<Utc>>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<f64>,
);

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
    /// Σ amount_sol across the legs — the venue's curve-side amount, which prices
    /// the fill (`price_per_token`) but is not what the wallet moved.
    pub amount_sol: f64,
    /// Σ the payer's net SOL flow over these signatures' transactions, each counted
    /// once (`trades.payer_net_lamports`): negative for a buy, positive for a sell.
    /// What a real position books. `None` when any transaction carried no flow.
    pub wallet_lamports: Option<i64>,
    /// Earliest leg's block time (the fill's entry time).
    pub first_block_time: DateTime<Utc>,
    /// Latest leg's block time (the fill's exit time).
    pub last_block_time: DateTime<Utc>,
    /// Earliest leg's slot — the entry fill's `entry_slot` (mig 0004). `None`
    /// when no leg carried one; never defaulted, so a latency read is either
    /// real or absent.
    pub first_slot: Option<u64>,
    /// Latest leg's slot — the exit fill's `exit_slot`.
    pub last_slot: Option<u64>,
    /// Spot (`reserve_sol / reserve_token`, SOL per raw unit) of the pool right
    /// after the newest leg. `None` when that leg carried no reserve pair.
    pub post_spot: Option<f64>,
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
    /// What those buys took from the wallet: −Σ `payer_net_lamports` over their
    /// transactions, each counted once, every fee included. `None` when any of them
    /// carried no flow (written before the flow was captured).
    pub wallet_paid_lamports: Option<i64>,
}

/// One mint's slot range for [`TradeRepo::prints_in_slot_windows`].
///
/// The slot pair is the filter; the time pair only bounds which chunks the read
/// touches, so it may be wider than the slots imply — never narrower, or the
/// window loses prints the slot range asks for.
#[derive(Debug, Clone)]
pub struct SlotWindow {
    pub mint_address: String,
    /// Inclusive, both ends.
    pub lo_slot: i64,
    pub hi_slot: i64,
    pub lo_time: DateTime<Utc>,
    pub hi_time: DateTime<Utc>,
}

/// One leg inside a [`SlotWindow`] — the ix shape, the fee budget, the side, the
/// size, and its tape position. Deliberately not a [`Trade`]: this read fans out
/// over many mints, and the model's reserves / signature / price reconstruction
/// are all cost no classifier spends.
#[derive(Debug, Clone)]
pub struct TapePrint {
    pub mint_address: String,
    /// `(slot, tx_index)` is the tape order AND the transaction identity within a
    /// mint — `block_time` ties across a whole slot and cannot order two prints.
    pub slot: i64,
    pub tx_index: i32,
    pub wallet_address: String,
    pub is_buy: bool,
    pub amount_lamports: i64,
    /// The tx's ordered instruction labels. `None` on a pre-`0002` row that has no
    /// labels to read — unknowable, never an empty sequence.
    pub ix_labels: Option<serde_json::Value>,
    /// The `0013` fee trio, three-state throughout: `None` is "not captured"
    /// (every row written before the fee cutover), never a zero budget.
    pub cu_limit: Option<i64>,
    pub cu_price: Option<i64>,
    pub tip_lamports: Option<i64>,
}

/// One token a wallet traded in the window, with the wallet's interaction stats
/// on that mint — the recent-first ordering key + the wallet-specific columns for
/// the Trader Analysis token table ([`TradeRepo::wallet_traded_mints`]).
///
/// `buy_count`/`sell_count`/`buy_sol`/`sell_sol`/`*_token_amount` are all scoped to
/// the same `block_time >= since` window, so a mint the wallet only *exited* in the
/// window can show `buy_count = 0` (its buys predate the window). These are
/// curve-side activity figures; the row's PnL comes from
/// [`TradeRepo::wallet_txs_on`] through the episode ledger.
#[derive(Debug, Clone, serde::Serialize)]
pub struct WalletTradedMint {
    /// The wallet these stats belong to. Redundant on the single-wallet read,
    /// load-bearing on the co-trade one ([`TradeRepo::wallets_traded_mints_on`]),
    /// where one mint carries a row per wallet.
    pub wallet_address: String,
    pub mint_address: String,
    /// The wallet's first trade on this mint *within the window* (not lifetime) —
    /// paired with `last_trade_at` as a hold-duration proxy for the per-mint grain.
    pub first_trade_at: DateTime<Utc>,
    pub last_trade_at: DateTime<Utc>,
    pub buy_count: i64,
    pub sell_count: i64,
    /// Σ `amount_lamports` (→ human SOL) for `trade_type='buy'` legs in the window —
    /// the recorded curve-side amount, i.e. **before** the pump.fun protocol fee
    /// (see `kernel::FEE_BPS_PER_LEG`'s doc comment for how that was measured).
    pub buy_sol: f64,
    /// Σ `amount_lamports` (→ human SOL) for `trade_type='sell'` legs — likewise
    /// pre-fee (the curve-side gross proceeds, before the fee taken on the way out).
    pub sell_sol: f64,
    /// Σ raw `token_amount` bought/sold in the window. Exact integers (never
    /// negative on either side) — a negative `net_token_amount` downstream just
    /// means the wallet sold more than it bought *in this window* (position
    /// predates `since`), not that anything here underflowed.
    pub buy_token_amount: i64,
    pub sell_token_amount: i64,
    /// The wallet's FIRST buy leg in the window — the position's entry. `None`
    /// when the window holds no buy (the entry predates `since`).
    pub entry_at: Option<DateTime<Utc>>,
    /// The entry leg's tape position. `block_time` is only second-precision and
    /// ties across a whole slot, so `(slot, tx_index)` is the ONLY key that can
    /// order two wallets' entries against each other — the co-trade read's
    /// entire content. `None` exactly when `entry_at` is.
    pub entry_slot: Option<i64>,
    pub entry_tx_index: Option<i32>,
    /// The wallet's LAST sell leg in the window — the position's exit. `None`
    /// while the bag is still held.
    pub exit_at: Option<DateTime<Utc>>,
    /// The exit leg's tape position, same convention as the entry pair.
    pub exit_slot: Option<i64>,
    pub exit_tx_index: Option<i32>,
    /// Real (non-virtual) SOL in the pool **immediately before** the entry buy
    /// landed — the curve depth the wallet bought into, with its own impact
    /// backed out. `None` when there is no entry leg or the row carries no
    /// reserve snapshot. See [`pre_trade_real_sol`].
    pub entry_curve_sol: Option<f64>,
    /// Same for the exit sell — the depth it sold into, before its own impact.
    pub exit_curve_sol: Option<f64>,
}

/// Real SOL reserves **before** one leg executed, from that leg's post-trade
/// reserve snapshot and its own size.
///
/// `reserve_lamports` is the venue-neutral SOL side of the reserve pair *after*
/// the swap settled, so a leg's own impact is already inside it. A buy adds its
/// `amount_lamports` to the pool and a sell removes it, so the pre-trade reserve
/// is the snapshot minus (buy) or plus (sell) the leg. The virtual offset is
/// stripped afterwards by the [`approx_real_sol_reserves`] SSOT, which is
/// venue-dependent — the curve carries a 30-SOL virtual seed, the AMM does not.
///
/// `None` propagates when any input is missing: an absent reserve snapshot must
/// read as "unknown depth", never as depth 0.
///
/// [`approx_real_sol_reserves`]: crate::config::constants::approx_real_sol_reserves
fn pre_trade_real_sol(
    reserve_lamports: Option<i64>,
    leg_lamports: Option<i64>,
    venue: Option<&str>,
    side: TradeType,
) -> Option<f64> {
    let post = lamports_to_sol(reserve_lamports?);
    let leg = lamports_to_sol(leg_lamports.unwrap_or(0));
    let pre = match side {
        TradeType::Buy => post - leg,
        TradeType::Sell => post + leg,
    };
    Some(crate::config::constants::approx_real_sol_reserves(
        pre.max(0.0),
        venue.unwrap_or("curve"),
    ))
}

/// The in-RAM own-leg preview is the same rollup the SQL sum produces, one
/// ingest-to-PG commit earlier.
impl From<crate::state::trade_signals::ObservedLegs> for SigLegs {
    fn from(o: crate::state::trade_signals::ObservedLegs) -> Self {
        Self {
            token_amount: o.token_amount,
            amount_sol: o.amount_sol,
            wallet_lamports: o.wallet_lamports,
            first_block_time: o.first_block_time,
            last_block_time: o.last_block_time,
            first_slot: o.first_slot,
            last_slot: o.last_slot,
            post_spot: o.post_spot,
        }
    }
}

impl SigLegs {
    /// The price a real buy's position measures its PnL ratio from: the pool's spot
    /// right after the buy landed. It is the price series paper and simulate enter
    /// at, and it already holds our own impact, which stays in the pool the real
    /// position is marked against - so a real position reads 0 % the moment it
    /// fills, as a paper one does, and its take-profit / stop-loss fire on the same
    /// market move. The execution price only when the legs carried no reserve pair.
    pub fn entry_price(&self) -> f64 {
        self.post_spot
            .filter(|p| p.is_finite() && *p > 0.0)
            .unwrap_or_else(|| self.price_per_token())
    }

    /// Weighted-average execution price (Σsol / Σtokens), or 0 when no tokens.
    pub fn price_per_token(&self) -> f64 {
        if self.token_amount > 0 {
            self.amount_sol / self.token_amount as f64
        } else {
            0.0
        }
    }

    /// The SOL buy legs took from the wallet, every fee included (the flow's
    /// outflow, positive). Falls back to the curve-side `amount_sol` only for a
    /// transaction written before the flow was captured - the caller logs that it
    /// did (`.1 == false`).
    pub fn wallet_paid_sol(&self) -> (f64, bool) {
        match self.wallet_lamports {
            Some(l) => (lamports_to_sol(-l), true),
            None => (self.amount_sol, false),
        }
    }

    /// The SOL sell legs left in the wallet, every fee included - NEGATIVE when
    /// the fees exceeded the proceeds (a dust bag), because that is what the
    /// wallet moved. Same fallback as [`Self::wallet_paid_sol`].
    pub fn wallet_received_sol(&self) -> (f64, bool) {
        match self.wallet_lamports {
            Some(l) => (lamports_to_sol(l), true),
            None => (self.amount_sol, false),
        }
    }
}

/// Per-mint aggregates the cache seed needs alongside a mint's (capped) recent
/// trade run, computed in the same single scan as the trades (see
/// [`TradeRepo::for_each_seed_mint`]) over every in-window trade, not only the
/// capped run. The newest trade's own facts are read off the run's last row.
pub struct SeedAgg {
    pub lifetime_count: u64,
    pub lifetime_volume: f64,
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

    fn legs(wallet_lamports: Option<i64>) -> SigLegs {
        SigLegs {
            token_amount: 1_000,
            amount_sol: 0.1,
            wallet_lamports,
            first_block_time: Utc::now(),
            last_block_time: Utc::now(),
            first_slot: None,
            last_slot: None,
            post_spot: None,
        }
    }

    /// A real entry measures from the pool's spot after the buy; the execution
    /// price only when the legs carried no reserve pair.
    #[test]
    fn entry_price_is_the_spot_the_buy_left() {
        let mut l = legs(Some(-101_250_000));
        assert_eq!(l.entry_price(), l.price_per_token());
        l.post_spot = Some(1.2e-7);
        assert_eq!(l.entry_price(), 1.2e-7);
        l.post_spot = Some(f64::NAN);
        assert_eq!(l.entry_price(), l.price_per_token());
    }

    /// A buy books its outflow as a positive cost; a sell books what it left in
    /// the wallet, keeping the sign when its fees beat its proceeds.
    #[test]
    fn wallet_flow_keeps_its_sign() {
        assert_eq!(legs(Some(-101_250_000)).wallet_paid_sol(), (0.10125, true));
        assert_eq!(legs(Some(98_000_000)).wallet_received_sol(), (0.098, true));
        assert_eq!(legs(Some(-125_000)).wallet_received_sol(), (-0.000125, true));
        // No captured flow: the curve-side amount, flagged inexact.
        assert_eq!(legs(None).wallet_received_sol(), (0.1, false));
    }

    /// `insert_many` binds [`TRADE_INSERT_BINDS_PER_ROW`] params per row
    /// (mint_address, wallet_id, trade_type, venue, amount_lamports, token_amount,
    /// reserve_lamports, reserve_token, slot, tx_index, leg_index, block_time,
    /// tx_signature, ix_labels, fee_lamports, cu_limit, cu_price, tip_lamports,
    /// payer_id, is_proxied, payer_net_lamports); one Postgres statement is capped at 65535 (sqlx 0.6
    /// wraps `len() as i16` past it → a Postgres parse error). Pin the chunk so
    /// adding a bound column re-checks the ceiling here instead of surfacing as a
    /// runtime parse error on the backfill path.
    // ── pre_trade_real_sol (Trader Analysis entry/exit curve depth) ─────────

    /// A buy's own SOL is already inside its post-trade snapshot, so backing it
    /// out is what makes the figure "the depth it bought into". 62.5 vSOL after
    /// a 2.5 SOL buy = 60.0 vSOL before = 30.0 real once the curve's virtual
    /// seed is stripped.
    #[test]
    fn pre_trade_real_sol_backs_a_buy_out_of_its_own_snapshot() {
        let sol = pre_trade_real_sol(
            Some(sol_to_lamports(62.5)),
            Some(sol_to_lamports(2.5)),
            Some("curve"),
            TradeType::Buy,
        )
        .unwrap();
        assert!((sol - 30.0).abs() < 1e-6, "got {sol}");
    }

    /// A sell REMOVES SOL, so its pre-trade pool is larger than the snapshot —
    /// the opposite sign. 57.5 vSOL after a 2.5 SOL sell = 60.0 before.
    #[test]
    fn pre_trade_real_sol_adds_a_sell_back() {
        let sol = pre_trade_real_sol(
            Some(sol_to_lamports(57.5)),
            Some(sol_to_lamports(2.5)),
            Some("curve"),
            TradeType::Sell,
        )
        .unwrap();
        assert!((sol - 30.0).abs() < 1e-6, "got {sol}");
    }

    /// The AMM's virtual quote is PumpSwap's, not the curve's 30 SOL seed.
    #[test]
    fn pre_trade_real_sol_takes_the_pools_virtual_quote_on_amm() {
        let virtual_quote = crate::config::constants::PUMP_SWAP_VIRTUAL_QUOTE_SOL;
        let sol = pre_trade_real_sol(
            Some(sol_to_lamports(62.5 + virtual_quote)),
            Some(sol_to_lamports(2.5)),
            Some("amm"),
            TradeType::Buy,
        )
        .unwrap();
        assert!((sol - 60.0).abs() < 1e-6, "got {sol}");
    }

    // ── prints_in_slot_windows (pre-entry ix probe) ─────────────────────────

    /// Self-skips without a reachable `DATABASE_URL`, so a keyless run stays green.
    async fn probe_pool() -> Option<PgPool> {
        let url = std::env::var("DATABASE_URL").ok()?;
        PgPoolOptions::new().max_connections(2).connect(&url).await.ok()
    }

    /// The per-mint window read against real tape: the binds encode, the UNNEST
    /// arity holds, and every row lands inside the slot range it was asked for,
    /// in execution order. Pure read — seeds nothing, deletes nothing.
    #[tokio::test]
    #[ignore = "requires a local Postgres (DATABASE_URL); run with --ignored"]
    async fn prints_in_slot_windows_reads_each_window_in_order() {
        let Some(pool) = probe_pool().await else { return };
        let repo = TradeRepo::new(pool.clone());

        // Anchor on real tape rather than a fixture: the point of this test is the
        // live column set (`ix_labels` + the fee trio) surviving the round trip.
        let Some((mint, slot, at)): Option<(String, i64, DateTime<Utc>)> = sqlx::query_as(
            "SELECT mint_address, slot, block_time FROM trades              WHERE block_time > now() - interval '2 days' ORDER BY block_time DESC LIMIT 1",
        )
        .fetch_optional(&pool)
        .await
        .expect("anchor read") else {
            return; // no recent tape on this box
        };

        let win = SlotWindow {
            mint_address: mint.clone(),
            lo_slot: slot - 50,
            hi_slot: slot,
            lo_time: at - chrono::Duration::minutes(5),
            hi_time: at + chrono::Duration::minutes(1),
        };
        let prints = repo
            .prints_in_slot_windows(std::slice::from_ref(&win), None)
            .await
            .expect("window read");

        let mut last = (0i64, 0i32);
        for p in &prints {
            assert_eq!(p.mint_address, mint);
            assert!(p.slot >= win.lo_slot && p.slot <= win.hi_slot, "slot {} outside", p.slot);
            assert!((p.slot, p.tx_index) >= last, "rows must arrive in execution order");
            last = (p.slot, p.tx_index);
        }

        // The oldest chunk start is what a truncated probe window runs into.
        let floor = repo.tape_floor().await.expect("tape floor");
        assert!(floor.is_some_and(|f| f <= at), "floor must not sit after live tape");
    }

    /// An empty window list is answered without touching the database — the
    /// probe's own early return, which a page with no anchors relies on.
    #[tokio::test]
    async fn no_windows_is_no_query() {
        let repo = TradeRepo::new(
            PgPoolOptions::new()
                .connect_lazy("postgres://invalid:invalid@127.0.0.1:1/none")
                .expect("lazy pool"),
        );
        assert!(repo.prints_in_slot_windows(&[], None).await.unwrap().is_empty());
    }

    /// A row with no reserve snapshot has UNKNOWN depth. Reading it as 0 would
    /// paint a fresh-launch entry on every un-snapshotted leg.
    #[test]
    fn pre_trade_real_sol_is_none_without_a_snapshot() {
        assert_eq!(
            pre_trade_real_sol(None, Some(sol_to_lamports(2.5)), Some("curve"), TradeType::Buy),
            None
        );
    }

    #[test]
    fn trade_insert_chunk_stays_under_param_ceiling() {
        const BINDS_PER_ROW: usize = TRADE_INSERT_BINDS_PER_ROW;
        assert!(
            TRADE_INSERT_CHUNK * BINDS_PER_ROW <= 65_535,
            "TRADE_INSERT_CHUNK ({TRADE_INSERT_CHUNK}) × {BINDS_PER_ROW} binds exceeds the 65535 ceiling"
        );
    }

    /// Malformed signatures must fail closed — never become empty BYTEA keys that
    /// collide on `(block_time, tx_signature, leg_index)`.
    #[test]
    fn invalid_signature_rejected_before_insert() {
        let err = sig_base58_to_bytes("not-a-valid-base58-signature!!")
            .expect_err("garbage signature must error");
        assert!(
            err.to_string().contains("invalid base58 signature"),
            "unexpected error: {err}"
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
