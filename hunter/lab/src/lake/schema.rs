//! Canonical **lake column names** — the single source both the writer
//! ([`super::export`]) and the DuckDB reader ([`super::duck`]) reference.
//!
//! The two halves are coupled by column *name*: the writer tags each Parquet column
//! with a name, and the reader `SELECT`s those columns by name (DuckDB is name-based,
//! so a writer column *reorder* is harmless, but a *rename* on one side without the
//! other silently stops matching → a runtime "column not found" or null-fill). Naming
//! every column once here makes a rename a single edit and pins the writer's schema
//! order to a test, so a same-typed builder swap in `finish()` (e.g. `slot` ↔
//! `block_time`, both `Int64`) can't slip through Arrow's count/type check unnoticed.

// --- trades day-file columns (physical write order) ---
pub const T_MINT: &str = "mint";
pub const T_IS_BUY: &str = "is_buy";
pub const T_SOL_AMOUNT: &str = "sol_amount";
pub const T_TOKEN_AMOUNT: &str = "token_amount";
pub const T_PRICE: &str = "price";
pub const T_SLOT: &str = "slot";
pub const T_BLOCK_TIME: &str = "block_time";
pub const T_LEG_INDEX: &str = "leg_index";
pub const T_VSOL: &str = "vsol";
pub const T_VTOK: &str = "vtok";
pub const T_VENUE: &str = "venue";
pub const T_TX_INDEX: &str = "tx_index";
pub const T_TX_SIGNATURE: &str = "tx_signature";
/// Normalized ix-label JSON array string (same form as token-dim `fp_ix_labels`).
pub const T_IX_LABELS: &str = "ix_labels";
/// Wallet address (LEFT JOIN `wallet_dict`; `unknown:{id}` on dict gap).
pub const T_WALLET: &str = "wallet";
/// Requested compute-unit limit (`SetComputeUnitLimit`). Nullable: the column is
/// forward-only (core migration `0013`), and a transaction may set no budget at all.
pub const T_CU_LIMIT: &str = "cu_limit";
/// Requested compute-unit price, **micro-lamports per CU** (`SetComputeUnitPrice`).
pub const T_CU_PRICE: &str = "cu_price";
/// Lamports transferred to a known tip account. `0` is a reading ("transfers landed,
/// none were tips"), NULL is "not captured" — see core migration `0013`.
pub const T_TIP_LAMPORTS: &str = "tip_lamports";

/// The trades columns in the exact order the writer's Arrow schema + `finish()` vec
/// build them. A guard test pins `trades_schema()` to this, so a reorder/rename in
/// either the schema or the builder vec fails loudly instead of silently mis-mapping.
pub const TRADE_WRITE_COLS: [&str; 18] = [
    T_MINT, T_IS_BUY, T_SOL_AMOUNT, T_TOKEN_AMOUNT, T_PRICE, T_SLOT, T_BLOCK_TIME,
    T_LEG_INDEX, T_VSOL, T_VTOK, T_VENUE, T_TX_INDEX, T_TX_SIGNATURE,
    T_IX_LABELS, T_WALLET, T_CU_LIMIT, T_CU_PRICE, T_TIP_LAMPORTS,
];

// --- tokens dimension columns (physical write order) ---
pub const K_MINT: &str = "mint";
pub const K_SYMBOL: &str = "symbol";
pub const K_FP_TOKEN_PROGRAM_ID: &str = "fp_token_program_id";
pub const K_FP_INITIAL_BUY_SOL: &str = "fp_initial_buy_sol";
pub const K_FP_CU_LIMIT: &str = "fp_cu_limit";
pub const K_FP_CU_PRICE: &str = "fp_cu_price";
pub const K_FP_IS_CASHBACK_ENABLED: &str = "fp_is_cashback_enabled";
pub const K_FP_MAX_SOL_COST: &str = "fp_max_sol_cost";
pub const K_FP_SPENDABLE_SOL_IN: &str = "fp_spendable_sol_in";
pub const K_FP_FIRST_SLOT_BUY_SOL: &str = "fp_first_slot_buy_sol";
pub const K_FP_FIRST_SLOT_SELL_SOL: &str = "fp_first_slot_sell_sol";
pub const K_FP_IX_LABELS: &str = "fp_ix_labels";
pub const K_IS_MAYHEM_MODE: &str = "is_mayhem_mode";
pub const K_CREATED_AT: &str = "created_at";
/// `hunter_engine::token_identity_hash(name, symbol)` — the copycat key, stored as
/// the hash rather than the raw `name`/`symbol` pair: 8 bytes instead of two
/// strings, and it is the exact value the engine compares, so a lake row and a
/// live event cannot disagree. Nullable (`NULL` = blank name or symbol ⇒ no
/// identity). Always 63-bit, so `Int64` holds it unchanged — see
/// [`hunter_engine::identity`]. The lab joins back to PG `tokens` when it needs
/// the human-readable name.
pub const K_IDENTITY_HASH: &str = "identity_hash";

/// The tokens-dimension columns in writer order (guard-tested against `tokens_schema`).
pub const TOKEN_WRITE_COLS: [&str; 15] = [
    K_MINT, K_SYMBOL, K_FP_TOKEN_PROGRAM_ID, K_FP_INITIAL_BUY_SOL, K_FP_CU_LIMIT,
    K_FP_CU_PRICE, K_FP_IS_CASHBACK_ENABLED, K_FP_MAX_SOL_COST, K_FP_SPENDABLE_SOL_IN,
    K_FP_FIRST_SLOT_BUY_SOL, K_FP_FIRST_SLOT_SELL_SOL, K_FP_IX_LABELS, K_IS_MAYHEM_MODE,
    K_CREATED_AT, K_IDENTITY_HASH,
];
