//! Recovery reaper — safety backstop for `strategy_positions` rows the engine
//! loop can't resolve on its own:
//! - `BuySubmitted` with no live entry task → adopt / wait / drop (never re-send);
//!   unresolved past the review window → `needs_review` SSE flag (manual Verify)
//! - `ExitPending` with no live exit task → re-drive sell (or nudge the engine)
//! - externally-cleared `Holding` (bag gone via Trade sell) → book End (PG `trades` net)
//! - `ExitStuck` with remaining bag (PG net) → re-drive sell with backoff, then park
//! - `ExitStuck` / `ExitUnconfirmed` whose bag is gone → heal to End
//! - stale `ExitPending` → bag-check first: cleared heals to End, a held bag
//!   flips to `ExitStuck` (open, attention) — never a blind terminal stamp
//! - **paper** `ExitStuck` → book End at the last known price (no bag to redrive;
//!   every real path above is `mode = 'real'`, so these had no owner at all)
//!
//! Fires once at boot (immediate tick) then every 60 s. Skips rows whose
//! [`InFlightGuards`] slot is held so it never races a live buy/sell task.
//!
//! Helius: no periodic account/balance RPC. Cleared/stranded detection is Postgres
//! `trades` net only; sell confirm stays feed-first inside `run_exit`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::sync::{broadcast, mpsc, watch};
use tokio::task::JoinHandle;
use tracing::{info, warn};
use uuid::Uuid;

use hunter_engine::event::{Event, Fill, FillFailReason};

use trading_core::config::constants::BUY_SUBMITTED_REVIEW_SECS;
use trading_core::models::ingest::SseEvent;
use trading_core::models::StrategyPosition;

/// The ONE decoder for `EXIT_BAG_ONCHAIN_CHECK` (default **off** — it spends
/// Helius credits). Both `ReaperDeps` construction sites call this so the flag
/// cannot mean different things in the boot reaper and the manual-action path.
pub fn onchain_bag_check_from_env() -> bool {
    std::env::var("EXIT_BAG_ONCHAIN_CHECK")
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
}
use trading_core::state::token_cache::TokenCache;
use trading_core::state::trade_signals::TradeSignals;
use trading_core::storage::repositories::settings_repo::AppSettings;
use trading_core::storage::repositories::strategy_repo::StrategyRepo;
use trading_core::storage::repositories::trade_repo::TradeRepo;

use crate::trader::PumpFunTrader;
use pump_trader::NonceTxState;

use super::exec_real::{self, BuyRecoveryVerdict};
use super::orphan_exit::{self, OrphanExitDeps, OrphanStart, BAG_CLEARED_THRESHOLD_RAW};
use super::{InFlightGuards, PositionRegistry};

const INTERVAL: Duration = Duration::from_secs(60);
const EXIT_PENDING_STALE: Duration = Duration::from_secs(300);
const UNENTERED_STALE: Duration = Duration::from_secs(600);
/// Flag unresolved `BuySubmitted` rows past this age for manual review (SSOT
/// window shared with the API's `needs_review` derivation).
const BUY_SUBMITTED_REVIEW: Duration = Duration::from_secs(BUY_SUBMITTED_REVIEW_SECS);
/// After a Fatal/unresolved ExitStuck redrive, skip this many reaper ticks
/// before trying again (no Helius spam).
const EXIT_STUCK_BACKOFF_TICKS: u8 = 5;
/// Bounded-then-park: total automatic redrives of one ExitStuck position before
/// it is parked (reaper stops auto-retrying, surfaced for a manual decision). Each
/// redrive can send reverting sells (tip+fees) on a dead/rugged pool, so this is
/// kept low. Reset by a manual Retry (`unpark_exit`) or a fresh entry.
const EXIT_REDRIVE_CAP: i32 = 2;

pub struct ReaperDeps {
    pub strategy_repo: StrategyRepo,
    pub trade_repo: TradeRepo,
    pub trader: Arc<PumpFunTrader>,
    pub token_cache: Arc<TokenCache>,
    pub trade_signals: Arc<TradeSignals>,
    pub inflight: InFlightGuards,
    pub registry: PositionRegistry,
    pub fill_tx: mpsc::Sender<Event>,
    pub settings: watch::Receiver<AppSettings>,
    /// SSE hub — used to push the `needs_review` flag on a stale BuySubmitted.
    /// Logging it alone leaves the stale row invisible to the UI.
    pub sse_tx: broadcast::Sender<SseEvent>,
    /// Opt-in on-chain bag check before a stranded row is parked
    /// (`EXIT_BAG_ONCHAIN_CHECK`, default **off** — it spends Helius credits).
    ///
    /// Everything else in this reaper decides "is the bag gone?" from the
    /// Postgres `trades` net, which is blind in two ways the 2026-07-28 review
    /// found: a feed gap makes a landed sell invisible (PG says "still held"
    /// forever), and a bag sitting in an account no row references is invisible
    /// to any PG query at all. One `getTokenAccountsByOwner` answers both — it
    /// returns *every* account for the mint with its balance.
    ///
    /// Bounded by construction: only rows about to be parked reach it (5 in the
    /// 8 h sample), never the hot path.
    pub onchain_bag_check: bool,
}

