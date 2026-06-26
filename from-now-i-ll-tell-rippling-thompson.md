# TPSL Real-Trading Bug Fixes — Consolidated Plan

## Context

While running TPSL1/TPSL2 rules in **real trading mode**, the user hit a series of
bugs. This file collects each reported error, its root cause (traced in source),
and the fix, so the whole batch can be executed in one follow-up session.

> Convention: TPSL1 = `tpsl_sniper_1`, TPSL2 = `tpsl_sniper_2`. These two modules
> are intentional clones — a fix in one almost always belongs in both
> (`tpsl-clones-intentional`).

---

## Error 1 — Concurrency caps ignored in real mode (bought 10+ tokens with caps = 2)

**Report:** Set `Max Concurrent Tokens = 2` and `Max Total Tokens = 2` on a TPSL1
rule, switched to real mode. It bought 10+ tokens, ignoring both caps.

### Root cause

The cap **check** and the cap **counter** are out of sync across the real-buy
latency window:

- **Check:** [service.rs:237-257](backend/src/strategies/tpsl_sniper_1/service.rs#L237-L257)
  reads `holding_count_by_rule` / `total_count_by_rule`.
- **Counters only bump on fill:** in `sync_position`, the cap counters increment
  only when `entry_price.is_some()` —
  [runtime_cache.rs:722-741](backend/src/strategies/tpsl_sniper_1/runtime_cache.rs#L722-L741).
- **The "inline claim" doesn't claim a cap slot:** the
  `sync_position(None, &position)` at
  [service.rs:308](backend/src/strategies/tpsl_sniper_1/service.rs#L308) runs on a
  fresh position with `entry_price = None`, so it only inserts into the *holding
  index* (exit-gating), and does **not** move the cap counters. The comment at
  service.rs:298-308 claims it reserves the slot — it does not.

Result: real buys take seconds to fill; until they do, every ping in a launch wave
reads count = 0, passes the cap, and submits a buy → far more than the cap.

**Why real-only:** paper mode fills the entry from the in-memory cache almost
instantly, so the counter bumps before the next ping. Real on-chain fill latency
leaves the counter stale and the cap wide open.

**Scope:** present in both TPSL1 and TPSL2 (identical counter logic).

### Fix (recommended)

Count **in-flight** positions against the cap, not just filled ones. Add a
reserved/in-flight counter:

- Bump it **inline** at the claim ([service.rs:308](backend/src/strategies/tpsl_sniper_1/service.rs#L308)).
- Release it on buy-fail rollback (service.rs:332-336 and 373-385).
- Cap check sums `reserved + holding` (and reserved + total for the total cap).

Alternative: gate the cap check on the per-rule holding-index size, which already
includes `Arming` / `BuySubmitted` states ([position.rs:256-261](backend/src/models/position.rs#L256-L261)).

Apply to **both** TPSL1 and TPSL2.

---

## Error 2 — Successful real buys never become "Holding" (10+ tokens bought on-chain, 0 saved)

**Report:** 10+ tokens had buy tx sent; the real buys **succeeded on-chain**
(confirmed on GMGN), but **none** were saved as `Holding`.

### How a buy becomes Holding (normal path)

The real buy is confirmed **only via the `trades` gRPC feed**, never RPC:

1. `buy_until_filled_or_give_up` submits the buy, persists the signature, and flips
   the row to `BuySubmitted` via the write-ahead `on_signed` hook
   ([real.rs:356-357](backend/src/strategies/tpsl_sniper_1/execution/real.rs#L356-L357)).
2. It then polls the feed: `poll_feed_until_entry_fill` →
   `adopt_existing_fill_if_present` →
   `trade_repo.find_fill_by_signature(wallet, mint, sig)`
   ([real.rs:469-555](backend/src/strategies/tpsl_sniper_1/execution/real.rs#L469-L555)).
   On first sight of a matching `trades` row it calls `update_entry` → `Holding`.
3. **Adoption window is short:** `BUY_POLL_MAX_ATTEMPTS = 12 × BUY_POLL_INTERVAL_MS = 1000ms`
   = **~12 s** per attempt ([execution/mod.rs:14-16](backend/src/strategies/tpsl_sniper_1/execution/mod.rs#L14-L16)).
   On timeout it does one on-chain status check
   ([real.rs:415-449](backend/src/strategies/tpsl_sniper_1/execution/real.rs#L415-L449)):
   - landed-but-not-indexed → `WaitThenSettle`: polls **one more ~12 s window**, then
     returns the position still **unentered**.

### Root cause

Matching itself is fine (wallet + mint + exact submitted signature) and works in
normal operation — what fails here is **timing under overload**, compounded by an
**over-aggressive inline cleanup**:

1. **Overload (caused by Error 1).** Error 1 fired **10+ simultaneous real buys** on
   the single EC2 box (2 vCPU / 4 GB, IO-bound). The `trades` feed → DB-writer →
   index pipeline fell behind, so the bot's own buy rows weren't queryable within the
   ~12 s (×2) adoption window. Every buy timed out unentered.

2. **Inline cleanup deletes landed-but-unindexed positions.** When the buy task
   returns unentered, the spawned task immediately deletes the position:
   [service.rs:373-385](backend/src/strategies/tpsl_sniper_1/service.rs#L373-L385)
   ```rust
   if let Ok(Some(pos)) = position_repo.find_by_id(position_id).await {
       if pos.entry_price.is_none() {          // BuySubmitted, fill not yet indexed
           trader.release_sol_for_position(...);
           let _ = position_repo.delete_position(position_id).await;  // ⚠️ orphans tokens
           runtime.remove_position(&pos);
       }
   }
   ```
   This deletes **any** unentered position — including the `WaitThenSettle` case where
   the buy **provably landed on-chain**. The tokens are now held in the wallet with no
   position tracking them → never marked Holding, never sold.

3. **It pre-empts the safe reaper.** The periodic reaper `redrive_orphaned_buy_submitted`
   ([service.rs:926-1004](backend/src/strategies/tpsl_sniper_1/service.rs#L926-L1004),
   ticked every interval at service.rs:131) is specifically designed to **never delete
   a `BuySubmitted` row that might own tokens** — it re-runs `adopt_existing_fill_if_present`
   once the row finally indexes, and drops only if **every** submitted signature is a
   *confirmed revert*. But the inline cleanup (#2) has already deleted the row, so the
   reaper never sees it. The inline path directly contradicts the reaper's safety model.

**Net:** genuinely-successful buys → unentered past the window → deleted inline →
bought tokens orphaned. This is why **all** of them failed, not a random few.

### Fix (recommended)

1. **Remove the destructive inline cleanup** at
   [service.rs:373-385](backend/src/strategies/tpsl_sniper_1/service.rs#L373-L385) — or
   gate it to delete **only** when every submitted signature is a confirmed on-chain
   revert (mirror the reaper's `classify_submitted_buy` / `BuyRecoveryVerdict` check).
   Leave landed-or-unknown buys as `BuySubmitted` and let the periodic
   `redrive_orphaned_buy_submitted` reaper own adopt/wait/drop (single responsibility).
2. **Add `release_sol_for_position` to the reaper's confirmed-revert drop branch**
   ([service.rs:983-988](backend/src/strategies/tpsl_sniper_1/service.rs#L983-L988)).
   Confirmed missing: the reaper calls `delete_position` + `remove_position` but never
   releases the SOL commitment. The inline cleanup (service.rs:378) was the only call
   site — once it is removed, every confirmed-revert handled by the reaper leaks
   committed SOL. The bot's budget tracker then believes SOL is reserved even though the
   wallet balance is intact, and the bot eventually refuses all new buys until a restart.
   Fix: call `self.trader.release_sol_for_position(&position.id.to_string()).await`
   immediately before or after `remove_position` in the revert-drop branch.
3. **Fixing Error 1 removes most of the trigger** — without 10+ concurrent buys the
   indexing pipeline keeps up and the ~12 s window is sufficient. The two fixes are
   complementary: Error 1 stops the overload, Error 2 stops orphaning when a buy does
   outrun the window.

**Scope:** both TPSL1 and TPSL2 (clones — same inline-cleanup and reaper logic).

---

## Error 3 — Bot sniped stale/dead tokens (created 10+ hours ago, no trades since)

**Report:** The bot bought tokens — e.g.
`4syqCLagMxUKYWTjFuz2aUigzV7gefqhLgsGSFjJpump` and
`9eAKH9JrQxepX5Q73zwrhimkvGwRhWS6NvwS5jGkpump` — that were **created 10+ hours
ago and had no trade history since creation**. A sniper should only buy fresh
launches.

### Root cause — no freshness/age gate on the entry path

The buy decision matches **only the token's creation-transaction properties**, with
**no token-age criterion**:

- `on_token_created` ([service.rs:177-201](backend/src/strategies/tpsl_sniper_1/service.rs#L177-L201))
  delegates the match to `find_all_matching_buy_rules`
  ([entry/mod.rs:63-81](backend/src/strategies/tpsl_sniper_1/entry/mod.rs#L63-L81)).
- The criteria list ([entry/mod.rs:36-43](backend/src/strategies/tpsl_sniper_1/entry/mod.rs#L36-L43))
  is `initial_buy_sol`, `cu_limit`, `cu_price`, `max_sol_cost`, `spendable_sol_in`,
  `instruction_labels` — all immutable properties of the **create tx**. None depend
  on the token's age or whether it has any post-create activity.

So **any** `TokenCreated` event that structurally matches the rule is bought, whether
the token is 2 seconds or 10 hours old. The `Time` / `Stall` / `Liq` params are *exit*
controls, not entry filters.

`token.created_at` is the genuine on-chain create `block_time`
([create.rs:164,178-184](backend/src/ingest_laserstream/decoder/create.rs#L164)),
so the strategy *has* the data to reject stale creates — it just never checks it.

### The trigger — stale creates delivered late

Activation and boot-seeding were both ruled out as the source: rule activation only
flips `is_active` + reloads rules (no cache rescan, no pings); boot seeding loads
tokens into `token_cache` without pinging the strategy. The **only** source of a
`TokenCreated` ping is live ingest
([pipeline.rs:446](backend/src/ingest_laserstream/pipeline.rs#L446)).

The bot received a batch of **stale `TokenCreated` events** via the **live ingest
reconnect replay** (Mechanism A — see Error 5). `client.rs` replays
`from_slot = last_slot+1` on reconnect, up to Helius's ~24 h window, and routes those
replayed events straight through the normal pipeline →
`ping_strategy(TokenCreated)` → buy path
([client.rs:180-184,300-310](backend/src/ingest_laserstream/client.rs#L180-L184),
[pipeline.rs:446](backend/src/ingest_laserstream/pipeline.rs#L446)). After the
overload from Errors 1+2 (10+ concurrent buys + sell loops saturating the 2 vCPU /
4 GB box) the stream dropped and reconnected, replaying a backlog of old-slot creates.
With no freshness gate the strategy treated each as fresh, and with the cap broken
(Error 1) it bought every match.

> **Note:** the **token_sync backfill** (Mechanism B, the "Fetch All/Fetch New"
> button) is **NOT** the trigger — it never pings the strategy and cannot cause a buy
> (see Error 5). Only Mechanism A reaches the buy path.

### Fix (recommended)

1. **Add a freshness criterion** to the entry path: reject the buy when
   `Utc::now() - token.created_at > MAX_SNIPE_AGE`. Implement it either as the first
   guard in `on_token_created` (cheapest — bail before building the handler) or as a
   `check_*` added to the `CRITERIA` list in
   [entry/mod.rs](backend/src/strategies/tpsl_sniper_1/entry/mod.rs). Make the
   threshold small and configurable (e.g. 5–30 s); a sniper has no reason to buy a
   token older than a few seconds.
2. **(Defense in depth)** Ensure the replay/backfill path never *pings the strategy*
   with historical creates — persist them, but don't route old-slot `TokenCreated`
   events into the live buy path. Optionally drop/flag any create whose `block_time`
   lags the current slot time by more than the freshness threshold before pinging.

**Scope:** both TPSL1 and TPSL2 (shared entry-matcher shape; clones).

---

## Error 4 — Backfill/replay stamps re-fetched txs with `now()` instead of on-chain time

**Report:** For `9eAKH9...pump`, the on-chain txs happened ~10 h ago, but after the
project's gap-replay/backlog **re-fetched** them (~30 min ago), their stored time was
set to the re-fetch time — wrong. The **slot numbers are exactly correct**.

### Root cause

The LaserStream replay path returns each tx's **slot** (immutable, from chain) but
**no on-chain `blockTime`** — Yellowstone/Geyser transaction frames don't carry block
time. The backfill therefore hard-codes the wall clock:

[token_sync.rs:803-820](backend/src/services/token_sync.rs#L803-L820)
```rust
// Replay frames carry no on-chain blockTime ... so their backfilled trades use
// `now()` as block_time
let now = Utc::now();
...
txs.push(FetchedTx { slot: r.slot, block_time: now, update: r.update });
```

The live decoder does the same thing — `block_time = received_at`
([grpc/mod.rs:180](backend/src/ingest_laserstream/decoder/grpc/mod.rs#L180)). For
**live** ingest that's harmless (received ≈ created, sub-second). The bug is the
**replay/backfill path applying that same "now" clock to old slots**: a tx from a slot
10 h ago gets `block_time = now()`, so it looks ~minutes old, not ~10 h old. The slot
is right because it comes straight from the chain.

### Why this matters (couples to Error 3)

`block_time` is the source of `created_at` for tokens and of trade timestamps. A
replayed **create** event gets `created_at = now()`, which is exactly why the freshness
gate proposed for Error 3 would *not* work on its own — a replayed 10 h-old create
would look fresh. **Fixing Error 4 is a prerequisite for Error 3's freshness gate.**

### Fix — slot-anchor estimation (unified for both paths, 1 RPC call total)

**Do not change `received_at`** — it correctly records when we fetched. Only
`block_time` needs to reflect on-chain time.

#### Step 1 — pin the anchor once at startup/reconnect (1 `getBlockTime` call)

Add a `SlotAnchor { slot: u64, time: DateTime<Utc> }` field to `AppState`. On
startup and on each stream reconnect, call `getBlockTime(current_tip_slot)` once via
the existing Helius RPC client → store the result as the anchor. This is the only RPC
call; it pins the anchor to exact chain time rather than approximating from
`received_at`.

#### Step 2 — estimate `block_time` for any historical slot

```rust
fn estimate_block_time(anchor: &SlotAnchor, tx_slot: u64) -> DateTime<Utc> {
    const SLOT_MS: i64 = 400;
    let slot_delta = anchor.slot.saturating_sub(tx_slot) as i64;
    anchor.time - Duration::milliseconds(slot_delta * SLOT_MS)
}
```

Error is negligible for the chart/freshness use-case: Solana slot timing is consistent
to within a few percent; for a 10 h gap the absolute error is minutes at most — far
better than the current `now()` error of 10 h.

#### Step 3 — apply in both replay paths

- **Gap-replay (Mechanism A) —** [grpc/mod.rs:180](backend/src/ingest_laserstream/decoder/grpc/mod.rs#L180):
  the live decoder sets `block_time = received_at` for every frame including replayed
  ones. Add a branch: if `frame.slot` is significantly behind the anchor slot (replayed
  frame), use `estimate_block_time(anchor, frame.slot)` instead of `received_at`. Live
  frames (slot ≈ tip) keep `received_at` as before.

- **Token_sync (Mechanism B) —** [token_sync.rs:807-820](backend/src/services/token_sync.rs#L807-L820):
  replace `let now = Utc::now(); txs.push(FetchedTx { block_time: now, ... })` with
  `txs.push(FetchedTx { block_time: estimate_block_time(&anchor, r.slot), ... })`.
  Works for any number of distinct slots — no per-slot RPC, no cache needed.

#### No DB persistence needed

The anchor is re-pinned from a single `getBlockTime` call on each process start.
Re-pinning is cheap and the anchor is immediately available before any replay or sync
runs.

---

## Error 5 (design question) — "Do I need the gap-replay/backfill?" — two mechanisms, only one touches trading

**Question:** whether the gap-replay/backfill is needed, given it caused the stale
buys (Error 3) and wrong timestamps (Error 4).

### Two distinct mechanisms — keep them separate

| | **A — live ingest reconnect replay** ([client.rs](backend/src/ingest_laserstream/client.rs)) | **B — token_sync backfill** ([token_sync.rs](backend/src/services/token_sync.rs)) |
| --- | --- | --- |
| Trigger | Automatic, every stream reconnect | **User clicks "Fetch All/Fetch New"** (`POST /api/token/sync`, [api/handlers/tokens/sync.rs](backend/src/api/handlers/tokens/sync.rs)) |
| Scope | Gap since last slot → tip (≤ ~24 h Helius window; falls back to live if too old) | A token's full history (creation → now) |
| **Feeds buy path?** | **YES** → pipeline → `ping_strategy` → buy/exit ([pipeline.rs:446,566](backend/src/ingest_laserstream/pipeline.rs#L446)) | **NO** — writes `trades`/`raw_transactions` only; **zero** `ping_strategy` calls |
| Consumers | Live strategy (entry/exit) | Token-detail trades chart, swing analysis, sync modal |

### Findings

- **Error 3's stale buys came from Mechanism A**, not B. token_sync (B) provably
  cannot cause a buy (no `ping_strategy`; decode-and-persist only). Its sole defect is
  the wrong timestamps (Error 4), which is a **display/history** issue — trading never
  reads B's output.
- **Error 4's wrong `block_time` affects both** replay paths: B sets `now()` explicitly
  ([token_sync.rs:807](backend/src/services/token_sync.rs#L807)); A's replayed frames go
  through the live decoder which sets `block_time = received_at = now()`
  ([grpc/mod.rs:180](backend/src/ingest_laserstream/decoder/grpc/mod.rs#L180)).

### Recommendation

- **Mechanism A — keep, but stop it driving buys.** Gap recovery is needed so a brief
  disconnect doesn't lose *trades*. The harm is replaying old *creates* into the sniper
  — fixed by the **Error 3 freshness gate** (the money-protecting fix). Optionally also
  tighten A's reconnect replay window (e.g. minutes, not 24 h). **Do not disable A**
  (you'd silently drop trades on every reconnect).
- **Mechanism B — optional; no trading impact.** Keep it only if you use historical
  charts / swing analysis; otherwise it can sit unused or be disabled with **zero**
  effect on live sniping. Fix its timestamps (Error 4) only if you care about accurate
  history — not for trading correctness.

**Net:** the fix that actually protects funds is the **Error 3 freshness gate** on the
live path; Error 4 is history-accuracy only.

---

## Feature A — Settings controls: gap-replay toggle + max replay window (default OFF / 5 min)

**Goal:** two operator controls on the Settings page for the LaserStream **reconnect
gap-replay** (Mechanism A):

1. **Toggle** `gap_replay_on_reconnect` (bool, default **OFF**) — master on/off.
2. **Max replay window** `gap_replay_max_window_secs` (number, default **300 s = 5 min**)
   — when the toggle is ON, replay only if the disconnect gap ≤ this window; longer
   outages resume live (**gap-gate** semantics).

Default OFF = conservative interim protection so a reconnect can never flood the buy
path with stale creates, even before the Error 3 freshness gate lands. The window then
bounds recovery once the operator turns replay on.

### Scope — Mechanism A only

Gate **only** the live reconnect replay in `client.rs` that feeds the buy path. **Do
NOT** affect Mechanism B (`token_sync` "Fetch All/Fetch New") — it's user-initiated,
never buys, and is useful for charts. Label the toggle precisely as **reconnect
gap-replay**, not "replay" in general.

### Behavior

- **Toggle OFF (default)** → always reconnect live (`from_slot = None`); the gap is
  skipped (window ignored). Missed **trades** are not recovered on reconnect
  (token_sync/Mechanism B can still backfill on demand), but no stale **creates** can
  reach the buy path.
- **Toggle ON, gap ≤ window** → resume from `last_slot+1`, re-fetch the missed window
  (keep the existing `PipelineBackpressure` / `ResourceExhausted` bailouts that already
  force live).
- **Toggle ON, gap > window** → resume live (`from_slot = None`). A long outage (e.g.
  the 10 h case) never replays — that's the gap-gate.

**Gap-gate measurement:** track an `Instant` of last stream progress in the client
(set whenever `last_slot` advances). At reconnect compute `disconnected_for = now −
last_progress_at` and compare to the window. No live-slot-tip estimation needed.

Guard at the `from_slot` computation
([client.rs:300-310](backend/src/ingest_laserstream/client.rs#L300-L310)):

```rust
let within_window = disconnected_for <= Duration::from_secs(gap_replay_max_window_secs);
from_slot = if seen > 0 && gap_replay_enabled && within_window
    && !matches!(reason, DisconnectReason::PipelineBackpressure
        | DisconnectReason::StreamError(tonic::Code::ResourceExhausted))
{
    Some(seen + 1)
} else {
    None
};
```

### Wiring (reuse the existing settings watch — no new endpoint)

Mirror the existing `live` / `persist_raw` / `track_mayhem` bool toggles end-to-end:

1. **[settings_repo.rs](backend/src/storage/repositories/settings_repo.rs)** — add two
   keys: `Setting::new("ingest.gap_replay_on_reconnect", || false)` (bool, default
   **false**) and `Setting::new("ingest.gap_replay_max_window_secs", || 300)` (number,
   default **300**). Add matching `pub gap_replay_on_reconnect: bool` and
   `pub gap_replay_max_window_secs: u64` fields on `AppSettings`, and the two
   `from_map()` lines. The bool mirrors `live` / `persist_raw`; the number mirrors an
   existing numeric setting (e.g. `slippage_bps` / `max_committed_sol`).
2. **[system.rs](backend/src/api/handlers/system/system.rs)** (`PUT /api/system/settings`)
   — add both fields to `UpdateSettingsRequest` and the two existing patch spots (DB
   `set_many` write + `state.modify_settings`). No new route.
3. **[client.rs](backend/src/ingest_laserstream/client.rs)** `run()` +
   **[main.rs](backend/src/main.rs)** spawn — thread the **existing**
   `settings_tx.subscribe()` (`watch::Receiver<AppSettings>`) into `run()` and read both
   `.borrow().gap_replay_on_reconnect` and `.borrow().gap_replay_max_window_secs` at the
   `from_slot` line. Also add the `last_progress_at: Instant` tracking for the gap-gate.
   Reading the watch borrow once per reconnect is free (reconnects are rare; zero
   hot-path cost). Changes take effect on the **next reconnect** — no restart needed.
4. **Frontend** — one `ToggleRow` + one numeric input row on
   [SettingsPage.tsx](frontend-react/src/pages/settings/SettingsPage.tsx) (greyed when
   the toggle is OFF), plus both fields on the `AppSettings` TS interface
   ([services/api.ts](frontend-react/src/services/api.ts)). The `updateSettings` RTK
   mutation already forwards any `Partial<AppSettings>`, so no mutation changes.
   Descriptions: *toggle — off = skip missed data on reconnect, on = re-fetch the gap
   (can re-trigger snipes on old tokens unless the freshness gate is enabled); window —
   only replay when the disconnect was shorter than this many seconds.*

### Relationship to the fixes

These controls are a **safety layer, not the fix.** The real fix for stale snipes is the
**Error 3 freshness gate**, which sits underneath both — even a within-window replay
can't snipe stale creates. Once the gate lands, replay is safe and you'd typically turn
the toggle **ON** with the window bounding recovery (short blips heal, long outages
resume live). Until then, default **OFF**.

---

<!-- Additional errors appended below as reported -->
