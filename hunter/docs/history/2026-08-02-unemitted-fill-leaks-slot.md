# Ten unemitted fill events filled a rule's concurrency cap — 17 h silent (2026-08-02)

**Symptom.** A live rule stopped entering positions for ~17 h. Nothing errored; the rule
was armed, loaded, and evaluating.

**Cause.** Ten `strategy_positions` rows sat in `BuySubmitted` forever, each holding a
`max_concurrent_tokens` slot, until the cap was full and `decide_arm` could never
authorize another entry.

The chain: the `BuySubmitted` row is durable **before** the send. The buy send had **no
timeout** — the sell path had `SELL_SEND_TIMEOUT`, the buy path had no mirror — so a
wedged send parked `run_entry` indefinitely with neither `FillConfirmed` nor `FillFailed`
emitted. The arm stayed in `EntryPending`, and because boot re-adopts an open row as an
inert arm, the leaked slot survived process restarts too.

**Why it was silent.** Every visible signal stayed green: ingest healthy, rule active, no
error rows. The only observable was a count that had quietly reached its ceiling.

**Fix.**

- `BUY_SEND_TIMEOUT` — the buy-path mirror of `SELL_SEND_TIMEOUT`, so the send is bounded.
- Every exit from `dispatch_buy` / `run_entry` must emit a fill event; use
  `decision_loop::fail_entry` (`Fatal` for structural causes, `Reverted` where a later
  attempt can succeed), never a bare `return`.
- The reaper drops a `BuySubmitted` row with **zero signatures** past `UNENTERED_STALE`
  (600 s) — it provably sent no transaction, so it cannot own a bag. A row **with** a
  signature is untouched and still waits for adopt-or-all-reverted.

**The rule this produced.** When a durable row is written before an external call, every
path out of that call must transition it — including the paths you did not plan for. Two
independent guards, not one, because the bounded timeout and the reaper fail differently.

Same family as [2026-07-22 heartbeat](2026-07-22-heartbeat-green-through-wedge.md) and
[2026-07-30 boot killstorm](2026-07-30-boot-recovery-killstorm.md): a failure that leaves
every visible signal green.

Current contract: [`@arch/position-lifecycle.md`](../arch/position-lifecycle.md).