impl ReaperDeps {
    fn orphan_deps(&self) -> OrphanExitDeps {
        OrphanExitDeps {
            strategy_repo: self.strategy_repo.clone(),
            trade_repo: self.trade_repo.clone(),
            trader: self.trader.clone(),
            token_cache: self.token_cache.clone(),
            trade_signals: self.trade_signals.clone(),
            inflight: self.inflight.clone(),
            registry: self.registry.clone(),
            fill_tx: self.fill_tx.clone(),
            settings: self.settings.clone(),
        }
    }
}

/// Spawn the reaper loop (immediate first tick, then every `INTERVAL`).
pub fn spawn_reaper(deps: ReaperDeps) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // Skip counters for stuck BuySubmitted rows past the review window — still
        // try feed-adopt every tick, but don't burn signature_state RPC every 60s.
        let mut stuck_rpc_skip: HashMap<Uuid, u8> = HashMap::new();
        let mut exit_stuck_backoff: HashMap<Uuid, u8> = HashMap::new();
        let mut exit_unconfirmed_backoff: HashMap<Uuid, u8> = HashMap::new();
        // Do NOT consume the immediate first tick — boot sweep runs right away.
        loop {
            tick.tick().await;
            redrive_orphaned_buy_submitted(&deps, &mut stuck_rpc_skip).await;
            orphan_exit::reconcile_externally_cleared_holdings(&deps.orphan_deps()).await;
            // Book any ExitPending whose bag is already gone BEFORE re-driving the
            // sell — else the re-drive would submit a phantom sell of an empty bag.
            heal_exit_pending_cleared(&deps).await;
            redrive_orphaned_exit_pending(&deps).await;
            redrive_exit_stuck(&deps, &mut exit_stuck_backoff).await;
            // Bag-gone heals: ExitStuck (post-redrive clear) and ExitUnconfirmed
            // (the sell DID land, the feed just confirmed late) → book End.
            heal_cleared_by_status(&deps, "ExitStuck").await;
            heal_cleared_by_status(&deps, "ExitUnconfirmed").await;
            // Runs AFTER its heal so a sell that merely confirmed late books End
            // instead of being re-sent. What is left genuinely still holds a bag.
            redrive_exit_unconfirmed(&deps, &mut exit_unconfirmed_backoff).await;
            // Stale ExitPending (real): bag-check first (S4) — the cleared heal
            // above already booked landed sells; what's left with a bag flips to
            // ExitStuck (open, attention) for the redrive/park machinery.
            match deps
                .strategy_repo
                .mark_stale_exit_pending_stuck(EXIT_PENDING_STALE, BAG_CLEARED_THRESHOLD_RAW)
                .await
            {
                Ok(n) if n > 0 => info!(n, "reaper: stale ExitPending → ExitStuck"),
                Ok(_) => {}
                Err(e) => warn!("reaper: mark_stale_exit_pending_stuck: {e}"),
            }
            // Stale paper ExitPending (crash mid-simulated sell): no on-chain bag
            // exists — book End at the entry (breakeven) rather than fabricating
            // a loss or stranding an attention row with no legal action.
            close_stale_paper_exit_pending(&deps).await;
            // Paper `ExitStuck`: no bag, no redrive — book End at the last known
            // price. Every real recovery path above is `mode = 'real'`, so without
            // this a paper row that failed its exit stays open forever.
            close_paper_exit_stuck(&deps).await;
            // Paper `BuySubmitted` that never filled (crash mid-sim buy) owns no
            // on-chain bag to recover — drop it past the stale window so it can't
            // linger as a forever-`Open` paper row. Real `BuySubmitted` is left to
            // the buy-recovery reaper (it may own on-chain tokens).
            match deps.strategy_repo.delete_stale_paper_buy_submitted(UNENTERED_STALE).await {
                Ok(n) if n > 0 => info!(n, "reaper: deleted stale paper BuySubmitted rows"),
                Ok(_) => {}
                Err(e) => warn!("reaper: delete_stale_paper_buy_submitted: {e}"),
            }
        }
    })
}

/// How many reaper ticks to skip `signature_state` RPC for a stuck row past
/// [`BUY_SUBMITTED_REVIEW`] (feed-adopt still runs every tick). 10 × 60s ≈ 10 min.
const STUCK_RPC_SKIP_TICKS: u8 = 10;

/// Outcome of a one-shot `BuySubmitted` resolve — shared by the reaper tick and
/// the Console's on-demand Verify action (`close?action=verify`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuyResolution {
    /// A submitted sig was already indexed — entry fill recorded → Holding.
    Adopted,
    /// Every submitted sig is a confirmed revert — row deleted, SOL released.
    Dropped,
    /// Still ambiguous (a sig may yet land) — wait; NEVER re-send.
    Wait,
}

/// Resolve one real `BuySubmitted` row: adopt an indexed fill, or drop it when
/// every submitted sig confirmed-reverted, else wait. Never re-sends. The caller
/// must hold the entry in-flight guard.
pub async fn resolve_buy_submitted(
    deps: &ReaperDeps,
    position: &trading_core::models::StrategyPosition,
) -> BuyResolution {
    resolve_buy_submitted_inner(deps, position, true).await
}

