# Boot recovery vs. the watchdog: 70 consecutive kills, 14 h with no rule evaluated (2026-07-30)

**Symptom.** For 14 h no strategy rule was evaluated and not one position was entered.
Externally the box looked healthy the entire time — the ingest task kept landing tokens
and trades in Postgres. The process restarted 70 times.

**Cause.** A chain of three independent defects, each of which individually would have
been survivable.

1. **Unbounded boot work.** `recover_armed` read every `events-*.jsonl` front-to-back into
   one `Vec<LoggedEvent>` and applied the age cutoff *afterwards* — ~8.2 GB of JSONL on a
   4 GB box, to use its last 30 s. The process reached 2.4 GB RES and starved the 2-worker
   runtime until `DbWriter` could not land a flush for 90 s.
2. **The watchdog policed a booting process.** With no successful DB write inside
   `watchdog_stall_timeout_secs` (90 s), it force-exited — **mid-recovery**, every time.
   Killing a slow boot converts it into an unbreakable crash loop rather than a recovery,
   so the decision loop was never reached once across 70 boots.
3. **The shed was silent.** Nothing ever drained `ping_rx`, so the strategy queue stayed
   full and `ping_strategy`'s `try_send` shed 100% of pings into a counter nothing logged.
   That is why the outage was invisible: the only component that knew was mute.

**Fix — three guards, all still load-bearing.**

- Bounded tail scan: retention by **bytes** as well as days, day-splitting into rolling
  segments, and `read_log_tail` reading each kept file **backwards** in 1 MiB chunks,
  stopping at the first event older than the window.
- `BootGate`: the watchdog is armed only by `boot_gate.mark_ready()`, latched by the
  decision loop immediately before it starts consuming. While unset, the heartbeat is
  stamped each check.
- A loud, rate-limited `warn!` on the shed path in `consumer.rs`.

**The rules this produced.**

- Boot work must be **bounded at both ends**, and a watchdog must not police a booting
  process. Detail: [`@arch/strategies.md`](../arch/strategies.md).
- A `try_send`-and-drop on a path that decides trades must be **loud** — a rate-limited
  `warn!`, never a bare counter.
- **Diagnostic:** `strategy engine loop running` absent from the log means the engine
  never started, no matter how healthy ingest looks. A boot that genuinely hangs now
  surfaces as a stuck process rather than a kill loop.

Same family as [2026-07-22 heartbeat green through a wedge](2026-07-22-heartbeat-green-through-wedge.md).
