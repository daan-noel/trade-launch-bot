# Real-trade position lifecycle

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
There is no `ExitFailed` and no `Arming` status — if you see either in an old query or
fixture it maps to `EntryFailed` (entry NULL) or `ExitStuck` (everything else).

| Status | Partition | Meaning |
|---|---|---|
| `BuySubmitted` | Open | buy in flight; stale >600 s (`BUY_SUBMITTED_REVIEW_SECS` SSOT) surfaces `needs_review` via API + SSE |
| `Holding` | Open | entry filled, SOL deployed (mid-ladder scale-out stays `Holding` after a partial fill; banked fraction is `sold_token_amount` / ledger — see `position_fills` + [../plans/strategies/partial-exits.md](../plans/strategies/partial-exits.md)) |
| `ExitPending` | Open | sell in flight (partial **or** full; portion sized at exec from `Portion`) |
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
- **An entry retry re-clears the entry gate** (`entry_enabled && can_enter`) at the
  failure's `at`, not just the `Fatal`/attempts/lamports checks. A retry is a new
  buy and the decision that authorized attempt 1 is only as fresh as attempt 1 —
  confirming a revert takes seconds, and `can_enter` is otherwise reachable only
  from `decide_arm`'s `Armed` branch, which a retry never passes through. This is
  why `Event::FillFailed` carries `at` at all (it was the one event with no clock;
  `None` = a pre-`at` JSONL line, replayed unqualified).
  Failing the re-check terminates the entry exactly like attempt exhaustion —
  safe, because `FillFailReason::Reverted` is only ever emitted on a **proven**
  non-fill (`exec_real`'s `EntryOutcome::Retry` needs a confirmed on-chain revert;
  anything uncertain is `Ambiguous`, which emits nothing and leaves the row for the
  reaper). Manual episodes have no rule row and keep retrying — they bypass entry
  conditions by design.
  Failing the re-check is **not** terminal: with attempts still on the ladder and
  the rule still loaded, the arm goes back to `Armed` (counters rolled back, row
  booked `EntryFailed`) so the ONE gate re-decides on the next trade/tick. That is
  also how the gates a retry cannot express get applied — `exclusive` means *wait*
  in `decide_arm`, not *give up*, and `dead` / `entry_unsatisfiable` disarm
  properly. Only an exhausted ladder or `Fatal` ends in `Done`, so re-arming
  cannot spin: re-entry has to clear the full gate again, exactly like a first
  entry.
  The **copycat guard** needs no re-check on this path and must not get one: the
  entry attempt already recorded its own `(name, symbol)`, and the guard exempts
  the mint that recorded it — a retry on the same mint is by construction never
  blocked. Blocking it would mean an entry poisoning its own retry ladder.
- The pre-submit SOL guards in `dispatch_buy` (balance floor, `max_committed_sol`)
  emit **`Fatal`**, not `Reverted`. The engine's retry is immediate, so `Reverted`
  did not mean "retry when free SOL returns" — it meant re-running the same guard
  twice more within microseconds, three PG writes deep, against a wall that cannot
  have moved. `Reverted` is reserved for a buy that reached the chain and provably
  did not fill.

Reaper (`reapers.rs`, boot + 60 s):
| Sweep | Behavior |
|---|---|
| `resolve_buy_submitted` (shared with Verify) | adopt indexed fill → Holding; drop only when EVERY sig confirmed-reverted; else wait. Past review window → `needs_review` SSE flag |
| `heal_exit_pending_cleared` | ExitPending net ≤ dust (+ sell on record) → book End (runs BEFORE redrive) |
| *(all "bag gone → book End" sweeps)* | price the leg from the feed, **healing it from the row's exit sigs first**; unresolvable → park, never a zero-proceeds close (§2.1b) |
| `redrive_orphaned_exit_pending` | nudge engine / orphan sell |
| `redrive_exit_stuck` | ExitStuck-with-bag (`find_bags_by_status`, excl. parked) → orphan sell + backoff; at cap → `park_or_recover` |
| `heal_cleared_by_status("ExitStuck"/"ExitUnconfirmed")` | bag gone → book End (the ExitUnconfirmed heal that never existed pre-split) |
| `redrive_exit_unconfirmed` | ExitUnconfirmed-with-bag → **only if every recorded sell sig is provably dead** (`burn_nonce_tx`) → orphan sell; else left for manual Verify. Runs AFTER its heal |
| `mark_stale_exit_pending_stuck` | real ExitPending >300 s: bag-check first — heal-eligible rows are left to the heal; the rest flip `ExitStuck` (never a blind terminal stamp) |
| `close_stale_paper_exit_pending` | paper crash artifact → book End at entry (breakeven) |
| `close_paper_exit_stuck` | **paper** ExitStuck → book End at the feed price as of `updated_at` (else cache spot, else entry). No bag ⇒ nothing to redrive; every real sweep above is `mode='real'`, so these had no owner at all (§2.2) |

### 2.1b A cleared bag is proof a sell landed — never book it at zero