/// [`resolve_buy_submitted`] with the signature_state RPC pass optional — the
/// reaper throttles it for rows already flagged past the review window (feed
/// adopt still runs every tick).
async fn resolve_buy_submitted_inner(
    deps: &ReaperDeps,
    position: &trading_core::models::StrategyPosition,
    check_sigs: bool,
) -> BuyResolution {
    let wallet = deps.trader.wallet_pubkey();

    // 1. Adopt: any submitted sig already indexed → record the entry.
    for sig in &position.submitted_buy_signatures {
        if let Ok(Some(legs)) = deps
            .trade_repo
            .find_fill_by_signature(&wallet, &position.mint_address, sig)
            .await
        {
            if legs.token_amount == 0 {
                continue;
            }
            let token_account = position
                .token_account
                .clone()
                .or_else(|| deps.trader.cached_token_account(&position.mint_address));
            match deps
                .strategy_repo
                .record_entry_fill(
                    position.id,
                    sig,
                    legs.token_amount,
                    legs.price_per_token(),
                    legs.amount_sol,
                    legs.last_block_time,
                    token_account.as_deref(),
                )
                .await
            {
                Ok(_) => {
                    info!(
                        position_id = %position.id,
                        mint = %position.mint_address,
                        "reaper: adopted BuySubmitted fill → Holding"
                    );
                    if let Some(engine_id) = deps.registry.engine_id(position.id) {
                        if let Some(meta) = deps.registry.get(engine_id) {
                            if let Some(intent) = meta.inflight_intent {
                                let _ = deps
                                    .fill_tx
                                    .send(Event::FillConfirmed {
                                        intent,
                                        fill: Fill {
                                            price: legs.price_per_token(),
                                            sol: legs.amount_sol,
                                            token_amount: legs.token_amount,
                                            at: legs.last_block_time,
                                        },
                                    })
                                    .await;
                            }
                        }
                    }
                    return BuyResolution::Adopted;
                }
                Err(e) => warn!(
                    position_id = %position.id,
                    "reaper: record_entry_fill failed: {e}"
                ),
            }
        }
    }

    // A row that never recorded a signature never sent a transaction, so it
    // cannot own tokens — the reason real `BuySubmitted` is spared the paper
    // row's stale-drop does not apply to it. Waiting forever is the strictly
    // worse answer: the row is re-adopted at boot as an inert `EntryPending`
    // arm, so it holds its (rule, mint) slot AND a `max_concurrent_tokens`
    // slot for the process's whole life. Ten of them silenced a live rule for
    // ~17 h on 2026-08-02 (see the buy-send timeout in `exec_real`, the defect
    // that produced them). Give the signature a bounded grace window — the
    // write-ahead `on_signed` persist is best-effort and can land late — then
    // drop exactly like the all-reverted case.
    if position.submitted_buy_signatures.is_empty() {
        let age = Utc::now().signed_duration_since(position.updated_at);
        if age <= chrono::Duration::from_std(UNENTERED_STALE).unwrap_or_default() {
            return BuyResolution::Wait;
        }
        warn!(
            position_id = %position.id,
            mint = %position.mint_address,
            age_secs = age.num_seconds(),
            "reaper: BuySubmitted never recorded a signature — no tx was sent, dropping"
        );
        return drop_buy_submitted(deps, position, "never signed").await;
    }

    // 2. Drop ONLY if EVERY submitted sig is a confirmed revert.
    if !check_sigs {
        return BuyResolution::Wait;
    }
    let mut all_reverted = true;
    for sig in &position.submitted_buy_signatures {
        let status = deps.trader.signature_state(sig).await;
        if exec_real::classify_submitted_buy(&status) == BuyRecoveryVerdict::Wait {
            all_reverted = false;
            break;
        }
    }
    if all_reverted {
        return drop_buy_submitted(deps, position, "all sigs reverted").await;
    }

    // 3. Not reverted is NOT the same as still alive. A buy that was signed but
    //    never actually broadcast reports `Pending` forever, so every check above
    //    returns `Wait` on every tick for the life of the process — and boot
    //    re-adopts the row, so restarts don't clear it either. It holds its
    //    `(rule, mint)` slot AND a `max_concurrent_tokens` slot permanently: the
    //    same cap-starvation that silenced a live rule for ~17 h on 2026-08-02,
    //    reached from the other side. Thirteen such rows piled up 2026-07-31..08-03
    //    when a server-side tip below the Helius Sender floor made every submit
    //    fail pre-broadcast. `55164323` only retires rows that never recorded a
    //    signature; these recorded one, so nothing could ever retire them.
    resolve_never_broadcast(deps, position).await
}

/// Age a `BuySubmitted` row must reach before the never-broadcast proof is even
/// attempted. Well past a recent blockhash's ~60-90 s lifetime, and long enough
/// that feed/RPC lag cannot masquerade as "never landed".
const NEVER_BROADCAST_STALE: Duration = Duration::from_secs(600);

