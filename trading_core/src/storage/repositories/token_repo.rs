use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::postgres::PgArguments;
use sqlx::{types::Json, Postgres, PgPool};
use uuid::Uuid;

use crate::api::handlers::tokens::SqlArg;
use crate::config::constants::{lamports_to_sol, sol_to_lamports};
use crate::models::token::Token;
use crate::storage::token_enrichment::MARKET_CAP_SQL;

/// Bind an ordered `SqlArg` slice onto a `query_as` builder, in `$1..$n` order.
/// Split from the scalar variant because sqlx's `Query`/`QueryScalar` are distinct
/// types with no shared bind trait we can name here.
fn bind_sql_args<'q, O>(
    mut query: sqlx::query::QueryAs<'q, Postgres, O, PgArguments>,
    args: &'q [SqlArg],
) -> sqlx::query::QueryAs<'q, Postgres, O, PgArguments> {
    for arg in args {
        query = match arg {
            SqlArg::Str(s) => query.bind(s),
            SqlArg::F64(v) => query.bind(*v),
            SqlArg::I64(v) => query.bind(*v),
            SqlArg::Ts(t) => query.bind(*t),
            SqlArg::Bool(b) => query.bind(*b),
            SqlArg::StrArray(a) => query.bind(a),
        };
    }
    query
}

/// Same as [`bind_sql_args`] for a `query_scalar` (`COUNT(*)`) builder.
fn bind_sql_args_scalar<'q, O>(
    mut query: sqlx::query::QueryScalar<'q, Postgres, O, PgArguments>,
    args: &'q [SqlArg],
) -> sqlx::query::QueryScalar<'q, Postgres, O, PgArguments> {
    for arg in args {
        query = match arg {
            SqlArg::Str(s) => query.bind(s),
            SqlArg::F64(v) => query.bind(*v),
            SqlArg::I64(v) => query.bind(*v),
            SqlArg::Ts(t) => query.bind(*t),
            SqlArg::Bool(b) => query.bind(*b),
            SqlArg::StrArray(a) => query.bind(a),
        };
    }
    query
}

pub struct TokenRepo {
    pool: PgPool,
}

// ---------------------------------------------------------------------------
// DB row — keeps sqlx derives out of domain models
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct TokenDbRow {
    mint_address: String,
    creator_wallet: String,
    name: String,
    symbol: String,
    token_program_id: Option<String>,
    bonding_curve_address: Option<String>,
    initial_supply_token: Option<i64>,
    /// Lamports (BIGINT); converted to human SOL on read.
    initial_buy_lamports: Option<i64>,
    initial_buy_instruction: Option<Json<Value>>,
    cu_limit: Option<i64>,
    cu_price: Option<i64>,
    is_mayhem_mode: bool,
    is_cashback_enabled: bool,
    ix_labels: Json<Value>,
    creation_tx_signature: String,
    creation_slot: Option<i64>,
    created_at: DateTime<Utc>,
}

/// A joined `tokens` + `tokens_info` row, shaped to build a `TokenSummary`
/// directly (so the list endpoint can serve mints no longer resident in the live
/// cache). All `tokens_info` columns are `Option` because the join is a LEFT JOIN
/// — a token that has never traded (or predates its first metrics flush) has no
/// `tokens_info` row yet, exactly like a freshly-tracked mint with zeroed stats.
///
/// Note: `tokens_info.age` is the wall-clock age at the last metrics flush.
/// `lifetime_secs` is read from the DB column (written at eviction / final flush).
#[derive(sqlx::FromRow)]
pub struct TokenListRow {
    // tokens
    pub mint_address: String,
    pub creator_wallet: String,
    pub name: String,
    pub symbol: String,
    pub bonding_curve_address: Option<String>,
    pub initial_supply_token: Option<i64>,
    pub initial_buy_sol: Option<f64>,
    pub initial_buy_instruction: Option<Json<Value>>,
    pub cu_limit: Option<i64>,
    pub cu_price: Option<i64>,
    pub is_mayhem_mode: bool,
    pub is_cashback_enabled: bool,
    pub ix_labels: Json<Value>,
    pub creation_tx_signature: String,
    pub created_at: DateTime<Utc>,
    // tokens_info (LEFT JOIN → all nullable)
    pub ath_price: Option<f64>,
    pub ath_timestamp: Option<DateTime<Utc>>,
    pub volume_sol: Option<f64>,
    pub market_cap: Option<f64>,
    pub trade_count: Option<i64>,
    pub last_trade_at: Option<DateTime<Utc>>,
    pub current_price: Option<f64>,
    pub is_dead: Option<bool>,
    pub is_migrated: Option<bool>,
    pub last_synced_at: Option<DateTime<Utc>>,
    pub lifetime_secs: Option<i64>,
    // Same-creation-slot buy/sell SOL totals. Stored lamports (BIGINT); the SELECT
    // divides by 1e9 so these arrive as human SOL f64, like `initial_buy_sol`.
    pub first_slot_buy_sol: Option<f64>,
    pub first_slot_sell_sol: Option<f64>,
}

