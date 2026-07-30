use std::collections::HashMap;
use std::sync::LazyLock;

use dashmap::DashMap;
use sqlx::PgPool;

/// Process-wide `address -> id` cache shared by every [`WalletDictRepo`] handle
/// (each is a thin per-call wrapper over the same pool, so an instance-local cache
/// would never hit). `wallet_dict.id` is an `IDENTITY` value that is assigned once
/// and **never changes**, so a cached entry can never go stale — no invalidation is
/// needed. Bounded by [`WALLET_ID_CACHE_CAP`] so the RAM-constrained live box's
/// budget holds across the full wallet population; once full it simply stops
/// admitting new entries (the hottest wallets are already resident), which keeps the
/// hot ingest/confirm paths off Postgres for interned addresses.
static WALLET_ID_CACHE: LazyLock<DashMap<String, i32>> = LazyLock::new(DashMap::new);

/// Upper bound on cached `address -> id` entries (~48 B/entry → a few MB at the cap).
const WALLET_ID_CACHE_CAP: usize = 200_000;

/// Drop the process-wide interning cache (admin reseed). Cold misses re-hit PG.
pub fn clear_wallet_id_cache() {
    WALLET_ID_CACHE.clear();
}

/// Cache a freshly-resolved `(address, id)` if there's headroom (see cap rationale).
fn cache_put(address: &str, id: i32) {
    // Racy len check is fine — the cap is a soft RAM guard, not an exact bound.
    if WALLET_ID_CACHE.len() < WALLET_ID_CACHE_CAP {
        WALLET_ID_CACHE.entry(address.to_string()).or_insert(id);
    }
}

/// Repository for the `wallet_dict` interning table: a bijection between a wallet
/// `address` (TEXT, UNIQUE) and a compact `id` (Postgres `IDENTITY INTEGER` → `i32`
/// in Rust), so high-volume rows can reference wallets by 4 bytes instead of a
/// base58 string. Backed by a shared in-process id cache ([`WALLET_ID_CACHE`]) so a
/// re-seen address costs zero round-trips on the hot ingest + sell-confirm paths.
pub struct WalletDictRepo {
    pool: PgPool,
}

impl WalletDictRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Intern a single address, returning its stable `id`. Cache-first: a hit costs
    /// no query; a miss does a SELECT fast path, then `INSERT … ON CONFLICT DO
    /// NOTHING RETURNING id`, then a final SELECT only if a concurrent inserter won
    /// the race. This replaces the old `ON CONFLICT DO UPDATE SET address =
    /// EXCLUDED.address`, which wrote a **dead tuple on every re-intern of an existing
    /// address** (M3) — pure WAL/bloat churn on the hottest write path.
    pub async fn intern(&self, address: &str) -> anyhow::Result<i32> {
        if let Some(id) = WALLET_ID_CACHE.get(address) {
            return Ok(*id);
        }
        // SELECT fast path — the common case is an already-interned wallet, and a bare
        // SELECT never touches a tuple (no dead rows, no WAL).
        if let Some(id) = self.id_for_uncached(address).await? {
            cache_put(address, id);
            return Ok(id);
        }
        // First sighting: insert without churning on conflict. `RETURNING id` yields a
        // row only when THIS statement inserted; a concurrent inserter → no row → the
        // final SELECT resolves it.
        let inserted: Option<i32> = sqlx::query_scalar(
            "INSERT INTO wallet_dict (address) VALUES ($1) \
             ON CONFLICT (address) DO NOTHING RETURNING id",
        )
        .bind(address)
        .fetch_optional(&self.pool)
        .await?;
        let id = match inserted {
            Some(id) => id,
            None => self
                .id_for_uncached(address)
                .await?
                .ok_or_else(|| anyhow::anyhow!("wallet_dict intern raced with no resolvable id"))?,
        };
        cache_put(address, id);
        Ok(id)
    }

    /// Intern a batch of addresses in as few round-trips as possible, returning an
    /// `address -> id` map. Input is de-duplicated first; empty input is a no-op.
    /// Chunked well under Postgres' 65535 bind-param ceiling (1 bind/row). Serves
    /// cache hits without a query and warms the cache with every resolved id.
    pub async fn intern_many(&self, addresses: &[String]) -> anyhow::Result<HashMap<String, i32>> {
        /// Rows per round-trip. 1 bind per row → far under the 65535 ceiling, leaving
        /// generous headroom.
        const CHUNK: usize = 10_000;

        let mut out: HashMap<String, i32> = HashMap::new();
        if addresses.is_empty() {
            return Ok(out);
        }

        // De-dup, and serve cache hits without hitting Postgres. Only the addresses
        // still unresolved go into the bind set.
        let mut unique: Vec<&String> = Vec::with_capacity(addresses.len());
        {
            let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
            for a in addresses {
                if !seen.insert(a.as_str()) {
                    continue;
                }
                if let Some(id) = WALLET_ID_CACHE.get(a.as_str()) {
                    out.insert(a.clone(), *id);
                } else {
                    unique.push(a);
                }
            }
        }

        for chunk in unique.chunks(CHUNK) {
            // Idempotent, dead-tuple-free: `DO NOTHING` inserts absent rows; the
            // `RETURNING` covers only those, so the union with the outer SELECT below
            // resolves the rest. To get every id in one shot we SELECT the whole chunk
            // afterward (all rows now exist). Simpler + no bloat vs the old
            // conflict-update RETURNING.
            let mut qb = sqlx::QueryBuilder::new("INSERT INTO wallet_dict (address) ");
            qb.push_values(chunk, |mut b, addr| {
                b.push_bind(*addr);
            });
            qb.push(" ON CONFLICT (address) DO NOTHING");
            qb.build().execute(&self.pool).await?;

            let chunk_owned: Vec<String> = chunk.iter().map(|s| (*s).clone()).collect();
            let rows: Vec<(i32, String)> =
                sqlx::query_as("SELECT id, address FROM wallet_dict WHERE address = ANY($1)")
                    .bind(&chunk_owned)
                    .fetch_all(&self.pool)
                    .await?;
            out.reserve(rows.len());
            for (id, address) in rows {
                cache_put(&address, id);
                out.insert(address, id);
            }
        }

        Ok(out)
    }

    /// Look up the `id` for a single address without interning it. Cache-first (M4):
    /// the confirm loop calls this per query, so a resident id collapses the old
    /// per-call round-trip to zero.
    pub async fn id_for(&self, address: &str) -> anyhow::Result<Option<i32>> {
        if let Some(id) = WALLET_ID_CACHE.get(address) {
            return Ok(Some(*id));
        }
        let id = self.id_for_uncached(address).await?;
        if let Some(id) = id {
            cache_put(address, id);
        }
        Ok(id)
    }

    /// The bare `SELECT id` round-trip, cache-bypassing — the shared miss path for
    /// [`id_for`](Self::id_for) and [`intern`](Self::intern).
    async fn id_for_uncached(&self, address: &str) -> anyhow::Result<Option<i32>> {
        let id: Option<i32> = sqlx::query_scalar("SELECT id FROM wallet_dict WHERE address = $1")
            .bind(address)
            .fetch_optional(&self.pool)
            .await?;
        Ok(id)
    }
}
