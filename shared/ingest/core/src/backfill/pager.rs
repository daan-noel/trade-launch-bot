//! JSON-RPC pager for the `rpc-backfill` feature: enumerate an address's
//! signatures (`getSignaturesForAddress`) and fetch their transactions in
//! batches (`getTransaction`, `encoding="base64"`).
//!
//! The lowered `getTransaction` results feed [`super::rpc_to_protobuf`] →
//! `Decoder::decode_protobuf`, exactly like the live feed — the decoder can't
//! tell the source apart. Lifted from hunter's `helius_rpc.rs` (no hunter dep) so
//! forge's keystore-restore backfill can page an arbitrary wallet's history.
//!
//! New functions only — additive for the existing (hunter) consumer of this
//! feature, which uses only [`super::rpc_to_protobuf`].

use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{anyhow, Context};
use serde_json::{json, Value};

/// Per-request timeout — interactive rather than the old 120 s so a wedged RPC
/// fails fast instead of hanging a backfill for two minutes.
const RPC_TIMEOUT_SECS: u64 = 30;
/// `getSignaturesForAddress` page size (the RPC maximum).
const SIG_PAGE_LIMIT: usize = 1000;
/// Retry/backoff for transient RPC failures (429 / 5xx / network).
const RPC_MAX_RETRIES: usize = 4;
const RPC_BASE_BACKOFF_MS: u64 = 250;
const RPC_MAX_BACKOFF_MS: u64 = 4_000;

/// Shared RPC client (30 s timeout), reused across every pager call.
static RPC_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

fn rpc_client() -> reqwest::Client {
    RPC_CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .timeout(Duration::from_secs(RPC_TIMEOUT_SECS))
                .build()
                .expect("failed to build backfill RPC client")
        })
        .clone()
}

/// One `getSignaturesForAddress` entry (successful txs only — a failed tx has no
/// trade/create to decode). Sorted oldest-slot-first by the pager.
#[derive(Debug, Clone)]
pub struct SignatureInfo {
    pub signature: String,
    pub slot: u64,
    /// On-chain block time (Unix seconds); `None` for a not-yet-finalized entry.
    pub block_time: Option<i64>,
}

/// Paginate ALL successful signatures for `address`, oldest-slot first.
///
/// `before` / `until` bound the window (both are signatures, RPC-native cursors):
/// `before` starts paging backwards from that signature, `until` stops once it's
/// reached. `page_limit` caps the per-page size (clamped to the 1000 RPC max).
/// Pages until history is exhausted.
pub async fn get_signatures_for_address(
    rpc_url: &str,
    address: &str,
    before: Option<&str>,
    until: Option<&str>,
    page_limit: usize,
) -> anyhow::Result<Vec<SignatureInfo>> {
    let limit = page_limit.clamp(1, SIG_PAGE_LIMIT);
    let client = rpc_client();
    let mut all: Vec<SignatureInfo> = Vec::new();
    let mut cursor: Option<String> = before.map(str::to_string);

    loop {
        let mut cfg = json!({ "limit": limit, "commitment": "confirmed" });
        if let Some(ref sig) = cursor {
            cfg["before"] = json!(sig);
        }
        if let Some(sig) = until {
            cfg["until"] = json!(sig);
        }

        let batch = rpc_call(
            &client,
            rpc_url,
            "getSignaturesForAddress",
            json!([address, cfg]),
        )
        .await?;

        let entries = batch
            .as_array()
            .ok_or_else(|| anyhow!("getSignaturesForAddress: expected array"))?;
        let page_len = entries.len();
        if page_len == 0 {
            break;
        }

        // The page's last signature is the cursor for the next (older) page.
        let page_before = entries
            .last()
            .and_then(|e| e.get("signature"))
            .and_then(Value::as_str)
            .map(str::to_string);

        for item in entries {
            // Skip failed txs — nothing to decode.
            if item.get("err").map(|e| !e.is_null()).unwrap_or(false) {
                continue;
            }
            let Some(signature) = item.get("signature").and_then(Value::as_str) else {
                continue;
            };
            all.push(SignatureInfo {
                signature: signature.to_string(),
                slot: item.get("slot").and_then(Value::as_u64).unwrap_or(0),
                block_time: item.get("blockTime").and_then(Value::as_i64),
            });
        }

        if page_len < limit {
            break;
        }
        cursor = page_before;
    }

    all.sort_by_key(|e| e.slot);
    Ok(all)
}

