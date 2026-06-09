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
use crate::constants::{NONCE_MAX_WAIT_ITERS, NONCE_WAIT_SLEEP_MS};
use anyhow::{Context, Result};
use solana_client::nonce_utils;
use solana_sdk::{hash::Hash, nonce::State, pubkey::Pubkey};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

impl PumpFunTrader {
    /// Acquire the next available nonce slot (round-robin, spin-wait if all busy).
    pub(super) async fn acquire_nonce(&self) -> Result<(Pubkey, Hash)> {
        if self.nonce_pubkeys.is_empty() {
            anyhow::bail!("No nonce accounts configured");
        }

        let mut waited = 0usize;

        for _ in 0..NONCE_MAX_WAIT_ITERS {
            let mut slots = self.nonce_slots.lock().await;
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
                                    let avg = self.nonce_wait_iters_total.load(Ordering::Relaxed)
                                        as f64
                                        / events as f64;
                                    info!("📊 Nonce wait: events={} avg_iters={:.1}", events, avg);
                                }
                            }
                            return Ok((pk, hash));
                        }
                    }
                }
            }

            drop(slots);
            waited += 1;
            tokio::time::sleep(Duration::from_millis(NONCE_WAIT_SLEEP_MS)).await;
        }

        anyhow::bail!("All nonce slots busy after {} iters", NONCE_MAX_WAIT_ITERS)
    }

    /// After a tx is sent, refresh the nonce hash in the background and clear in_use.
    pub(super) fn schedule_nonce_refresh(&self, nonce_pubkey: Pubkey) {
        let rpc = Arc::clone(&self.rpc);
        let slots = Arc::clone(&self.nonce_slots);

        tokio::spawn(async move {
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

            let mut guard = slots.lock().await;
            if let Some(slot) = guard.get_mut(&nonce_pubkey) {
                match result {
                    Ok(hash) => slot.cached_hash = Some(hash),
                    Err(e) => {
                        warn!("⚠️  Failed to refresh nonce {}: {}", nonce_pubkey, e);
                        slot.cached_hash = None;
                    }
                }
                slot.in_use = false;
            }
        });
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
