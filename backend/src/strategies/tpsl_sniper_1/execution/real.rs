//! Real (on-chain) execution: snipe-buy a matched token and resolve its entry
//! fill from the WS/DB feed; sell a position out and close it. The double-buy
//! safety rule — *re-send only on a confirmed on-chain revert* — is centralized
//! in [`classify_silent_send`] and unit-tested without a live chain.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio::time::{sleep, Instant};
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::super::Tpsl1RuntimeCache;
use crate::models::Position;
use crate::state::token_cache::TokenCache;
use crate::state::trade_signals::TradeSignals;
use crate::storage::repositories::{
    tpsl1_position_repo::Tpsl1PositionRepo,
    trade_repo::{SigLegs, TradeRepo},
};
use crate::trader::{PumpFunTrader, SigStatus};

/// Decision for a snipe buy that was sent but whose fill never appeared in the
/// WS/DB feed within the poll window. Centralizing it keeps the **double-buy
/// safety rule** — *re-send only on a confirmed on-chain revert* — in one place
/// that is unit-tested without a live chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SilentSendOutcome {
    /// Tx reverted on-chain (no tokens bought) — safe to re-send.
    Resend,
    /// Tx landed but isn't indexed yet — wait for the row; never re-send (a
    /// second buy would double-fill).
    WaitThenSettle,
    /// Pending/dropped, or the status check failed — ambiguous, so give up rather
    /// than risk double-buying from a nonce tx that could still land.
    GiveUp,
}

/// Map a one-shot `signature_state` result to the post-poll action. Pure; generic
/// over the error type so tests need no `anyhow` value.
fn classify_silent_send<E>(status: &Result<Option<bool>, E>) -> SilentSendOutcome {
    match status {
        Ok(Some(false)) => SilentSendOutcome::Resend,
        Ok(Some(true)) => SilentSendOutcome::WaitThenSettle,
        Ok(None) | Err(_) => SilentSendOutcome::GiveUp,
    }
}

/// Convert the token cache's in-memory virtual reserves — token side in raw
/// units, SOL side in SOL (the decoder divides lamports by 1e9) — into the
/// `(virtual_token, virtual_quote=lamports)` pair the snipe buy's slippage
/// `min_out` expects. Returns `None` when either snapshot is missing or
/// non-positive, so the buy proceeds unprotected (`min_out=1`) rather than
/// blocking on an inline reserve RPC (the latency budget forbids it on this path).
pub(crate) fn snipe_reserves_from_cache(
    virtual_token_reserves: Option<f64>,
    virtual_sol_reserves: Option<f64>,
) -> Option<(u128, u128)> {
    let vt = virtual_token_reserves?;
    let vsol = virtual_sol_reserves?;
    if vt <= 0.0 || vsol <= 0.0 {
        return None;
    }
    let vq_lamports = vsol * pump_trader::constants::LAMPORTS_PER_SOL as f64;
    Some((vt as u128, vq_lamports as u128))
}

/// Off-chain dependency of the snipe buy flow: send a buy (no RPC confirm) and
/// classify a sent signature. A trait so `buy_until_filled_or_give_up` can be
/// driven by a scripted fake in tests instead of a live chain.
#[async_trait::async_trait]
pub(crate) trait SnipeExecutor: Send + Sync {
    fn wallet(&self) -> String;
    async fn send_snipe_buy(
        &self,
        mint: &str,
        creator: &str,
        token_program_id: &str,
        amount: f64,
        slippage_bps: Option<u64>,
        reserves: Option<(u128, u128)>,
    ) -> anyhow::Result<String>;
    async fn check_signature(&self, signature: &str) -> anyhow::Result<Option<bool>>;
}

#[async_trait::async_trait]
impl SnipeExecutor for PumpFunTrader {
    fn wallet(&self) -> String {
        self.wallet_pubkey()
    }
    async fn send_snipe_buy(
        &self,
        mint: &str,
        creator: &str,
        token_program_id: &str,
        amount: f64,
        slippage_bps: Option<u64>,
        reserves: Option<(u128, u128)>,
    ) -> anyhow::Result<String> {
        // Slippage min_out is derived from the triggering event's reserves
        // (threaded in, no RPC); a missing snapshot yields min_out=1 inside
        // `buy_token_snipe` — never an inline reserve read on the hot path.
        self.buy_token_snipe(mint, creator, token_program_id, amount, slippage_bps, reserves)
            .await
    }
    async fn check_signature(&self, signature: &str) -> anyhow::Result<Option<bool>> {
        self.signature_state(signature).await
    }
}

/// Retry/poll timing for [`buy_until_filled_or_give_up`]. `production()` mirrors
/// the `execution::BUY_*` constants; tests shrink it so the give-up and re-send
/// paths don't wait out real 12×1s poll windows.
#[derive(Clone, Copy)]
pub(crate) struct BuyRetryCfg {
    max_attempts: usize,
    backoff_ms: u64,
    poll_attempts: usize,
    poll_interval: Duration,
}

impl BuyRetryCfg {
    pub(crate) fn production() -> Self {
        Self {
            max_attempts: super::BUY_MAX_ATTEMPTS,
            backoff_ms: 500,
            poll_attempts: super::BUY_POLL_MAX_ATTEMPTS,
            poll_interval: Duration::from_millis(super::BUY_POLL_INTERVAL_MS),
        }
    }
}

