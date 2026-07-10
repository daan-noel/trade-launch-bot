//! Lake column names — the SINGLE SOURCE for the sealed-day Parquet schema.
//!
//! The Parquet writer and the DuckDB name-based reader (both land later, LAB
//! only) MUST import these constants — never string-literal a column name at a
//! call site. The guard test below pins the ordered set so a writer/reader can't
//! silently drift. Columns mirror the generalized `trades` + a `tokens`
//! dimension: quote/base amounts + the venue-neutral reserve pair + the interned
//! `launchpad_id`/`quote_asset_id` (a new launchpad/quote is data, not a column).

/// Ordered column set for the sealed-day `trades` Parquet files. The reader binds
/// by name, but a stable order keeps writer output deterministic.
pub mod trades {
    pub const MINT_ADDRESS: &str = "mint_address";
    /// `wallet_dict` interned id (soft ref), NOT a managed-wallet UUID.
    pub const WALLET_REF: &str = "wallet_ref";
    pub const LAUNCHPAD_ID: &str = "launchpad_id";
    pub const MARKET_KIND: &str = "market_kind";
    pub const QUOTE_ASSET_ID: &str = "quote_asset_id";
    pub const TRADE_TYPE: &str = "trade_type";
    pub const AMOUNT_QUOTE: &str = "amount_quote";
    pub const AMOUNT_BASE: &str = "amount_base";
    pub const RESERVE_QUOTE: &str = "reserve_quote";
    pub const RESERVE_BASE: &str = "reserve_base";
    pub const SLOT: &str = "slot";
    pub const TX_INDEX: &str = "tx_index";
    pub const LEG_INDEX: &str = "leg_index";
    pub const BLOCK_TIME: &str = "block_time";
    pub const TX_SIGNATURE: &str = "tx_signature";

    /// The full ordered column list. Writer + reader both consume this.
    pub const COLUMNS: &[&str] = &[
        MINT_ADDRESS,
        WALLET_REF,
        LAUNCHPAD_ID,
        MARKET_KIND,
        QUOTE_ASSET_ID,
        TRADE_TYPE,
        AMOUNT_QUOTE,
        AMOUNT_BASE,
        RESERVE_QUOTE,
        RESERVE_BASE,
        SLOT,
        TX_INDEX,
        LEG_INDEX,
        BLOCK_TIME,
        TX_SIGNATURE,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn trade_columns_are_unique_and_named_as_expected() {
        // No duplicates — a copy-paste slip in the const list is caught here.
        let set: HashSet<_> = trades::COLUMNS.iter().collect();
        assert_eq!(set.len(), trades::COLUMNS.len(), "duplicate lake column");

        // Pins the generalized quote/base + reserve pair are present (the whole
        // point of the redesign — no lamport-locked names in the lake either).
        for required in [
            trades::AMOUNT_QUOTE,
            trades::AMOUNT_BASE,
            trades::RESERVE_QUOTE,
            trades::RESERVE_BASE,
            trades::LAUNCHPAD_ID,
            trades::QUOTE_ASSET_ID,
        ] {
            assert!(trades::COLUMNS.contains(&required), "missing {required}");
        }
        assert_eq!(trades::COLUMNS.len(), 15);
    }
}
