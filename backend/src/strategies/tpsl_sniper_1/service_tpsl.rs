use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use sqlx::PgPool;
use tokio::sync::broadcast;
use tokio::time::sleep;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::models::ingest::SseEvent;
use crate::models::Position;
use crate::state::token_cache::TokenCache;
use crate::storage::repositories::{
    paper_trading_repo::PaperTradingRepo, position_repo::PositionRepo,
    strategy_tpsl_rule_repo::StrategyTPSLRuleRepo, trade_repo::TradeRepo,
};
use super::{TPSLStrategyHandler, TpslRuntimeCache};
use crate::trader::PumpFunTrader;
use super::util::ignore_zero_u64;

pub struct TpslStrategyService {
    pool: PgPool,
    position_repo: PositionRepo,
    paper_repo: PaperTradingRepo,
    trade_repo: TradeRepo,
    trader: Arc<PumpFunTrader>,
    runtime: Arc<TpslRuntimeCache>,
    /// Cold-lane broadcast for client notifications (e.g. paper-run finished).
    sse_tx: broadcast::Sender<SseEvent>,
}

impl TpslStrategyService {
    pub fn new(
        pool: PgPool,
        trader: Arc<PumpFunTrader>,
        runtime: Arc<TpslRuntimeCache>,
        sse_tx: broadcast::Sender<SseEvent>,
    ) -> Self {
        Self {
            position_repo: PositionRepo::new(pool.clone()),
            paper_repo: PaperTradingRepo::new(pool.clone()),
            trade_repo: TradeRepo::new(pool.clone()),
            pool,
            trader,
            runtime,
            sse_tx,
        }
    }

    const BUY_MAX_ATTEMPTS: usize = 3;
    const BUY_POLL_MAX_ATTEMPTS: usize = 12;
    const BUY_POLL_INTERVAL_MS: u64 = 1_000;
    const SELL_MAX_ATTEMPTS: usize = 6;
    const SELL_POLL_MAX_ATTEMPTS: usize = 10;
    const SELL_POLL_INTERVAL_MS: u64 = 500;
    const PARTIAL_FILL_THRESHOLD: f64 = 0.0001;
    const EXIT_PENDING_CLEANUP_INTERVAL_MS: u64 = 60_000;
    const EXIT_PENDING_STALE_MS: u64 = 300_000;
}

impl TpslStrategyService {
    pub fn spawn_background_tasks(&self) {
        let cleanup_repo = self.position_repo.clone();
        let paper_repo = self.paper_repo.clone();
        let runtime = self.runtime.clone();
        let pool = self.pool.clone();
        tokio::spawn(async move {
            let stale = Duration::from_millis(TpslStrategyService::EXIT_PENDING_STALE_MS);
            let mut interval = tokio::time::interval(Duration::from_millis(
                TpslStrategyService::EXIT_PENDING_CLEANUP_INTERVAL_MS,
            ));
            loop {
                interval.tick().await;
                let mut reopened_total: u64 = 0;
                match cleanup_repo.reopen_stale_exit_pending(stale).await {
                    Ok(n) => reopened_total += n,
                    Err(err) => warn!("Failed to clean stale ExitPending positions: {err}"),
                }
                match paper_repo.reopen_stale_exit_pending(stale).await {
                    Ok(n) => reopened_total += n,
                    Err(err) => warn!("Failed to clean stale paper ExitPending positions: {err}"),
                }
                if reopened_total > 0 {
                    info!("Reopened {reopened_total} stale ExitPending positions");
                    if let Err(err) = runtime.reload_holding(&pool).await {
                        warn!("Failed to refresh TPSL holding cache after cleanup: {err}");
                    }
                }
            }
        });
    }

