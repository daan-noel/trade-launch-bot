//! Shared orphan / recovery exit path — sell via [`exec_real::run_exit`] (feed
//! confirm) and book PG rows without requiring a live engine registry entry.
//!
//! Helius budget: no account/balance RPC here. Cleared/stranded detection is
//! Postgres `trades` net only. Sell send + existing feed confirm live inside
//! `run_exit`.

use std::sync::Arc;

use chrono::Utc;
use tokio::sync::{mpsc, watch};
use tracing::{info, warn};
use uuid::Uuid;

use hunter_engine::event::{Event, Fill, FillFailReason, IntentId, Mint, PositionId, RuleId};

use trading_core::config::constants::resolve_sell_slippage_bps;
use trading_core::models::strategy::StrategyPosition;
use trading_core::models::trade::TradeType;
use trading_core::state::token_cache::TokenCache;
use trading_core::state::trade_signals::TradeSignals;
use trading_core::storage::repositories::settings_repo::AppSettings;
use trading_core::storage::repositories::strategy_repo::StrategyRepo;
use trading_core::storage::repositories::trade_repo::TradeRepo;

use crate::trader::PumpFunTrader;

use super::exec_real::{self, RealExecDeps, SellOrder};
use super::{FillSigStore, InFlightGuards, PositionRegistry, SubmittedBuyJournal};

/// Raw-token dust floor for "bag cleared" (PG net). Matches Trade reconcile (`<= 0`).
pub const BAG_CLEARED_THRESHOLD_RAW: i64 = 0;

/// Dependencies for an orphan sell / book-close (HTTP + reapers share this).
pub struct OrphanExitDeps {
    pub strategy_repo: StrategyRepo,
    pub trade_repo: TradeRepo,
    pub trader: Arc<PumpFunTrader>,
    pub token_cache: Arc<TokenCache>,
    pub trade_signals: Arc<TradeSignals>,
    pub inflight: InFlightGuards,
    pub registry: PositionRegistry,
    /// Engine fill channel — used to fold `ExternallyCleared` for siblings still
    /// in the live registry (no extra sell).
    pub fill_tx: mpsc::Sender<Event>,
    pub settings: watch::Receiver<AppSettings>,
}

/// Outcome of [`spawn_orphan_sell`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrphanStart {
    /// Sell task spawned (or engine path should be used instead).
    Started,
    /// Could not claim exit / mint lock (another exit in flight).
    Busy,
    /// Position has nothing to sell (zero tokens).
    NothingToSell,
}

