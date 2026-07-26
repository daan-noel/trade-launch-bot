# Real-trade position lifecycle & UI/UX reference

> Audit date: 2026-07-26 (`strategy-redesign`). Cross-cutting map of how a real-money
> position moves through the backend state machine, the manual-trade API surface, and
> the frontend pages — plus every case that can occur and how the current implementation
> handles or misses it. Read this instead of re-exploring the source. Companion arch docs:
> [trade-execution.md](trade-execution.md), [strategies.md](strategies.md),
> [frontend.md](frontend.md).

Legend used throughout: **OK** handled well · **FRAGILE** handled but confusing/lossy ·
**GAP** missing or broken.

---

## 0. TL;DR — the two structural problems

Everything below reduces to two root causes:

1. **Two disconnected worlds.** Bot trades live in `strategy_positions` with a full status
   machine. **Manual buys live nowhere** — `POST /api/solana/wallet/buy` never creates a
   position row. A manual buy has no status, no TP/SL, no Dead-exit, and never appears on the
   Floor page. It is visible only as a wallet balance + a `trades`-feed cost-basis rollup, and
   the only way to exit it is the wallet-level "Sell All".

2. **One status word covers several realities.** `ExitFailed` means both "the sell reverted,
   you still hold the bag" **and** "the buy never happened at all" (entry exhausted, `fill:None`).
   `ExitUnconfirmed` means "we genuinely don't know if you still hold it." Because the statuses
   are ambiguous, the UI can't render honest, actionable controls.

---

## 1. The state machine (backend truth)

There are two unrelated `PositionStatus` enums in the monorepo. The live trading one is
**`hunter_engine::event::PositionStatus`** ([hunter/engine/src/event.rs:363](../../engine/src/event.rs))
persisted as `strategy_positions.status` TEXT. (The `forge` one — `Open/Closed/Dropped` in
`forge/core/src/models/status.rs` — is a different product; ignore it.)

Architecture split:
- Pure state machine (no I/O): [hunter/engine/src/reduce.rs](../../engine/src/reduce.rs) +
  [arm.rs](../../engine/src/arm.rs) drive an internal `ArmState`.
- Live side-effects: [hunter/live/src/strategies/engine/](../../live/src/strategies/engine/)
  `{sinks,exec_real,orphan_exit,reapers,decision_loop}.rs`.
- The DB `status` vocabulary is a **projection** of the engine's `ArmState`; only
  `EntryPending/Entered/ExitPending` project to PG rows.

### 1.1 Wire statuses

| Variant | Meaning | Partition (`sinks.rs:648`) |
|---|---|---|
| `BuySubmitted` | buy in flight (durable marker, write-ahead journal) | Open |
| `Holding` | entry filled, SOL deployed | Open |
| `ExitPending` | sell in flight | Open |
| `ExitUnconfirmed` | sell may/may-not have cleared, feed never confirmed — engine-terminal, alarmed, never auto-re-sold | **Open** (needs eyes) |
| `End` | confirmed exit fill — terminal | Terminal |
| `ExitFailed` | sell reverted/gave up (nothing sold) **or** entry exhausted (never bought) — terminal, loss booked | Terminal |
| `Arming` | **vestigial** — CHECK-constraint value + `StrategyPosition::new` default, but the engine never persists it (sink overwrites to `BuySubmitted` before first insert). Only legacy rows + `delete_stale_unentered` reference it | — |

### 1.2 ASCII lifecycle

```
                                 BOT PATH
 ┌─────────┐   entry    ┌──────────────┐   fill on   ┌─────────┐   exit    ┌─────────────┐  feed-   ┌─────┐
 │  Armed  │──trigger──▶│ BuySubmitted │────feed────▶│ Holding │──trigger─▶│ ExitPending │─confirm─▶│ End │
 │(no DB   │            └──────┬───────┘             └────┬────┘           └──────┬──────┘          └─────┘
 │ row!)   │                   │ revert ×3 / fatal        │                       │
 └─────────┘                   ▼                          │                       ├─ never confirmed ──▶ ExitUnconfirmed  ◀ DEAD END
                          ExitFailed                      │                       ├─ fatal / revert ×5 ─▶ ExitFailed
                          (never bought —                 │                       └─ stale >300s ───────▶ ExitFailed (blind flip, no bag check)
                           SAME label as                  │
                           a failed sell)                 │                              ExitFailed w/ bag
                                                          │                                │ reaper redrive ×2
              MANUAL BUY (Trade/Wallet page)              │                                ▼
              ───────────────────────────                 │                             PARKED  ◀ invisible in UI
              buy lands on-chain ──▶ ( nothing )  ────────┘                                │ manual only
                                    NO position row,                                       ▼
                                    no TP/SL, no status,                          Retry / Dump / Write off
                                    not on Floor at all
```

