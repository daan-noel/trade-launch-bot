# 45% of paper positions stranded as `ExitStuck` (2026-08-05)

**Symptom.** 333 of 742 paper positions (45%) sat open in `ExitStuck` indefinitely.
Backlog at discovery: **359 local, 329 server**.

**Cause — two independent halves.**

1. **Paper reached `ExitStuck` constantly.** `exec_paper`'s exit resolved with
   `market_fill_on_empty_window = false`, while lab replay/simulate and the sweep both
   pass `true`. A `Dead` exit fires *because* the token stopped printing, so its fill
   window is empty by construction: each of the engine's 5 retries re-fired against the
   same last trade, all 5 timed out, `ExitStuck`. A manual close on a mint no longer in
   the cache did the same.
2. **Nothing owned the result.** `ExitStuck` means "the sell gave up, the bag is still
   held" — a real-only premise — so every recovery/reaper query filtered `mode = 'real'`.
   A paper row that reached it had no owner at all.

**Why it mattered.** The bias is **one-directional**: the stranded rows are exactly the
dead-token losers, so paper PnL read systematically high for as long as they sat open.
A stuck-row backlog that is uniformly distributed is a nuisance; one correlated with
outcome is a wrong number.

**Fix.** The paper exit leg market-fills like analysis (falling back to the token's last
known spot when no window can price it), and `close_paper_exit_stuck` was added to the
reaper sweep to own any paper row that still fails. The backlog was healed **through that
same pricing path**, not a one-shot script — both databases then held zero `ExitStuck`
rows. There is deliberately no migration or script to re-run: the reaper is the only path.

**The rule this produced.** A status whose premise is mode-specific needs a mode-specific
owner, or the other mode's rows are invisible-by-construction. Check the `mode =` filter
on every recovery query when adding a status.

Current contract: [`@arch/position-lifecycle.md`](../arch/position-lifecycle.md) §2.2.
Related: [2026-08-04 token-scale 1e6 PnL](2026-08-04-token-scale-1e6-pnl.md) — the pricing
rule that made the heal safe on both sides of that fix.
