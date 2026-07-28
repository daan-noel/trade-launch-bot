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
| `ExitUnconfirmed` | Open · attention | sell may/may-not have cleared; auto-re-sold **only** once the original is proven unexecutable (§2.1); bag-cleared reaper heal + manual Verify |
| `End` | Terminal | confirmed exit (incl. `Dead` write-off) |
| `EntryFailed` | Terminal | buy never filled; **no hypothetical exit price stamped; excluded from realized PnL** (entry NULL). Carries no `exit_reason` (nothing exited) — the cause is in `last_entry_error`, §2.2 |

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
| `redrive_exit_stuck` | ExitStuck-with-bag (`find_bags_by_status`, excl. parked) → orphan sell + backoff; at cap → `park_or_recover` |
| `heal_cleared_by_status("ExitStuck"/"ExitUnconfirmed")` | bag gone → book End (the ExitUnconfirmed heal that never existed pre-split) |
| `redrive_exit_unconfirmed` | ExitUnconfirmed-with-bag → **only if every recorded sell sig is provably dead** (`burn_nonce_tx`) → orphan sell; else left for manual Verify. Runs AFTER its heal |
| `mark_stale_exit_pending_stuck` | real ExitPending >300 s: bag-check first — heal-eligible rows are left to the heal; the rest flip `ExitStuck` (never a blind terminal stamp) |
| `close_stale_paper_exit_pending` | paper crash artifact → book End at entry (breakeven) |

### 2.1 Why a stranded bag needs more than "retry"

Three invariants the 2026-07-28 review established. All three had produced stranded
real bags; each is now closed at its source.

**A durable-nonce sell never expires.** If it doesn't land, `schedule_nonce_refresh`
re-arms the slot with the *same* hash, so it stays executable indefinitely. Blind
resending can therefore land twice, and against a token account shared with a sibling
position the second sell eats the sibling's bag. `Engine::{note_nonce_tx,
nonce_tx_state, burn_nonce_tx}` (executor-core `nonce.rs`) make this decidable: the
`signature → (nonce_account, hash)` index proves a tx is dead once the nonce moves
past its hash, and `burn_nonce_tx` advances the nonce to *make* it dead. `Unknown`
(untracked / aged out across a restart) is never treated as dead — those rows still
go to manual Verify.

**6005 `BondingCurveComplete` is proof of migration, not a hint.** The exit loop
latches the AMM route on it and retries with zero RPC (the pool is a pure PDA
derivation). It used to re-confirm via `refresh_curve_facts`, whose `Err(_)` fell
through to `Fatal` — an RPC blip stranded a token that was merely migrated and
perfectly sellable. The latch is a **loop-local** `route_migrated`, not the token
cache: `get_mut` returns `None` on an aged-out entry, which is exactly the long-hold
case, so a cache-only flip was silently dropped.

**A position's token account is a fact about its own buy.** `SnipeBuy` returns the
account the buy funded and the position records that. Reading it back from the
trader's per-mint `user_token_accounts` cache was wrong: two concurrent snipes on one
mint each draw their own seeded account, then both write that one key — the loser
persisted its sibling's account and its exit sold from an account holding none of its
tokens. A retry for the same position now also passes its known account back as the
buy override, so one position can never split its bag across two accounts.

Sharing one account across sibling positions is the *intended* shape (each sells its
own `token_amount`; `has_other_open_position_on_mint` gates the rent-reclaim close so
neither closes it under the other), and a buy with nothing recorded reaches for it via
`find_reusable_token_account` — but only after `cached_token_account` proves the mint
has been traded in-process, so a fresh-mint snipe never pays the query. Best-effort:
a cold cache after restart just draws a new seeded account, which stays correctly
attributed. Accounts that accumulated anyway are folded back with
`probe consolidate-dryrun <mint> --into <the position's account> --execute`.

Recovery of rows written before these fixes (and any future divergence) is the
opt-in `EXIT_BAG_ONCHAIN_CHECK` (default **off** — Helius spend): before parking,
one `getTokenAccountsByOwner` either books a row whose bag is genuinely gone (feed
gap) or re-points it at the account actually holding the tokens and resets the
redrive budget. PG-net alone cannot see either case.

### 2.2 Why a buy failed — `last_entry_error` (mig 0017)

An `EntryFailed` row used to explain nothing. `reduce.rs` emits the terminal delta
with `reason: None` (there is no `ExitReason` for a position that never opened), and
the real-exec adapter discarded the one fact that *did* explain it: the `TradeError`
from the send — not even logged — and the Anchor custom code from the on-chain revert.
On 2026-07-27 that cost a log-dig: 9 `EntryFailed` rows in an 8 h window, all with
zero on-chain buys, with no way to tell a **slippage** revert (6002/6042 ⇒ the buy
floor is too tight for the market — a *tuning* fix, and ~27 landed reverts of burnt
fees hinge on it) from a structural one.

