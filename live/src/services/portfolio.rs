//! Portfolio/PnL read service — the composition SSOT the Holdings, Home, and
//! Live-Trading surfaces all read (Phase 1 keystone). Mirrors
//! [`super::wallet_tokens`]: composes on-chain holdings (`state.trader`) + live
//! Jupiter marks + cost basis ([`TradeRepo::avg_entry_by_wallet_and_mints`]) +
//! unrealized PnL ([`trading_core::models::portfolio::unrealized_pnl`], the single
//! compute site) + bot correlation ([`StrategyRepo::managed_mints`]) + token
//! enrichment ([`trading_core::storage::token_enrichment::fetch_by_mints`]).
//!
//! Bounded/cheap on the 4GB EC2 box: the held-mint set and the open-position set
//! are both tiny, so every join here is over a handful of rows.

use std::collections::HashMap;

use serde::Serialize;
use tracing::warn;

use trading_core::models::portfolio::{unrealized_pnl, ManagedMint};
use trading_core::models::StrategyPosition;
use trading_core::storage::token_enrichment::{fetch_by_mints, TokenEnrichment, TokenEnrichmentRow};

use crate::services::clients::jupiter;
use crate::state::deploy_state::DeployState;
use crate::trader::WalletHolding;

/// One enriched wallet holding with cost basis, unrealized PnL, and the bot that
/// manages it (if any). The Holdings-page row and Home top-holdings widget both
/// read this. `is_migrated`/`is_cashback_enabled`/`symbol` are the live-authoritative
/// values (they overwrite any stale DB copy in the flattened `token`).
#[derive(Debug, Serialize)]
pub struct PortfolioHolding {
    // Identity + on-chain balance (live — wins over any DB copy).
    pub mint: String,
    /// Raw token units (exact integer).
    pub amount: u64,
    pub ui_amount: f64,
    pub decimals: u8,
    pub token_account: String,
    pub token_program_id: String,
    pub symbol: Option<String>,
    // Live Jupiter marks (USD display side — same source as `value_usd`).
    pub price_usd: Option<f64>,
    pub value_usd: Option<f64>,
    pub liquidity: Option<f64>,
    pub price_change_24h: Option<f64>,
    pub token_created_at: Option<String>,
    // SOL valuation + PnL (SSOT: `trading_core::models::portfolio`).
    /// Mark-to-market SOL value of the bag (`mark_sol_per_ui × ui_amount`); `None`
    /// when no live SOL mark is available (missing Jupiter price or SOL/USD).
    pub value_sol: Option<f64>,
    /// Remaining bag's cost basis in SOL (`avg_entry × held`); `None` when the
    /// wallet has no recorded buys for the mint (received/transferred bags).
    pub cost_basis_sol: Option<f64>,
    pub unrealized_pnl_sol: Option<f64>,
    pub unrealized_pnl_pct: Option<f64>,
    /// The live (real) strategy managing this bag, if any — the manual-vs-bot
    /// double-sell guard. `None` for an unmanaged (orphan/manual) bag.
    pub managed_by: Option<ManagedMint>,
    /// Full token enrichment (name, market_cap, current_price, …) — the same SSOT
    /// the strategy result tables flatten. `is_migrated`/`is_cashback_enabled` here
    /// carry the live-authoritative values.
    #[serde(flatten)]
    pub token: TokenEnrichment,
}

/// Wallet-wide roll-up backing the Home KPI row. Totals sum the per-holding SOL
/// values; `realized_pnl_today_sol` / `active_rules` / `open_position_count` are
/// cross-strategy real-money aggregates.
#[derive(Debug, Serialize)]
pub struct PortfolioSummary {
    pub total_value_sol: f64,
    pub total_value_usd: f64,
    pub total_cost_basis_sol: f64,
    pub total_unrealized_pnl_sol: f64,
    /// Number of held bags (token accounts with a balance).
    pub position_count: usize,
    /// Realized SOL PnL from real positions that cleanly exited since 00:00 UTC.
    pub realized_pnl_today_sol: f64,
    /// Active real-mode rules (eligible to fire live money).
    pub active_rules: usize,
    /// Open real strategy positions across all rules.
    pub open_position_count: usize,
}