/// Spawn a direct `run_exit` for a PG row that is not (or may not be) in the live
/// engine registry. Claims pg + mint locks on `deps.inflight`; nested `run_exit`
/// uses a fresh guard set so it does not deadlock on the outer pg claim.
pub fn spawn_orphan_sell(
    deps: &OrphanExitDeps,
    position: StrategyPosition,
    default_reason: &str,
) -> OrphanStart {
    let token_amount = position.entry_token_amount.unwrap_or(0);
    if token_amount == 0 {
        return OrphanStart::NothingToSell;
    }
    if deps.inflight.exit_held(position.id) || deps.inflight.exit_mint_held(&position.mint_address) {
        return OrphanStart::Busy;
    }
    let Some(pg_guard) = deps.inflight.try_begin_exit(position.id) else {
        return OrphanStart::Busy;
    };
    let Some(mint_guard) = deps.inflight.try_begin_exit_mint(&position.mint_address) else {
        drop(pg_guard);
        return OrphanStart::Busy;
    };

    let slippage = {
        let s = deps.settings.borrow();
        resolve_sell_slippage_bps(s.sell_slippage_bps, None)
    };
    let intent = IntentId {
        rule: RuleId(position.rule_id.unwrap_or_default()),
        mint: Mint::from(position.mint_address.as_str()),
        seq: 0,
    };
    let order = SellOrder {
        intent,
        pg_id: position.id,
        mint: position.mint_address.clone(),
        token_amount,
        token_account: position.token_account.clone(),
        creator: None,
        token_program_id: position.token_program_id.clone(),
        cashback_enabled: false,
        slippage_bps: slippage,
    };

    let (fill_tx, mut fill_rx) = mpsc::channel::<Event>(4);
    let real_deps = RealExecDeps {
        trader: deps.trader.clone(),
        token_cache: deps.token_cache.clone(),
        trade_repo: deps.trade_repo.clone(),
        strategy_repo: deps.strategy_repo.clone(),
        trade_signals: deps.trade_signals.clone(),
        fill_sigs: FillSigStore::new(),
        fill_tx,
        inflight: InFlightGuards::new(),
        buy_journal: SubmittedBuyJournal::new(),
        registry: deps.registry.clone(),
        engine_fill_tx: Some(deps.fill_tx.clone()),
    };

    let repo = deps.strategy_repo.clone();
    let trade_repo = deps.trade_repo.clone();
    let token_cache = deps.token_cache.clone();
    let registry = deps.registry.clone();
    let engine_fill_tx = deps.fill_tx.clone();
    let wallet = deps.trader.wallet_pubkey();
    let exit_reason = position
        .exit_reason
        .clone()
        .unwrap_or_else(|| default_reason.to_string());
    let entry_price = position.entry_price;
    let mint = position.mint_address.clone();
    let pg_id = position.id;

    info!(
        position_id = %pg_id,
        mint = %mint,
        reason = %exit_reason,
        "orphan_exit: spawning direct sell"
    );

    tokio::spawn(async move {
        let _pg_guard = pg_guard;
        let _mint_guard = mint_guard;
        exec_real::run_exit(real_deps, order).await;
        match fill_rx.recv().await {
            Some(Event::FillConfirmed { fill, .. }) => {
                if let Ok(Some(mut pos)) = repo.find_position(pg_id).await {
                    if matches!(pos.status.as_str(), "End") {
                        return;
                    }
                    pos.close(
                        fill.price,
                        fill.sol,
                        fill.token_amount,
                        vec![],
                        fill.at,
                        &exit_reason,
                    );
                    if let Err(e) = repo.update_position(&pos).await {
                        warn!(position_id = %pg_id, "orphan_exit: close after sell failed: {e}");
                    } else {
                        info!(position_id = %pg_id, "orphan_exit: sold → End");
                    }
                }
                // Sibling book-close is handled inside run_exit/finish_cleared_sell
                // when wallet net is cleared (PG). Also heal here if that path missed.
                let _ = close_siblings_if_mint_cleared(
                    &repo,
                    &trade_repo,
                    &registry,
                    &engine_fill_tx,
                    &wallet,
                    &mint,
                    pg_id,
                    &fill,
                )
                .await;
            }
            Some(Event::FillFailed { reason: FillFailReason::Unconfirmed, .. }) => {
                if let Ok(Some(mut pos)) = repo.find_position(pg_id).await {
                    if pos.status == "ExitPending" || pos.status == "Holding" {
                        let price = token_cache
                            .get(&mint)
                            .and_then(|e| e.value().current_price)
                            .or(entry_price)
                            .unwrap_or(0.0);
                        pos.mark_exit_unconfirmed(price, Utc::now());
                        let _ = repo.update_position(&pos).await;
                    }
                }
            }
            Some(Event::FillFailed { reason: FillFailReason::Fatal, .. })
            | Some(Event::FillFailed { reason: FillFailReason::Reverted, .. }) => {
                if let Ok(Some(mut pos)) = repo.find_position(pg_id).await {
                    if matches!(pos.status.as_str(), "Holding" | "ExitPending" | "ExitFailed") {
                        let price = token_cache
                            .get(&mint)
                            .and_then(|e| e.value().current_price)
                            .or(entry_price)
                            .unwrap_or(0.0);
                        pos.mark_exit_failed(price, Utc::now());
                        if pos.exit_reason.is_none() {
                            pos.exit_reason = Some(exit_reason);
                        }
                        let _ = repo.update_position(&pos).await;
                    }
                }
            }
            _ => {
                warn!(
                    position_id = %pg_id,
                    "orphan_exit: sell unresolved — leaving for reaper"
                );
            }
        }
    });

    OrphanStart::Started
}

