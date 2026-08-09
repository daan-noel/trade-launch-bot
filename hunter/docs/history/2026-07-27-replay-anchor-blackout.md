# Replay anchor dropped on a fruitless attempt — a two-minute feed blackout (2026-07-27)

**Symptom.** 20:11–20:12 came back **empty** in `trades`, with
`gap_replay_on_reconnect = true` — i.e. gap replay was armed and still closed nothing.

**Blast radius, wider than the feed.** Every strategy decision in the window ran on
incomplete data, and two tokens were never flagged migrated or dead because the trades
that would have flagged them vanished. A feed gap is not a data-completeness problem in
isolation; it is a wrong-decision problem downstream.

**Cause.** `from_slot` was recomputed from *that attempt's own* `last_slot`. A connect
failure, a subscribe rejection, or a stream that opens and then goes silent until the idle
watchdog trips all leave `progress = None` — so one fruitless attempt zeroed the anchor,
and the next (successful) attempt subscribed **live**, permanently losing the window.

A second, latent defect was found in the same area: `last_progress_at` was reset
immediately *before* the window was measured against it, so the measured gap was always
~0. `gap_replay_max_window_secs` was therefore unreachable, the "gap exceeds replay
window" warning was dead code, and a multi-hour backlog would have been requested in full
had the first bug not masked it.

**Fix.** `resolve_from_slot` became the ONE decider, resolving `from_slot` at the **top**
of each iteration from a retained `ReplayAnchor { slot, at }`. The anchor outlives a
no-progress attempt; `at` records when the last slot was *observed*; retention is bounded
by `MAX_REPLAY_ATTEMPTS = 3` (LaserStream serves only a few minutes of history, so a
`from_slot` it refuses outright fails *every* attempt — losing one window beats leaving
the feed down).

**The rule this produced.** A recovery anchor must survive the failure it exists to
recover from. Deriving it from the failing attempt's own state is self-defeating.

Current contract + the guard tests:
[`@plans/ingest/reconnect-restart-flow.md`](../plans/ingest/reconnect-restart-flow.md).
