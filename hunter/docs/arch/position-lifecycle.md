# Real-trade position lifecycle (post-Console redesign)

> Rewritten 2026-07-26 (`strategy-redesign`) after the status split (mig 0013) + manual
> positions + Console landed. Describes the CURRENT machine; the pre-redesign audit
> (defect matrix B1-B5 / S1-S7 / M1-M4 / N1-N4) is in git history of this file, and the
> decision record is [../roadmap/real-trade-console-redesign.md](../roadmap/real-trade-console-redesign.md).
> Companion arch docs: [strategies.md](strategies.md), [trade-execution.md](trade-execution.md),
> [frontend.md](frontend.md).

## 0. Principles (locked)

1. **Everything you hold real SOL in is a row** — bot or manual, one list, one status
   machine (`strategy_positions`, `origin` = `bot | manual`).
2. **A row with SOL still in it is OPEN.** "Stuck" is open·attention, never history.
3. **Status is truth.** The backend says exactly what a row is and which actions are
   legal; the FE renders, never infers (`needs_review` is backend-derived too).
4. **Every non-OK state has at least one action** (the close-action matrix, §3).
5. **Async + observable.** Manual buys 202 immediately; progress arrives over SSE like
   bot trades.

## 1. Status vocabulary

`hunter_engine::event::PositionStatus` ⇄ `strategy_positions.status` (CHECK in mig 0013).
`ExitFailed` and `Arming` no longer exist (0013 remapped: `ExitFailed` + entry NULL →
`EntryFailed`; other `ExitFailed` → `ExitStuck`; `Arming` rows deleted/promoted).

| Status | Partition | Meaning |
|---|---|---|
| `BuySubmitted` | Open | buy in flight; stale >600 s (`BUY_SUBMITTED_REVIEW_SECS` SSOT) surfaces `needs_review` via API + SSE |
| `Holding` | Open | entry filled, SOL deployed |
| `ExitPending` | Open | sell in flight |
| `ExitStuck` | Open · attention | sell gave up, **bag still held**; reaper redrives ×`EXIT_REDRIVE_CAP=2` then `exit_parked` (mig 0012 cols) |
| `ExitUnconfirmed` | Open · attention | sell may/may-not have cleared; never auto-re-sold; bag-cleared reaper heal + manual Verify |
| `End` | Terminal | confirmed exit (incl. `Dead` write-off) |
| `EntryFailed` | Terminal | buy never filled; **no hypothetical exit price stamped; excluded from realized PnL** (entry NULL) |

Partition SSOT guards: `sinks.rs::status_partition_guard` (Rust, compile-forcing) ⟷
`liveStatusSlice.ts` `OPEN_STATUSES`/`ATTENTION_STATUSES`/`TERMINAL_STATUSES` (+ vitest).
Closed-PnL predicate everywhere: `entry_price IS NOT NULL AND status = 'End'`
(`CLOSED_PRED` in `strategy_repo.rs`).

## 2. Transitions

Engine (`hunter/engine/src/reduce.rs`), unchanged shape except the split:
- EntryPending exhaust (×3) / Fatal → **`EntryFailed`** (caps rolled back).
- ExitPending Fatal / attempts ≥5 → **`ExitStuck`** (engine drops the arm; PG row stays
  open — the reaper owns it from here).
- `FillFailed::Unconfirmed` → `ExitUnconfirmed` (arm dropped, row open).
- `ArmState::EntryPending` carries frozen `lamports` — retries resize identically
  (manual episodes have no rule row; `0` = boot-adopted, falls back to rule config).

Reaper (`reapers.rs`, boot + 60 s):
| Sweep | Behavior |
|---|---|
| `resolve_buy_submitted` (shared with Verify) | adopt indexed fill → Holding; drop only when EVERY sig confirmed-reverted; else wait. Past review window → `needs_review` SSE flag |
| `heal_exit_pending_cleared` | ExitPending net ≤ dust (+ sell on record) → book End (runs BEFORE redrive) |
| `redrive_orphaned_exit_pending` | nudge engine / orphan sell |
| `redrive_exit_stuck` | ExitStuck-with-bag (`find_exit_stuck_bags`, excl. parked) → orphan sell + backoff; at cap → `exit_parked` |
| `heal_cleared_by_status("ExitStuck"/"ExitUnconfirmed")` | bag gone → book End (the ExitUnconfirmed heal that never existed pre-split) |
| `mark_stale_exit_pending_stuck` | real ExitPending >300 s: bag-check first — heal-eligible rows are left to the heal; the rest flip `ExitStuck` (never a blind terminal stamp) |
| `close_stale_paper_exit_pending` | paper crash artifact → book End at entry (breakeven) |