### 1.3 Transition table (from → to, trigger, location)

| From (ArmState / status) | To (status) | Trigger / Event | Location |
|---|---|---|---|
| Armed | `BuySubmitted` + `SubmitBuy` | entry cond `can_enter` & caps ok | `reduce.rs:537-572` |
| EntryPending | `Holding` | `FillConfirmed{intent}` matches pend | `reduce.rs:158-181` |
| EntryPending | resubmit (`SubmitBuy`, attempts+1) | `FillFailed` non-Fatal, attempts<3 | `reduce.rs:212-234` |
| EntryPending | `ExitFailed` (no fill) | `FillFailed` Fatal OR attempts≥`MAX_ENTRY_ATTEMPTS`(3); rolls back caps | `reduce.rs:235-249` |
| Entered | `ExitPending` + `SubmitSell` | exit fired (Dead > TP/SL > metric) | `reduce.rs:574-593` |
| Entered | `ExitPending` + `SubmitSell` | `ManualClose` (Sell-All / stop) | `reduce.rs:305-340` |
| Entered | `End` (no sell) | `ExternallyCleared{fill}` | `reduce.rs:342-367` |
| ExitPending | `End` | `FillConfirmed` matches pend | `reduce.rs:182-199` |
| ExitPending | `ExitUnconfirmed` | `FillFailed::Unconfirmed` | `reduce.rs:257-270` |
| ExitPending | `ExitFailed` | `FillFailed::Fatal` OR attempts≥`MAX_EXIT_ATTEMPTS`(5) | `reduce.rs:271-283` |
| ExitPending | resubmit (`SubmitSell`, attempts+1) | `FillFailed::Reverted`, attempts<5 | `reduce.rs:284-296` |
| Armed/PendingFirstSlot/Cooldown | `Disarmed` (no PG row) | `Migrated` before entry | `reduce.rs:369-393` |
| Entered (open) | *(unchanged)* | `Migrated` — rides it out on AMM | `reduce.rs:386-388` |
| ExitPending→End | `Cooldown` or `Done` | re-entry cfg + normal exit reason | `reduce.rs:188`, `rearm_after_close:602-626` |

Sink persistence: `sinks.rs:162-186` (`on_position_update`). Terminal-clobber guard:
`mark_status` uses `WHERE status NOT IN ('End','ExitFailed','ExitUnconfirmed')`
(`strategy_repo.rs:1229`).

### 1.4 Underlying engine `ArmState` (for reference)

`PendingFirstSlot → Armed → EntryPending → Entered → ExitPending → {Done | …}`, plus
`Cooldown` (re-entry) and terminals `Done` / `Disarmed(reason)` (`arm.rs:462-495`).

---

## 2. BUY-failure handling (backend)

Path: `SubmitBuy` effect → `exec_real::run_entry` (`exec_real.rs:184`).

**Write-ahead durability:** `on_signed` hook synchronously appends the signature to the in-RAM
`SubmittedBuyJournal` (`mod.rs:403`) **before** network send, then fire-and-forget
`mark_buy_submitted` PG persist bounded to 400 ms (`exec_real.rs:50, 240-273`). So there is a
durable "buy pending" state = `BuySubmitted`.