/// Retire a `BuySubmitted` row whose transaction was signed but provably never
/// executed. Three gates, in this order, because the order is what makes it safe:
///
/// 1. **Make execution impossible.** `burn_nonce_tx` returns `Dead` only once the
///    nonce has moved past this tx's hash, advancing the nonce itself when it has
///    not. `Unknown` (untracked, or aged out across a restart) proves nothing, so
///    the row is left for manual Verify rather than guessed at.
/// 2. **Only then ask whether it ever landed.** A `Dead` nonce on its own is
///    worthless as a drop signal — a tx that *executed* consumes the nonce too, so
///    `Dead` covers "never sent" and "already filled" alike, and dropping on it
///    alone would delete a filled position and strand its tokens. After gate 1 no
///    new execution is possible, so this answer cannot go stale underneath us.
/// 3. **Re-check the indexed feed**, in case the fill was recorded while 1-2 ran.
///
/// Anything short of all three ⇒ `Wait`. Never re-sends.
async fn resolve_never_broadcast(
    deps: &ReaperDeps,
    position: &StrategyPosition,
) -> BuyResolution {
    let age = Utc::now().signed_duration_since(position.updated_at);
    if age <= chrono::Duration::from_std(NEVER_BROADCAST_STALE).unwrap_or_default() {
        return BuyResolution::Wait;
    }

    for sig in &position.submitted_buy_signatures {
        match deps.trader.burn_nonce_tx(sig).await {
            NonceTxState::Dead => {}
            state => {
                info!(
                    id = %position.id, mint = %position.mint_address, sig = %sig, ?state,
                    "reaper: BuySubmitted not provably dead — leaving for manual Verify"
                );
                return BuyResolution::Wait;
            }
        }
    }

    for sig in &position.submitted_buy_signatures {
        if !matches!(deps.trader.signature_state(sig).await, Ok(None)) {
            return BuyResolution::Wait;
        }
    }

    let wallet = deps.trader.wallet_pubkey();
    for sig in &position.submitted_buy_signatures {
        if let Ok(Some(legs)) = deps
            .trade_repo
            .find_fill_by_signature(&wallet, &position.mint_address, sig)
            .await
        {
            if legs.token_amount > 0 {
                return BuyResolution::Wait;
            }
        }
    }

    warn!(
        id = %position.id, mint = %position.mint_address, age_secs = age.num_seconds(),
        "reaper: BuySubmitted tx was signed but never executed — dropping, slot released"
    );
    drop_buy_submitted(deps, position, "signed but never executed").await
}

/// Terminate one unfillable `BuySubmitted` row: delete it, release its SOL
/// earmark, and fold the engine arm so the `max_concurrent_tokens` counter is
/// rolled back. The ONE drop path — reached only once the caller has PROVEN the
/// row owns no tokens (every sig reverted, or no sig was ever sent).
async fn drop_buy_submitted(
    deps: &ReaperDeps,
    position: &StrategyPosition,
    why: &str,
) -> BuyResolution {
    match deps.strategy_repo.delete_position(position.id).await {
        Ok(()) => {
            deps.trader.release_sol_for_position(&position.id.to_string());
            info!(
                position_id = %position.id,
                mint = %position.mint_address,
                "reaper: dropped BuySubmitted ({why} — no tokens)"
            );
            if let Some(engine_id) = deps.registry.engine_id(position.id) {
                if let Some(meta) = deps.registry.get(engine_id) {
                    if let Some(intent) = meta.inflight_intent {
                        let _ = deps
                            .fill_tx
                            .send(Event::FillFailed {
                                intent,
                                reason: FillFailReason::Fatal,
                                at: Some(Utc::now()),
                            })
                            .await;
                    }
                }
            }
            BuyResolution::Dropped
        }
        Err(err) => {
            warn!(
                position_id = %position.id,
                "reaper: delete BuySubmitted failed: {err}"
            );
            BuyResolution::Wait
        }
    }
}

/// Recover `BuySubmitted` rows — adopt indexed fills, drop only when every
/// submitted sig confirmed-reverted, else wait. Never re-sends.
async fn redrive_orphaned_buy_submitted(
    deps: &ReaperDeps,
    stuck_rpc_skip: &mut HashMap<Uuid, u8>,
) {
    let submitted = match deps.strategy_repo.find_all_buy_submitted("real").await {
        Ok(p) => p,
        Err(err) => {
            warn!("reaper: load BuySubmitted failed: {err}");
            return;
        }
    };
    let live_ids: std::collections::HashSet<Uuid> = submitted.iter().map(|p| p.id).collect();
    stuck_rpc_skip.retain(|id, _| live_ids.contains(id));

    for position in submitted {
        if deps.inflight.entry_held(position.id) {
            continue;
        }
        let Some(_guard) = deps.inflight.try_begin_entry(position.id) else {
            continue;
        };

        let age = Utc::now().signed_duration_since(position.updated_at);
        let past_review =
            age > chrono::Duration::from_std(BUY_SUBMITTED_REVIEW).unwrap_or_default();
        // Feed-adopt runs every tick; the signature_state RPC pass is throttled
        // for rows already flagged past the review window.
        let mut check_sigs = true;
        if past_review {
            match stuck_rpc_skip.get_mut(&position.id) {
                Some(n) if *n > 0 => {
                    *n -= 1;
                    check_sigs = false;
                }
                _ => {
                    warn!(
                        position_id = %position.id,
                        mint = %position.mint_address,
                        "reaper: BuySubmitted unresolved past review window — manual review \
                         (throttling signature_state RPC)"
                    );
                    // Surface the flag to the UI (B3: was log-only). The API's
                    // `needs_review` derivation covers snapshots; this covers a
                    // session already hydrated before the row went stale.
                    let _ = deps.sse_tx.send(SseEvent::StrategyPositionUpdate {
                        rule_id: position.rule_id.unwrap_or_default(),
                        mint_address: position.mint_address.clone(),
                        position_id: position.id,
                        status: "BuySubmitted".to_string(),
                        exit_reason: None,
                        entry_price: position.entry_price,
                        exit_price: None,
                        trade_mode: Some("real".to_string()),
                        rule_name: None,
                        needs_review: Some(true),
                        sold_token_amount: None,
                        sold_bps: None,
                        scale_stage: None,
                    });
                    stuck_rpc_skip.insert(position.id, STUCK_RPC_SKIP_TICKS);
                }
            }
        }

        match resolve_buy_submitted_inner(deps, &position, check_sigs).await {
            BuyResolution::Adopted | BuyResolution::Dropped => {
                stuck_rpc_skip.remove(&position.id);
            }
            BuyResolution::Wait => {}
        }
    }
}