impl From<TokenDbRow> for Token {
    fn from(r: TokenDbRow) -> Self {
        Self {
            // `tokens` is now mint-keyed (no surrogate id column). The runtime
            // `Token` model still carries a `Uuid` id (consumed by caches/handlers
            // until Phase 2/3); synthesize a fresh one on read — nothing persists
            // or joins on it.
            id: Uuid::new_v4(),
            mint_address: r.mint_address,
            creator_wallet: r.creator_wallet,
            name: r.name,
            symbol: r.symbol,
            token_program_id: r.token_program_id,
            bonding_curve_address: r.bonding_curve_address,
            initial_supply_token: r.initial_supply_token.map(|v| v as u64),
            // Lamports (BIGINT) → human SOL f64 on read.
            initial_buy_sol: r.initial_buy_lamports.map(lamports_to_sol),
            initial_buy_instruction: r.initial_buy_instruction.map(|v| v.0),
            cu_limit: r.cu_limit.map(|v| v as u64),
            cu_price: r.cu_price.map(|v| v as u64),
            is_mayhem_mode: r.is_mayhem_mode,
            is_cashback_enabled: r.is_cashback_enabled,
            instruction_labels: r.ix_labels.0,
            creation_tx_signature: r.creation_tx_signature,
            creation_slot: r.creation_slot.map(|v| v as u64),
            created_at: r.created_at,
        }
    }
}

// SOL ↔ lamports use the shared `config::constants` DB-boundary helpers:
// `initial_buy_sol` is human SOL (f64) in the model but stored as exact lamports
// (BIGINT) in the column, mirroring `trades.amount_lamports`.

// ---------------------------------------------------------------------------
// Repo
// ---------------------------------------------------------------------------

/// Columns selected for `TokenDbRow`, in struct order. Explicit (not `SELECT *`)
/// so a future column added to `tokens` isn't pulled into every read, and the
/// query's wire contract is decoupled from the physical table layout.
const TOKEN_COLS: &str = "mint_address, creator_wallet, name, symbol, token_program_id, \
    bonding_curve_address, initial_supply_token, initial_buy_lamports, initial_buy_instruction, \
    cu_limit, cu_price, is_mayhem_mode, is_cashback_enabled, ix_labels, creation_tx_signature, \
    creation_slot, created_at";

