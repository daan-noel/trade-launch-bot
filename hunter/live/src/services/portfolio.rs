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
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::sync::Mutex;
use tracing::warn;

use trading_core::models::portfolio::{unrealized_pnl, ManagedMint};
use trading_core::models::{cash_symbol, AssetKind, StrategyPosition};
use trading_core::storage::token_enrichment::{fetch_by_mints, TokenEnrichment, TokenEnrichmentRow};

use crate::services::clients::jupiter;
use crate::state::deploy_state::DeployState;
use crate::trader::WalletHolding;

/// One enriched wallet holding with cost basis, unrealized PnL, and the bot that
/// manages it (if any). The Holdings-page row and Home top-holdings widget both
/// read this. `is_migrated`/`is_cashback_enabled`/`symbol` are the live-authoritative
/// values (they overwrite any stale DB copy in the flattened `token`).
///
/// Cash (`asset_kind = cash`, e.g. USDC) is face-valued and carries no trading PnL.
#[derive(Debug, Serialize)]
pub struct PortfolioHolding {
    // Identity + on-chain balance (live — wins over any DB copy).
    pub mint_address: String,
    /// Raw token units (exact integer).
    pub amount: u64,
    pub ui_amount: f64,
    pub decimals: u8,
    pub token_account: String,
    pub token_program_id: String,
    pub symbol: Option<String>,
    /// Wallet classification — cash vs meme position (see [`AssetKind`]).
    pub asset_kind: AssetKind,
    // Live Jupiter marks (USD display side — same source as `value_usd`).
    // Cash overrides to face value ($1 / UI unit).
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
    /// wallet has no recorded buys for the mint (received/transferred bags),
    /// and always `None` for cash.
    pub cost_basis_sol: Option<f64>,
    pub unrealized_pnl_sol: Option<f64>,
    pub unrealized_pnl_pct: Option<f64>,
    /// The live (real) strategy managing this bag, if any — the manual-vs-bot
    /// double-sell guard. `None` for an unmanaged (orphan/manual) bag / cash.
    pub managed_by: Option<ManagedMint>,
    /// Full token enrichment (name, market_cap, current_price, …) — the same SSOT
    /// the strategy result tables flatten. `is_migrated`/`is_cashback_enabled` here
    /// carry the live-authoritative values.
    #[serde(flatten)]
    pub token: TokenEnrichment,
}

/// One cash balance line for the Holdings cash strip (always unfiltered).
#[derive(Debug, Clone, Serialize)]
pub struct CashHoldingSummary {
    pub mint_address: String,
    pub symbol: String,
    pub ui_amount: f64,
    pub value_usd: f64,
    pub value_sol: Option<f64>,
}

/// Wallet-wide roll-up backing the Home KPI row. Value totals include cash;
/// cost basis / unrealized PnL / `position_count` are **meme positions only**.
/// `realized_pnl_today_sol` / `active_rules` / `open_position_count` are
/// cross-strategy real-money aggregates.
#[derive(Debug, Serialize)]
pub struct PortfolioSummary {
    pub total_value_sol: f64,
    pub total_value_usd: f64,
    /// Face-value cash (USDC) in USD / SOL-equivalent.
    pub cash_value_usd: f64,
    pub cash_value_sol: f64,
    /// Meme-bag mark-to-market (excludes cash).
    pub positions_value_sol: f64,
    pub positions_value_usd: f64,
    pub total_cost_basis_sol: f64,
    pub total_unrealized_pnl_sol: f64,
    /// Number of held **meme** bags (excludes cash / WSOL plumbing).
    pub position_count: usize,
    /// Realized SOL PnL from real positions that cleanly exited since 00:00 UTC.
    pub realized_pnl_today_sol: f64,
    /// Active real-mode rules (eligible to fire live money).
    pub active_rules: usize,
    /// Open real strategy positions across all rules.
    pub open_position_count: usize,
}

/// How long a composed holdings scan stays warm. The composition is a full wallet
/// RPC scan + Jupiter batch + several DB reads, so a short TTL lets the server-side
/// Holdings table page/sort/filter over many POSTs from **one** scan instead of
/// re-scanning per request — honoring the hot-path "no new RPC" budget. A confirmed
/// trade (`?fresh=true` on the query endpoint) busts it so post-trade reads are fresh.
const HOLDINGS_TTL: Duration = Duration::from_secs(8);

