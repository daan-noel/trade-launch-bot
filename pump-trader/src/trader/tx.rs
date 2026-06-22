// ============================================================
// Transaction helpers — shared by buy and sell.
//
//  - build_nonce_tx:     wrap instructions in a durable-nonce tx and sign.
//  - send_transaction:   submit base64 tx to the Helius sender (skipPreflight).
//  - confirm_transaction: poll signature status until confirmed or timeout.
//
// All `pub(super)` — internal to the trader module, called from buy.rs/sell.rs.
// ============================================================

use super::PumpFunTrader;
use crate::constants::{BLOCKHASH_CACHE_MAX_AGE_MS, CONFIRM_POLL_MS, CONFIRM_POLL_SCHEDULE_MS};
use anyhow::{Context, Result};
use base64::{engine::general_purpose, Engine as _};
use serde_json::json;
use solana_client::rpc_config::RpcSimulateTransactionConfig;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    hash::Hash,
    instruction::{Instruction, InstructionError},
    message::Message,
    signature::{Keypair, Signature, Signer},
    pubkey::Pubkey,
    transaction::{Transaction, TransactionError},
};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info};

/// Upper bound on a single detached fan-out submission. The fast path returns on
/// the FIRST endpoint that accepts the tx; the losers keep running in the
/// background, so without a cap a black-holed endpoint would leak its task for
/// the process lifetime. 10s comfortably exceeds a healthy sender RTT while still
/// reaping a wedged connection. (The single-endpoint path is awaited directly and
/// isn't bounded here — its caller already wraps the send in a timeout.)
const FANOUT_SEND_TIMEOUT: Duration = Duration::from_secs(10);

/// Monotonic JSON-RPC request id. A simple atomic counter replaces a
/// `timestamp_millis` syscall on every `sendTransaction` (the id only needs to
/// be unique per request, not a clock).
static SEND_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

/// A transaction that landed on-chain and then reverted — as opposed to one
/// that never landed (dropped / lost the Jito auction / expired). The
/// distinction matters for retries and budget: a tx that never lands costs
/// *nothing*, so a blind re-send is free; a tx that landed-and-reverted already
/// burned base + priority fee, and re-sending an identical tx just re-pays them
/// for the same deterministic revert. `confirm_transaction` returns this so a
/// caller's retry loop can stop early on the latter. Carried as an
/// `anyhow::Error` cause — match it with `err.downcast_ref::<OnChainRevert>()`.
#[derive(Debug)]
pub(super) struct OnChainRevert(pub String);

impl std::fmt::Display for OnChainRevert {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Transaction failed on-chain: {}", self.0)
    }
}

impl std::error::Error for OnChainRevert {}

impl PumpFunTrader {
    pub(super) fn build_nonce_tx(
        &self,
        instructions: Vec<Instruction>,
        nonce_account: &Pubkey,
        nonce_hash: Hash,
        keypair: &Keypair,
    ) -> Result<Transaction> {
        let msg = Message::new_with_nonce(
            instructions,
            Some(&keypair.pubkey()),
            nonce_account,
            &keypair.pubkey(),
        );
        let mut tx = Transaction::new_unsigned(msg);
        tx.sign(&[keypair], nonce_hash);
        Ok(tx)
    }

    /// Build + sign a legacy tx against a recent blockhash (no durable nonce).
    /// Used for AMM buys, whose ~27-account instruction would overflow the
    /// 1232-byte tx limit once a nonce-advance (+2 accounts) is prepended (the
    /// largest cashback buy measures 1245 B with a nonce vs 1171 B with a
    /// blockhash — see `amm::tests`). Reads the background-refreshed blockhash
    /// cache and only fetches on-chain when it's empty/stale, so the AMM buy
    /// avoids a `getLatestBlockhash` RPC on the hot path without ever riding an
    /// expired hash.
    pub(super) async fn build_recent_tx(
        &self,
        instructions: Vec<Instruction>,
        keypair: &Keypair,
    ) -> Result<Transaction> {
        let blockhash = match self
            .blockhash_cache
            .get_fresh(Duration::from_millis(BLOCKHASH_CACHE_MAX_AGE_MS))
        {
            Some(hash) => hash,
            None => self
                .rpc
                .get_latest_blockhash()
                .await
                .context("fetch recent blockhash")?,
        };
        let msg = Message::new(&instructions, Some(&keypair.pubkey()));
        let mut tx = Transaction::new_unsigned(msg);
        tx.sign(&[keypair], blockhash);
        Ok(tx)
    }

