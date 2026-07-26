# Real-trade Console redesign — plan

> Status: **ALL PHASES LANDED** (2026-07-26, `strategy-redesign`) — P1..P4 implemented and
> committed; migration 0013 auto-applies on next boot. REMAINING before this file is
> deleted: paper/zero-SOL smoke of the new status flow + manual-buy path, then a real-SOL
> smoke on the EC2 box. [../arch/position-lifecycle.md](../arch/position-lifecycle.md) now
> describes the NEW machine (the pre-redesign audit this plan grew from lives in git
> history).
>
> User decisions locked in:
> 1. **One unified Console** replaces Floor + Trade (Wallet slimmed to funding/keys).
> 2. **Manual buys become full positions** (tracked, MTM PnL, optional TP/SL).
> 3. Attention lane and manual entry are **both first-class** (balanced layout).
> 4. **Full backend status redesign** (split the overloaded statuses; UI renders truth, never infers).
> 5. **Replace, don't duplicate** — old pages deleted, old routes redirect.

---

## 0. Design principles

1. **Everything you hold real SOL in is a row.** Bot or manual, one list, one status machine.
   No SOL can be "invisible" (kills the two-worlds split, M1).
2. **A row with SOL still in it can never land in a closed/actionless table.** "Stuck" is open,
   not history (kills the 9QLez class of confusion, S2/S6).
3. **Status is truth, not a hint.** The FE never infers meaning from `fill IS NULL` or bag
   heuristics — the backend says exactly what a row is and which actions are legal.
