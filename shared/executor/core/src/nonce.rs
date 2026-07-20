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

use crate::engine::Engine;
use crate::error::{bail, Context, Result};
use solana_client::nonce_utils;
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

impl Engine {
    /// Acquire the next available nonce slot (round-robin, spin-wait if all busy).
    pub async fn acquire_nonce(&self) -> Result<(Pubkey, Hash)> {
        if self.nonce_pubkeys.is_empty() {
            bail!("No nonce accounts configured");
        }

        let max_wait_iters = self.config.nonce.max_wait_iters;
        let wait_sleep_ms = self.config.nonce.wait_sleep_ms;
        let mut waited = 0usize;

        for _ in 0..max_wait_iters {
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
                                // New use — lets the pending refresh poll (if
                                // any) detect it must not touch this slot.
                                slot.use_epoch = slot.use_epoch.wrapping_add(1);
                                if waited > 0 {
                                    let events =
                                        self.nonce_wait_events.fetch_add(1, Ordering::Relaxed) + 1;
                                    self.nonce_wait_iters_total
                                        .fetch_add(waited, Ordering::Relaxed);
                                    // Every wait event is worth surfacing — a single
                                    // contention hit means the pool is close to its
                                    // capacity for current trade throughput.
                                    warn!(
                                        wait_iters = waited,
                                        total_events = events,
                                        "⏳ Nonce contention: waited {waited} iters for a free slot \
                                         (lifetime events={events})"
                                    );
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
            let _ = tokio::time::timeout(Duration::from_millis(wait_sleep_ms), freed).await;
        }

        // Every busy-bail is a signal the pool is undersized for current
        // trade throughput. Check nonce pool size and consider adding accounts
        // (mind the EC2 connection-count guardrail — new pools require
        // shrinking something else).
        warn!(
            nonce_pool_size = self.nonce_pubkeys.len(),
            max_wait_iters,
            "🚨 Nonce pool exhausted — all slots busy after {max_wait_iters} iters; \
             buy/sell blocked. If this is frequent, the nonce pool needs resizing."
        );
        bail!("All nonce slots busy after {max_wait_iters} iters")
    }

    /// After a tx is sent, refresh the nonce hash in the background and clear in_use.
    ///
    /// With a host-bridged nonce-account push feed (`on_nonce_account_update`),
    /// this poll is a FALLBACK only: the push normally re-arms the slot at feed
    /// speed, and this task notices (epoch/in_use check) and exits without
    /// spending any RPC read. `nonce.refresh_first_delay_ms` (0 by default, set
    /// by push-fed hosts) delays the first read into the window where the push
    /// has normally already won.
    pub fn schedule_nonce_refresh(&self, nonce_pubkey: Pubkey) {
        let rpc = Arc::clone(&self.rpc);
        let slots = Arc::clone(&self.nonce_slots);
        let available = Arc::clone(&self.nonce_available);
        let refresh_max_attempts = self.config.nonce.refresh_max_attempts;
        let refresh_retry_ms = self.config.nonce.refresh_retry_ms;
        let first_delay_ms = self.config.nonce.refresh_first_delay_ms;

        tokio::spawn(async move {
            // The hash the just-sent tx spent — still cached (the slot stayed
            // `in_use`, so nobody else touched it). We must NOT re-arm the slot
            // with this same hash: once the in-flight tx lands it consumes the
            // nonce and advances the on-chain blockhash, so a retry built on the
            // old hash is a guaranteed-fail tx (burned retry + escalated tip).
            // `epoch` identifies OUR use of the slot: if it changes (push re-armed
            // it and another tx acquired it), this task must not touch the slot.
            let (used_hash, epoch): (Option<Hash>, u64) = {
                let guard = slots.lock().unwrap_or_else(|p| p.into_inner());
                match guard.get(&nonce_pubkey) {
                    Some(s) => (s.cached_hash, s.use_epoch),
                    None => return,
                }
            };

            if first_delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(first_delay_ms)).await;
            }

            // Re-read the nonce account until its blockhash actually advances past
            // `used_hash` (i.e. the spend is visible — robust to the read commitment
            // lagging the tx's landing slot). If it never advances within the
            // window, the in-flight tx almost certainly didn't land, so the old
            // hash is still valid and safe to re-arm; we fall back to it.
            let mut advanced: Option<Hash> = None;
            let mut last_ok: Option<Hash> = None;
            for attempt in 0..refresh_max_attempts {
                // Push already re-armed this use (or a new use started) — done,
                // and every remaining RPC read is saved.
                {
                    let guard = slots.lock().unwrap_or_else(|p| p.into_inner());
                    match guard.get(&nonce_pubkey) {
                        Some(s) if s.use_epoch == epoch && s.in_use => {}
                        _ => return,
                    }
                }

                let result: Result<Hash> = async {
                    let account = rpc
                        .get_account(&nonce_pubkey)
                        .await
                        .with_context(|| format!("get_account failed for {}", nonce_pubkey))?;
                    match nonce_utils::state_from_account(&account)? {
                        State::Initialized(data) => Ok(data.blockhash()),
                        _ => bail!("Nonce account {} not initialized", nonce_pubkey),
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
                tokio::time::sleep(Duration::from_millis(refresh_retry_ms)).await;
            }

            let mut guard = slots.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(slot) = guard.get_mut(&nonce_pubkey) {
                // Same guard as inside the loop: if the push feed re-armed this
                // use (and possibly a new tx acquired the slot), writing here
                // would free a slot that's genuinely busy — leave it alone.
                if slot.use_epoch != epoch || !slot.in_use {
                    return;
                }
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

    /// Push-based nonce re-arm: feed one subscribed nonce-account update
    /// (`accounts` filter on the LaserStream subscription) into the slot cache.
    /// `data` is the raw account data; `slot` the update's slot (gates out
    /// replayed/out-of-order updates on reconnect).
    ///
    /// Semantics per state:
    /// - blockhash unchanged → no-op (startup snapshot, or our tx hasn't landed).
    /// - blockhash advanced while `in_use` → the in-flight tx landed: cache the
    ///   new hash and re-arm the slot at feed speed (the fallback poll in
    ///   [`Self::schedule_nonce_refresh`] then exits without any RPC read).
    /// - blockhash advanced while free → external advance; just refresh the cache.
    pub fn on_nonce_account_update(&self, pubkey: &Pubkey, data: &[u8], slot: u64) {
        // Not one of ours / not yet initialized — ignore silently (the accounts
        // filter should only carry our pool, but stay defensive).
        if !self.nonce_pubkeys.contains(pubkey) {
            return;
        }
        // Decode the durable-nonce state from the raw pushed bytes via the same
        // `nonce_utils` decode the RPC path uses (wrap in a system-owned Account).
        let account = solana_sdk::account::Account {
            lamports: 1,
            data: data.to_vec(),
            owner: solana_sdk::system_program::id(),
            executable: false,
            rent_epoch: 0,
        };
        let hash = match nonce_utils::state_from_account(&account) {
            Ok(State::Initialized(d)) => d.blockhash(),
            _ => return,
        };

        let rearmed = {
            let mut guard = self.nonce_slots.lock().unwrap_or_else(|p| p.into_inner());
            let Some(s) = guard.get_mut(pubkey) else {
                return;
            };
            if slot <= s.last_push_slot {
                return; // replay / out-of-order
            }
            s.last_push_slot = slot;
            if s.cached_hash == Some(hash) {
                return; // no advance visible yet
            }
            s.cached_hash = Some(hash);
            if s.in_use {
                s.in_use = false;
                true
            } else {
                false
            }
        };
        if rearmed {
            self.nonce_available.notify_one();
        }
    }

    /// Read-only audit of **every** configured nonce account: fetch each on-chain,
    /// decode its state, and report whether its stored authority is the current
    /// trading wallet. Zero SOL, no tx. Unlike a fan-out probe (which only
    /// exercises whichever slots round-robin hands out), this covers the full
    /// pool — so a re-authorization can be confirmed to have landed on all of
    /// them. A mismatched authority means a durable-nonce tx on that slot would
    /// fail `advance_nonce_account`.
    pub async fn check_nonce_authorities(&self) -> Vec<NonceAuthCheck> {
        let wallet = self.config.signer.pubkey();
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
            _ => bail!("Nonce account {} not initialized", pubkey),
        }
    }

    /// Pre-fetch nonce blockhashes for many accounts in one `getMultipleAccounts`
    /// (chunked at the 100-key RPC cap) rather than one `getAccount` each — the
    /// durable-nonce startup prefetch. Bails if any account is missing or is not an
    /// initialized nonce (same contract as [`fetch_nonce_hash_async`]).
    pub async fn prefetch_nonce_hashes(&self, pubkeys: &[Pubkey]) -> Result<Vec<(Pubkey, Hash)>> {
        let mut out = Vec::with_capacity(pubkeys.len());
        for chunk in pubkeys.chunks(100) {
            let accounts = self
                .rpc
                .get_multiple_accounts(chunk)
                .await
                .context("Failed to batch-fetch nonce accounts")?;
            for (pk, acct) in chunk.iter().zip(accounts) {
                let Some(account) = acct else {
                    bail!("Nonce account {} not found", pk);
                };
                match nonce_utils::state_from_account(&account)? {
                    State::Initialized(data) => out.push((*pk, data.blockhash())),
                    _ => bail!("Nonce account {} not initialized", pk),
                }
            }
        }
        Ok(out)
    }
}
