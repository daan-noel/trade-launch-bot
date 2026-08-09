//! Volume-making bots (docs/arch/launcher.md) — the subsystem's second
//! automation, and its first autonomous *buy* loop.
//!
//! A bot cycles a jittered buy (+ an optional sell-back) across a rotating wallet
//! selection on a jittered interval, generating trade volume on one of our launched
//! tokens. Each leg runs through the **Phase 2/3 buy + sell pipelines**
//! (`execute_action` — audit row + confirm), so there is NO second trade engine:
//! this is purely a scheduler over the existing primitives + the wallet pool.
//!
//! Gated by the same `MANAGE_ENABLED` kill switch as the ladder evaluator: the
//! scheduler no-ops while management is off, so a `running` bot placed ahead of
//! time never silently trades. Every bot carries a hard SOL budget (and optional
//! max-cycle cap); it stops itself the moment either is reached.

use std::time::Duration;

use anyhow::{bail, Result};
use chrono::{Duration as ChronoDuration, Utc};
use platform_core::models::{ManagedWallet, VolumeBot};
use platform_core::storage::repositories::{ManagedWalletRepo, VolumeBotRepo};
use pump_trader::protocol::LAMPORTS_PER_SOL;
use rand::Rng;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::{info, warn};

use super::execute::execute_action;
use super::model::{ManageRequest, PlanLeg, WalletSelection};
use crate::config::LauncherSettings;

/// How often the scheduler wakes to look for due bots. Bots pace themselves via
/// their own jittered per-cycle interval (`next_run_at`); this is just the poll
/// granularity, kept coarse so an idle box does almost no work.
const VOLUME_SCHED_INTERVAL: Duration = Duration::from_secs(5);

/// Default wallet role a bot rotates across when its selection names no role/ids —
/// the dedicated volume/trading pool, kept funded by the existing funder.
const DEFAULT_VOLUME_ROLE: &str = "trading";

/// A bot's tunables (stored as the `volume_bots.config` JSONB). Buy sizes are human
/// SOL (`_sol`, f64) — matching the buy primitive's `fixed_sol` input; the budget
/// is likewise SOL. Intervals are seconds. Every cycle jitters the buy size and the
/// wait uniformly within its `[min, max]` band so the cadence never looks metronomic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeConfig {
    /// Per-cycle buy spend band (SOL), jittered uniformly.
    pub buy_sol_min: f64,
    pub buy_sol_max: f64,
    /// Inter-cycle wait band (seconds), jittered uniformly.
    pub interval_secs_min: u64,
    pub interval_secs_max: u64,
    /// Percent of the just-bought balance to sell back each cycle (0–100). `0`
    /// disables the sell-back — the bot only accumulates (buy-only volume).
    pub sell_back_pct: f64,
    /// Hard cumulative buy-spend cap (SOL). The bot stops once `spent_quote`
    /// reaches this — the treasury-drain guard, mirroring `FundingConfig`'s caps.
    pub budget_sol: f64,
    /// Optional cap on the number of cycles; `None` = run until the budget is hit.
    #[serde(default)]
    pub max_cycles: Option<i32>,
}

impl VolumeConfig {
    /// Validate operator input up front so a nonsensical bot fails at start time,
    /// not silently mid-run.
    fn validate(&self) -> Result<()> {
        if !(self.buy_sol_min > 0.0) || self.buy_sol_max < self.buy_sol_min {
            bail!("buy_sol range must satisfy 0 < min <= max");
        }
        if self.interval_secs_min == 0 || self.interval_secs_max < self.interval_secs_min {
            bail!("interval_secs range must satisfy 0 < min <= max");
        }
        if !(0.0..=100.0).contains(&self.sell_back_pct) {
            bail!("sell_back_pct must be in [0, 100]");
        }
        if self.budget_sol < self.buy_sol_max {
            bail!("budget_sol must be at least buy_sol_max (one cycle's worth)");
        }
        if matches!(self.max_cycles, Some(n) if n <= 0) {
            bail!("max_cycles must be positive when set");
        }
        Ok(())
    }

    fn budget_lamports(&self) -> i64 {
        (self.budget_sol * LAMPORTS_PER_SOL as f64).round() as i64
    }
}

