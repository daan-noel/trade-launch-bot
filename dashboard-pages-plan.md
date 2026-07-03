# Dashboard pages — phased plan & todos

Live-app dashboard surfaces for the three operator jobs: **manual buy/sell**, **manage
holdings**, **manage + monitor real trading per strategy**. Self-contained — a fresh
session can execute from here. Absorbs the former `wallet-holdings-recommendations.md`
(Holdings deep-dive) into Phase 2. Companion: `@arch/frontend.md`, `@arch/architecture.md`.

## Mental model

Four focused surfaces, not one mega-page — each answers one operator question, all read a
**shared portfolio/PnL + bot-position service** so the same fact (cost basis, PnL,
who-manages-this-mint) is never computed two ways (SSOT — CLAUDE.md forbids duplicating a
fact that must stay consistent). Built for extension: a new strategy or action plugs into
existing surfaces without reshaping the app.

- **Home** — "how's my money right now?" (glanceable single pane)
- **Trade** — "buy/sell this mint, now" (fast manual action)
- **Holdings** — "what do I hold, up or down, act" (position manager)
- **Live Trading** — "how is my real trading doing across all strategies?" (monitor)

The mature per-strategy pages (`TpslPage`/`Swing1Page`) stay as **rule-authoring**
surfaces (Real/Paper sections, positions panel + PnL summaries, activate/pause/stop, bulk,
inspect). We do **not** rebuild them.

## Decisions (locked)

- Manual buy/sell → **dedicated Trade page** (Holdings keeps quick per-row sell).
- Cross-strategy monitor → **its own "Live Trading" page** (keeps Home glanceable).
- First build → **foundation first**: Phases 1–3 below. **No sell-path change** until Phase 5.

## Backend map (verified — file:line the plan builds on)

- **Read service pattern:** `live/src/services/wallet_tokens.rs` (`list_enriched` :28,
  `enrich_one` :37, `enrich_holdings` :47) — composes `state.trader` (on-chain RPC) +
  `jupiter::fetch_prices` + in-memory `state.token_cache`. **Not** DB-repo backed. The new
  portfolio service mirrors this.
- **Handlers + routes:** `live/src/api/handlers/trading/solana.rs`; registered in
  `configure_deploy_routes` (`live/src/api/mod.rs:14`, `/api` scope). A new
  `/api/portfolio/*` route registers here.
- **State:** handlers take `web::Data<Arc<DeployState>>` (`live/src/state/deploy_state.rs:24`),
  which `Deref`s to `CoreState` (`trading_core/src/state/core_state.rs:27`): `token_repo()`,
  `trade_repo()`; positions via `app_state.strategy.repo()` → `StrategyRepo`. Wiring
  `live/src/main.rs:859-865`.
- **Trades (cost basis):** `TradeRepo` (`trading_core/src/storage/repositories/trade_repo.rs:24`).
  Wallet is `wallet_id` INTEGER FK → `wallet_dict` (no wallet string col on `trades`). Entry
  **not stored** — derived from `amount_lamports` ÷ `token_amount`, `trade_type='buy'`.
  **No avg-entry aggregate exists yet — Phase 1 adds it.** Closest primitives:
  `find_latest_by_wallet_mint_type` (:393), `sum_legs_by_signatures` (:559 →
  `SigLegs::price_per_token` :803).
- **Positions:** `StrategyPosition` (`trading_core/src/models/strategy.rs:133`) — `mint`,
  `token_account`, `rule_id`, `mode` ('real'/'paper'), `status`
  (`Arming|BuySubmitted|Holding|ExitPending|End|ExitFailed`), `entry_price`/`entry_sol`,
  helpers `is_holding` (:189), `realized_pnl_sol` (:205), `pnl_pct` (:213), `is_win` (:222).
  `StrategyRepo` **already has cross-strategy** `find_open_positions` (:1484, all rules,
  `status NOT IN ('End','ExitFailed')`) + `distinct_unsettled_real_mints` (:1499) + per-run
  `PositionsSummary` (`positions_summary` SQL :1279). **No HTTP endpoint exposes the
  all-strategies view yet** — handlers are per-`{strategy}` (`handlers/strategies/positions.rs`).
- **SSE `trade_executed`:** emit `live/src/ingest/consumer.rs:283`; render
  `trading_core/src/api/handlers/system/stream.rs:89-113` (event name `"trade_executed"`,
  **mint-scoped**, fresh price/mcap under the `"live"` key). Frontend `TokensPage`
  `visibleMintsRef` filters it. Reuse for held/position mints.
- **Enrichment SSOT:** `token_enrichment::{ENRICH_SELECT, fetch_by_mints}`
  (`trading_core/src/storage/token_enrichment.rs:50/:179`); `TokenRepo::find_by_mints`
  (`token_repo.rs:492`); conversions `config::constants::{sol_to_lamports, lamports_to_sol}`
  (`config/constants/token_math.rs:79/:85`).

---

## Phase 1 — Portfolio/PnL + bot-position service (backend SSOT) ⭐ keystone

