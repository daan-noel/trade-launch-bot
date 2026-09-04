# Portfolio/PnL service — cost-basis & PnL SSOT (Phase 1)

Reference for the keystone `/api/portfolio/*` backend built in `dashboard-pages-plan.md`
Phase 1. The Holdings, Home, and Live-Trading surfaces all read this one service, so
cost basis, PnL, and "who manages this mint" are each computed in exactly one place.

## Compute sites (SSOT)

| Fact | The one place it lives | Notes |
| --- | --- | --- |
| Manual-buy **cost basis** | `TradeRepo::avg_entry_by_wallet_and_mints` → `AvgEntry` | `SUM(amount_lamports)/SUM(token_amount)` over `trade_type='buy'` per mint. `avg_entry_price` is **SOL per raw token unit** — same convention as `StrategyPosition::entry_price` / `SigLegs::price_per_token`, so a manual bag and a bot bag price identically. The wallet's bot buys are on-chain trades too, so this blends manual + bot buys into one "what did I pay" number. |
| **Unrealized PnL** | `trading_core::models::portfolio::unrealized_pnl` | Pure: `(avg_entry, mark, held)` → `{cost_basis_sol, unrealized_pnl_sol, unrealized_pnl_pct}`. Unit-agnostic (all three share a basis). Mirrors `Position::pnl_sol` (price × amount = human SOL). **JS never re-derives PnL.** |
| **Realized PnL (today)** | `StrategyRepo::realized_pnl_lamports_since(ts)` | Real `End`-position `SUM(exit_lamports − entry_lamports)` since 00:00 UTC. Same exit−entry lamports basis as `positions_summary`. |
| **Bot correlation** | `StrategyRepo::managed_mints(real_only)` → `ManagedMint` | Open positions `LEFT JOIN strategy_rules` for the rule name; projection-only. The service reduces to one badge per mint via `status_rank` (ExitPending > Holding > BuySubmitted > Arming) — the sharpest double-sell risk wins. |

## Cash vs meme positions (`AssetKind`)

USDC is **working capital**, not a trading bag. Classification SSOT:
`trading_core::models::asset` (`asset_kind` / `is_cash` / `is_expected_non_position`) over
`USDC_MINT` + `WSOL_MINT` in `config::constants::protocol`.

| Rule | Cash (USDC) | Meme positions |
| --- | --- | --- |
| Pricing | Face $1 / UI unit | Curve SOL spot; Jupiter fallback |
| Cost basis / unrealized PnL | Always `None` | `holding_pnl` → `unrealized_pnl` |
| Paged Holdings table | Excluded (cash strip) | Included |
| Summary / Home KPIs | `cash_value_*` / `cash_holdings` | `positions_value_*`, PnL, 24h, `position_count` |
| Boot orphan reconcile | Excluded (with WSOL) | Flagged if untracked |

Wire field: `PortfolioHolding.asset_kind` (`cash` \| `wrapped_sol` \| `meme`). Frontend mirror:
`shared/lib/assetKind.ts`.

## Service composition — `live/src/services/portfolio.rs`

Mirrors `wallet_tokens.rs` (on-chain + Jupiter + cache), not DB-repo-backed for the live
fields. `compose()` fires five independent reads together (`tokio::join!`): Jupiter marks,
on-chain curve facts (uncached mints only), cost basis, token enrichment, real
`managed_mints`, then resolves the curve marks for the whole mint set in one pass through
`token_cache::mark_quote` — up front, never inside the per-holding map, so no `DashMap`
read guard is live while a second shard read is taken. Then per holding:

- **Cash** → face `price_usd = 1`, `value_usd = ui_amount`, SOL via `value_usd / sol_usd`;
  no Jupiter / no PnL / no managed-by / no curve flags.
- **SOL mark (meme)** = the **curve spot** from `mark_quote` (`price × 10^decimals`), with
  Jupiter's `price_usd / sol_usd` as the fallback for a bag the engine never tracked. The
  curve wins because the cost basis is an on-chain SOL fill: marking it against a third
  party's USD quote divided by a separately-polled SOL/USD rate puts two price universes on
  the two legs of one ratio and lets them drift. `None` ⇒ no PnL.
- **USD and `value_sol` follow that one mark** — `price_usd = mark × sol_usd` and
  `value_sol = mark × ui_amount` whenever the curve priced the row — so the value column and
  the PnL column cannot tell different stories. Jupiter still supplies `liquidity`,
  `price_change_24h` and `token_created_at`, which are not prices.
- **Percent is served, not divided in the browser**: `total_unrealized_pnl_pct` on both
  `PortfolioSummary` and `HoldingsTableSummary`, through `weighted_return_pct`.
- **PnL (meme)** via `holding_pnl()` — the service's single call into `unrealized_pnl`. It
  only lifts the SOL/raw average entry into **UI space** (`× 10^decimals`) to match the
  per-UI mark; the arithmetic is the SSOT helper's. Cost basis needs only the average entry
  (shown even with no live mark); mark-to-market needs both.
- **Enrichment** flattened via `TokenEnrichment` (the strategy-table SSOT). `is_migrated` /
  `is_cashback_enabled` / `symbol` are overwritten with the **live-authoritative** values
  (cache → on-chain fallback), so the live wallet facts win over any stale DB copy.

### What this service does NOT price

A holding is the **wallet's whole balance of a mint**, against an average cost across every
buy the wallet ever made on it. That is the right answer for the Holdings and Home
surfaces, and the wrong one for a **position**, which owns a specific bag bought at a
specific fill. Never join a holding to a position by mint: an open position is priced by
`models::portfolio::mark_bag` and served per position id by
`GET /strategies/{strategy}/positions/open/marks`. See
[pnl-percent-definition](../strategies/pnl-percent-definition.md).

Endpoints: `holdings` (full list incl. cash), `holdings/query` (meme positions only),
`holdings/summary` (cash strip unfiltered + filtered position metrics), `summary` (Home KPIs
with cash/positions split), `positions` (`open_positions`, `?real=` default true).

## Bounds / cost (EC2 4GB)

Held-mint and open-position sets are both tiny, so every join here is over a handful of
rows — no pagination needed, no new pools. `summary` runs the same composition as `holdings`
(Home hits both); acceptable at this scale, revisit with a short-TTL cache if it shows up.

## Open decision (revisit)

Cost basis for manual buys uses the lifetime `trades` avg-entry (locked in Phase 1.1). It
blends across a buy→sell-all→re-buy cycle rather than tracking the current bag's lots
(FIFO/avg-of-open). Fine for a display number; upgrade to a dedicated cost-basis store only
if lot accuracy is ever needed.
