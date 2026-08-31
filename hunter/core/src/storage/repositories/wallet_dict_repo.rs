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

/// Ids already written as `is_proxy` by THIS process. A router's proxy PDA is the
/// busiest account on the tape — the OKX one carries ~5k legs a day — and marking
/// it is idempotent, so without this every one of those legs would `UPDATE` a row
/// that already says what we want, leaving a dead tuple behind on the hottest write
/// path (the exact churn `intern` was rewritten to avoid). The set is the count of
/// distinct proxies ever seen, not of trades: 930 addresses across the whole
/// dictionary, so it needs no cap.
static PROXY_MARKED: LazyLock<DashMap<i32, ()>> = LazyLock::new(DashMap::new);

/// Drop the process-wide interning cache (admin reseed). Cold misses re-hit PG.
pub fn clear_wallet_id_cache() {
    WALLET_ID_CACHE.clear();
    PROXY_MARKED.clear();
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

    /// Record that an address is a PROGRAM, not a trader.
    ///
    /// The caller's evidence is a trade whose venue-side actor put no signature on
    /// its own transaction, which can only happen for a PDA signing a CPI — an
    /// aggregator routing a customer's swap through an account of its own. That
    /// makes it a fact about the ADDRESS rather than about the trade, so it lives
    /// here, where a per-wallet study can exclude the address's whole history and
    /// not just the legs ingested since the flag existed.
    ///
    /// Idempotent and near-free on repeat: a process-local set means the second and
    /// every later sighting of the same proxy costs no round-trip at all, and the
    /// `NOT is_proxy` predicate means a cold process re-marking a known proxy
    /// updates zero rows instead of writing a dead tuple.
    pub async fn mark_proxy(&self, id: i32) -> anyhow::Result<()> {
        self.mark_proxies(std::slice::from_ref(&id)).await
    }

    /// Batch [`mark_proxy`](Self::mark_proxy) — one statement for a whole ingest
    /// flush. Ids already marked by this process are filtered out first, so a
    /// steady-state flush full of one router's legs issues no query.
    pub async fn mark_proxies(&self, ids: &[i32]) -> anyhow::Result<()> {
        let fresh: Vec<i32> = ids
            .iter()
            .copied()
            .filter(|id| !PROXY_MARKED.contains_key(id))
            .collect();
        if fresh.is_empty() {
            return Ok(());
        }
        sqlx::query("UPDATE wallet_dict SET is_proxy = TRUE WHERE id = ANY($1) AND NOT is_proxy")
            .bind(&fresh)
            .execute(&self.pool)
            .await?;
        // Only after the write lands — a failed statement must be retried by the
        // next flush, not swallowed by an optimistic cache entry.
        for id in fresh {
            PROXY_MARKED.insert(id, ());
        }
        Ok(())
    }

    /// The ids in `wallet_dict` that are flagged `is_proxy` — routers' proxy PDAs
    /// and other program accounts that no keypair can sign for. A wallet-level
    /// study subtracts these before it counts traders or ranks them; see migration
    /// `0015_backfill_known_proxy_wallets.sql` for why the set is not empty on a
    /// database that predates the flag.
    pub async fn proxy_ids(&self) -> anyhow::Result<Vec<i32>> {
        let ids: Vec<i32> = sqlx::query_scalar("SELECT id FROM wallet_dict WHERE is_proxy")
            .fetch_all(&self.pool)
            .await?;
        Ok(ids)
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