    pub async fn on_token_created(&self, mint: &str, cache: &TokenCache) {
        let mint = mint.to_string();
        let token = match cache.get(&mint) {
            Some(entry) => entry.value().token.clone(),
            None => {
                debug!("Token {mint} not in cache — skipping TPSL create");
                return;
            }
        };

        let rules = self.runtime.active_rules();

        if rules.is_empty() {
            debug!("No active TPSL rules — skipping token {mint}");
            return;
        }

        let handler = TPSLStrategyHandler::new(rules);
        if let Some(rule_id) = handler.check_buy_entry(&token) {
            info!("Token {mint} matches TPSL buy entry rule {rule_id}");

            if let Some(rule) = handler.get_rule(rule_id) {
                let is_paper = rule.trade_mode == "paper";
                let max_concurrent_tokens = ignore_zero_u64(rule.p_max_concurrent_tokens).map(|v| v as usize);
                let max_total_tokens =
                    ignore_zero_u64(rule.p_max_total_tokens).map(|v| v as usize);

                // Paper rules trade inside a run; ensure one exists before the
                // caps are checked (its counters are scoped to the run). A run is
                // normally started on activation — this lazily starts one for a
                // rule that became active without going through the API path.
                let paper_run_id = if is_paper {
                    Some(match self.runtime.current_paper_run(rule_id) {
                        Some(r) => r.run_id,
                        None => match self
                            .runtime
                            .start_paper_run(&self.pool, rule_id, ignore_zero_u64(rule.p_max_total_tokens))
                            .await
                        {
                            Ok(r) => r.id,
                            Err(err) => {
                                warn!("Failed to start paper run for rule {rule_id}: {err}");
                                return;
                            }
                        },
                    })
                } else {
                    None
                };

                if let Some(cap) = max_concurrent_tokens {
                    let current_holding = self.runtime.holding_count_by_rule(rule_id);
                    if current_holding >= cap as i64 {
                        debug!(
                            "Rule {rule_id} reached max concurrent tokens \
                             ({current_holding}/{cap}), skipping {mint}"
                        );
                        return;
                    }
                }

                if let Some(total_max) = max_total_tokens {
                    let total_traded = self.runtime.total_count_by_rule(rule_id);
                    if total_traded >= total_max as i64 {
                        debug!(
                            "Rule {rule_id} reached max total tokens \
                             ({total_traded}/{total_max}), skipping {mint}"
                        );
                        return;
                    }
                }

                let mut position = Position::new(
                    mint.clone(),
                    self.trader.wallet_pubkey(),
                    0.0,
                    token.creation_tx_signature.clone(),
                    "TPSL".to_string(),
                    rule_id,
                    rule.buy_amount,
                );
                position.token_program_id = token.token_program_id.clone();

                let insert_res = match paper_run_id {
                    Some(run_id) => self.paper_repo.insert(&position, run_id).await,
                    None => self.position_repo.insert(&position).await,
                };
                if let Err(err) = insert_res {
                    warn!("Failed to create position for token {mint}: {err}");
                    return;
                }
                self.runtime.sync_position(None, &position);

                info!(
                    "Created position {} for token {mint} under rule {rule_id}",
                    position.id
                );

                if rule.trade_mode == "paper" {
                    let trade_repo = self.trade_repo.clone();
                    let mint_for_trades = mint.clone();
                    let position_repo = self.paper_repo.clone();
                    let runtime = self.runtime.clone();
                    let position_id = position.id;
                    let buy_amount = rule.buy_amount;
                    tokio::spawn(async move {
                        // Poll the WS-fed trade feed for the token's opening trades,
                        // exactly as real mode polls for its on-chain fill. The entry
                        // price/tx/time come only from a real indexed trade — paper mode
                        // never synthesizes a fill from a create-time snapshot.
                        let mut recorded = false;
                        for _ in 0..TpslStrategyService::BUY_POLL_MAX_ATTEMPTS {
                            sleep(Duration::from_millis(
                                TpslStrategyService::BUY_POLL_INTERVAL_MS,
                            ))
                            .await;
                            let Ok(trades) = trade_repo.find_by_mint_all(&mint_for_trades).await
                            else {
                                continue;
                            };
                            if let Some((entry_price, entry_tx, entry_time)) =
                                super::simulation_tpsl::find_entry(&trades, 5)
                            {
                                if let Ok(Some(prev)) = position_repo.find_by_id(position_id).await {
                                    let _ = position_repo
                                        .update_entry(position_id, &entry_tx, buy_amount, entry_price, entry_time)
                                        .await;
                                    if let Ok(Some(current)) = position_repo.find_by_id(position_id).await {
                                        runtime.sync_position(Some(&prev), &current);
                                    }
                                }
                                info!(
                                    "[PAPER] Set entry for position {}: {} (tx: {})",
                                    position_id, entry_price, entry_tx
                                );
                                recorded = true;
                                break;
                            }
                        }
                        if !recorded {
                            // No real fill was indexed within the poll window. Mirror
                            // real mode's "buy not found" cleanup: drop the unentered
                            // position rather than leave a 0-entry row that can never
                            // trade. No create-time price is ever synthesized.
                            if let Ok(Some(pos)) = position_repo.find_by_id(position_id).await {
                                if pos.entry_price <= 0.0 {
                                    let _ = position_repo.delete_position(position_id).await;
                                    runtime.remove_position(&pos);
                                    info!(
                                        "[PAPER] Removed position {} for mint {}: no entry fill indexed",
                                        position_id, mint_for_trades
                                    );
                                }
                            }
                        }
                    });
                } else if rule.trade_mode == "real" {
                    let trader = self.trader.clone();
                    let mint_clone = mint.clone();
                    let creator = token.creator_wallet.clone();
                    let buy_amount = rule.buy_amount;
                    let position_id = position.id;
                    let position_repo = self.position_repo.clone();
                    let trade_repo = self.trade_repo.clone();
                    let runtime = self.runtime.clone();
                    let token_program_id = position
                        .token_program_id
                        .clone()
                        .unwrap_or_else(|| crate::config::constants::TOKEN_PROGRAM_ID.to_string());
                    tokio::spawn(async move {
                        let mint_for_log = mint_clone.clone();
                        buy_with_retries(
                            trader,
                            mint_clone,
                            creator,
                            token_program_id,
                            buy_amount,
                            position_id,
                            position_repo.clone(),
                            trade_repo.clone(),
                            runtime.clone(),
                            BuyRetryCfg::production(),
                        )
                        .await;
                        if let Ok(Some(pos)) = position_repo.find_by_id(position_id).await {
                            if pos.entry_price == 0.0 {
                                let _ = position_repo.delete_position(position_id).await;
                                runtime.remove_position(&pos);
                                info!(
                                    "[REAL] Removed position {} for mint {}: buy not found",
                                    position_id, mint_for_log
                                );
                            }
                        }
                    });
                }
            }
        } else {
            debug!("Token {mint} does not match any TPSL buy entry rule");
        }
    }

