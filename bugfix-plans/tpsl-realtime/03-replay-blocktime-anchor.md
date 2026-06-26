# A3 — Replay/backfill stamps re-fetched txs with `now()` instead of on-chain time (Error 4)

> Workstream A (tpsl-realtime). **Prerequisite for [A4](04-snipe-freshness-gate.md)** — without
> this, a replayed 10 h-old create gets `created_at = now()` and looks fresh, so A4's age gate
> can't reject it.
> Paths are pre-crate-split — see [../README.md](../README.md#-path-caveat--line-refs-predate-the-crate-split).

## Report

For `9eAKH9...pump`, the on-chain txs happened ~10 h ago, but after the gap-replay/backlog
**re-fetched** them (~30 min ago), their stored time was set to the re-fetch time — wrong. The
**slot numbers are exactly correct**.

## Root cause

The LaserStream replay path returns each tx's **slot** (immutable, from chain) but **no on-chain
`blockTime`** — Yellowstone/Geyser transaction frames don't carry block time. The backfill
hard-codes the wall clock:

[token_sync.rs:803-820](../../backend/src/services/token_sync.rs#L803-L820)
```rust
// Replay frames carry no on-chain blockTime ... so their backfilled trades use
// `now()` as block_time
let now = Utc::now();
...
txs.push(FetchedTx { slot: r.slot, block_time: now, update: r.update });
```

The live decoder does the same — `block_time = received_at`
([grpc/mod.rs:180](../../backend/src/ingest_laserstream/decoder/grpc/mod.rs#L180)). For **live**
ingest that's harmless (received ≈ created). The bug is the **replay/backfill path applying that
same "now" clock to old slots**: a tx from a slot 10 h ago gets `block_time = now()`, so it looks
~minutes old, not ~10 h old. The slot is right because it comes straight from the chain.

## Why it matters (couples to A4)

`block_time` is the source of `created_at` for tokens and of trade timestamps. A replayed
**create** event gets `created_at = now()` — exactly why A4's freshness gate would *not* work on
its own. **Fix this first.**

## Fix — slot-anchor estimation (unified for both paths, 1 RPC call total)

**Do not change `received_at`** — it correctly records when we fetched. Only `block_time` needs
to reflect on-chain time.

### Step 1 — pin the anchor once at startup/reconnect (1 `getBlockTime` call)

Add `SlotAnchor { slot: u64, time: DateTime<Utc> }` to `AppState`. On startup and each stream
reconnect, call `getBlockTime(current_tip_slot)` once via the existing Helius RPC client → store
as the anchor. Only RPC call; pins to exact chain time rather than approximating from `received_at`.

### Step 2 — estimate `block_time` for any historical slot

```rust
fn estimate_block_time(anchor: &SlotAnchor, tx_slot: u64) -> DateTime<Utc> {
    const SLOT_MS: i64 = 400;
    let slot_delta = anchor.slot.saturating_sub(tx_slot) as i64;
    anchor.time - Duration::milliseconds(slot_delta * SLOT_MS)
}
```

Error is negligible for chart/freshness use: slot timing is consistent to within a few percent;
for a 10 h gap the absolute error is minutes — far better than the current 10 h error.

### Step 3 — apply in both replay paths

- **Gap-replay (Mechanism A) —** [grpc/mod.rs:180](../../backend/src/ingest_laserstream/decoder/grpc/mod.rs#L180):
  the live decoder sets `block_time = received_at` for every frame incl. replayed ones. Add a
  branch: if `frame.slot` is significantly behind the anchor slot (replayed frame), use
  `estimate_block_time(anchor, frame.slot)`; live frames (slot ≈ tip) keep `received_at`.
- **Token_sync (Mechanism B) —** [token_sync.rs:807-820](../../backend/src/services/token_sync.rs#L807-L820):
  replace `let now = Utc::now(); txs.push(FetchedTx { block_time: now, ... })` with
  `txs.push(FetchedTx { block_time: estimate_block_time(&anchor, r.slot), ... })`. Works for any
  number of distinct slots — no per-slot RPC, no cache.

### No DB persistence needed

The anchor is re-pinned from a single `getBlockTime` on each process start; it's immediately
available before any replay or sync runs.

> Mechanism A vs B are explained in [00-gap-replay-mechanisms.md](00-gap-replay-mechanisms.md).
> A's wrong timestamps drive **trading** (it feeds the buy path); B's are **display/history** only.

## Done

- `cargo check` clean on the owning crate(s); estimate the anchor on startup + reconnect; spot-check
  a replayed slot's `block_time` lands within minutes of true chain time.