Build once; Phases 2, 3, 4 all read it. Bounded/cheap (held-mint + open-position sets are
tiny → safe on 4GB EC2). No new pools.

- [ ] **1.1 Cost-basis repo fn** — add `TradeRepo::avg_entry_by_wallet_and_mints(wallet, &[mint])`
  in `trade_repo.rs`. SQL: `SUM(amount_lamports)/SUM(token_amount)` grouped by mint over
  `trade_type='buy'` for the wallet (resolve `wallet_id` via `wallet_dict`). Returns
  `{mint → (avg_entry_price, total_token_amount, total_cost_lamports)}`. **This is the
  manual-buy cost-basis SSOT.** (Bot buys already carry `strategy_positions.entry_*`.)
- [ ] **1.2 Pure PnL math** — one helper (in `trading_core`, e.g. `models/position.rs` or a
  small `portfolio` mod) computing `{cost_basis_sol, unrealized_pnl_sol, unrealized_pnl_pct}`
  from `(avg_entry_price, current_mark, held_ui_amount)`. Reuse the existing
  `StrategyPosition::{realized_pnl_sol,pnl_pct}` conventions; do **not** re-derive PnL in JS.
- [ ] **1.3 Bot-correlation read** — thin wrapper over `StrategyRepo::find_open_positions`
  returning `{mint → {rule_id, rule_name, status, mode}}` for a held-mint set (correlate on
  `mint` / `token_account`). Real-only filter available via `mode='real'`.
- [ ] **1.4 Portfolio service** — new `live/src/services/portfolio.rs` mirroring
  `wallet_tokens.rs`; composes: held holdings (`state.trader`) + marks
  (`jupiter::fetch_prices`) + cost basis (1.1) + PnL (1.2) + bot info (1.3) + enrichment
  (`token_enrichment::fetch_by_mints`). Live wallet fields (symbol/migrated/cashback/marks)
  **win** over any DB copy.
- [ ] **1.5 Endpoints** (handler in `live/src/api/handlers/trading/` — new `portfolio.rs`;
  register in `configure_deploy_routes`, `api/mod.rs:14`):
  - `GET /api/portfolio/holdings` — enriched holdings + cost basis + unrealized PnL + bot
    tag (Holdings page + Home top-holdings).
  - `GET /api/portfolio/summary` — totals: value SOL/USD, total unrealized PnL, # positions,
    realized PnL today, # active rules, open-position count (Home KPIs).
  - `GET /api/portfolio/positions` — all open **strategy** positions cross-rule via
    `find_open_positions` (Live Trading roll-up, Phase 4). Real-only param.
- [ ] **1.6 SSOT guard test** — assert the JS-facing PnL never re-implements the math (single
  compute site); a `live` unit test on 1.1/1.2 with a fixture (avg-entry + mark → known PnL).
- [ ] **1.7 Docs** — `@arch/database.md` (new `TradeRepo` fn), `@arch/architecture.md`
  (new service + `/api/portfolio/*`); note the cost-basis SSOT decision in `@plans/`.
- [ ] **DoD:** `cargo check -p live` + `cargo check -p trading_core` clean; clippy on touched;
  the guard test passes.

## Phase 2 — Holdings page → position manager

Evolve `MyWalletPage.tsx` (+ `@live/components/wallet/walletColumns.tsx`) from **balance
viewer** to **position manager**. Absorbs the former recommendations doc. Priority order
is deliberate: PnL + bot-awareness first (neither needs the risky sell-path change); partial
sells wait for Phase 5.

Current 3 data sources stay (RPC scan, Jupiter 20s poll, token-DB enrichment) — Phase 1's
`/api/portfolio/holdings` folds the enrichment + cost basis + bot tag into one server join.

- [ ] **2.1 Point at `/api/portfolio/holdings`** — new `getPortfolioHoldings` endpoint in
  `@live/store/liveEndpoints.ts`; `WalletHolding` `extends TokenEnrichmentFields` and gains
  `cost_basis_sol`, `unrealized_pnl_sol`, `unrealized_pnl_pct`, `managed_by` (`{rule_name,
  status, mode} | null`). Keep the surgical single-mint post-trade patch (`confirmTrade`).
- [ ] **2.2 Portfolio header** — stat row above the table: **total value (SOL + USD)**, 24h
  change, **# positions**, **total unrealized PnL**. Build from `components/ui/`; memoize so
  the 20s price tick doesn't re-render the table.
- [ ] **2.3 Per-row PnL** — add **cost basis** + **PnL% / PnL SOL** columns (green/red) in
  `walletColumns.tsx`.
- [ ] **2.4 Bot-managed badge** ⭐ — per row, show `managed_by` (rule name + status:
  Holding / TP-armed / exit-pending). **Why critical:** a manual Sell-All can race the bot's
  own exit → the double-sell risk that's a hard constraint. Surfaces autopilot vs. orphaned.
- [ ] **2.5 Keep actions** — Manual Buy + row Sell-All + manual-sell dialog stay as-is
  (100% sell); add a confirm warning when `managed_by` is set.