/// Short-TTL cache of the composed holdings (see [`HOLDINGS_TTL`]). Held on
/// [`DeployState`]; there is exactly one bot wallet per process, so a single slot
/// suffices. `Default` = empty (cold).
#[derive(Default)]
pub struct HoldingsCache {
    inner: Mutex<Option<(Instant, Arc<Vec<PortfolioHolding>>)>>,
}

impl HoldingsCache {
    async fn get_fresh(&self) -> Option<Arc<Vec<PortfolioHolding>>> {
        match self.inner.lock().await.as_ref() {
            Some((at, v)) if at.elapsed() < HOLDINGS_TTL => Some(v.clone()),
            _ => None,
        }
    }

    async fn put(&self, v: Arc<Vec<PortfolioHolding>>) {
        *self.inner.lock().await = Some((Instant::now(), v));
    }

    /// Drop the cached scan so the next read re-scans (called after a confirmed trade).
    pub async fn invalidate(&self) {
        *self.inner.lock().await = None;
    }
}

/// Whole-population roll-up for the server-paged Holdings table's summary bar.
///
/// Cash is always taken from the full scan (unfiltered — the cash strip must not
/// vanish under dust/search). Position metrics (value/PnL/24h/count) are measured
/// over the **filtered** meme set so the bar agrees with the table. Values are
/// scan-time (the client price-poll overlays fresher display marks on the current
/// page only). Native SOL is the System-account balance (push-fed cache), not an
/// SPL row — shown separately from USDC cash.
#[derive(Debug, Default, Serialize)]
pub struct HoldingsTableSummary {
    /// Meme position count (filtered set; excludes cash).
    pub positions: usize,
    /// Native wallet SOL (System account lamports → SOL). `None` until the
    /// LaserStream push / safety poll has seeded the trader cache.
    pub sol_balance_sol: Option<f64>,
    /// Native SOL × latest SOL/USD (when both are known).
    pub sol_balance_usd: Option<f64>,
    /// Cash balances for the sticky cash strip (unfiltered).
    pub cash_holdings: Vec<CashHoldingSummary>,
    pub cash_value_usd: Option<f64>,
    pub cash_value_sol: Option<f64>,
    /// Meme-only mark-to-market over the filtered set.
    pub positions_value_sol: Option<f64>,
    pub positions_value_usd: Option<f64>,
    /// Cash + positions (when either side has a mark).
    pub total_value_sol: Option<f64>,
    pub total_value_usd: Option<f64>,
    pub total_cost_basis_sol: f64,
    pub total_unrealized_pnl_sol: Option<f64>,
    /// Value-weighted 24h across **meme** rows that have both a value and a 24h.
    pub change_24h_pct: Option<f64>,
}

/// Native wallet SOL from the push-fed trader cache (no RPC). Returns
/// `(sol, usd)` — USD is `None` when the SOL/USD poller has not landed yet.
pub fn native_sol_balance(state: &DeployState) -> (Option<f64>, Option<f64>) {
    let Some(lamports) = state.trader.cached_balance_lamports() else {
        return (None, None);
    };
    // Wallet balances fit in i64; clamp rather than panic on a corrupt cache.
    let sol = trading_core::config::constants::lamports_to_sol(lamports.min(i64::MAX as u64) as i64);
    let usd = state.latest_sol_price().map(|rate| sol * rate);
    (Some(sol), usd)
}

/// Split composed holdings into cash vs trading positions.
pub fn partition_cash(
    holdings: &[PortfolioHolding],
) -> (Vec<&PortfolioHolding>, Vec<&PortfolioHolding>) {
    let mut cash = Vec::new();
    let mut positions = Vec::new();
    for h in holdings {
        if h.asset_kind == AssetKind::Cash {
            cash.push(h);
        } else {
            positions.push(h);
        }
    }
    (cash, positions)
}

/// Build the cash strip lines + roll-up from cash holdings.
pub fn cash_summary(cash: &[&PortfolioHolding]) -> (Vec<CashHoldingSummary>, f64, Option<f64>) {
    let mut lines = Vec::with_capacity(cash.len());
    let mut usd = 0.0;
    let mut sol = 0.0;
    let mut has_sol = false;
    for h in cash {
        let value_usd = h.value_usd.unwrap_or(h.ui_amount);
        usd += value_usd;
        if let Some(v) = h.value_sol {
            sol += v;
            has_sol = true;
        }
        lines.push(CashHoldingSummary {
            mint_address: h.mint_address.clone(),
            symbol: h
                .symbol
                .clone()
                .or_else(|| cash_symbol(&h.mint_address).map(str::to_string))
                .unwrap_or_else(|| "CASH".to_string()),
            ui_amount: h.ui_amount,
            value_usd,
            value_sol: h.value_sol,
        });
    }
    (lines, usd, has_sol.then_some(sol))
}