/// Start a bot for `mint`. Validates the config, then inserts a `running` row (due
/// immediately). Placing a bot is always allowed — it won't trade until
/// `MANAGE_ENABLED` is on (the scheduler enforces that), mirroring ladder arming.
pub async fn start_volume_bot(
    pool: &PgPool,
    mint: &str,
    selection: WalletSelection,
    config: VolumeConfig,
) -> Result<VolumeBot> {
    config.validate()?;
    VolumeBotRepo::insert(
        pool,
        mint,
        serde_json::to_value(&selection)?,
        serde_json::to_value(&config)?,
    )
    .await
}

/// Spawn the volume scheduler as a long-lived task. Cheap when idle (bounded scan).
pub fn spawn_volume_scheduler(
    pool: PgPool,
    settings: LauncherSettings,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(VOLUME_SCHED_INTERVAL);
        loop {
            tick.tick().await;
            if let Err(e) = evaluate_once(&pool, &settings).await {
                warn!(?e, "volume scheduler pass failed");
            }
        }
    })
}

/// One scheduler pass: run a cycle for each due bot. No-op (bots stay `running`,
/// untouched) when management is disabled — so a bot armed ahead of time never
/// silently trades before the operator flips `MANAGE_ENABLED`.
async fn evaluate_once(pool: &PgPool, settings: &LauncherSettings) -> Result<()> {
    if settings.manage.is_none() {
        return Ok(()); // disabled — leave bots running-but-idle until turned on
    }

    for bot in VolumeBotRepo::list_due(pool).await? {
        let config: VolumeConfig = match serde_json::from_value(bot.config.clone()) {
            Ok(c) => c,
            Err(e) => {
                warn!(bot_id = %bot.id, %e, "volume config unparseable — stopping bot");
                VolumeBotRepo::stop(pool, bot.id, Some("config unparseable")).await?;
                continue;
            }
        };

        // Stop conditions BEFORE trading: max cycles, or budget exhausted (no room
        // for even the smallest cycle). Either terminates the bot cleanly.
        if matches!(config.max_cycles, Some(n) if bot.cycles_done >= n) {
            VolumeBotRepo::stop(pool, bot.id, Some("max cycles reached")).await?;
            info!(bot_id = %bot.id, mint = %bot.mint_address, "volume bot stopped: max cycles");
            continue;
        }
        let min_cycle_lamports = (config.buy_sol_min * LAMPORTS_PER_SOL as f64).round() as i64;
        if bot.spent_quote + min_cycle_lamports > config.budget_lamports() {
            VolumeBotRepo::stop(pool, bot.id, Some("budget exhausted")).await?;
            info!(bot_id = %bot.id, mint = %bot.mint_address, "volume bot stopped: budget exhausted");
            continue;
        }

        match run_cycle(pool, settings, &bot, &config).await {
            Ok(delta) => {
                let next_run_at = Utc::now() + jittered_interval(&config);
                VolumeBotRepo::record_cycle(
                    pool,
                    bot.id,
                    delta.spent_quote,
                    delta.volume_quote,
                    next_run_at,
                    None,
                )
                .await?;
            }
            Err(e) => {
                // A cycle failure (e.g. no funded wallet) is transient — record it
                // and retry next interval rather than stopping the bot. It still
                // counts as a cycle so a persistently failing bot backs off, not
                // spins; the budget guard above bounds total spend regardless.
                let next_run_at = Utc::now() + jittered_interval(&config);
                warn!(bot_id = %bot.id, mint = %bot.mint_address, %e, "volume cycle failed");
                VolumeBotRepo::record_cycle(
                    pool,
                    bot.id,
                    0,
                    0,
                    next_run_at,
                    Some(&e.to_string()),
                )
                .await?;
            }
        }
    }
    Ok(())
}

/// The spend + volume a single cycle actually generated (confirmed legs only).
struct CycleDelta {
    spent_quote: i64,
    volume_quote: i64,
}