/// Snipe-buy `mint` and record the entry fill on first sight in the WS feed,
/// retrying per `cfg`. Honors the double-buy invariant: only a confirmed
/// on-chain revert re-sends.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn buy_until_filled_or_give_up<E: SnipeExecutor + 'static>(
    trader: Arc<E>,
    mint: String,
    creator: String,
    token_program_id: String,
    buy_amount: f64,
    position_id: Uuid,
    position_repo: Tpsl1PositionRepo,
    trade_repo: TradeRepo,
    runtime: Arc<Tpsl1RuntimeCache>,
    trade_signals: Arc<TradeSignals>,
    cfg: BuyRetryCfg,
    // Effective slippage for the buy and the triggering event's curve reserves
    // (token, quote=lamports). `reserves = None` ⇒ min_out=1 (no RPC); see
    // `send_snipe_buy` / `buy_token_snipe`.
    slippage_bps: Option<u64>,
    reserves: Option<(u128, u128)>,
) {
    let wallet = trader.wallet();
    let max_attempts = cfg.max_attempts;
    let mut backoff_ms = cfg.backoff_ms;
    // The submitted signatures of OUR buys so far. Per-signature attribution: the
    // entry is recovered by one of THESE signatures, never the latest buy on the
    // shared (wallet, mint) feed — so two concurrent positions on the same token
    // can't adopt each other's fill (decision #2). Empty on the first attempt, so
    // the top guard is a no-op until we've actually sent something.
    let mut sent_sigs: Vec<String> = Vec::new();

    for attempt in 1..=max_attempts {
        // Never double-send: if one of our previous attempts' buys has since landed
        // and been indexed, adopt that fill instead of firing another buy.
        if adopt_existing_fill_if_present(&mint, &wallet, position_id, &position_repo, &trade_repo, &runtime, &sent_sigs)
            .await
        {
            return;
        }

        // Snipe send WITHOUT the blocking RPC confirm — the WS/DB trade feed below
        // is the sole confirmation and the entry-price source. `buy_token_snipe`
        // returns the submitted signature so a silent feed can be classified.
        let signature = match trader
            .send_snipe_buy(&mint, &creator, &token_program_id, buy_amount, slippage_bps, reserves)
            .await
        {
            Ok(sig) => sig,
            Err(err) => {
                warn!(mint = %mint, attempt, "buy send failed: {err}");
                if attempt < max_attempts {
                    sleep(Duration::from_millis(backoff_ms)).await;
                    backoff_ms = (backoff_ms * 2).saturating_add(100);
                }
                continue;
            }
        };
        // Track our own submitted signature so the fill is attributed to THIS buy.
        sent_sigs.push(signature.clone());
        info!(mint = %mint, attempt, sig = %signature,
            "buy submitted (no RPC confirm); polling WS feed for fill");

        // Poll the WS-fed trade feed for this wallet's buy row.
        if poll_feed_until_entry_fill(&mint, &wallet, position_id, &position_repo, &trade_repo, &runtime, &trade_signals, &cfg, &sent_sigs)
            .await
        {
            return;
        }

        // The fill never showed within the window. One status check decides the
        // next move; the decision table lives in `classify_silent_send` so the
        // double-buy invariant is unit-tested without a live chain.
        let status = trader.check_signature(&signature).await;
        match classify_silent_send(&status) {
            SilentSendOutcome::Resend => {
                // Reverted on-chain (e.g. slippage) → no tokens bought → safe to retry.
                warn!(mint = %mint, attempt, sig = %signature, "buy reverted on-chain; retrying");
            }
            SilentSendOutcome::WaitThenSettle => {
                // Landed, but the indexer is lagging. Re-sending would double-buy, so
                // wait one more window for the row rather than fire again.
                warn!(mint = %mint, sig = %signature,
                    "buy landed but not yet indexed; awaiting fill without re-send");
                if poll_feed_until_entry_fill(&mint, &wallet, position_id, &position_repo, &trade_repo, &runtime, &trade_signals, &cfg, &sent_sigs)
                    .await
                {
                    return;
                }
                warn!(mint = %mint, sig = %signature,
                    "buy confirmed on-chain but never indexed — leaving position unentered for review");
                return;
            }
            SilentSendOutcome::GiveUp => {
                // Pending/dropped, or the status check failed. A durable-nonce tx can
                // still land later, so re-sending risks a double-buy — give up instead.
                match &status {
                    Err(err) => warn!(mint = %mint, sig = %signature,
                        "buy status check failed: {err}; not re-sending (double-buy risk)"),
                    _ => warn!(mint = %mint, sig = %signature,
                        "buy neither indexed nor on-chain; not re-sending (double-buy risk)"),
                }
                return;
            }
        }

        if attempt < max_attempts {
            sleep(Duration::from_millis(backoff_ms)).await;
            backoff_ms = (backoff_ms * 2).saturating_add(100);
        } else {
            warn!(mint = %mint, "buy failed after {max_attempts} attempts");
        }
    }
}

/// Wait for this wallet's buy on `mint` to land in the WS-fed trade feed; on first
/// sight, record the entry and sync the runtime cache. Returns true once an entry
/// has been recorded, false if the window elapses without a fill.
///
/// Event-driven: the DbWriter [`notify`](TradeSignals::notify)s the moment the row
/// is persisted, so the common case adopts the fill immediately instead of waiting
/// out a poll tick. A fallback tick (the old `poll_interval`) and a hard deadline
/// equal to the old `poll_attempts × poll_interval` window are kept, so a missed
/// signal degrades to exactly the previous polling behaviour — never worse.
#[allow(clippy::too_many_arguments)]
async fn poll_feed_until_entry_fill(
    mint: &str,
    wallet: &str,
    position_id: Uuid,
    position_repo: &Tpsl1PositionRepo,
    trade_repo: &TradeRepo,
    runtime: &Arc<Tpsl1RuntimeCache>,
    trade_signals: &Arc<TradeSignals>,
    cfg: &BuyRetryCfg,
    sent_sigs: &[String],
) -> bool {
    let guard = trade_signals.register(wallet, mint);
    let deadline = Instant::now() + cfg.poll_interval * cfg.poll_attempts as u32;

    loop {
        // Arm the wakeup BEFORE the DB check so a notify that fires in the gap
        // between checking and awaiting isn't lost (tokio `notify_waiters` stores
        // no permit; `enable()` registers this waiter up front).
        let notified = guard.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();

        if adopt_existing_fill_if_present(mint, wallet, position_id, position_repo, trade_repo, runtime, sent_sigs).await
        {
            return true;
        }

        let now = Instant::now();
        if now >= deadline {
            return false;
        }
        let fallback = cfg.poll_interval.min(deadline - now);
        tokio::select! {
            _ = &mut notified => {}
            _ = sleep(fallback) => {}
        }
    }
}

/// If one of OUR sent buy signatures (`sent_sigs`) is already present in the trade
/// feed for `(wallet, mint)`, record it as the position entry (price/amount/tx/time
/// summed from that signature's legs) and return true; otherwise return false
/// without sleeping. Per-signature attribution: we adopt only a fill the bot itself
/// submitted, so a concurrent same-token position never adopts the other's buy.
async fn adopt_existing_fill_if_present(
    mint: &str,
    wallet: &str,
    position_id: Uuid,
    position_repo: &Tpsl1PositionRepo,
    trade_repo: &TradeRepo,
    runtime: &Arc<Tpsl1RuntimeCache>,
    sent_sigs: &[String],
) -> bool {
    for sig in sent_sigs {
        let fill = match trade_repo.find_fill_by_signature(wallet, mint, sig).await {
            Ok(Some(fill)) => fill,
            Ok(None) => continue,
            Err(err) => {
                warn!(mint = %mint, "failed to query trades for buy confirmation: {err}");
                continue;
            }
        };
        if let Ok(Some(prev)) = position_repo.find_by_id(position_id).await {
            // `update_entry` RETURNs the updated row, so we sync the cache off it
            // directly — no follow-up read of the row we just wrote.
            match position_repo
                .update_entry(
                    position_id,
                    sig,
                    fill.token_amount,
                    fill.price_per_token(),
                    fill.first_block_time,
                )
                .await
            {
                Ok(current) => {
                    runtime.sync_position(Some(&prev), &current);
                    info!(mint = %mint, tx = %sig, "position entry recorded from buy fill");
                }
                Err(err) => warn!(mint = %mint, "failed to update position entry after buy: {err}"),
            }
        }
        return true;
    }
    false
}