/// Enriched holdings + cost basis + unrealized PnL + bot tag for the bot wallet.
pub async fn list_holdings(state: &DeployState) -> anyhow::Result<Vec<PortfolioHolding>> {
    let holdings = state.trader.get_all_token_accounts().await?;
    compose(state, holdings).await
}

/// Wallet-wide summary (Home KPIs). Reuses [`list_holdings`] for the value/PnL
/// totals, then layers the real-money strategy aggregates on top.
pub async fn summary(state: &DeployState) -> anyhow::Result<PortfolioSummary> {
    let holdings = list_holdings(state).await?;

    let mut total_value_sol = 0.0;
    let mut total_value_usd = 0.0;
    let mut total_cost_basis_sol = 0.0;
    let mut total_unrealized_pnl_sol = 0.0;
    for h in &holdings {
        total_value_sol += h.value_sol.unwrap_or(0.0);
        total_value_usd += h.value_usd.unwrap_or(0.0);
        total_cost_basis_sol += h.cost_basis_sol.unwrap_or(0.0);
        total_unrealized_pnl_sol += h.unrealized_pnl_sol.unwrap_or(0.0);
    }

    // Start of the current UTC day — the "realized today" window boundary.
    let start_of_day = chrono::Utc::now()
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .expect("00:00:00 is a valid time")
        .and_utc();
    let realized_pnl_today_sol = trading_core::config::constants::lamports_to_sol(
        state.strategy_repo().realized_pnl_lamports_since(start_of_day).await?,
    );

    let active_rules = state
        .strategy_repo()
        .find_active_rules()
        .await?
        .into_iter()
        .filter(|r| r.trade_mode == "real")
        .count();

    let open_position_count = state.strategy_repo().managed_mints(true).await?.len();

    Ok(PortfolioSummary {
        total_value_sol,
        total_value_usd,
        total_cost_basis_sol,
        total_unrealized_pnl_sol,
        position_count: holdings.len(),
        realized_pnl_today_sol,
        active_rules,
        open_position_count,
    })
}

/// All open **strategy** positions across every rule (Live-Trading roll-up).
/// `real_only` drops paper positions (Live Trading monitors real money).
pub async fn open_positions(
    state: &DeployState,
    real_only: bool,
) -> anyhow::Result<Vec<StrategyPosition>> {
    let mut positions = state.strategy_repo().find_open_positions().await?;
    if real_only {
        positions.retain(|p| p.mode == "real");
    }
    Ok(positions)
}

/// Rank open statuses by how much a manual sell would conflict with the bot's own
/// action — `ExitPending` (sell in flight) is the sharpest double-sell risk.
fn status_rank(status: &str) -> u8 {
    match status {
        "ExitPending" => 3,
        "Holding" => 2,
        "BuySubmitted" => 1,
        _ => 0, // Arming
    }
}

