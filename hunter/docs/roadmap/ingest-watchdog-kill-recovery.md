# Ingest watchdog kills — two open defects

Both surfaced while confirming that watchdog kills are **real outages, not false
positives** (evidence: [`@history/2026-08-05-watchdog-kills-are-real-outages.md`](../history/2026-08-05-watchdog-kills-are-real-outages.md)).
Neither is fixed.

`live::ingest::watchdog` force-exits when no **successful** DB write lands for
`watchdog_stall_timeout_secs` (90 s) while live + booted.

## 1. A kill costs 1–2 min of feed data, permanently

In-process reconnect replays from the last slot (`ReplayAnchor`), but a process exit
discards that anchor and the restart comes up live. The gap is simply absent from `trades`
and is never backfilled — **the killer destroys the state the healer needed.**

Options, none evaluated yet: persist the anchor across restarts (cheapest, but it must not
replay a stale slot after a long downtime); or have the restart backfill the gap from RPC
(costs Helius credits — needs explicit approval per the product `CLAUDE.md`).

## 2. Something defeats the ~12 s self-heal for 90 s+

The transport self-heals in about 12 s (`idle_reconnect_timeout` 10 s +
`idle_check_interval` 2 s), far inside the 90 s watchdog window. A plain feed stall should
therefore never reach the watchdog at all — so a reaching kill means the reconnect path
itself is blocked, not that the feed merely paused.

Diagnosing this needs the transport logs, which are now enabled (see the history entry).
**Enable-then-wait; do not guess** — the next kill should be read from
`LaserStream: connecting` / `stream error` / `forcing reconnect` / `pipeline backpressured`
lines around the kill timestamp.
