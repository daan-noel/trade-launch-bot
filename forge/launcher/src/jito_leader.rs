//! Jito leader-schedule gate — fire a launch bundle only when a Jito-participating
//! validator is (about to be) the slot leader.
//!
//! A Jito bundle can ONLY land when a Jito validator builds the block, i.e. is the
//! slot leader. Submitting into a non-Jito leader slot drops the bundle *regardless
//! of tip* — the measured, dominant cause of our launch drops (a 0.005 SOL tip, 70x
//! market p99, still dropped 0 trades because the tip was never the problem).
//!
//! **How it knows.** Jito's `getNextScheduledLeader` is NOT exposed on the public
//! block-engine HTTP API (only `sendBundle` / `getBundleStatuses` / `getTipAccounts`
//! are), so this derives the same answer from two SOL-free public reads:
//!   1. Solana RPC `getSlotLeaders(current, N)` → the leader *identity* for each of
//!      the next N slots.
//!   2. The Jito validator identity set (StakeNet feed, `validators_url`) → who runs
//!      the Jito client and can build bundles.
//! If a Jito identity leads within `send_within_slots`, submit now; else wait
//! (bounded) for the nearest Jito slot and re-poll.
//!
//! **Fail-open by construction.** The gate is an optimization, never a hard
//! dependency: any RPC/parse error, a disabled gate, an empty validator set, or an
//! exhausted wait budget returns immediately so a launch is never blocked by a gate
//! failure — worst case reverts to ungated submit.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tracing::{debug, warn};

use crate::config::LeaderGateConfig;

/// Approximate mainnet slot time (ms). Used only to *estimate* how long to sleep
/// until the next Jito leader slot; the loop re-polls `getSlot` afterward, so any
/// drift in this constant self-corrects rather than compounding.
const SLOT_MS: u64 = 450;

/// How many upcoming slots to scan for a Jito leader each poll. Leaders lead in
/// runs of 4 consecutive slots, so 64 slots ≈ 16 distinct leaders — a wide enough
/// horizon that a Jito validator (a large fraction of stake) is almost always found,
/// while keeping the `getSlotLeaders` response small.
const LOOKAHEAD_SLOTS: u64 = 64;

/// Cap on the per-level wait-budget multiplier. A re-bid climbs the tip ladder AND
/// hunts a Jito leader harder (it has already burned a confirm timeout, so wasting
/// its escalated tip on a non-Jito slot is the exact failure to avoid) — but the
/// hunt is still bounded so a launch can't wait unboundedly across many re-bids.
const MAX_WAIT_LEVEL_MULT: u64 = 4;

/// TTL for the cached Jito validator identity set (StakeNet membership changes
/// slowly — epoch-scale). Refreshing at most this often keeps the launch path off a
/// per-submit StakeNet HTTP GET without ever going stale for long.
const JITO_IDENTITIES_TTL: Duration = Duration::from_secs(600);

/// Cached Jito validator identity set (base58 identities), refreshed on [`JITO_IDENTITIES_TTL`].
type JitoIdentityCache = OnceLock<Mutex<Option<(Instant, Arc<HashSet<String>>)>>>;
static JITO_IDENTITIES: JitoIdentityCache = OnceLock::new();

/// The per-epoch leader schedule reduced to just the Jito-led slot indices. Built
/// once per epoch from `getLeaderSchedule` ∩ the Jito identity set, then every poll
/// is a local lookup instead of a `getSlotLeaders` RPC. `jito_indices` holds only
/// Jito-led indices (a fraction of the ~432k-slot epoch), so it stays a few MB — a
/// bounded per-epoch cache, not a raised cap.
struct EpochLeaders {
    epoch: u64,
    /// Slot indices (relative to the epoch's first slot) led by a Jito validator.
    jito_indices: Arc<HashSet<u64>>,
}

static EPOCH_LEADERS: OnceLock<Mutex<Option<EpochLeaders>>> = OnceLock::new();