/// Re-drive `ExitPending` rows whose exit guard is not held.
async fn redrive_orphaned_exit_pending(deps: &ReaperDeps) {
    let pending = match deps.strategy_repo.find_all_exit_pending("real").await {
        Ok(p) => p,
        Err(err) => {
            warn!("reaper: load ExitPending failed: {err}");
            return;
        }
    };
    let orphan = deps.orphan_deps();
    for position in pending {
        if position.entry_price.is_none() {
            continue;
        }
        if deps.inflight.exit_held(position.id) || deps.inflight.exit_mint_held(&position.mint_address)
        {
            continue;
        }

        // Prefer nudging the live engine when it still owns the position.
        if let Some(engine_id) = deps.registry.engine_id(position.id) {
            if let Some(meta) = deps.registry.get(engine_id) {
                if let Some(intent) = meta.inflight_intent.clone() {
                    info!(
                        position_id = %position.id,
                        mint = %position.mint_address,
                        "reaper: nudging engine ExitPending via FillFailed::Reverted"
                    );
                    let _ = deps
                        .fill_tx
                        .send(Event::FillFailed {
                            intent,
                            reason: FillFailReason::Reverted,
                            at: Some(Utc::now()),
                        })
                        .await;
                    continue;
                }
            }
        }

        match orphan_exit::spawn_orphan_sell(&orphan, position, "Recovery", false) {
            OrphanStart::Started => {}
            OrphanStart::Busy | OrphanStart::NothingToSell => {}
        }
    }
}

/// ExitPending rows whose PG net is already ≤ dust → book End (no sell, no RPC).
/// The in-flight sell that set `ExitPending` already cleared the bag; without this
/// the ExitPending re-drive would send a second sell that reverts for no tokens.
async fn heal_exit_pending_cleared(deps: &ReaperDeps) {
    let cleared = match deps
        .strategy_repo
        .find_exit_pending_cleared(BAG_CLEARED_THRESHOLD_RAW)
        .await
    {
        Ok(p) => p,
        Err(e) => {
            warn!("reaper: find_exit_pending_cleared failed: {e}");
            return;
        }
    };
    if cleared.is_empty() {
        return;
    }
    let orphan = deps.orphan_deps();
    let wallet = deps.trader.wallet_pubkey();
    let mut n = 0u32;
    for pos in cleared {
        // Skip a position whose exit is actively in flight in THIS process — its
        // own confirm loop will book the close; don't race a book-close under it.
        if deps.inflight.exit_held(pos.id) || deps.inflight.exit_mint_held(&pos.mint_address) {
            continue;
        }
        let fill = orphan_exit::fill_from_latest_sell(&deps.trade_repo, &wallet, &pos).await;
        if orphan_exit::book_externally_cleared(&orphan, &pos, fill).await.is_ok() {
            n += 1;
        }
    }
    if n > 0 {
        info!(closed = n, "reaper: booked externally-cleared ExitPending rows");
    }
}

/// Re-drive real `ExitStuck` rows that still have a bag (PG net > dust).
async fn redrive_exit_stuck(deps: &ReaperDeps, backoff: &mut HashMap<Uuid, u8>) {
    let stuck = match deps
        .strategy_repo
        .find_exit_stuck_bags(BAG_CLEARED_THRESHOLD_RAW)
        .await
    {
        Ok(p) => p,
        Err(err) => {
            warn!("reaper: load ExitStuck-with-bag failed: {err}");
            return;
        }
    };
    let live_ids: std::collections::HashSet<Uuid> = stuck.iter().map(|p| p.id).collect();
    backoff.retain(|id, _| live_ids.contains(id));

    let orphan = deps.orphan_deps();
    for position in stuck {
        if let Some(n) = backoff.get_mut(&position.id) {
            if *n > 0 {
                *n -= 1;
                continue;
            }
        }
        if deps.inflight.exit_held(position.id) || deps.inflight.exit_mint_held(&position.mint_address)
        {
            continue;
        }
        match orphan_exit::spawn_orphan_sell(&orphan, position.clone(), "Recovery", false) {
            OrphanStart::Started => {
                // Back off regardless of outcome so we don't re-spam every 60s while
                // the sell task runs / after Fatal.
                backoff.insert(position.id, EXIT_STUCK_BACKOFF_TICKS);
                // Bounded-then-park: count this redrive; at the cap, park it so the
                // reaper stops auto-retrying (a dead/rugged pool would otherwise burn
                // tip+fees forever) and it surfaces for a manual decision. Never
                // auto-written-off.
                park_or_recover(deps, &position, "ExitStuck").await;
            }
            OrphanStart::Busy => {}
            OrphanStart::NothingToSell => {
                let wallet = deps.trader.wallet_pubkey();
                let fill =
                    orphan_exit::fill_from_latest_sell(&deps.trade_repo, &wallet, &position).await;
                let _ = orphan_exit::book_externally_cleared(&orphan, &position, fill).await;
            }
        }
    }
}