    pub async fn on_trade_executed(&self, mint: &str, cache: &TokenCache) {
        let mint = mint.to_string();
        let current_price = match cache.get(&mint) {
            Some(entry) => entry.value().current_price.unwrap_or(0.0),
            None => return,
        };

        let positions = self.runtime.holding_by_mint(&mint);
        if positions.is_empty() {
            return;
        }

        let handler = TPSLStrategyHandler::new(self.runtime.all_rules_vec());

        for mut position in positions {
            if let Some(rule) = handler.get_rule(position.rule_id) {
                if let Some(exit_reason) = handler.check_exit(&position, current_price, rule) {
                    debug!(
                        "Position {} for token {mint} triggered exit: {:?}",
                        position.id, exit_reason
                    );

                    if rule.trade_mode == "paper" {
                        let prev = position.clone();
                        position.mark_exit_pending();
                        if let Err(err) = self.paper_repo.update(&position).await {
                            warn!(
                                "Failed to mark position {} as ExitPending: {err}",
                                position.id
                            );
                        } else {
                            self.runtime.sync_position(Some(&prev), &position);
                            info!(position_id = %position.id, mint = %mint,
                                "[PAPER] Position marked ExitPending");
                        }
                        let trade_repo = self.trade_repo.clone();
                        let mint_for_trades = mint.clone();
                        let position_repo = self.paper_repo.clone();
                        let runtime = self.runtime.clone();
                        let position_id = position.id;
                        let entry_tx = position.entry_tx.clone();
                        let entry_price = position.entry_price;
                        // The block time of this position's recorded entry. Used to
                        // window `find_exit` to post-entry trades only.
                        let entry_time_db = position.entry_time;
                        let take_profit = rule.take_profit;
                        let stop_loss = rule.stop_loss;
                        // Captured for the post-close finish check (cap reached + all exited).
                        let pool = self.pool.clone();
                        let sse_tx = self.sse_tx.clone();
                        let rule_id = rule.id;
                        let rule_name = rule.rule_name.clone();
                        let max_total = ignore_zero_u64(rule.p_max_total_tokens);
                        tokio::spawn(async move {
                            let mut found = false;
                            let start = std::time::Instant::now();
                            while start.elapsed() < Duration::from_secs(10) {
                                if let Ok(trades) =
                                    trade_repo.find_by_mint_all(&mint_for_trades).await
                                {
                                    // Prefer the entry trade's own block time; fall
                                    // back to the entry_time stored on the position
                                    // (set together with entry_price). The old
                                    // `Utc::now()` fallback made every trade look
                                    // pre-entry whenever the entry row wasn't in the
                                    // fetched set, so `find_exit` saw nothing and the
                                    // position always reverted to Holding.
                                    let entry_block_time = trades
                                        .iter()
                                        .find(|t| t.tx_signature == entry_tx)
                                        .map(|t| t.block_time)
                                        .or(entry_time_db)
                                        .unwrap_or_else(chrono::Utc::now);
                                    if let Some((exit_price, exit_tx, exit_time, reason)) =
                                        super::simulation_tpsl::find_exit(
                                            &trades,
                                            entry_block_time,
                                            entry_price,
                                            take_profit,
                                            stop_loss,
                                        )
                                    {
                                        if let Ok(Some(prev)) = position_repo.find_by_id(position_id).await {
                                            let _ = position_repo
                                                .update_exit(position_id, &exit_tx, exit_price, exit_time)
                                                .await;
                                            if let Ok(Some(current)) =
                                                position_repo.find_by_id(position_id).await
                                            {
                                                runtime.sync_position(Some(&prev), &current);
                                            }
                                        }
                                        info!(
                                            "[PAPER] Set exit for position {}: {} (tx: {}, reason: {})",
                                            position_id, exit_price, exit_tx, reason
                                        );
                                        found = true;
                                        break;
                                    }
                                }
                                sleep(Duration::from_millis(500)).await;
                            }
                            if !found {
                                // No real post-entry exit trade was indexed within the
                                // poll window. Don't synthesize an exit price — revert to
                                // Holding so the next trade event can re-trigger the exit
                                // (mirrors real mode reopening a position when the sell
                                // doesn't go through). The exit price only ever comes from
                                // a real indexed trade via `find_exit`.
                                if let Ok(Some(prev)) = position_repo.find_by_id(position_id).await {
                                    let _ = position_repo.revert_exit_pending(position_id).await;
                                    if let Ok(Some(current)) = position_repo.find_by_id(position_id).await {
                                        runtime.sync_position(Some(&prev), &current);
                                    }
                                }
                                info!(
                                    "[PAPER] No exit fill indexed; reverted position {} to Holding",
                                    position_id
                                );
                            }

                            // If this exit closed the position, the run may now be
                            // complete (cap reached and nothing left open).
                            if let Ok(Some(pos)) = position_repo.find_by_id(position_id).await {
                                if pos.status == crate::models::PositionStatus::End {
                                    maybe_finish_paper_run(
                                        &pool, &runtime, &sse_tx, rule_id, &rule_name, max_total,
                                    )
                                    .await;
                                }
                            }
                        });
                    } else if rule.trade_mode == "real" {
                        let prev = position.clone();
                        position.mark_exit_pending();
                        if let Err(err) = self.position_repo.update(&position).await {
                            warn!(
                                "Failed to mark position {} as ExitPending: {err}",
                                position.id
                            );
                        } else {
                            self.runtime.sync_position(Some(&prev), &position);
                            info!(position_id = %position.id, mint = %mint,
                                "[REAL] Position marked ExitPending");
                        }
                        // Single exit pass. `sell_with_retries` (inside) owns the
                        // retry + partial-fill loop and re-reads migration routing
                        // from the WS cache on every attempt, so a mid-exit
                        // migration self-heals to the AMM path.
                        // `execute_sell_for_position` then closes the position (on a
                        // confirmed sell) or reopens it to Holding — in which case
                        // the next trade event re-triggers the exit. This replaces
                        // the old outer 10× loop, which multiplied with the inner
                        // retries (10×6×5) for no benefit.
                        execute_sell_for_position(
                            self.trader.clone(),
                            position.clone(),
                            self.position_repo.clone(),
                            self.trade_repo.clone(),
                            self.runtime.clone(),
                            cache,
                        )
                        .await;
                    }
                }
            }
        }
    }
}