/// Block until a Jito-participating validator leads within `send_within_slots` of the
/// current slot (so a `sendBundle` now lands in a slot that can build the bundle), or
/// until the wait budget is spent — whichever comes first. See the module docs for
/// the fail-open contract: this NEVER prevents a submit, it only delays one (bounded)
/// to a slot that can actually land it.
///
/// `rpc_url` is the Solana RPC (Helius) for `getSlot`/`getSlotLeaders`. `level` is the
/// tip-escalation level (0 = first attempt; a confirm-watcher re-bid passes its
/// `submit_attempts`), scaling the wait budget so each escalating re-bid hunts a Jito
/// leader harder — pairing the tip ladder with the gate — capped at
/// [`MAX_WAIT_LEVEL_MULT`]× so the total hunt across re-bids stays bounded.
pub async fn wait_for_jito_leader(cfg: &LeaderGateConfig, rpc_url: &str, level: u8) {
    if !cfg.enabled {
        return;
    }
    let http = reqwest::Client::new();

    // Jito validator identity set (cached with a TTL). Empty ⇒ we can't classify a
    // leader → fail open and submit immediately.
    let jito = jito_identities_cached(&http, &cfg.validators_url).await;
    if jito.is_empty() {
        warn!("Jito validator set empty/unavailable — submitting ungated");
        return;
    }

    let start = Instant::now();
    let mult = (level as u64 + 1).min(MAX_WAIT_LEVEL_MULT);
    let budget = Duration::from_millis(cfg.max_wait_ms.saturating_mul(mult));

    loop {
        // One `getEpochInfo` gives the current slot, epoch, and in-epoch index — the
        // epoch drives the per-epoch leader-schedule cache; the index drives the lookup.
        let (epoch, current, slot_index) = match fetch_epoch_info(&http, rpc_url).await {
            Ok(t) => t,
            Err(e) => {
                warn!(%e, "getEpochInfo failed — submitting ungated");
                return;
            }
        };
        let epoch_first_slot = current.saturating_sub(slot_index);

        // Offset (in slots from `current`) of the nearest Jito-led slot, if any. Prefer
        // the cached per-epoch schedule (local lookup, no RPC); fall back to a fresh
        // `getSlotLeaders` when the schedule is unavailable (fail-open).
        let nearest = match epoch_leaders_cached(&http, rpc_url, epoch, epoch_first_slot, &jito)
            .await
        {
            Some(jito_indices) => (0..LOOKAHEAD_SLOTS)
                .find(|off| jito_indices.contains(&(slot_index + off))),
            None => match fetch_slot_leaders(&http, rpc_url, current, LOOKAHEAD_SLOTS).await {
                Ok(leaders) => leaders.iter().position(|id| jito.contains(id)).map(|p| p as u64),
                Err(e) => {
                    warn!(%e, "getSlotLeaders failed — submitting ungated");
                    return;
                }
            },
        };
        match nearest {
            Some(offset) if offset <= cfg.send_within_slots => {
                debug!(
                    offset,
                    waited_ms = start.elapsed().as_millis(),
                    "Jito leader within window — releasing bundle submit"
                );
                return;
            }
            _ => {}
        }

        let Some(remaining) = budget.checked_sub(start.elapsed()) else {
            warn!(
                budget_ms = budget.as_millis(),
                level, "Jito leader-schedule wait budget spent — submitting anyway (no leader in window)"
            );
            return;
        };
        // Sleep toward the nearest Jito slot (or one slot if none found in the
        // lookahead), bounded by the remaining budget.
        let slots_out = nearest
            .map(|o| o.saturating_sub(cfg.send_within_slots))
            .unwrap_or(1)
            .max(1);
        let sleep_for = Duration::from_millis(slots_out * SLOT_MS).min(remaining);
        if sleep_for.is_zero() {
            return;
        }
        tokio::time::sleep(sleep_for).await;
    }
}

