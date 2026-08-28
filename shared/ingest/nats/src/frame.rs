//! One relay frame → one neutral update.
//!
//! Pure and unit-tested: unwrap whatever envelope the publisher uses, screen the
//! failures gRPC would have screened server-side, and hand the result to the ONE
//! JSON→protobuf adapter in `ingest-core`. No second converter is written here.

use ingest_core::convert;
use ingest_core::feed::FeedUpdate;
use serde_json::Value;

/// Why a frame produced no transaction. Counted so a silent feed is diagnosable
/// from the stats line alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reject {
    /// Not JSON, or no transaction survived the conversion.
    Unparseable,
    /// The transaction failed on chain — gRPC screens these server-side
    /// (`failed: Some(false)`), so this wire applies the same screen locally.
    Failed,
}

/// Parse one relay frame. `Err` says why it carried no transaction; the caller
/// still counts it as a frame, because a frame is liveness evidence either way.
pub fn parse(payload: &[u8]) -> Result<FeedUpdate, Reject> {
    let envelope: Value = serde_json::from_slice(payload).map_err(|_| Reject::Unparseable)?;
    let result = extract_result(&envelope).ok_or(Reject::Unparseable)?;

    if convert::json_tx_failed(result) {
        return Err(Reject::Failed);
    }

    convert::json_tx_to_protobuf(result)
        .map(FeedUpdate::Transaction)
        .ok_or(Reject::Unparseable)
}

/// Unwrap the transaction result from whatever envelope the relay uses.
///
/// Handles the Helius WS notification (`params.result`), a bare JSON-RPC response
/// (`result`), and an already-unwrapped result — so a relay that decides to strip
/// its envelope does not break the feed.
pub fn extract_result(envelope: &Value) -> Option<&Value> {
    if let Some(r) = envelope.get("params").and_then(|p| p.get("result")) {
        return Some(r);
    }
    if let Some(r) = envelope.get("result") {
        return Some(r);
    }
    envelope.get("transaction").map(|_| envelope)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn result_is_found_in_every_envelope_shape() {
        let inner = json!({"slot": 1, "transaction": {"meta": {}}});
        let wrapped = json!({
            "jsonrpc": "2.0",
            "method": "transactionNotification",
            "params": {"subscription": 7, "result": inner.clone()}
        });
        assert_eq!(extract_result(&wrapped), Some(&inner));

        let rpc = json!({"jsonrpc": "2.0", "id": 1, "result": inner.clone()});
        assert_eq!(extract_result(&rpc), Some(&inner));

        assert_eq!(extract_result(&inner), Some(&inner));

        assert_eq!(extract_result(&json!({"nope": 1})), None);
    }

    #[test]
    fn a_non_json_frame_is_unparseable_not_a_panic() {
        assert_eq!(parse(b"\x00\x01not json").err(), Some(Reject::Unparseable));
    }

    /// gRPC never delivers a failed transaction; this wire must not either, or
    /// the two sources would decode different corpora.
    #[test]
    fn a_failed_transaction_is_screened_locally() {
        let frame = json!({
            "params": {"result": {
                "slot": 1,
                "signature": "sig",
                "transaction": {"meta": {"err": {"InstructionError": [0, "Custom"]}}}
            }}
        });
        assert_eq!(parse(frame.to_string().as_bytes()).err(), Some(Reject::Failed));
    }
}