/// After a paper position closes, finish the run if its total-token cap has
/// been reached and no positions remain open: auto-deactivate the rule, refresh
/// the rules cache, and broadcast a [`SseEvent::PaperTestFinished`] notification.
///
/// No-ops when the rule has no cap (`max_total` is `None`) — such a run only ends
/// on manual stop — or when the cap/holding conditions are not yet met.
async fn maybe_finish_paper_run(
    pool: &PgPool,
    runtime: &Arc<TpslRuntimeCache>,
    sse_tx: &broadcast::Sender<SseEvent>,
    rule_id: Uuid,
    rule_name: &str,
    max_total: Option<u64>,
) {
    let Some(cap) = max_total else { return };
    let total = runtime.total_count_by_rule(rule_id);
    let holding = runtime.holding_count_by_rule(rule_id);
    if total < cap as i64 || holding > 0 {
        return;
    }

    match runtime.finish_paper_run(pool, rule_id).await {
        Ok(Some(run)) => {
            // Auto-deactivate the rule so it stops cleanly, then refresh the cache.
            let rule_repo = StrategyTPSLRuleRepo::new(pool.clone());
            match rule_repo.find_by_id(rule_id).await {
                Ok(Some(mut rule)) if rule.is_active => {
                    rule.is_active = false;
                    if let Err(err) = rule_repo.update(&rule).await {
                        warn!("Failed to deactivate finished paper rule {rule_id}: {err}");
                    }
                }
                Ok(_) => {}
                Err(err) => warn!("Failed to load rule {rule_id} for paper finish: {err}"),
            }
            if let Err(err) = runtime.reload_rules(pool).await {
                warn!("Failed to reload rules after paper finish: {err}");
            }
            let _ = sse_tx.send(SseEvent::PaperTestFinished {
                rule_id,
                rule_name: rule_name.to_string(),
                run_seq: run.run_seq,
                tokens_traded: total,
                timestamp: Utc::now(),
            });
            info!(
                %rule_id, run_seq = run.run_seq, tokens = total,
                "[PAPER] run finished — rule auto-deactivated"
            );
        }
        Ok(None) => {} // already finished or stopped
        Err(err) => warn!("Failed to finish paper run for rule {rule_id}: {err}"),
    }
}