- [ ] **2.6 Niceties (optional)** — dust filter (hide value < threshold); click-row →
  `TokenTradeChart` detail (Tokens page already has this).
- [ ] **DoD:** `npm run build` clean; no extra re-render on SOL/USD tick or trade stream.

## Phase 3 — Home → "Command Center"

Replace the empty `pages/home/HomePage.tsx` with the single pane of glass. Mostly
aggregation once Phase 1 exists.

- [ ] **3.1 KPI row** — from `/api/portfolio/summary`: wallet value (SOL + USD), unrealized
  PnL, realized PnL today, open positions (all strategies), # active rules, **live-mode
  status** (reuse `useGetLiveModeQuery`), SOL balance. Reusable `StatTile` in
  `components/ui/`.
- [ ] **3.2 Top holdings widget** — top N by value from `/api/portfolio/holdings`, link → Holdings.
- [ ] **3.3 Live trade feed** — reuse the `trade_executed` SSE (`visibleMintsRef` pattern
  from `TokensPage`); memoize ticks — hot path, no per-tick table re-render.
- [ ] **3.4 Per-strategy real-P&L strip** — compact per-strategy open/realized from
  `/api/portfolio/summary` (or the Phase-4 positions endpoint), link → Live Trading.
- [ ] **3.5 Docs** — `@arch/frontend.md` (new Home surface + `/api/portfolio/*` hooks).
- [ ] **DoD:** `npm run build` clean; Home stays glanceable + cheap to render.

---

## Phase 4 — Live Trading roll-up page (deferred; cheap once Phase 1 lands)

Cross-strategy **real** monitor. New route `/live-trading` + `liveNav` item (`nav.ts`) +
lazy route in `App.tsx`. Reads `GET /api/portfolio/positions` (Phase 1.5).

- [ ] **4.1 Route + nav + page shell** (`live/pages/...`, `nav.ts`, `App.tsx`).
- [ ] **4.2 Combined open-positions table** across tpsl1/tpsl2/swing1 (reuse `DataTable` +
  the strategy position columns); real-only.
- [ ] **4.3 Realized-P&L over time** + **per-strategy win-rate / comparison** (roll up
  `PositionsSummary` per strategy; reuse `SimSummaryCard`-style tiles).
- [ ] **DoD:** `cargo check -p live` + `npm run build` clean.

## Phase 5 — Dedicated Trade page + partial sells (deferred; heaviest, riskiest)

New route `/trade` + nav + lazy route. **Backend `sell(amount)` path is the biggest,
riskiest change — carries double-sell risk; do last.**

- [ ] **5.1 Trade page shell** — route/nav; mint input → live price/liquidity + **chart
  preview** (reuse `TokenTradeChart`); Buy (SOL + slippage) + Sell forms.
- [ ] **5.2 Bot-managed interlock** ⭐ — before a manual action, warn if a live strategy
  manages the mint (reuse Phase 1.3 correlation). Preserves double-sell safety.
- [ ] **5.3 Recent manual-trades log**.
- [ ] **5.4 Backend partial sell** — add a `sell(amount)`/`sell(pct)` path across the `live`
  bin and `pump-trader`; sell-25/50/75%. **Keep sell-confirm on the `trades` gRPC feed — no new
  RPC poll** (hard constraint; the exit loop polls the full window before retry to avoid
  duplicate sells — preserve `execution/real.rs` behavior).
- [ ] **5.5 Wire partial sell** into Holdings (2.5) + Trade page.
- [ ] **DoD:** `cargo check -p live` + `cargo test -p pump-trader` + `npm run build` clean;
  double-sell path re-verified.

---

## Cross-cutting constraints (from CLAUDE.md)

- **Double-sell safety** — sell-confirm stays on the `trades` gRPC feed, no new RPC poll;
  bot-awareness (2.4 badge / 5.2 interlock) exists partly to prevent manual-vs-bot races.
- **EC2 4GB / IO-bound** — held-mint + open-position joins are bounded and cheap. Prefer the
  existing SSE over new short-interval RPC polling. No new pools / infra spend.
- **SSOT** — one PnL/cost-basis compute site (Phase 1.1/1.2); reuse `find_by_mints` +
  `ENRICH_SELECT` + the shared lamports↔SOL helpers; no copy-pasted formulas/column lists.
- **Frontend perf** — no extra re-render on the SOL/USD tick or live-trade stream; memoize
  high-freq ticks; build from `components/ui/`, `DataTable`, shared hooks.
- **Stay in the owning crate**; docs updated per touched tier (CLAUDE.md / `@arch` / `@plans`).

## Open decisions (revisit at the relevant phase)

- Cost-basis source for **manual** (non-bot) buys — `trades` avg-entry (Phase 1.1) vs. a
  dedicated cost-basis store. Bot buys already have `strategy_positions.entry_*`.
- Whether partial-sell (Phase 5.4) justifies the backend sell-path change now or later.
- Whether to replace the 20s Jupiter price poll with the `trade_executed` SSE for held mints,
  or run both.