/// Jito validator identity set, TTL-cached ([`JITO_IDENTITIES_TTL`]). Returns the
/// warm set when fresh; otherwise refetches. On a fetch failure it returns the last
/// cached set (even if stale) so a transient StakeNet blip doesn't drop the gate to
/// ungated — only a cold cache with a failing fetch yields an empty set.
///
/// Double-checked: the mutex is **not** held across the HTTP fetch so concurrent
/// bundle submits are not serialized behind a slow StakeNet refresh.
async fn jito_identities_cached(http: &reqwest::Client, url: &str) -> Arc<HashSet<String>> {
    let cell = JITO_IDENTITIES.get_or_init(|| Mutex::new(None));
    let stale_fallback = {
        let guard = cell.lock().await;
        if let Some((at, set)) = guard.as_ref() {
            if at.elapsed() < JITO_IDENTITIES_TTL {
                return set.clone();
            }
            Some(set.clone())
        } else {
            None
        }
    };
    match fetch_jito_identities(http, url).await {
        Ok(set) if !set.is_empty() => {
            let arc = Arc::new(set);
            let mut guard = cell.lock().await;
            // Another caller may have refreshed while we were fetching.
            if let Some((at, set)) = guard.as_ref() {
                if at.elapsed() < JITO_IDENTITIES_TTL {
                    return set.clone();
                }
            }
            *guard = Some((Instant::now(), arc.clone()));
            arc
        }
        Ok(_) => stale_fallback.unwrap_or_default(),
        Err(e) => {
            warn!(%e, "Jito validator-set refresh failed — reusing cached set if any");
            stale_fallback.unwrap_or_default()
        }
    }
}

/// The current epoch's Jito-led slot-index set, cached and rebuilt only on epoch
/// rollover. Returns `None` on a `getLeaderSchedule` failure so the caller falls back
/// to `getSlotLeaders` (fail-open). The intersection is computed against `jito` at
/// build time, so a mid-epoch identity refresh isn't reflected until the next epoch —
/// acceptable for a fail-open gate (worst case: an occasional non-Jito-slot submit).
async fn epoch_leaders_cached(
    http: &reqwest::Client,
    rpc_url: &str,
    epoch: u64,
    epoch_first_slot: u64,
    jito: &HashSet<String>,
) -> Option<Arc<HashSet<u64>>> {
    let cell = EPOCH_LEADERS.get_or_init(|| Mutex::new(None));
    {
        let guard = cell.lock().await;
        if let Some(cached) = guard.as_ref() {
            if cached.epoch == epoch {
                return Some(cached.jito_indices.clone());
            }
        }
    }
    match fetch_leader_schedule(http, rpc_url, epoch_first_slot).await {
        Ok(schedule) => {
            // Keep only the slot indices led by a Jito validator (a fraction of the epoch).
            let mut jito_indices: HashSet<u64> = HashSet::new();
            for (identity, indices) in &schedule {
                if jito.contains(identity) {
                    jito_indices.extend(indices.iter().copied());
                }
            }
            let arc = Arc::new(jito_indices);
            let mut guard = cell.lock().await;
            if let Some(cached) = guard.as_ref() {
                if cached.epoch == epoch {
                    return Some(cached.jito_indices.clone());
                }
            }
            *guard = Some(EpochLeaders {
                epoch,
                jito_indices: arc.clone(),
            });
            Some(arc)
        }
        Err(e) => {
            warn!(%e, "getLeaderSchedule failed — falling back to getSlotLeaders this poll");
            None
        }
    }
}