    /// Submit a signed tx to the Helius Sender. With one configured endpoint this
    /// is a single POST; with several the *identical* signed tx is fanned out to
    /// all of them concurrently. Because the signature is identical, the bank
    /// dedups on-chain and the Jito tip is paid at most once, so extra
    /// submissions only add landing paths — never cost. Returns as soon as one
    /// endpoint accepts the tx; the rest keep running in the background so a
    /// slow or briefly-down endpoint can't gate the send.
    /// Encode a signed tx into the `sendTransaction` JSON-RPC body used by every
    /// sender endpoint. Extracted so the fan-out and the probe path serialize the
    /// tx exactly once and submit byte-identical bodies.
    pub(super) fn encode_send_body(&self, tx: &Transaction) -> Result<serde_json::Value> {
        let encoded = general_purpose::STANDARD.encode(bincode::serialize(tx)?);
        Ok(json!({
            "jsonrpc": "2.0",
            "id": SEND_REQUEST_ID.fetch_add(1, Ordering::Relaxed),
            "method": "sendTransaction",
            "params": [
                encoded,
                { "encoding": "base64", "skipPreflight": true, "maxRetries": 0 }
            ]
        }))
    }

    /// Simulate a tx (no SOL, no land) and return `(units_consumed, err, logs)`.
    /// `sig_verify: false` + `replace_recent_blockhash: true` so the node swaps in
    /// a current blockhash — the caller doesn't need a fresh hash or valid
    /// signatures, only a correctly-built instruction set. Used by the simulate
    /// probe to validate accounts / data / CU without sending.
    pub(super) async fn simulate_transaction(
        &self,
        tx: &Transaction,
    ) -> Result<(Option<u64>, Option<String>, Vec<String>)> {
        let cfg = RpcSimulateTransactionConfig {
            sig_verify: false,
            replace_recent_blockhash: true,
            commitment: Some(CommitmentConfig::processed()),
            ..Default::default()
        };
        let resp = self
            .rpc
            .simulate_transaction_with_config(tx, cfg)
            .await
            .context("simulateTransaction RPC")?;
        let v = resp.value;
        Ok((
            v.units_consumed,
            v.err.map(|e| format!("{e:?}")),
            v.logs.unwrap_or_default(),
        ))
    }