/// Sell a position's full balance out (retrying / re-routing across migration)
/// and, on a confirmed clear, close it; otherwise mark it ExitFailed at the
/// trigger price. `trigger_price`/`trigger_time` are the hypothetical exit
/// recorded if the sell never confirms; `exit_reason` is persisted either way.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn sell_and_close_position(
    trader: Arc<PumpFunTrader>,
    mut position: Position,
    position_repo: Tpsl1PositionRepo,
    trade_repo: TradeRepo,
    runtime: Arc<Tpsl1RuntimeCache>,
    cache: &TokenCache,
    trade_signals: Arc<TradeSignals>,
    trigger_price: f64,
    trigger_time: DateTime<Utc>,
    exit_reason: String,
    slippage_bps: u64,
) {
    let mint = position.mint.clone();
    let target_tokens = position.entry_token_amount.unwrap_or(0.0);
    let amount = target_tokens as u64;
    let base_token_program = position
        .token_program_id
        .clone()
        .unwrap_or_else(|| crate::config::constants::TOKEN_PROGRAM_ID.to_string());

    info!(
        position_id = %position.id, mint = %mint, amount,
        "Executing sell for exited position"
    );

    // The retry loop confirms the clear by summing THIS position's own sell
    // signatures' token legs against its entry amount (not the shared net balance),
    // so on a confirmed clear it hands back those signatures + their rolled-up legs
    // — the close step reuses them instead of re-querying.
    let (sigs, legs) = match sell_until_balance_cleared(
        trader.clone(),
        mint.clone(),
        amount,
        trade_repo.clone(),
        cache,
        trade_signals,
        base_token_program,
        slippage_bps,
    )
    .await
    {
        SellOutcome::Cleared { sigs, legs } => (sigs, legs),
        SellOutcome::Failed { sigs } => {
            warn!(
                position_id = %position.id, mint = %mint,
                "Sell execution finished without clearing token balance; marking position ExitFailed"
            );
            // (no rent reclaim: balance not confirmed cleared)
            let prev = position.clone();
            position.mark_exit_failed(trigger_price, trigger_time);
            position.exit_reason = Some(exit_reason.clone());
            // Record whatever legs DID sell so the failed row still attributes them.
            position.exit_tx_signatures = sigs;
            if let Err(err) = position_repo.update(&position).await {
                warn!(
                    position_id = %position.id, mint = %mint,
                    "Failed to mark position {} ExitFailed: {err}", position.id
                );
            } else {
                runtime.sync_position(Some(&prev), &position);
            }
            return;
        }
    };

    // Rent reclaim (off the hot path): the balance is confirmed cleared, so close
    // the now-empty token account to recover its ~0.002 SOL rent. Fire-and-forget
    // — a recent-blockhash, preflight-checked close that never blocks the exit and
    // cheaply no-ops if sub-threshold dust kept the account funded.
    {
        let trader = trader.clone();
        let mint = mint.clone();
        tokio::spawn(async move {
            if let Err(err) = trader.close_token_account(&mint, None).await {
                debug!(mint = %mint, "rent-reclaim close skipped: {err}");
            }
        });
    }

    if let Some(legs) = legs {
        position.close(
            legs.price_per_token(),
            sigs,
            legs.token_amount,
            legs.last_block_time,
        );
        position.exit_reason = Some(exit_reason.clone());
        let prev = position.clone();
        if let Err(err) = position_repo.update(&position).await {
            warn!(
                position_id = %position.id, mint = %mint,
                "Failed to close position after confirmed sell: {err}"
            );
        } else {
            runtime.sync_position(Some(&prev), &position);
            let pnl_percent = position.pnl_percentage().unwrap_or(0.0);
            info!(
                position_id = %position.id, mint = %mint, pnl_percent,
                "Position closed after confirmed sell"
            );
        }
        return;
    }

    warn!(
        position_id = %position.id, mint = %mint,
        "Sell completed but no confirmed sell record found"
    );
}

/// Outcome of [`sell_until_balance_cleared`]. Both variants carry the position's
/// OWN accumulated sell signatures (one per landed leg/attempt) so the close /
/// ExitFailed record attributes the exit per-signature. `Cleared` also carries the
/// rolled-up sell legs (`SigLegs`) so the close reuses the exit price/amount/time
/// without a re-query; `None` only on the amount==0 no-op.
enum SellOutcome {
    Cleared { sigs: Vec<String>, legs: Option<SigLegs> },
    Failed { sigs: Vec<String> },
}

/// pump.fun bonding-curve `TooLittleSolReceived` (Anchor error 6003) — the
/// sell-side slippage floor: the price moved past `min_out` between build and
/// land. Retryable (re-quote and resend).
const CURVE_TOO_LITTLE_SOL_RECEIVED: u32 = 6003;
/// PumpSwap AMM `ExceededSlippage` (Anchor error 6004) — the AMM-side slippage
/// floor. Retryable, same as the curve case.
const AMM_EXCEEDED_SLIPPAGE: u32 = 6004;

/// After a feed-confirm sell's poll window elapses without the balance clearing,
/// one `signature_state_detailed` check decides whether re-sending is worth the
/// fee. A landed-and-reverted sell collapses two very different causes into one
/// signal, so we read the on-chain **program error code** to tell them apart:
///
/// - **Slippage-floor rejection** (curve `TooLittleSolReceived` / AMM
///   `ExceededSlippage`) — the price moved past `min_out`. Retryable: the next
///   attempt re-quotes and can clear.
/// - **Structural revert** (already-sold / empty-or-wrong account / migrated) or
///   an **unknown** code — can never clear on the SAME venue and re-pays
///   base+priority fee on every retry, so stop (the feed-path analogue of
///   `sell_token`'s `OnChainRevert` budget guard). Unknown stays conservative
///   (stop) and the caller logs the raw code so unmapped reasons surface.
///
/// The one cross-cutting exception: the token migrated mid-exit, so the next
/// attempt re-routes to the other venue (curve↔AMM) — keep going regardless of
/// the code. A send that didn't land / whose status is unknown costs nothing to
/// re-send, so retry. Pure so the fee-guard decision is unit-tested without a
/// live chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SellRetryDecision {
    /// Re-send the remainder (escalated tip) — the prior send cost nothing, the
    /// route will change on the next attempt, or it reverted on the slippage floor.
    Retry,
    /// Stop: the send landed-and-reverted (structural / unknown) on a venue the
    /// next attempt would reuse, so a blind re-send only re-pays fees. Caller marks
    /// the position ExitFailed.
    StopFeeBurn,
}

