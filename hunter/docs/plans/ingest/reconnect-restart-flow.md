# Reconnect & Restart Flow

Two separate mechanisms handle failures at different layers.

---

## Mechanism 1 — Stream Reconnect (`shared/ingest/core/src/supervisor.rs`)

Runs inside the per-feed supervisor task. The loop is wire-neutral and shared by every feed; where the wires genuinely differ it reads `FeedCaps` (a feed with `replay: false` never sends a resume point, and sheds under back-pressure instead of reconnecting).
**Never restarts the process** — drops and re-opens the gRPC subscription only.

### Flow

```
run() outer loop
  ├─ [wait] live_rx == true
  ├─ run_once()  ←── single connection attempt
  │     ├─ connect() with connect_timeout = 10s
  │     ├─ send initial SubscribeRequest
  │     ├─ inner select! loop:
  │     │     ├─ stream.message()           → new tx arrives
  │     │     │     ├─ slot advanced?       → reset last_update (idle timer)
  │     │     │     └─ relevant tx?         → send_timeout(tx, PIPELINE_SEND_TIMEOUT=10s)
  │     │     │           ├─ Ok             → continue
  │     │     │           ├─ Timeout        → return PipelineBackpressure
  │     │     │           └─ Closed         → return Graceful
  │     │     ├─ pools_changed.notified()   → resubscribe (no reconnect)
  │     │     ├─ live_rx.changed()          → return Graceful
  │     │     └─ idle_check.tick() [every 2s]
  │     │           └─ elapsed > 10s?       → return IdleTimeout
  │     └─ returns DisconnectReason
  ├─ advance the ReplayAnchor — ONLY on progress (see "Replay anchor" below)
  ├─ drop the anchor when replaying would be wrong or futile:
  │     ├─ reason = PipelineBackpressure | StreamError(ResourceExhausted)
  │     │     (replay re-triggers the same condition, doubling Helius egress
  │     │      in a self-reinforcing cycle)
  │     └─ MAX_REPLAY_ATTEMPTS consecutive replays that got nowhere
  ├─ log reason + running counters
  ├─ decide delay:
  │     ├─ made progress: reset backoff to reconnect_interval, delay = reconnect_interval
  │     └─ no progress: delay = current backoff, then backoff *= 2 (cap 30s)
  ├─ add 0–50% jitter to delay
  └─ sleep(delay) → loop back
```

### Replay anchor — how a gap is (and was not) closed

`from_slot` is resolved at the **top** of each iteration by `resolve_from_slot`, the
one decider, from a retained `ReplayAnchor { slot, at }`:

| Condition | `from_slot` | Anchor after |
|---|---|---|
| no anchor (cold start / dropped) | `None` (live) | `None` |
| `gap_replay_on_reconnect = false` | `None` (live) | **kept** — flipping the toggle back on mid-outage still replays |
| gap since `at` ≤ `gap_replay_max_window_secs` | `Some(slot + 1)` | kept |
| gap since `at` > window | `None` (live) | **dropped** — that window is gone; keeping it would re-log the refusal forever |

Three properties are load-bearing. Each has failed in production before, and breaking
any of them silently loses feed windows rather than erroring — detail:
[`@history/2026-07-27-replay-anchor-blackout.md`](../../history/2026-07-27-replay-anchor-blackout.md).

1. **The anchor outlives an attempt that makes no progress.** A connect failure, a
   subscribe rejection, or a stream that opens and then goes silent until the idle
   watchdog trips leaves `progress = None`, and the anchor must *not* be advanced or
   cleared. Recomputing `from_slot` from that attempt's own `last_slot` zeroes it, so
   the next (successful) attempt subscribes live and the window is gone for good.
2. **`at` is when the last slot was *observed*, not when the attempt ended.** The
   two differ by the whole idle timeout on a silently-dead stream. Resetting
   `last_progress_at` immediately *before* measuring the window against it makes the
   measured gap always ~0 — `gap_replay_max_window_secs` becomes unreachable, the
   "gap exceeds replay window" warning becomes dead code, and a multi-hour backlog
   would be requested in full.
