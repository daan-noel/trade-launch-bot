# Floor + Portfolio IA

Live money surfaces after Rules Control/Evidence:

| Page | Route | Job |
| --- | --- | --- |
| **Floor** | `/ops` (alias `/floor`) | Live book — Open / Waiting / Needs attention / Recent; Sell ALL |
| **Portfolio** | `/portfolio` | Cross-rule closed PnL — today / 7d / all-time; by-rule table |
| **Rules** | `/strategies/rules` | Keep/kill + Evidence (runs) |
| **Wallet** | `/wallet` | Bag balances |
| **Trade** | `/trade` | Manual mint execute |

## Floor (Ops reshape)

- Title **Floor**; nav label Floor; path stays `/ops` for notify deep-links.
- Tabs: **Open** (non-stuck) · **Waiting** · **Needs attention** (ExitPending/Failed/Unconfirmed) · **Recent** (short tail + PnL).
- Rule column → Rules Evidence; Recent is not full history.
- Notify: clean `End` → Rules Evidence; stuck exits → Floor attention; else Floor open/waiting.

## Portfolio

- `GET /api/portfolio/performance?range=today|7d|all&mode=real|paper`
- Totals + by-rule rows (`RulePeriodPnlRow`); links to Rules Evidence + Floor.

## Home bridges

- Realized Today → Portfolio; Open → Floor; StrategyStrip → Floor / Evidence.
