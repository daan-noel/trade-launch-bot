# Reference — Gap-replay/backfill: two mechanisms, only one touches trading (Error 5)

> Workstream A (tpsl-realtime). **Reference doc, not a fix.** It grounds
> [A3](03-replay-blocktime-anchor.md), [A4](04-snipe-freshness-gate.md) and
> [A5](05-gap-replay-settings-controls.md). Read it to decide whether to keep the gap-replay.
> Paths are pre-crate-split — see [../README.md](../README.md#-path-caveat--line-refs-predate-the-crate-split).

## Question

Whether the gap-replay/backfill is needed, given it caused the stale buys (A4/Error 3) and wrong
timestamps (A3/Error 4).

## Two distinct mechanisms — keep them separate

| | **A — live ingest reconnect replay** ([client.rs](../../backend/src/ingest_laserstream/client.rs)) | **B — token_sync backfill** ([token_sync.rs](../../backend/src/services/token_sync.rs)) |
| --- | --- | --- |
| Trigger | Automatic, every stream reconnect | **User clicks "Fetch All/Fetch New"** (`POST /api/token/sync`, [api/handlers/tokens/sync.rs](../../backend/src/api/handlers/tokens/sync.rs)) |
| Scope | Gap since last slot → tip (≤ ~24 h Helius window; falls back to live if too old) | A token's full history (creation → now) |
| **Feeds buy path?** | **YES** → pipeline → `ping_strategy` → buy/exit ([pipeline.rs:446,566](../../backend/src/ingest_laserstream/pipeline.rs#L446)) | **NO** — writes `trades`/`raw_transactions` only; **zero** `ping_strategy` calls |
| Consumers | Live strategy (entry/exit) | Token-detail trades chart, swing analysis, sync modal |

## Findings

- **The stale buys came from Mechanism A**, not B. token_sync (B) provably cannot cause a buy (no
  `ping_strategy`; decode-and-persist only). Its sole defect is the wrong timestamps (A3/Error 4),
  a **display/history** issue — trading never reads B's output.
- **The wrong `block_time` affects both** replay paths: B sets `now()` explicitly
  ([token_sync.rs:807](../../backend/src/services/token_sync.rs#L807)); A's replayed frames go
  through the live decoder which sets `block_time = received_at = now()`
  ([grpc/mod.rs:180](../../backend/src/ingest_laserstream/decoder/grpc/mod.rs#L180)).

## Recommendation

- **Mechanism A — keep, but stop it driving buys.** Gap recovery is needed so a brief disconnect
  doesn't lose *trades*. The harm is replaying old *creates* into the sniper — fixed by the
  [A4 freshness gate](04-snipe-freshness-gate.md). Optionally tighten A's reconnect window via
  [A5](05-gap-replay-settings-controls.md). **Do not disable A** (you'd silently drop trades on
  every reconnect).
- **Mechanism B — optional; no trading impact.** Keep it only if you use historical charts / swing
  analysis; otherwise it can sit unused. Fix its timestamps ([A3](03-replay-blocktime-anchor.md))
  only if you care about accurate history — not for trading correctness.

**Net:** the fix that protects funds is the [A4 freshness gate](04-snipe-freshness-gate.md) on the
live path; A3/Error 4 is history-accuracy only (but is a prerequisite for A4 to work on replayed
creates).