/// One volume cycle: pick the next wallet in the rotation, buy a jittered amount
/// through it, then (optionally) sell a pct of the fresh balance back — both via
/// the Phase 2/3 `execute_action` pipeline. Spend/volume are counted from the
/// CONFIRMED legs only, so a failed buy costs nothing against the budget.
async fn run_cycle(
    pool: &PgPool,
    settings: &LauncherSettings,
    bot: &VolumeBot,
    config: &VolumeConfig,
) -> Result<CycleDelta> {
    // Rotate across the selection's wallets, one per cycle, so no single wallet
    // dominates the volume (the "fresh-wallet rotation" of the plan). Freshly
    // funded pool wallets enter the rotation automatically.
    let selection: WalletSelection =
        serde_json::from_value(bot.selection.clone()).unwrap_or_default();
    let candidates = resolve_candidates(pool, &selection).await?;
    if candidates.is_empty() {
        bail!("no candidate wallets for the bot's selection");
    }
    let wallet = &candidates[(bot.cycles_done as usize) % candidates.len()];
    let one = WalletSelection { role: None, wallet_ids: vec![wallet.id] };

    // Clamp this cycle's buy so cumulative spend never overshoots the budget.
    let remaining_lamports = (config.budget_lamports() - bot.spent_quote).max(0);
    let remaining_sol = remaining_lamports as f64 / LAMPORTS_PER_SOL as f64;
    let buy_sol = jittered_buy_sol(config).min(remaining_sol);

    // BUY leg.
    let buy = execute_action(
        pool,
        settings,
        &bot.mint_address,
        &ManageRequest {
            kind: "buy".to_string(),
            sizing: "fixed_sol".to_string(),
            size: buy_sol,
            selection: one.clone(),
            // Inert here — `one` names an explicit wallet id, so the resolver never
            // reaches the role/token-scope branch.
            token_scoped: true,
        },
        None, // automated volume-bot cycle — no live operator surface
    )
    .await?;
    let buy_spend = confirmed_quote(&buy.plan, "buy", |l| l.spend_quote);
    let mut delta = CycleDelta { spent_quote: buy_spend, volume_quote: buy_spend };

    // SELL-BACK leg (optional). Sizes off the balance the buy just added — the
    // sell's own pre-plan reconcile picks that up (getTokenAccountsByOwner is
    // confirmed, so the fresh balance is visible immediately).
    if config.sell_back_pct > 0.0 && buy.legs_confirmed > 0 {
        match execute_action(
            pool,
            settings,
            &bot.mint_address,
            &ManageRequest {
                kind: "sell".to_string(),
                sizing: "pct_of_holdings".to_string(),
                size: config.sell_back_pct,
                selection: one,
                token_scoped: true, // inert for sell (position-scoped) + explicit id
            },
            None, // automated volume-bot cycle — no live operator surface
        )
        .await
        {
            Ok(sell) => delta.volume_quote += confirmed_quote(&sell.plan, "sell", |l| l.est_quote),
            Err(e) => warn!(bot_id = %bot.id, %e, "volume sell-back leg failed (buy stands)"),
        }
    }

    Ok(delta)
}

/// Resolve the wallet set a bot rotates across: explicit `wallet_ids` win, else the
/// selection's `role`, else the default volume role. Never "all wallets".
async fn resolve_candidates(
    pool: &PgPool,
    selection: &WalletSelection,
) -> Result<Vec<ManagedWallet>> {
    if !selection.wallet_ids.is_empty() {
        return ManagedWalletRepo::get_many(pool, &selection.wallet_ids).await;
    }
    let role = selection.role.as_deref().unwrap_or(DEFAULT_VOLUME_ROLE);
    ManagedWalletRepo::by_role(pool, role).await
}

/// Sum a confirmed-leg quote field over an executed plan's legs of one side. `plan`
/// is the `ManageAction.plan` JSON (a `PlanLeg[]`); unparseable ⇒ 0.
fn confirmed_quote(
    plan: &serde_json::Value,
    side: &str,
    field: impl Fn(&PlanLeg) -> i64,
) -> i64 {
    let legs: Vec<PlanLeg> = serde_json::from_value(plan.clone()).unwrap_or_default();
    legs.iter()
        .filter(|l| l.side == side && l.status.as_deref() == Some("confirmed"))
        .map(field)
        .sum()
}

fn jittered_buy_sol(config: &VolumeConfig) -> f64 {
    if config.buy_sol_max <= config.buy_sol_min {
        return config.buy_sol_min;
    }
    rand::thread_rng().gen_range(config.buy_sol_min..=config.buy_sol_max)
}

fn jittered_interval(config: &VolumeConfig) -> ChronoDuration {
    let secs = if config.interval_secs_max <= config.interval_secs_min {
        config.interval_secs_min
    } else {
        rand::thread_rng().gen_range(config.interval_secs_min..=config.interval_secs_max)
    };
    ChronoDuration::seconds(secs as i64)
}