/// Book one position closed from an external/manual clear (no sell). Prefer
/// folding `ExternallyCleared` when the engine still owns the row; else PG-only.
pub async fn book_externally_cleared(
    deps: &OrphanExitDeps,
    pos: &StrategyPosition,
    fill: Fill,
) -> anyhow::Result<()> {
    if let Some(engine_id) = deps.registry.engine_id(pos.id) {
        let _ = deps
            .fill_tx
            .send(Event::ExternallyCleared { position: engine_id, fill })
            .await;
        return Ok(());
    }
    book_externally_cleared_pg(&deps.strategy_repo, pos.id, fill, "Manual").await
}

/// PG-only externally-cleared close (registry miss / reaper heal).
pub async fn book_externally_cleared_pg(
    repo: &StrategyRepo,
    pg_id: Uuid,
    fill: Fill,
    reason: &str,
) -> anyhow::Result<()> {
    let Some(mut pos) = repo.find_position(pg_id).await? else {
        return Ok(());
    };
    if matches!(pos.status.as_str(), "End") {
        return Ok(());
    }
    pos.close(
        fill.price,
        fill.sol,
        fill.token_amount,
        vec![],
        fill.at,
        reason,
    );
    repo.update_position(&pos).await?;
    info!(position_id = %pg_id, reason, "orphan_exit: booked externally cleared → End");
    Ok(())
}

/// Resolve a Manual/Recovery fill from the wallet's latest sell on the mint (PG).
pub async fn fill_from_latest_sell(
    trade_repo: &TradeRepo,
    wallet: &str,
    pos: &StrategyPosition,
) -> Fill {
    let last_sell = trade_repo
        .find_latest_by_wallet_mint_type(wallet, &pos.mint_address, TradeType::Sell)
        .await
        .ok()
        .flatten();
    let token_amount = pos.entry_token_amount.unwrap_or(0);
    match last_sell {
        Some(s) => Fill {
            price: s.price_per_token,
            sol: s.price_per_token * token_amount as f64,
            token_amount,
            at: s.block_time,
        },
        None => Fill {
            price: pos.entry_price.unwrap_or(0.0),
            sol: 0.0,
            token_amount,
            at: Utc::now(),
        },
    }
}

/// After a mint's wallet bag is cleared (PG net), close every other unsettled real
/// row on that mint — engine siblings via `ExternallyCleared`, else PG.
#[allow(clippy::too_many_arguments)]
pub async fn close_siblings_if_mint_cleared(
    repo: &StrategyRepo,
    trade_repo: &TradeRepo,
    registry: &PositionRegistry,
    engine_fill_tx: &mpsc::Sender<Event>,
    wallet: &str,
    mint: &str,
    exclude_pg: Uuid,
    leader_fill: &Fill,
) -> anyhow::Result<()> {
    let net = trade_repo
        .net_token_amount_by_wallet_and_mint(wallet, mint)
        .await
        .unwrap_or(i64::MAX);
    if net > BAG_CLEARED_THRESHOLD_RAW {
        return Ok(());
    }
    let siblings = repo
        .find_unsettled_real_on_mint(wallet, mint, exclude_pg)
        .await
        .unwrap_or_default();
    for sib in siblings {
        let fill = Fill {
            price: leader_fill.price,
            sol: leader_fill.price * sib.entry_token_amount.unwrap_or(0) as f64,
            token_amount: sib.entry_token_amount.unwrap_or(0),
            at: leader_fill.at,
        };
        if let Some(engine_id) = registry.engine_id(sib.id) {
            let _ = engine_fill_tx
                .send(Event::ExternallyCleared { position: engine_id, fill })
                .await;
        } else if let Err(e) = book_externally_cleared_pg(repo, sib.id, fill, "Manual").await {
            warn!(position_id = %sib.id, "orphan_exit: sibling book-close failed: {e}");
        }
    }
    Ok(())
}

