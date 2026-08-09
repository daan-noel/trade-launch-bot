# Seven watchdog kills in one day — real outages, and the transport was mute (2026-08-05)

**Symptom.** `live::ingest::watchdog` force-exited the process seven times in one day. The
log showed a healthy process and then a kill, with no evidence of why the feed stopped.

**Checked against the data rather than assumed.** `trades` shows a genuine hole at each
kill, so the watchdog was reporting a true fault, not a false positive:

- 13:44 — **zero** rows, in a feed averaging ~1500/min.
- 12:08 → 12:09 — decayed 1538 → 225 → 103 before the 12:09:47 kill.

**Cause of the blindness.** `ingest_core` and `ingest_laserstream` were not named in
`live/main.rs`'s default `EnvFilter`, and the box sets no `RUST_LOG`. So every
`LaserStream: connecting` / `stream error` / `no transaction update … forcing reconnect` /
`pipeline backpressured` line was dropped, and the sole live transport ran invisibly in
production. `live::ingest` (host adapter + watchdog) was always covered by `live=info` —
the crates *underneath* it, which are exactly the layer that knows why a feed died, were
not.

**Fix.** Both crates named explicitly in the default `EnvFilter`. Every one of those lines
is per-connection, never per-message, so there is no hot-path cost to keeping them on.

**The rule this produced.** The sole live transport must be in the log filter — a
subsystem you cannot see is a subsystem you cannot diagnose, and "the host adapter is
covered" is not the same as "the transport is covered".

**What this did not resolve** — carried forward as open work in
[`@roadmap/ingest-watchdog-kill-recovery.md`](../roadmap/ingest-watchdog-kill-recovery.md):
a kill costs 1–2 min of feed data permanently, and something defeats the transport's ~12 s
self-heal for 90 s+.