impl TokenRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Bulk-insert a slice of tokens in one round-trip. Silently ignores duplicates
    /// (`ON CONFLICT DO NOTHING`), matching `insert`'s semantics. Falls back to
    /// per-row via `insert` for the caller's error-handling path. Empty slice is a
    /// no-op.
    pub async fn insert_many(&self, tokens: &[Token]) -> anyhow::Result<()> {
        if tokens.is_empty() {
            return Ok(());
        }
        let mut qb = sqlx::QueryBuilder::new(
            "INSERT INTO tokens \
             (mint_address, creator_wallet, name, symbol, token_program_id, \
              bonding_curve_address, initial_supply_token, initial_buy_lamports, initial_buy_instruction, \
              cu_limit, cu_price, is_mayhem_mode, is_cashback_enabled, ix_labels, \
              creation_tx_signature, creation_slot, created_at) ",
        );
        qb.push_values(tokens, |mut b, t| {
            b.push_bind(&t.mint_address)
                .push_bind(&t.creator_wallet)
                .push_bind(&t.name)
                .push_bind(&t.symbol)
                .push_bind(t.token_program_id.as_ref())
                .push_bind(&t.bonding_curve_address)
                .push_bind(t.initial_supply_token.map(|v| v as i64))
                .push_bind(t.initial_buy_sol.map(sol_to_lamports))
                .push_bind(t.initial_buy_instruction.as_ref().map(Json))
                .push_bind(t.cu_limit.map(|v| v as i64))
                .push_bind(t.cu_price.map(|v| v as i64))
                .push_bind(t.is_mayhem_mode)
                .push_bind(t.is_cashback_enabled)
                .push_bind(Json(&t.instruction_labels))
                .push_bind(&t.creation_tx_signature)
                .push_bind(t.creation_slot.map(|v| v as i64))
                .push_bind(t.created_at);
        });
        qb.push(" ON CONFLICT (mint_address) DO NOTHING");
        qb.build().execute(&self.pool).await?;
        Ok(())
    }

    /// Insert a new token. Silently ignores duplicates (idempotent on replay).
    pub async fn insert(&self, token: &Token) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO tokens
                (mint_address, creator_wallet, name, symbol, token_program_id,
                    bonding_curve_address, initial_supply_token, initial_buy_lamports, initial_buy_instruction, cu_limit, cu_price, is_mayhem_mode, is_cashback_enabled, ix_labels,
                    creation_tx_signature, creation_slot, created_at)
              VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)
            ON CONFLICT (mint_address) DO NOTHING
            "#,
        )
        .bind(&token.mint_address)
        .bind(&token.creator_wallet)
        .bind(&token.name)
        .bind(&token.symbol)
        .bind(token.token_program_id.as_ref())
        .bind(&token.bonding_curve_address)
        .bind(token.initial_supply_token.map(|v| v as i64))
        .bind(token.initial_buy_sol.map(sol_to_lamports))
        .bind(token.initial_buy_instruction.as_ref().map(|v| Json(v)))
        .bind(token.cu_limit.map(|v| v as i64))
        .bind(token.cu_price.map(|v| v as i64))
        .bind(token.is_mayhem_mode)
        .bind(token.is_cashback_enabled)
        .bind(Json(&token.instruction_labels))
        .bind(&token.creation_tx_signature)
        .bind(token.creation_slot.map(|v| v as i64))
        .bind(token.created_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Insert or refresh token metadata from a parsed create transaction.
    pub async fn upsert(&self, token: &Token) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO tokens
                (mint_address, creator_wallet, name, symbol, token_program_id,
                    bonding_curve_address, initial_supply_token, initial_buy_lamports, initial_buy_instruction, cu_limit, cu_price, is_mayhem_mode, is_cashback_enabled, ix_labels,
                    creation_tx_signature, creation_slot, created_at)
              VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)
            ON CONFLICT (mint_address) DO UPDATE SET
                creator_wallet = EXCLUDED.creator_wallet,
                name = EXCLUDED.name,
                symbol = EXCLUDED.symbol,
                token_program_id = EXCLUDED.token_program_id,
                bonding_curve_address = EXCLUDED.bonding_curve_address,
                initial_supply_token = EXCLUDED.initial_supply_token,
                initial_buy_lamports = EXCLUDED.initial_buy_lamports,
                initial_buy_instruction = EXCLUDED.initial_buy_instruction,
                cu_limit = EXCLUDED.cu_limit,
                cu_price = EXCLUDED.cu_price,
                is_mayhem_mode = EXCLUDED.is_mayhem_mode,
                is_cashback_enabled = EXCLUDED.is_cashback_enabled,
                ix_labels = EXCLUDED.ix_labels,
                creation_tx_signature = EXCLUDED.creation_tx_signature,
                creation_slot = COALESCE(EXCLUDED.creation_slot, tokens.creation_slot),
                created_at = LEAST(tokens.created_at, EXCLUDED.created_at)
            "#,
        )
        .bind(&token.mint_address)
        .bind(&token.creator_wallet)
        .bind(&token.name)
        .bind(&token.symbol)
        .bind(token.token_program_id.as_ref())
        .bind(&token.bonding_curve_address)
        .bind(token.initial_supply_token.map(|v| v as i64))
        .bind(token.initial_buy_sol.map(sol_to_lamports))
        .bind(token.initial_buy_instruction.as_ref().map(|v| Json(v)))
        .bind(token.cu_limit.map(|v| v as i64))
        .bind(token.cu_price.map(|v| v as i64))
        .bind(token.is_mayhem_mode)
        .bind(token.is_cashback_enabled)
        .bind(Json(&token.instruction_labels))
        .bind(&token.creation_tx_signature)
        .bind(token.creation_slot.map(|v| v as i64))
        .bind(token.created_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn find_by_mint(&self, mint: &str) -> anyhow::Result<Option<Token>> {
        let row = sqlx::query_as::<_, TokenDbRow>(&format!(
            "SELECT {TOKEN_COLS} FROM tokens WHERE mint_address = $1"
        ))
            .bind(mint)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.map(Token::from))
    }

    /// Load the most-recent tokens created since `since` for cache seeding on
    /// startup, capped at `limit`. Bounded on *both* axes — recency window and row
    /// cap — rather than an unbounded full-table scan over the continuously growing
    /// `tokens` table, so cold start scales with recent activity, not total history.
    /// See [`crate::config::constants::SEED_TRACKING_LIMIT`] /
    /// [`crate::config::constants::SEED_ACTIVITY_WINDOW_DAYS`]. The `created_at >=`
    /// predicate rides `idx_tokens_created_at`; unsettled-position mints older than
    /// the window are seeded separately (see `find_by_mints`).
    pub async fn find_recent_active(
        &self,
        limit: i64,
        since: DateTime<Utc>,
    ) -> anyhow::Result<Vec<Token>> {
        let rows = sqlx::query_as::<_, TokenDbRow>(&format!(
            "SELECT {TOKEN_COLS} FROM tokens WHERE created_at >= $1 ORDER BY created_at DESC LIMIT $2"
        ))
        .bind(since)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(Token::from).collect())
    }

    /// One keyset page of tokens, newest-first, for the analysis scans (tpsl
    /// matched / simulate) that must consider the **whole** `tokens` table rather
    /// than only the live cache. `cursor = None` fetches the first page; otherwise
    /// rows strictly older than the cursor `(created_at, mint_address)` are returned,
    /// so deep pages never pay an `OFFSET` scan. Optional `since` (inclusive) / `until`
    /// (exclusive) clip the scan to a creation-time window; all three predicates ride
    /// `idx_tokens_created_at`. The caller loops, advancing the cursor to the page's
    /// last row, until a short (< `limit`) page ends the scan — keeping only the sparse
    /// matches resident, never the whole table. Reuses the explicit `TOKEN_COLS`.
    pub async fn find_page_before(
        &self,
        cursor: Option<(DateTime<Utc>, String)>,
        limit: i64,
        since: Option<DateTime<Utc>>,
        until: Option<DateTime<Utc>>,
    ) -> anyhow::Result<Vec<Token>> {
        let (cursor_at, cursor_mint) = match cursor {
            Some((at, mint)) => (Some(at), Some(mint)),
            None => (None, None),
        };
        let rows = sqlx::query_as::<_, TokenDbRow>(&format!(
            "SELECT {TOKEN_COLS} FROM tokens \
             WHERE ($1::timestamptz IS NULL OR (created_at, mint_address) < ($1, $2)) \
               AND ($3::timestamptz IS NULL OR created_at >= $3) \
               AND ($4::timestamptz IS NULL OR created_at < $4) \
             ORDER BY created_at DESC, mint_address DESC \
             LIMIT $5"
        ))
        .bind(cursor_at)
        .bind(cursor_mint)
        .bind(since)
        .bind(until)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(Token::from).collect())
    }

    /// Load the token-list rows (`tokens LEFT JOIN tokens_info`) created since
    /// `since`, newest-first, capped at `limit`. Backs the DB-base of the
    /// `GET /api/tokens` snapshot so the list reflects the whole seeded universe —
    /// including mints already evicted from the live cache — not just resident
    /// ones. Bounded on both axes (recency window + row cap), index-servable via
    /// `idx_tokens_created_at`, so it never scans the full, forever-growing
    /// `tokens` table. Mirrors `find_recent_active`'s bounding contract.
    pub async fn find_list_rows(
        &self,
        limit: i64,
        since: DateTime<Utc>,
    ) -> anyhow::Result<Vec<TokenListRow>> {
        let sql = format!(
            // `created_at DESC, mint_address DESC` matches the in-RAM snapshot's
            // `newest_first` order (token_list_cache) so the two pre-sorted halves
            // stay mergeable and the LIMIT boundary is deterministic across refreshes.
            "{} WHERE t.created_at >= $1 ORDER BY t.created_at DESC, t.mint_address DESC LIMIT $2",
            Self::list_row_select()
        );
        let rows = sqlx::query_as::<_, TokenListRow>(&sql)
            .bind(since)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;

        Ok(rows)
    }

    /// The full `TokenListRow` `SELECT … FROM tokens t LEFT JOIN tokens_info i`
    /// projection shared by the mint-scoped list lookups (`find_list_rows`,
    /// `find_list_rows_for_mints`, `find_list_row_by_mint`) — one definition so the
    /// ~28-column projection (market_cap via [`MARKET_CAP_SQL`] included) can't drift
    /// between them; each caller appends only its own `WHERE`. The paged
    /// `find_list_page` builds its own SELECT because it sources `last_synced_at`
    /// from [`LIST_FROM`]'s lateral join rather than the correlated subquery here.
    fn list_row_select() -> String {
        format!(
            "SELECT t.mint_address, t.creator_wallet, t.name, t.symbol, \
                    t.bonding_curve_address, t.initial_supply_token, \
                    t.initial_buy_lamports::float8 / 1e9 AS initial_buy_sol, t.initial_buy_instruction, \
                    t.cu_limit, t.cu_price, t.is_mayhem_mode, t.is_cashback_enabled, \
                    t.ix_labels, t.creation_tx_signature, t.created_at, \
                    i.ath_price, i.ath_timestamp, i.volume_sol, \
                    {MARKET_CAP_SQL} AS market_cap, i.trade_count, \
                    i.last_trade_at, i.current_price, i.is_dead, i.is_migrated, \
                    (SELECT MAX(s.last_synced_at) FROM token_sync_state s \
                       WHERE s.mint_address = t.mint_address) AS last_synced_at, \
                    i.lifetime_secs, \
                    i.first_slot_buy_lamports::float8 / 1e9 AS first_slot_buy_sol, \
                    i.first_slot_sell_lamports::float8 / 1e9 AS first_slot_sell_sol \
             FROM tokens t \
             LEFT JOIN tokens_info i ON i.mint_address = t.mint_address"
        )
    }

    /// Fragment shared by the DB-paged list methods: the `tokens t LEFT JOIN
    /// tokens_info i` core plus a `LEFT JOIN LATERAL` that surfaces the per-mint
    /// `last_synced_at` as `sync.last_synced_at`, so the dynamic WHERE/ORDER built
    /// by `api::handlers::tokens::sql` can reference it as a plain column.
    const LIST_FROM: &'static str = "FROM tokens t \
         LEFT JOIN tokens_info i ON i.mint_address = t.mint_address \
         LEFT JOIN LATERAL (SELECT MAX(s.last_synced_at) AS last_synced_at \
                              FROM token_sync_state s WHERE s.mint_address = t.mint_address) sync ON true";

    /// DB-paged token-list page for the live bin's `/api/tokens`. Takes the
    /// pre-built `WHERE`/`ORDER BY` bodies + positional args from
    /// `api::handlers::tokens::sql::build_where_and_order`, appends `LIMIT`/`OFFSET`,
    /// and returns one page of `TokenListRow`. The whole `tokens` universe is
    /// pageable — there is no recency window or row cap here (the 7-day window only
    /// governs the live *cache* seed, not this list). Deep `OFFSET` scans-and-
    /// discards; acceptable for now (see the plan's keyset note).
    pub async fn find_list_page(
        &self,
        where_sql: &str,
        order_sql: &str,
        args: &[SqlArg],
        limit: i64,
        offset: i64,
    ) -> anyhow::Result<Vec<TokenListRow>> {
        // LIMIT / OFFSET are bound after the WHERE/ORDER args, so their `$n` indices
        // are (args.len()+1) and (args.len()+2).
        let limit_ph = args.len() + 1;
        let offset_ph = args.len() + 2;
        let sql = format!(
            "SELECT t.mint_address, t.creator_wallet, t.name, t.symbol, \
                    t.bonding_curve_address, t.initial_supply_token, \
                    t.initial_buy_lamports::float8 / 1e9 AS initial_buy_sol, t.initial_buy_instruction, \
                    t.cu_limit, t.cu_price, t.is_mayhem_mode, t.is_cashback_enabled, \
                    t.ix_labels, t.creation_tx_signature, t.created_at, \
                    i.ath_price, i.ath_timestamp, i.volume_sol, \
                    {MARKET_CAP_SQL} AS market_cap, i.trade_count, \
                    i.last_trade_at, i.current_price, i.is_dead, i.is_migrated, \
                    sync.last_synced_at, i.lifetime_secs, \
                    i.first_slot_buy_lamports::float8 / 1e9 AS first_slot_buy_sol, \
                    i.first_slot_sell_lamports::float8 / 1e9 AS first_slot_sell_sol \
             {} WHERE {} ORDER BY {} LIMIT ${} OFFSET ${}",
            Self::LIST_FROM, where_sql, order_sql, limit_ph, offset_ph,
        );
        let mut query = sqlx::query_as::<_, TokenListRow>(&sql);
        query = bind_sql_args(query, args);
        let rows = query.bind(limit).bind(offset).fetch_all(&self.pool).await?;
        Ok(rows)
    }

    /// Filtered `COUNT(*)` over the whole token universe for the same `WHERE` the
    /// page uses — the pager's `total`. Bounded only by the filters (no window/cap),
    /// so it reflects the true filtered universe size.
    pub async fn count_list(&self, where_sql: &str, args: &[SqlArg]) -> anyhow::Result<i64> {
        let sql = format!("SELECT COUNT(*) {} WHERE {}", Self::LIST_FROM, where_sql);
        let mut query = sqlx::query_scalar::<_, i64>(&sql);
        query = bind_sql_args_scalar(query, args);
        Ok(query.fetch_one(&self.pool).await?)
    }

    /// Load the full `Token` rows for an explicit set of mints (`mint = ANY($1)`,
    /// chunked). Used by the cache seed to pull in mints that fall outside the
    /// recency window but must still be tracked (e.g. an unsettled position older
    /// than `SEED_ACTIVITY_WINDOW_DAYS`). Mints with no row are simply absent.
    pub async fn find_by_mints(&self, mints: &[String]) -> anyhow::Result<Vec<Token>> {
        /// Mints per round-trip; keeps each `= ANY($1)` array index-friendly.
        const MINT_CHUNK: usize = 1000;
        if mints.is_empty() {
            return Ok(Vec::new());
        }
        let mut out = Vec::with_capacity(mints.len());
        for chunk in mints.chunks(MINT_CHUNK) {
            let rows = sqlx::query_as::<_, TokenDbRow>(&format!(
                "SELECT {TOKEN_COLS} FROM tokens WHERE mint_address = ANY($1)"
            ))
            .bind(chunk)
            .fetch_all(&self.pool)
            .await?;
            out.extend(rows.into_iter().map(Token::from));
        }
        Ok(out)
    }

    /// Load the token-list rows (`tokens LEFT JOIN tokens_info`) for an explicit set
    /// of mints. Used by `GET /api/tokens/batch` so strategy pages can enrich their
    /// result tables with full token metadata. Empty input returns immediately;
    /// results are in unspecified (DB) order.
    pub async fn find_list_rows_for_mints(&self, mints: &[String]) -> anyhow::Result<Vec<TokenListRow>> {
        if mints.is_empty() {
            return Ok(Vec::new());
        }
        let sql = format!("{} WHERE t.mint_address = ANY($1)", Self::list_row_select());
        let rows = sqlx::query_as::<_, TokenListRow>(&sql)
            .bind(mints)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows)
    }

    /// Single-mint detail lookup with `tokens_info` stats (LEFT JOIN). Used by
    /// `GET /api/tokens/:mint` slow path so the detail modal shows real stats for
    /// tokens that were evicted from the live cache.
    pub async fn find_list_row_by_mint(&self, mint: &str) -> anyhow::Result<Option<TokenListRow>> {
        let sql = format!("{} WHERE t.mint_address = $1", Self::list_row_select());
        let row = sqlx::query_as::<_, TokenListRow>(&sql)
            .bind(mint)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row)
    }

    /// Resolve display symbols for a specific set of mints (`mint = ANY($1)`), so a
    /// caller that only needs a handful of symbols (e.g. a paper run's positions)
    /// doesn't `SELECT *` the whole growing `tokens` table into memory. Returns a
    /// `mint -> symbol` map; mints with no row are simply absent.
    pub async fn find_symbols_for(
        &self,
        mints: &[String],
    ) -> anyhow::Result<std::collections::HashMap<String, String>> {
        if mints.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT mint_address, symbol FROM tokens WHERE mint_address = ANY($1)")
                .bind(mints)
                .fetch_all(&self.pool)
                .await?;

        Ok(rows.into_iter().collect())
    }
}

