# Single-position-per-token exclusivity toggle (not started)

Extracted from `fingerprint-metrics-engine-plan.md` §9.2 (deleted — that plan's Phases
0-7 all shipped; this was the one item in it never built or tracked elsewhere). Sibling
idea §9.1 (re-entry after exit) is superseded — it shipped for real as
`RuleParams.reentry` / `ArmState::Cooldown` / per-token episode counters (see
[../arch/strategies.md](../arch/strategies.md) `arm.rs`/`state.rs` rows), so it is not
carried forward here.

## The idea

Today multiple rules can independently hold the same token at once (each rule's
`ArmState` is independent). An **exclusivity toggle** would let a rule declare "skip
entry if ANY rule already holds this token" — useful when rules are meant to compete for
the same opportunity rather than stack positions on it.

- Per-rule `exclusive BOOLEAN` (or a global env var — decide which when building). An
  exclusive rule skips entry if any rule holds the token; non-exclusive rules ignore
  others' holdings (today's behavior, unchanged when the flag is off/absent).
- Engine: the entry check consults the open-positions view inside `reduce` — the fold's
  serialized per-event loop makes the claim race-free by construction (single transition
  point, no separate lock needed).
- Priority if two exclusive rules match the same event: needs a deterministic tiebreak
  (e.g. rule `created_at`) — decide when implemented.

No design work has started beyond this sketch. Nothing depends on it; it's parked until
a concrete need for cross-rule exclusivity shows up (e.g. two flow-scalper rule variants
fighting over the same hot token).
