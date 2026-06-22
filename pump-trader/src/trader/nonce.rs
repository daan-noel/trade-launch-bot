// ============================================================
// Nonce management.
//
//  - acquire_nonce:          round-robin pick of a free, cached slot
//                            (spin-waits if all in use). Hot path.
//  - schedule_nonce_refresh: after a send, refresh the slot's blockhash
//                            in the background and clear in_use.
//  - fetch_nonce_hash_async: one-shot blockhash fetch (used at init and
//                            by the refresh path).
// ============================================================

use super::PumpFunTrader;
use crate::constants::{
    NONCE_MAX_WAIT_ITERS, NONCE_REFRESH_MAX_ATTEMPTS, NONCE_REFRESH_RETRY_MS, NONCE_WAIT_SLEEP_MS,
};
use anyhow::{Context, Result};
use solana_client::nonce_utils;
use solana_sdk::signature::Signer;
use solana_sdk::{hash::Hash, nonce::State, pubkey::Pubkey};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

/// Read-only audit result for one configured nonce account.
pub struct NonceAuthCheck {
    pub pubkey: Pubkey,
    /// `false` if the account is missing or not an initialized nonce account.
    pub initialized: bool,
    /// The nonce-authority stored on-chain, if the account is an initialized nonce.
    pub authority: Option<Pubkey>,
    /// `true` iff `authority == trading wallet` — i.e. this slot can be advanced
    /// by a tx signed with the current wallet key.
    pub matches_wallet: bool,
    /// Populated when the RPC read or state decode failed.
    pub error: Option<String>,
}

impl PumpFunTrader {
    /// Acquire the next available nonce slot (round-robin, spin-wait if all busy).
    pub(super) async fn acquire_nonce(&self) -> Result<(Pubkey, Hash)> {
        if self.nonce_pubkeys.is_empty() {
            anyhow::bail!("No nonce accounts configured");
        }

        let mut waited = 0usize;

        for _ in 0..NONCE_MAX_WAIT_ITERS {
            // Arm the wakeup BEFORE scanning, so a slot freed between our failed
            // scan and the await below still wakes us (no lost notification).
            let freed = self.nonce_available.notified();

            {
                let mut slots = self
                    .nonce_slots
                    .lock()
                    .unwrap_or_else(|p| p.into_inner());
                let start =
                    self.nonce_cursor.fetch_add(1, Ordering::Relaxed) % self.nonce_pubkeys.len();

                for offset in 0..self.nonce_pubkeys.len() {
                    let pk = self.nonce_pubkeys[(start + offset) % self.nonce_pubkeys.len()];
                    if let Some(slot) = slots.get_mut(&pk) {
                        if !slot.in_use {
                            if let Some(hash) = slot.cached_hash {
                                slot.in_use = true;
                                if waited > 0 {
                                    let events =
                                        self.nonce_wait_events.fetch_add(1, Ordering::Relaxed) + 1;
                                    self.nonce_wait_iters_total
                                        .fetch_add(waited, Ordering::Relaxed);
                                    if events % 50 == 0 {
                                        let avg = self
                                            .nonce_wait_iters_total
                                            .load(Ordering::Relaxed)
                                            as f64
                                            / events as f64;
                                        info!(
                                            "📊 Nonce wait: events={} avg_iters={:.1}",
                                            events, avg
                                        );
                                    }
                                }
                                return Ok((pk, hash));
                            }
                        }
                    }
                }
            }

            waited += 1;
            // Event-driven wait: wake the instant a slot is refreshed free, but
            // cap each wait so we still re-scan periodically as a safety net (and
            // to pick up a slot whose hash arrives without a notify race).
            let _ = tokio::time::timeout(
                Duration::from_millis(NONCE_WAIT_SLEEP_MS),
                freed,
            )
            .await;
        }

        anyhow::bail!("All nonce slots busy after {} iters", NONCE_MAX_WAIT_ITERS)
    }