/// What the chain says about a stranded position's bag.
enum OnChainBag {
    /// Wallet holds nothing for this mint — the sell landed; the feed missed it.
    Cleared,
    /// Tokens are in an account other than the one on the row; the row was
    /// re-pointed at it, so a redrive can now actually reach them.
    Repointed,
    /// The row already points at the account holding the bag, or the check is
    /// disabled / failed. Nothing learned — fall through to normal handling.
    Unchanged,
}

/// Opt-in on-chain reconciliation for a row that is about to be parked. See
/// [`ReaperDeps::onchain_bag_check`] for why PG alone cannot answer this.
/// One `getTokenAccountsByOwner`; never called on the hot path.
async fn reconcile_bag_onchain(deps: &ReaperDeps, position: &StrategyPosition) -> OnChainBag {
    if !deps.onchain_bag_check {
        return OnChainBag::Unchanged;
    }
    let accounts = match deps
        .trader
        .get_all_token_accounts_for_mint(&position.mint_address)
        .await
    {
        Ok(a) => a,
        Err(e) => {
            warn!(id = %position.id, mint = %position.mint_address,
                  "reaper: on-chain bag check failed: {e}");
            return OnChainBag::Unchanged;
        }
    };
    let total: u64 = accounts.iter().map(|(_, amount)| *amount).sum();
    if total == 0 {
        info!(id = %position.id, mint = %position.mint_address,
              "reaper: on-chain bag empty — sell landed, feed missed it");
        return OnChainBag::Cleared;
    }
    // Point the row at the account actually holding the tokens, when that is not
    // where it is looking. Largest balance wins: the position's own bag is the
    // dominant holding in the account its buy funded.
    let Some((holder, amount)) = accounts.into_iter().filter(|(_, a)| *a > 0).max_by_key(|(_, a)| *a)
    else {
        return OnChainBag::Unchanged;
    };
    let holder = holder.to_string();
    if position.token_account.as_deref() == Some(holder.as_str()) {
        return OnChainBag::Unchanged;
    }
    match deps.strategy_repo.set_token_account(position.id, &holder).await {
        Ok(()) => {
            warn!(
                id = %position.id, mint = %position.mint_address,
                was = ?position.token_account, now = %holder, amount,
                "reaper: bag found in a different token account — row re-pointed"
            );
            OnChainBag::Repointed
        }
        Err(e) => {
            warn!(id = %position.id, "reaper: set_token_account failed: {e}");
            OnChainBag::Unchanged
        }
    }
}

/// Count this redrive and, at the cap, decide between parking and recovery.
///
/// Parking is the "stop burning tip+fees, a human should look" terminal for an
/// open bag. Before accepting it, the opt-in on-chain check gets a say: the bag
/// may already be gone (feed gap) or reachable at a different account, and both
/// are recoverable without human involvement. Only a genuinely stuck bag parks.
async fn park_or_recover(deps: &ReaperDeps, position: &StrategyPosition, status: &str) {
    let n = match deps.strategy_repo.bump_exit_redrive(position.id).await {
        Ok(n) => n,
        Err(e) => {
            warn!(id = %position.id, "reaper: bump_exit_redrive failed: {e}");
            return;
        }
    };
    if n < EXIT_REDRIVE_CAP {
        return;
    }
    match reconcile_bag_onchain(deps, position).await {
        OnChainBag::Cleared => {
            let orphan = deps.orphan_deps();
            let wallet = deps.trader.wallet_pubkey();
            let fill = orphan_exit::fill_from_latest_sell(&deps.trade_repo, &wallet, position).await;
            let _ = orphan_exit::book_externally_cleared(&orphan, position, fill).await;
        }
        OnChainBag::Repointed => {
            // The redrives so far were aimed at the wrong account, so they are not
            // evidence the bag is unsellable. Reset the budget and let the next
            // tick try against the account that actually holds it.
            if let Err(e) = deps.strategy_repo.unpark_exit(position.id).await {
                warn!(id = %position.id, "reaper: redrive-budget reset failed: {e}");
            }
        }
        OnChainBag::Unchanged => {
            match deps.strategy_repo.set_exit_parked(position.id, true).await {
                Ok(()) => info!(
                    id = %position.id, mint = %position.mint_address, redrives = n, status,
                    "reaper: hit redrive cap → parked for manual resolution"
                ),
                Err(e) => warn!(id = %position.id, "reaper: park {status} failed: {e}"),
            }
        }
    }
}