/// The shared composition: enrich each holding with marks, cost basis, PnL, bot
/// tag, and full token enrichment. Split out so `list_holdings` (full scan) and a
/// future single-mint post-trade patch can share one code path.
async fn compose(
    state: &DeployState,
    holdings: Vec<WalletHolding>,
) -> anyhow::Result<Vec<PortfolioHolding>> {
    let mints: Vec<String> = holdings.iter().map(|h| h.mint.clone()).collect();
    let wallet = state.trader.wallet_pubkey();
    let sol_usd = state.latest_sol_price();

    // Cache-first migration/cashback fallback for mints the live stream never saw
    // (mirrors `wallet_tokens::enrich_holdings`).
    let uncached: Vec<String> = mints
        .iter()
        .filter(|m| !state.token_cache.contains_key(m.as_str()))
        .cloned()
        .collect();

    // Fire the independent network/DB reads together: Jupiter marks, on-chain
    // curve facts (uncached only), DB cost basis, DB enrichment, and the
    // real-position bot correlation. The repo handles are bound first so they
    // outlive the borrows their futures hold inside the `join!`.
    let trade_repo = state.trade_repo();
    let strategy_repo = state.strategy_repo();
    let (jupiter, chain_facts, avg_entries, enrich_rows, managed) = tokio::join!(
        jupiter::fetch_prices(&mints),
        state.trader.resolve_curve_facts_batch(&uncached),
        trade_repo.avg_entry_by_wallet_and_mints(&wallet, &mints),
        fetch_by_mints(&state.db, &mints),
        strategy_repo.managed_mints(true),
    );
    let jupiter = jupiter.unwrap_or_else(|e| {
        warn!("Jupiter price fetch failed: {e}");
        Default::default()
    });
    let avg_entries = avg_entries.unwrap_or_else(|e| {
        warn!("cost-basis fetch failed: {e}");
        Default::default()
    });
    let enrich_by_mint: HashMap<String, TokenEnrichmentRow> = enrich_rows
        .unwrap_or_else(|e| {
            warn!("token enrichment fetch failed: {e}");
            Vec::new()
        })
        .into_iter()
        .map(|r| (r.mint_address.clone(), r))
        .collect();
    // Reduce the open real positions to one badge per mint (sharpest status wins).
    let managed_by_mint: HashMap<String, ManagedMint> = managed
        .unwrap_or_else(|e| {
            warn!("managed-mints fetch failed: {e}");
            Vec::new()
        })
        .into_iter()
        .fold(HashMap::new(), |mut acc, m| {
            match acc.get(&m.mint) {
                Some(existing) if status_rank(&existing.status) >= status_rank(&m.status) => {}
                _ => {
                    acc.insert(m.mint.clone(), m);
                }
            }
            acc
        });

    Ok(holdings
        .into_iter()
        .map(|h| {
            let cached = state.token_cache.get(&h.mint);
            let entry = jupiter.get(&h.mint);
            let enrich_row = enrich_by_mint.get(&h.mint);

            let price_usd = entry.and_then(|e| e.price_usd);
            let value_usd = price_usd.map(|p| p * h.ui_amount);

            // Live-authoritative migration/cashback (cache → on-chain fallback).
            let is_migrated = cached
                .as_ref()
                .map(|s| s.is_migrated)
                .or_else(|| chain_facts.get(&h.mint).map(|f| f.is_migrated))
                .unwrap_or(false);
            let is_cashback_enabled = cached
                .as_ref()
                .map(|s| s.token.is_cashback_enabled)
                .or_else(|| chain_facts.get(&h.mint).map(|f| f.cashback_enabled))
                .unwrap_or(false);
            let symbol = cached
                .as_ref()
                .map(|s| s.token.symbol.clone())
                .or_else(|| enrich_row.map(|r| r.symbol.clone()));

            // SOL mark from the same Jupiter source as `value_usd` (per UI token),
            // converted through the live SOL/USD. `None` when either is missing.
            let mark_sol_per_ui = match (price_usd, sol_usd) {
                (Some(pu), Some(su)) if su > 0.0 => Some(pu / su),
                _ => None,
            };
            let value_sol = mark_sol_per_ui.map(|m| m * h.ui_amount);

            // Cost basis + unrealized PnL in UI space (avg entry is SOL/raw → SOL/ui
            // via the decimals factor), so the SOL outputs come out in human SOL.
            let decimals_factor = 10f64.powi(h.decimals as i32);
            let avg_entry_per_ui = avg_entries
                .get(&h.mint)
                .map(|a| a.avg_entry_price * decimals_factor);
            let cost_basis_sol = avg_entry_per_ui.map(|e| e * h.ui_amount);
            let pnl = match (avg_entry_per_ui, mark_sol_per_ui) {
                (Some(entry_ui), Some(mark)) => Some(unrealized_pnl(entry_ui, mark, h.ui_amount)),
                _ => None,
            };

            let mut token = enrich_row.map(TokenEnrichment::from).unwrap_or_default();
            token.is_migrated = is_migrated;
            token.is_cashback_enabled = is_cashback_enabled;

            // Resolve the bot badge before `h.mint` is moved into the struct.
            let managed_by = managed_by_mint.get(&h.mint).cloned();

            PortfolioHolding {
                mint: h.mint,
                amount: h.amount,
                ui_amount: h.ui_amount,
                decimals: h.decimals,
                token_account: h.token_account,
                token_program_id: h.token_program_id,
                symbol,
                price_usd,
                value_usd,
                liquidity: entry.and_then(|e| e.liquidity),
                price_change_24h: entry.and_then(|e| e.price_change_24h),
                token_created_at: entry.and_then(|e| e.token_created_at.clone()),
                value_sol,
                cost_basis_sol,
                unrealized_pnl_sol: pnl.map(|p| p.unrealized_pnl_sol),
                unrealized_pnl_pct: pnl.map(|p| p.unrealized_pnl_pct),
                managed_by,
                token,
            }
        })
        .collect())
}
