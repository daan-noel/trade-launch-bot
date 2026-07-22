# Reconnect & Restart Flow

Two separate mechanisms handle failures at different layers.

---

## Mechanism 1 — gRPC Stream Reconnect (`client.rs`)

Runs inside the `producer_task` tokio task. **Never restarts the process** — drops and re-opens the gRPC subscription only.

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
  ├─ record last seen slot → set from_slot for replay
  │     (exception: from_slot = None when reason = PipelineBackpressure or
  │      StreamError(ResourceExhausted) — replay would re-trigger the same
  │      condition, doubling Helius egress in a self-reinforcing cycle)
  ├─ log reason + running counters
  ├─ decide delay:
  │     ├─ made progress (seen slot > 0): reset backoff to reconnect_interval, delay = reconnect_interval
  │     └─ no progress: delay = current backoff, then backoff *= 2 (cap 30s)
  ├─ add 0–50% jitter to delay
  └─ sleep(delay) → loop back
```

### Timing constants (hardcoded in `client.rs`)

| Constant | Value | Purpose |
|---|---|---|
| `STREAM_RECONNECT_IDLE_TIMEOUT` | **10s** | No slot advance → `IdleTimeout` |
| `STREAM_RECONNECT_IDLE_CHECK_INTERVAL` | **2s** | How often idle is tested |
| `PIPELINE_SEND_TIMEOUT` | **10s** | Max wait on a full pipeline channel |
| `MAX_RECONNECT_BACKOFF` | **30s** | Exponential backoff cap (no-progress arm) |
| `RECONNECT_INTERVAL` | **1s** (hardcoded in `client.rs`) | Base delay between reconnects |
| `connect_timeout` (tonic) | **10s** | TCP/TLS connect hard deadline |

### Two distinct idle paths

- **Stream silent** (no tx updates): triggers at ≤ `IDLE_TIMEOUT` + up to 1 `CHECK_INTERVAL` = worst **~12s**
- **Pipeline full** (downstream stall): triggers at exactly `PIPELINE_SEND_TIMEOUT` = **10s** — only fires on a relevant tx; a silent stream hits the idle path first

---

## Mechanism 2 — Process Watchdog (`ingest_health.rs`)

Runs on a **dedicated OS thread** (not tokio). Calls `std::process::exit(1)` → supervisor (systemd/PM2) restarts the entire process.

### What it watches

`DbWriter` stamps a shared atomic (`DbHeartbeat`) at the end of a `flush()` **only
when that flush persisted at least one row** (`any_ok`). An all-failed flush (e.g.
every write timing out on an exhausted pool) does **not** stamp — so the heartbeat
means "data is landing", not merely "the writer loop is spinning". Stamping
unconditionally was the 2026-07-22 root cause: a wedged pipeline kept the heartbeat
fresh and the watchdog never fired for 7h.

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
regardless of queue depth, so the gate was removed. This now catches BOTH a wedged
downstream (pool exhausted) and a dead upstream (transport) with one condition.

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
| Base reconnect delay | Hardcoded `RECONNECT_INTERVAL` in `client.rs` (1s) |
| Watchdog stall window | UI settings page → `watchdog_stall_timeout_secs` (floor 180s) |
| Watchdog wake cadence | UI settings page → `watchdog_check_interval_secs` (floor 5s) |
| Pipeline stall timeout | Hardcoded `PIPELINE_SEND_TIMEOUT` in `client.rs` (10s) |
| Idle stream timeout | Hardcoded `STREAM_RECONNECT_IDLE_TIMEOUT` in `client.rs` (10s) |

> Watchdog stall/cadence defaults (180s / 15s) are applied by `AppSettings` when the key is absent from the DB — no env seed needed.