Every "bag gone → book End" sweep above prices its closing leg from
`orphan_exit::fill_from_latest_sell`, i.e. from the `trades` feed. When the trigger is
the **on-chain** bag check (`reconcile_bag_onchain` → `OnChainBag::Cleared`), that feed
has already been shown to be wrong: an empty wallet with no sell row means the sell
landed and the feed missed it. Asking it for the proceeds anyway returns nothing, and a
zero-proceeds `End` is a permanent −100% stamped on a position that may have won.

Two rules close that:

- **Heal the feed, don't price around it.** `fill_from_latest_sell` first re-fetches the
  row's own `exit_tx_signatures` (`sell_backfill::heal_missing_sell_legs`: one batched
  `getTransaction` → `rpc_to_protobuf` → the ONE ingest decoder → `trades`). Repairing
  `trades` fixes the bag net, the sibling close and the token's chart in the same write,
  and keeps the healed fill on the same `user_quote_amount_out` convention as every
  feed-confirmed one. `exec_real`'s extended-poll timeout does the same before giving up,
  but only when the RPC says the signature **succeeded**.
- **Unresolvable ⇒ park, never zero.** `book_cleared_or_park` books `None` as
  `exit_parked` for a manual decision. The one honest zero is a position with no
  remainder left to sell (a finished scale-out ladder).

A row that reaches these paths also keeps **its own** `exit_reason`; `"Manual"` is the
fallback for a row that has none, which is what `ExitCode::Manual` means. Stamping it
over a rule's reason claims a human sold.

The AMM feed is where this bites: a pool's swaps only reach `trades` once ingest is
subscribed to it, so our own first sell into a freshly graduated pool can land seconds
before the feed carries it.

### 2.2 Paper needs its own `ExitStuck` owner

`ExitStuck` means "the sell gave up, the bag is still held" — a real-only premise, and
every recovery query above filters `mode = 'real'`. A **paper** row that reaches it has no
owner and stays open forever unless `close_paper_exit_stuck` claims it.

Paper reaches it easily: a `Dead` exit fires *because* the token stopped printing, so its
fill window is empty by construction — with `market_fill_on_empty_window = false` each of
the engine's 5 retries re-fires against the same last trade, all 5 time out, `ExitStuck`.
Same for a manual close on a mint that has aged out of the cache. `exec_paper` therefore
market-fills like analysis (lab replay/simulate and the sweep both pass `true`), falling
back to the token's last known spot when no window can price it.

**The bias is one-directional, which is why this is not cosmetic:** the stranded rows are
exactly the dead-token losers, so paper PnL reads high while they sit open.

Healing is the reaper's job and only the reaper's — no one-shot script. Full incident:
[`@history/2026-08-05-paper-exitstuck-backlog.md`](../history/2026-08-05-paper-exitstuck-backlog.md).

**Size the closing leg from cost basis × price ratio, never `price × tokens`.** A price
*ratio* is scale-free; `price × tokens` books a 1e6× fantasy PnL (and can overflow
`bigint`) against any row whose `entry_token_amount` carries the old token scale, while
`entry_lamports` was always right. See
[`@history/2026-08-04-token-scale-1e6-pnl.md`](../history/2026-08-04-token-scale-1e6-pnl.md).

### 2.3 What "Stop" actually waits on

`POST /api/strategy-rules/{id}/stop` (and `stop-all?mode=`) returns **202 + `action_id`**
immediately; the closes stream over `action_progress` SSE and the Rules row shows
"Stopping n/N" until a non-`running` frame arrives
([action_progress.rs](../../live/src/api/handlers/strategies/action_progress.rs)).
Three properties that a "stop takes forever" report should be checked against:

- **The watch set is the statuses the engine will emit *again* for** — `BuySubmitted`,
  `Holding`, `ExitPending`, via the ONE decider `stop_in_flight`. `find_open_positions`
  is "not End/EntryFailed", so it also returns `ExitStuck`/`ExitUnconfirmed`: open rows
  in the attention lane, but engine-terminal — `CloseRule` never touches them and no
  further frame ever fires. Counting them made `total` unreachable, which is how a
  paper rule holding any §2.2 row parked the spinner permanently. Locked by
  `sinks::status_partition_guard::stop_watch_set_is_exactly_the_statuses_the_sink_still_emits_for`.
- **Postgres is the authority; SSE is only the fast path.** The watcher re-reads its
  rows every 3 s, on every `Lagged`, and once more at the deadline. It shares the
  512-slot broadcast bus with one frame *per ingested trade*, and a terminal frame is
  emitted exactly once, so a dropped frame is unrecoverable from SSE alone. Never
  re-derive completion from the stream here.
- **It gives up out loud** (180 s → `partial`/`failed` naming the stranded count)
  rather than spinning. Anything still open then is the reaper's, not the stop's.

The engine side is *not* where the time goes: `CloseRule` folds every `ManualClose` in
one batch and each `SubmitSell` is spawned, so N positions close concurrently — a paper
exit is bounded by `exec_paper::FILL_WAIT` (2 s) and retries fire immediately.