/// Reconcile Holding rows whose bag is already gone (PG `trades` net) — zero RPC.
pub async fn reconcile_externally_cleared_holdings(deps: &OrphanExitDeps) {
    let mints = match deps
        .strategy_repo
        .find_externally_cleared_holding_mints(BAG_CLEARED_THRESHOLD_RAW)
        .await
    {
        Ok(m) => m,
        Err(e) => {
            warn!("orphan_exit: find_externally_cleared_holding_mints failed: {e}");
            return;
        }
    };
    if mints.is_empty() {
        return;
    }
    let wallet = deps.trader.wallet_pubkey();
    let mut closed = 0u32;
    for mint in mints {
        let positions = match deps.strategy_repo.find_open_by_mint(&mint, "real").await {
            Ok(p) => p,
            Err(e) => {
                warn!(mint = %mint, "orphan_exit: find_open_by_mint failed: {e}");
                continue;
            }
        };
        for pos in positions {
            let fill = fill_from_latest_sell(&deps.trade_repo, &wallet, &pos).await;
            match book_externally_cleared(deps, &pos, fill).await {
                Ok(()) => closed += 1,
                Err(e) => warn!(position_id = %pos.id, "orphan_exit: cleared-Holding book failed: {e}"),
            }
        }
    }
    if closed > 0 {
        info!(closed, "orphan_exit: booked externally-cleared Holding rows");
    }
}

/// Adopt PG `Holding` rows into the live engine + registry so TP/SL/Dead and Ops
/// close work after restart — for **both** modes: a real adopt resumes the live
/// exit, a paper adopt resumes the simulated exit (`dispatch_sell` routes by the
/// frozen `trade_mode`) so a restart doesn't strand paper bags as forever-`Open`.
/// **PG-only** — no RPC.
pub fn adopt_holding_into_engine(
    state: &mut hunter_engine::EngineState,
    registry: &PositionRegistry,
    pos: &StrategyPosition,
) -> Option<PositionId> {
    use hunter_engine::arm::ArmState;
    use hunter_engine::event::TradeMode;
    use hunter_engine::grouping::TokenFingerprint;
    use hunter_engine::state::{PositionRef, RuleCounters, TokenState};
    use super::PositionMeta;

    if pos.status != "Holding" || pos.entry_price.is_none() {
        return None;
    }
    let trade_mode = match pos.mode.as_str() {
        "real" => TradeMode::Real,
        "paper" => TradeMode::Paper,
        _ => return None,
    };
    if registry.engine_id(pos.id).is_some() {
        return None;
    }
    let rule_id = RuleId(pos.rule_id?);
    let mint = Mint::from(pos.mint_address.as_str());
    let entry_price = pos.entry_price.unwrap_or(0.0);
    let created_at = pos.entry_time.unwrap_or(pos.created_at);

    if let Some(token) = state.tokens.get(&mint) {
        if token.arms.contains_key(&rule_id) {
            return None;
        }
    }

    let track = state.new_track(created_at);
    let position = state.next_position();
    state
        .positions
        .insert(position, PositionRef { mint: mint.clone(), rule: rule_id });

    if !state.tokens.contains_key(&mint) {
        state.tokens.insert(
            mint.clone(),
            TokenState {
                created_at,
                tf: TokenFingerprint::default(),
                track,
                last_meaningful_at: None,
                first_slot_settled: true,
                arms: Default::default(),
                episodes: Default::default(),
            },
        );
    }
    let token = state.tokens.get_mut(&mint)?;
    // Seed the position context from the adopted fill: `held` counts from the entry
    // time, `retrace` from the entry price (the peak is re-established as live trades
    // fold in — a conservative restart baseline).
    token.arms.insert(
        rule_id,
        ArmState::Entered { position, entry_price, entered_at: created_at, peak_price: entry_price },
    );

    let ctr = state.counters.entry(rule_id).or_insert(RuleCounters::default());
    ctr.open = ctr.open.saturating_add(1);

    registry.upsert(
        position,
        PositionMeta {
            pg_id: pos.id,
            run_id: pos.run_id,
            rule_id,
            mint: pos.mint_address.clone(),
            trade_mode,
            token_program_id: pos.token_program_id.clone(),
            creator: None,
            entry_token_amount: pos.entry_token_amount,
            token_account: pos.token_account.clone(),
            entry_price: pos.entry_price,
            paper_target: None,
            cashback_enabled: false,
            inflight_intent: None,
        },
    );
    Some(position)
}