/// Enriched holdings + cost basis + unrealized PnL + bot tag for the bot wallet —
/// **one live scan**, uncached. Prefer [`list_holdings_cached`] on read paths.
pub async fn list_holdings(state: &DeployState) -> anyhow::Result<Vec<PortfolioHolding>> {
    let holdings = state.trader.get_all_token_accounts().await?;
    compose(state, holdings).await
}

/// Cache-first holdings: reuse a warm scan (< [`HOLDINGS_TTL`]) or run one and warm
/// the cache. Shared by the Home widgets (full list), the server-paged Holdings
/// table, and the summary — so paging/sorting a table costs at most one scan per TTL
/// window. Pass `force = true` (post-trade) to bypass + refresh the cache.
pub async fn list_holdings_cached(
    state: &DeployState,
    force: bool,
) -> anyhow::Result<Arc<Vec<PortfolioHolding>>> {
    if force {
        state.holdings_cache.invalidate().await;
    } else if let Some(v) = state.holdings_cache.get_fresh().await {
        return Ok(v);
    }
    let fresh = Arc::new(list_holdings(state).await?);
    state.holdings_cache.put(fresh.clone()).await;
    Ok(fresh)
}

/// Wallet-wide summary (Home KPIs). Reuses [`list_holdings`] for the value/PnL
/// totals, then layers the real-money strategy aggregates on top.
pub async fn summary(state: &DeployState) -> anyhow::Result<PortfolioSummary> {
    let holdings = list_holdings_cached(state, false).await?;
    let (cash, positions) = partition_cash(&holdings);

    let mut cash_value_usd = 0.0;
    let mut cash_value_sol = 0.0;
    for h in &cash {
        cash_value_usd += h.value_usd.unwrap_or(h.ui_amount);
        cash_value_sol += h.value_sol.unwrap_or(0.0);
    }

    let mut positions_value_sol = 0.0;
    let mut positions_value_usd = 0.0;
    let mut total_cost_basis_sol = 0.0;
    let mut total_unrealized_pnl_sol = 0.0;
    for h in &positions {
        positions_value_sol += h.value_sol.unwrap_or(0.0);
        positions_value_usd += h.value_usd.unwrap_or(0.0);
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

    // Active real rules live in the generic `strategy_rules` table (RuleRepo).
    let active_rules = state
        .rule_repo
        .list_active()
        .await?
        .into_iter()
        .filter(|r| r.trade_mode == "real")
        .count();

    let open_position_count = state.strategy_repo().managed_mints(true).await?.len();

    Ok(PortfolioSummary {
        total_value_sol: cash_value_sol + positions_value_sol,
        total_value_usd: cash_value_usd + positions_value_usd,
        cash_value_usd,
        cash_value_sol,
        positions_value_sol,
        positions_value_usd,
        total_cost_basis_sol,
        total_unrealized_pnl_sol,
        position_count: positions.len(),
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

/// Per-bag cost basis + unrealized PnL. **The only place the service turns a mark
/// into PnL** — the arithmetic is delegated to
/// [`trading_core::models::portfolio::unrealized_pnl`] (the SSOT compute site);
/// this helper only lifts the SOL/raw average entry into UI space to match the
/// per-UI-token mark. `avg_entry_price` is SOL per raw unit (`AvgEntry`);
/// `mark_sol_per_ui` is SOL per UI token (Jupiter mark ÷ SOL/USD). Cost basis needs
/// only the average entry; the mark-to-market fields need a mark too.
struct HoldingPnl {
    cost_basis_sol: Option<f64>,
    unrealized_pnl_sol: Option<f64>,
    unrealized_pnl_pct: Option<f64>,
}

fn holding_pnl(
    avg_entry_price: Option<f64>,
    mark_sol_per_ui: Option<f64>,
    decimals: u8,
    ui_amount: f64,
) -> HoldingPnl {
    // SOL/raw → SOL/ui so it shares a unit basis with the per-UI-token mark.
    let avg_entry_per_ui = avg_entry_price.map(|a| a * 10f64.powi(decimals as i32));
    match (avg_entry_per_ui, mark_sol_per_ui) {
        (Some(entry_ui), Some(mark)) => {
            let p = unrealized_pnl(entry_ui, mark, ui_amount);
            HoldingPnl {
                cost_basis_sol: Some(p.cost_basis_sol),
                unrealized_pnl_sol: Some(p.unrealized_pnl_sol),
                unrealized_pnl_pct: Some(p.unrealized_pnl_pct),
            }
        }
        // No live mark: we can still show what was paid, but not the PnL.
        _ => HoldingPnl {
            cost_basis_sol: avg_entry_per_ui.map(|e| e * ui_amount),
            unrealized_pnl_sol: None,
            unrealized_pnl_pct: None,
        },
    }
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
            match acc.get(&m.mint_address) {
                Some(existing) if status_rank(&existing.status) >= status_rank(&m.status) => {}
                _ => {
                    acc.insert(m.mint_address.clone(), m);
                }
            }
            acc
        });

    Ok(holdings
        .into_iter()
        .map(|h| {
            let kind = trading_core::models::asset_kind(&h.mint);
            let cached = state.token_cache.get(&h.mint);
            let entry = jupiter.get(&h.mint);
            let enrich_row = enrich_by_mint.get(&h.mint);

            // Cash = face $1 / UI unit (no Jupiter flicker, no trading PnL).
            // Meme bags use Jupiter marks + avg-entry unrealized PnL.
            let (price_usd, value_usd, liquidity, price_change_24h, token_created_at, pnl) =
                if kind == AssetKind::Cash {
                    let price_usd = Some(1.0);
                    let value_usd = Some(h.ui_amount);
                    (
                        price_usd,
                        value_usd,
                        None,
                        None,
                        None,
                        HoldingPnl {
                            cost_basis_sol: None,
                            unrealized_pnl_sol: None,
                            unrealized_pnl_pct: None,
                        },
                    )
                } else {
                    let price_usd = entry.and_then(|e| e.price_usd);
                    let value_usd = price_usd.map(|p| p * h.ui_amount);
                    let mark_sol_per_ui = match (price_usd, sol_usd) {
                        (Some(pu), Some(su)) if su > 0.0 => Some(pu / su),
                        _ => None,
                    };
                    let pnl = holding_pnl(
                        avg_entries.get(&h.mint).map(|a| a.avg_entry_price),
                        mark_sol_per_ui,
                        h.decimals,
                        h.ui_amount,
                    );
                    (
                        price_usd,
                        value_usd,
                        entry.and_then(|e| e.liquidity),
                        entry.and_then(|e| e.price_change_24h),
                        entry.and_then(|e| e.token_created_at.clone()),
                        pnl,
                    )
                };

            let value_sol = match (value_usd, sol_usd) {
                (Some(vu), Some(su)) if su > 0.0 => Some(vu / su),
                _ => None,
            };

            // Live-authoritative migration/cashback (cache → on-chain fallback).
            // Cash has no curve/AMM migration story — force false.
            let (is_migrated, is_cashback_enabled) = if kind == AssetKind::Cash {
                (false, false)
            } else {
                (
                    cached
                        .as_ref()
                        .map(|s| s.is_migrated)
                        .or_else(|| chain_facts.get(&h.mint).map(|f| f.is_migrated))
                        .unwrap_or(false),
                    cached
                        .as_ref()
                        .map(|s| s.token.is_cashback_enabled)
                        .or_else(|| chain_facts.get(&h.mint).map(|f| f.cashback_enabled))
                        .unwrap_or(false),
                )
            };
            let symbol = if kind == AssetKind::Cash {
                Some(cash_symbol(&h.mint).unwrap_or("USDC").to_string())
            } else {
                cached
                    .as_ref()
                    .map(|s| s.token.symbol.clone())
                    .or_else(|| enrich_row.map(|r| r.symbol.clone()))
            };

            let mut token = enrich_row.map(TokenEnrichment::from).unwrap_or_default();
            token.is_migrated = is_migrated;
            token.is_cashback_enabled = is_cashback_enabled;

            // Cash is never strategy-managed; resolve bot badge for meme bags only.
            let managed_by = if kind == AssetKind::Cash {
                None
            } else {
                managed_by_mint.get(&h.mint).cloned()
            };

            PortfolioHolding {
                mint_address: h.mint,
                amount: h.amount,
                ui_amount: h.ui_amount,
                decimals: h.decimals,
                token_account: h.token_account,
                token_program_id: h.token_program_id,
                symbol,
                asset_kind: kind,
                price_usd,
                value_usd,
                liquidity,
                price_change_24h,
                token_created_at,
                value_sol,
                cost_basis_sol: pnl.cost_basis_sol,
                unrealized_pnl_sol: pnl.unrealized_pnl_sol,
                unrealized_pnl_pct: pnl.unrealized_pnl_pct,
                managed_by,
                token,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SSOT guard (Phase 1.6): the service's PnL composition is the SSOT
    /// `unrealized_pnl` fed with a UI-space average entry — never a re-implemented
    /// formula. Fixture: avg entry 1e-9 SOL/raw at 6 decimals = 1e-3 SOL/UI token;
    /// mark 2e-3 SOL/UI; 1000 UI held ⇒ cost 1.0 SOL, +1.0 SOL, +100%.
    #[test]
    fn holding_pnl_matches_known_fixture() {
        let p = holding_pnl(Some(1e-9), Some(2e-3), 6, 1000.0);
        assert!((p.cost_basis_sol.unwrap() - 1.0).abs() < 1e-12);
        assert!((p.unrealized_pnl_sol.unwrap() - 1.0).abs() < 1e-12);
        assert!((p.unrealized_pnl_pct.unwrap() - 100.0).abs() < 1e-9);
    }

    /// No live mark (untracked mint / Jupiter miss): cost basis still resolves from
    /// the recorded buys, but PnL cannot be marked.
    #[test]
    fn no_mark_yields_cost_basis_but_no_pnl() {
        let p = holding_pnl(Some(1e-9), None, 6, 1000.0);
        assert!((p.cost_basis_sol.unwrap() - 1.0).abs() < 1e-12);
        assert!(p.unrealized_pnl_sol.is_none());
        assert!(p.unrealized_pnl_pct.is_none());
    }

    /// A received/transferred bag with no recorded buys has no cost basis and no PnL.
    #[test]
    fn no_recorded_buys_has_no_basis() {
        let p = holding_pnl(None, Some(2e-3), 6, 1000.0);
        assert!(p.cost_basis_sol.is_none());
        assert!(p.unrealized_pnl_sol.is_none());
        assert!(p.unrealized_pnl_pct.is_none());
    }

    /// `status_rank` picks the sharpest double-sell risk: an in-flight exit
    /// outranks a plain hold, which outranks a buy-in-flight / arming.
    #[test]
    fn status_rank_orders_by_double_sell_risk() {
        assert!(status_rank("ExitPending") > status_rank("Holding"));
        assert!(status_rank("Holding") > status_rank("BuySubmitted"));
        assert!(status_rank("BuySubmitted") > status_rank("Arming"));
    }

    fn stub_holding(mint: &str, kind: AssetKind, ui: f64, value_usd: f64) -> PortfolioHolding {
        PortfolioHolding {
            mint_address: mint.to_string(),
            amount: (ui * 1_000_000.0) as u64,
            ui_amount: ui,
            decimals: 6,
            token_account: format!("ata-{mint}"),
            token_program_id: "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA".into(),
            symbol: Some(if kind == AssetKind::Cash { "USDC".into() } else { "MEME".into() }),
            asset_kind: kind,
            price_usd: Some(if kind == AssetKind::Cash { 1.0 } else { value_usd / ui }),
            value_usd: Some(value_usd),
            liquidity: None,
            price_change_24h: if kind == AssetKind::Cash { None } else { Some(5.0) },
            token_created_at: None,
            value_sol: Some(value_usd / 100.0),
            cost_basis_sol: if kind == AssetKind::Cash { None } else { Some(1.0) },
            unrealized_pnl_sol: if kind == AssetKind::Cash { None } else { Some(0.5) },
            unrealized_pnl_pct: if kind == AssetKind::Cash { None } else { Some(50.0) },
            managed_by: None,
            token: TokenEnrichment::default(),
        }
    }

    #[test]
    fn partition_cash_splits_usdc_from_meme() {
        let usdc = trading_core::config::constants::USDC_MINT;
        let rows = vec![
            stub_holding(usdc, AssetKind::Cash, 100.0, 100.0),
            stub_holding("MemeMint", AssetKind::Meme, 10.0, 50.0),
        ];
        let (cash, positions) = partition_cash(&rows);
        assert_eq!(cash.len(), 1);
        assert_eq!(cash[0].mint_address, usdc);
        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0].mint_address, "MemeMint");
    }

    #[test]
    fn cash_summary_face_values_roll_up() {
        let usdc = trading_core::config::constants::USDC_MINT;
        let rows = vec![stub_holding(usdc, AssetKind::Cash, 250.0, 250.0)];
        let refs: Vec<&PortfolioHolding> = rows.iter().collect();
        let (lines, usd, sol) = cash_summary(&refs);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].symbol, "USDC");
        assert!((usd - 250.0).abs() < 1e-9);
        assert!((sol.unwrap() - 2.5).abs() < 1e-9);
    }
}
