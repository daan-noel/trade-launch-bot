//! Jito leader-schedule gate — fire a launch bundle only when a Jito-participating
//! validator is (about to be) the slot leader.
//!
//! A Jito bundle can ONLY land when a Jito validator builds the block, i.e. is the
//! slot leader. Submitting into a non-Jito leader slot drops the bundle *regardless
//! of tip* — the measured, dominant cause of our launch drops (a 0.005 SOL tip, 70x
//! market p99, still dropped 0 trades because the tip was never the problem). This
//! queries the block engine's `getNextScheduledLeader` and waits — bounded — until a
//! Jito leader is within a small slot window before returning, so the caller submits
//! INTO a slot that can actually build the bundle.
//!
//! **Fail-open by construction.** The gate is an optimization, never a hard
//! dependency: any RPC/parse error, a disabled gate, or an exhausted wait budget
//! returns immediately so a launch is never blocked by a gate failure — worst case
//! reverts to today's ungated submit.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::time::{Duration, Instant};
use tracing::{debug, warn};

use crate::config::LeaderGateConfig;

/// Approximate mainnet slot time (ms). Used only to *estimate* how long to sleep
/// until the next Jito leader slot; the loop re-polls `currentSlot` afterward, so
/// any drift in this constant self-corrects rather than compounding.
const SLOT_MS: u64 = 450;

/// The `getNextScheduledLeader` result — the next slot a Jito validator leads.
#[derive(Debug, Deserialize)]
struct NextLeader {
    #[serde(rename = "currentSlot")]
    current_slot: u64,
    #[serde(rename = "nextLeaderSlot")]
    next_leader_slot: u64,
}

/// Block until a Jito-participating validator is within `send_within_slots` of the
/// current slot (so a `sendBundle` now lands in a slot that can build the bundle),
/// or until the `max_wait_ms` budget is spent — whichever comes first. See the
/// module docs for the fail-open contract: this NEVER prevents a submit, it only
/// delays one (bounded) to a slot that can actually land it.
pub async fn wait_for_jito_leader(cfg: &LeaderGateConfig, engine_url: &str) {
    if !cfg.enabled {
        return;
    }
    let http = reqwest::Client::new();
    let start = Instant::now();
    let budget = Duration::from_millis(cfg.max_wait_ms);

    loop {
        let leader = match fetch_next_leader(&http, engine_url).await {
            Ok(l) => l,
            Err(e) => {
                // Fail-open: a leader-schedule hiccup must not block a launch.
                warn!(%e, "Jito leader-schedule fetch failed — submitting ungated");
                return;
            }
        };

        let slots_until = leader.next_leader_slot.saturating_sub(leader.current_slot);
        if slots_until <= cfg.send_within_slots {
            debug!(
                slots_until,
                waited_ms = start.elapsed().as_millis(),
                "Jito leader within window — releasing bundle submit"
            );
            return;
        }

        // Sleep until ~`send_within_slots` out, bounded by the remaining budget.
        let Some(remaining) = budget.checked_sub(start.elapsed()) else {
            warn!(
                slots_until,
                max_wait_ms = cfg.max_wait_ms,
                "Jito leader-schedule wait budget spent — submitting anyway (leader not yet in window)"
            );
            return;
        };
        let want = Duration::from_millis(slots_until.saturating_sub(cfg.send_within_slots) * SLOT_MS);
        let sleep_for = want.min(remaining);
        // `remaining` shrinks each loop, so this terminates: once it hits zero the
        // `checked_sub` above returns `None` and we submit.
        if sleep_for.is_zero() {
            return;
        }
        tokio::time::sleep(sleep_for).await;
    }
}

/// One `getNextScheduledLeader` JSON-RPC call against the block engine (served on the
/// same `/api/v1/bundles` endpoint as `sendBundle`).
async fn fetch_next_leader(http: &reqwest::Client, engine_url: &str) -> Result<NextLeader> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getNextScheduledLeader",
        "params": []
    });
    let resp = http
        .post(engine_url)
        .json(&body)
        .send()
        .await
        .context("getNextScheduledLeader HTTP")?
        .error_for_status()
        .context("getNextScheduledLeader status")?;
    let v: serde_json::Value = resp.json().await.context("parse getNextScheduledLeader")?;
    if let Some(err) = v.get("error") {
        bail!("getNextScheduledLeader error: {err}");
    }
    let result = v
        .get("result")
        .context("getNextScheduledLeader response missing result")?;
    serde_json::from_value(result.clone()).context("decode getNextScheduledLeader result")
}