// ===========================================================================
// SQL-vs-in-RAM parity tests (the "full parity in SQL" guarantee)
//
// The token-list filter/sort grammar has two backends: the in-RAM
// `TokenQuery::matches`/`sort_refs` (lab) and the SQL `build_where_and_order`
// (live). These tests prove they return IDENTICAL ordered mint lists over a real
// Postgres fixture, across a matrix of filter/sort permutations. Runs automatically
// whenever `DATABASE_URL` is set (self-skips, staying green, when it is not) — no
// `--ignored` opt-in, so the parity guarantee is actually enforced in any DB-backed run.
// ===========================================================================
#[cfg(test)]
mod parity_tests {
    use super::*;
    use crate::api::handlers::tokens::{
        build_where_and_order, PaginationParams, TokenQuery, TokenSummary,
    };
    use sqlx::postgres::PgPoolOptions;

    async fn test_pool() -> Option<PgPool> {
        let url = std::env::var("DATABASE_URL").ok()?;
        PgPoolOptions::new().max_connections(2).connect(&url).await.ok()
    }

    fn q(json: serde_json::Value) -> TokenQuery {
        let p: PaginationParams = serde_json::from_value(json).expect("params");
        TokenQuery::from_params(&p)
    }

    /// Insert one fixture token (+ tokens_info) with the fields the filters touch.
    #[allow(clippy::too_many_arguments)]
    async fn seed(
        pool: &PgPool,
        mint: &str,
        symbol: &str,
        created_at: DateTime<Utc>,
        initial_buy_lamports: i64,
        initial_supply: i64,
        buy_ix: serde_json::Value,
        ath_price: Option<f64>,
        current_price: Option<f64>,
        volume: f64,
        trade_count: i64,
        last_trade_at: Option<DateTime<Utc>>,
        is_dead: bool,
        is_migrated: bool,
    ) {
        sqlx::query(
            "INSERT INTO tokens (mint_address, creator_wallet, name, symbol, \
                initial_supply_token, initial_buy_lamports, initial_buy_instruction, \
                is_mayhem_mode, is_cashback_enabled, ix_labels, creation_tx_signature, created_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,false,false,'[\"buy\"]'::jsonb,$8,$9) \
             ON CONFLICT (mint_address) DO NOTHING",
        )
        .bind(mint)
        .bind(format!("creator-{mint}"))
        .bind(format!("name-{symbol}"))
        .bind(symbol)
        .bind(initial_supply)
        .bind(initial_buy_lamports)
        .bind(Json(buy_ix))
        .bind(format!("tx-{mint}"))
        .bind(created_at)
        .execute(pool)
        .await
        .expect("insert token");

        sqlx::query(
            "INSERT INTO tokens_info (mint_address, current_price, ath_price, volume_sol, \
                trade_count, last_trade_at, is_dead, is_migrated) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8) \
             ON CONFLICT (mint_address) DO UPDATE SET \
                current_price=$2, ath_price=$3, volume_sol=$4, trade_count=$5, \
                last_trade_at=$6, is_dead=$7, is_migrated=$8",
        )
        .bind(mint)
        .bind(current_price)
        .bind(ath_price)
        .bind(volume)
        .bind(trade_count)
        .bind(last_trade_at)
        .bind(is_dead)
        .bind(is_migrated)
        .execute(pool)
        .await
        .expect("insert info");
    }

