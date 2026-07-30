//! THE one serialized decision loop (plan 4.1). Every `reduce` call in the live
//! bin happens here, so there is exactly one place a token/rule decision is made —
//! no mint-sharding (decision 11), determinism preserved by construction.
//!
//! `select!` inputs (biased order — commands before fills so HTTP reloads/closes
//! are not starved under heavy fill traffic; fills still beat the tick so a
//! confirmed fill is folded before the next tick re-evaluates):
//! 1. **commands** — rule reloads + manual closes from HTTP ([`EngineCommand`]),
//! 2. **fills** — `FillConfirmed`/`FillFailed` from the executor adapters,
//! 3. **ingest pings** — mapped to events by the [`Producer`],
//! 4. **Clock tick** (`TICK_MS`, currently 200 ms) — a synthetic `Tick(now)` so
//!    quiet-token metrics fire.
//!
//! Effects are dispatched in two passes per event: state effects
//! (`PositionUpdate`/`ArmedChanged` → PG + SSE via the [`Sink`]) **first**, so a
//! `BuySubmitted` row + registry entry exist before the buy is spawned; then the
//! submit effects (`SubmitBuy`/`SubmitSell` → executor tasks).

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::sync::{broadcast, mpsc, watch};
use tokio::task::JoinHandle;
use tracing::{info, warn};

use hunter_engine::arm::ArmState;
use hunter_engine::event::{
    Effect, Event, IntentId, LoadedRule, Mint, PositionId, RuleId, TradeMode,
};
use hunter_engine::fingerprint::Fingerprint as EngineFingerprint;
use hunter_engine::{reduce, EngineState};

use trading_core::config::constants::{
    resolve_buy_slippage_bps, resolve_sell_slippage_bps, sol_to_lamports, MAX_SNIPE_AGE_SECS,
};
use trading_core::models::ingest::{SseEvent, StrategyPing};
use trading_core::storage::repositories::settings_repo::AppSettings;
use trading_core::state::token_cache::TokenCache;
use trading_core::state::trade_signals::TradeSignals;
use trading_core::storage::repositories::fingerprint_repo::FingerprintRepo;
use trading_core::storage::repositories::rule_repo::RuleRepo;
use trading_core::storage::repositories::strategy_repo::StrategyRepo;
use trading_core::storage::repositories::token_info_repo::TokenInfoRepo;
use trading_core::storage::repositories::trade_repo::TradeRepo;

use crate::trader::PumpFunTrader;

use super::convert;
use super::event_log::{self, EventLogRecorder};
use super::exec_real::{BuyOrder, RealExecDeps, SellOrder};
use super::producers::Producer;
use super::sinks::Sink;
use super::{
    exec_paper, ArmedRegistry, EngineCommand, EngineHandle, InFlightGuards, PositionRegistry,
};

/// The clock tick — derived from the engine's [`hunter_engine::TICK_MS`] SSOT so
/// live + lab replay tick at the one cadence (plan 5.3).
const TICK: Duration = Duration::from_millis(hunter_engine::TICK_MS as u64);
/// How often (in ticks) to prune the producer's per-mint maps to the live set.
/// ≈ 2 min at `TICK_MS = 200`.
const PRUNE_EVERY_TICKS: u64 = 600;

/// Everything the engine loop needs to run. Built once in `main.rs` and moved into
/// [`spawn_engine`].
pub struct EngineDeps {
    /// The ingest strategy-ping channel (same one the old `StrategyRunner` drained).
    pub ping_rx: mpsc::Receiver<StrategyPing>,
    pub rule_repo: RuleRepo,
    pub fp_repo: FingerprintRepo,
    pub strategy_repo: StrategyRepo,
    pub trade_repo: TradeRepo,
    pub token_cache: Arc<TokenCache>,
    pub trader: Arc<PumpFunTrader>,
    pub trade_signals: Arc<TradeSignals>,
    pub sse_tx: broadcast::Sender<SseEvent>,
    pub settings: watch::Receiver<AppSettings>,
    /// Retains LaserStream AMM pool subs for unsettled real positions.
    pub held_pools: crate::ingest::HeldPoolGate,
}