// ---------------------------------------------------------------------------
// Trading helpers — used only by TPSL for now
// ---------------------------------------------------------------------------

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
/// classify a sent signature. A trait so `buy_with_retries` can be driven by a
/// scripted fake in tests (see the `tests` module) instead of a live chain.
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

/// Retry/poll timing for `buy_with_retries`. `production()` mirrors the
/// `TpslStrategyService::BUY_*` constants; tests shrink it so the give-up and
/// re-send paths don't wait out real 12×1s poll windows.
#[derive(Clone, Copy)]
struct BuyRetryCfg {
    max_attempts: usize,
    backoff_ms: u64,
    poll_attempts: usize,
    poll_interval: Duration,
}

impl BuyRetryCfg {
    fn production() -> Self {
        Self {
            max_attempts: TpslStrategyService::BUY_MAX_ATTEMPTS,
            backoff_ms: 500,
            poll_attempts: TpslStrategyService::BUY_POLL_MAX_ATTEMPTS,
            poll_interval: Duration::from_millis(TpslStrategyService::BUY_POLL_INTERVAL_MS),
        }
    }
}

async fn buy_with_retries<E: SnipeExecutor + 'static>(
    trader: Arc<E>,
    mint: String,
    creator: String,
    token_program_id: String,
    buy_amount: f64,
    position_id: Uuid,
    position_repo: PositionRepo,
    trade_repo: TradeRepo,
    runtime: Arc<TpslRuntimeCache>,
    cfg: BuyRetryCfg,
) {
    let wallet = trader.wallet();
    let max_attempts = cfg.max_attempts;
    let mut backoff_ms = cfg.backoff_ms;

    for attempt in 1..=max_attempts {
        // Never double-send: if a previous attempt's buy has since landed and been
        // indexed, adopt that fill instead of firing another buy.
        if record_entry_if_present(&mint, &wallet, position_id, &position_repo, &trade_repo, &runtime)
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
        if poll_for_entry(&mint, &wallet, position_id, &position_repo, &trade_repo, &runtime, &cfg)
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
                if poll_for_entry(&mint, &wallet, position_id, &position_repo, &trade_repo, &runtime, &cfg)
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
async fn poll_for_entry(
    mint: &str,
    wallet: &str,
    position_id: Uuid,
    position_repo: &PositionRepo,
    trade_repo: &TradeRepo,
    runtime: &Arc<TpslRuntimeCache>,
    cfg: &BuyRetryCfg,
) -> bool {
    for _ in 0..cfg.poll_attempts {
        if record_entry_if_present(mint, wallet, position_id, position_repo, trade_repo, runtime).await
        {
            return true;
        }
        sleep(cfg.poll_interval).await;
    }
    false
}

/// If a buy by `wallet` on `mint` is already present in the trade feed, record it
/// as the position entry (price/amount/tx/time come from the on-chain fill) and
/// return true; otherwise return false without sleeping.
async fn record_entry_if_present(
    mint: &str,
    wallet: &str,
    position_id: Uuid,
    position_repo: &PositionRepo,
    trade_repo: &TradeRepo,
    runtime: &Arc<TpslRuntimeCache>,
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

async fn execute_sell_for_position(
    trader: Arc<PumpFunTrader>,
    mut position: Position,
    position_repo: PositionRepo,
    trade_repo: TradeRepo,
    runtime: Arc<TpslRuntimeCache>,
    cache: &TokenCache,
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

    let completed = sell_with_retries(
        trader.clone(),
        mint.clone(),
        amount,
        trade_repo.clone(),
        cache,
        base_token_program,
    )
    .await;
    if !completed {
        warn!(
            position_id = %position.id, mint = %mint,
            "Sell execution finished without clearing token balance; reverting position to Holding"
        );
        let prev = position.clone();
        position.reopen();
        if let Err(err) = position_repo.update(&position).await {
            warn!(
                position_id = %position.id, mint = %mint,
                "Failed to revert position {} to Holding: {err}", position.id
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
            if remaining <= TpslStrategyService::PARTIAL_FILL_THRESHOLD {
                let exit_amount = last_sell.token_amount;
                position.close(
                    last_sell.price_per_token,
                    last_sell.tx_signature.clone(),
                    exit_amount,
                    last_sell.block_time,
                );
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

async fn sell_with_retries(
    trader: Arc<PumpFunTrader>,
    mint: String,
    mut amount: u64,
    trade_repo: TradeRepo,
    cache: &TokenCache,
    base_token_program: String,
) -> bool {
    let mut attempt = 0usize;
    let max_attempts = TpslStrategyService::SELL_MAX_ATTEMPTS;
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
        let sell_result = if is_migrated {
            trader
                .amm_sell(
                    &mint,
                    amount,
                    &base_token_program,
                    None,
                    token_account_override.as_deref(),
                    None,
                )
                .await
        } else {
            trader
                .sell_token_once(&mint, amount, None, is_cashback, token_account_override.as_deref(), None)
                .await
        };
        match sell_result {
            Ok(true) => {
                info!(mint = %mint, attempt, amount, "sell submitted");
                let mut poll_attempts = 0usize;
                while poll_attempts < TpslStrategyService::SELL_POLL_MAX_ATTEMPTS {
                    poll_attempts += 1;
                    match trade_repo
                        .net_token_amount_by_wallet_and_mint(&wallet, &mint)
                        .await
                    {
                        Ok(balance) => {
                            let remaining = balance.max(0.0);
                            if remaining <= TpslStrategyService::PARTIAL_FILL_THRESHOLD {
                                info!(mint = %mint, attempt, amount,
                                    "sell completed, no remaining balance");
                                return true;
                            }
                            warn!(mint = %mint, attempt, remaining,
                                "partial fill detected, retrying remaining amount");
                            amount = remaining as u64;
                            break;
                        }
                        Err(err) => warn!("Failed to query net token balance: {err}"),
                    }
                    sleep(Duration::from_millis(TpslStrategyService::SELL_POLL_INTERVAL_MS)).await;
                }

                if poll_attempts >= TpslStrategyService::SELL_POLL_MAX_ATTEMPTS {
                    warn!(
                        mint = %mint,
                        "Timed out waiting for sell confirmations; remaining amount: {}", amount
                    );
                    return false;
                }
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
    // Full-flow integration tests for `buy_with_retries`, driven by a scripted
    // fake executor against a real local Postgres. `#[ignore]` like the other
    // DB/network tests — run with a local DB:
    //   $env:DATABASE_URL = "postgres://..."; cargo test -p backend -- --ignored
    // Unique mint/wallet ids keep them from colliding with real data, and each
    // test cleans up the rows it created.
    // -------------------------------------------------------------------------

    use crate::config::constants::TOKEN_PROGRAM_ID;
    use crate::models::trade::{Trade, TradeType};
    use chrono::Utc;
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
    /// the tests can drive every branch of `buy_with_retries` deterministically.
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
    ) -> (Position, PositionRepo, TradeRepo, Arc<TpslRuntimeCache>) {
        let position_repo = PositionRepo::new(pool.clone());
        let trade_repo = TradeRepo::new(pool.clone());
        let runtime = Arc::new(TpslRuntimeCache::new());
        let position = Position::new(
            mint.to_string(),
            wallet.to_string(),
            0.0,
            unique("create-"),
            "TPSL".to_string(),
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
        let _ = sqlx::query("DELETE FROM positions WHERE id = $1")
            .bind(position_id)
            .execute(pool)
            .await;
    }

    async fn run_buy(
        fake: Arc<FakeExecutor>,
        mint: &str,
        position: &Position,
        position_repo: &PositionRepo,
        trade_repo: &TradeRepo,
        runtime: &Arc<TpslRuntimeCache>,
    ) {
        buy_with_retries(
            fake,
            mint.to_string(),
            "creator".to_string(),
            TOKEN_PROGRAM_ID.to_string(),
            0.001,
            position.id,
            position_repo.clone(),
            trade_repo.clone(),
            runtime.clone(),
            test_cfg(),
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "requires a local Postgres (DATABASE_URL); run with --ignored"]
    async fn db_happy_path_records_entry_from_ws_fill() {
        let Some(pool) = test_pool().await else { return };
        let (mint, wallet) = (unique("MINT"), unique("WALLET"));
        let (position, position_repo, trade_repo, runtime) = setup(&pool, &mint, &wallet).await;

        // First send produces the on-chain fill row; the poll picks it up.
        let fake = Arc::new(FakeExecutor::new(wallet, pool.clone(), mint.clone()).fill_on_send(1));
        run_buy(fake.clone(), &mint, &position, &position_repo, &trade_repo, &runtime).await;

        let updated = position_repo.find_by_id(position.id).await.unwrap().unwrap();
        assert!(updated.entry_price > 0.0, "entry recorded from the WS fill");
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
