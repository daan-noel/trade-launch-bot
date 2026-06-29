use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{types::Json, PgPool};
use uuid::Uuid;

use crate::models::token::Token;

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
    initial_buy_sol: Option<i64>,
    initial_buy_instruction: Option<Json<Value>>,
    cu_limit: Option<i64>,
    cu_price: Option<i64>,
    is_mayhem_mode: bool,
    is_cashback_enabled: bool,
    ix_labels: Json<Value>,
    creation_tx_signature: String,
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
    pub volume: Option<f64>,
    pub market_cap: Option<f64>,
    pub trade_count: Option<i64>,
    pub last_trade_at: Option<DateTime<Utc>>,
    pub current_price: Option<f64>,
    pub is_dead: Option<bool>,
    pub is_migrated: Option<bool>,
    pub last_synced_at: Option<DateTime<Utc>>,
    pub lifetime_secs: Option<i64>,
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
            initial_buy_sol: r.initial_buy_sol.map(lamports_to_sol),
            initial_buy_instruction: r.initial_buy_instruction.map(|v| v.0),
            cu_limit: r.cu_limit.map(|v| v as u64),
            cu_price: r.cu_price.map(|v| v as u64),
            is_mayhem_mode: r.is_mayhem_mode,
            is_cashback_enabled: r.is_cashback_enabled,
            instruction_labels: r.ix_labels.0,
            creation_tx_signature: r.creation_tx_signature,
            created_at: r.created_at,
        }
    }
}

// ---------------------------------------------------------------------------
// SOL ↔ lamports — `initial_buy_sol` is human SOL (f64) in the model but stored
// as exact lamports (BIGINT) in the column, mirroring `trades.sol_amount`.
// ---------------------------------------------------------------------------

/// Human SOL (f64) → lamports (i64).
fn sol_to_lamports(sol: f64) -> i64 {
    (sol * 1_000_000_000.0).round() as i64
}

/// Lamports (i64) → human SOL (f64).
fn lamports_to_sol(lamports: i64) -> f64 {
    lamports as f64 / 1_000_000_000.0
}

// ---------------------------------------------------------------------------
// Repo
// ---------------------------------------------------------------------------

