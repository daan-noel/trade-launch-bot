use anyhow::{anyhow, Context};
use serde_json::{json, Value};
use std::sync::OnceLock;
use std::time::Duration;

use crate::services::http;

/// Shared RPC client/pool reused across every `HeliusRpc::new()`. Distinct from
/// `http::client()` because the RPC path wants a longer (30 s) timeout.
static RPC_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

fn rpc_client() -> reqwest::Client {
    RPC_CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .timeout(Duration::from_secs(RPC_TIMEOUT_SECS))
                .user_agent(http::USER_AGENT)
                .build()
                .expect("failed to build Helius RPC client")
        })
        .clone()
}

/// Per-request timeout. Interactive rather than the old 120 s so a wedged RPC
/// fails fast instead of hanging an API request or a sync step for two minutes.
const RPC_TIMEOUT_SECS: u64 = 30;
const SIG_PAGE_LIMIT: usize = 1000;
/// Retry/backoff for transient RPC failures (429 / 5xx / network).
const RPC_MAX_RETRIES: usize = 4;
const RPC_BASE_BACKOFF_MS: u64 = 250;
const RPC_MAX_BACKOFF_MS: u64 = 4_000;

/// JSON-RPC client for Helius / Solana HTTP RPC.
#[derive(Clone)]
pub struct HeliusRpc {
    url: String,
    client: reqwest::Client,
}

impl HeliusRpc {
    pub fn new(url: String) -> Self {
        Self {
            url,
            client: rpc_client(),
        }
    }

