use crate::config::Settings;

/// Builds the Helius `transactionSubscribe` JSON-RPC subscription payload.
/// Filters for all transactions that touch the Pump.fun program.
pub fn build_subscribe_message(settings: &Settings) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": settings.subscription_method,
        "params": [
            {
                // Subscribe to all txns that include the Pump.fun program account
                "accountInclude": [&settings.pump_program_id],
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
