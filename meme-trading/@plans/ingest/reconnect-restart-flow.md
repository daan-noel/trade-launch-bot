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

`DbWriter` stamps a shared atomic (`IngestHeartbeat`) at the **end of every `flush()`** — after all DB writes complete. This is the single "real progress" signal.

```
spawn_watchdog OS thread loop:
  ├─ read settings (enabled, stall_timeout ≥180s floor, check_interval ≥5s floor)
  ├─ sleep(check_interval)                 [default 15s]
  ├─ if live just resumed (off→on):        stamp heartbeat (give fresh window)
  ├─ idle = now − last_heartbeat_stamp
  ├─ is_stalled = enabled && live && work_pending() && idle >= timeout
  │     work_pending() = db_tx.capacity() < db_tx.max_capacity()
  │                    = DB write queue has undrained items
  └─ if stalled: log error → std::process::exit(1)
```

### Timing (DB settings, adjustable via UI)

| Setting | Default | Floor | Purpose |
|---|---|---|---|
| `watchdog_stall_timeout_secs` | **90s** | **90s** (hard floor in code) | Stale heartbeat + work pending → exit |
| `watchdog_check_interval_secs` | **10s** | **5s** | How often the watchdog wakes |

**Worst-case detection latency:** `stall_timeout` (90s) + `check_interval` (10s) = **~100s** from last DB commit to process exit.

---

## How the Two Layers Compose

### Wedged DbWriter (hung DB `.await`)

```
Time 0:    DbWriter hangs on a DB call — last heartbeat stamp frozen here
Time ~10s: Pipeline channel fills, client send_timeout fires
           → PipelineBackpressure → reconnect after ~1s
           → new stream, same wedge, same 10s → repeat
Time ~90s: heartbeat still stale, db_tx queue still backed up (work_pending=true)
           → watchdog fires on its next 10s tick
           → exit(1) → supervisor restarts
```

### Dead/silent gRPC stream (pipeline healthy)

```
Time ~10s: No slot advance for STREAM_RECONNECT_IDLE_TIMEOUT
           → IdleTimeout → reconnect after ~10s delay (+ jitter)
           DbWriter keeps committing → heartbeat kept fresh
           → watchdog never fires
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
