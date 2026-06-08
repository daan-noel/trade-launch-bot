use crate::config::constants::PUMP_SWAP_PROGRAM_ID;
use crate::config::Settings;

/// Builds the Helius `transactionSubscribe` JSON-RPC subscription payload.
/// Filters for all transactions that touch the Pump.fun bonding-curve program
/// OR the PumpSwap AMM program — `accountInclude` is a union, so this delivers
/// both pre-migration curve trades and post-migration pool swaps.
pub fn build_subscribe_message(settings: &Settings) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": settings.subscription_method,
        "params": [
            {
                // Any txn that includes the bonding-curve program (curve trades,
                // creates, migrations) or the PumpSwap program (post-migration
                // AMM swaps). Without the latter, a token's recorded trades stop
                // at graduation and its "lifetime" freezes there.
                "accountInclude": [&settings.pump_program_id, PUMP_SWAP_PROGRAM_ID],
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