/// Fetch the Jito StakeNet validator identity set — the pubkeys that run the Jito
/// client and can build bundles. One HTTP GET, parsed to a `HashSet` for O(1)
/// leader membership tests.
async fn fetch_jito_identities(http: &reqwest::Client, url: &str) -> Result<HashSet<String>> {
    #[derive(Deserialize)]
    struct Feed {
        validators: Vec<ValidatorRow>,
    }
    #[derive(Deserialize)]
    struct ValidatorRow {
        identity_account: String,
    }
    let feed: Feed = http
        .get(url)
        .send()
        .await
        .context("Jito validators HTTP")?
        .error_for_status()
        .context("Jito validators status")?
        .json()
        .await
        .context("parse Jito validators feed")?;
    Ok(feed.validators.into_iter().map(|v| v.identity_account).collect())
}

/// `getSlotLeaders(start, limit)` → the leader identity (base58) for each of `limit`
/// consecutive slots beginning at `start`.
async fn fetch_slot_leaders(
    http: &reqwest::Client,
    rpc_url: &str,
    start: u64,
    limit: u64,
) -> Result<Vec<String>> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getSlotLeaders",
        "params": [start, limit]
    });
    let v = rpc_call(http, rpc_url, &body).await?;
    let arr = v
        .get("result")
        .and_then(|r| r.as_array())
        .context("getSlotLeaders result not an array")?;
    Ok(arr
        .iter()
        .filter_map(|x| x.as_str().map(str::to_string))
        .collect())
}

/// `getEpochInfo` → `(epoch, absolute_slot, slot_index)`. One call gives the current
/// slot (for the wait/leader window), the epoch (schedule-cache key), and the in-epoch
/// slot index (schedule lookup) — replacing a separate `getSlot`.
async fn fetch_epoch_info(http: &reqwest::Client, rpc_url: &str) -> Result<(u64, u64, u64)> {
    let body = serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "getEpochInfo", "params": [] });
    let v = rpc_call(http, rpc_url, &body).await?;
    let r = v.get("result").context("getEpochInfo result missing")?;
    let epoch = r.get("epoch").and_then(|x| x.as_u64()).context("getEpochInfo epoch")?;
    let absolute_slot = r
        .get("absoluteSlot")
        .and_then(|x| x.as_u64())
        .context("getEpochInfo absoluteSlot")?;
    let slot_index = r
        .get("slotIndex")
        .and_then(|x| x.as_u64())
        .context("getEpochInfo slotIndex")?;
    Ok((epoch, absolute_slot, slot_index))
}

/// `getLeaderSchedule(slot)` → `identity → [slot indices in the epoch]` for the epoch
/// containing `slot`. Fetched once per epoch; the caller reduces it to the Jito-led
/// index set. A `null` result (slot not in a schedulable epoch) is an error so the
/// caller fails open to `getSlotLeaders`.
async fn fetch_leader_schedule(
    http: &reqwest::Client,
    rpc_url: &str,
    slot: u64,
) -> Result<HashMap<String, Vec<u64>>> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getLeaderSchedule",
        "params": [slot]
    });
    let v = rpc_call(http, rpc_url, &body).await?;
    let result = v.get("result").context("getLeaderSchedule result missing")?;
    let map = result
        .as_object()
        .context("getLeaderSchedule result not an object (null/slot out of range)")?;
    let mut out: HashMap<String, Vec<u64>> = HashMap::with_capacity(map.len());
    for (identity, indices) in map {
        let idxs: Vec<u64> = indices
            .as_array()
            .map(|a| a.iter().filter_map(serde_json::Value::as_u64).collect())
            .unwrap_or_default();
        out.insert(identity.clone(), idxs);
    }
    Ok(out)
}

/// POST a JSON-RPC body to the Solana RPC and return the parsed response, bailing on
/// an HTTP error or a JSON-RPC `error` member.
async fn rpc_call(
    http: &reqwest::Client,
    rpc_url: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value> {
    let resp = http
        .post(rpc_url)
        .json(body)
        .send()
        .await
        .context("RPC HTTP")?
        .error_for_status()
        .context("RPC status")?;
    let v: serde_json::Value = resp.json().await.context("parse RPC response")?;
    if let Some(err) = v.get("error") {
        bail!("RPC error: {err}");
    }
    Ok(v)
}
