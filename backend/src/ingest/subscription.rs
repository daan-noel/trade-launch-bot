use crate::config::Settings;

/// Build a Helius `transactionSubscribe` message for an arbitrary
/// `account_include` list. `id` is the JSON-RPC request id, echoed back in the
/// subscription confirmation; we never unsubscribe, so any unique value works.
///
/// The ingest runs two kinds of subscription over one connection:
///   - a static one on the pump.fun bonding-curve program — token discovery,
///     curve trades, and migrations; and
///   - dynamic ones on the specific PumpSwap *pool* accounts of migrated tokens,
///     so post-migration AMM swaps are delivered without subscribing to the
///     entire PumpSwap program (which would fire-hose every token's swaps).
pub fn build_subscribe_message(settings: &Settings, id: u64, account_include: &[&str]) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": settings.subscription_method,
        "params": [
            {
                "accountInclude": account_include,
                "failed": false
            },
            {
                "commitment": "processed",
                "encoding": "jsonParsed",
                "transactionDetails": "full",
                "showRewards": false,
                "maxSupportedTransactionVersion": 0
            }
        ]
    })
    .to_string()
}
