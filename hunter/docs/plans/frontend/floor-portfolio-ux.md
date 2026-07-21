# Floor + Portfolio IA

Live money surfaces after Rules Control/Evidence:

| Page | Route | Job |
| --- | --- | --- |
| **Floor** | `/floor` (alias `/ops`) | Live book — Open / Waiting / Needs attention / Recent; Sell ALL |
| **Portfolio** | `/portfolio` | Cross-rule closed PnL — today / 7d / all-time; by-rule table |
| **Rules** | `/strategies/rules` | Keep/kill + Evidence (runs) + **TOTAL** scoreboard rollup |
| **Wallet** | `/wallet` | Bag balances |
| **Trade** | `/trade` | Manual mint execute |

## Floor (Ops reshape)

- Title **Floor**; nav label Floor; canonical path `/floor` (`/ops` and legacy inventory paths redirect, query preserved).
- Tabs: **Open** (non-stuck) · **Waiting** · **Needs attention** (ExitPending/Failed/Unconfirmed) · **Recent** (short tail + PnL).
- Open columns: Age · Entry ◎ · **MTM ◎** · **PnL%** (bag `unrealized_pnl` SSOT, real mode) · Sell/Trade.
- Recent columns: PnL ◎ · **PnL%** · Hold · Closed.
- Row select expands **`rowDetail`**: fact tiles + `FloorMintChart` (entry/exit markers). Waiting also charts the mint; Open/Recent use `FloorPositionDetail`.
- Page strips: Open MTM by rule · Recent PnL by rule (`FloorBookStrip`).
- Rule column → Rules Evidence; Recent is not full history.
- Notify: clean `End` → Rules Evidence; stuck exits → Floor attention; else Floor open/waiting.

## Portfolio

- `GET /api/portfolio/performance?range=today|7d|all&mode=real|paper`
- Totals + by-rule rows (`RulePeriodPnlRow`) with **PnL%** = `realized / total_entry`.
- Page charts: by-rule PnL bars + PnL% bars (`PortfolioByRuleCharts`).
- Row select → `PortfolioRuleDetail` (tiles, Evidence/Floor links, closes in range, mint chart).

## Rules Control TOTAL

- Above the scoreboard (live `showScores`): rollup of visible rules under Current run / All-time — Total PnL · Avg% · Win% · Closed · Active · Open · Rules count.
- Evidence pane stays per-selected-rule.

## Home bridges

- Realized Today → Portfolio; Open → Floor; StrategyStrip → Floor / Evidence.