/// Fetch many confirmed transactions in ONE JSON-RPC batch request (an array of
/// call objects in a single HTTP POST). Returns one entry per input signature, in
/// the SAME order, `None` for any tx that is missing / errored / failed on-chain.
///
/// Each `Some` is the raw `getTransaction` result (`slot` / `blockTime` /
/// `transaction` / `meta`), ready to hand to [`wrap_transaction_result`] +
/// [`super::rpc_to_protobuf`]. `encoding="base64"` so the raw instruction `data`
/// survives (jsonParsed pre-parses it away). Batching cuts HTTP round-trips, not
/// Helius credits (billed per `getTransaction`).
pub async fn get_transactions_batch(
    rpc_url: &str,
    signatures: &[String],
) -> anyhow::Result<Vec<Option<Value>>> {
    if signatures.is_empty() {
        return Ok(Vec::new());
    }
    let client = rpc_client();

    // One call object per signature; `id` is the index so out-of-order responses
    // map back to their input slot.
    let batch: Vec<Value> = signatures
        .iter()
        .enumerate()
        .map(|(i, sig)| {
            json!({
                "jsonrpc": "2.0",
                "id": i,
                "method": "getTransaction",
                "params": [
                    sig,
                    {
                        "encoding": "base64",
                        "commitment": "confirmed",
                        "maxSupportedTransactionVersion": 0
                    }
                ]
            })
        })
        .collect();

    let resp = client
        .post(rpc_url)
        .json(&Value::Array(batch))
        .send()
        .await
        .context("getTransaction batch request failed")?;
    let status = resp.status();
    let payload: Value = resp.json().await.context("getTransaction batch invalid JSON")?;

    let arr = match payload.as_array() {
        Some(arr) => arr,
        None => {
            if let Some(err) = payload.get("error") {
                return Err(anyhow!("getTransaction batch error: {err}"));
            }
            return Err(anyhow!("getTransaction batch HTTP {status}: unexpected response"));
        }
    };

    let mut out: Vec<Option<Value>> = vec![None; signatures.len()];
    for item in arr {
        let Some(idx) = item.get("id").and_then(Value::as_u64).map(|i| i as usize) else {
            continue;
        };
        if idx >= out.len() || item.get("error").is_some() {
            continue;
        }
        let result = match item.get("result") {
            Some(r) if !r.is_null() => r,
            _ => continue,
        };
        // Drop txs that failed on-chain (no real trade/create to decode).
        let failed = result
            .get("meta")
            .and_then(|m| m.get("err"))
            .map(|e| !e.is_null())
            .unwrap_or(false);
        if failed {
            continue;
        }
        out[idx] = Some(result.clone());
    }

    Ok(out)
}

/// Fetch ONE archival `getTransactionsForAddress` (gTFA) page for `address`,
/// cursor-paginated. Each returned item is already shaped like a `getTransaction`
/// result (`slot` / `blockTime` / `transaction` / `meta`), so it lowers straight
/// to a decoder frame via [`wrap_transaction_result`] + [`super::rpc_to_protobuf`]
/// — no second `getTransaction` round-trip per signature.
///
/// This is the **archival** path: ~0.1 credit/tx at `limit=1000` (10 credits per
/// 100 returned) vs 1 credit/tx for the [`get_signatures_for_address`] +
/// [`get_transactions_batch`] loop, AND it collapses the two calls into one
/// cursor-paginated call. `filters.status = "succeeded"` drops failed txs
/// server-side (nothing to decode). `encoding="base64"` keeps the raw instruction
/// `data` (jsonParsed pre-parses it away); pass `""` as the signature to
/// [`wrap_transaction_result`] and let `rpc_to_protobuf` recover it from the
/// bincode tx. Returns the page plus the next `paginationToken` (`None` ⇒ end of
/// history). `sort_order` is `"asc"` (oldest-first) or `"desc"` (newest-first).
pub async fn get_transactions_for_address_page(
    rpc_url: &str,
    address: &str,
    sort_order: &str,
    limit: usize,
    pagination_token: Option<&str>,
    encoding: &str,
) -> anyhow::Result<(Vec<Value>, Option<String>)> {
    let mut options = json!({
        "transactionDetails": "full",
        "encoding": encoding,
        "commitment": "confirmed",
        "maxSupportedTransactionVersion": 0,
        "sortOrder": sort_order,
        "limit": limit.clamp(1, SIG_PAGE_LIMIT),
        "filters": { "status": "succeeded" }
    });
    if let Some(token) = pagination_token {
        options["paginationToken"] = json!(token);
    }

    let client = rpc_client();
    let result = rpc_call(
        &client,
        rpc_url,
        "getTransactionsForAddress",
        json!([address, options]),
    )
    .await?;

    let data = result
        .get("data")
        .and_then(|d| d.as_array())
        .cloned()
        .unwrap_or_default();
    let next = result
        .get("paginationToken")
        .and_then(|t| t.as_str())
        .map(|s| s.to_string());

    Ok((data, next))
}