3. **Retention is bounded (`MAX_REPLAY_ATTEMPTS = 3`).** Retaining the anchor is what
   closes the gap, but unbounded retention can wedge: LaserStream serves only a few
   minutes of history, so a `from_slot` it refuses outright fails *every* attempt.
   Losing one window is always preferable to leaving the feed down.

**Resume is `slot + 1`, never `slot`** — deliberately. Re-requesting the anchor slot
would re-deliver its transactions, and nothing between the transport and the strategy
fold dedups by signature (only the PG insert does, via `ON CONFLICT DO NOTHING` on
`(block_time, tx_signature, leg_index)`), so a replayed slot double-counts into the
live volume/flow metrics. The residual cost is the tail of a slot the stream died
mid-way through — one slot (~400 ms) against the minute-scale gap this closes. Do not
"fix" it to `slot` without adding signature dedup ahead of the fold first.

Locked by `transport::tests::{a_no_progress_attempt_keeps_the_gap_replayable,
a_gap_wider_than_the_window_reconnects_live_and_disarms,
the_toggle_gates_replay_without_forgetting_where_we_were,
a_fresh_anchor_resumes_one_slot_past_the_last_one_seen, no_anchor_means_live}`.

### Timing constants (hardcoded in the transport)

| Constant | Value | Purpose |
|---|---|---|
| `STREAM_RECONNECT_IDLE_TIMEOUT` | **10s** | No slot advance → `IdleTimeout` |
| `STREAM_RECONNECT_IDLE_CHECK_INTERVAL` | **2s** | How often idle is tested |
| `PIPELINE_SEND_TIMEOUT` | **10s** | Max wait on a full pipeline channel |
| `MAX_RECONNECT_BACKOFF` | **30s** | Exponential backoff cap (no-progress arm) |
| `RECONNECT_INTERVAL` | **1s** (hardcoded in the transport) | Base delay between reconnects |
| `connect_timeout` (tonic) | **10s** | TCP/TLS connect hard deadline |

### Two distinct idle paths

- **Stream silent**: triggers at ≤ `IDLE_TIMEOUT` + up to 1 `CHECK_INTERVAL` = worst **~12s**
- **Pipeline full** (downstream stall): triggers at exactly `PIPELINE_SEND_TIMEOUT` = **10s** — only fires on a relevant tx; a silent stream hits the idle path first

### What counts as silence depends on the role (`idle_for`)

The idle guard's premise is "this subscription is never legitimately quiet" — a
property of the **role**, not of the transport.

| Role | Judged by | Why |
| --- | --- | --- |
| `All` | last **transaction** | A firehose carrying the venue program id. A tx gap means the stream died while block metas kept arriving — the silent death the tx-only clock exists to catch. |
| `AmmOnly` + block metas | last **frame of any kind** | Tracked pool PDAs only: 0-14 accounts that go minutes without a trade, and zero right after a boot. Block metas arrive ~2.5/s on any live connection, so their absence still catches a dead stream while quiet pools do not read as one. |
| `AmmOnly`, no block metas | nothing — guard stands down | No liveness signal exists on a narrow filter; silence proves nothing. HTTP/2 + TCP keepalive police the socket. |

The third row is the **steady state under `CURVE_SOURCE=nats` with no pool tracked**, not
an edge case: a subscription carrying no transactions does not ask for block metas
(`supervisor::build_subscription` — they are ~2.5 metered frames/s forever and fill a
cache only the AMM buy path reads), so what is left is the `accounts` filter, which is
what holds the stream open at all. There is then nothing whose silence means "dead", and
the guard says so instead of guessing.

Judging `AmmOnly` by transactions force-reconnects a healthy stream every
`IDLE_TIMEOUT` forever. It churns the provider connection, drops the block-meta
stream (which then shows up as a `feed_lag` stale-slot spike), and spends the
replay anchor on attempts that cannot make progress — after `MAX_REPLAY_ATTEMPTS`
the anchor is dropped, leaving the next real AMM gap with nothing to replay from.
Raising the timeout does not fix it: with zero tracked pools no timeout is right.

