//! Real (on-chain) execution: snipe-buy a matched token and resolve its entry
//! fill from the WS/DB feed; sell a position out and close it. The double-buy
//! safety rule — *re-send only on a confirmed on-chain revert* — is centralized
//! in [`classify_silent_send`] and unit-tested without a live chain.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio::time::{sleep, Instant};
use tracing::{info, warn};
use uuid::Uuid;

use super::super::Tpsl2RuntimeCache;
use crate::models::{Position, Tpsl2StrategyRule};
use crate::state::token_cache::TokenCache;
use crate::state::trade_signals::TradeSignals;
use crate::storage::repositories::{tpsl2_position_repo::Tpsl2PositionRepo, trade_repo::TradeRepo};
use crate::trader::PumpFunTrader;

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
    ) -> anyhow::Result<String> {
        // Snipe slippage is `None` (min_out=1) — see `buy_token_snipe`.
        self.buy_token_snipe(mint, creator, token_program_id, amount, None)
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

/// Poll/timeout timing for the scalp-entry arming wait ([`await_scalp_entry_signal`]).
/// Separate from [`BuyRetryCfg`] because the entry signal can take far longer to
/// form than a buy fill takes to index. `for_rule` sizes the window to the rule's
/// own time gates (see `scalp_arming_attempts`); tests construct it directly.
#[derive(Clone, Copy)]
pub(crate) struct ScalpWaitCfg {
    attempts: usize,
    interval: Duration,
}

impl ScalpWaitCfg {
    /// Window sized to the rule's gates, so a larger min-age / higher-low widens it
    /// automatically — no fixed timeout to keep in sync with the params by hand.
    pub(crate) fn for_rule(rule: &Tpsl2StrategyRule) -> Self {
        Self {
            attempts: super::scalp_arming_attempts(rule),
            interval: Duration::from_millis(super::SCALP_ENTRY_WAIT_INTERVAL_MS),
        }
    }
}

/// Wait for the rule's scalp entry signal before any buy is sent: poll the WS-fed
/// trade feed until [`find_scalp_entry`](super::super::entry::find_scalp_entry)
/// holds on some trade, arming the snipe buy. Returns `true` once armed, `false`
/// if the window elapses without a signal (the caller then drops the unentered
/// position, exactly as a missed buy does).
///
/// In real mode the qualifying trade is only the **timing** signal — the actual
/// entry price comes from the wallet's own on-chain fill, recorded later by
/// [`adopt_existing_fill_if_present`] — so the returned fill is discarded and only
/// its presence matters. This shares `find_scalp_entry` with the paper poll and the
/// backtest, so all three resolve the same entry moment and live honors `p_entry_*`.
pub(crate) async fn await_scalp_entry_signal(
    mint: &str,
    rule: &Tpsl2StrategyRule,
    trade_repo: &TradeRepo,
    cfg: ScalpWaitCfg,
) -> bool {
    for _ in 0..cfg.attempts {
        match trade_repo.find_by_mint_all(mint).await {
            Ok(trades) => {
                if super::super::entry::find_scalp_entry(&trades, rule).is_some() {
                    return true;
                }
            }
            Err(err) => warn!(mint = %mint, "scalp-wait trade fetch failed: {err}"),
        }
        sleep(cfg.interval).await;
    }
    false
}

/// Snipe-buy `mint` and record the entry fill on first sight in the WS feed,
/// retrying per `cfg`. Honors the double-buy invariant: only a confirmed
/// on-chain revert re-sends.
pub(crate) async fn buy_until_filled_or_give_up<E: SnipeExecutor + 'static>(
    trader: Arc<E>,
    mint: String,
    creator: String,
    token_program_id: String,
    buy_amount: f64,
    position_id: Uuid,
    position_repo: Tpsl2PositionRepo,
    trade_repo: TradeRepo,
    runtime: Arc<Tpsl2RuntimeCache>,
    trade_signals: Arc<TradeSignals>,
    cfg: BuyRetryCfg,
) {
    let wallet = trader.wallet();
    let max_attempts = cfg.max_attempts;
    let mut backoff_ms = cfg.backoff_ms;

    for attempt in 1..=max_attempts {
        // Never double-send: if a previous attempt's buy has since landed and been
        // indexed, adopt that fill instead of firing another buy.
        if adopt_existing_fill_if_present(&mint, &wallet, position_id, &position_repo, &trade_repo, &runtime)
            .await
        {
            return;
        }

        // Snipe send WITHOUT the blocking RPC confirm — the WS/DB trade feed below
        // is the sole confirmation and the entry-price source. `buy_token_snipe`
        // returns the submitted signature so a silent feed can be classified.
        let signature = match trader
            .send_snipe_buy(&mint, &creator, &token_program_id, buy_amount)
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
        info!(mint = %mint, attempt, sig = %signature,
            "buy submitted (no RPC confirm); polling WS feed for fill");

        // Poll the WS-fed trade feed for this wallet's buy row.
        if poll_feed_until_entry_fill(&mint, &wallet, position_id, &position_repo, &trade_repo, &runtime, &trade_signals, &cfg)
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
                if poll_feed_until_entry_fill(&mint, &wallet, position_id, &position_repo, &trade_repo, &runtime, &trade_signals, &cfg)
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

/// Poll the WS-fed trade feed for this wallet's buy on `mint`; on first sight,
/// record the entry and sync the runtime cache. Returns true once an entry has
/// been recorded, false if the poll window elapses without a fill.
/// Event-driven: the DbWriter [`notify`](TradeSignals::notify)s the moment the row
/// is persisted, so the common case adopts the fill immediately instead of waiting
/// out a poll tick. A fallback tick (the old `poll_interval`) and a hard deadline
/// equal to the old `poll_attempts × poll_interval` window are kept, so a missed
/// signal degrades to exactly the previous polling behaviour — never worse.
async fn poll_feed_until_entry_fill(
    mint: &str,
    wallet: &str,
    position_id: Uuid,
    position_repo: &Tpsl2PositionRepo,
    trade_repo: &TradeRepo,
    runtime: &Arc<Tpsl2RuntimeCache>,
    trade_signals: &Arc<TradeSignals>,
    cfg: &BuyRetryCfg,
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

        if adopt_existing_fill_if_present(mint, wallet, position_id, position_repo, trade_repo, runtime).await
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

/// If a buy by `wallet` on `mint` is already present in the trade feed, record it
/// as the position entry (price/amount/tx/time come from the on-chain fill) and
/// return true; otherwise return false without sleeping.
async fn adopt_existing_fill_if_present(
    mint: &str,
    wallet: &str,
    position_id: Uuid,
    position_repo: &Tpsl2PositionRepo,
    trade_repo: &TradeRepo,
    runtime: &Arc<Tpsl2RuntimeCache>,
) -> bool {
    let trades = match trade_repo.find_by_mint(mint, 20).await {
        Ok(trades) => trades,
        Err(err) => {
            warn!(mint = %mint, "failed to query trades for buy confirmation: {err}");
            return false;
        }
    };
    let Some(fill) = trades.into_iter().find(|t| {
        t.wallet_address == wallet && t.trade_type == crate::models::trade::TradeType::Buy
    }) else {
        return false;
    };
    if let Ok(Some(prev)) = position_repo.find_by_id(position_id).await {
        if let Err(err) = position_repo
            .update_entry(
                position_id,
                &fill.tx_signature,
                fill.token_amount,
                fill.price_per_token,
                fill.block_time,
            )
            .await
        {
            warn!(mint = %mint, "failed to update position entry after buy: {err}");
        } else if let Ok(Some(current)) = position_repo.find_by_id(position_id).await {
            runtime.sync_position(Some(&prev), &current);
            info!(mint = %mint, tx = %fill.tx_signature, "position entry recorded from buy fill");
        }
    }
    true
}

/// Sell a position's full balance out (retrying / re-routing across migration)
/// and, on a confirmed clear, close it; otherwise mark it ExitFailed at the
/// trigger price. `trigger_price`/`trigger_time` are the hypothetical exit
/// recorded if the sell never confirms; `exit_reason` is persisted either way.
pub(crate) async fn sell_and_close_position(
    trader: Arc<PumpFunTrader>,
    mut position: Position,
    position_repo: Tpsl2PositionRepo,
    trade_repo: TradeRepo,
    runtime: Arc<Tpsl2RuntimeCache>,
    cache: &TokenCache,
    trade_signals: Arc<TradeSignals>,
    trigger_price: f64,
    trigger_time: DateTime<Utc>,
    exit_reason: String,
) {
    let mint = position.mint.clone();
    let wallet = trader.wallet_pubkey();
    let amount = position.entry_amount as u64;
    let base_token_program = position
        .token_program_id
        .clone()
        .unwrap_or_else(|| crate::config::constants::TOKEN_PROGRAM_ID.to_string());

    info!(
        position_id = %position.id, mint = %mint, amount,
        "Executing sell for exited position"
    );

    let completed = sell_until_balance_cleared(
        trader.clone(),
        mint.clone(),
        amount,
        trade_repo.clone(),
        cache,
        trade_signals,
        base_token_program,
    )
    .await;
    if !completed {
        warn!(
            position_id = %position.id, mint = %mint,
            "Sell execution finished without clearing token balance; marking position ExitFailed"
        );
        let prev = position.clone();
        position.mark_exit_failed(trigger_price, trigger_time);
        position.exit_reason = Some(exit_reason.clone());
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

    if let Ok(trades) = trade_repo.find_by_mint(&mint, 20).await {
        if let Some(last_sell) = trades.into_iter().find(|t| {
            t.wallet_address == wallet && t.trade_type == crate::models::trade::TradeType::Sell
        }) {
            let remaining = trade_repo
                .net_token_amount_by_wallet_and_mint(&wallet, &mint)
                .await
                .unwrap_or(0.0)
                .max(0.0);
            if remaining <= super::PARTIAL_FILL_THRESHOLD {
                let exit_amount = last_sell.token_amount;
                position.close(
                    last_sell.price_per_token,
                    last_sell.tx_signature.clone(),
                    exit_amount,
                    last_sell.block_time,
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
                        position_id = %position.id, mint = %mint,
                        tx = %last_sell.tx_signature, pnl_percent,
                        "Position closed after confirmed sell"
                    );
                }
                return;
            }
        }
    }

    warn!(
        position_id = %position.id, mint = %mint,
        "Sell completed but no confirmed sell record found, or token balance remained"
    );
}

async fn sell_until_balance_cleared(
    trader: Arc<PumpFunTrader>,
    mint: String,
    mut amount: u64,
    trade_repo: TradeRepo,
    cache: &TokenCache,
    trade_signals: Arc<TradeSignals>,
    base_token_program: String,
) -> bool {
    let mut attempt = 0usize;
    let max_attempts = super::SELL_MAX_ATTEMPTS;
    let mut backoff_ms = 300u64;
    let wallet = trader.wallet_pubkey();

    if amount == 0 {
        info!(mint = %mint, "sell skipped because amount is zero");
        return true;
    }

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
        let sell_result = if is_migrated {
            trader
                .amm_sell(
                    &mint,
                    amount,
                    &base_token_program,
                    None,
                    token_account_override.as_deref(),
                    None,
                    tip_level,
                    false,
                )
                .await
        } else {
            trader
                .sell_token_once(&mint, amount, None, is_cashback, token_account_override.as_deref(), None, tip_level, false)
                .await
        };
        match sell_result {
            Ok(true) => {
                info!(mint = %mint, attempt, amount, "sell submitted (feed-confirm)");
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
                let guard = trade_signals.register(&wallet, &mint);
                let interval = Duration::from_millis(super::SELL_POLL_INTERVAL_MS);
                let deadline = Instant::now() + interval * super::SELL_POLL_MAX_ATTEMPTS as u32;
                loop {
                    // Arm the wakeup before the balance read (see the buy path:
                    // `notify_waiters` stores no permit, `enable()` registers up front).
                    let notified = guard.notified();
                    tokio::pin!(notified);
                    notified.as_mut().enable();

                    match trade_repo
                        .net_token_amount_by_wallet_and_mint(&wallet, &mint)
                        .await
                    {
                        Ok(balance) => {
                            let remaining = balance.max(0.0);
                            if remaining <= super::PARTIAL_FILL_THRESHOLD {
                                info!(mint = %mint, attempt, "sell cleared the balance");
                                cleared = true;
                                break;
                            }
                            remaining_amount = remaining as u64;
                        }
                        Err(err) => warn!("Failed to query net token balance: {err}"),
                    }
                    let now = Instant::now();
                    if now >= deadline {
                        break;
                    }
                    let fallback = interval.min(deadline - now);
                    tokio::select! {
                        _ = &mut notified => {}
                        _ = sleep(fallback) => {}
                    }
                }
                if cleared {
                    return true;
                }
                // Not cleared within the window: a partial fill retries the
                // remainder; an unchanged balance means the tx never landed, so
                // the next attempt re-sends with an escalated Jito tip (the outer
                // loop bumps `tip_level`). Bounded by SELL_MAX_ATTEMPTS, after
                // which the function returns false → position marked ExitFailed.
                warn!(mint = %mint, attempt, remaining = remaining_amount,
                    "sell not cleared within poll window; retrying with a higher tip");
                amount = remaining_amount;
            }
            Ok(false) => warn!(mint = %mint, attempt, amount, "sell returned false (no-op)"),
            Err(err) => warn!(mint = %mint, attempt, amount, "sell error: {err}"),
        }

        if attempt < max_attempts {
            sleep(Duration::from_millis(backoff_ms)).await;
            backoff_ms = (backoff_ms * 2).saturating_add(100);
        } else {
            warn!(mint = %mint, amount, "sell failed after {max_attempts} attempts");
            return false;
        }
    }

    amount == 0
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
        /// Write a Buy trade row for this wallet+mint, simulating the WS feed.
        async fn insert_fill(&self) {
            let trade = Trade::new(
                self.mint.clone(),
                self.wallet.clone(),
                TradeType::Buy,
                0.05,
                1000.0,
                format!("fill-{}", Uuid::new_v4().simple()),
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
        ) -> anyhow::Result<String> {
            let n = self.sends.fetch_add(1, Ordering::SeqCst) + 1;
            if self.fill_on_send == Some(n) {
                self.insert_fill().await;
            }
            Ok(format!("fakesig-{n}"))
        }
        async fn check_signature(&self, _signature: &str) -> anyhow::Result<Option<bool>> {
            if self.fill_on_status_check {
                self.insert_fill().await;
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
    ) -> (Position, Tpsl2PositionRepo, TradeRepo, Arc<Tpsl2RuntimeCache>) {
        let position_repo = Tpsl2PositionRepo::new(pool.clone());
        let trade_repo = TradeRepo::new(pool.clone());
        let runtime = Arc::new(Tpsl2RuntimeCache::new());
        let position = Position::new(
            mint.to_string(),
            wallet.to_string(),
            0.0,
            unique("create-"),
            "TPSL2".to_string(),
            Uuid::new_v4(),
            0.001,
        );
        position_repo.insert(&position).await.expect("insert position");
        runtime.sync_position(None, &position);
        (position, position_repo, trade_repo, runtime)
    }

    async fn cleanup(pool: &PgPool, mint: &str, position_id: Uuid) {
        let _ = sqlx::query("DELETE FROM trades WHERE mint_address = $1")
            .bind(mint)
            .execute(pool)
            .await;
        let _ = sqlx::query("DELETE FROM tpsl2_real_positions WHERE id = $1")
            .bind(position_id)
            .execute(pool)
            .await;
    }

    async fn run_buy(
        fake: Arc<FakeExecutor>,
        mint: &str,
        position: &Position,
        position_repo: &Tpsl2PositionRepo,
        trade_repo: &TradeRepo,
        runtime: &Arc<Tpsl2RuntimeCache>,
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
        assert!(updated.entry_price > 0.0, "entry recorded from the on-chain fill");
        assert_eq!(fake.send_count(), 1, "exactly one buy sent");
        cleanup(&pool, &mint, position.id).await;
    }

    #[tokio::test]
    #[ignore = "requires a local Postgres (DATABASE_URL); run with --ignored"]
    async fn db_top_guard_adopts_existing_fill_without_sending() {
        let Some(pool) = test_pool().await else { return };
        let (mint, wallet) = (unique("MINT"), unique("WALLET"));
        let (position, position_repo, trade_repo, runtime) = setup(&pool, &mint, &wallet).await;

        // A fill is ALREADY present before the buy task runs → adopt, never send.
        let fake = Arc::new(FakeExecutor::new(wallet, pool.clone(), mint.clone()));
        fake.insert_fill().await;
        run_buy(fake.clone(), &mint, &position, &position_repo, &trade_repo, &runtime).await;

        let updated = position_repo.find_by_id(position.id).await.unwrap().unwrap();
        assert!(updated.entry_price > 0.0, "adopted the pre-existing fill");
        assert_eq!(fake.send_count(), 0, "guard adopted the fill — no buy sent (no double-buy)");
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
        assert!(updated.entry_price > 0.0, "entry recorded after the re-send filled");
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
        assert_eq!(updated.entry_price, 0.0, "no entry recorded");
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
        assert_eq!(updated.entry_price, 0.0, "no entry recorded");
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
        assert!(updated.entry_price > 0.0, "entry recorded after the indexer caught up");
        assert_eq!(fake.send_count(), 1, "landed tx must NOT be re-sent (double-buy guard)");
        cleanup(&pool, &mint, position.id).await;
    }
}
