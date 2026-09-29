//! Dead-slot sweep: removes the `trades` legs the finalized chain never kept.
//!
//! Ingest reads at `processed` commitment, so it stores a block the moment its
//! leader broadcasts it. A block built on a fork the cluster abandons never
//! finalizes: its slot is skipped on the finalized chain and every transaction in
//! it never happened there (an explorer reports the signature as not found). The
//! legs are true decodes of a real block, so nothing at ingest tells them apart.
//!
//! The sweep runs beside ingest, never in front of it. Ingest and every decision
//! keep acting on processed data the instant it lands; the sweep corrects the
//! stored history once the chain has finalized past it. It asks a plain Solana
//! RPC ([`crate::config::Settings::canonical_rpc_url`], the public endpoint -
//! never Helius) which slots hold a finalized block, and deletes the legs of every
//! slot at or below the finalized tip that holds none.
//!
//! It walks `trades` by received time (`block_time`) from a cursor persisted in
//! `app_settings` ([`keys::DEAD_SLOT_SWEEP_THROUGH`]), one [`WINDOW`] a step, so a
//! restart resumes where it stopped and a fresh database sweeps its whole history
//! once. Algorithm and sizing: `hunter/docs/plans/ingest/dead-slot-sweep.md`.

use std::collections::HashSet;
use std::time::Duration;

use anyhow::{anyhow, Context};
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tracing::{info, warn};

use crate::services::helius_rpc::HeliusRpc;
use crate::storage::repositories::settings_repo::{keys, SettingsRepo};
use crate::storage::repositories::trade_repo::{SlotSeen, TradeRepo};

/// Received time judged per step: about 9,000 slots, one `getBlocks` call.
const WINDOW: chrono::Duration = chrono::Duration::hours(1);

/// Each step re-reads this much behind the cursor. A slot's legs can straddle a
/// window edge, the writer lands a batch after its receive stamp, and a gap
/// replay (at most `ingest.gap_replay_max_window_secs`, 300 s by default) lands
/// legs stamped behind the live edge.
const OVERLAP: chrono::Duration = chrono::Duration::minutes(10);

/// Slots per `getBlocks` call. The public endpoint serves old ranges from
/// long-term storage and times out on a day's range (~320,000 slots); 20,000
/// answers in 2-10 s.
const RPC_SPAN_SLOTS: u64 = 20_000;

/// Pause between catch-up steps, so a backfill neither floods the public
/// endpoint nor holds a core on the 2-vCPU box.
const CATCH_UP_PAUSE: Duration = Duration::from_millis(250);

/// Pause between passes once caught up.
const IDLE_INTERVAL: Duration = Duration::from_secs(60);

/// A window may hold at most `max(DEAD_SLOTS_FLOOR, DEAD_SHARE_CEILING x judged)`
/// dead slots. A busy day (3.2M legs over 318,600 slots) carries about 18, in
/// runs of four, so a count past this is an RPC answer missing blocks - which
/// would read as dead and delete real legs - and the step refuses it.
const DEAD_SLOTS_FLOOR: usize = 16;
const DEAD_SHARE_CEILING: f64 = 0.01;

/// What one window's slots say once the finalized block list is known.
#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    /// Slots at or below the tip that hold no finalized block, ascending.
    pub dead: Vec<i64>,
    /// Slots at or below the tip, the denominator of the plausibility guard.
    pub judged: usize,
    /// Earliest `first_seen` among slots above the tip: the cursor may not pass
    /// it, or those legs would never be judged. `None` when every slot is judged.
    pub resume_at: Option<DateTime<Utc>>,
}

/// Split a window's slots against the finalized block set. A slot above `tip`
/// is not judged: the chain has not settled whether it keeps a block there.
pub fn judge(seen: &[SlotSeen], blocks: &HashSet<u64>, tip: u64) -> Verdict {
    let mut dead = Vec::new();
    let mut judged = 0usize;
    let mut resume_at: Option<DateTime<Utc>> = None;
    for s in seen {
        let slot = u64::try_from(s.slot).unwrap_or(0);
        if slot > tip {
            resume_at = Some(resume_at.map_or(s.first_seen, |r| r.min(s.first_seen)));
            continue;
        }
        judged += 1;
        if !blocks.contains(&slot) {
            dead.push(s.slot);
        }
    }
    dead.sort_unstable();
    Verdict { dead, judged, resume_at }
}