/// `ExitUnconfirmed` rows that still hold a bag: prove the un-landed sell can
/// never execute, then re-drive it.
///
/// Why this needs more than [`redrive_exit_stuck`]: `ExitUnconfirmed` means the
/// sell neither cleared nor reverted. Those sells ride a **durable nonce**, which
/// does not expire — `schedule_nonce_refresh` even re-arms the slot with the same
/// hash when the send didn't land. So the original can still execute at any time,
/// and a blind resend risks two landing sells; against a shared token account the
/// second one eats a sibling position's bag. So the row cannot simply be resent —
/// without a liveness proof it can only be parked for a human, even when the token
/// is migrated and perfectly sellable.
///
/// [`Engine::burn_nonce_tx`] closes that gap: it returns `Dead` only once the
/// nonce has moved past the tx's hash — usually already true (a busy wallet
/// reuses the slot within seconds, costing nothing), otherwise it advances the
/// nonce itself. **Every** recorded sell sig must be provably dead before we
/// resend; anything `Unknown` (untracked, or aged out across a restart) leaves
/// the row exactly as it is today, for manual Verify.
async fn redrive_exit_unconfirmed(deps: &ReaperDeps, backoff: &mut HashMap<Uuid, u8>) {
    let stranded = match deps
        .strategy_repo
        .find_bags_by_status("ExitUnconfirmed", BAG_CLEARED_THRESHOLD_RAW)
        .await
    {
        Ok(p) => p,
        Err(err) => {
            warn!("reaper: load ExitUnconfirmed-with-bag failed: {err}");
            return;
        }
    };
    let live_ids: std::collections::HashSet<Uuid> = stranded.iter().map(|p| p.id).collect();
    backoff.retain(|id, _| live_ids.contains(id));

    let orphan = deps.orphan_deps();
    for position in stranded {
        if let Some(n) = backoff.get_mut(&position.id) {
            if *n > 0 {
                *n -= 1;
                continue;
            }
        }
        if deps.inflight.exit_held(position.id)
            || deps.inflight.exit_mint_held(&position.mint_address)
        {
            continue;
        }

        // Safety gate. No recorded sig ⇒ nothing was ever sent under this row's
        // exit, so there is no tx that could land behind our back and a resend is
        // safe. Any recorded sig must be proven dead first.
        let sigs = position.exit_tx_sigs();
        let mut all_dead = true;
        for sig in &sigs {
            match deps.trader.burn_nonce_tx(sig).await {
                NonceTxState::Dead => {}
                state => {
                    info!(
                        id = %position.id, mint = %position.mint_address, sig = %sig, ?state,
                        "reaper: ExitUnconfirmed sell not provably dead — leaving for manual Verify"
                    );
                    all_dead = false;
                    break;
                }
            }
        }
        if !all_dead {
            backoff.insert(position.id, EXIT_STUCK_BACKOFF_TICKS);
            continue;
        }

        match orphan_exit::spawn_orphan_sell(&orphan, position.clone(), "Recovery", false) {
            OrphanStart::Started => {
                backoff.insert(position.id, EXIT_STUCK_BACKOFF_TICKS);
                // Same bounded-then-park policy as ExitStuck: never burn tip+fees
                // forever on a dead pool, and never auto-write-off.
                park_or_recover(deps, &position, "ExitUnconfirmed").await;
            }
            OrphanStart::Busy => {}
            OrphanStart::NothingToSell => {
                let wallet = deps.trader.wallet_pubkey();
                let fill =
                    orphan_exit::fill_from_latest_sell(&deps.trade_repo, &wallet, &position).await;
                let _ = orphan_exit::book_externally_cleared(&orphan, &position, fill).await;
            }
        }
    }
}

/// Rows in `status` (`ExitStuck` / `ExitUnconfirmed`) whose PG net is already
/// ≤ dust → book End (no sell, no RPC). For `ExitUnconfirmed` this is the heal
/// that never existed (S3): the sell DID land, the feed just confirmed late.
async fn heal_cleared_by_status(deps: &ReaperDeps, status: &str) {
    let cleared = match deps
        .strategy_repo
        .find_cleared_by_status(status, BAG_CLEARED_THRESHOLD_RAW)
        .await
    {
        Ok(p) => p,
        Err(e) => {
            warn!(status, "reaper: find_cleared_by_status failed: {e}");
            return;
        }
    };
    if cleared.is_empty() {
        return;
    }
    let orphan = deps.orphan_deps();
    let wallet = deps.trader.wallet_pubkey();
    let mut n = 0u32;
    for pos in cleared {
        if deps.inflight.exit_held(pos.id) || deps.inflight.exit_mint_held(&pos.mint_address) {
            continue;
        }
        let fill = orphan_exit::fill_from_latest_sell(&deps.trade_repo, &wallet, &pos).await;
        if orphan_exit::book_externally_cleared(&orphan, &pos, fill).await.is_ok() {
            n += 1;
        }
    }
    if n > 0 {
        info!(n, status, "reaper: booked cleared rows → End");
    }
}

/// How many paper `ExitStuck` rows one tick may heal. Only bounds the historical
/// backlog — steady state is ~0 now that a paper exit market-fills instead of
/// timing out.
const PAPER_STUCK_HEAL_PER_TICK: i64 = 200;

