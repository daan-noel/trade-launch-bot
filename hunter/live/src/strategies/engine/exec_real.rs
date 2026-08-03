//! Real execution — turns a `SubmitBuy`/`SubmitSell` effect into an on-chain
//! submit-and-return, then **synthesizes** a `FillConfirmed`/`FillFailed` event
//! from the **trades feed** (our wallet's own legs), with an RPC confirmation
//! **watchdog** as the timeout fallback.
//!
//! Double-fire safety (the crate's cardinal rule):
//! the engine's `FillFailed` handling *resubmits*, so this adapter must only emit
//! `FillFailed::Reverted` when re-submitting is safe — i.e. the buy was never
//! signed/sent, or a confirmed on-chain revert classified as retryable by
//! `pump_trader::classify_swap_revert`. Structural reverts emit `Fatal`. When an
//! outcome is genuinely ambiguous (submitted, neither a feed fill nor an
//! on-chain revert), it emits **nothing** and leaves the durable
//! `BuySubmitted`/`ExitPending` row for the recovery reaper — never a speculative
//! resubmit.

use std::borrow::Cow;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::mpsc;
use tracing::{info, warn};
use uuid::Uuid;

use hunter_engine::event::{Event, Fill, FillFailReason, IntentId};

use pump_trader::{classify_swap_revert, SwapDirection, SwapRetryDecision, SwapRoute};

use trading_core::models::trade::TradeType;
use trading_core::state::token_cache::TokenCache;
use trading_core::state::trade_signals::TradeSignals;
use trading_core::storage::repositories::strategy_repo::StrategyRepo;
use trading_core::storage::repositories::token_info_repo::TokenInfoRepo;
use trading_core::storage::repositories::trade_repo::{SigLegs, TradeRepo};

use crate::trader::{PumpFunTrader, SigStatus};

use super::orphan_exit;
use super::{FillSigStore, FillSigs, InFlightGuards, PositionRegistry, SubmittedBuyJournal};

/// How long to wait for the entry fill on the feed before the RPC watchdog fires
/// (buffers the feed's index lag — mirrors the old 12 × 1 s window).
const ENTRY_FEED_WINDOW: Duration = Duration::from_secs(12);
/// Bound on the write-ahead `mark_buy_submitted` PG persist inside the nonce
/// slot (audit M2). The in-memory [`SubmittedBuyJournal`] is set synchronously
/// first, so a slow/wedged DB never holds the send — recovery for this process
/// still sees the sig; the reaper heals cross-process via PG when the write
/// eventually lands (or the row stays BuySubmitted without the sig and the
/// wallet-reconcile backstop flags it).
/// Background persist budget (send is never blocked). Sized for a few short
/// retries while Pass-1 `insert_position` lands.
const MARK_BUY_SUBMITTED_TIMEOUT: Duration = Duration::from_millis(400);
/// Hard cap on the buy RPC send itself (nonce/blockhash build + fan-out) — the
/// mirror of [`SELL_SEND_TIMEOUT`], which the buy path went without until
/// 2026-08-03. Without it a wedged send parks `run_entry` forever: it emits
/// neither `FillConfirmed` nor `FillFailed`, so the arm stays `EntryPending`,
/// the row stays `BuySubmitted` with no signature, and the position holds a
/// `max_concurrent_tokens` slot for the life of the process. Ten of those
/// silenced a live rule for ~17 h on 2026-08-02. Sized above the sell's 15 s:
/// the snipe path front-loads a rebroadcast fan-out, so a slow-but-working send
/// must still be allowed to finish rather than be abandoned mid-flight.
const BUY_SEND_TIMEOUT: Duration = Duration::from_secs(20);
/// Extended feed poll after the RPC says the buy *landed* but the feed hasn't
/// indexed it yet.
const EXTENDED_FEED_WINDOW: Duration = Duration::from_secs(20);
/// Feed re-poll cadence while waiting.
const FEED_POLL: Duration = Duration::from_millis(500);
/// Min gap between sell-leg balance queries (dump can fire many feed wakeups).
const SELL_BALANCE_QUERY_MIN_INTERVAL: Duration = Duration::from_millis(250);
/// Sell attempts inside one `SubmitSell` (escalating Jito tip); classify/heal
/// runs between attempts. The engine adds bounded outer retries for safe Reverted.
const SELL_ATTEMPTS: u8 = 6;
/// Per-attempt sell confirm window before classifying the sent sig.
const SELL_CONFIRM_WINDOW: Duration = Duration::from_secs(5);
/// Hard cap on the RPC send itself (nonce/blockhash build + fan-out).
const SELL_SEND_TIMEOUT: Duration = Duration::from_secs(15);
/// Extended poll when sell status is Succeeded/Pending (never re-send).
const SELL_UNCONFIRMED_EXTENDED: Duration = Duration::from_secs(20);
/// Dust threshold (raw units) below which the remaining balance counts as cleared.
const PARTIAL_FILL_THRESHOLD: u64 = 0;
/// Bounded wait for the asynchronous `insert_position` before giving up on the
/// `last_entry_error` write (mig 0017) — same shape as `MARK_BUY_SUBMITTED_TIMEOUT`.
const NOTE_ENTRY_ERROR_ATTEMPTS: u8 = 8;
const NOTE_ENTRY_ERROR_BACKOFF: Duration = Duration::from_millis(25);

/// Decision for a snipe buy that was sent but whose fill never appeared in the
/// feed within the poll window. Funnel through `classify_swap_revert` (curve buy
/// — snipes are curve-only) so a futile revert gives up instead of re-paying fees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SilentSendOutcome {
    Resend,
    RefreshCreatorThenResend,
    WaitThenSettle,
    GiveUp,
}

fn classify_silent_send<E>(status: &Result<SigStatus, E>) -> SilentSendOutcome {
    match status {
        Ok(SigStatus::Reverted { custom }) => {
            match classify_swap_revert(*custom, SwapRoute::Curve, SwapDirection::Buy) {
                SwapRetryDecision::Retry => SilentSendOutcome::Resend,
                SwapRetryDecision::RefreshCreator => SilentSendOutcome::RefreshCreatorThenResend,
                _ => SilentSendOutcome::GiveUp,
            }
        }
        Ok(SigStatus::Succeeded) => SilentSendOutcome::WaitThenSettle,
        Ok(SigStatus::Pending) | Err(_) => SilentSendOutcome::GiveUp,
    }
}

/// Recovery verdict for a `BuySubmitted` row — reaper never re-sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuyRecoveryVerdict {
    Drop,
    Wait,
}