---

## Mechanism 2 — Process Watchdog (`live/src/ingest/watchdog.rs`)

Runs on a **dedicated OS thread** (not tokio). Calls `std::process::exit(1)` → supervisor (systemd/PM2) restarts the entire process.

### What it watches

`DbWriter` stamps a shared atomic (`DbHeartbeat`) at the end of a `flush()` **only
when that flush persisted at least one row** (`any_ok`). An all-failed flush (e.g.
every write timing out on an exhausted pool) does **not** stamp — so the heartbeat
means "data is landing", not merely "the writer loop is spinning" — stamping
unconditionally lets a wedged pipeline hold the heartbeat fresh indefinitely, and the
watchdog never fires
([history](../../history/2026-07-22-heartbeat-green-through-wedge.md)).

```
spawn_watchdog OS thread loop:
  ├─ read settings (enabled, stall_timeout ≥90s floor, check_interval ≥5s floor)
  ├─ sleep(check_interval)                 [default 10s]
  ├─ if live just resumed (off→on):        stamp heartbeat (give fresh window)
  ├─ idle = now − last_successful_write_stamp
  ├─ is_stalled = enabled && live && idle >= timeout
  └─ if stalled: log error → std::process::exit(1)
```

**No queue-depth gate.** The old condition also required `work_pending()`
(db_tx queue non-empty). That had a blind spot: an *upstream* stall (transport
dead, nothing arriving) drains the queue empty, so `work_pending` was false and the
watchdog stayed silent even though the feed was dead. The pump.fun firehose is never
quiet — `live && no successful write for the timeout` is unambiguously a fault
regardless of queue depth, so there is no queue-depth gate. One condition catches BOTH a
wedged downstream (pool exhausted) and a dead upstream (transport).

### Timing (DB settings, adjustable via UI)

| Setting | Default | Floor | Purpose |
|---|---|---|---|
| `watchdog_stall_timeout_secs` | **90s** | **90s** (hard floor in code) | Live + no successful write for this long → exit |
| `watchdog_check_interval_secs` | **10s** | **5s** | How often the watchdog wakes |

**Worst-case detection latency:** `stall_timeout` (90s) + `check_interval` (10s) = **~100s** from last successful write to process exit.

---

## How the Two Layers Compose

### Wedged DbWriter (DB pool exhausted / hung write)

```
Time 0:    Writes start failing — a hung query holds its hot-pool connection.
           A 60s statement_timeout on the hot pool (see storage/postgres.rs) aborts
           the stuck query server-side, frees the connection, and writes usually
           resume BEFORE the watchdog window — self-heal without a restart.
Time ~90s: If writes still aren't landing (heartbeat stale, any_ok never true),
           the watchdog fires on its next tick → exit(1) → supervisor restarts.
```

### Dead/silent gRPC stream (transport wedged, nothing arriving)

```
Time ~10s: Transport idle-timeout / reconnect attempts (Mechanism 1).
Time ~90s: If the reconnect never revives the feed, no rows are written, the
           heartbeat goes stale, and the watchdog fires — this is the case the old
           work_pending gate MISSED (empty queue) and the 7h stall proved.
```

---

## Tunable Levers

| Lever | How to change |
|---|---|
| Base reconnect delay | Hardcoded `RECONNECT_INTERVAL` in the transport (1s) |
| Watchdog stall window | UI settings page → `watchdog_stall_timeout_secs` (floor 180s) |
| Watchdog wake cadence | UI settings page → `watchdog_check_interval_secs` (floor 5s) |
| Pipeline stall timeout | Hardcoded `PIPELINE_SEND_TIMEOUT` in the transport (10s) |
| Idle stream timeout | Hardcoded `STREAM_RECONNECT_IDLE_TIMEOUT` in the transport (10s) |

> Watchdog stall/cadence defaults (180s / 15s) are applied by `AppSettings` when the key is absent from the DB — no env seed needed.
