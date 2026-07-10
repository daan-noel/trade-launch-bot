// ============================================================
// Transaction helpers — shared by buy and sell.
//
//  - build_nonce_tx:     wrap instructions in a durable-nonce tx and sign.
//  - send_transaction:   submit base64 tx to the Helius sender (skipPreflight).
//  - confirm_transaction: poll signature status until confirmed or timeout.
//
// All `pub` — internal to the trader module, called from buy.rs/sell.rs.
// ============================================================

use crate::engine::Engine;
use crate::error::{bail, Context, Result, TradeError};
use base64::{engine::general_purpose, Engine as _};
use serde_json::json;
// `simulate_transaction` (and its config / commitment imports) is only built for
// the cashback `claim` path.
#[cfg(feature = "claim")]
use solana_client::rpc_config::RpcSimulateTransactionConfig;
#[cfg(feature = "claim")]
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::{
    address_lookup_table::AddressLookupTableAccount,
    hash::Hash,
    instruction::{Instruction, InstructionError},
    message::{v0, Message, VersionedMessage},
    signature::{Signature, Signer},
    pubkey::Pubkey,
    transaction::{Transaction, TransactionError, VersionedTransaction},
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

/// Solana's hard wire cap for a single serialized transaction (`PACKET_DATA_SIZE`).
/// A tx over this can't land; the Helius sender rejects it with the opaque
/// `base64 encoded too large`. We check locally before the network round-trip so
/// the failure names the real cause (too many accounts → needs an ALT) and, on
/// the create path, fails BEFORE signing burns the fresh mint keypair.
const PACKET_DATA_SIZE: usize = 1232;

/// Reject an over-limit serialized tx locally with an actionable error instead of
/// letting the sender return `-32602 base64 encoded too large`. Applied to the raw
/// tx wire (bincode), which is what the 1232 B limit governs — not the base64/JSON
/// envelope. Shared by the legacy and versioned send paths.
fn guard_wire_size(wire: &[u8]) -> Result<()> {
    if wire.len() > PACKET_DATA_SIZE {
        return Err(TradeError::Other(format!(
            "transaction is {} B, over the {PACKET_DATA_SIZE} B limit by {} B — too \
             many accounts to fit a single message (use an address lookup table / v0 tx)",
            wire.len(),
            wire.len() - PACKET_DATA_SIZE,
        )));
    }
    Ok(())
}

/// Monotonic JSON-RPC request id. A simple atomic counter replaces a
/// `timestamp_millis` syscall on every `sendTransaction` (the id only needs to
/// be unique per request, not a clock).
static SEND_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

impl Engine {
    // The signer is `&(dyn Signer + Send + Sync)` (not bare `&dyn Signer`) so a
    // `&signer` held across the `.await` in `build_recent_tx` stays `Send` — i.e.
    // the trade futures remain spawnable. The `as &dyn Signer` cast picks the
    // `Signers for [&dyn Signer; 1]` impl when signing.
    pub fn build_nonce_tx(
        &self,
        instructions: Vec<Instruction>,
        nonce_account: &Pubkey,
        nonce_hash: Hash,
        signer: &(dyn Signer + Send + Sync),
    ) -> Result<Transaction> {
        let msg = Message::new_with_nonce(
            instructions,
            Some(&signer.pubkey()),
            nonce_account,
            &signer.pubkey(),
        );
        let mut tx = Transaction::new_unsigned(msg);
        // `try_sign` (vs `sign`) so a remote/HSM signer surfaces a typed error
        // instead of panicking; a durable-nonce signature is fixed locally here.
        tx.try_sign(&[signer as &dyn Signer], nonce_hash)?;
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
    pub async fn build_recent_tx(
        &self,
        instructions: Vec<Instruction>,
        signer: &(dyn Signer + Send + Sync),
    ) -> Result<Transaction> {
        let blockhash = match self
            .blockhash_cache
            .get_fresh(Duration::from_millis(self.config.cache.blockhash_max_age_ms))
        {
            Some(hash) => hash,
            None => self
                .rpc
                .get_latest_blockhash()
                .await
                .context("fetch recent blockhash")?,
        };
        let msg = Message::new(&instructions, Some(&signer.pubkey()));
        let mut tx = Transaction::new_unsigned(msg);
        tx.try_sign(&[signer as &dyn Signer], blockhash)?;
        Ok(tx)
    }

    /// Build + sign a legacy tx against a recent blockhash with multiple signers.
    /// Used by token `create` (wallet + fresh mint keypair). `signers[0]` is the
    /// fee payer.
    pub async fn build_recent_tx_multi(
        &self,
        instructions: Vec<Instruction>,
        signers: &[&(dyn Signer + Send + Sync)],
    ) -> Result<Transaction> {
        if signers.is_empty() {
            bail!("build_recent_tx_multi requires at least one signer");
        }
        let blockhash = match self
            .blockhash_cache
            .get_fresh(Duration::from_millis(self.config.cache.blockhash_max_age_ms))
        {
            Some(hash) => hash,
            None => self
                .rpc
                .get_latest_blockhash()
                .await
                .context("fetch recent blockhash")?,
        };
        let fee_payer = signers[0].pubkey();
        let msg = Message::new(&instructions, Some(&fee_payer));
        let mut tx = Transaction::new_unsigned(msg);
        let refs: Vec<&dyn Signer> = signers.iter().map(|s| *s as &dyn Signer).collect();
        tx.try_sign(&refs, blockhash)?;
        Ok(tx)
    }

    /// Like [`Self::build_recent_tx`] but uses a caller-supplied blockhash so every
    /// tx in a Jito bundle shares the same hash.
    pub async fn build_recent_tx_with_blockhash(
        &self,
        instructions: Vec<Instruction>,
        signer: &(dyn Signer + Send + Sync),
        blockhash: Hash,
    ) -> Result<Transaction> {
        let msg = Message::new(&instructions, Some(&signer.pubkey()));
        let mut tx = Transaction::new_unsigned(msg);
        tx.try_sign(&[signer as &dyn Signer], blockhash)?;
        Ok(tx)
    }

    /// Build + sign a **v0 (versioned)** tx that references `alts` so its account
    /// list is compressed via the lookup table(s). `signers[0]` is the fee payer.
    /// Used by the launch create path: a create_v2 + dev-buy names ~27 accounts,
    /// ~15 of them immutable program IDs / constant PDAs — moving those into an ALT
    /// drops the tx below the 1232 B legacy-message limit it otherwise overflows.
    /// Reads the same background-refreshed blockhash cache as [`build_recent_tx`].
    pub async fn build_recent_v0_tx_multi(
        &self,
        instructions: Vec<Instruction>,
        signers: &[&(dyn Signer + Send + Sync)],
        alts: &[AddressLookupTableAccount],
    ) -> Result<VersionedTransaction> {
        if signers.is_empty() {
            bail!("build_recent_v0_tx_multi requires at least one signer");
        }
        let blockhash = match self
            .blockhash_cache
            .get_fresh(Duration::from_millis(self.config.cache.blockhash_max_age_ms))
        {
            Some(hash) => hash,
            None => self
                .rpc
                .get_latest_blockhash()
                .await
                .context("fetch recent blockhash")?,
        };
        let fee_payer = signers[0].pubkey();
        let msg = v0::Message::try_compile(&fee_payer, &instructions, alts, blockhash)
            .context("compile v0 message")?;
        let refs: Vec<&dyn Signer> = signers.iter().map(|s| *s as &dyn Signer).collect();
        let tx = VersionedTransaction::try_new(VersionedMessage::V0(msg), &refs)
            .context("sign v0 tx")?;
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
    pub fn encode_send_body(&self, tx: &Transaction) -> Result<serde_json::Value> {
        let wire = bincode::serialize(tx)?;
        guard_wire_size(&wire)?;
        Ok(self.encode_send_body_wire(&wire))
    }

    /// Wrap already-serialized tx wire bytes in the `sendTransaction` JSON-RPC
    /// body. The single site both the legacy (`Transaction`) and versioned
    /// (`VersionedTransaction`) encoders funnel through, so the base64 params +
    /// options are identical regardless of tx version.
    fn encode_send_body_wire(&self, wire: &[u8]) -> serde_json::Value {
        let encoded = general_purpose::STANDARD.encode(wire);
        json!({
            "jsonrpc": "2.0",
            "id": SEND_REQUEST_ID.fetch_add(1, Ordering::Relaxed),
            "method": "sendTransaction",
            "params": [
                encoded,
                { "encoding": "base64", "skipPreflight": true, "maxRetries": 0 }
            ]
        })
    }

    /// Simulate a tx (no SOL, no land) and return `(units_consumed, err, logs)`.
    /// `sig_verify: false` + `replace_recent_blockhash: true` so the node swaps in
    /// a current blockhash — the caller doesn't need a fresh hash or valid
    /// signatures, only a correctly-built instruction set. Used by the cashback
    /// claim path to validate accounts / data / CU without sending.
    #[cfg(feature = "claim")]
    pub async fn simulate_transaction(
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

    pub async fn send_transaction(&self, tx: &Transaction) -> Result<String> {
        let body = self.encode_send_body(tx)?;
        // Serialize the JSON-RPC envelope to bytes EXACTLY ONCE here, then share
        // the buffer across every endpoint. The previous `.json(body)` re-walked
        // and re-serialized the (base64-tx-bearing) Value per endpoint; now each
        // request just clones an `Arc<Vec<u8>>` pointer and ships the same bytes.
        let raw = Arc::new(serde_json::to_vec(&body).context("serialize sendTransaction body")?);
        self.send_raw(raw).await
    }

    /// Submit a signed **v0 (versioned)** tx over the same fan-out as
    /// [`Self::send_transaction`]. The launch create path uses this when a launch
    /// ALT is configured. Size-guarded on the tx wire like the legacy path.
    pub async fn send_versioned_transaction(&self, tx: &VersionedTransaction) -> Result<String> {
        let wire = bincode::serialize(tx).context("serialize versioned tx")?;
        guard_wire_size(&wire)?;
        let body = self.encode_send_body_wire(&wire);
        let raw = Arc::new(serde_json::to_vec(&body).context("serialize sendTransaction body")?);
        self.send_raw(raw).await
    }

    /// Fan-out core shared by the legacy and versioned send paths: ships the
    /// pre-serialized JSON-RPC body to every configured sender endpoint and
    /// returns the first acceptance. Single endpoint = a direct await; multiple =
    /// detached concurrent submissions (same signature → on-chain dedup, tip paid
    /// once), first success wins, losers keep running in the background.
    async fn send_raw(&self, raw: Arc<Vec<u8>>) -> Result<String> {
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
                    Err(_) => Err(TradeError::Other(format!(
                        "sender {url} timed out after {FANOUT_SEND_TIMEOUT:?}"
                    ))),
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
        Err(last_err
            .unwrap_or_else(|| TradeError::Other("no sender endpoints configured".into())))
    }

    /// Poll the RPC until the transaction is confirmed or retries are exhausted.
    pub async fn confirm_transaction(&self, signature: &str, max_retries: usize) -> Result<()> {
        let sig = Signature::from_str(signature)?;

        // Poll status first, then sleep *between* polls (not before the first and
        // not after the last) — so a tx that already landed returns immediately
        // and the worst-case wait is `(max_retries - 1) × CONFIRM_POLL_MS`.
        for i in 0..max_retries {
            match self.rpc.get_signature_status(&sig).await? {
                Some(Ok(())) => return Ok(()),
                // A tx that LANDED and reverted — carries the program's custom
                // (Anchor) error code when present, so the sell retry path can
                // tell a structural revert from a transient min-out miss.
                Some(Err(e)) => {
                    return Err(TradeError::Reverted {
                        custom: custom_error_code(&e),
                    })
                }
                None => {
                    if i + 1 < max_retries {
                        info!("⏳ Confirmation pending ({}/{})", i + 1, max_retries);
                        // Ramp the early polls (a confirmed tx usually lands in
                        // ~1–2 slots); fall back to the steady gap past the ramp.
                        let gap = self
                            .config
                            .retry
                            .confirm_poll_schedule_ms
                            .get(i)
                            .copied()
                            .unwrap_or(self.config.retry.confirm_poll_ms);
                        tokio::time::sleep(Duration::from_millis(gap)).await;
                    }
                }
            }
        }

        Err(TradeError::ConfirmTimeout)
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
pub fn custom_error_code(err: &TransactionError) -> Option<u32> {
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
#[cfg(feature = "probe")]
pub async fn post_tx(
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
pub async fn post_tx_bytes(
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
        bail!("HTTP error from sender {url}: {}", resp.text().await?);
    }

    let json: serde_json::Value = resp.json().await?;
    if let Some(err) = json.get("error") {
        bail!("JSON-RPC error from {url}: {err:?}");
    }

    json.get("result")
        .and_then(|r| r.as_str())
        .map(str::to_string)
        .context("No signature in response")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TraderConfig;
    use crate::engine::Engine;
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

    fn trader_with(urls: Vec<String>) -> Engine {
        // The send fan-out only touches `config.helius_sender_urls` + `http`, so a
        // bare engine (no init, dummy tip account / rent) is enough to drive it.
        Engine::new(
            Arc::new(TraderConfig::new(
                "http://localhost".into(),
                urls,
                Arc::new(Keypair::new()),
                vec![solana_sdk::pubkey::Pubkey::new_unique()],
            )),
            solana_sdk::pubkey::Pubkey::new_unique(),
            0,
            0,
            0,
            0,
        )
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