**Outcome classification** (`confirm_entry`, `exec_real.rs:417-472`):
- **Fill on feed** → `FillConfirmed` → Holding.
- **Never signed** (`Err(_), None`) → `FillFailed::Reverted` (safe retry).
- **Silent send** (sent, no feed fill): `classify_silent_send` (`exec_real.rs:81`) — Reverted+retryable→Retry;
  2006→refresh creator vault then retry; Succeeded→extended 20 s poll then **Ambiguous**;
  Pending/RPC-err→GiveUp→Ambiguous/Fatal.
- **Migrated during window** → `Fatal` (curve-only snipe can't fill).
- **Ambiguous** → emits *nothing*, leaves `BuySubmitted` row for the reaper
  (`exec_real.rs:404-406`). **This is the cardinal double-buy safety rule.**

Retry cap: `MAX_ENTRY_ATTEMPTS = 3` (`reduce.rs:34`); exhaustion → `ExitFailed` with `fill:None`,
caps rolled back (`rollback_entry:647`). Sink stamps a hypothetical exit price + releases the SOL
commitment (`on_terminal_no_fill`, `sinks.rs:407-436`).

**Partial fill:** `PARTIAL_FILL_THRESHOLD = 0` (`exec_real.rs:68`). Any `token_amount > 0` counts as
a full fill; there is no fractional-fill accounting.

---

## 3. SELL/EXIT-failure handling (backend)

Path: `SubmitSell` → `exec_real::run_exit` (`exec_real.rs:512`). Confirm is **feed-based**
(`confirm_sell` sums own sell legs vs held amount, `exec_real.rs:842`) — no confirm-only RPC
(Helius budget). Up to `SELL_ATTEMPTS = 6` escalating-tip attempts; between attempts
`classify_sell_confirm` + `classify_swap_revert` heal (refresh creator/coin-creator/cashback,
reroute migrated). One sell per mint (shared-ATA `exit_mint` guard, `exec_real.rs:519`).

Terminal emit (`exec_real.rs:720-726`): sigs sent but unconfirmed → `Unconfirmed`;
nothing sent → `Reverted`; structural → `Fatal`. Engine mapping (`reduce.rs:251-297`):
`Unconfirmed`→`ExitUnconfirmed` (never re-sell, alarm); `Fatal`/attempts≥5→`ExitFailed`;
`Reverted`&attempts<5→resubmit.

### 3.1 Reaper (`reapers.rs`, boot tick then every 60 s)

| Reaper | What it does | Location |
|---|---|---|
| `heal_exit_pending_cleared` | ExitPending whose PG net ≤ dust → book End (no phantom sell). Runs before redrive | `reapers.rs:344` |
| `redrive_orphaned_exit_pending` | prefer nudging live engine via `FillFailed::Reverted`; else `spawn_orphan_sell` | `reapers.rs:295` |
| `redrive_exit_failed_bags` | real `ExitFailed` with remaining bag (`find_exit_failed_with_bag`, excludes parked) → `spawn_orphan_sell` + `EXIT_FAILED_BACKOFF_TICKS=5` backoff. `bump_exit_redrive` each time; at **`EXIT_REDRIVE_CAP=2`** → `set_exit_parked(true)` (stop auto-retry so a dead pool doesn't burn tips forever; surfaced for manual decision, **never auto-written-off**) | `reapers.rs:379` |
| `heal_exit_failed_cleared` | ExitFailed whose bag gone → book End | `reapers.rs:450` |
| `fail_stale_exit_pending` | ExitPending older than `EXIT_PENDING_STALE=300s` → ExitFailed (**blind, no bag check**) | `reapers.rs:101` |
| `redrive_orphaned_buy_submitted` | adopt indexed fill→Holding; else drop only if every submitted sig is a confirmed revert; past 600 s → flag manual review (log only) | `reapers.rs:131` |

Park is un-set only by a fresh entry / bag-cleared heal.

### 3.2 orphan_exit module ([orphan_exit.rs](../../live/src/strategies/engine/orphan_exit.rs))

- `spawn_orphan_sell` (`:68`): direct `run_exit` for a PG row not in the live registry; claims
  pg+mint locks (`Busy` if held). `dump=true` → `slippage_bps=None`, min_out=1 (accept dust,
  force-close near-drained pool). Outcome→PG: Confirmed→close/End; Unconfirmed→`mark_exit_unconfirmed`;
  Fatal/Reverted→`mark_exit_failed`.
- `book_externally_cleared` / `book_externally_cleared_pg` (`:233,:249`): close a row from an
  external/manual wallet clear with no sell (folds `ExternallyCleared` if engine-owned, else PG-only).
- `close_siblings_if_mint_cleared` (`:305`): after a mint's bag clears, close every other unsettled
  real row on that mint.
- `reconcile_externally_cleared_holdings` (`:345`): 60 s PG-net sweep of Holding rows whose bag is
  gone — the backstop for external sells.
- All clear detection uses Postgres `trades` net with `BAG_CLEARED_THRESHOLD_RAW = 0` (zero balance
  RPC, Helius budget).

---

## 4. Restart / boot behavior ([decision_loop.rs:191-205](../../live/src/strategies/engine/decision_loop.rs))

1. `boot_recover` (`:580`): replay event-log tail to rebuild **armed** in-RAM state only (effects
   discarded); `held` mint set from `find_open_positions`.
2. `boot_adopt_holdings` (`:606` → `orphan_exit.rs:388`): real+paper `Holding` re-inserted as
   `Entered` (peak/trough reset to entry — conservative) so TP/SL/Dead/Ops-close resume. PG-only.
3. `boot_adopt_buy_submitted` (`:635` → `orphan_exit.rs:490`): real `BuySubmitted` → inert
   `EntryPending` (double-buy guard); advances only on reaper-fed fill/fail, never on a Tick.
4. `boot_seed_episodes` (`:663`): rebuild re-entry episode counters from PG COUNT.

**Per status at startup:** Holding→adopted + reaper cleared-check. BuySubmitted→adopted inert +
reaper adopt/wait/drop. **ExitPending→NOT boot-adopted into engine** (reaper-only heal).
ExitUnconfirmed/End/ExitFailed→terminal; ExitFailed-with-bag re-driven by reaper.

DB-level double-buy backstop: unique index on entry sig-0 in real mode (`0001_init.sql:417`).

---

## 5. Manual-trade API surface

Routes under `/api` ([hunter/live/src/api/mod.rs](../../live/src/api/mod.rs)). There is **no**
snipe endpoint, no floor handler, and **no HTTP reconcile/adopt endpoint** (adopt/reconcile are
background-only).

| Route | Method | Guards | Behavior | Sync/Async | Error surface |
|---|---|---|---|---|---|
| `/solana/wallet/buy` | POST | amount finite/>0/≤`MAX_MANUAL_BUY_SOL`=5.0, valid mint | resolve routing on-chain, pre-buy consolidate, curve/AMM buy w/ ≤3 slippage retries. **Creates NO position row** | **SYNC** (blocks on confirm) | 400 bad input; 500 revert/RPC; **200 `{success:false,pending:true}` on ConfirmTimeout** |
| `/solana/wallet/sell` | POST | valid mint | "Sell All": sweep **every** token account for mint, ≤`SELL_ALL_MAX_PASSES`=3, escalating tip, rent-close; then spawn reconcile (≤6×2s) to book any open Holding closed | SYNC sell legs, ASYNC reconcile | 400 mint; 500 pre-first-leg fail; 200 if `sold_any` or empty |
| `/strategies/{s}/positions/{id}/close` | POST | **mode==real** (else 409); `?action=writeoff` requires status==`ExitFailed`; rejects `End`/`ExitUnconfirmed`; accepts `Holding`/`ExitPending`/`ExitFailed` | registry hit→engine `ManualClose`; miss→`spawn_orphan_sell` or `book_externally_cleared` if net≤0. `dump`→no floor; `writeoff`→book `Dead` no sell | ASYNC sell (202 `{closing:true}`), SYNC book-close no-ops | 404; 409 wrong-mode/terminal/bad-writeoff/Busy/NothingToSell; 500 |
| `/strategy-rules/{id}/stop` | POST | — | force-close all open positions of rule via `spawn_stop_watcher` + `close_rule` + deactivate | ASYNC (202, `action_progress` SSE) | 500 |
| `/strategy-rules/stop-all?mode=` | POST | by mode | force-close every open position of mode + pause rules | ASYNC (202) | 500 |
| `/strategy-rules/{id}/pause` · `pause-all?mode=` | POST | — | entries off, **open positions left to drain** | SYNC | 404/500 |
| `/strategy-rules/{id}/activate·disable·enable` | POST | activate requires `is_enabled` | rule config toggles | SYNC | 400/404/500 |
| `/cashback/claim` | POST | `CLAIM_IN_FLIGHT` atomic guard | sweep both cashback pots to wallet | SYNC | 409 in-progress; 500 |

**Manual buy story:** `manual_buy` never inserts a `strategy_positions` row (no strategy/rule/run
id). Visible only via on-chain balance + `trades` rollup; no auto TP/SL/Dead exit; exit only via
wallet Sell-All. On ConfirmTimeout it returns 200 `{pending:true}` (no signature).

**Coordination:** `close_position` takes engine `inflight` + pg + mint locks. `manual_sell`
(`/wallet/sell`) deliberately does **not** take the engine exit lock — it can race a concurrent bot
exit on the same mint.

**Error surface to FE:** inline alerts, not toasts (Ops `OpsPage.tsx:310,849`; Trade
`TradePage.tsx:114-116`). Async orphan/reconcile failures are **silent to the FE** (server
`tracing::warn` only; reaper heals later). Position events also drive desktop notifications
(`usePositionNotifications.ts`, `desktopNotify.ts`).

---

## 6. Frontend pages & partitioning

### 6.1 Route map ([App.tsx:67-84](../../frontend/src/live/App.tsx))

| Route | Page | Purpose |
|---|---|---|
| `/floor` | `pages/strategies/OpsPage.tsx` | Live book — Waiting/Open/Needs-attention/Recent. `/ops`,`/positions`,`/live-trading` redirect here |
| `/portfolio` | `PortfolioPage.tsx` | cross-rule closed PnL |
| `/wallet` | `MyWalletPage.tsx` | on-chain bag; per-row + manual Buy/Sell |
| `/trade` | `TradePage.tsx` | mint-first execute desk (load chart → Buy/Sell) |
| `/strategies/rules[/:id]` | `RulesPage` / `RuleAnalyzePage` | activate/pause + evidence |

Header `LiveModeControl` = global engine **LIVE/DEAD kill switch** (`/api/system/live`) — unrelated
to the per-page `sseLive` gate that actually enables sells.

### 6.2 `liveStatusSlice.ts` partitioning (SSOT)

- `OPEN_STATUSES = {Arming, BuySubmitted, Holding, ExitPending, ExitUnconfirmed}` → `open` map.
- `TERMINAL_STATUSES = {End, ExitFailed}` → `recentClosed[]` ring, cap `MAX_RECENT_CLOSED=50`.
- `armed` map = waiting/queued, keyed `ruleId|mint`.
- `ATTENTION_STATUSES = {ExitPending, ExitFailed, ExitUnconfirmed}` is defined in the **page**
  (`OpsPage.tsx:53`), not the slice.
- SSE: `applyPositionDelta` ← `strategy_position_update`; `applyArmedDelta` ← `strategy_armed_changed`;
  nil-UUID ignored; snapshot on mount / SSE reopen / tab-visible / `sse_resync`.

### 6.3 Floor tabs → status → action

| Tab | Statuses landing here | Row actions |
|---|---|---|
| **Waiting** | armed (never-fired) | Trade link only |
| **Open** | Arming, BuySubmitted, Holding | Sell (Holding only), Trade link |
| **Needs attention** | ExitPending, ExitUnconfirmed (**not** ExitFailed) | ExitPending: busy; ExitUnconfirmed: **disabled Sell, no action** |
| **Recent** | End, ExitFailed | ExitFailed+real+sseLive: **Retry / Dump / Write off** (`recentCols:729-775`); else `—` |

Sell gating: `canSell = sseLive && mode==='real' && (status==='Holding' || status==='ExitFailed')`.
`!sseLive` → global "Status may be stale — sells disabled" banner.

---

## 7. Case matrix — everything that can happen

### 7.1 Bot BUY

| # | Case | Backend | UI | Verdict |
|---|---|---|---|---|
| B1 | Buy fills | `BuySubmitted`→`Holding` | Open tab | **OK** |
| B2 | Buy reverts ×3 → give up | `ExitFailed` w/ `fill:None`, caps rolled back, hypothetical exit price stamped | Shows in **Recent** as a "failed exit" of a position that never existed; pollutes PnL | **FRAGILE** (overloaded label) |
| B3 | Buy ambiguous (sent, feed silent) | Stays `BuySubmitted`; reaper adopts/waits; after 600 s flagged manual-review **in logs only** | Sits in Open forever, no badge, no action | **GAP** (unresolvable from UI) |
| B4 | Crash/restart mid-buy | Boot-adopt inert arm; journal + unique sig index prevent double-buy | transparent | **OK** |
| B5 | Partial fill | Threshold 0 → any fill = full | invisible | **FRAGILE** (low impact) |

### 7.2 Bot SELL

| # | Case | Backend | UI | Verdict |
|---|---|---|---|---|
| S1 | Exit confirms | `ExitPending`→`End` | Recent | **OK** |
| S2 | Sell reverts → retry (6 tips, cap 5) → `ExitFailed` → redrive ×2 → **parked** | Solid bounded-then-park | Parked is **completely invisible** — no badge/count/tab; Recent caps at 50 so old parked bags fall off the UI entirely | **GAP** (visibility) |
| S3 | Sell never confirmed → `ExitUnconfirmed` | Engine-terminal but partitioned "open"; **no reaper heal exists for it** | Needs-attention with a **disabled Sell and zero actions** | **GAP** (dead end) |
| S4 | ExitPending stale >300 s | Blind flip → `ExitFailed`, **no bag check**; a landed-but-feed-lagged sell booked as a loss (healed later) | transient lie | **FRAGILE** |
| S5 | Crash mid-sell | No boot-adopt for ExitPending; reaper-only recovery | row sits until reaper acts | **FRAGILE** |
| S6 | Rug / drained pool | Slippage sell reverts forever → park; manual Dump/Write-off | Buttons reachable in Recent (07-26 fix); rugged token looks identical to healthy (MTM `—`) | **FRAGILE** (deadness invisible) |
| S7 | Token migrates curve→AMM mid-hold | Rides to AMM silently; sell re-reads durable flag | no migration indicator | **FRAGILE** |

### 7.3 Manual buy

| # | Case | What happens | Verdict |
|---|---|---|---|
| M1 | Trade-page buy succeeds | On-chain fill, **no position row** — untracked, no auto-exit, invisible on Floor | **GAP** (biggest model gap) |
| M2 | Buy confirm times out | BE returns 200 `{pending:true}`; `TradePage.tsx:111` ignores `pending` → logs green "submitted" for a maybe-landed buy | **GAP** (real bug) |
| M3 | Double-click Buy | No double-submit guard (unlike close/cashback) → two on-chain buys | **FRAGILE** |
| M4 | Wallet-page header "Manual Buy"/"Manual Sell" modal | **Broken**: onChange writes stray `mint` key but submit reads `mint_address` (`MyWalletPage.tsx:731,:812`) → always "Enter a mint address". Only row-triggered buys work | **GAP** (real bug) |

### 7.4 Manual sell / external

| # | Case | What happens | Verdict |
|---|---|---|---|
| N1 | Floor close (position-aware) | Coordinated via engine locks; Retry/Dump/Write-off per status | **OK** (07-26 fix) |
| N2 | Wallet "Sell All" while bot is also exiting | Wallet sell takes **no engine lock** → both fire; one lands, other reverts into empty wallet; reconcile/reaper cleans up | **FRAGILE** (race by design) |
| N3 | Sell from Phantom / another app | DB says `Holding` until 60 s reaper sweep; a fast re-buy on the same mint can mask the clear | **FRAGILE** |
| N4 | Sell leaves dust | Returns success; rent reclaimed fire-and-forget | **OK** |

---

## 8. Status → tab → action mismatch (the UX core)

```
 STATUS            LANDS IN                YOU CAN DO           SHOULD BE
 ─────────────    ─────────────────────    ──────────────────   ─────────────────────────────
 Armed             Waiting tab              (watch)              OK
 BuySubmitted      Open tab                 nothing              stuck-buy (B3) needs a resolve action
 Holding           Open tab                 Sell                 OK
 ExitPending       Needs attention          nothing (busy)       OK (in flight)
 ExitUnconfirmed   Needs attention          NOTHING AT ALL       needs verify-on-chain / book-closed / re-sell
 ExitFailed        Recent (as "closed")     Retry/Dump/Writeoff  it is STUCK, not closed — belongs in attention
 ExitFailed+parked Recent, no marker        same, if you find it needs a PARKED badge + dedicated count
 manual buy        NOWHERE                  Wallet Sell-All only needs to exist as a position
```

Downstream symptoms: the Needs-attention tile **undercounts** stuck positions (ExitFailed
excluded); an ExitFailed desktop notification deep-links to the attention tab **where the row isn't**
(`nav.ts:84`); and the Dump/Write-off branch in `openCols` is **dead code** (an ExitFailed row can
never reach the open table).

---

## 9. Full defect list (ranked)

### Outright bugs (fix regardless of any redesign)
1. **Wallet modal `mint` vs `mint_address`** — header Manual Buy/Sell has never worked
   (`MyWalletPage.tsx:731,:812`).
2. **Pending/timed-out buy rendered as green success** on the Trade page (`TradePage.tsx:111`
   ignores `{pending:true}`).
3. **ExitFailed notification deep-links to a tab that can't show the row** (`nav.ts:84`).

### Model gaps (backend decisions — the real redesign)
4. **Manual buys should become positions** (e.g. a `manual` strategy row) so they gain status,
   optional TP/SL, and Floor visibility — collapses the two-worlds split.
5. **Reclassify ExitFailed-with-bag as open/attention** ("stuck", not "closed") — the deferred
   "Option A"; the Recent-tab placement is the root of the 9QLez confusion.
6. **Give ExitUnconfirmed a resolution path** — an on-demand "verify on-chain" action + a
   bag-cleared heal like ExitFailed already has.
7. **Split the overloaded `ExitFailed`** into "buy never filled" vs "sell failed, bag held".

### Visibility gaps (mostly FE)
8. **Parked badge + redrive count + a dedicated "stuck bags" count** that doesn't fall off the
   50-row Recent cap.
9. **Deadness/liquidity indicator on open rows** (a rug should not look like "no data yet").
10. Per-row **mode badge** (real/paper); a `null` mode currently masquerades as real (`modeOk`
    default). Per-row **stale-data cue** when SSE drops (rows keep ticking Age with a dead feed).
    **Persistent manual-trade log** (currently ephemeral, cap 20). **Disentangle the two "LIVE"
    indicators** (engine kill-switch vs SSE state).

### Recommended priority
Items 1–3 are quick independent fixes. Highest-leverage redesign = **4 + 5 together**: "everything
you hold real SOL in is a tracked position, and anything stuck is in Needs-attention with an action"
turns the Floor into a single honest console.

---

## 10. Key source files

- Engine state machine: `hunter/engine/src/{event.rs,reduce.rs,arm.rs}`
- Live side-effects: `hunter/live/src/strategies/engine/{sinks,exec_real,orphan_exit,reapers,decision_loop,mod}.rs`
- Repo / model: `hunter/core/src/models/strategy.rs`, `hunter/core/src/storage/repositories/strategy_repo.rs`
- Migrations: `hunter/core/migrations/{0001_init.sql,0003_part1_realmoney.sql,0012_exit_redrive_park.sql}`
- API handlers: `hunter/live/src/api/{mod.rs,handlers/trading/solana.rs,handlers/strategies/{positions,engine}.rs,handlers/trading/cashback.rs}`
- Frontend: `hunter/frontend/src/live/{App.tsx,nav.ts,slices/liveStatusSlice.ts,store/liveEndpoints.ts,pages/strategies/OpsPage.tsx,pages/trade/TradePage.tsx,pages/profiles/MyWalletPage.tsx,pages/profiles/walletColumns.tsx}`