/// Whether a verdict's dead count is one the chain produces (see
/// [`DEAD_SLOTS_FLOOR`]).
pub fn is_plausible(v: &Verdict) -> bool {
    let ceiling = (v.judged as f64 * DEAD_SHARE_CEILING) as usize;
    v.dead.len() <= DEAD_SLOTS_FLOOR.max(ceiling)
}

/// Totals over one or more steps.
#[derive(Debug, Default, Clone, Copy)]
pub struct SweepTotals {
    pub windows: usize,
    pub dead_slots: usize,
    pub legs_deleted: u64,
}

/// One step's result.
struct Step {
    cursor: DateTime<Utc>,
    dead_slots: usize,
    legs_deleted: u64,
    /// The cursor reached the newest leg, or a slot the chain has not finalized.
    done: bool,
}

pub struct DeadSlotSweep {
    trades: TradeRepo,
    settings: SettingsRepo,
    rpc: HeliusRpc,
    rpc_url: String,
}

impl DeadSlotSweep {
    /// `None` when `rpc_url` is empty (the sweep is off) or a Helius endpoint,
    /// which this job must never spend.
    pub fn new(pool: PgPool, rpc_url: &str) -> Option<Self> {
        let url = rpc_url.trim();
        if url.is_empty() {
            return None;
        }
        if url.to_ascii_lowercase().contains("helius") {
            warn!("dead-slot sweep off: CANONICAL_RPC_URL points at Helius, which this job never spends");
            return None;
        }
        Some(Self {
            trades: TradeRepo::new(pool.clone()),
            settings: SettingsRepo::new(pool),
            rpc: HeliusRpc::new(url.to_string()),
            rpc_url: url.to_string(),
        })
    }

    /// Sweep forever: catch up, then one pass a minute. A failed pass is logged
    /// and retried; the cursor only moves past a window that was judged.
    pub async fn run(self) {
        info!(rpc = %self.rpc_url, "dead-slot sweep running");
        loop {
            if let Err(e) = self.catch_up().await {
                warn!("dead-slot sweep: {e:#}");
            }
            tokio::time::sleep(IDLE_INTERVAL).await;
        }
    }

    /// Step until the cursor reaches the newest leg (or the finalized tip), then
    /// return what was removed.
    pub async fn catch_up(&self) -> anyhow::Result<SweepTotals> {
        let mut totals = SweepTotals::default();
        let Some(mut cursor) = self.load_cursor().await? else {
            return Ok(totals);
        };
        loop {
            let step = self.step(cursor).await?;
            totals.windows += 1;
            totals.dead_slots += step.dead_slots;
            totals.legs_deleted += step.legs_deleted;
            if step.cursor > cursor {
                cursor = step.cursor;
                self.settings
                    .set_one(&keys::DEAD_SLOT_SWEEP_THROUGH, &Some(cursor.to_rfc3339()))
                    .await?;
            }
            if step.done {
                return Ok(totals);
            }
            tokio::time::sleep(CATCH_UP_PAUSE).await;
        }
    }

    /// The stored cursor, else the oldest chunk's start. `None` on an empty table.
    async fn load_cursor(&self) -> anyhow::Result<Option<DateTime<Utc>>> {
        let stored = self.settings.get_one(&keys::DEAD_SLOT_SWEEP_THROUGH).await?;
        if let Some(ts) = stored.as_deref().and_then(|s| DateTime::parse_from_rfc3339(s).ok()) {
            return Ok(Some(ts.with_timezone(&Utc)));
        }
        self.trades.tape_floor().await
    }