/// Classify one submitted buy signature's on-chain status for recovery.
pub(crate) fn classify_submitted_buy<E>(status: &Result<Option<bool>, E>) -> BuyRecoveryVerdict {
    match status {
        Ok(Some(false)) => BuyRecoveryVerdict::Drop,
        Ok(Some(true)) | Ok(None) | Err(_) => BuyRecoveryVerdict::Wait,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SellConfirmAction {
    Reclassify(SwapRetryDecision),
    WaitConfirm,
}

fn classify_sell_confirm<E>(
    state: &Result<SigStatus, E>,
    used_migrated: bool,
    now_migrated: bool,
) -> SellConfirmAction {
    match state {
        Ok(SigStatus::Reverted { custom }) if used_migrated == now_migrated => {
            let route = if used_migrated { SwapRoute::Amm } else { SwapRoute::Curve };
            SellConfirmAction::Reclassify(classify_swap_revert(*custom, route, SwapDirection::Sell))
        }
        Ok(SigStatus::Reverted { .. }) => SellConfirmAction::Reclassify(SwapRetryDecision::Retry),
        Ok(SigStatus::Succeeded) | Ok(SigStatus::Pending) | Err(_) => SellConfirmAction::WaitConfirm,
    }
}

/// The dependencies a real submit needs — cheap Arc-backed clones.
#[derive(Clone)]
pub struct RealExecDeps {
    pub trader: Arc<PumpFunTrader>,
    pub token_cache: Arc<TokenCache>,
    pub trade_repo: TradeRepo,
    pub strategy_repo: StrategyRepo,
    /// Durable `tokens_info.is_migrated` source. The exit loop consults it when the
    /// volatile token cache aged out on a long hold, so a cache miss can't route a
    /// doomed curve sell into a migrated pool. Off the snipe hot path (exit only).
    pub token_info_repo: TokenInfoRepo,
    pub trade_signals: Arc<TradeSignals>,
    pub fill_sigs: FillSigStore,
    pub fill_tx: mpsc::Sender<Event>,
    pub inflight: InFlightGuards,
    /// Process-local signed-buy journal (M2) — sync before send; PG is best-effort.
    pub buy_journal: SubmittedBuyJournal,
    /// Live position registry — sibling ExternallyCleared after a mint clears.
    pub registry: PositionRegistry,
    /// Engine event channel (same as the decision loop's fill_tx). When set,
    /// successful mint clears fold `ExternallyCleared` for siblings still in-engine.
    /// Orphan sells use a private fill_tx for their own outcome, so they pass the
    /// loop channel here separately.
    pub engine_fill_tx: Option<mpsc::Sender<Event>>,
}

/// Everything a buy submit needs, resolved by the loop from the cache + rule.
pub struct BuyOrder {
    pub intent: IntentId,
    pub pg_id: Uuid,
    pub mint: String,
    pub creator: String,
    pub token_program_id: String,
    pub lamports: u64,
    pub cashback_enabled: bool,
    pub slippage_bps: Option<u64>,
    /// Token account an earlier attempt for this position already funded, if any.
    /// Passed back as the buy's account override so a retry cannot mint a second
    /// seeded account and split this position's bag across two of them.
    pub token_account: Option<String>,
}

/// Everything a sell submit needs, resolved by the loop from the position meta.
pub struct SellOrder {
    pub intent: IntentId,
    pub pg_id: Uuid,
    pub mint: String,
    pub token_amount: u64,
    pub token_account: Option<String>,
    pub creator: Option<String>,
    pub token_program_id: Option<String>,
    pub cashback_enabled: bool,
    pub slippage_bps: Option<u64>,
}

/// Submit a real buy, then confirm the fill off the feed (RPC watchdog fallback),
/// emitting the definitive `FillConfirmed`/`FillFailed` back to the engine.
pub async fn run_entry(deps: RealExecDeps, order: BuyOrder) {
    let Some(_guard) = deps.inflight.try_begin_entry(order.pg_id) else {
        // Another attempt for this position owns the send. It emits for ITS
        // intent, which the engine has already superseded with this one — so a
        // bare return would strand the arm in `EntryPending` (the engine's
        // `FillFailed` handler only folds a matching intent). Fail THIS intent
        // as retryable: the engine re-submits, and the bounded attempt ladder
        // terminates it if the guard is still held.
        warn!(pg = %order.pg_id, mint = %order.mint, "real buy: entry guard held — skipping");
        let _ = deps
            .fill_tx
            .send(Event::FillFailed { intent: order.intent, reason: FillFailReason::Reverted })
            .await;
        return;
    };
    let wallet = deps.trader.wallet_pubkey();
    let sol_amount = order.lamports as f64 / 1_000_000_000.0;

    // SOL earmark — released on Fatal / sink terminal / sell-start / reaper drop.
    deps.trader.commit_sol_for_position(order.pg_id.to_string(), order.lamports);

    // Adopt a fill from signatures this position already submitted (engine retry
    // or crash between sign and confirm) before sending again. First attempt
    // (empty journal) skips the PG round-trip entirely.
    if let Some(legs) = adopt_existing_fill(&deps, &wallet, &order).await {
        // Adopted a fill from a signature this position already sent — the account
        // that buy funded is whatever the position recorded then (falls back to the
        // per-mint cache inside `emit_entry_filled` when it recorded nothing).
        let known_account = order.token_account.clone();
        emit_entry_filled(&deps, &order, legs.0, known_account, legs.1).await;
        return;
    }

    // Entry migration gate (option a): the snipe is CURVE-ONLY, so a token that has
    // already migrated to the AMM can only revert on the curve. Skip migrated
    // tokens entirely rather than chase them on the AMM. Cache-only (in-RAM) read —
    // no DB/RPC on the snipe hot path; the migrate-during-window race is caught in
    // `confirm_entry` below.
    if deps.token_cache.get(&order.mint).map(|e| e.value().is_migrated).unwrap_or(false) {
        info!(
            mint = %order.mint,
            "real buy skipped — token already migrated (curve snipe would revert)"
        );
        deps.trader.release_sol_for_position(&order.pg_id.to_string());
        note_entry_error(
            &deps,
            &order,
            "skipped before send: token already migrated (curve-only snipe)",
        )
        .await;
        let _ = deps
            .fill_tx
            .send(Event::FillFailed { intent: order.intent, reason: FillFailReason::Fatal })
            .await;
        return;
    }

    let guard_sig = deps.trade_signals.register(&wallet, &order.mint);
    let reserves = reserves_from_cache(&deps.token_cache, &order.mint);

    let signed: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let on_signed = {
        let signed = signed.clone();
        let journal = deps.buy_journal.clone();
        let repo = deps.strategy_repo.clone();
        let pg = order.pg_id;
        let mint = order.mint.clone();
        Box::new(move |sig: String| {
            // Sync write-ahead (M2): process-local truth before any await so the
            // nonce slot is never held on PG. Durable persist is fire-and-forget
            // with a 250 ms bound (slow DB → warn + drop; journal + reaper cover).
            *signed.lock().unwrap() = Some(sig.clone());
            journal.append(pg, sig.clone());
            let repo = repo.clone();
            let mint = mint.clone();
            let sig_bg = sig.clone();
            tokio::spawn(async move {
                // Fire-and-forget: never blocks send. Retry briefly when the
                // background insert_position hasn't landed yet (Pass-1 async).
                let persist = async {
                    let mut attempt = 0u8;
                    loop {
                        match repo.mark_buy_submitted(pg, &sig_bg).await {
                            Ok(Some(_)) => return Ok(()),
                            Ok(None) => {
                                attempt = attempt.saturating_add(1);
                                if attempt >= 8 {
                                    return Err(
                                        "row missing or already Holding after retries".into(),
                                    );
                                }
                                tokio::time::sleep(Duration::from_millis(25)).await;
                            }
                            Err(e) => return Err(e.to_string()),
                        }
                    }
                };
                match tokio::time::timeout(MARK_BUY_SUBMITTED_TIMEOUT, persist).await {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => warn!(
                        mint = %mint, pg = %pg, sig = %sig_bg,
                        "mark_buy_submitted failed (journal kept): {e}"
                    ),
                    Err(_) => warn!(
                        mint = %mint, pg = %pg, sig = %sig_bg,
                        "mark_buy_submitted timed out after {}ms (journal kept)",
                        MARK_BUY_SUBMITTED_TIMEOUT.as_millis()
                    ),
                }
            });
            Box::pin(async {}) as std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>
        })
    };

    // Prior submitted sigs (engine Retry after a confirmed safe revert) climb
    // the tip ladder. Pending/ambiguous outcomes never resend here — that would
    // risk a double-buy if the first tx is still in flight.
    let tip_level = deps.buy_journal.sigs(order.pg_id).len().min(u8::MAX as usize) as u8;

    // Reuse the account a previous attempt for this position funded (if any), so a
    // retry buys into the SAME account instead of drawing another seeded one. With
    // nothing recorded, fall back to an account a *sibling* position on this mint
    // already holds.
    let reuse_account = match order.token_account.as_deref() {
        Some(a) => std::str::FromStr::from_str(a).ok(),
        None => resolve_sibling_account(&deps, &wallet, &order.mint)
            .await
            .and_then(|a| std::str::FromStr::from_str(&a).ok()),
    };

    let send = deps.trader.buy_token_snipe_write_ahead(
        &order.mint,
        &order.creator,
        &order.token_program_id,
        sol_amount,
        order.slippage_bps,
        reserves,
        on_signed,
        order.cashback_enabled,
        reuse_account,
        tip_level,
    );
    // Bounded send (mirrors the sell path). `None` = the timeout elapsed. We
    // still fall through with whatever `on_signed` recorded: a signature means
    // the tx IS out there, so the confirm below runs and the reaper can verify
    // it — only a send that never signed is a proven no-op we may terminate.
    let submit = match tokio::time::timeout(BUY_SEND_TIMEOUT, send).await {
        Ok(r) => Some(r),
        Err(_) => {
            warn!(
                mint = %order.mint, pg = %order.pg_id,
                "real buy send timed out after {}s (abandoning the send task)",
                BUY_SEND_TIMEOUT.as_secs()
            );
            None
        }
    };

    let signed_sig = signed.lock().unwrap().clone();
    // The account THIS buy funded. Authoritative — never re-read from the trader's
    // per-mint cache, which a concurrent same-mint snipe overwrites (see `SnipeBuy`).
    // The non-`Ok` arms have no such fact in hand: fall back to whatever the
    // position already knew, then the cache.
    let funded_account = match &submit {
        Some(Ok(b)) => Some(b.user_token_account.to_string()),
        _ => order.token_account.clone(),
    };
    // The submit error used to be dropped on the floor — not even logged — which is
    // half the reason the 2026-07-27 `EntryFailed` rows could not be explained. It
    // is only the *fallback* cause: when the buy did get signed we still confirm
    // on-chain below, and that verdict is more authoritative than the send error.
    let send_error = match &submit {
        Some(Ok(_)) => None,
        Some(Err(e)) => Some(format!("buy send failed: {e}")),
        None => {
            Some(format!("buy send timed out after {}s", BUY_SEND_TIMEOUT.as_secs()))
        }
    };
    // Say it out loud the moment it happens. A send that fails AFTER the tx was
    // signed used to vanish: the arms below prefer the on-chain verdict, so the
    // cause was dropped and the row was left `BuySubmitted` with a NULL
    // `last_entry_error` and not one log line. That is exactly how a server-side
    // `JITO_MIN_TIP_SOL` under the Helius Sender floor (which rejects
    // pre-broadcast with `-32602`) killed 13 consecutive real buys unnoticed
    // between 2026-07-31 and 08-03. The send error is never authoritative about
    // the FILL — but it is always the truth about the SEND, so it must be visible.
    if let Some(err) = &send_error {
        warn!(mint = %order.mint, pg = %order.pg_id, "{err}");
    }
    let submitted_sig = match submit {
        Some(Ok(b)) => Some(b.signature),
        _ => None,
    };
    match (submitted_sig, signed_sig) {
        (Some(sig), _) | (None, Some(sig)) => {
            let outcome =
                confirm_entry(&deps, &guard_sig, &wallet, &order.mint, &sig, ENTRY_FEED_WINDOW)
                    .await;
            emit_entry_outcome(&deps, &order, sig, funded_account, outcome, send_error).await;
        }
        (None, None) => {
            let cause = send_error
                .unwrap_or_else(|| "buy was never signed (no signature to confirm)".to_string());
            note_entry_error(&deps, &order, &cause).await;
            warn!(mint = %order.mint, "real buy never signed; reporting revert for retry");
            let _ = deps
                .fill_tx
                .send(Event::FillFailed {
                    intent: order.intent,
                    reason: FillFailReason::Reverted,
                })
                .await;
        }
    }
}

/// The token account an already-open position on this `(wallet, mint)` holds, so a
/// second buy into a mint the wallet is already in lands in that same account
/// instead of the template pool minting another one. Two positions sharing ONE
/// account is the intended shape: each sells its own `token_amount`, and
/// `has_other_open_position_on_mint` already gates the rent-reclaim close so
/// neither closes it out from under the other.
///
/// **Skipped entirely unless the trader has already resolved an account for this
/// mint in-process.** That check is a pure in-memory read, so the snipe path — a
/// mint nobody has touched — pays nothing and reaches the send with no DB round
/// trip. Only a re-buy into an already-traded mint pays the (indexed, local) query,
/// and that is never the latency-critical case.
///
/// Best-effort by design, never load-bearing: a cold cache after a restart, or a
/// sibling that settles and reclaims the account inside this window, just means the
/// buy draws a fresh seeded account. Attribution stays correct either way because
/// the position records the account its own buy returned (`SnipeBuy`).
async fn resolve_sibling_account(
    deps: &RealExecDeps,
    wallet: &str,
    mint: &str,
) -> Option<String> {
    deps.trader.cached_token_account(mint)?;
    match deps.strategy_repo.find_reusable_token_account(wallet, mint, "real").await {
        Ok(Some(acct)) => {
            info!(mint = %mint, account = %acct, "re-buying into the account this mint is already held in");
            Some(acct)
        }
        Ok(None) => None,
        Err(e) => {
            warn!(mint = %mint, "reusable-account lookup failed; drawing a fresh account: {e}");
            None
        }
    }
}

/// Try to adopt a fill already indexed for this position's submitted signatures.
/// Uses the process-local journal only — empty ⇒ first attempt, skip PG. Cross-
/// process orphans are the reaper's job (`find_all_buy_submitted`).
async fn adopt_existing_fill(
    deps: &RealExecDeps,
    wallet: &str,
    order: &BuyOrder,
) -> Option<(String, SigLegs)> {
    let sigs = deps.buy_journal.sigs(order.pg_id);
    if sigs.is_empty() {
        return None;
    }
    for sig in &sigs {
        if let Some(obs) = deps.trade_signals.observed_legs(sig) {
            if obs.token_amount > PARTIAL_FILL_THRESHOLD {
                info!(mint = %order.mint, sig = %sig, "adopted own-leg preview before re-send");
                return Some((
                    sig.clone(),
                    SigLegs {
                        token_amount: obs.token_amount,
                        amount_sol: obs.amount_sol,
                        first_block_time: obs.first_block_time,
                        last_block_time: obs.last_block_time,
                    },
                ));
            }
        }
        if let Ok(Some(legs)) = deps.trade_repo.find_fill_by_signature(wallet, &order.mint, sig).await
        {
            if legs.token_amount > PARTIAL_FILL_THRESHOLD {
                info!(mint = %order.mint, sig = %sig, "adopted existing buy fill before re-send");
                return Some((sig.clone(), legs));
            }
        }
    }
    None
}

async fn emit_entry_filled(
    deps: &RealExecDeps,
    order: &BuyOrder,
    sig: String,
    funded_account: Option<String>,
    legs: SigLegs,
) {
    // The account this position's buy actually funded. Falls back to the per-mint
    // cache only when the buy returned no fact (never-signed / adopt paths) — that
    // cache is unreliable under concurrent same-mint snipes (see `SnipeBuy`).
    let token_account =
        funded_account.or_else(|| deps.trader.cached_token_account(&order.mint));
    deps.fill_sigs
        .put(order.intent.clone(), FillSigs { sigs: vec![sig], token_account });
    let _ = deps
        .fill_tx
        .send(Event::FillConfirmed {
            intent: order.intent.clone(),
            fill: Fill {
                price: legs.price_per_token(),
                sol: legs.amount_sol,
                token_amount: legs.token_amount,
                at: legs.last_block_time,
            },
        })
        .await;
}

async fn emit_entry_outcome(
    deps: &RealExecDeps,
    order: &BuyOrder,
    sig: String,
    funded_account: Option<String>,
    outcome: EntryOutcome,
    // Why the submit call failed, when it did. Not authoritative about the fill
    // (the tx was already signed, so it may still be on the wire) — but it is the
    // ONLY explanation available for an `Ambiguous` row, so it is persisted there
    // instead of being dropped.
    send_error: Option<String>,
) {
    match outcome {
        EntryOutcome::Filled(legs) => {
            emit_entry_filled(deps, order, sig, funded_account, legs).await
        }
        EntryOutcome::Retry(cause) => {
            note_entry_error(deps, order, &cause).await;
            let _ = deps
                .fill_tx
                .send(Event::FillFailed {
                    intent: order.intent.clone(),
                    reason: FillFailReason::Reverted,
                })
                .await;
        }
        EntryOutcome::Fatal(cause) => {
            note_entry_error(deps, order, &cause).await;
            deps.trader.release_sol_for_position(&order.pg_id.to_string());
            let _ = deps
                .fill_tx
                .send(Event::FillFailed {
                    intent: order.intent.clone(),
                    reason: FillFailReason::Fatal,
                })
                .await;
        }
        // Ambiguous: leave BuySubmitted for the reaper — never resend. Record the
        // send failure (when there was one) so the row carries its own diagnosis:
        // a send that never reached the network and a send that landed but hasn't
        // been indexed yet are indistinguishable from the row alone, and only the
        // first one is a bug. Status is deliberately unchanged — the tx was signed,
        // so it may still execute, and only the reaper may retire the row.
        EntryOutcome::Ambiguous => {
            match send_error {
                Some(cause) => {
                    note_entry_error(
                        deps,
                        order,
                        &format!("{cause} (tx was signed; left BuySubmitted for the reaper)"),
                    )
                    .await;
                }
                None => warn!(
                    mint = %order.mint,
                    "real buy outcome ambiguous — left BuySubmitted for the reaper"
                ),
            }
        }
    }
}

/// Persist why this buy attempt did not fill, and log it. The ONE call path into
/// `last_entry_error` (mig 0017).
///
/// Failure path only — never the snipe hot path. Retries briefly on a row that
/// hasn't landed yet: `insert_position` is asynchronous (Pass-1), so the
/// already-migrated skip can outrun its own insert. Best-effort throughout; a
/// failed diagnostic write must never change the entry's outcome.
async fn note_entry_error(deps: &RealExecDeps, order: &BuyOrder, cause: &str) {
    warn!(mint = %order.mint, pg = %order.pg_id, "real buy attempt did not fill: {cause}");
    for _ in 0..NOTE_ENTRY_ERROR_ATTEMPTS {
        match deps.strategy_repo.note_last_entry_error(order.pg_id, cause).await {
            Ok(true) => return,
            Ok(false) => tokio::time::sleep(NOTE_ENTRY_ERROR_BACKOFF).await,
            Err(e) => {
                warn!(pg = %order.pg_id, "note_last_entry_error failed: {e}");
                return;
            }
        }
    }
    warn!(pg = %order.pg_id, "note_last_entry_error: row never appeared — cause is log-only");
}

/// Describe a `SigStatus` for `last_entry_error`. The Anchor custom code is the
/// whole point: `6002`/`6042` mean the buy slippage floor is too tight for the
/// current market (a TUNING fix), anything else points at code.
fn describe_status<E: std::fmt::Display>(status: &Result<SigStatus, E>) -> Cow<'static, str> {
    match status {
        Ok(SigStatus::Reverted { custom: Some(code) }) => {
            Cow::Owned(format!("reverted on-chain, curve buy error {code}"))
        }
        Ok(SigStatus::Reverted { custom: None }) => {
            Cow::Borrowed("reverted on-chain, no Anchor code (account / funds error)")
        }
        Ok(SigStatus::Pending) => Cow::Borrowed("never landed (pending or dropped)"),
        Ok(SigStatus::Succeeded) => Cow::Borrowed("tx succeeded but no own buy leg on the feed"),
        Err(e) => Cow::Owned(format!("signature status unavailable: {e}")),
    }
}

/// How a buy attempt ended. The non-fill arms carry the **cause** — the send
/// error or the Anchor custom code — because it is the only fact that explains an
/// `EntryFailed` row, and the engine has no `ExitReason` for a position that never
/// opened. It is persisted to `last_entry_error` (mig 0017) so the
/// slippage-revert-vs-structural question is answerable from the DB rather than
/// from container logs on the box.
enum EntryOutcome {
    Filled(SigLegs),
    Retry(Cow<'static, str>),
    Fatal(Cow<'static, str>),
    /// Submitted, neither a feed fill nor a proven revert — emits nothing and
    /// leaves the `BuySubmitted` row for the reaper, so there is nothing to book.
    Ambiguous,
}

async fn confirm_entry(
    deps: &RealExecDeps,
    guard: &trading_core::state::trade_signals::WaitGuard,
    wallet: &str,
    mint: &str,
    sig: &str,
    window: Duration,
) -> EntryOutcome {
    if let Some(legs) = poll_feed_buy(deps, wallet, mint, sig, guard, window).await {
        return EntryOutcome::Filled(legs);
    }
    // Race (option a): the token migrated during the buy window. The feed poll above
    // saw no own-leg over the full window, and a curve snipe into a completed curve
    // cannot fill — so give up decisively instead of chasing the AMM or spending a
    // `signature_state_detailed` RPC just to confirm the inevitable curve revert.
    if deps.token_cache.get(mint).map(|e| e.value().is_migrated).unwrap_or(false) {
        warn!(
            mint = %mint,
            "buy unfilled and token migrated during window — giving up (curve-only snipe)"
        );
        return EntryOutcome::Fatal(Cow::Borrowed(
            "token migrated during the buy window (a curve-only snipe cannot fill)",
        ));
    }
    let status = deps.trader.signature_state_detailed(sig).await;
    match classify_silent_send(&status) {
        SilentSendOutcome::Resend => EntryOutcome::Retry(describe_status(&status)),
        SilentSendOutcome::RefreshCreatorThenResend => {
            match deps.trader.refresh_curve_creator_vault(mint).await {
                Ok(Some(vault)) => {
                    warn!(mint = %mint, new_creator_vault = %vault,
                        "buy reverted 2006; refreshed creator — reporting retry");
                    EntryOutcome::Retry(Cow::Borrowed(
                        "reverted 2006 (stale creator); creator refreshed, retrying",
                    ))
                }
                Ok(None) => EntryOutcome::Fatal(Cow::Borrowed(
                    "reverted 2006 but the creator vault is unchanged",
                )),
                Err(e) => EntryOutcome::Fatal(Cow::Owned(format!(
                    "reverted 2006 and the creator refresh failed: {e}"
                ))),
            }
        }
        SilentSendOutcome::WaitThenSettle => {
            match poll_feed_buy(deps, wallet, mint, sig, guard, EXTENDED_FEED_WINDOW).await {
                Some(legs) => EntryOutcome::Filled(legs),
                None => EntryOutcome::Ambiguous,
            }
        }
        SilentSendOutcome::GiveUp => match &status {
            // Pending/unknown → ambiguous (nonce may still land); structural revert → Fatal.
            Ok(SigStatus::Pending) | Err(_) => EntryOutcome::Ambiguous,
            Ok(SigStatus::Reverted { .. }) => EntryOutcome::Fatal(describe_status(&status)),
            Ok(SigStatus::Succeeded) => EntryOutcome::Ambiguous,
        },
    }
}

async fn poll_feed_buy(
    deps: &RealExecDeps,
    wallet: &str,
    mint: &str,
    sig: &str,
    guard: &trading_core::state::trade_signals::WaitGuard,
    window: Duration,
) -> Option<SigLegs> {
    let deadline = tokio::time::Instant::now() + window;
    loop {
        // Prefer process-local own-leg preview (ingest saw our buy before PG).
        if let Some(obs) = deps.trade_signals.observed_legs(sig) {
            if obs.token_amount > PARTIAL_FILL_THRESHOLD {
                return Some(SigLegs {
                    token_amount: obs.token_amount,
                    amount_sol: obs.amount_sol,
                    first_block_time: obs.first_block_time,
                    last_block_time: obs.last_block_time,
                });
            }
        }
        if let Ok(Some(legs)) = deps.trade_repo.find_fill_by_signature(wallet, mint, sig).await {
            if legs.token_amount > PARTIAL_FILL_THRESHOLD {
                return Some(legs);
            }
        }
        if tokio::time::Instant::now() >= deadline {
            return None;
        }
        tokio::select! {
            _ = guard.notified() => {}
            _ = tokio::time::sleep(FEED_POLL) => {}
        }
    }
}

/// Submit a real sell (escalating tip across attempts), confirm by summing the
/// position's OWN sell legs vs the held amount, and emit the definitive outcome.
pub async fn run_exit(deps: RealExecDeps, order: SellOrder) {
    let Some(_guard) = deps.inflight.try_begin_exit(order.pg_id) else {
        warn!(pg = %order.pg_id, mint = %order.mint, "real sell: exit guard held — skipping");
        return;
    };
    // One sell per mint (shared ATA) — siblings wait for reaper/nudge rather than
    // fanning out parallel Helius sell sends.
    let Some(_mint_guard) = deps.inflight.try_begin_exit_mint(&order.mint) else {
        warn!(pg = %order.pg_id, mint = %order.mint, "real sell: mint exit lock held — skipping");
        return;
    };

    // Release FIRST — must fire even if the process crashes mid-exit.
    deps.trader.release_sol_for_position(&order.pg_id.to_string());

    if order.token_amount == 0 {
        // Nothing to sell — treat as cleared at zero.
        let _ = deps
            .fill_tx
            .send(Event::FillConfirmed {
                intent: order.intent.clone(),
                fill: Fill { price: 0.0, sol: 0.0, token_amount: 0, at: chrono::Utc::now() },
            })
            .await;
        return;
    }

    let wallet = deps.trader.wallet_pubkey();
    let feed_guard = deps.trade_signals.register(&wallet, &order.mint);
    let mut sell_sigs: Vec<String> = Vec::new();
    let mut cashback = order.cashback_enabled;
    let base_program = order.token_program_id.clone().unwrap_or_default();

    // Durable migration seed (option 2): the volatile token cache can age out on a
    // long hold, and reading `is_migrated` from it alone (`.unwrap_or(false)`) would
    // route a doomed CURVE sell into a migrated AMM pool. Migration is monotonic, so
    // consult the sticky durable `tokens_info.is_migrated` ONCE up front when the
    // cache doesn't already know — off the snipe hot path (this is an exit): one
    // indexed read per exit only, and skipped entirely when the cache is warm.
    let durable_migrated = if deps
        .token_cache
        .get(&order.mint)
        .map(|e| e.value().is_migrated)
        .unwrap_or(false)
    {
        true
    } else {
        deps.token_info_repo.is_migrated(&order.mint).await.unwrap_or(false)
    };

    // Route state OWNED BY THIS LOOP. Seeded from the durable flag, latched true by a
    // 6005 revert (see `RerouteMigrated`). Deliberately NOT stored only in the token
    // cache: `get_mut` returns `None` when the entry has aged out — which is exactly
    // the long-hold case `durable_migrated` exists for — so a cache-only flip would be
    // silently dropped and every remaining attempt would re-route to the dead curve.
    let mut route_migrated = durable_migrated;

    for attempt in 0..SELL_ATTEMPTS {
        // Loop-owned latch OR the live cache (ingest may flip it true mid-hold) — so
        // the FIRST attempt already routes to the AMM when migration is known.
        let is_migrated = route_migrated
            || deps.token_cache.get(&order.mint).map(|e| e.value().is_migrated).unwrap_or(false);

        let submit = {
            let send = submit_one_sell(&deps, &order, attempt, is_migrated, cashback);
            match tokio::time::timeout(SELL_SEND_TIMEOUT, send).await {
                Ok(r) => r,
                Err(_) => {
                    warn!(mint = %order.mint, attempt, "sell send timed out");
                    Err(pump_trader::TradeError::Other("sell send timed out".into()))
                }
            }
        };

        let sig = match submit {
            Ok(Some(sig)) => {
                sell_sigs.push(sig.clone());
                Some(sig)
            }
            Ok(None) => None,
            Err(e) => {
                warn!(mint = %order.mint, attempt, "real sell attempt failed: {e}");
                None
            }
        };

        if let Some(legs) =
            confirm_sell(&deps, &feed_guard, &wallet, &order, &sell_sigs, SELL_CONFIRM_WINDOW).await
        {
            finish_cleared_sell(&deps, &order, &sell_sigs, legs).await;
            return;
        }

        let Some(sig) = sig else {
            continue;
        };

        let now_migrated = route_migrated
            || deps.token_cache.get(&order.mint).map(|e| e.value().is_migrated).unwrap_or(is_migrated);
        let state = deps.trader.signature_state_detailed(&sig).await;
        match classify_sell_confirm(&state, is_migrated, now_migrated) {
            SellConfirmAction::WaitConfirm => {
                if let Some(legs) = confirm_sell(
                    &deps,
                    &feed_guard,
                    &wallet,
                    &order,
                    &sell_sigs,
                    SELL_UNCONFIRMED_EXTENDED,
                )
                .await
                {
                    finish_cleared_sell(&deps, &order, &sell_sigs, legs).await;
                    return;
                }
                info!(mint = %order.mint, "sell unconfirmed after extended poll — not re-sending");
                fail_exit(&deps, &order, &sell_sigs, FillFailReason::Unconfirmed).await;
                return;
            }
            SellConfirmAction::Reclassify(decision) => {
                match decision {
                    SwapRetryDecision::StopFeeBurn => {
                        warn!(mint = %order.mint, attempt, "sell structural revert — Fatal");
                        fail_exit(&deps, &order, &sell_sigs, FillFailReason::Fatal).await;
                        return;
                    }
                    SwapRetryDecision::RefreshCreator => {
                        match deps.trader.refresh_curve_creator_vault(&order.mint).await {
                            Ok(Some(_)) => {}
                            Ok(None) | Err(_) => {
                                fail_exit(&deps, &order, &sell_sigs, FillFailReason::Fatal).await;
                                return;
                            }
                        }
                    }
                    SwapRetryDecision::RefreshCoinCreator => {
                        match deps.trader.refresh_amm_pool_info(&order.mint, &base_program).await {
                            Ok(Some(_)) => {}
                            Ok(None) | Err(_) => {
                                fail_exit(&deps, &order, &sell_sigs, FillFailReason::Fatal).await;
                                return;
                            }
                        }
                    }
                    SwapRetryDecision::RefreshCashback => {
                        match deps.trader.refresh_curve_facts(&order.mint).await {
                            Ok(facts) => {
                                cashback = facts.cashback_enabled;
                                if let Some(mut e) = deps.token_cache.get_mut(&order.mint) {
                                    e.token.is_cashback_enabled = facts.cashback_enabled;
                                }
                            }
                            Err(_) => {
                                fail_exit(&deps, &order, &sell_sigs, FillFailReason::Fatal).await;
                                return;
                            }
                        }
                    }
                    SwapRetryDecision::RerouteMigrated => {
                        // 6005 `BondingCurveComplete` IS the proof of migration — the
                        // curve program itself refused the swap because the curve is
                        // complete. Re-confirming it with an on-chain
                        // `refresh_curve_facts` read bought no information and added a
                        // failure mode: an RPC hiccup fell through to `Fatal` and
                        // stranded a sellable bag (observed 2026-07-28, mint 57aJ…).
                        // Latch the route and retry on the AMM — no RPC. The pool
                        // address is a pure PDA derivation (`derive_amm_pool`), so
                        // `amm_sell` needs no lookup to build the next attempt.
                        warn!(
                            mint = %order.mint, attempt,
                            "curve sell reverted 6005 (BondingCurveComplete) — latching AMM route"
                        );
                        route_migrated = true;
                        if let Some(mut e) = deps.token_cache.get_mut(&order.mint) {
                            e.is_migrated = true;
                        }
                        // Durable, monotonic (`is_migrated OR EXCLUDED`) so a later
                        // exit / reaper redrive on a cold cache routes correctly even
                        // across a restart. Off the hot path and best-effort: a write
                        // failure must never strand the bag we are mid-sell on.
                        let repo = deps.token_info_repo.clone();
                        let mint = order.mint.clone();
                        tokio::spawn(async move {
                            if let Err(e) = repo.update_migration_status(&mint, true).await {
                                warn!(mint = %mint, "durable is_migrated write failed: {e}");
                            }
                        });
                    }
                    SwapRetryDecision::Retry => {
                        // tip escalates on next attempt
                    }
                }
            }
        }
    }

    let reason = if sell_sigs.is_empty() {
        FillFailReason::Reverted
    } else {
        FillFailReason::Unconfirmed
    };
    info!(mint = %order.mint, ?reason, "real sell unresolved after {SELL_ATTEMPTS} attempts");
    fail_exit(&deps, &order, &sell_sigs, reason).await;
}

/// Emit an exit `FillFailed` **after** stashing whatever sells were actually
/// submitted. Previously `sell_sigs` was only persisted inside
/// [`finish_cleared_sell`], so every failure path dropped it on the floor — a row
/// could sit in `ExitUnconfirmed` with `exit_tx_signatures = []` despite having
/// sent sells (observed 2026-07-28, mint 57aJ…). That is exactly the state where
/// the signatures matter most: they are what a manual Verify, the reaper, and the
/// `uq_strategy_positions_exit_sig0` dedup index need in order to tell "landed
/// late" from "never landed".
async fn fail_exit(
    deps: &RealExecDeps,
    order: &SellOrder,
    sell_sigs: &[String],
    reason: FillFailReason,
) {
    if !sell_sigs.is_empty() {
        let token_account =
            order.token_account.clone().or_else(|| deps.trader.cached_token_account(&order.mint));
        deps.fill_sigs.put(
            order.intent.clone(),
            FillSigs { sigs: sell_sigs.to_vec(), token_account },
        );
    }
    let _ = deps
        .fill_tx
        .send(Event::FillFailed { intent: order.intent.clone(), reason })
        .await;
}

async fn finish_cleared_sell(
    deps: &RealExecDeps,
    order: &SellOrder,
    sell_sigs: &[String],
    legs: SigLegs,
) {
    let token_account =
        order.token_account.clone().or_else(|| deps.trader.cached_token_account(&order.mint));
    deps.fill_sigs.put(
        order.intent.clone(),
        FillSigs { sigs: sell_sigs.to_vec(), token_account },
    );
    let fill = Fill {
        price: legs.price_per_token(),
        sol: legs.amount_sol,
        token_amount: legs.token_amount,
        at: legs.last_block_time,
    };
    let _ = deps
        .fill_tx
        .send(Event::FillConfirmed {
            intent: order.intent.clone(),
            fill,
        })
        .await;

    // Sibling book-close when the wallet mint bag is gone (PG net — no RPC).
    let wallet = deps.trader.wallet_pubkey();
    let engine_tx = deps.engine_fill_tx.clone().unwrap_or_else(|| deps.fill_tx.clone());
    let _ = orphan_exit::close_siblings_if_mint_cleared(
        &deps.strategy_repo,
        &deps.trade_repo,
        &deps.registry,
        &engine_tx,
        &wallet,
        &order.mint,
        order.pg_id,
        &fill,
    )
    .await;

    // Fire-and-forget rent reclaim when no sibling still shares the account.
    let trader = deps.trader.clone();
    let repo = deps.strategy_repo.clone();
    let mint = order.mint.clone();
    let pg = order.pg_id;
    tokio::spawn(async move {
        reclaim_token_account_if_last(&trader, &repo, &wallet, &mint, "real", pg).await;
    });
}

/// Reclaim rent only when no OTHER open position shares `(wallet, mint)` (M1).
pub(crate) async fn reclaim_token_account_if_last(
    trader: &Arc<PumpFunTrader>,
    repo: &StrategyRepo,
    wallet: &str,
    mint: &str,
    mode: &str,
    exclude_position: Uuid,
) {
    match repo.has_other_open_position_on_mint(wallet, mint, mode, exclude_position).await {
        Ok(true) => return,
        Ok(false) => {}
        Err(err) => {
            warn!(mint = %mint, "rent-reclaim other-open check failed; deferring: {err}");
            return;
        }
    }
    if let Err(err) = trader.close_token_account(mint, None).await {
        tracing::debug!(mint = %mint, "rent-reclaim close skipped: {err}");
    }
}

async fn submit_one_sell(
    deps: &RealExecDeps,
    order: &SellOrder,
    attempt: u8,
    is_migrated: bool,
    cashback: bool,
) -> pump_trader::Result<Option<String>> {
    if is_migrated {
        deps.trader
            .amm_sell(
                &order.mint,
                order.token_amount,
                order.token_program_id.as_deref().unwrap_or_default(),
                None,
                order.token_account.as_deref(),
                order.slippage_bps,
                attempt,
                false,
            )
            .await
    } else {
        // Cache reserves for min_out when sell slippage is configured — mirrors
        // snipe buy; avoids a cold `curve_reserves` RPC on the exit hot path.
        let reserves = reserves_from_cache(&deps.token_cache, &order.mint);
        deps.trader
            .sell_token_once(
                &order.mint,
                order.token_amount,
                order.creator.as_deref(),
                cashback,
                order.token_account.as_deref(),
                order.slippage_bps,
                attempt,
                false,
                reserves,
            )
            .await
    }
}

async fn confirm_sell(
    deps: &RealExecDeps,
    guard: &trading_core::state::trade_signals::WaitGuard,
    wallet: &str,
    order: &SellOrder,
    sell_sigs: &[String],
    window: Duration,
) -> Option<SigLegs> {
    if sell_sigs.is_empty() {
        return None;
    }
    let deadline = tokio::time::Instant::now() + window;
    let mut last_query: Option<tokio::time::Instant> = None;
    loop {
        let now = tokio::time::Instant::now();
        let at_deadline = now >= deadline;
        let rate_ok =
            at_deadline || last_query.map_or(true, |t| now.duration_since(t) >= SELL_BALANCE_QUERY_MIN_INTERVAL);
        if rate_ok {
            last_query = Some(now);
            // Prefer process-local own-leg preview before SQL.
            if let Some(obs) = deps.trade_signals.sum_observed_legs(sell_sigs) {
                if obs.token_amount.saturating_add(PARTIAL_FILL_THRESHOLD) >= order.token_amount {
                    return Some(SigLegs {
                        token_amount: obs.token_amount,
                        amount_sol: obs.amount_sol,
                        first_block_time: obs.first_block_time,
                        last_block_time: obs.last_block_time,
                    });
                }
            }
            if let Ok(Some(legs)) = deps
                .trade_repo
                .sum_legs_by_signatures(wallet, &order.mint, sell_sigs, TradeType::Sell)
                .await
            {
                if legs.token_amount.saturating_add(PARTIAL_FILL_THRESHOLD) >= order.token_amount {
                    return Some(legs);
                }
            }
        }
        if at_deadline {
            return None;
        }
        tokio::select! {
            _ = guard.notified() => {}
            _ = tokio::time::sleep(FEED_POLL) => {}
        }
    }
}

/// Slippage `min_out` reserves for the snipe buy, read from the cache (no RPC).
fn reserves_from_cache(token_cache: &Arc<TokenCache>, mint: &str) -> Option<(u128, u128)> {
    let (reserve_token, reserve_sol) = token_cache
        .get(mint)
        .map(|e| {
            let s = e.value();
            (s.current_reserve_token, s.current_reserve_sol)
        })
        .unwrap_or((None, None));
    snipe_reserves_from_cache(reserve_token, reserve_sol)
}

/// Convert the token cache's in-memory reserve pair into the snipe buy's
/// `(virtual_token, virtual_quote=lamports)` pair.
pub(crate) fn snipe_reserves_from_cache(
    reserve_token: Option<f64>,
    reserve_sol: Option<f64>,
) -> Option<(u128, u128)> {
    let vt = reserve_token?;
    let vsol = reserve_sol?;
    if vt <= 0.0 || vsol <= 0.0 {
        return None;
    }
    let vq_lamports = vsol * pump_trader::constants::LAMPORTS_PER_SOL as f64;
    Some((vt as u128, vq_lamports as u128))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_silent_send_buy_slippage_resends() {
        let status: Result<SigStatus, ()> =
            Ok(SigStatus::Reverted { custom: Some(6002) });
        assert_eq!(classify_silent_send(&status), SilentSendOutcome::Resend);
    }

    #[test]
    fn classify_silent_send_2006_refreshes() {
        let status: Result<SigStatus, ()> =
            Ok(SigStatus::Reverted { custom: Some(2006) });
        assert_eq!(
            classify_silent_send(&status),
            SilentSendOutcome::RefreshCreatorThenResend
        );
    }

    #[test]
    fn classify_silent_send_structural_gives_up() {
        let status: Result<SigStatus, ()> =
            Ok(SigStatus::Reverted { custom: Some(6022) });
        assert_eq!(classify_silent_send(&status), SilentSendOutcome::GiveUp);
    }

    #[test]
    fn classify_silent_send_succeeded_waits() {
        let status: Result<SigStatus, ()> = Ok(SigStatus::Succeeded);
        assert_eq!(classify_silent_send(&status), SilentSendOutcome::WaitThenSettle);
    }

    #[test]
    fn classify_submitted_buy_only_drops_proven_revert() {
        assert_eq!(
            classify_submitted_buy::<()>(&Ok(Some(false))),
            BuyRecoveryVerdict::Drop
        );
        assert_eq!(
            classify_submitted_buy::<()>(&Ok(Some(true))),
            BuyRecoveryVerdict::Wait
        );
        assert_eq!(
            classify_submitted_buy::<()>(&Ok(None)),
            BuyRecoveryVerdict::Wait
        );
    }

    /// 6005 on a curve sell must resolve to `RerouteMigrated`, and the exit loop
    /// must act on it WITHOUT re-confirming migration over RPC.
    ///
    /// Regression (2026-07-28, mint 57aJ…): the branch used to gate the reroute
    /// on `refresh_curve_facts`, whose `Err(_)` fell through to `Fatal` — an RPC
    /// blip turned a token that was merely migrated (and perfectly sellable on
    /// the AMM) into a permanently stranded bag. `BondingCurveComplete` is the
    /// curve program's own statement that the curve is done; there is nothing to
    /// re-confirm, and the AMM pool address is a pure PDA derivation.
    #[test]
    fn curve_sell_6005_reroutes_to_amm_without_an_rpc_reconfirm() {
        let state: Result<SigStatus, ()> =
            Ok(SigStatus::Reverted { custom: Some(6005) });
        assert_eq!(
            classify_sell_confirm(&state, false, false),
            SellConfirmAction::Reclassify(SwapRetryDecision::RerouteMigrated),
        );
        // And the same code on the AMM route is NOT a reroute (it would loop).
        assert_eq!(
            classify_swap_revert(Some(6005), SwapRoute::Amm, SwapDirection::Sell),
            SwapRetryDecision::StopFeeBurn,
        );
    }

    #[test]
    fn classify_sell_confirm_route_change_retries_on_revert() {
        let state: Result<SigStatus, ()> =
            Ok(SigStatus::Reverted { custom: Some(6003) });
        assert_eq!(
            classify_sell_confirm(&state, false, true),
            SellConfirmAction::Reclassify(SwapRetryDecision::Retry)
        );
    }

    #[test]
    fn classify_sell_confirm_succeeded_pending_rpc_error_wait_never_resend() {
        // C1: Succeeded / Pending / RPC-error must never trigger a second sell.
        let succeeded: Result<SigStatus, ()> = Ok(SigStatus::Succeeded);
        let pending: Result<SigStatus, ()> = Ok(SigStatus::Pending);
        let rpc_err: Result<SigStatus, ()> = Err(());
        assert_eq!(
            classify_sell_confirm(&succeeded, false, false),
            SellConfirmAction::WaitConfirm
        );
        assert_eq!(
            classify_sell_confirm(&pending, true, true),
            SellConfirmAction::WaitConfirm
        );
        assert_eq!(
            classify_sell_confirm(&rpc_err, false, false),
            SellConfirmAction::WaitConfirm
        );
    }
}
