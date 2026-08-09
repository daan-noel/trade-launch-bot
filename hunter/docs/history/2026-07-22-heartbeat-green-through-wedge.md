# The liveness heartbeat stayed green through a 7 h feed outage (2026-07-22)

**Symptom.** The LaserStream feed stopped producing for ~7 h. The ingest watchdog — whose
entire job is to force-exit a wedged process — never fired. Every external health signal
read normal for the whole window.

**Cause.** Two independent mistakes in what "alive" meant.

- `db_writer.rs` stamped `DbHeartbeat` at the end of **every** `flush()`, whether or not a
  single row persisted. A flush in which every write failed still refreshed the heartbeat,
  so the heartbeat measured *"the flush loop is running"*, not *"data is landing"*.
- The watchdog additionally gated on DB-queue depth. That proxy is exactly backwards for
  an upstream stall: a dead transport produces nothing, so the queue **drains empty** —
  the healthiest-looking possible reading — while no data arrives at all.

The actual failure was a connection-pool exhaustion wedge downstream, which the
unconditional stamp masked.

**Fix.** `flush()` stamps only when it persisted ≥1 row (`any_ok`); an all-failed flush
leaves the heartbeat stale. The queue-depth gate was dropped: `live + stale` alone now
catches both a wedged downstream and a dead upstream.

**The rule this produced.** A liveness signal must be derived from **work completed**,
never from a loop iterating — and never from a queue being short, which a total upstream
failure also produces. See the watchdog section of [`@arch/ingest.md`](../arch/ingest.md).

Same family as [2026-07-30 boot-recovery killstorm](2026-07-30-boot-recovery-killstorm.md)
and the silent-shed gotcha in `hunter/CLAUDE.md`: failures that leave every visible signal
green.