/// Columns selected for `TokenDbRow`, in struct order. Explicit (not `SELECT *`)
/// so a future column added to `tokens` isn't pulled into every read, and the
/// query's wire contract is decoupled from the physical table layout.
const TOKEN_COLS: &str = "mint_address, creator_wallet, name, symbol, token_program_id, \
    bonding_curve_address, initial_supply_token, initial_buy_sol, initial_buy_instruction, \
    cu_limit, cu_price, is_mayhem_mode, is_cashback_enabled, ix_labels, creation_tx_signature, \
    created_at";

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
              bonding_curve_address, initial_supply_token, initial_buy_sol, initial_buy_instruction, \
              cu_limit, cu_price, is_mayhem_mode, is_cashback_enabled, ix_labels, \
              creation_tx_signature, created_at) ",
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
                    bonding_curve_address, initial_supply_token, initial_buy_sol, initial_buy_instruction, cu_limit, cu_price, is_mayhem_mode, is_cashback_enabled, ix_labels,
                    creation_tx_signature, created_at)
              VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)
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
                    bonding_curve_address, initial_supply_token, initial_buy_sol, initial_buy_instruction, cu_limit, cu_price, is_mayhem_mode, is_cashback_enabled, ix_labels,
                    creation_tx_signature, created_at)
              VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)
            ON CONFLICT (mint_address) DO UPDATE SET
                creator_wallet = EXCLUDED.creator_wallet,
                name = EXCLUDED.name,
                symbol = EXCLUDED.symbol,
                token_program_id = EXCLUDED.token_program_id,
                bonding_curve_address = EXCLUDED.bonding_curve_address,
                initial_supply_token = EXCLUDED.initial_supply_token,
                initial_buy_sol = EXCLUDED.initial_buy_sol,
                initial_buy_instruction = EXCLUDED.initial_buy_instruction,
                cu_limit = EXCLUDED.cu_limit,
                cu_price = EXCLUDED.cu_price,
                is_mayhem_mode = EXCLUDED.is_mayhem_mode,
                is_cashback_enabled = EXCLUDED.is_cashback_enabled,
                ix_labels = EXCLUDED.ix_labels,
                creation_tx_signature = EXCLUDED.creation_tx_signature,
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
    /// See [`crate::config::constants::SEED_TOKEN_LIMIT`] /
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
        let rows = sqlx::query_as::<_, TokenListRow>(
            r#"
            SELECT t.mint_address, t.creator_wallet, t.name, t.symbol,
                   t.bonding_curve_address,
                   t.initial_supply_token,
                   t.initial_buy_sol::float8 / 1e9 AS initial_buy_sol, t.initial_buy_instruction,
                   t.cu_limit, t.cu_price, t.is_mayhem_mode, t.is_cashback_enabled,
                   t.ix_labels, t.creation_tx_signature, t.created_at,
                   i.ath_price, i.ath_timestamp, i.volume,
                   (i.current_price * t.initial_supply_token) AS market_cap, i.trade_count,
                   i.last_trade_at, i.current_price, i.is_dead, i.is_migrated,
                   (SELECT MAX(s.last_synced_at) FROM token_sync_state s
                      WHERE s.mint_address = t.mint_address) AS last_synced_at,
                   i.lifetime_secs
              FROM tokens t
              LEFT JOIN tokens_info i ON i.mint_address = t.mint_address
             WHERE t.created_at >= $1
             ORDER BY t.created_at DESC
             LIMIT $2
            "#,
        )
        .bind(since)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
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
        let rows = sqlx::query_as::<_, TokenListRow>(
            r#"
            SELECT t.mint_address, t.creator_wallet, t.name, t.symbol,
                   t.bonding_curve_address,
                   t.initial_supply_token,
                   t.initial_buy_sol::float8 / 1e9 AS initial_buy_sol, t.initial_buy_instruction,
                   t.cu_limit, t.cu_price, t.is_mayhem_mode, t.is_cashback_enabled,
                   t.ix_labels, t.creation_tx_signature, t.created_at,
                   i.ath_price, i.ath_timestamp, i.volume,
                   (i.current_price * t.initial_supply_token) AS market_cap, i.trade_count,
                   i.last_trade_at, i.current_price, i.is_dead, i.is_migrated,
                   (SELECT MAX(s.last_synced_at) FROM token_sync_state s
                      WHERE s.mint_address = t.mint_address) AS last_synced_at,
                   i.lifetime_secs
              FROM tokens t
              LEFT JOIN tokens_info i ON i.mint_address = t.mint_address
             WHERE t.mint_address = ANY($1)
            "#,
        )
        .bind(mints)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Single-mint detail lookup with `tokens_info` stats (LEFT JOIN). Used by
    /// `GET /api/tokens/:mint` slow path so the detail modal shows real stats for
    /// tokens that were evicted from the live cache.
    pub async fn find_list_row_by_mint(&self, mint: &str) -> anyhow::Result<Option<TokenListRow>> {
        let row = sqlx::query_as::<_, TokenListRow>(
            r#"
            SELECT t.mint_address, t.creator_wallet, t.name, t.symbol,
                   t.bonding_curve_address,
                   t.initial_supply_token,
                   t.initial_buy_sol::float8 / 1e9 AS initial_buy_sol, t.initial_buy_instruction,
                   t.cu_limit, t.cu_price, t.is_mayhem_mode, t.is_cashback_enabled,
                   t.ix_labels, t.creation_tx_signature, t.created_at,
                   i.ath_price, i.ath_timestamp, i.volume,
                   (i.current_price * t.initial_supply_token) AS market_cap, i.trade_count,
                   i.last_trade_at, i.current_price, i.is_dead, i.is_migrated,
                   (SELECT MAX(s.last_synced_at) FROM token_sync_state s
                      WHERE s.mint_address = t.mint_address) AS last_synced_at,
                   i.lifetime_secs
              FROM tokens t
              LEFT JOIN tokens_info i ON i.mint_address = t.mint_address
             WHERE t.mint_address = $1
            "#,
        )
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