    /// After a tx is sent, refresh the nonce hash in the background and clear in_use.
    pub(super) fn schedule_nonce_refresh(&self, nonce_pubkey: Pubkey) {
        let rpc = Arc::clone(&self.rpc);
        let slots = Arc::clone(&self.nonce_slots);
        let available = Arc::clone(&self.nonce_available);

        tokio::spawn(async move {
            // The hash the just-sent tx spent — still cached (the slot stayed
            // `in_use`, so nobody else touched it). We must NOT re-arm the slot
            // with this same hash: once the in-flight tx lands it consumes the
            // nonce and advances the on-chain blockhash, so a retry built on the
            // old hash is a guaranteed-fail tx (burned retry + escalated tip).
            let used_hash: Option<Hash> = {
                let guard = slots.lock().unwrap_or_else(|p| p.into_inner());
                guard.get(&nonce_pubkey).and_then(|s| s.cached_hash)
            };

            // Re-read the nonce account until its blockhash actually advances past
            // `used_hash` (i.e. the spend is visible — robust to the read commitment
            // lagging the tx's landing slot). If it never advances within the
            // window, the in-flight tx almost certainly didn't land, so the old
            // hash is still valid and safe to re-arm; we fall back to it.
            let mut advanced: Option<Hash> = None;
            let mut last_ok: Option<Hash> = None;
            for attempt in 0..NONCE_REFRESH_MAX_ATTEMPTS {
                let result: anyhow::Result<Hash> = async {
                    let account = rpc
                        .get_account(&nonce_pubkey)
                        .await
                        .with_context(|| format!("get_account failed for {}", nonce_pubkey))?;
                    match nonce_utils::state_from_account(&account)? {
                        State::Initialized(data) => Ok(data.blockhash()),
                        _ => anyhow::bail!("Nonce account {} not initialized", nonce_pubkey),
                    }
                }
                .await;

                match result {
                    Ok(hash) => {
                        last_ok = Some(hash);
                        if used_hash != Some(hash) {
                            advanced = Some(hash);
                            break;
                        }
                    }
                    Err(e) => {
                        warn!(
                            "⚠️  Failed to refresh nonce {} (attempt {}): {}",
                            nonce_pubkey, attempt, e
                        );
                    }
                }
                tokio::time::sleep(Duration::from_millis(NONCE_REFRESH_RETRY_MS)).await;
            }

            let mut guard = slots.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(slot) = guard.get_mut(&nonce_pubkey) {
                // Prefer the advanced hash; else the last good read (== the old
                // hash, valid because the tx didn't consume it). Only when every
                // read errored do we drop the hash (re-fetched fresh on next use).
                match advanced.or(last_ok) {
                    Some(hash) => slot.cached_hash = Some(hash),
                    None => {
                        warn!("⚠️  Nonce {} refresh: all reads failed; clearing hash", nonce_pubkey);
                        slot.cached_hash = None;
                    }
                }
                slot.in_use = false;
            }
            drop(guard);
            // Wake one waiter now that a slot is free again. On a refresh error
            // the slot has no hash, but the waiter re-scans and simply waits on
            // the next free slot — no worse than the old fixed-sleep poll.
            available.notify_one();
        });
    }

    /// Read-only audit of **every** configured nonce account: fetch each on-chain,
    /// decode its state, and report whether its stored authority is the current
    /// trading wallet. Zero SOL, no tx. Unlike a fan-out probe (which only
    /// exercises whichever slots round-robin hands out), this covers the full
    /// pool — so a re-authorization can be confirmed to have landed on all of
    /// them. A mismatched authority means a durable-nonce tx on that slot would
    /// fail `advance_nonce_account`.
    pub async fn check_nonce_authorities(&self) -> Vec<NonceAuthCheck> {
        let wallet = self.config.keypair.pubkey();
        let mut out = Vec::with_capacity(self.nonce_pubkeys.len());
        for pk in &self.nonce_pubkeys {
            let check = match self.rpc.get_account(pk).await {
                Ok(account) => match nonce_utils::state_from_account(&account) {
                    Ok(State::Initialized(data)) => NonceAuthCheck {
                        pubkey: *pk,
                        initialized: true,
                        authority: Some(data.authority),
                        matches_wallet: data.authority == wallet,
                        error: None,
                    },
                    Ok(_) => NonceAuthCheck {
                        pubkey: *pk,
                        initialized: false,
                        authority: None,
                        matches_wallet: false,
                        error: Some("account is not an initialized nonce".to_string()),
                    },
                    Err(e) => NonceAuthCheck {
                        pubkey: *pk,
                        initialized: false,
                        authority: None,
                        matches_wallet: false,
                        error: Some(format!("decode nonce state: {e}")),
                    },
                },
                Err(e) => NonceAuthCheck {
                    pubkey: *pk,
                    initialized: false,
                    authority: None,
                    matches_wallet: false,
                    error: Some(format!("get_account: {e}")),
                },
            };
            out.push(check);
        }
        out
    }

    pub async fn fetch_nonce_hash_async(&self, pubkey: &Pubkey) -> Result<Hash> {
        let account = self
            .rpc
            .get_account(pubkey)
            .await
            .with_context(|| format!("Failed to fetch nonce account {}", pubkey))?;
        match nonce_utils::state_from_account(&account)? {
            State::Initialized(data) => Ok(data.blockhash()),
            _ => anyhow::bail!("Nonce account {} not initialized", pubkey),
        }
    }
}