/// Build the command/fill channels, spawn the loop, and return the HTTP-facing
/// [`EngineHandle`] plus the loop's `JoinHandle` (a fault there is fatal, like the
/// old strategy runner).
pub fn spawn_engine(deps: EngineDeps) -> EngineHandles {
    let (cmd_tx, cmd_rx) = mpsc::channel::<EngineCommand>(256);
    let (fill_tx, fill_rx) = mpsc::channel::<Event>(1024);
    let handle = EngineHandle::new(cmd_tx);
    let armed = ArmedRegistry::new();
    let positions = PositionRegistry::new();
    let inflight = InFlightGuards::new();
    let task = tokio::spawn(run_loop(
        deps,
        cmd_rx,
        fill_tx.clone(),
        fill_rx,
        armed.clone(),
        positions.clone(),
        inflight.clone(),
    ));
    EngineHandles {
        handle,
        armed,
        positions,
        inflight,
        fill_tx,
        task,
    }
}

/// What [`spawn_engine`] hands back to `main.rs`: the HTTP-facing handle + the two
/// shared registries the rest of the process reads, plus the loop's task handle
/// (a fault there is fatal, like the old strategy runner).
pub struct EngineHandles {
    pub handle: EngineHandle,
    pub armed: ArmedRegistry,
    pub positions: PositionRegistry,
    /// Shared with HTTP orphan-close + reapers so Ops/Trade never race a live exit.
    pub inflight: InFlightGuards,
    /// Engine event ingress (fills + ExternallyCleared) — HTTP orphan-close uses it
    /// to fold sibling clears into the live loop without a second sell.
    pub fill_tx: mpsc::Sender<Event>,
    pub task: JoinHandle<()>,
}