/// Wrap a raw `getTransaction` result into the shape [`super::rpc_to_protobuf`]
/// expects: `{ signature, slot, blockTime, transaction: { transaction, meta } }`.
/// Pass `""` for `signature` on a base64 result (the adapter recovers it from the
/// bincode tx).
pub fn wrap_transaction_result(signature: &str, rpc_tx: &Value) -> Value {
    json!({
        "signature": signature,
        "slot": rpc_tx.get("slot").unwrap_or(&Value::Null),
        "blockTime": rpc_tx.get("blockTime").unwrap_or(&Value::Null),
        "transaction": {
            "transaction": rpc_tx.get("transaction").unwrap_or(&Value::Null),
            "meta": rpc_tx.get("meta").unwrap_or(&Value::Null),
        }
    })
}

/// One JSON-RPC call with retry/backoff on transient failures (429 / 5xx /
/// network), returning the `result` field. Same policy as hunter's `HeliusRpc`.
async fn rpc_call(
    client: &reqwest::Client,
    url: &str,
    method: &str,
    params: Value,
) -> anyhow::Result<Value> {
    let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });

    let mut attempt = 0usize;
    loop {
        attempt += 1;
        let resp = match client.post(url).json(&body).send().await {
            Ok(resp) => resp,
            Err(e) => {
                if attempt <= RPC_MAX_RETRIES {
                    tokio::time::sleep(backoff_delay(attempt, None)).await;
                    continue;
                }
                return Err(anyhow::Error::new(e).context(format!("{method} request failed")));
            }
        };

        let http_status = resp.status();
        // Retry 429 / 5xx (often a non-JSON page) after a backoff.
        if http_status == reqwest::StatusCode::TOO_MANY_REQUESTS || http_status.is_server_error() {
            if attempt <= RPC_MAX_RETRIES {
                let retry_after = retry_after_ms(&resp);
                tokio::time::sleep(backoff_delay(attempt, retry_after)).await;
                continue;
            }
            return Err(anyhow!("{method} HTTP {http_status} after {attempt} attempts"));
        }
        if !http_status.is_success() {
            return Err(anyhow!("{method} HTTP {http_status}"));
        }

        let payload: Value = resp.json().await.context("invalid JSON")?;
        if let Some(err) = payload.get("error") {
            return Err(anyhow!("{method} error: {err}"));
        }
        return payload
            .get("result")
            .cloned()
            .ok_or_else(|| anyhow!("{method} missing result"));
    }
}

/// `Retry-After` header (whole seconds) as milliseconds, if present and numeric.
fn retry_after_ms(resp: &reqwest::Response) -> Option<u64> {
    resp.headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(|secs| secs.saturating_mul(1000))
}

/// Exponential backoff for `attempt` (1-based), capped at `RPC_MAX_BACKOFF_MS`.
/// A 429 `Retry-After` wins when it asks for a longer wait.
fn backoff_delay(attempt: usize, retry_after_ms: Option<u64>) -> Duration {
    let shift = attempt.saturating_sub(1).min(5) as u32;
    let exp = RPC_BASE_BACKOFF_MS.saturating_mul(1u64 << shift);
    let ms = exp.min(RPC_MAX_BACKOFF_MS).max(retry_after_ms.unwrap_or(0));
    Duration::from_millis(ms)
}
