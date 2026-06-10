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
use crate::constants::{BLOCKHASH_CACHE_MAX_AGE_MS, CONFIRM_POLL_MS};
use anyhow::{Context, Result};
use base64::{engine::general_purpose, Engine as _};
use serde_json::json;
use solana_sdk::{
    hash::Hash,
    instruction::Instruction,
    message::Message,
    signature::{Keypair, Signature, Signer},
    pubkey::Pubkey,
    transaction::Transaction,
};
use std::str::FromStr;
use std::time::Duration;
use tracing::info;

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

    pub(super) async fn send_transaction(&self, tx: &Transaction) -> Result<String> {
        let encoded = general_purpose::STANDARD.encode(bincode::serialize(tx)?);

        let body = json!({
            "jsonrpc": "2.0",
            "id": chrono::Utc::now().timestamp_millis().to_string(),
            "method": "sendTransaction",
            "params": [
                encoded,
                { "encoding": "base64", "skipPreflight": true, "maxRetries": 0 }
            ]
        });

        let resp = self
            .http
            .post(&self.config.helius_sender_url)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            anyhow::bail!("HTTP error from sender: {}", resp.text().await?);
        }

        let json: serde_json::Value = resp.json().await?;

        if let Some(err) = json.get("error") {
            anyhow::bail!("JSON-RPC error: {:?}", err);
        }

        json.get("result")
            .and_then(|r| r.as_str())
            .map(str::to_string)
            .context("No signature in response")
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
                Some(Err(e)) => anyhow::bail!("Transaction failed on-chain: {:?}", e),
                None => {
                    if i + 1 < max_retries {
                        info!("⏳ Confirmation pending ({}/{})", i + 1, max_retries);
                        tokio::time::sleep(Duration::from_millis(CONFIRM_POLL_MS)).await;
                    }
                }
            }
        }

        anyhow::bail!("Confirmation timed out for {}", signature)
    }
}