    async fn cleanup(pool: &PgPool, mints: &[&str]) {
        for m in mints {
            let _ = sqlx::query("DELETE FROM tokens_info WHERE mint_address = $1").bind(m).execute(pool).await;
            let _ = sqlx::query("DELETE FROM tokens WHERE mint_address = $1").bind(m).execute(pool).await;
        }
    }

    /// In-RAM engine result: full universe → TokenSummary → matches → sort → mints.
    async fn in_ram_mints(repo: &TokenRepo, query: &TokenQuery, mints: &[&str], now: DateTime<Utc>) -> Vec<String> {
        // Pull just the fixture rows so the assertion is deterministic regardless of
        // other rows already in the local DB.
        let rows = repo.find_list_rows_for_mints(&mints.iter().map(|s| s.to_string()).collect::<Vec<_>>())
            .await
            .expect("rows");
        let mut summaries: Vec<TokenSummary> = rows.into_iter().map(TokenSummary::from).collect();
        // Mirror `TokenListSnapshot::newest_first`: the in-RAM engine always sees the
        // snapshot pre-sorted newest-first with a `mint_address` DESC tiebreak, so the
        // no-sort default order is `created_at DESC, mint_address DESC` (the SQL
        // default). Reproduce that here before filtering.
        summaries.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| b.mint_address.cmp(&a.mint_address))
        });
        let mut refs: Vec<&TokenSummary> = summaries.iter().filter(|t| query.matches_public(t, now)).collect();
        query.sort_refs_public(&mut refs);
        refs.into_iter().map(|t| t.mint_address.clone()).collect()
    }

    /// SQL engine result, restricted to the fixture mints for the same determinism.
    async fn sql_mints(repo: &TokenRepo, query: &TokenQuery, fixture: &[&str], now: DateTime<Utc>) -> Vec<String> {
        let mut built = build_where_and_order(query, now);
        // AND a fixture-scope guard so we compare only the seeded rows.
        let ph = built.args.len() + 1;
        built.where_sql = format!("({}) AND t.mint_address = ANY(${ph})", built.where_sql);
        built.args.push(SqlArg::StrArray(fixture.iter().map(|s| s.to_string()).collect()));
        let rows = repo
            .find_list_page(&built.where_sql, &built.order_sql, &built.args, 10_000, 0)
            .await
            .expect("page");
        rows.into_iter().map(|r| r.mint_address).collect()
    }

    // NOT `#[ignore]`: this SSOT guard must actually run. It self-skips (returns Ok)
    // when `DATABASE_URL` is unset, so a keyless CI stays green, but the moment a
    // local Postgres is configured the SQL-vs-in-RAM parity is enforced automatically
    // — no `--ignored` opt-in that everyone forgets to pass.
    #[tokio::test]
    async fn sql_and_in_ram_agree_across_filter_sort_matrix() {
        let Some(pool) = test_pool().await else {
            eprintln!("skipping token-list parity: no DATABASE_URL");
            return;
        };
        let repo = TokenRepo::new(pool.clone());
        let now = Utc::now();
        let base = now - chrono::Duration::days(2);
        let mints = ["PARITYa", "PARITYb", "PARITYc", "PARITYd", "PARITYe", "PARITYf", "PARITYg"];

        // Diverse fixture: prices, volumes, deadness, missing metrics (LEFT JOIN
        // nulls), buy-ix args, migrated flag.
        seed(&pool, "PARITYa", "AAA", base + chrono::Duration::minutes(10), 1_000_000_000, 1_000_000,
            serde_json::json!({"token_amount": 500, "max_cost_lamports": 2_000_000_000u64}),
            Some(0.002), Some(0.001), 12.5, 40, Some(now - chrono::Duration::hours(3)), false, false).await;
        seed(&pool, "PARITYb", "BBB", base + chrono::Duration::minutes(20), 2_000_000_000, 2_000_000,
            serde_json::json!({"token_amount": 900, "max_cost_lamports": 5_000_000_000u64}),
            Some(0.010), Some(0.008), 100.0, 5, Some(now - chrono::Duration::hours(3)), false, true).await;
        seed(&pool, "PARITYc", "CCC", base + chrono::Duration::minutes(30), 500_000_000, 1_000_000,
            serde_json::json!({}), None, None, 0.0, 0, None, true, false).await; // no metrics
        seed(&pool, "PARITYd", "ABAB", base + chrono::Duration::minutes(40), 3_000_000_000, 3_000_000,
            serde_json::json!({"token_amount": 100}), Some(0.05), Some(0.0), 3.3, 200,
            Some(now - chrono::Duration::hours(3)), false, false).await;
        seed(&pool, "PARITYe", "AAB", base + chrono::Duration::minutes(50), 1_500_000_000, 1_000_000,
            serde_json::json!({"token_amount": 700}), Some(0.001), Some(0.0009), 7.0, 40,
            Some(now - chrono::Duration::hours(3)), false, false).await;
        // PARITYf/g deliberately SHARE a created_at so the default-order case exercises
        // the `mint_address DESC` tiebreak (g must precede f). Guards against the
        // engines diverging on same-created_at pairs (the Phase-1 default-order bug).
        seed(&pool, "PARITYf", "TIEF", base + chrono::Duration::minutes(25), 1_000_000_000, 1_000_000,
            serde_json::json!({"token_amount": 300}), Some(0.003), Some(0.002), 9.0, 40,
            Some(now - chrono::Duration::hours(3)), false, false).await;
        seed(&pool, "PARITYg", "TIEG", base + chrono::Duration::minutes(25), 1_000_000_000, 1_000_000,
            serde_json::json!({"token_amount": 300}), Some(0.003), Some(0.002), 9.0, 40,
            Some(now - chrono::Duration::hours(3)), false, false).await;

        // (label, query-json) matrix — each must agree between the two engines.
        let cases: Vec<(&str, serde_json::Value)> = vec![
            ("no filter default order", serde_json::json!({})),
            ("sort trade_count desc", serde_json::json!({"sort": "trade_count:desc"})),
            ("sort volume asc", serde_json::json!({"sort": "volume:asc"})),
            ("sort current_price desc (nulls last)", serde_json::json!({"sort": "current_price:desc"})),
            ("multi-key mcap desc, symbol asc", serde_json::json!({"sort": "market_cap:desc,symbol:asc"})),
            ("text symbol AA", serde_json::json!({"f_symbol": "AA"})),
            ("dead only", serde_json::json!({"f_dead": "yes"})),
            ("migrated only", serde_json::json!({"f_migrated": "yes"})),
            ("volume range", serde_json::json!({"f_volume_min": "5", "f_volume_max": "50"})),
            ("ath_price present min", serde_json::json!({"f_ath_price_min": "0.001"})),
            ("trades >= 40", serde_json::json!({"cf": "trade_count:>=40"})),
            ("global search AB", serde_json::json!({"search": "ab"})),
            ("token_amount range (buy-ix)", serde_json::json!({"f_token_amount_min": "200", "f_token_amount_max": "800"})),
            ("filter + sort combo", serde_json::json!({"f_ath_price_min": "0.001", "sort": "volume:desc"})),
        ];

        for (label, json) in cases {
            let query = q(json);
            let ram = in_ram_mints(&repo, &query, &mints, now).await;
            let sql = sql_mints(&repo, &query, &mints, now).await;
            assert_eq!(sql, ram, "PARITY MISMATCH for case: {label}\n  sql={sql:?}\n  ram={ram:?}");
        }

        // Explicit default-order tiebreak assertion: both engines agreeing isn't
        // enough (they could agree on a WRONG order). PARITYg/f share a created_at,
        // so `mint_address DESC` must place g immediately before f in the default view.
        let default_order = sql_mints(&repo, &q(serde_json::json!({})), &mints, now).await;
        let gi = default_order.iter().position(|m| m == "PARITYg").expect("g present");
        let fi = default_order.iter().position(|m| m == "PARITYf").expect("f present");
        assert_eq!(gi + 1, fi, "same-created_at pair must order g→f by mint DESC: {default_order:?}");

        cleanup(&pool, &mints).await;
    }
}