## 3. Close-action matrix (backend-enforced)

`POST /api/strategies/{s}/positions/{id}/close?action=retry|dump|writeoff|verify`
([handlers/strategies/positions.rs](../../live/src/api/handlers/strategies/positions.rs)):

| Status | retry/sell | dump | writeoff | verify |
|---|---|---|---|---|
| Holding | ✓ (engine ManualClose / orphan) | — | — | — |
| ExitPending | 409 busy | — | — | — |
| ExitStuck | ✓ (un-parks: `unpark_exit` resets count) | ✓ | ✓ (`Dead`, manual-only forever) | — |
| ExitUnconfirmed | ✓ re-sell (safe: landed original → NothingToSell → booked cleared) | ✓ | ✓ | ✓ PG-net check → heal or `{still_held}` |
| BuySubmitted | 409 (use verify) | — | — | ✓ reaper adopt-or-drop, one shot |
| End / EntryFailed | 409 terminal | — | — | — |

## 4. Manual positions (`origin='manual'`)

- `POST /api/positions/manual-buy` `{mint_address, amount_sol, tp_pct?, sl_pct?}` →
  validates (`MAX_MANUAL_BUY_SOL`, mint, one-open-position-per-mint) → 202
  `{position_id}` (pre-minted uuid). The engine command mints a **fresh per-episode
  rule id** and folds `Event::ManualBuy`; the sink creates the row born with that id
  under ONE `strategy_runs` row (`strategy_id='manual'`, `rule_id` NULL —
  `strategy_positions.rule_id` has no FK). From there it IS a bot buy: journal,
  `confirm_entry`, retry ×3, reaper adopt, double-buy sig index.
- Manual buys bypass entry gates/caps; keep the per-mint guard + size cap.
- **TP/SL** (`manual_exit` JSONB, `POST …/{id}/manual-exit` to change): compiled into a
  per-position one-off rule (`EngineState::manual_rules`, keyed by POSITION, outside
  `rules` so reloads can't wipe it) through the ONE bot TP/SL desugar — full exit stack
  incl. Dead-exit. **Without TP/SL: tracked-only — NO auto-exit of any kind** (the
  Entered arm resolves no rule and makes no decision). Boot adopt re-installs from the
  JSONB.
- Wallet "Sell All by mint" (external bags) now claims the engine per-mint exit lock —
  409 while a bot exit is in flight (the old double-sell race is closed).

## 5. Frontend — the Console (`/console`)

`hunter/frontend/src/live/pages/console/ConsolePage.tsx`. `/floor`, `/trade`, `/ops`,
`/positions` redirect (query preserved: `/trade?mint=X` prefills the manual panel).
TradePage/OpsPage are deleted; MyWalletPage keeps holdings/cashback + row dialogs only.

Lanes (one page, no tabs): **⚠ ATTENTION** (ExitStuck w/ PARKED · retry n/2 chips,
ExitUnconfirmed, needs-review BuySubmitted — action cells mirror §3 exactly) → **OPEN**
∥ **MANUAL TRADE** (buy 202→SSE, TP/SL, sell-all-by-mint w/ tracked-row warning, chart,
localStorage-persistent trade log) → **WAITING** (collapsible) → **RECENT CLOSED**
(End·EntryFailed only). Rows: origin dot (● bot / ○ manual) · status chip + sub-chips ·
dead-pool ❗ (holdings `is_dead`) · mode badge (`mode?` when unknown) · MTM · age (⚠
when SSE stale). Notifications deep-link to `/console?position=…` (`opsNotifyHref`).

## 6. Key files

- Engine: `hunter/engine/src/{event,reduce,arm,state,event_log}.rs` (+ golden tests
  `manual_buy_*`, `set_manual_exit_*`)
- Live: `hunter/live/src/strategies/engine/{sinks,reapers,orphan_exit,decision_loop,mod}.rs`
- Repo/model: `hunter/core/src/models/strategy.rs`,
  `hunter/core/src/storage/repositories/strategy_repo.rs`
- Migrations: `0012_exit_redrive_park.sql`, `0013_status_split_manual_origin.sql`
- API: `hunter/live/src/api/handlers/strategies/positions.rs`,
  `handlers/trading/solana.rs` (sell lock)
- FE: `live/pages/console/ConsolePage.tsx`, `live/slices/liveStatusSlice.ts`,
  `shared/lib/strategy/nav.ts`
