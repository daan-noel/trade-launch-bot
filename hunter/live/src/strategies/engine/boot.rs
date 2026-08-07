//! PG-backed boot / reseed helpers for the live engine loop.
//!
//! Used at process start and on admin `ReseedFromDb` to reconcile in-memory
//! position registry + episode counters with durable rows without a restart.

use chrono::{Duration, Utc};
use hunter_engine::event::RuleId;
use hunter_engine::event::{Mint, TradeMode};
use hunter_engine::state::EngineState;
use hunter_engine::token_identity_hash;
use tracing::{info, warn};

use trading_core::storage::repositories::settings_repo::AppSettings;
use trading_core::storage::repositories::strategy_repo::StrategyRepo;

use super::PositionRegistry;

/// Counts from a PG position-adopt pass (holdings, in-flight buys, re-entry seeds,
/// copycat-guard identities).
#[derive(Debug, Clone, Copy, Default)]
pub struct BootAdoptReport {
    pub holdings: u32,
    pub buy_submitted: u32,
    pub episodes: u32,
    pub traded_identities: u32,
}

/// Rebuild in-memory `Entered` arms + registry meta from PG `Holding` rows (real
/// AND paper) so a process restart does not orphan live bags (Ops close / TP-SL)
/// or strand paper bags as forever-`Open`. **PG-only.**
pub async fn adopt_holdings(
    strategy_repo: &StrategyRepo,
    state: &mut EngineState,
    registry: &PositionRegistry,
) -> u32 {
    let mut n = 0u32;
    for mode in ["real", "paper"] {
        let holdings = match strategy_repo.find_all_holding(mode).await {
            Ok(h) => h,
            Err(e) => {
                warn!(mode, "engine: adopt Holding load failed: {e}");
                continue;
            }
        };
        for pos in &holdings {
            if super::orphan_exit::adopt_holding_into_engine(state, registry, pos).is_some() {
                n += 1;
            }
        }
    }
    if n > 0 {
        info!(adopted = n, "engine: adopted Holding positions into registry");
    }
    n
}

/// Adopt real PG `BuySubmitted` rows as inert `EntryPending` arms so the engine
/// treats the (rule, mint) slot as occupied and a fresh trigger cannot double-buy
/// it; the reaper folds each to `Holding` (fill indexed) or terminal (all sigs
/// reverted). Real-only — paper carries no on-chain double-spend risk. **PG-only.**
pub async fn adopt_buy_submitted(
    strategy_repo: &StrategyRepo,
    state: &mut EngineState,
    registry: &PositionRegistry,
) -> u32 {
    let submitted = match strategy_repo.find_all_buy_submitted("real").await {
        Ok(s) => s,
        Err(e) => {
            warn!("engine: adopt BuySubmitted load failed: {e}");
            return 0;
        }
    };
    let mut n = 0u32;
    for pos in &submitted {
        if super::orphan_exit::adopt_buy_submitted_into_engine(state, registry, pos).is_some() {
            n += 1;
        }
    }
    if n > 0 {
        info!(adopted = n, "engine: adopted BuySubmitted positions into registry (dedup)");
    }
    n
}

/// Seed the in-RAM re-entry episode counters (`TokenState::episodes`) from PG:
/// COUNT of terminal positions per (rule, mint), batched over every tracked mint
/// whose rule has re-entry configured.
pub async fn seed_episodes(strategy_repo: &StrategyRepo, state: &mut EngineState) -> u32 {
    let mints: Vec<String> = state
        .tokens
        .iter()
        .filter(|(_, t)| {
            t.arms
                .keys()
                .any(|r| state.rules.get(r).is_some_and(|c| c.reentry.is_some()))
        })
        .map(|(m, _)| m.to_string())
        .collect();
    if mints.is_empty() {
        return 0;
    }
    let counts = match strategy_repo.count_closed_by_rule_mint(&mints).await {
        Ok(c) => c,
        Err(e) => {
            warn!("engine: episode-seed load failed: {e}");
            return 0;
        }
    };
    let mut n = 0u32;
    for (rule_uuid, mint_s, count) in counts {
        let rule = RuleId(rule_uuid);
        if state.rules.get(&rule).is_none_or(|c| c.reentry.is_none()) {
            continue;
        }
        let mint = Mint::from(mint_s.as_str());
        if let Some(token) = state.tokens.get_mut(&mint) {
            token.episodes.insert(rule, count.max(0) as u32);
            n += 1;
            // Adoption mutates a tracked token outside the fold — tell the engine,
            // or a settled verdict predating it could survive.
            state.touch_token(&mint);
        }
    }
    if n > 0 {
        info!(seeded = n, "engine: seeded re-entry episode counters");
    }
    n
}