4. **Every non-OK state has at least one action.** Nothing is a dead end (kills S3, B3).
5. **Async + observable over sync + blocking.** Manual buy/sell return immediately with a row;
   progress arrives over SSE like bot trades (kills M2's pending-shown-green).

---

## 1. New status model (backend)

### 1.1 Status vocabulary

| Status | Partition | Meaning | Change |
|---|---|---|---|
| `BuySubmitted` | Open | buy in flight | unchanged; **stale >600 s now surfaces a `needs_review` flag via API/SSE** (was log-only, B3) |
| `Holding` | Open | entry filled, SOL deployed | unchanged |
| `ExitPending` | Open | sell in flight | unchanged |
| `ExitUnconfirmed` | Open · attention | sell may/may-not have cleared | kept, but gains a **heal reaper + Verify / Re-sell / Write-off actions** (was dead end, S3) |
| **`ExitStuck`** | **Open · attention** | sell gave up, **bag still held**; sub-state `exit_parked` + `exit_redrive_count` already exist (mig 0012) | **NEW** — replaces "ExitFailed with bag". Open, not terminal |
| **`EntryFailed`** | Terminal | buy never filled (entry exhausted / fatal), no SOL deployed | **NEW** — replaces "ExitFailed with `fill:None`". Excluded from PnL (fixes B2's phantom-loss pollution) |
| `End` | Terminal | confirmed exit (incl. Dead write-off) | unchanged |
| `Arming` | — | vestigial | **DELETED** (CHECK value + `StrategyPosition::new` default → `BuySubmitted`) |

`ExitFailed` disappears from the vocabulary entirely — every current meaning has an honest home.

### 1.2 Transition changes (delta vs `reduce.rs` today)

| Transition | Old | New |
|---|---|---|
| EntryPending exhaust/Fatal | → `ExitFailed` (fill None) | → **`EntryFailed`**; caps still rolled back; **no hypothetical exit price stamped** |
| ExitPending Fatal / attempts≥5 | → `ExitFailed` | → **`ExitStuck`** |
| `fail_stale_exit_pending` (>300 s) | blind flip → `ExitFailed` | **bag-check first** (S4 fix): PG net ≤ dust → heal to `End`; bag present → `ExitStuck` |
| ExitStuck redrive ×`EXIT_REDRIVE_CAP=2` | (same, as ExitFailed) | → `exit_parked=true`, still Open |
| ExitStuck bag cleared | heal → End | unchanged (rename only) |
| **ExitUnconfirmed bag cleared** | *(no heal existed)* | **NEW reaper**: PG `trades` net ≤ dust → book `End` (mirror of `heal_exit_failed_cleared`) |
| Write-off (manual) | book `Dead` via `book_externally_cleared_pg` | unchanged; allowed on `ExitStuck` **and** `ExitUnconfirmed` |

### 1.3 Action legality matrix (backend-enforced, `close` endpoint)

| Status | Sell/Retry | Dump | Write-off | Verify-cleared | Notes |
|---|---|---|---|---|---|
| Holding | yes | — | — | — | normal manual close |
| ExitPending | no (busy) | — | — | — | in flight |
| ExitStuck | yes (retry) | yes | yes | — | parked ⇒ retry un-parks |
| ExitUnconfirmed | yes (**re-sell**, safe: orphan sell books NothingToSell→cleared if the original landed) | yes | yes | yes (on-demand PG-net check → heal or report "still held") | |
| BuySubmitted `needs_review` | — | — | — | yes (adopt-or-drop resolve) | reuse reaper adopt logic on demand |
| EntryFailed / End | — | — | — | — | terminal, no actions |

The endpoint returns the legality matrix per row (or FE derives it from status alone — status is
now sufficient, that's the point).

### 1.4 Migration + code touch list

- **Migration 0013**: remap existing rows —
  `ExitFailed` with entry fill NULL → `'EntryFailed'`; other `ExitFailed` → `'ExitStuck'`
  (bag-gone ones get healed to `End` by the reaper on next tick); update CHECK constraint
  (drop `Arming`, `ExitFailed`; add `EntryFailed`, `ExitStuck`).
- `hunter/engine/src/event.rs` enum + serde. **Caution:** `boot_recover` replays the event-log
  tail — keep a serde alias accepting legacy `"ExitFailed"` (mapped by fill presence) for one
  deploy generation.
- `reduce.rs` transitions (§1.2); `sinks.rs` partitioning + `on_terminal_no_fill` (no more
  hypothetical exit price); partition-drift test updated.
- `reapers.rs`: rename `redrive_exit_failed_bags`→`redrive_exit_stuck`, add
  `heal_exit_unconfirmed_cleared`, fix `fail_stale_exit_pending` bag-check, surface
  `needs_review` on stale BuySubmitted (new column or derived `submitted_at` age in the DTO).
- `strategy_repo.rs`: `find_open_positions` / `find_recent_closed` / `find_exit_failed_with_bag`
  (→ `find_exit_stuck_bags`) / `mark_*` guards / terminal-clobber `NOT IN` list.
- `close` endpoint (`handlers/strategies/positions.rs`): new action matrix (§1.3), add
  `?action=verify`.
- PnL surfaces (portfolio, summaries): exclude `EntryFailed` from realized PnL.

---

## 2. Manual buys as positions (backend)

### 2.1 Model

- `strategy_positions.origin TEXT NOT NULL DEFAULT 'bot'` (`'bot' | 'manual'`) — same
  migration 0013. Badge + filter key for the FE.
- One reserved **system rule per mode** (`name='manual'`, hidden from Rules UI, never
  auto-armed) so the row satisfies rule/strategy FKs with zero schema surgery elsewhere.
- Optional per-position exit config: `manual_exit JSONB NULL` (`{tp_pct, sl_pct}` to start).
  When present, the engine synthesizes a one-off `CompiledRule` (TP/SL desugar already exists
  from the flow-scalper work) and adopts the position as `Entered` — full TP/SL + Dead-exit +
  reaper coverage. When absent, the position is **tracked-only**: row, MTM PnL, manual close;
  no auto-exit of any kind.

### 2.2 Flow — manual buy goes through the engine, not around it

`POST /api/solana/wallet/buy` is replaced by `POST /api/positions/manual-buy`:

1. Validate (mint, `MAX_MANUAL_BUY_SOL=5.0` kept), take a per-mint **inflight guard** (fixes
   M3 double-click).
2. Insert `BuySubmitted` row (`origin='manual'`) + inject a manual episode into the engine as
   `EntryPending` — from here it is a bot buy: same `run_entry`, journal write-ahead,
   `confirm_entry` classification, retry ×3, reaper adoption, double-buy sig index. B3/B4
   handling comes free.
3. Return **202 `{position_id}` immediately**; the FE just renders the `BuySubmitted` row and
   watches SSE (M2 dies — there is no sync "green submitted" to lie with).
4. Entry gates/caps: manual buys **bypass** entry conditions and rule caps (they're the user's
   call) but still respect the one-position-per-mint guard.

Fill → `Holding` (+ synthesized TP/SL arm if requested). Exhaust → `EntryFailed`.

### 2.3 Manual sell

- Position rows (bot or manual): the existing `close` endpoint — already coordinated via
  engine/pg/mint locks.
- Wallet-level "Sell All by mint" stays (for external/Phantom bags with no row) but now
  **acquires the engine mint exit lock** before sweeping (fixes N2 race); on lock-busy it
  returns 409 "bot exit in progress".
- `reconcile_externally_cleared_holdings` unchanged — still the backstop for external sells.

---

## 3. The Console (frontend)

### 3.1 Layout

One route: **`/console`**. `/floor`, `/trade`, `/ops`, `/positions` redirect. `TradePage.tsx`
and `OpsPage.tsx` are deleted; `MyWalletPage` keeps funding/keys/cashback only (its broken
manual buy/sell modals — M4 — die with it).

```
┌─ CONSOLE ────────────────────────────────────────────────────────────────────┐
│ header: SOL balance · engine LIVE/DEAD switch · SSE ● live / ○ stale (two    │
│         clearly separate indicators, defect #10) · mode chip                 │
├──────────────────────────────────────────────────────────────────────────────┤
│ ⚠ ATTENTION (n)                                    ← always on top, never    │
│   9QLez  ExitStuck·PARKED   dead-pool❗  [Retry][Dump][Write off]   caps off │
│   BK3c   ExitStuck·retry 1/2             [Retry][Dump][Write off]            │
│   Xyz9   ExitUnconfirmed                 [Verify][Re-sell][Write off]        │
│   Abc2   BuySubmitted·stale 12m          [Verify]                            │
├───────────────────────────────┬──────────────────────────────────────────────┤
│ OPEN (n)  ● bot ○ manual      │  MANUAL TRADE                                │
│  ○ ABCd +0.42 SOL  [Sell][+TP/SL]  mint ________  [chart preview]            │
│  ● EFgh -0.10 SOL Holding [Sell]│  amount ___ SOL   ☐ TP __% ☐ SL __%        │
│  ● IJkl  BuySubmitted  (busy) │  [ BUY ]          [ Sell all by mint ]       │
│                               │  ── trade log (persistent) ──────────────    │
│                               │   12:01 buy ABCd 0.5 → filled               │
├───────────────────────────────┴──────────────────────────────────────────────┤
│ WAITING (armed rules, collapsible)                                           │
├──────────────────────────────────────────────────────────────────────────────┤
│ RECENT CLOSED (End · EntryFailed, ring 50 — safe now: stuck rows never here) │
└──────────────────────────────────────────────────────────────────────────────┘
```

Balanced layout per decision 3: attention full-width on top (it's rare but urgent), then
open-positions and manual-trade side-by-side as co-equal panels (stacked on narrow viewports).

### 3.2 Row anatomy (open + attention lanes)

`mint · origin dot (●bot/○manual) · status chip (+parked / retry n/2 / stale-age sub-chip) ·
mode badge · MTM PnL · dead-pool ❗ chip (from `is_dead_verdict`, defect #9 — a rug must not
look like "no data") · age · actions`.

Status chips map 1:1 to backend statuses — zero FE inference. `ATTENTION_STATUSES` moves from
the page into `liveStatusSlice` next to its siblings:
`OPEN = {BuySubmitted, Holding, ExitPending, ExitUnconfirmed, ExitStuck}`,
`ATTENTION = {ExitUnconfirmed, ExitStuck} ∪ {BuySubmitted where needs_review}`,
`TERMINAL = {End, EntryFailed}`.

### 3.3 Manual-trade panel

- Buy: mint + amount + optional TP/SL toggles → `manual-buy` (202) → row appears instantly in
  OPEN as `BuySubmitted` with a spinner; all further truth via SSE. Submit button disabled
  while inflight (guard mirrors backend).
- `[+TP/SL]` on any manual `Holding` row → PATCH `manual_exit` → engine adopts (2.1).
- Sell-all-by-mint kept for external bags; warns when a tracked row exists ("use the row's
  Sell instead").
- **Persistent trade log**: replace the ephemeral page-local array (cap 20) with a small
  `manual_actions` PG table (or reuse the engine event log filtered to manual origin) so the
  log survives reloads — defect #10.

### 3.4 Notifications / deep links

`nav.ts`: every position notification deep-links to `/console` with a `?focus=<positionId>`
that scrolls/flashes the row in whichever lane it lives — fixes defect #3 structurally
(there is no wrong-tab to land in; lanes derive from the same slice sets).

---

## 4. What gets deleted

- `TradePage.tsx` (+ its pending-ignore bug M2), `OpsPage.tsx` (+ dead `openCols`
  Dump/Write-off branch), MyWalletPage manual buy/sell modals (M4 bug), `/floor`-era tab logic,
  page-local `ATTENTION_STATUSES`.
- `POST /api/solana/wallet/buy` (replaced by `manual-buy`); wallet `sell` kept but
  lock-coordinated.
- `ExitFailed` + `Arming` status values; hypothetical-exit-price stamping for never-bought rows.

---

## 5. Phasing

| Phase | Scope | Ships alone? |
|---|---|---|
| **P1 — status split** ✅ DONE | §1 whole: enum, migration 0013 (remap + CHECK + `origin` + `manual_exit` cols in one migration), reduce/sinks/reapers/repo, close-action matrix (incl. `?action=verify`), drift test. FE minimal patch: slice sets + labels + notification maps updated; OpsPage attention lane now holds ExitStuck with Retry/Dump/Write-off | yes (old UI still renders; ExitStuck rows now appear in Needs-attention — already an improvement) |
| **P2 — manual positions** ✅ DONE | §2: engine `ManualBuy`/`SetManualExit` events + per-position `manual_rules`, `POST /api/positions/manual-buy` (202 `{position_id}`), `POST …/{id}/manual-exit`, wallet-sell mint lock (N2). Deviation from decision 2: since `strategy_positions.rule_id` has NO FK, manual episodes use a **fresh per-episode rule uuid** + ONE `strategy_runs` row (`strategy_id='manual'`, `rule_id` NULL) instead of a hidden rule row — same goal, zero Rules-UI filtering needed | yes (FE wiring lands with P3's Console) |
| **P3 — Console** ✅ DONE | `/console` (`ConsolePage.tsx`): attention lane on top (per-status action cells mirroring the close matrix, PARKED/retry/stale chips), OPEN ∥ MANUAL TRADE panels (buy→202+SSE, TP/SL, sell-all-by-mint with tracked-row warning, session trade log), collapsible WAITING, RECENT (End·EntryFailed). Deleted `TradePage.tsx`+`OpsPage.tsx`; `/floor` `/trade` `/ops` `/positions` redirect (query preserved — `/trade?mint=X` prefills the panel); MyWalletPage header manual modals (M4) removed, links → Console; notifications deep-link to `/console` | yes |
| **P4 — polish** ✅ DONE | dead-pool ❗ chip (holdings `is_dead` join), trade log persists across reloads (localStorage `mt:console-trade-log`; the durable record stays the positions table — a PG `manual_actions` table remains optional future work), stale-SSE age cue (⚠ + warning tone), unknown-mode badge (`mode?` instead of masquerading as real) | incremental |

P1 and P2 are backend-first and independently smoke-testable via paper mode + the probe/dryrun
bins (zero-SOL verification per the usual workflow). Real-SOL smoke happens once per phase on
the EC2 box (migrations auto-apply on boot).

### Definition of done per phase
`cargo check -p hunter-live -p hunter-lab` + clippy clean · partition-drift test green ·
`npm run build:live` + `npm run lint` clean (P3) · arch docs updated
(`position-lifecycle.md` rewritten to describe the NEW machine once P1 lands; `frontend.md`
Console section once P3 lands) · this roadmap file updated/deleted as phases land.

---

## 6. Decisions taken without asking (veto anytime)

1. `ExitStuck` / `EntryFailed` naming (vs `SellStuck`/`BuyFailed`).
2. Manual rows satisfy FKs via a hidden per-mode `manual` system rule (vs nullable FKs — less
   schema churn, keeps every existing JOIN working).
3. Manual positions **without** TP/SL get no auto-exit at all (not even Dead-exit) — tracked
   only. Adding TP/SL later opts into the full engine exit stack including Dead.
4. Manual buys bypass entry gates/caps but keep the per-mint single-position guard and
   `MAX_MANUAL_BUY_SOL`.
5. Wallet-page survives as funding/keys/cashback only (not folded into Console).
6. `Arming` removed in the same migration (it was vestigial).
7. Write-off stays manual-only forever (bounded-then-park policy unchanged).
