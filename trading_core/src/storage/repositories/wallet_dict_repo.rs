use std::collections::HashMap;

use sqlx::PgPool;

/// Repository for the `wallet_dict` interning table: a bijection between a wallet
/// `address` (TEXT, UNIQUE) and a compact `id` (Postgres `IDENTITY INTEGER` → `i32`
/// in Rust), so high-volume rows can reference wallets by 4 bytes instead of a
/// base58 string.
pub struct WalletDictRepo {
    pool: PgPool,
}

impl WalletDictRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Intern a single address, returning its stable `id`. Idempotent: the no-op
    /// `DO UPDATE SET address = EXCLUDED.address` exists solely so `RETURNING id`
    /// yields a row on conflict (a bare `DO NOTHING` returns no row for an existing
    /// address).
    pub async fn intern(&self, address: &str) -> anyhow::Result<i32> {
        let id: i32 = sqlx::query_scalar(
            r#"
            INSERT INTO wallet_dict (address)
            VALUES ($1)
            ON CONFLICT (address) DO UPDATE SET address = EXCLUDED.address
            RETURNING id
            "#,
        )
        .bind(address)
        .fetch_one(&self.pool)
        .await?;

        Ok(id)
    }

    /// Intern a batch of addresses in as few round-trips as possible, returning an
    /// `address -> id` map. Input is de-duplicated first; empty input is a no-op.
    /// Chunked well under Postgres' 65535 bind-param ceiling (1 bind/row).
    pub async fn intern_many(&self, addresses: &[String]) -> anyhow::Result<HashMap<String, i32>> {
        /// Rows per round-trip. 1 bind per row → far under the 65535 ceiling, leaving
        /// generous headroom.
        const CHUNK: usize = 10_000;

        let mut out: HashMap<String, i32> = HashMap::new();
        if addresses.is_empty() {
            return Ok(out);
        }

        // De-dup, preserving only distinct addresses (the map dedups on collect too,
        // but this also shrinks the bind set we send to Postgres).
        let mut unique: Vec<&String> = Vec::with_capacity(addresses.len());
        {
            let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
            for a in addresses {
                if seen.insert(a.as_str()) {
                    unique.push(a);
                }
            }
        }

        for chunk in unique.chunks(CHUNK) {
            let mut qb = sqlx::QueryBuilder::new("INSERT INTO wallet_dict (address) ");
            qb.push_values(chunk, |mut b, addr| {
                b.push_bind(*addr);
            });
            qb.push(" ON CONFLICT (address) DO UPDATE SET address = EXCLUDED.address");
            qb.push(" RETURNING id, address");

            let rows: Vec<(i32, String)> = qb.build_query_as().fetch_all(&self.pool).await?;
            out.reserve(rows.len());
            for (id, address) in rows {
                out.insert(address, id);
            }
        }

        Ok(out)
    }

    /// Reverse lookup for a set of ids → `id -> address` map (`id = ANY($1)`).
    /// Empty input is a no-op; ids with no row are simply absent.
    pub async fn addresses_for(&self, ids: &[i32]) -> anyhow::Result<HashMap<i32, String>> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let rows: Vec<(i32, String)> =
            sqlx::query_as("SELECT id, address FROM wallet_dict WHERE id = ANY($1)")
                .bind(ids)
                .fetch_all(&self.pool)
                .await?;

        Ok(rows.into_iter().collect())
    }

    /// Look up the `id` for a single address without interning it.
    pub async fn id_for(&self, address: &str) -> anyhow::Result<Option<i32>> {
        let id: Option<i32> = sqlx::query_scalar("SELECT id FROM wallet_dict WHERE address = $1")
            .bind(address)
            .fetch_optional(&self.pool)
            .await?;

        Ok(id)
    }
}