### 2.1 Why a stranded bag needs more than "retry"

Three invariants. Each has produced stranded **real** bags when violated, and each is
closed at its source — none is defensive-programming filler.

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
derivation). Do **not** re-confirm it via `refresh_curve_facts`: its `Err(_)` falls
through to `Fatal`, so an RPC blip strands a token that is merely migrated and
perfectly sellable. The latch is a **loop-local** `route_migrated`, not the token
cache: `get_mut` returns `None` on an aged-out entry, which is exactly the long-hold
case, so a cache-only flip is silently lost.

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

Without this column an `EntryFailed` row explains nothing: `reduce.rs` emits the terminal
delta with `reason: None` (there is no `ExitReason` for a position that never opened), so
the only facts that *do* explain it — the `TradeError` from the send, and the Anchor
custom code from the on-chain revert — must be captured by the real-exec adapter or they
are gone. What that buys you is the ability to tell a **slippage** revert (6002/6042 ⇒
the buy floor is too tight for the market, a *tuning* fix) from a structural one without
pulling container logs.

`last_entry_error` is now written at every buy give-up / retry point:

| Path | Recorded cause |
|---|---|
| pre-send migrated skip | `skipped before send: token already migrated (curve-only snipe)` |
| send returned `Err` and nothing was signed | `buy send failed: <TradeError>` |
| send exceeded `BUY_SEND_TIMEOUT` (20 s) | `buy send timed out after 20s` |
| confirmed on-chain revert | `reverted on-chain, curve buy error <code>` — the code is the point |
| revert with no Anchor code | `reverted on-chain, no Anchor code (account / funds error)` |
| 2006 stale creator | refreshed-and-retrying, or `creator vault is unchanged` |
| migrated during the buy window | `token migrated during the buy window (…)` |

Semantics: **"the most recent buy attempt that did not fill"**, whatever the row's
final status — not an `EntryFailed`-only field, and never cleared on success. A
`Holding` row reading `reverted 6002` entered on a later attempt, and that is useful
history. `Ambiguous` outcomes record nothing (the row stays `BuySubmitted` for the
reaper; there is no verdict yet).

**Every exit from the buy dispatch path must emit a fill event.** A `BuySubmitted`
row is durable *before* the send, so a `dispatch_buy` / `run_entry` path that
returns without a `FillConfirmed`/`FillFailed` strands the arm in `EntryPending`
and the row in `BuySubmitted` **forever** — it holds its `max_concurrent_tokens`
slot for the life of the process, and past it, since boot re-adopts the row as an
inert arm. Use `decision_loop::fail_entry` (`Fatal` for structural causes,
`Reverted` where a later attempt can succeed); never a bare `return`. Two independent
guards back this up — the bounded `BUY_SEND_TIMEOUT` upstream (an unbounded send is what
parks `run_entry` with nothing emitted), and the reaper's no-signature drop below.
Incident: [`@history/2026-08-02-unemitted-fill-leaks-slot.md`](../history/2026-08-02-unemitted-fill-leaks-slot.md).

**A `BuySubmitted` row with zero signatures provably sent no transaction**, so it
cannot own tokens and the reaper drops it past `UNENTERED_STALE` (600 s) exactly
like the all-reverted case (`reapers::drop_buy_submitted`, the ONE drop path). This
is the only place real `BuySubmitted` is dropped without proving reverts — sound
because the "it might hold a bag" premise that spares real rows from the paper
stale-drop does not apply when no tx exists. A row **with** a signature is
untouched: it still waits for adopt-or-all-reverted, forever if need be.

Mechanically it follows the `exit_redrive_count` / `exit_parked` pattern (§2.1, mig
0012): a **dedicated column, deliberately absent from `update_position`'s SET list**,
because the executor writes it at the moment of failure and the sink's full-row
terminal write lands *after* — a shared write path would clobber it.
`note_last_entry_error` is the ONE writer; it returns `false` when no row matched
(`insert_position` is async, so the pre-send skip can outrun its own insert) and the
caller retries briefly, exactly as `mark_buy_submitted` does. The whole path is
best-effort: a failed diagnostic write never changes the entry's outcome.
Locked by `position_col_guard::writer_owned_columns_never_enter_the_full_row_write`,
which also guards the 0012 columns.

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
- **Recording the copycat guard at the fill instead of the attempt.** A buy that
  reverts on a copycat is exactly the trap worth remembering, and confirming a revert
  is slow (12.3 s measured) — long enough for the next re-launch to arrive. The record
  is written at `ArmDecision::Enter` and `rollback_entry` deliberately leaves it.
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
- Schema: `hunter/core/migrations/0001_init.sql` (`strategy_positions` — the status
  domain, `origin`/`manual_exit`, `exit_redrive_count`/`exit_parked`)
- API: `hunter/live/src/api/handlers/strategies/positions.rs`,
  `handlers/trading/solana.rs` (sell lock)
- FE: `live/pages/console/ConsolePage.tsx`, `live/slices/liveStatusSlice.ts`,
  `shared/lib/strategy/nav.ts`