    pub(super) async fn send_transaction(&self, tx: &Transaction) -> Result<String> {
        let body = self.encode_send_body(tx)?;
        // Serialize the JSON-RPC envelope to bytes EXACTLY ONCE here, then share
        // the buffer across every endpoint. The previous `.json(body)` re-walked
        // and re-serialized the (base64-tx-bearing) Value per endpoint; now each
        // request just clones an `Arc<Vec<u8>>` pointer and ships the same bytes.
        let raw = Arc::new(serde_json::to_vec(&body).context("serialize sendTransaction body")?);

        let urls = &self.config.helius_sender_urls;

        // Single endpoint: await directly — no spawn/channel overhead, identical
        // to the pre-fan-out hot path.
        if urls.len() == 1 {
            return post_tx_bytes(&self.http, &urls[0], Arc::clone(&raw)).await;
        }

        // Fan out: fire every endpoint concurrently as a detached task and return
        // the first acceptance. Losers keep submitting in the background (their
        // send on the dropped channel just no-ops), so the slowest endpoint never
        // gates us while still adding its landing path. The serialized body is
        // shared via `Arc` so each task clones a pointer, not the full tx bytes.
        let (done_tx, mut done_rx) = tokio::sync::mpsc::channel::<Result<String>>(urls.len());
        for url in urls {
            let http = self.http.clone();
            let url = url.clone();
            let raw = Arc::clone(&raw);
            let done_tx = done_tx.clone();
            tokio::spawn(async move {
                // Bound each detached submission so a hung/black-holed endpoint
                // can't leak a background task, and surface the failure at debug
                // (was a fully-swallowed `let _ =`) so a degraded endpoint is
                // diagnosable. A timeout is reported as a normal send error so the
                // receiver only ever returns on a real acceptance.
                let result = match tokio::time::timeout(
                    FANOUT_SEND_TIMEOUT,
                    post_tx_bytes(&http, &url, raw),
                )
                .await
                {
                    Ok(res) => res,
                    Err(_) => Err(anyhow::anyhow!(
                        "sender {url} timed out after {FANOUT_SEND_TIMEOUT:?}"
                    )),
                };
                if let Err(ref e) = result {
                    debug!("sender fan-out endpoint {url} failed: {e}");
                }
                let _ = done_tx.send(result).await;
            });
        }
        drop(done_tx);

        let mut last_err = None;
        while let Some(res) = done_rx.recv().await {
            match res {
                Ok(sig) => return Ok(sig),
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no sender endpoints configured")))
    }

    /// Poll the RPC until the transaction is confirmed or retries are exhausted.
    pub(super) async fn confirm_transaction(&self, signature: &str, max_retries: usize) -> Result<()> {
        let sig = Signature::from_str(signature)?;

        // Poll status first, then sleep *between* polls (not before the first and
        // not after the last) — so a tx that already landed returns immediately
        // and the worst-case wait is `(max_retries - 1) × CONFIRM_POLL_MS`.
        for i in 0..max_retries {
            match self.rpc.get_signature_status(&sig).await? {
                Some(Ok(())) => return Ok(()),
                Some(Err(e)) => return Err(anyhow::Error::new(OnChainRevert(format!("{e:?}")))),
                None => {
                    if i + 1 < max_retries {
                        info!("⏳ Confirmation pending ({}/{})", i + 1, max_retries);
                        // Ramp the early polls (a confirmed tx usually lands in
                        // ~1–2 slots); fall back to the steady gap past the ramp.
                        let gap = CONFIRM_POLL_SCHEDULE_MS.get(i).copied().unwrap_or(CONFIRM_POLL_MS);
                        tokio::time::sleep(Duration::from_millis(gap)).await;
                    }
                }
            }
        }

        anyhow::bail!("Confirmation timed out for {}", signature)
    }

    /// One-shot signature status (no polling). Lets the snipe buy path classify a
    /// sent-but-unconfirmed tx without blocking on confirmation:
    ///   `Ok(Some(true))`  — landed and succeeded
    ///   `Ok(Some(false))` — landed but failed on-chain (reverted)
    ///   `Ok(None)`        — not yet visible (still pending, or dropped)
    pub async fn signature_state(&self, signature: &str) -> Result<Option<bool>> {
        Ok(match self.signature_state_detailed(signature).await? {
            SigStatus::Succeeded => Some(true),
            SigStatus::Reverted { .. } => Some(false),
            SigStatus::Pending => None,
        })
    }

    /// One-shot signature status that, for a landed-and-reverted tx, also surfaces
    /// the on-chain program error code. Lets the sell-confirm path tell a
    /// slippage-floor revert (price moved past `min_out` → retryable, re-quote and
    /// resend) apart from a structural revert (already-sold / empty account /
    /// wrong venue → not retryable, don't burn fees) — a distinction the bare
    /// landed-bool of [`signature_state`] collapses. The custom code is the
    /// program's Anchor error (e.g. curve `TooLittleSolReceived` = 6003, AMM
    /// `ExceededSlippage` = 6004); `None` when the failure was not an
    /// `InstructionError::Custom`.
    pub async fn signature_state_detailed(&self, signature: &str) -> Result<SigStatus> {
        let sig = Signature::from_str(signature)?;
        Ok(match self.rpc.get_signature_status(&sig).await? {
            Some(Ok(())) => SigStatus::Succeeded,
            Some(Err(err)) => SigStatus::Reverted {
                custom: custom_error_code(&err),
            },
            None => SigStatus::Pending,
        })
    }
}

/// Outcome of a one-shot signature status check that, unlike a bare landed-bool,
/// preserves the on-chain program error for a reverted tx so callers can
/// distinguish slippage reverts from structural ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SigStatus {
    /// Landed and succeeded.
    Succeeded,
    /// Landed but reverted on-chain. `custom` is the program's custom (Anchor)
    /// error code when the failure was an `InstructionError::Custom(code)`, else
    /// `None` (a non-custom failure such as insufficient funds / account error).
    Reverted { custom: Option<u32> },
    /// Not yet visible — still pending, or dropped.
    Pending,
}

/// Extract the program's custom error code from a failed-tx `TransactionError`,
/// if the failure was an `InstructionError::Custom(code)`. Other failure shapes
/// (account-not-found, insufficient funds, …) carry no Anchor code → `None`.
fn custom_error_code(err: &TransactionError) -> Option<u32> {
    match err {
        TransactionError::InstructionError(_, InstructionError::Custom(code)) => Some(*code),
        _ => None,
    }
}

/// POST one signed-tx JSON-RPC body to a single sender endpoint and return the
/// transaction signature it echoes back. Free function so the fan-out path can
/// drive it from detached tasks (no `&self` borrow to hold across the spawn).
/// Serializes `body` itself — used by callers (the probe) that hold a `Value`
/// and only hit a single endpoint, so the one-time serialize cost is irrelevant.
pub(super) async fn post_tx(
    http: &reqwest::Client,
    url: &str,
    body: &serde_json::Value,
) -> Result<String> {
    let raw = Arc::new(serde_json::to_vec(body).context("serialize JSON-RPC body")?);
    post_tx_bytes(http, url, raw).await
}

/// As [`post_tx`] but ships a pre-serialized JSON body. The fan-out serializes
/// the `sendTransaction` envelope once and hands every endpoint an `Arc` clone
/// of the same bytes, so the costly base64 tx is walked exactly once per send.
pub(super) async fn post_tx_bytes(
    http: &reqwest::Client,
    url: &str,
    body: Arc<Vec<u8>>,
) -> Result<String> {
    let resp = http
        .post(url)
        .header("Content-Type", "application/json")
        .body((*body).clone())
        .send()
        .await?;

    if !resp.status().is_success() {
        anyhow::bail!("HTTP error from sender {url}: {}", resp.text().await?);
    }

    let json: serde_json::Value = resp.json().await?;
    if let Some(err) = json.get("error") {
        anyhow::bail!("JSON-RPC error from {url}: {err:?}");
    }

    json.get("result")
        .and_then(|r| r.as_str())
        .map(str::to_string)
        .context("No signature in response")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trader::{PumpFunTrader, TraderConfig};
    use solana_sdk::{hash::Hash, message::Message, signature::Keypair, transaction::Transaction};
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Spin up a throwaway HTTP/1.1 endpoint that answers `max_conns` requests
    /// with a fixed status line + JSON body (after an optional delay), and return
    /// its `http://127.0.0.1:PORT/` URL. Enough to exercise the sender fan-out
    /// without a real Helius endpoint.
    async fn spawn_mock(
        max_conns: usize,
        status_line: &'static str,
        body: &'static str,
        delay_ms: u64,
    ) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            for _ in 0..max_conns {
                let (mut sock, _) = match listener.accept().await {
                    Ok(x) => x,
                    Err(_) => break,
                };
                tokio::spawn(async move {
                    // Drain a chunk of the request (don't need it all) so the
                    // client's write side doesn't error before we reply.
                    let mut buf = [0u8; 4096];
                    let _ = sock.read(&mut buf).await;
                    if delay_ms > 0 {
                        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                    }
                    let resp = format!(
                        "{status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = sock.write_all(resp.as_bytes()).await;
                    let _ = sock.flush().await;
                });
            }
        });
        format!("http://{addr}/")
    }

    fn trader_with(urls: Vec<String>) -> PumpFunTrader {
        PumpFunTrader::new(Arc::new(TraderConfig {
            rpc_url: "http://localhost".into(),
            helius_sender_urls: urls,
            keypair: Keypair::new(),
            nonce_accounts: vec![solana_sdk::pubkey::Pubkey::new_unique().to_string()],
        }))
    }

    fn dummy_signed_tx() -> Transaction {
        let kp = Keypair::new();
        let mut tx = Transaction::new_unsigned(Message::new(&[], Some(&kp.pubkey())));
        tx.sign(&[&kp], Hash::default());
        tx
    }

    const OK_BODY: &str = r#"{"jsonrpc":"2.0","id":"1","result":"5xMockSignature111111111111111111111111111111"}"#;
    const ERR_BODY: &str = r#"{"jsonrpc":"2.0","id":"1","error":{"code":-32000,"message":"nope"}}"#;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn single_endpoint_returns_the_signature() {
        let url = spawn_mock(1, "HTTP/1.1 200 OK", OK_BODY, 0).await;
        let trader = trader_with(vec![url]);
        let sig = trader.send_transaction(&dummy_signed_tx()).await.unwrap();
        assert!(sig.starts_with("5xMock"), "got {sig}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn fan_out_returns_success_despite_a_failing_endpoint() {
        // One endpoint 500s immediately; the other succeeds after a small delay.
        // The fan-out must surface the success, not the failure.
        let bad = spawn_mock(1, "HTTP/1.1 500 Internal Server Error", ERR_BODY, 0).await;
        let good = spawn_mock(1, "HTTP/1.1 200 OK", OK_BODY, 30).await;
        let trader = trader_with(vec![bad, good]);
        let sig = trader.send_transaction(&dummy_signed_tx()).await.unwrap();
        assert!(sig.starts_with("5xMock"), "got {sig}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn fan_out_errors_only_when_every_endpoint_fails() {
        let a = spawn_mock(1, "HTTP/1.1 500 Internal Server Error", ERR_BODY, 0).await;
        let b = spawn_mock(1, "HTTP/1.1 200 OK", ERR_BODY, 0).await; // 200 but JSON-RPC error
        let trader = trader_with(vec![a, b]);
        assert!(
            trader.send_transaction(&dummy_signed_tx()).await.is_err(),
            "all endpoints failed → send must error"
        );
    }
}