    async fn call(&self, method: &str, params: Value) -> anyhow::Result<Value> {
        let body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        });

        let mut attempt = 0usize;
        loop {
            attempt += 1;

            let resp = match self.client.post(&self.url).json(&body).send().await {
                Ok(resp) => resp,
                Err(e) => {
                    // Network error / timeout — retry a few times before giving up.
                    if attempt <= RPC_MAX_RETRIES {
                        tokio::time::sleep(backoff_delay(attempt, None)).await;
                        continue;
                    }
                    return Err(anyhow::Error::new(e)
                        .context(format!("Helius RPC {method} request failed")));
                }
            };

            let status = resp.status();

            // Check the HTTP status BEFORE reading the body: a rate-limited (429)
            // or transient server-error (5xx) response is often a non-JSON page,
            // which the old "parse first" path misreported as "invalid JSON".
            // Back off (honoring a 429 `Retry-After`) and retry up to the cap.
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
                if attempt <= RPC_MAX_RETRIES {
                    let retry_after = retry_after_ms(&resp);
                    tokio::time::sleep(backoff_delay(attempt, retry_after)).await;
                    continue;
                }
                return Err(anyhow!(
                    "Helius RPC {method} HTTP {status} after {attempt} attempts"
                ));
            }

            // Other non-success (4xx besides 429) is not retryable.
            if !status.is_success() {
                return Err(anyhow!("Helius RPC {method} HTTP {status}"));
            }

            let payload: Value = resp.json().await.context("Helius RPC invalid JSON")?;
            if let Some(err) = payload.get("error") {
                return Err(anyhow!("Helius RPC {method} error: {err}"));
            }
            return payload
                .get("result")
                .cloned()
                .ok_or_else(|| anyhow!("Helius RPC {method} missing result"));
        }
    }

    /// Quick check: at least one successful signature exists for the address.
    pub async fn has_any_signature(&self, address: &str) -> anyhow::Result<bool> {
        let mut before: Option<String> = None;

        loop {
            let mut cfg = json!({ "limit": SIG_PAGE_LIMIT, "commitment": "confirmed" });
            if let Some(ref sig) = before {
                cfg["before"] = json!(sig);
            }

            let batch = self
                .call("getSignaturesForAddress", json!([address, cfg]))
                .await?;
            let entries = parse_signature_entries(&batch)?;

            if entries.is_empty() {
                return Ok(false);
            }

            if entries.iter().any(|e| e.err.is_none()) {
                return Ok(true);
            }

            if entries.len() < SIG_PAGE_LIMIT {
                return Ok(false);
            }

            before = entries.last().map(|e| e.signature.clone());
        }
    }

    /// Returns `true` when the account exists on-chain.
    pub async fn account_exists(&self, address: &str) -> anyhow::Result<bool> {
        let result = self
            .call(
                "getAccountInfo",
                json!([address, { "encoding": "base64" }]),
            )
            .await?;
        // getAccountInfo wraps the account in `{ context, value }`, with `value`
        // null when the account doesn't exist. Check `value`, not the wrapper
        // (which is never null) — otherwise this returns true for every address.
        Ok(result.get("value").is_some_and(|v| !v.is_null()))
    }

    /// Paginate successful (non-failed) signatures for an address, oldest slot first.
    pub async fn get_all_signatures(
        &self,
        address: &str,
        until: Option<&str>,
        on_page: impl Fn(usize, usize),
    ) -> anyhow::Result<Vec<SignatureEntry>> {
        let mut all = Vec::new();
        let mut before: Option<String> = None;
        let mut page = 0usize;

        loop {
            page += 1;
            let mut cfg = json!({ "limit": SIG_PAGE_LIMIT, "commitment": "confirmed" });
            if let Some(ref sig) = before {
                cfg["before"] = json!(sig);
            }
            if let Some(sig) = until {
                cfg["until"] = json!(sig);
            }

            let batch = self
                .call("getSignaturesForAddress", json!([address, cfg]))
                .await?;

            let entries = parse_signature_entries(&batch)?;
            let count = entries.len();
            if count == 0 {
                on_page(page, all.len());
                break;
            }

            let page_before = entries.last().map(|e| e.signature.clone());

            for entry in entries {
                if entry.err.is_none() {
                    all.push(entry);
                }
            }

            on_page(page, all.len());

            if count < SIG_PAGE_LIMIT {
                break;
            }

            before = page_before;
        }

        all.sort_by_key(|e| e.slot);
        Ok(all)
    }

    /// Count successful signatures for an address, optionally only those newer
    /// than `until`, stopping after `max_pages` pages.
    ///
    /// Returns `(count, capped)` where `capped` is true if the page cap was hit
    /// before history was exhausted. This is the cheap half of a sync — it pages
    /// `getSignaturesForAddress` (signatures only, no `getTransaction`), so it
    /// estimates how many transactions a sync would download without downloading
    /// them. With `until` set the RPC stops at that signature, so the "new since
    /// last sync" count is usually a single page.
    pub async fn count_signatures(
        &self,
        address: &str,
        until: Option<&str>,
        max_pages: usize,
    ) -> anyhow::Result<(usize, bool)> {
        let mut count = 0usize;
        let mut before: Option<String> = None;
        let mut page = 0usize;

        loop {
            if page >= max_pages {
                return Ok((count, true));
            }
            page += 1;

            let mut cfg = json!({ "limit": SIG_PAGE_LIMIT, "commitment": "confirmed" });
            if let Some(ref sig) = before {
                cfg["before"] = json!(sig);
            }
            if let Some(sig) = until {
                cfg["until"] = json!(sig);
            }

            let batch = self
                .call("getSignaturesForAddress", json!([address, cfg]))
                .await?;
            let entries = parse_signature_entries(&batch)?;
            let page_len = entries.len();
            if page_len == 0 {
                break;
            }

            count += entries.iter().filter(|e| e.err.is_none()).count();

            if page_len < SIG_PAGE_LIMIT {
                break;
            }
            before = entries.last().map(|e| e.signature.clone());
        }

        Ok((count, false))
    }

    /// Fetch a confirmed parsed transaction; returns `None` if missing or failed on-chain.
    pub async fn get_transaction(&self, signature: &str) -> anyhow::Result<Option<Value>> {
        let result = self
            .call(
                "getTransaction",
                json!([
                    signature,
                    {
                        "encoding": "jsonParsed",
                        "commitment": "confirmed",
                        "maxSupportedTransactionVersion": 0
                    }
                ]),
            )
            .await?;

        if result.is_null() {
            return Ok(None);
        }

        if result.get("meta").and_then(|m| m.get("err")).map(|e| !e.is_null()).unwrap_or(false)
        {
            return Ok(None);
        }

        Ok(Some(result))
    }

    /// Fetch many confirmed transactions in one JSON-RPC batch request (an array
    /// of call objects in a single HTTP POST). Returns one entry per input
    /// signature, in the SAME order, with `None` for any transaction that is
    /// missing, errored, or failed on-chain — matching [`Self::get_transaction`].
    ///
    /// Batching cuts HTTP round-trips (latency); it does NOT reduce Helius
    /// credits, which are billed per `getTransaction`, not per request.
    pub async fn get_transactions_batch(
        &self,
        signatures: &[String],
    ) -> anyhow::Result<Vec<Option<Value>>> {
        if signatures.is_empty() {
            return Ok(Vec::new());
        }

        // One call object per signature; `id` is the index so results can be
        // mapped back — a batch response may arrive out of order.
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
                            "encoding": "jsonParsed",
                            "commitment": "confirmed",
                            "maxSupportedTransactionVersion": 0
                        }
                    ]
                })
            })
            .collect();

        let resp = self
            .client
            .post(&self.url)
            .json(&Value::Array(batch))
            .send()
            .await
            .context("Helius RPC batch request failed")?;

        let status = resp.status();
        let payload: Value = resp.json().await.context("Helius RPC batch invalid JSON")?;

        // A non-array payload means the whole batch was rejected (e.g. an error
        // object) rather than answered per-call.
        let arr = match payload.as_array() {
            Some(arr) => arr,
            None => {
                if let Some(err) = payload.get("error") {
                    return Err(anyhow!("Helius RPC batch error: {err}"));
                }
                return Err(anyhow!("Helius RPC batch HTTP {status}: unexpected response"));
            }
        };

        // Place each response by its `id`; leave missing/errored/failed as None.
        let mut out: Vec<Option<Value>> = vec![None; signatures.len()];
        for item in arr {
            let Some(idx) = item.get("id").and_then(|v| v.as_u64()).map(|i| i as usize) else {
                continue;
            };
            if idx >= out.len() || item.get("error").is_some() {
                continue;
            }
            let result = match item.get("result") {
                Some(r) if !r.is_null() => r,
                _ => continue,
            };
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

    /// One page of Helius `getTransactionsForAddress` (gTFA) in FULL mode,
    /// `jsonParsed` encoding, succeeded-only. Returns the raw `data` items — each
    /// already shaped like a `getTransaction` result (`slot`/`blockTime`/
    /// `transaction`/`meta`) — and the next `paginationToken` (`None` at the end).
    ///
    /// This is the archival path for full backfills: ~0.1 credit/tx at limit 1000
    /// (10 credits per 100 returned), vs 1 credit/tx for per-sig `getTransaction`,
    /// AND it collapses the getSignaturesForAddress + N×getTransaction loop into
    /// one cursor-paginated call. `params` is positional: `[address, {options}]`.
    pub async fn get_transactions_for_address_full_page(
        &self,
        address: &str,
        sort_order: &str,
        limit: usize,
        pagination_token: Option<&str>,
    ) -> anyhow::Result<(Vec<Value>, Option<String>)> {
        let mut options = json!({
            "transactionDetails": "full",
            "encoding": "jsonParsed",
            "commitment": "confirmed",
            "maxSupportedTransactionVersion": 0,
            "sortOrder": sort_order,
            "limit": limit,
            "filters": { "status": "succeeded" }
        });
        if let Some(token) = pagination_token {
            options["paginationToken"] = json!(token);
        }

        let result = self
            .call("getTransactionsForAddress", json!([address, options]))
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

#[derive(Debug, Clone)]
pub struct SignatureEntry {
    pub signature: String,
    pub slot: u64,
    pub err: Option<Value>,
}

fn parse_signature_entries(batch: &Value) -> anyhow::Result<Vec<SignatureEntry>> {
    let arr = batch
        .as_array()
        .ok_or_else(|| anyhow!("getSignaturesForAddress: expected array"))?;

    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let signature = item
            .get("signature")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("signature entry missing signature"))?
            .to_string();
        let slot = item.get("slot").and_then(|v| v.as_u64()).unwrap_or(0);
        let err = item.get("err").cloned().filter(|e| !e.is_null());
        out.push(SignatureEntry {
            signature,
            slot,
            err,
        });
    }
    Ok(out)
}

/// Wrap RPC `getTransaction` result into Helius notification `params.result` shape.
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