/// The loop body. Owns all engine state; the only concurrency is the spawned
/// executor tasks, which feed fills back through `fill_rx`.
async fn run_loop(
    deps: EngineDeps,
    mut cmd_rx: mpsc::Receiver<EngineCommand>,
    fill_tx: mpsc::Sender<Event>,
    mut fill_rx: mpsc::Receiver<Event>,
    armed: ArmedRegistry,
    registry: PositionRegistry,
    inflight: InFlightGuards,
) {
    let EngineDeps {
        mut ping_rx,
        rule_repo,
        fp_repo,
        strategy_repo,
        trade_repo,
        token_cache,
        trader,
        trade_signals,
        sse_tx,
        settings,
        held_pools,
    } = deps;

    let wallet = trader.wallet_pubkey();
    let fill_sigs = super::FillSigStore::new();
    let buy_journal = super::SubmittedBuyJournal::new();

    let mut state = EngineState::new();
    let mut producer = Producer::new(token_cache.clone());
    let mut sink = Sink::new(
        strategy_repo.clone(),
        token_cache.clone(),
        sse_tx.clone(),
        registry.clone(),
        armed,
        fill_sigs.clone(),
        wallet.clone(),
        Some(trader.clone()),
        Some(held_pools),
    );
    let recorder = EventLogRecorder::from_env();

    let real_deps = RealExecDeps {
        trader: trader.clone(),
        token_cache: token_cache.clone(),
        trade_repo: trade_repo.clone(),
        strategy_repo: strategy_repo.clone(),
        token_info_repo: TokenInfoRepo::new(strategy_repo.pool().clone()),
        trade_signals: trade_signals.clone(),
        fill_sigs: fill_sigs.clone(),
        fill_tx: fill_tx.clone(),
        inflight: inflight.clone(),
        buy_journal,
        registry: registry.clone(),
        engine_fill_tx: Some(fill_tx.clone()),
    };

    // Initial rule load.
    if let Err(e) = reload_rules(
        &rule_repo,
        &fp_repo,
        &strategy_repo,
        &registry,
        &mut state,
        &mut sink,
    )
    .await
    {
        warn!("engine: initial rule load failed: {e}");
    }

    // Boot recovery: replay the recent log to re-arm tokens that had no open
    // position at crash time (effects discarded — PG + reapers own open rows).
    boot_recover(&strategy_repo, &recorder, &mut state).await;

    // Adopt PG position rows + re-entry episode counters (see `boot`).
    let _ = super::boot::adopt_from_db(&strategy_repo, &mut state, &registry).await;

    // Recovery reaper (buy adopt/drop + exit redrive + cleared Holding + ExitStuck
    // bag + stale ExitPending bag-check). Immediate first tick, then every 60 s.
    let _reaper = super::reapers::spawn_reaper(super::reapers::ReaperDeps {
        strategy_repo: strategy_repo.clone(),
        trade_repo: trade_repo.clone(),
        trader: trader.clone(),
        token_cache: token_cache.clone(),
        trade_signals: trade_signals.clone(),
        inflight: inflight.clone(),
        registry: registry.clone(),
        fill_tx: fill_tx.clone(),
        settings: settings.clone(),
        sse_tx: sse_tx.clone(),
        // Opt-in: this is the reaper's only Helius spend. Off unless asked for.
        onchain_bag_check: super::reapers::onchain_bag_check_from_env(),
    });

    let mut tick = tokio::time::interval(TICK);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut ticks: u64 = 0;

    info!(
        tick_ms = hunter_engine::TICK_MS,
        "strategy engine loop running (serialized clock tick)"
    );

    loop {
        // A batch of events to fold this iteration (usually one).
        let batch: EventBatch = tokio::select! {
            biased;
            Some(cmd) = cmd_rx.recv() => {
                if let EngineCommand::ReloadRules { ack } = cmd {
                    let mut acks = vec![ack];
                    while let Ok(EngineCommand::ReloadRules { ack }) = cmd_rx.try_recv() {
                        acks.push(ack);
                    }
                    let result = reload_rules(
                        &rule_repo,
                        &fp_repo,
                        &strategy_repo,
                        &registry,
                        &mut state,
                        &mut sink,
                    )
                    .await;
                    if acks.len() > 1 {
                        info!(waiters = acks.len(), "engine: coalesced reload requests");
                    } else {
                        info!("engine: reload requested");
                    }
                    for a in acks {
                        let _ = a.send(result.clone());
                    }
                    EventBatch::none()
                } else if let EngineCommand::ReseedFromDb { ack } = cmd {
                    let result = reseed_from_db(
                        &rule_repo,
                        &fp_repo,
                        &strategy_repo,
                        &registry,
                        &mut state,
                        &mut sink,
                    )
                    .await;
                    let _ = ack.send(result);
                    EventBatch::none()
                } else {
                    handle_command(
                        cmd,
                        &registry,
                        &mut state,
                        &mut sink,
                    )
                    .await
                }
            }
            Some(fill) = fill_rx.recv() => EventBatch::one(fill),
            Some(ping) = ping_rx.recv() => EventBatch::many(producer.on_ping(&ping).into_vec()),
            _ = tick.tick() => {
                ticks = ticks.wrapping_add(1);
                if ticks.is_multiple_of(PRUNE_EVERY_TICKS) {
                    let live: HashSet<String> = state.tokens.keys().map(|m| m.to_string()).collect();
                    producer.retain(|m| live.contains(m));
                }
                EventBatch::one(Event::Tick { now: Utc::now() })
            }
            else => break,
        };

        for event in batch.events {
            if let Some(rec) = recorder.as_ref() {
                rec.record(&event);
            }
            let effects = reduce(&mut state, event);
            dispatch(
                effects, &state, &mut sink, &registry, &real_deps, &token_cache, &settings,
            )
            .await;
        }
    }

    warn!("strategy engine loop exited (all inputs closed)");
}

/// A small batch wrapper so every `select!` arm yields the same type.
struct EventBatch {
    events: Vec<Event>,
}
impl EventBatch {
    fn one(e: Event) -> Self {
        Self { events: vec![e] }
    }
    fn many(events: Vec<Event>) -> Self {
        Self { events }
    }
    fn none() -> Self {
        Self { events: Vec::new() }
    }
}

