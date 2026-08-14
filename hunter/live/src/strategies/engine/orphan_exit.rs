//! Shared orphan / recovery exit path — sell via [`exec_real::run_exit`] (feed
//! confirm) and book PG rows without requiring a live engine registry entry.
//!
//! Helius budget: no account/balance RPC here. Cleared/stranded detection is
//! Postgres `trades` net only. Sell send + existing feed confirm live inside
//! `run_exit`. The one exception is [`fill_from_latest_sell`], which spends a
//! single batched `getTransaction` to heal a feed-missed sell — only when a bag
//! is already known cleared and PG still shows no sell leg, i.e. the alternative
//! is booking a false −100%.

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
use trading_core::storage::repositories::token_info_repo::TokenInfoRepo;
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
/// `dump = true` forces a no-floor sell (`slippage_bps = None`, min_out = 1):
/// accept whatever the pool gives, for a manual force-close of a near-drained
/// pool where the settings slippage floor would revert every attempt. The normal
/// (reaper / manual retry) path passes `false` and uses the configured slippage.
pub fn spawn_orphan_sell(
    deps: &OrphanExitDeps,
    position: StrategyPosition,
    default_reason: &str,
    dump: bool,
) -> OrphanStart {
    let token_amount = position.remaining_token_amount();
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

    let slippage = if dump {
        // No floor: min_out = 1, accept dust. Clears a rugged/near-drained pool
        // whose price has collapsed below any configured slippage floor.
        None
    } else {
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
        token_info_repo: TokenInfoRepo::new(deps.strategy_repo.pool().clone()),
        trade_signals: deps.trade_signals.clone(),
        fill_sigs: FillSigStore::new(),
        fill_tx,
        inflight: InFlightGuards::new(),
        buy_journal: SubmittedBuyJournal::new(),
        registry: deps.registry.clone(),
        engine_fill_tx: Some(deps.fill_tx.clone()),
        create_stamps: Arc::new(dashmap::DashMap::new()),
    };

    let repo = deps.strategy_repo.clone();
    let trade_repo = deps.trade_repo.clone();
    let registry = deps.registry.clone();
    let engine_fill_tx = deps.fill_tx.clone();
    let wallet = deps.trader.wallet_pubkey();
    let exit_reason = position
        .exit_reason
        .clone()
        .unwrap_or_else(|| default_reason.to_string());
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
                        pos.mark_exit_unconfirmed();
                        let _ = repo.update_position(&pos).await;
                    }
                }
            }
            Some(Event::FillFailed { reason: FillFailReason::Fatal, .. })
            | Some(Event::FillFailed { reason: FillFailReason::Reverted, .. }) => {
                if let Ok(Some(mut pos)) = repo.find_position(pg_id).await {
                    if matches!(
                        pos.status.as_str(),
                        "Holding" | "ExitPending" | "ExitStuck" | "ExitUnconfirmed"
                    ) {
                        pos.mark_exit_stuck();
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
///
/// The row keeps **its own** `exit_reason` when it has one. A bot exit that
/// reached this path was still decided by its rule — the bag merely cleared
/// outside the sell loop — and stamping `"Manual"` over it claimed a human sold
/// (2026-08-14, `FfuX44…pump`, whose real reason was `buy(50s) < 10`).
/// `"Manual"` stays the fallback for a row with no reason at all, which is
/// exactly the operator-closed case [`ExitCode::Manual`] means.
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
    let reason = pos.exit_reason.as_deref().unwrap_or("Manual");
    book_externally_cleared_pg(&deps.strategy_repo, pos.id, fill, reason).await
}

/// Book an externally-cleared row, or **park it** when the proceeds cannot be
/// established.
///
/// `fill = None` means no sell leg exists for this bag anywhere — not that we were
/// paid nothing. Booking it as a zero-proceeds close writes a −100% loss onto a
/// position that may well have been a winner, and `End` is terminal, so the lie
/// is permanent and silent. Parking keeps the row open and visible for a manual
/// decision instead, which is what every other unresolvable exit in the reaper
/// already does ("never auto-written-off").
///
/// A position with nothing left to sell is the one honest zero: a completed
/// scale-out ladder has no remainder, so a zero fill closes it correctly.
pub async fn book_cleared_or_park(
    deps: &OrphanExitDeps,
    pos: &StrategyPosition,
    fill: Option<Fill>,
) -> anyhow::Result<()> {
    match fill {
        Some(fill) => book_externally_cleared(deps, pos, fill).await,
        None if pos.remaining_token_amount() == 0 => {
            book_externally_cleared(
                deps,
                pos,
                Fill {
                    price: pos.exit_price.or(pos.entry_price).unwrap_or(0.0),
                    sol: 0.0,
                    token_amount: 0,
                    at: Utc::now(),
                },
            )
            .await
        }
        None => {
            warn!(
                position_id = %pos.id, mint = %pos.mint_address,
                "orphan_exit: bag cleared but no sell leg found — parking rather than \
                 booking a zero-proceeds close"
            );
            deps.strategy_repo.set_exit_parked(pos.id, true).await
        }
    }
}

/// PG-only externally-cleared close (registry miss / reaper heal).
///
/// Appends the final sell leg via `record_sell_fill` so the ledger + aggregates
/// stay consistent with the scale-out sink path (mig 0018).
pub async fn book_externally_cleared_pg(
    repo: &StrategyRepo,
    pg_id: Uuid,
    fill: Fill,
    reason: &str,
) -> anyhow::Result<()> {
    let Some(pos) = repo.find_position(pg_id).await? else {
        return Ok(());
    };
    if matches!(pos.status.as_str(), "End") {
        return Ok(());
    }
    // Size the final leg to the still-held remainder (scale-out stub or full bag).
    let token_amount = match pos.remaining_token_amount() {
        0 => fill.token_amount,
        rem => rem,
    };
    let sol = if fill.token_amount > 0 && token_amount != fill.token_amount {
        fill.sol * (token_amount as f64 / fill.token_amount as f64)
    } else {
        fill.sol
    };
    repo.record_sell_fill(
        pg_id,
        fill.price,
        sol,
        token_amount,
        fill.at,
        Some(reason),
        None,
        &[],
        true,
    )
    .await?;
    info!(position_id = %pg_id, reason, "orphan_exit: booked externally cleared → End");
    Ok(())
}

/// Resolve a Manual/Recovery fill from the wallet's latest sell on the mint (PG),
/// healing the feed from the row's own exit signatures first when it has to.
///
/// `None` when no sell leg for this bag exists anywhere — the caller must NOT
/// substitute a zero (see [`book_cleared_or_park`]). Previously this returned a
/// `sol: 0.0` fill in that case, which made a zero-proceeds close unavoidable in
/// precisely the branch built for "the sell landed, the feed missed it".
pub async fn fill_from_latest_sell(
    trade_repo: &TradeRepo,
    trader: &Arc<PumpFunTrader>,
    wallet: &str,
    pos: &StrategyPosition,
) -> Option<Fill> {
    let mut last_sell = trade_repo
        .find_latest_by_wallet_mint_type(wallet, &pos.mint_address, TradeType::Sell)
        .await
        .ok()
        .flatten();
    if last_sell.is_none() {
        // The bag is gone but the feed shows no sell — the exact gap the
        // signatures on the row exist to close.
        let healed = super::sell_backfill::heal_missing_sell_legs(
            trader,
            trade_repo,
            &pos.mint_address,
            &pos.exit_tx_sigs(),
        )
        .await;
        if healed > 0 {
            last_sell = trade_repo
                .find_latest_by_wallet_mint_type(wallet, &pos.mint_address, TradeType::Sell)
                .await
                .ok()
                .flatten();
        }
    }
    let token_amount = pos.remaining_token_amount();
    last_sell.map(|s| Fill {
        price: s.price_per_token,
        sol: s.price_per_token * token_amount as f64,
        token_amount,
        at: s.block_time,
    })
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
            sol: leader_fill.price * sib.remaining_token_amount() as f64,
            token_amount: sib.remaining_token_amount(),
            at: leader_fill.at,
        };
        if let Some(engine_id) = registry.engine_id(sib.id) {
            let _ = engine_fill_tx
                .send(Event::ExternallyCleared { position: engine_id, fill })
                .await;
            continue;
        }
        // The sibling's own reason, not a blanket `"Manual"` — its rule decided
        // this exit too; the leader's sell merely cleared the shared bag first.
        let reason = sib.exit_reason.clone().unwrap_or_else(|| "Manual".to_string());
        if let Err(e) = book_externally_cleared_pg(repo, sib.id, fill, &reason).await {
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
            let fill =
                fill_from_latest_sell(&deps.trade_repo, &deps.trader, &wallet, &pos).await;
            match book_cleared_or_park(deps, &pos, fill).await {
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
    use hunter_engine::arm::{ArmState, EnteredCtx};
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
                // An adopted row carries no metadata, and this token's
                // `TokenCreated` is long past — but the copycat guard is not blind
                // here: `boot::seed_dupe_guard` rebuilds its memory straight from
                // `strategy_positions ⋈ tokens`, so an adopted position still
                // blocks copycats. `None` only means a *further* entry on THIS
                // mint adds nothing the rebuild did not already record.
                identity: None,
                track,
                last_meaningful_at: None,
                last_trade_at: None,
                // Adopted cold: the evaluate sweep stamps the real verdict on the
                // first event this token sees.
                settled: None,
                first_slot_settled: true,
                arms: Default::default(),
                episodes: Default::default(),
            },
        );
    }
    // Manual position with TP/SL: re-synthesize its one-off exit rule so the
    // full exit stack (TP/SL + Dead) resumes after restart. Tracked-only manual
    // rows install nothing — the Entered arm stays inert (no auto-exit).
    if pos.origin == "manual" {
        state.set_manual_exit(position, rule_id, manual_exit_of(pos));
    }

    // Adoption rewrites this token's arms outside the fold, so the engine must
    // forget any settled verdict that predates it.
    state.touch_token(&mint);
    let token = state.tokens.get_mut(&mint)?;
    // Seed the position context from the adopted fill: `held` counts from the entry
    // time, `retrace` from the entry price (the peak is re-established as live trades
    // fold in — a conservative restart baseline). Mid-ladder `stage`/`sold_bps`
    // resume from PG aggregates (mig 0018).
    let sold_bps = pos.sold_bps();
    token.arms.insert(
        rule_id,
        ArmState::Entered(EnteredCtx {
            position,
            entry_price,
            entered_at: created_at,
            peak_price: entry_price,
            trough_price: entry_price,
            stage: pos.scale_stage,
            sold_bps,
        }),
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
            sold_token_amount: pos.sold_token_amount,
            scale_stage: pos.scale_stage,
            token_account: pos.token_account.clone(),
            entry_price: pos.entry_price,
            entry_sol: pos.entry_sol,
            entry_time: pos.entry_time,
            paper_target: None,
            cashback_enabled: false,
            inflight_intent: None,
        },
    );
    Some(position)
}