    /// Judge `[cursor - OVERLAP, cursor + WINDOW)`, capped at the newest leg, and
    /// delete the dead slots in it.
    async fn step(&self, cursor: DateTime<Utc>) -> anyhow::Result<Step> {
        let Some(newest) = self.trades.newest_block_time().await? else {
            return Ok(Step { cursor, dead_slots: 0, legs_deleted: 0, done: true });
        };
        // The cursor never passes the newest leg: a synced copy appends legs
        // older than the wall clock, and they must still land ahead of it.
        let edge = newest + chrono::Duration::microseconds(1);
        if edge <= cursor {
            return Ok(Step { cursor, dead_slots: 0, legs_deleted: 0, done: true });
        }
        let from = cursor - OVERLAP;
        let to = (cursor + WINDOW).min(edge);

        let seen = self.trades.slots_between(from, to).await?;
        let tip = self
            .rpc
            .get_finalized_slot()
            .await
            .with_context(|| format!("getSlot on {}", self.rpc_url))?;
        let blocks = self.finalized_blocks(&seen, tip).await?;
        let verdict = judge(&seen, &blocks, tip);
        if !is_plausible(&verdict) {
            return Err(anyhow!(
                "refusing window [{from} .. {to}): {} of {} slots hold no finalized block, \
                 far above what the chain drops - the RPC answer looks incomplete",
                verdict.dead.len(),
                verdict.judged
            ));
        }

        let legs_deleted = self.trades.delete_slots_between(from, to, &verdict.dead).await?;
        if legs_deleted > 0 {
            info!(
                window_from = %from,
                window_to = %to,
                slots = ?verdict.dead,
                legs_deleted,
                "dead-slot sweep: removed legs of blocks the chain never finalized"
            );
        }

        let next = verdict.resume_at.unwrap_or(to).max(cursor);
        Ok(Step {
            cursor: next,
            dead_slots: verdict.dead.len(),
            legs_deleted,
            done: to == edge || verdict.resume_at.is_some(),
        })
    }

    /// The finalized blocks over the judged slots' range, in [`RPC_SPAN_SLOTS`]
    /// calls. Empty when no slot is judged.
    async fn finalized_blocks(&self, seen: &[SlotSeen], tip: u64) -> anyhow::Result<HashSet<u64>> {
        let judged = seen
            .iter()
            .filter_map(|s| u64::try_from(s.slot).ok())
            .filter(|&s| s <= tip);
        let (lo, hi) = judged.fold((u64::MAX, 0u64), |(lo, hi), s| (lo.min(s), hi.max(s)));
        let mut blocks = HashSet::new();
        if lo > hi {
            return Ok(blocks);
        }
        let mut start = lo;
        while start <= hi {
            let end = start.saturating_add(RPC_SPAN_SLOTS - 1).min(hi);
            let got = self
                .rpc
                .get_finalized_blocks(start, end)
                .await
                .with_context(|| format!("getBlocks {start}..={end} on {}", self.rpc_url))?;
            blocks.extend(got);
            start = end + 1;
            if start <= hi {
                tokio::time::sleep(CATCH_UP_PAUSE).await;
            }
        }
        Ok(blocks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn seen(slot: i64, secs: i64) -> SlotSeen {
        SlotSeen { slot, first_seen: Utc.timestamp_opt(secs, 0).unwrap() }
    }

    /// The case that motivates the sweep: slot 450566826 holds legs but no
    /// finalized block, between two slots that do.
    #[test]
    fn a_slot_without_a_finalized_block_is_dead() {
        let s = [seen(450566825, 10), seen(450566826, 9), seen(450566827, 11)];
        let blocks: HashSet<u64> = [450566825, 450566827].into();
        let v = judge(&s, &blocks, 450566900);
        assert_eq!(v.dead, vec![450566826]);
        assert_eq!(v.judged, 3);
        assert_eq!(v.resume_at, None);
    }

    /// Above the finalized tip an absence means "not settled yet", never dead,
    /// and the cursor must stop at the earliest such slot.
    #[test]
    fn a_slot_above_the_tip_is_not_judged_and_holds_the_cursor() {
        let s = [seen(100, 1), seen(101, 5), seen(102, 3)];
        let blocks: HashSet<u64> = [100].into();
        let v = judge(&s, &blocks, 100);
        assert!(v.dead.is_empty());
        assert_eq!(v.judged, 1);
        assert_eq!(v.resume_at, Some(Utc.timestamp_opt(3, 0).unwrap()));
    }

    #[test]
    fn the_guard_passes_what_the_chain_drops_and_refuses_a_hollow_answer() {
        // A busy hour: ~13,000 slots with trades, a leader's run of four dead.
        let normal = Verdict { dead: vec![1, 2, 3, 4], judged: 13_000, resume_at: None };
        assert!(is_plausible(&normal));
        // A small live window still admits a few runs.
        let small = Verdict { dead: (0..16).collect(), judged: 400, resume_at: None };
        assert!(is_plausible(&small));
        // An answer missing a tenth of the blocks is not a fork.
        let hollow = Verdict { dead: (0..1_300).collect(), judged: 13_000, resume_at: None };
        assert!(!is_plausible(&hollow));
    }
}