`last_entry_error` is now written at every buy give-up / retry point:

| Path | Recorded cause |
|---|---|
| pre-send migrated skip | `skipped before send: token already migrated (curve-only snipe)` |
| send returned `Err` and nothing was signed | `buy send failed: <TradeError>` |
| confirmed on-chain revert | `reverted on-chain, curve buy error <code>` — the code is the point |
| revert with no Anchor code | `reverted on-chain, no Anchor code (account / funds error)` |
| 2006 stale creator | refreshed-and-retrying, or `creator vault is unchanged` |
| migrated during the buy window | `token migrated during the buy window (…)` |

Semantics: **"the most recent buy attempt that did not fill"**, whatever the row's
final status — not an `EntryFailed`-only field, and never cleared on success. A
`Holding` row reading `reverted 6002` entered on a later attempt, and that is useful
history. `Ambiguous` outcomes record nothing (the row stays `BuySubmitted` for the
reaper; there is no verdict yet).

Mechanically it follows the `exit_redrive_count` / `exit_parked` pattern (§2.1, mig
0012): a **dedicated column, deliberately absent from `update_position`'s SET list**,
because the executor writes it at the moment of failure and the sink's full-row
terminal write lands *after* — a shared write path would clobber it.
`note_last_entry_error` is the ONE writer; it returns `false` when no row matched
(`insert_position` is async, so the pre-send skip can outrun its own insert) and the
caller retries briefly, exactly as `mark_buy_submitted` does. The whole path is
best-effort: a failed diagnostic write never changes the entry's outcome.
Locked by `position_col_guard::writer_owned_columns_never_enter_the_full_row_write`,
which also guards the 0012 columns (previously untested).

## 3. Close-action matrix (backend-enforced)

`POST /api/strategies/{s}/positions/{id}/close?action=retry|dump|writeoff|verify`
([handlers/strategies/positions.rs](../../live/src/api/handlers/strategies/positions.rs)):

| Status | retry/sell | dump | writeoff | verify |
|---|---|---|---|---|
| Holding | ✓ (engine ManualClose / orphan) | — | — | — |
| ExitPending | 409 busy | — | — | — |
| ExitStuck | ✓ (un-parks: `unpark_exit` resets count) | ✓ | ✓ (`Dead`, manual-only forever) | — |
| ExitUnconfirmed | ✓ re-sell (safe: landed original → NothingToSell → booked cleared) | ✓ | ✓ | ✓ PG-net check → heal or `{still_held}` |
| ExitUnconfirmed (reaper, unattended) | ✓ **only** when every recorded sell sig is `NonceTxState::Dead` — see §2.1 | — | — | — |
| BuySubmitted | 409 (use verify) | — | — | ✓ reaper adopt-or-drop, one shot |
| End / EntryFailed | 409 terminal | — | — | — |

### 3.1 Recovering bags the reaper will not take

`find_bags_by_status(status, threshold_raw)` — the ONE stranded-bag query — filters
`NOT p.exit_parked`, so a **parked** `ExitStuck` row is invisible to the reaper by
design (parking IS the give-up state; auto-unparking would loop). Two ways out:

- **Per row:** a manual `retry`, which calls `unpark_exit` and resets the counter.
- **Per mint, sweeping:** `POST /api/trading/sell-all-by-mint` — the "Sell All by mint"
  action in MyWallet ([trading/solana.rs](../../live/src/api/handlers/trading/solana.rs)).
  It enumerates **every** token account for the mint, sells each using its own address
  as the override, re-resolves curve-vs-AMM routing live per pass, and closes each
  cleared account for rent. That covers both the orphaned-account case and a missed
  migration in one shot, and needs no deploy. The reaper's `heal_cleared_by_status`
  books the rows to `End` once the sells hit the feed.

### 3.2 Deliberately NOT done — re-check before "fixing" these again

Each of these looks like an obvious improvement and is a regression:

- **Emitting a re-queue event from the two silent `run_exit` guard returns.** The
  reaper's `redrive_orphaned_exit_pending` already owns that row at 60 s; emitting
  immediately would spin `MAX_EXIT_ATTEMPTS` out in milliseconds. Strictly worse.
- **Consolidation inside the reaper's unattended recovery path.** For real stranded
  bags each position's whole bag sat in ONE account and the row merely pointed at the
  wrong one — `reconcile_bag_onchain` re-pointing recovers 100%. A token-transfer tx
  inside unattended recovery is new risk against a split-bag case with no evidence
  behind it.
- **Turning on `exclusive` for the real rules.** Multiple rules per mint is a strategy
  choice and the executor is now safe under it (§2.1). Enabling it would cut coverage
  to hide a bug that is fixed.
- **Any new Helius spend on a default path.** `EXIT_BAG_ONCHAIN_CHECK` and the
  consolidation sender are both opt-in / operator-invoked. Keep it that way (standing
  rule in the super-root `CLAUDE.md`).

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