/// Decode a manual position's `manual_exit` JSONB (`{tp_pct, sl_pct}`) into the
/// engine's [`ManualExit`] — `None` when absent/empty (tracked-only).
pub fn manual_exit_of(pos: &StrategyPosition) -> Option<hunter_engine::event::ManualExit> {
    let v = pos.manual_exit.as_ref()?;
    let exit = hunter_engine::event::ManualExit {
        tp_pct: v.get("tp_pct").and_then(|x| x.as_f64()),
        sl_pct: v.get("sl_pct").and_then(|x| x.as_f64()),
    };
    exit.is_some().then_some(exit)
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
                // An adopted row carries no metadata, and this token's
                // `TokenCreated` is long past — but the copycat guard is not blind
                // here: `boot::seed_dupe_guard` rebuilds its memory straight from
                // `strategy_positions ⋈ tokens`, so an adopted position still
                // blocks copycats. `None` only means a *further* entry on THIS
                // mint adds nothing the rebuild did not already record.
                identity: None,
                track,
                last_meaningful_at: None,
                last_trade_at: None,
                // Adopted cold: the evaluate sweep stamps the real verdict on the
                // first event this token sees.
                settled: None,
                first_slot_settled: true,
                arms: Default::default(),
                episodes: Default::default(),
            },
        );
    }

    // Manual row: re-install its one-off TP/SL rule so a reaper-adopted fill
    // resumes the full exit stack.
    if pos.origin == "manual" {
        state.set_manual_exit(position, rule_id, manual_exit_of(pos));
    }

    // Fresh intent keying both the arm and the registry meta (see doc above).
    let intent = state.next_intent(rule_id, mint.clone());
    // See the sibling adopt path above.
    state.touch_token(&mint);
    let token = state.tokens.get_mut(&mint)?;
    token.arms.insert(
        rule_id,
        // `lamports: 0` — an adopted arm never re-sends on its own; an engine
        // retry falls back to the rule's configured amount (none for manual).
        ArmState::EntryPending { intent: intent.clone(), position, attempts: 1, lamports: 0 },
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
            sold_token_amount: pos.sold_token_amount,
            scale_stage: pos.scale_stage,
            token_account: pos.token_account.clone(),
            entry_price: pos.entry_price,
            entry_sol: pos.entry_sol,
            entry_time: pos.entry_time,
            paper_target: None,
            cashback_enabled: false,
            inflight_intent: Some(intent),
        },
    );
    Some(position)
}