/// Dispatch a `reduce` call's effects. State effects first (registry + SSE;
/// BuySubmitted/ExitPending PG is async — see [`Sink`]), then submit effects
/// so buy/sell spawn is not gated on durable writes.
async fn dispatch(
    effects: hunter_engine::reduce::Effects,
    state: &EngineState,
    sink: &mut Sink,
    registry: &PositionRegistry,
    real_deps: &RealExecDeps,
    token_cache: &Arc<TokenCache>,
    settings: &watch::Receiver<AppSettings>,
) {
    // Pass 1 — registry/SSE (+ background PG for BuySubmitted / ExitPending).
    for fx in &effects {
        match fx {
            Effect::PositionUpdate(delta) => sink.on_position_update(delta.clone()).await,
            Effect::ArmedChanged(delta) => sink.on_armed_changed(delta),
            _ => {}
        }
    }
    // Pass 2 — act on submit effects (spawn is ready once registry is upserted).
    for fx in effects {
        match fx {
            Effect::SubmitBuy { intent, rule, mint, lamports } => {
                dispatch_buy(
                    state, registry, real_deps, token_cache, settings, intent, rule, mint, lamports,
                );
            }
            Effect::SubmitSell { intent, position, reason: _, portion } => {
                dispatch_sell(registry, real_deps, token_cache, settings, intent, position, portion);
            }
            _ => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn dispatch_buy(
    state: &EngineState,
    registry: &PositionRegistry,
    real_deps: &RealExecDeps,
    token_cache: &Arc<TokenCache>,
    settings: &watch::Receiver<AppSettings>,
    intent: IntentId,
    rule: RuleId,
    mint: Mint,
    lamports: u64,
) {
    let mint_s = mint.to_string();
    // Pass 1 already persisted BuySubmitted → PositionMeta; prefer that frozen
    // trade_mode for this entry (incl. fold retries) so a rule reload cannot
    // flip paper↔real mid-attempt. Fall back to the live rule only if meta is
    // somehow missing.
    let position = state.tokens.get(&mint).and_then(|t| match t.arms.get(&rule) {
        Some(ArmState::EntryPending { position, .. }) => Some(*position),
        _ => None,
    });
    let mode = position
        .and_then(|p| registry.get(p).map(|m| m.trade_mode))
        .or_else(|| state.rules.get(&rule).map(|c| c.trade_mode));
    match mode {
        Some(TradeMode::Paper) => {
            let trigger_abs = exec_paper::latest_trade_abs_idx(token_cache, &mint_s);
            tokio::spawn(exec_paper::run_entry(
                real_deps.fill_tx.clone(),
                token_cache.clone(),
                registry.clone(),
                position,
                intent,
                mint_s,
                lamports,
                trigger_abs,
            ));
        }
        Some(TradeMode::Real) => {
            let Some(position) = position else {
                warn!(mint = %mint_s, "real buy: no EntryPending arm — skipping submit");
                return;
            };
            let Some(meta) = registry.get(position) else {
                warn!(mint = %mint_s, "real buy: no position meta — skipping submit");
                return;
            };

            // Pre-buy SOL guards (balance-floor + optional max_committed_sol).
            // Fail → FillFailed::Reverted so the engine can retry when free SOL returns.
            if !real_deps.trader.can_commit_buy(lamports) {
                warn!(mint = %mint_s, lamports, "real buy blocked by SOL balance-floor guard");
                let tx = real_deps.fill_tx.clone();
                let intent = intent.clone();
                tokio::spawn(async move {
                    let _ = tx
                        .send(Event::FillFailed {
                            intent,
                            reason: hunter_engine::event::FillFailReason::Reverted,
                        })
                        .await;
                });
                return;
            }
            if let Some(max_sol) = settings.borrow().max_committed_sol {
                let ceiling = sol_to_lamports(max_sol).max(0) as u64;
                let committed = real_deps.trader.committed_lamports();
                if committed.saturating_add(lamports) > ceiling {
                    warn!(
                        mint = %mint_s, lamports, committed, ceiling,
                        "real buy blocked by max_committed_sol guard"
                    );
                    let tx = real_deps.fill_tx.clone();
                    let intent = intent.clone();
                    tokio::spawn(async move {
                        let _ = tx
                            .send(Event::FillFailed {
                                intent,
                                reason: hunter_engine::event::FillFailReason::Reverted,
                            })
                            .await;
                    });
                    return;
                }
            }

            let (creator, token_program_id, cashback) = token_cache
                .get(&mint_s)
                .map(|e| {
                    let t = &e.value().token;
                    (
                        t.creator_wallet.clone(),
                        t.token_program_id.clone().unwrap_or_default(),
                        t.is_cashback_enabled,
                    )
                })
                .unwrap_or_default();
            let slippage_bps = buy_slippage(settings);
            registry.update(position, |m| {
                m.inflight_intent = Some(intent.clone());
            });
            let order = BuyOrder {
                intent,
                pg_id: meta.pg_id,
                mint: mint_s,
                creator,
                token_program_id,
                lamports,
                cashback_enabled: cashback,
                slippage_bps,
                // Set once an earlier attempt for THIS position funded an account:
                // the retry then buys straight into it instead of minting a second
                // seeded account for the same position.
                token_account: meta.token_account.clone(),
            };
            tokio::spawn(super::exec_real::run_entry(real_deps.clone(), order));
        }
        None => warn!(rule = %rule.0, "buy for unknown rule — skipping"),
    }
}

fn dispatch_sell(
    registry: &PositionRegistry,
    real_deps: &RealExecDeps,
    token_cache: &Arc<TokenCache>,
    settings: &watch::Receiver<AppSettings>,
    intent: IntentId,
    position: PositionId,
    portion: hunter_engine::event::Portion,
) {
    let Some(meta) = registry.get(position) else {
        warn!("sell: no position meta — skipping submit");
        return;
    };
    let mint = meta.mint.clone();
    let initial = meta.entry_token_amount.unwrap_or(0);
    let token_amount = portion.token_amount(initial, meta.sold_token_amount);
    if token_amount == 0 {
        warn!(portion = ?portion, initial, sold = meta.sold_token_amount, "sell: zero token amount — skipping");
        return;
    }
    match meta.trade_mode {
        TradeMode::Paper => {
            let fire_abs = exec_paper::latest_trade_abs_idx(token_cache, &mint);
            tokio::spawn(exec_paper::run_exit(
                real_deps.fill_tx.clone(),
                token_cache.clone(),
                intent,
                mint,
                token_amount,
                fire_abs,
            ));
        }
        TradeMode::Real => {
            registry.update(position, |m| {
                m.inflight_intent = Some(intent.clone());
            });
            let order = SellOrder {
                intent,
                pg_id: meta.pg_id,
                mint,
                token_amount,
                token_account: meta.token_account,
                creator: meta.creator,
                token_program_id: meta.token_program_id,
                cashback_enabled: meta.cashback_enabled,
                slippage_bps: sell_slippage(settings),
            };
            tokio::spawn(super::exec_real::run_exit(real_deps.clone(), order));
        }
    }
}

/// Handle an [`EngineCommand`], returning any events it produces to fold.
/// [`EngineCommand::ReloadRules`] is handled in `run_loop` (coalesced acks).
async fn handle_command(
    cmd: EngineCommand,
    registry: &PositionRegistry,
    state: &mut EngineState,
    sink: &mut Sink,
) -> EventBatch {
    match cmd {
        EngineCommand::ReloadRules { .. } | EngineCommand::ReseedFromDb { .. } => {
            // Handled in `run_loop` — unreachable through this path.
            EventBatch::none()
        }
        EngineCommand::ManualClose { pg_position_id, portion } => match registry.engine_id(pg_position_id) {
            Some(position) => EventBatch::one(Event::ManualClose { position, portion }),
            None => {
                // HTTP close_position handles registry-miss via orphan_exit; a bare
                // command with no registry entry is a no-op (never pretend success).
                warn!(pg = %pg_position_id, "manual close: no live engine position — ignoring");
                EventBatch::none()
            }
        },
        EngineCommand::ReconcileCleared { pg_position_id, fill } => {
            match registry.engine_id(pg_position_id) {
                Some(position) => EventBatch::one(Event::ExternallyCleared { position, fill }),
                None => {
                    // Trade-sell reconcile falls back to PG book-close on miss.
                    warn!(pg = %pg_position_id,
                        "reconcile cleared: no live engine position — ignoring");
                    EventBatch::none()
                }
            }
        }
        EngineCommand::CloseRule { rule_id } => {
            let rid = RuleId(rule_id);
            let mut positions = registry.positions_for_rule(rid);
            for (_mint, token) in &state.tokens {
                if let Some(ArmState::EntryPending { position, .. }) = token.arms.get(&rid) {
                    if registry.get(*position).is_none() {
                        positions.push(*position);
                    }
                }
            }
            positions.sort_unstable();
            positions.dedup();
            info!(rule = %rule_id, positions = positions.len(), "engine: stop rule — closing open positions");
            EventBatch::many(
                positions
                    .into_iter()
                    .map(|position| Event::ManualClose {
                        position,
                        portion: hunter_engine::event::Portion::All,
                    })
                    .collect(),
            )
        }
        EngineCommand::CloseMode { real } => {
            let mode = if real { TradeMode::Real } else { TradeMode::Paper };
            let positions = registry.positions_for_mode(mode);
            info!(?mode, positions = positions.len(), "engine: stop-all — closing open positions");
            EventBatch::many(
                positions
                    .into_iter()
                    .map(|position| Event::ManualClose {
                        position,
                        portion: hunter_engine::event::Portion::All,
                    })
                    .collect(),
            )
        }
        EngineCommand::ManualBuy { pg_id, mint, lamports, exit } => {
            // Fresh per-episode rule id: keys the arm, the intents, and the
            // synthesized exit rule. Staged with the sink BEFORE the fold so the
            // BuySubmitted delta creates the row with the handler's pre-minted id.
            let rule = RuleId(uuid::Uuid::new_v4());
            sink.stage_manual(
                rule,
                super::sinks::StagedManual {
                    pg_id,
                    manual_exit: exit.filter(|e| e.is_some()).map(|e| {
                        serde_json::json!({ "tp_pct": e.tp_pct, "sl_pct": e.sl_pct })
                    }),
                },
            );
            info!(pg = %pg_id, mint = %mint, lamports, "engine: manual buy episode");
            EventBatch::one(Event::ManualBuy {
                mint: Mint::from(mint.as_str()),
                rule,
                lamports,
                at: Utc::now(),
                exit,
            })
        }
        EngineCommand::SetManualExit { pg_position_id, exit } => {
            match registry.engine_id(pg_position_id) {
                Some(position) => EventBatch::one(Event::SetManualExit { position, exit }),
                None => {
                    // Not engine-held (e.g. still BuySubmitted-adopted or stuck) —
                    // the persisted JSONB re-installs the rule on the next adopt.
                    warn!(pg = %pg_position_id, "set manual exit: no live engine position — deferred to adopt");
                    EventBatch::none()
                }
            }
        }
    }
}

/// Load active rules + drain rules (paused but holding open positions) +
/// fingerprints from PG, refresh the sink's rule info, and fold a `RulesReloaded`
/// event into `state`.
async fn reload_rules(
    rule_repo: &RuleRepo,
    fp_repo: &FingerprintRepo,
    strategy_repo: &StrategyRepo,
    registry: &PositionRegistry,
    state: &mut EngineState,
    sink: &mut Sink,
) -> Result<(), String> {
    let rules = rule_repo
        .list_active()
        .await
        .map_err(|e| format!("list_active rules: {e}"))?;
    let fps = fp_repo
        .list()
        .await
        .map_err(|e| format!("list fingerprints: {e}"))?;

    let active_ids: HashSet<RuleId> = rules.iter().map(|r| RuleId(r.id)).collect();
    let mut drain_ids: HashSet<RuleId> = registry.open_rule_ids();
    if let Ok(open) = strategy_repo.find_open_positions().await {
        for p in open {
            if let Some(rid) = p.rule_id {
                drain_ids.insert(RuleId(rid));
            }
        }
    }
    drain_ids.retain(|id| !active_ids.contains(id));

    let mut all_rows = rules;
    for rid in drain_ids {
        let uuid = rid.0;
        match rule_repo.find(uuid).await {
            Ok(Some(row)) if row.is_enabled => all_rows.push(row),
            Ok(_) => {}
            Err(e) => return Err(format!("find drain rule {uuid}: {e}")),
        }
    }

    let mut names: Vec<(RuleId, String)> = Vec::with_capacity(all_rows.len());
    let loaded: Vec<LoadedRule> = all_rows
        .iter()
        .filter_map(|r| match convert::rule_to_loaded(r) {
            Ok(l) => {
                names.push((l.id, r.rule_name.clone()));
                Some(l)
            }
            Err(e) => {
                warn!(rule = %r.id, "engine: skipping rule with invalid params: {e}");
                None
            }
        })
        .collect();
    let engine_fps: Vec<EngineFingerprint> = fps.iter().map(convert::fp_to_engine).collect();

    sink.set_rules(&loaded, &names);
    sink.warm_runs().await;
    info!(
        rules = loaded.len(),
        fingerprints = engine_fps.len(),
        "engine: rules reloaded"
    );
    let _ = reduce(
        state,
        Event::RulesReloaded { rules: loaded.into(), fps: engine_fps.into() },
    );
    Ok(())
}

/// Admin reseed: reload rules/fingerprints from PG, then adopt open position rows
/// and re-entry episode counters (same boot path, without event-log replay).
async fn reseed_from_db(
    rule_repo: &RuleRepo,
    fp_repo: &FingerprintRepo,
    strategy_repo: &StrategyRepo,
    registry: &PositionRegistry,
    state: &mut EngineState,
    sink: &mut Sink,
) -> Result<super::EngineReseedReport, String> {
    reload_rules(rule_repo, fp_repo, strategy_repo, registry, state, sink).await?;
    let adopt = super::boot::adopt_from_db(strategy_repo, state, registry).await;
    Ok(super::EngineReseedReport {
        rules: state.rules.len(),
        holdings_adopted: adopt.holdings,
        buy_submitted_adopted: adopt.buy_submitted,
        episodes_seeded: adopt.episodes,
    })
}

/// Replay the recent event-log tail to re-arm tokens that had no open position at
/// crash time. Effects are **discarded** — this only rebuilds in-memory armed
/// state; PG rows (Holding/BuySubmitted/ExitPending) are reconciled by the reapers.
async fn boot_recover(
    strategy_repo: &StrategyRepo,
    recorder: &Option<EventLogRecorder>,
    state: &mut EngineState,
) {
    let Some(rec) = recorder else { return };
    let held: HashSet<String> = strategy_repo
        .find_open_positions()
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|p| p.mint_address)
        .collect();
    let events = event_log::recover_armed(rec.dir(), MAX_SNIPE_AGE_SECS, Utc::now(), &held);
    let n = events.len();
    for ev in events {
        let _ = reduce(state, ev); // discard effects — re-arm only, never re-act
    }
    if n > 0 {
        info!(events = n, held = held.len(), "engine: boot recovery replayed");
    }
}

fn buy_slippage(settings: &watch::Receiver<AppSettings>) -> Option<u64> {
    resolve_buy_slippage_bps(settings.borrow().buy_slippage_bps, None)
}

fn sell_slippage(settings: &watch::Receiver<AppSettings>) -> Option<u64> {
    resolve_sell_slippage_bps(settings.borrow().sell_slippage_bps, None)
}
