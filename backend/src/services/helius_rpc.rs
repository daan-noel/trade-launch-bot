use anyhow::{anyhow, Context};
use serde_json::{json, Value};
use std::time::Duration;

use crate::services::http;

const RPC_TIMEOUT_SECS: u64 = 120;
const SIG_PAGE_LIMIT: usize = 1000;

/// JSON-RPC client for Helius / Solana HTTP RPC.
#[derive(Clone)]
pub struct HeliusRpc {
    url: String,
    client: reqwest::Client,
}

impl HeliusRpc {
    pub fn new(url: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(RPC_TIMEOUT_SECS))
            .user_agent(http::USER_AGENT)
            .build()
            .expect("failed to build Helius RPC client");

        Self { url, client }
    }

    async fn call(&self, method: &str, params: Value) -> anyhow::Result<Value> {
        let body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        });

        let resp = self
            .client
            .post(&self.url)
            .json(&body)
            .send()
            .await
            .context("Helius RPC request failed")?;

        let status = resp.status();
        let payload: Value = resp.json().await.context("Helius RPC invalid JSON")?;

        if let Some(err) = payload.get("error") {
            return Err(anyhow!("Helius RPC {method} error: {err}"));
        }

        if !status.is_success() {
            return Err(anyhow!("Helius RPC {method} HTTP {status}"));
        }

        payload
            .get("result")
            .cloned()
            .ok_or_else(|| anyhow!("Helius RPC {method} missing result"))
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