/// **Paper** `ExitStuck` rows → `End` at the token's last known price.
///
/// Paper owns no on-chain bag, so `ExitStuck` — "the sell gave up, the bag is
/// still held, the reaper owns it now" — has no meaning for it: every redrive/park
/// path is `mode = 'real'`, so nothing could ever reach these rows and they stayed
/// open forever (333 of 742 paper positions before the `exec_paper` exit fill was
/// fixed to market-fill an empty window). This is the heal for that backlog, and
/// the backstop for any paper exit that still fails.
///
/// Price, in order: the mint's last `trades` row **as of the moment the exit gave
/// up** (`[entry_time, updated_at]` — the death price for a token that stopped
/// trading, and the honest as-of price for a backlog row on a token that kept
/// going) → the live cache's current spot → the entry price. The feed leads the
/// cache deliberately: `current_price` is *now*, which is the right answer only
/// for a row that just failed, and silently wrong for one stranded days ago. The
/// entry price is a breakeven fiction, used only when neither holds a price; it
/// matches what [`close_stale_paper_exit_pending`] already does. Books through
/// `record_sell_fill` (via
/// [`orphan_exit::book_externally_cleared_pg`]) so the `position_fills` ledger and
/// the row aggregates stay consistent — never a bare status UPDATE.
async fn close_paper_exit_stuck(deps: &ReaperDeps) {
    let stuck = match deps.strategy_repo.find_paper_exit_stuck(PAPER_STUCK_HEAL_PER_TICK).await {
        Ok(p) => p,
        Err(e) => {
            warn!("reaper: find_paper_exit_stuck failed: {e}");
            return;
        }
    };
    let mut n = 0u32;
    for pos in stuck {
        let since = pos.entry_time.unwrap_or(pos.created_at);
        let from_feed = deps
            .trade_repo
            .find_latest_by_mint(&pos.mint_address, since, pos.updated_at)
            .await
            .ok()
            .flatten()
            .filter(|t| t.price_per_token > 0.0)
            .map(|t| (t.price_per_token, t.block_time));
        let priced = from_feed.or_else(|| {
            deps.token_cache.get(&pos.mint_address).and_then(|e| {
                let s = e.value();
                s.current_price
                    .filter(|p| *p > 0.0)
                    .map(|price| (price, s.last_trade_at.unwrap_or(pos.updated_at)))
            })
        });
        let (price, at) =
            priced.unwrap_or_else(|| (pos.entry_price.unwrap_or(0.0), pos.updated_at));
        let token_amount = pos.remaining_token_amount();
        // Size the closing leg from the **cost basis × price ratio**, not
        // `price × tokens`. Paper rows written before the 2026-08-04 token-scale
        // fix carry `entry_token_amount` 1e6× too high (`Fill::price` is SOL per
        // RAW unit, and `exec_paper` still multiplied by `TOKEN_SCALE`), so
        // `price × tokens` would book a 1e6× fantasy PnL on 331 of the 359
        // stranded rows. `entry_sol` was always right and a price *ratio* is
        // scale-free, so this is correct on both sides of that fix — and exactly
        // equal to `price × tokens` on a row whose scale is consistent.
        let sol = match (pos.entry_sol, pos.entry_price, pos.entry_token_amount) {
            (Some(entry_sol), Some(entry_price), Some(entry_tokens))
                if entry_price > 0.0 && entry_tokens > 0 =>
            {
                entry_sol * (token_amount as f64 / entry_tokens as f64) * (price / entry_price)
            }
            _ => price * token_amount as f64,
        };
        let fill = Fill { price, sol, token_amount, at };
        let reason = pos.exit_reason.clone().unwrap_or_else(|| "Manual".to_string());
        match orphan_exit::book_externally_cleared_pg(&deps.strategy_repo, pos.id, fill, &reason)
            .await
        {
            Ok(()) => n += 1,
            Err(e) => warn!(position_id = %pos.id, "reaper: paper ExitStuck heal failed: {e}"),
        }
    }
    if n > 0 {
        info!(n, "reaper: closed paper ExitStuck rows at last known price");
    }
}

/// Stale **paper** `ExitPending` rows (crash mid-simulated sell): book End at the
/// entry price (breakeven) — paper has no on-chain bag to recover, and a
/// fabricated loss (or a forever-stuck attention row) would both lie.
async fn close_stale_paper_exit_pending(deps: &ReaperDeps) {
    let stale = match deps
        .strategy_repo
        .find_stale_paper_exit_pending(EXIT_PENDING_STALE)
        .await
    {
        Ok(p) => p,
        Err(e) => {
            warn!("reaper: find_stale_paper_exit_pending failed: {e}");
            return;
        }
    };
    let mut n = 0u32;
    for pos in stale {
        let fill = Fill {
            price: pos.entry_price.unwrap_or(0.0),
            sol: pos.entry_sol.unwrap_or(0.0)
                * (pos.remaining_token_amount() as f64
                    / pos.entry_token_amount.unwrap_or(1).max(1) as f64),
            token_amount: pos.remaining_token_amount(),
            at: Utc::now(),
        };
        if orphan_exit::book_externally_cleared_pg(&deps.strategy_repo, pos.id, fill, "Manual")
            .await
            .is_ok()
        {
            n += 1;
        }
    }
    if n > 0 {
        info!(n, "reaper: closed stale paper ExitPending at breakeven");
    }
}