/// Rebuild the copycat guard's memory from PG.
///
/// The guard's state is **not** in-RAM-only like the arms: it is a projection of
/// `strategy_positions ⋈ tokens`, so a restart restores it exactly instead of
/// silently forgetting every identity the bot already traded — which is precisely
/// when the protection matters most (the box restarts far more often than a
/// copycat campaign lasts).
///
/// Two floors, whichever is later:
/// * `now − window` — the guard only ever blocks inside its window, so older rows
///   are dead weight, and this keeps the boot scan small on the 2vCPU box.
/// * `duplicate_identity_since` — the instant the operator first switched the
///   guard on. Enabling it starts from an *empty* memory rather than retroactively
///   blocking a week of history nobody opted into. `None` (never enabled) seeds
///   nothing.
///
/// Paper and real are seeded into their own memories.
pub async fn seed_dupe_guard(
    strategy_repo: &StrategyRepo,
    state: &mut EngineState,
    settings: &AppSettings,
) -> u32 {
    if !settings.skip_duplicate_identity {
        return 0;
    }
    let window_hours = if settings.duplicate_identity_window_hours == 0 {
        hunter_engine::dupe_guard::DEFAULT_WINDOW_HOURS
    } else {
        settings.duplicate_identity_window_hours
    };
    let window_floor = Utc::now() - Duration::hours(window_hours as i64);
    let Some(since) = settings.duplicate_identity_since_ts() else {
        info!("engine: copycat guard on but never stamped — starting from an empty memory");
        return 0;
    };
    let floor = window_floor.max(since);

    let mut n = 0u32;
    for (mode_str, mode) in [("real", TradeMode::Real), ("paper", TradeMode::Paper)] {
        let rows = match strategy_repo.find_traded_identities(mode_str, floor).await {
            Ok(r) => r,
            Err(e) => {
                warn!(mode = mode_str, "engine: copycat guard rebuild failed: {e}");
                continue;
            }
        };
        for (mint, name, symbol, at) in rows {
            // Hashed here, through the ONE engine hasher — never in SQL, or a
            // seeded identity could drift from a live one.
            let identity = token_identity_hash(&name, &symbol);
            if identity.is_none() {
                continue;
            }
            state.seed_traded_identity(mode, identity, &Mint::from(mint.as_str()), at);
            n += 1;
        }
    }
    if n > 0 {
        info!(
            seeded = n,
            since = %floor,
            "engine: rebuilt copycat-guard memory from PG"
        );
    }
    n
}

/// Run all PG adopt passes (holdings, buy-submitted dedup, episode counters,
/// copycat-guard memory).
pub async fn adopt_from_db(
    strategy_repo: &StrategyRepo,
    state: &mut EngineState,
    registry: &PositionRegistry,
    settings: &AppSettings,
) -> BootAdoptReport {
    BootAdoptReport {
        holdings: adopt_holdings(strategy_repo, state, registry).await,
        buy_submitted: adopt_buy_submitted(strategy_repo, state, registry).await,
        episodes: seed_episodes(strategy_repo, state).await,
        traded_identities: seed_dupe_guard(strategy_repo, state, settings).await,
    }
}