/// Map a landed-and-reverted sell to a retry decision using the on-chain error
/// code, not a `min_out=1` assumption. `used_migrated` is the venue the sent tx
/// used (true = AMM, false = curve); `now_migrated` is the venue the next attempt
/// would use, so a mid-exit migration (`used_migrated != now_migrated`) always
/// retries on the re-routed venue.
fn classify_sell_revert<E>(
    state: &Result<SigStatus, E>,
    used_migrated: bool,
    now_migrated: bool,
) -> SellRetryDecision {
    match state {
        // Landed + reverted on a venue the retry would reuse: only the venue's
        // slippage-floor code is retryable; structural/unknown codes stop.
        Ok(SigStatus::Reverted { custom }) if used_migrated == now_migrated => {
            let slippage_code = if used_migrated {
                AMM_EXCEEDED_SLIPPAGE
            } else {
                CURVE_TOO_LITTLE_SOL_RECEIVED
            };
            if *custom == Some(slippage_code) {
                SellRetryDecision::Retry
            } else {
                SellRetryDecision::StopFeeBurn
            }
        }
        // Route changed (migration mid-exit), didn't land / pending, status error,
        // or a landed-success with no balance change → re-send costs nothing or can
        // succeed on the new route.
        _ => SellRetryDecision::Retry,
    }
}