/// Adopt a PG `BuySubmitted` row into the live engine + registry as an inert
/// `EntryPending` arm so a restart cannot **double-buy** the same (rule, mint):
/// the occupied arm makes the engine's entry sweep skip it (`decide_arm` returns
/// `None` for `EntryPending`), and the reaper's existing fill/revert nudge folds it
/// to `Holding` or terminal — exactly the same-process orphan path. **PG-only.**
///
/// The reconstructed `intent` keys BOTH the arm and the registry meta so the
/// reaper's `FillConfirmed`/`FillFailed` (which reads `meta.inflight_intent`)
/// matches the arm's `pend == intent` guard. The arm never re-buys on its own:
/// `EntryPending` only advances on those two events, never on a `Tick`.
pub fn adopt_buy_submitted_into_engine(
    state: &mut hunter_engine::EngineState,
    registry: &PositionRegistry,
    pos: &StrategyPosition,
) -> Option<PositionId> {
    use hunter_engine::arm::ArmState;
    use hunter_engine::event::TradeMode;
    use hunter_engine::grouping::TokenFingerprint;
    use hunter_engine::state::{PositionRef, RuleCounters, TokenState};
    use super::PositionMeta;

    if pos.status != "BuySubmitted" {
        return None;
    }
    if registry.engine_id(pos.id).is_some() {
        return None;
    }
    let rule_id = RuleId(pos.rule_id?);
    let mint = Mint::from(pos.mint_address.as_str());
    let trade_mode = match pos.mode.as_str() {
        "real" => TradeMode::Real,
        "paper" => TradeMode::Paper,
        _ => return None,
    };
    let created_at = pos.created_at;

    if let Some(token) = state.tokens.get(&mint) {
        if token.arms.contains_key(&rule_id) {
            return None;
        }
    }

    let track = state.new_track(created_at);
    let position = state.next_position();
    state
        .positions
        .insert(position, PositionRef { mint: mint.clone(), rule: rule_id });

    if !state.tokens.contains_key(&mint) {
        state.tokens.insert(
            mint.clone(),
            TokenState {
                created_at,
                tf: TokenFingerprint::default(),
                track,
                last_meaningful_at: None,
                first_slot_settled: true,
                arms: Default::default(),
                episodes: Default::default(),
            },
        );
    }

    // Fresh intent keying both the arm and the registry meta (see doc above).
    let intent = state.next_intent(rule_id, mint.clone());
    let token = state.tokens.get_mut(&mint)?;
    token.arms.insert(
        rule_id,
        ArmState::EntryPending { intent: intent.clone(), position, attempts: 1 },
    );

    let ctr = state.counters.entry(rule_id).or_insert(RuleCounters::default());
    ctr.open = ctr.open.saturating_add(1);

    registry.upsert(
        position,
        PositionMeta {
            pg_id: pos.id,
            run_id: pos.run_id,
            rule_id,
            mint: pos.mint_address.clone(),
            trade_mode,
            token_program_id: pos.token_program_id.clone(),
            creator: None,
            entry_token_amount: pos.entry_token_amount,
            token_account: pos.token_account.clone(),
            entry_price: pos.entry_price,
            paper_target: None,
            cashback_enabled: false,
            inflight_intent: Some(intent),
        },
    );
    Some(position)
}
