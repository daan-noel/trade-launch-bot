use sqlx::PgPool;

/// One durable PumpSwap pool-facts row (base58 pubkeys as TEXT). Field-for-field
/// the persisted form of the executor's `pump_trader::AmmPoolFacts` transport DTO
/// — kept in `trading_core` because the executor is a standalone drop-in with no
/// DB knowledge, and `trading_core` must not depend on `pump-trader`. `live` (which
/// depends on both) maps between the two; keep the two field lists in lockstep.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AmmPoolFactRow {
    pub mint_address: String,
    pub pool: String,
    pub base_mint: String,
    pub quote_mint: String,
    pub base_token_program: String,
    pub pool_base_token_account: String,
    pub pool_quote_token_account: String,
    pub coin_creator: String,
    pub coin_creator_vault_ata: String,
    pub coin_creator_vault_authority: String,
    pub is_cashback_coin: bool,
    pub fee_share_marker: Option<String>,
    pub needs_pool_v2: bool,
}

/// Column list shared by the upsert and the selects, so the bind order can never
/// drift from the read order. `updated_at` is written `now()` on every upsert and
/// is not part of this list (it is the 13th VALUES slot / not selected back).
const FACT_COLS: &str = "mint_address, pool, base_mint, quote_mint, base_token_program, \
    pool_base_token_account, pool_quote_token_account, coin_creator, coin_creator_vault_ata, \
    coin_creator_vault_authority, is_cashback_coin, fee_share_marker, needs_pool_v2";

/// Repo over `amm_pool_facts` (migration 0011). Low write volume — one upsert per
/// newly observed migrated pool, never on the ingest flush path.
#[derive(Clone)]
pub struct AmmPoolFactsRepo {
    pool: PgPool,
}

impl AmmPoolFactsRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Upsert one pool's facts. Pubkeys/flags are overwritten wholesale on
    /// conflict — the trader re-learns the freshest layout (2006 self-heal, cold
    /// re-read), and the latest observation is always authoritative.
    pub async fn upsert(&self, f: &AmmPoolFactRow) -> anyhow::Result<()> {
        sqlx::query(&format!(
            "INSERT INTO amm_pool_facts ({FACT_COLS}, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, now()) \
             ON CONFLICT (mint_address) DO UPDATE SET \
                 pool = EXCLUDED.pool, \
                 base_mint = EXCLUDED.base_mint, \
                 quote_mint = EXCLUDED.quote_mint, \
                 base_token_program = EXCLUDED.base_token_program, \
                 pool_base_token_account = EXCLUDED.pool_base_token_account, \
                 pool_quote_token_account = EXCLUDED.pool_quote_token_account, \
                 coin_creator = EXCLUDED.coin_creator, \
                 coin_creator_vault_ata = EXCLUDED.coin_creator_vault_ata, \
                 coin_creator_vault_authority = EXCLUDED.coin_creator_vault_authority, \
                 is_cashback_coin = EXCLUDED.is_cashback_coin, \
                 fee_share_marker = EXCLUDED.fee_share_marker, \
                 needs_pool_v2 = EXCLUDED.needs_pool_v2, \
                 updated_at = now()"
        ))
        .bind(&f.mint_address)
        .bind(&f.pool)
        .bind(&f.base_mint)
        .bind(&f.quote_mint)
        .bind(&f.base_token_program)
        .bind(&f.pool_base_token_account)
        .bind(&f.pool_quote_token_account)
        .bind(&f.coin_creator)
        .bind(&f.coin_creator_vault_ata)
        .bind(&f.coin_creator_vault_authority)
        .bind(f.is_cashback_coin)
        .bind(&f.fee_share_marker)
        .bind(f.needs_pool_v2)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Facts for the given mints (batched `= ANY($1)`, PK-indexed). Mints with no
    /// row are simply absent — the caller falls back to the cold RPC path.
    pub async fn find_for(&self, mints: &[String]) -> anyhow::Result<Vec<AmmPoolFactRow>> {
        /// Mints per round-trip; keeps each `= ANY($1)` array PK-index-friendly.
        const FACT_CHUNK: usize = 1000;
        if mints.is_empty() {
            return Ok(Vec::new());
        }
        let mut out = Vec::with_capacity(mints.len());
        for chunk in mints.chunks(FACT_CHUNK) {
            let batch = sqlx::query_as::<_, AmmPoolFactRow>(&format!(
                "SELECT {FACT_COLS} FROM amm_pool_facts WHERE mint_address = ANY($1)"
            ))
            .bind(chunk)
            .fetch_all(&self.pool)
            .await?;
            out.extend(batch);
        }
        Ok(out)
    }

    /// Every mint already persisted — the background persist loop preloads this
    /// once so it only upserts genuinely new pools (no re-write of existing rows).
    pub async fn all_mints(&self) -> anyhow::Result<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as("SELECT mint_address FROM amm_pool_facts")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(|(m,)| m).collect())
    }
}