#[allow(clippy::too_many_arguments)]
async fn sell_until_balance_cleared(
    trader: Arc<PumpFunTrader>,
    mint: String,
    mut amount: u64,
    trade_repo: TradeRepo,
    cache: &TokenCache,
    trade_signals: Arc<TradeSignals>,
    base_token_program: String,
    slippage_bps: u64,
) -> SellOutcome {
    let mut attempt = 0usize;
    let max_attempts = super::SELL_MAX_ATTEMPTS;
    let mut backoff_ms = 300u64;
    let wallet = trader.wallet_pubkey();

    if amount == 0 {
        info!(mint = %mint, "sell skipped because amount is zero");
        return SellOutcome::Cleared { sigs: Vec::new(), legs: None };
    }
    // Total tokens this position is selling out. The exit is confirmed by summing
    // OUR own sell signatures' token legs against this target, so concurrent
    // same-token positions never confirm against each other's sells.
    let target_tokens = amount as f64;

    // Resolve the token account once (cache-first; at most one wallet scan) and
    // reuse it across every attempt — it never changes for a given mint. If this
    // is None, sell_token_once/amm_sell still fall back to their own internal
    // lookup, so correctness is preserved while the per-attempt wallet scan is removed.
    let token_account_override = trader
        .resolve_cached_token_account(&mint)
        .await
        .ok()
        .flatten()
        .map(|pk| pk.to_string());

    // Register interest in this (wallet, mint) once for the whole exit (not per
    // attempt) so the feed's trade-sequence is observed continuously: the confirm
    // loop only re-runs the net-balance SQL aggregate when `seq` advanced — i.e.
    // when the feed actually persisted a new trade for us — and skips it on bare
    // fallback ticks where the balance cannot have changed.
    let guard = trade_signals.register(&wallet, &mint);
    let mut last_seq: Option<u64> = None;
    // Every sell signature THIS exit submitted (one per landed attempt/leg) — the
    // per-signature confirm set, also returned for per-signature exit attribution.
    let mut sell_sigs: Vec<String> = Vec::new();

    while attempt < max_attempts && amount > 0 {
        attempt += 1;
        // Re-read routing from the WS-fed cache each attempt: `is_cashback` gates
        // the bonding-curve cashback account; `is_migrated` selects the PumpSwap
        // AMM path. A held token can migrate mid-exit (is_migrated flips within
        // ~a slot) — re-reading re-routes the next attempt to the AMM instead of
        // pinning every retry to the stale curve route. The Ref is dropped (bools
        // copied out) before any await.
        let (is_cashback, is_migrated) = match cache.get(&mint) {
            Some(e) => (e.token.is_cashback_enabled, e.is_migrated),
            None => (false, false),
        };
        // Escalate the Jito tip each retry (attempt is 1-based → level 0 on the
        // first try): a sell that lost the auction just didn't land, so bid up to
        // win the next block rather than re-send the same losing tip.
        let tip_level = (attempt - 1) as u8;
        // confirm = false: this loop already confirms by polling the
        // LaserStream-fed `trades` balance below, so the trader skips its
        // redundant inner 1s RPC poll and returns as soon as the tx is accepted.
        // Bound the send: a wedged RPC inside the trader (nonce fetch, reserve
        // read, sender POST) must not hang the position until the 5-min stuck-
        // position cleanup. On timeout we treat the attempt as a failed send and
        // let the retry loop re-send with an escalated tip.
        const SELL_SEND_TIMEOUT: Duration = Duration::from_secs(15);
        let send = async {
            if is_migrated {
                trader
                    .amm_sell(
                        &mint,
                        amount,
                        &base_token_program,
                        None,
                        token_account_override.as_deref(),
                        Some(slippage_bps),
                        tip_level,
                        false,
                    )
                    .await
            } else {
                trader
                    .sell_token_once(&mint, amount, None, is_cashback, token_account_override.as_deref(), Some(slippage_bps), tip_level, false)
                    .await
            }
        };
        let sell_result = match tokio::time::timeout(SELL_SEND_TIMEOUT, send).await {
            Ok(res) => res,
            Err(_) => Err(anyhow::anyhow!(
                "sell send timed out after {SELL_SEND_TIMEOUT:?}"
            )),
        };
        match sell_result {
            // `Some(sig)` carries the submitted tx so a non-clearing window can be
            // classified for a landed-revert before re-paying fees (see below).
            Ok(Some(sig)) => {
                info!(mint = %mint, attempt, amount, "sell submitted (feed-confirm)");
                // Attribute this leg to the position: confirm by summing OUR sell
                // signatures, never the shared net balance.
                sell_sigs.push(sig.clone());
                // Confirm via the LaserStream-fed `trades` balance, waking on the
                // DbWriter's persist signal for this (wallet, mint) instead of
                // blindly sleeping each tick. A feed-confirmed send (confirm=false)
                // returns before its tx is indexed, so reading the balance once
                // would show the pre-sell amount and fire a needless duplicate —
                // the window gives the feed time to catch up. The same overall
                // window (SELL_POLL_MAX_ATTEMPTS × SELL_POLL_INTERVAL_MS) and a
                // fallback tick are kept, so a missed signal is never worse than
                // the old polling.
                let mut remaining_amount = amount;
                let mut cleared = false;
                // The rolled-up sell legs that confirmed the clear, captured so the
                // caller closes the position off them (exit price/amount/time).
                let mut confirmed_legs: Option<SigLegs> = None;
                let interval = Duration::from_millis(super::SELL_POLL_INTERVAL_MS);
                let min_query_gap =
                    Duration::from_millis(super::SELL_BALANCE_QUERY_MIN_INTERVAL_MS);
                let deadline = Instant::now() + interval * super::SELL_POLL_MAX_ATTEMPTS as u32;
                // Floor on aggregate frequency: set after each query so a burst of
                // notify-driven wakeups within `min_query_gap` coalesces into one
                // SUM. Never blocks the periodic fallback poll (gap < interval) and
                // is overridden at the deadline so the final confirm always runs.
                let mut last_query_at: Option<Instant> = None;
                loop {
                    // Arm the wakeup before the balance read (see the buy path:
                    // `notify_waiters` stores no permit, `enable()` registers up front).
                    let notified = guard.notified();
                    tokio::pin!(notified);
                    notified.as_mut().enable();

                    let now = Instant::now();
                    let at_deadline = now >= deadline;

                    // Only run the per-signature SUM when the feed has persisted a
                    // new trade for this (wallet, mint) since our last read — `seq`
                    // is bumped once per such trade. A bare fallback tick (no new
                    // trade) can't have changed how much we've sold, so we skip the
                    // per-tick scan over the large partitioned `trades` table
                    // entirely. On top of that, rate-limit the aggregate to at most
                    // once per `min_query_gap`: during a dump `seq` advances (and
                    // wakes us) once per landed leg, which would otherwise fire the
                    // SUM many times per poll interval. Coalescing is safe — SQL stays
                    // the authoritative, PK-deduped confirm, so a slightly later read
                    // only ever sees *more* sold, never less, and can't over-sell. The
                    // rate-limit is bypassed at the deadline so a clear that landed
                    // during a coalesced burst is always confirmed before we fall
                    // through to a (wasteful, double-sell-risking) retry. `seq` is
                    // sampled *before* the query so a trade landing during it is
                    // re-checked next tick.
                    let seq = guard.seq();
                    let seq_advanced = last_seq != Some(seq);
                    let rate_ok = at_deadline
                        || last_query_at.map_or(true, |t| now.duration_since(t) >= min_query_gap);
                    if seq_advanced && rate_ok {
                        last_query_at = Some(now);
                        // Sum only THIS position's own sell signatures' token legs and
                        // measure the remainder against its target — so a concurrent
                        // same-token position's sells never count toward (or against)
                        // this clear.
                        match trade_repo
                            .sum_legs_by_signatures(
                                &wallet,
                                &mint,
                                &sell_sigs,
                                crate::models::trade::TradeType::Sell,
                            )
                            .await
                        {
                            Ok(legs) => {
                                let sold = legs.as_ref().map(|l| l.token_amount).unwrap_or(0.0);
                                let remaining = (target_tokens - sold).max(0.0);
                                if remaining <= super::PARTIAL_FILL_THRESHOLD {
                                    info!(mint = %mint, attempt, "sell cleared the balance");
                                    cleared = true;
                                    confirmed_legs = legs;
                                    break;
                                }
                                remaining_amount = remaining as u64;
                                last_seq = Some(seq);
                            }
                            Err(err) => warn!("Failed to sum sell signature legs: {err}"),
                        }
                    }
                    if at_deadline {
                        break;
                    }
                    let now = Instant::now();
                    let fallback = interval.min(deadline - now);
                    tokio::select! {
                        _ = &mut notified => {}
                        _ = sleep(fallback) => {}
                    }
                }
                if cleared {
                    // The confirming sum is already in hand (`confirmed_legs`) — the
                    // caller closes the position off its rolled-up price/amount/time
                    // and OUR sell signatures, without any re-read.
                    return SellOutcome::Cleared { sigs: sell_sigs, legs: confirmed_legs };
                }
                // Not cleared within the window. A partial fill (balance dropped)
                // just leaves a smaller remainder to chase. But an *unchanged*
                // balance means this send cleared nothing, and a blind retry
                // re-pays base+priority fee. Before retrying, classify the sent tx
                // by its on-chain error code (`classify_sell_revert`): a slippage-
                // floor revert re-quotes and retries, while a structural revert
                // (already-sold / empty account / migrated) can never clear on the
                // same venue, so stop. This is the feed-path analogue
                // of `sell_token`'s `OnChainRevert` budget guard — ONE off-path
                // signature-state RPC, fired only after a failed poll window (never
                // a hot-path poll) and only when no progress was made.
                if remaining_amount < amount {
                    warn!(mint = %mint, attempt, remaining = remaining_amount,
                        "sell partially filled; retrying the remainder with a higher tip");
                    amount = remaining_amount;
                } else {
                    // Re-read the venue: a mid-exit migration flips `is_migrated`,
                    // re-routing the next attempt (curve↔AMM) to a venue that can
                    // still succeed — so only stop when the route wouldn't change.
                    let now_migrated =
                        cache.get(&mint).map(|e| e.is_migrated).unwrap_or(is_migrated);
                    let state = trader.signature_state_detailed(&sig).await;
                    if classify_sell_revert(&state, is_migrated, now_migrated)
                        == SellRetryDecision::StopFeeBurn
                    {
                        // Surface the raw program error so an unmapped (non-slippage)
                        // revert reason is visible, not silently swallowed.
                        let raw_code = match &state {
                            Ok(SigStatus::Reverted { custom }) => *custom,
                            _ => None,
                        };
                        warn!(mint = %mint, attempt, sig = %sig, raw_error_code = ?raw_code,
                            "sell reverted on-chain (structural/unknown) on a route the retry \
                             would reuse; stopping (a blind re-send would only re-pay fees)");
                        return SellOutcome::Failed { sigs: sell_sigs };
                    }
                    warn!(mint = %mint, attempt, remaining = remaining_amount,
                        "sell not cleared within poll window; retrying with a higher tip");
                }
            }
            Ok(None) => warn!(mint = %mint, attempt, amount, "sell returned no signature (no-op)"),
            Err(err) => warn!(mint = %mint, attempt, amount, "sell error: {err}"),
        }

        if attempt < max_attempts {
            sleep(Duration::from_millis(backoff_ms)).await;
            backoff_ms = (backoff_ms * 2).saturating_add(100);
        } else {
            warn!(mint = %mint, amount, "sell failed after {max_attempts} attempts");
            return SellOutcome::Failed { sigs: sell_sigs };
        }
    }

    // Loop exited with `amount == 0` (fully chased): confirm the clear from OUR
    // sell signatures' summed legs. Anything else is a failure to clear.
    if amount == 0 {
        let legs = trade_repo
            .sum_legs_by_signatures(&wallet, &mint, &sell_sigs, crate::models::trade::TradeType::Sell)
            .await
            .ok()
            .flatten();
        SellOutcome::Cleared { sigs: sell_sigs, legs }
    } else {
        SellOutcome::Failed { sigs: sell_sigs }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(status: Result<Option<bool>, &'static str>) -> SilentSendOutcome {
        classify_silent_send(&status)
    }

    #[test]
    fn confirmed_revert_is_the_only_resend() {
        // `Ok(Some(false))` == landed-but-failed on-chain → no fill → safe to retry.
        assert_eq!(outcome(Ok(Some(false))), SilentSendOutcome::Resend);
    }

    #[test]
    fn landed_but_unindexed_waits_never_resends() {
        assert_eq!(outcome(Ok(Some(true))), SilentSendOutcome::WaitThenSettle);
    }

    #[test]
    fn pending_gives_up_to_avoid_double_buy() {
        assert_eq!(outcome(Ok(None)), SilentSendOutcome::GiveUp);
    }

    #[test]
    fn status_error_gives_up_to_avoid_double_buy() {
        assert_eq!(outcome(Err("rpc down")), SilentSendOutcome::GiveUp);
    }

    #[test]
    fn nothing_but_a_confirmed_revert_ever_resends() {
        // The double-buy invariant: only a proven on-chain revert may re-send.
        for status in [Ok(Some(true)), Ok(None), Err("x")] {
            assert_ne!(outcome(status), SilentSendOutcome::Resend);
        }
    }

    // --- Sell-side fee guard (classify_sell_revert) ---------------------------

    /// A landed-and-reverted sell carrying program error `custom`.
    fn reverted(custom: Option<u32>) -> Result<SigStatus, &'static str> {
        Ok(SigStatus::Reverted { custom })
    }

    #[test]
    fn structural_revert_same_route_stops_fee_burn() {
        // A non-slippage (structural / unknown) revert on the venue the retry would
        // reuse → stop re-paying fees.
        let structural = reverted(Some(6022)); // e.g. curve SellZeroAmount
        assert_eq!(
            classify_sell_revert(&structural, false, false),
            SellRetryDecision::StopFeeBurn
        );
        assert_eq!(
            classify_sell_revert(&structural, true, true),
            SellRetryDecision::StopFeeBurn
        );
    }

    #[test]
    fn unknown_code_revert_stops_conservatively() {
        // No custom code (non-Anchor failure) or an unmapped code → stay
        // conservative and stop on a stable route.
        assert_eq!(
            classify_sell_revert(&reverted(None), false, false),
            SellRetryDecision::StopFeeBurn
        );
    }

    #[test]
    fn slippage_revert_retries_to_requote() {
        // Curve slippage floor (6003) on the curve route, AMM slippage (6004) on the
        // AMM route → price moved, re-quote and resend.
        assert_eq!(
            classify_sell_revert(&reverted(Some(CURVE_TOO_LITTLE_SOL_RECEIVED)), false, false),
            SellRetryDecision::Retry
        );
        assert_eq!(
            classify_sell_revert(&reverted(Some(AMM_EXCEEDED_SLIPPAGE)), true, true),
            SellRetryDecision::Retry
        );
    }

    #[test]
    fn slippage_code_for_wrong_venue_is_not_treated_as_slippage() {
        // The AMM slippage code on the curve route (or vice versa) is not the
        // venue's slippage floor → treated as structural, stop.
        assert_eq!(
            classify_sell_revert(&reverted(Some(AMM_EXCEEDED_SLIPPAGE)), false, false),
            SellRetryDecision::StopFeeBurn
        );
        assert_eq!(
            classify_sell_revert(&reverted(Some(CURVE_TOO_LITTLE_SOL_RECEIVED)), true, true),
            SellRetryDecision::StopFeeBurn
        );
    }

    #[test]
    fn landed_revert_after_migration_retries_new_route() {
        // Sold on the curve but the token migrated mid-exit (now AMM), or vice
        // versa → the next attempt re-routes, so don't stop (even structural).
        assert_eq!(
            classify_sell_revert(&reverted(Some(6022)), false, true),
            SellRetryDecision::Retry
        );
        assert_eq!(
            classify_sell_revert(&reverted(Some(6022)), true, false),
            SellRetryDecision::Retry
        );
    }

    #[test]
    fn unlanded_or_unknown_sell_retries() {
        // A sell that didn't land (or whose status we can't read) cost nothing —
        // re-send with an escalated tip.
        let states: [Result<SigStatus, &'static str>; 2] = [Ok(SigStatus::Pending), Err("rpc down")];
        for state in states {
            assert_eq!(classify_sell_revert(&state, false, false), SellRetryDecision::Retry);
        }
    }

    #[test]
    fn landed_successful_sell_retries_to_chase_remainder() {
        let state: Result<SigStatus, &'static str> = Ok(SigStatus::Succeeded);
        assert_eq!(classify_sell_revert(&state, false, false), SellRetryDecision::Retry);
    }

    // --- Snipe slippage reserve source (snipe_reserves_from_cache) -----------

    #[test]
    fn snipe_reserves_convert_sol_to_lamports() {
        // Token side passes through raw; SOL side is scaled to lamports (× 1e9),
        // matching the (vt raw, vq lamports) units `buy_token_inner` expects.
        assert_eq!(
            snipe_reserves_from_cache(Some(1_000.0), Some(2.0)),
            Some((1_000u128, 2_000_000_000u128))
        );
    }

    #[test]
    fn snipe_reserves_none_when_either_snapshot_missing_or_nonpositive() {
        // A missing or non-positive snapshot ⇒ None ⇒ the buy proceeds with
        // min_out=1 (no inline RPC), never blocking the snipe.
        assert_eq!(snipe_reserves_from_cache(None, Some(2.0)), None);
        assert_eq!(snipe_reserves_from_cache(Some(1_000.0), None), None);
        assert_eq!(snipe_reserves_from_cache(Some(0.0), Some(2.0)), None);
        assert_eq!(snipe_reserves_from_cache(Some(1_000.0), Some(0.0)), None);
    }

    // -------------------------------------------------------------------------
    // Full-flow integration tests for `buy_until_filled_or_give_up`, driven by a
    // scripted fake executor against a real local Postgres. `#[ignore]` like the
    // other DB/network tests — run with a local DB:
    //   $env:DATABASE_URL = "postgres://..."; cargo test -p backend -- --ignored
    // Unique mint/wallet ids keep them from colliding with real data, and each
    // test cleans up the rows it created.
    // -------------------------------------------------------------------------

    use crate::config::constants::TOKEN_PROGRAM_ID;
    use crate::models::trade::{Trade, TradeType};
    use sqlx::postgres::PgPoolOptions;
    use sqlx::PgPool;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// What the chain "reports" for a sent-but-unindexed signature.
    #[derive(Clone, Copy)]
    enum FakeStatus {
        Reverted,
        Landed,
        Pending,
        Error,
    }

    /// Scripted `SnipeExecutor` — never touches the chain. Optionally writes a Buy
    /// trade row (as the WS indexer would) on the Nth send or on a status check, so
    /// the tests can drive every branch of `buy_until_filled_or_give_up` deterministically.
    struct FakeExecutor {
        wallet: String,
        pool: PgPool,
        mint: String,
        status: FakeStatus,
        fill_on_send: Option<usize>,
        fill_on_status_check: bool,
        sends: AtomicUsize,
    }

    impl FakeExecutor {
        fn new(wallet: String, pool: PgPool, mint: String) -> Self {
            Self {
                wallet,
                pool,
                mint,
                status: FakeStatus::Pending,
                fill_on_send: None,
                fill_on_status_check: false,
                sends: AtomicUsize::new(0),
            }
        }
        fn status(mut self, status: FakeStatus) -> Self {
            self.status = status;
            self
        }
        fn fill_on_send(mut self, n: usize) -> Self {
            self.fill_on_send = Some(n);
            self
        }
        fn fill_on_status_check(mut self) -> Self {
            self.fill_on_status_check = true;
            self
        }
        fn send_count(&self) -> usize {
            self.sends.load(Ordering::SeqCst)
        }
        /// Write a Buy trade row for this wallet+mint under transaction signature
        /// `sig`, simulating the WS feed. Per-signature attribution means the fill
        /// must carry the SAME signature the bot submitted, or it won't be adopted.
        async fn insert_fill(&self, sig: &str) {
            let trade = Trade::new(
                self.mint.clone(),
                self.wallet.clone(),
                TradeType::Buy,
                0.05,
                1000.0,
                sig.to_string(),
                100,
                Utc::now(),
            );
            TradeRepo::new(self.pool.clone())
                .insert(&trade)
                .await
                .expect("insert fill trade");
        }
    }

    #[async_trait::async_trait]
    impl SnipeExecutor for FakeExecutor {
        fn wallet(&self) -> String {
            self.wallet.clone()
        }
        async fn send_snipe_buy(
            &self,
            _mint: &str,
            _creator: &str,
            _token_program_id: &str,
            _amount: f64,
            _slippage_bps: Option<u64>,
            _reserves: Option<(u128, u128)>,
        ) -> anyhow::Result<String> {
            let n = self.sends.fetch_add(1, Ordering::SeqCst) + 1;
            let sig = format!("fakesig-{n}");
            if self.fill_on_send == Some(n) {
                self.insert_fill(&sig).await;
            }
            Ok(sig)
        }
        async fn check_signature(&self, signature: &str) -> anyhow::Result<Option<bool>> {
            if self.fill_on_status_check {
                // Fill under the SAME signature the bot is checking, so the extended
                // poll adopts it (per-signature attribution).
                self.insert_fill(signature).await;
            }
            match self.status {
                FakeStatus::Reverted => Ok(Some(false)),
                FakeStatus::Landed => Ok(Some(true)),
                FakeStatus::Pending => Ok(None),
                FakeStatus::Error => Err(anyhow::anyhow!("fake rpc error")),
            }
        }
    }

    /// Tiny poll/retry timing so give-up and re-send paths don't wait real windows.
    fn test_cfg() -> BuyRetryCfg {
        BuyRetryCfg {
            max_attempts: 3,
            backoff_ms: 1,
            poll_attempts: 3,
            poll_interval: Duration::from_millis(5),
        }
    }

    /// Connect to the local test DB, or `None` to skip when `DATABASE_URL` is unset.
    async fn test_pool() -> Option<PgPool> {
        let url = std::env::var("DATABASE_URL").ok()?;
        PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .ok()
    }

    fn unique(prefix: &str) -> String {
        format!("{prefix}{}", Uuid::new_v4().simple())
    }

    /// Insert a fresh Holding position (entry_price 0) and return it + repos + a
    /// runtime cache seeded with it.
    async fn setup(
        pool: &PgPool,
        mint: &str,
        wallet: &str,
    ) -> (Position, Tpsl1PositionRepo, TradeRepo, Arc<Tpsl1RuntimeCache>) {
        let position_repo = Tpsl1PositionRepo::new(pool.clone());
        let trade_repo = TradeRepo::new(pool.clone());
        let runtime = Arc::new(Tpsl1RuntimeCache::new(tokio::sync::broadcast::channel(8).0));
        let mut position = Position::new(
            mint.to_string(),
            wallet.to_string(),
            "TPSL1".to_string(),
            Uuid::new_v4(),
        );
        // entry_tx_signatures starts empty ([]) — the partial unique backstop only
        // covers non-empty arrays, so an unentered Holding row needs no placeholder.
        position.entry_token_amount = Some(0.001);
        position_repo.insert(&position).await.expect("insert position");
        runtime.sync_position(None, &position);
        (position, position_repo, trade_repo, runtime)
    }

    async fn cleanup(pool: &PgPool, mint: &str, position_id: Uuid) {
        let _ = sqlx::query("DELETE FROM trades WHERE mint_address = $1")
            .bind(mint)
            .execute(pool)
            .await;
        let _ = sqlx::query("DELETE FROM tpsl1_real_positions WHERE id = $1")
            .bind(position_id)
            .execute(pool)
            .await;
    }

    async fn run_buy(
        fake: Arc<FakeExecutor>,
        mint: &str,
        position: &Position,
        position_repo: &Tpsl1PositionRepo,
        trade_repo: &TradeRepo,
        runtime: &Arc<Tpsl1RuntimeCache>,
    ) {
        buy_until_filled_or_give_up(
            fake,
            mint.to_string(),
            "creator".to_string(),
            TOKEN_PROGRAM_ID.to_string(),
            0.001,
            position.id,
            position_repo.clone(),
            trade_repo.clone(),
            runtime.clone(),
            Arc::new(TradeSignals::new()),
            test_cfg(),
            None,
            None,
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "requires a local Postgres (DATABASE_URL); run with --ignored"]
    async fn db_happy_path_records_entry_from_onchain_fill() {
        let Some(pool) = test_pool().await else { return };
        let (mint, wallet) = (unique("MINT"), unique("WALLET"));
        let (position, position_repo, trade_repo, runtime) = setup(&pool, &mint, &wallet).await;

        // First send produces the on-chain fill row; the poll picks it up.
        let fake = Arc::new(FakeExecutor::new(wallet, pool.clone(), mint.clone()).fill_on_send(1));
        run_buy(fake.clone(), &mint, &position, &position_repo, &trade_repo, &runtime).await;

        let updated = position_repo.find_by_id(position.id).await.unwrap().unwrap();
        assert!(updated.entry_price.unwrap_or(0.0) > 0.0, "entry recorded from the on-chain fill");
        assert_eq!(fake.send_count(), 1, "exactly one buy sent");
        cleanup(&pool, &mint, position.id).await;
    }

    #[tokio::test]
    #[ignore = "requires a local Postgres (DATABASE_URL); run with --ignored"]
    async fn db_foreign_fill_is_not_adopted() {
        let Some(pool) = test_pool().await else { return };
        let (mint, wallet) = (unique("MINT"), unique("WALLET"));
        let (position, position_repo, trade_repo, runtime) = setup(&pool, &mint, &wallet).await;

        // A buy on this (wallet, mint) under a signature the bot NEVER submitted —
        // e.g. a concurrent same-token position's fill (decision #2). Per-signature
        // attribution must NOT adopt it: the bot sends its own buy instead of
        // mistaking the foreign fill for its entry. Status stays Pending → give up.
        let fake = Arc::new(FakeExecutor::new(wallet, pool.clone(), mint.clone()));
        fake.insert_fill(&unique("foreign-")).await;
        run_buy(fake.clone(), &mint, &position, &position_repo, &trade_repo, &runtime).await;

        let updated = position_repo.find_by_id(position.id).await.unwrap().unwrap();
        assert_eq!(
            updated.entry_price.unwrap_or(0.0),
            0.0,
            "foreign fill must NOT be adopted as this position's entry"
        );
        assert!(
            updated.entry_tx_signatures.is_empty(),
            "no signature attributed from a fill the bot didn't send"
        );
        assert_eq!(fake.send_count(), 1, "bot sent its own buy rather than adopting the foreign fill");
        cleanup(&pool, &mint, position.id).await;
    }

    #[tokio::test]
    #[ignore = "requires a local Postgres (DATABASE_URL); run with --ignored"]
    async fn db_revert_resends_then_records() {
        let Some(pool) = test_pool().await else { return };
        let (mint, wallet) = (unique("MINT"), unique("WALLET"));
        let (position, position_repo, trade_repo, runtime) = setup(&pool, &mint, &wallet).await;

        // 1st send doesn't fill; chain reports a revert → re-send; 2nd send fills.
        let fake = Arc::new(
            FakeExecutor::new(wallet, pool.clone(), mint.clone())
                .status(FakeStatus::Reverted)
                .fill_on_send(2),
        );
        run_buy(fake.clone(), &mint, &position, &position_repo, &trade_repo, &runtime).await;

        let updated = position_repo.find_by_id(position.id).await.unwrap().unwrap();
        assert!(updated.entry_price.unwrap_or(0.0) > 0.0, "entry recorded after the re-send filled");
        assert_eq!(fake.send_count(), 2, "reverted buy was re-sent exactly once");
        cleanup(&pool, &mint, position.id).await;
    }

    #[tokio::test]
    #[ignore = "requires a local Postgres (DATABASE_URL); run with --ignored"]
    async fn db_pending_gives_up_without_double_buy() {
        let Some(pool) = test_pool().await else { return };
        let (mint, wallet) = (unique("MINT"), unique("WALLET"));
        let (position, position_repo, trade_repo, runtime) = setup(&pool, &mint, &wallet).await;

        // No fill ever; status stays Pending → give up, never re-send.
        let fake =
            Arc::new(FakeExecutor::new(wallet, pool.clone(), mint.clone()).status(FakeStatus::Pending));
        run_buy(fake.clone(), &mint, &position, &position_repo, &trade_repo, &runtime).await;

        let updated = position_repo.find_by_id(position.id).await.unwrap().unwrap();
        assert_eq!(updated.entry_price.unwrap_or(0.0), 0.0, "no entry recorded");
        assert_eq!(fake.send_count(), 1, "pending tx must NOT be re-sent (double-buy guard)");
        cleanup(&pool, &mint, position.id).await;
    }

    #[tokio::test]
    #[ignore = "requires a local Postgres (DATABASE_URL); run with --ignored"]
    async fn db_status_error_gives_up_without_double_buy() {
        let Some(pool) = test_pool().await else { return };
        let (mint, wallet) = (unique("MINT"), unique("WALLET"));
        let (position, position_repo, trade_repo, runtime) = setup(&pool, &mint, &wallet).await;

        // The status check itself errors → ambiguous → give up, never re-send.
        let fake =
            Arc::new(FakeExecutor::new(wallet, pool.clone(), mint.clone()).status(FakeStatus::Error));
        run_buy(fake.clone(), &mint, &position, &position_repo, &trade_repo, &runtime).await;

        let updated = position_repo.find_by_id(position.id).await.unwrap().unwrap();
        assert_eq!(updated.entry_price.unwrap_or(0.0), 0.0, "no entry recorded");
        assert_eq!(fake.send_count(), 1, "status-check error must NOT trigger a re-send");
        cleanup(&pool, &mint, position.id).await;
    }

    #[tokio::test]
    #[ignore = "requires a local Postgres (DATABASE_URL); run with --ignored"]
    async fn db_landed_but_lagging_records_without_resend() {
        let Some(pool) = test_pool().await else { return };
        let (mint, wallet) = (unique("MINT"), unique("WALLET"));
        let (position, position_repo, trade_repo, runtime) = setup(&pool, &mint, &wallet).await;

        // Poll window elapses with no row; status says it landed and the indexer
        // catches up (fill written on the status check) → extended poll records it.
        let fake = Arc::new(
            FakeExecutor::new(wallet, pool.clone(), mint.clone())
                .status(FakeStatus::Landed)
                .fill_on_status_check(),
        );
        run_buy(fake.clone(), &mint, &position, &position_repo, &trade_repo, &runtime).await;

        let updated = position_repo.find_by_id(position.id).await.unwrap().unwrap();
        assert!(updated.entry_price.unwrap_or(0.0) > 0.0, "entry recorded after the indexer caught up");
        assert_eq!(fake.send_count(), 1, "landed tx must NOT be re-sent (double-buy guard)");
        cleanup(&pool, &mint, position.id).await;
    }
}
