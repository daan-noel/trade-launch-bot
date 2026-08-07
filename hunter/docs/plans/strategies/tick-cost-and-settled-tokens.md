# Tick cost, settled tokens, and where a simulate's seconds go

Deep-dive reference for the 2026-08-07 simulate-performance pass. Read this before
touching `TokenTrack`'s window reads, `reduce`'s `Tick` branch, or the corpus's flow
columns — each of them looks innocuous and each was a multiplier.

Structure/flow lives in [`../../arch/strategies.md`](../../arch/strategies.md); this
file is the *why*.

---

## 1. The dominant cost was a token that can never die

`reduce`'s `Tick` branch swept **every** tracked token, arm by arm, at the `TICK_MS`
(200 ms) cadence. A token leaves `EngineState::tokens` only when every arm goes
terminal, and the only thing that disarms an idle *armed* token is the dead verdict —
which per [`deadness.rs`](../../../engine/src/deadness.rs) needs **both** silence for
`DEAD_QUIET_SECS` **and** real reserves under `DEAD_MAX_LIQUIDITY_SOL` (30 SOL). So
these tokens are never pruned:

* anything that pumped past 30 SOL real reserves and then went quiet;
* every post-migration / AMM row, where real reserve *is* the pool reserve;
* anything whose rows carry no `vsol` at all, so `liquidity` reads `NaN` and
  `is_dead_verdict` answers "alive" (`None` reserves ⇒ not depleted).

They accumulate for the whole corpus window. A 30-day simulate ticks ~13 M times; a
few hundred such tokens turn that into billions of arm sweeps, each doing real metric
reads. **The cost scaled with corpus width, not with anything the rule does.**

### The fix: `Settled`

`state::Settled` records that a token is done changing on its own. `reduce`'s `Tick`
branch skips it. Soundness rests on two facts that must *both* hold:

**(a) The sweep that stamped the verdict already ran at or past the token's horizon.**
The horizon is the last instant any reading can move. Almost every metric is frozen
between trades — price, reserves, lifetime flows and extrema, and every `m_position`
metric except `held` are functions of trade data alone. Only four things move on a
bare tick, and `arm::ClockHorizons` (computed per rule at `compile`, unioned per rule
set at reload) bounds each:

| Moves on a tick | Anchor | Horizon field |
| --- | --- | --- |
| trailing windows decay | newest trade | `max_window_secs` |
| `m_snapshot.time` | token creation | `time_secs` |
| `m_price_lifetime.stall` | newest trade (upper bound on the last high) | `stall_secs` |
| `m_position.held` | the entry fill | `held_secs` |

plus two token-scoped one-shots `reduce` adds directly: the dead flip at
`last_meaningful + DEAD_QUIET_SECS`, and any `ArmState::Cooldown { until }`.

> **It is deliberately "has already been evaluated past the horizon", not "`now` is
> past the horizon".** The first version compared `now` against the horizon plus one
> `TICK_MS` of slack, and the parity guard immediately caught it: tick cadence is not
> the engine's to assume. The live loop ticks every 200 ms, but a replay driver may
> tick at arbitrary instants, and any crossing landing inside a tick gap was silently
> swallowed (the failing case was a `Dead` exit that the dense run booked and the
> sparse run never did). Evaluating *at* the horizon is what makes every later instant
> provably identical; nothing else does.

**(b) No cross-token input has moved since.** Three things a token's decision reads
are not its own:

* **cap counters** — a freed slot is exactly what an arm that stayed `Armed` because
  the cap refused it is waiting for;
* **the copycat guard's memory** — a newly recorded identity can disarm an armed
  token;
* **the rule set** — a reload changes horizons, priorities, and arming.

All three bump `EngineState::cross_epoch`, and `Settled` carries the epoch it was
stamped at. A stale epoch means "re-evaluate once, then re-settle". Guard-rail: the
counter mutations go through the ONE path `EngineState::with_counters`, and the guard
writes through `EngineState::record_identity`, so no call site can change them without
the bump. (Guard *expiry* deliberately does **not** bump: it only ever un-blocks, and
a copycat block is a terminal `Disarm`, so nothing tracked is waiting for one to
lapse.)

The fourth cross-arm input, `exclusive`, resolves through events on *this* token — a
fill or close on a sibling arm — so those branches call `TokenState::unsettle()`
outright. Anything mutating a tracked token outside the fold (live's boot adoption in
`orphan_exit.rs` / `boot.rs`) must call `EngineState::touch_token`.

### `all_settled_at` — the O(1) whole-tick skip

Per-token skipping still costs one iteration + one compare per tracked token per tick,
and the token set only grows. Over millions of ticks that walk becomes the cost by
itself, so a `Tick` first checks the memo `all_settled_at == Some(tokens.len())` and
returns immediately. It is conservative — a stale `None` costs one wasted walk — and
is cleared by every non-`Tick` event (once, at the top of `reduce`), by any change in
the token count, and by `touch_token`.

### Measured

`engine/tests/tick_bench.rs` (`--release -- --ignored --nocapture`), 500 quiet
un-prunable tokens × 200 000 ticks, workstation i9-11900F:

```text
  dense ticks: 200000 ticks x 500 tokens in   27.05s  (270.5 ns / token-tick)
settled ticks: 200000 ticks x 500 tokens in 148.61ms  (  1.5 ns / token-tick)
```

~180×, widening with token count because the memo short-circuits the map rather than
each token.

### Guard

`engine/tests/settled_ticks.rs` is a **differential** test: one recorded event stream
replayed through a skipping engine and through one with `EngineState::dense_ticks`
forced, asserting identical effect streams *and* identical end state, over 24 fuzzed
seeds plus three targeted scenarios (the un-prunable token, a freed cap slot waking a
settled token, and a fixture-really-skips check so the parity test can't pass
vacuously).

Per-anchor unit tests would not have been enough — the horizons are only correct in
aggregate, and a test per anchor passes while the conjunction is wrong. `dense_ticks`
is also the kill switch: set it and the engine is back to pre-optimization behaviour,
at pre-optimization cost.

---

## 2. Window reads claimed O(1) and delivered O(n)

`flow_window::WindowState` and `flow_split::FlowSplitWindowState` both maintained
running sums on push/evict — and then **threw them away**, rescanning the whole deque
on every read. Worse, the filter called the shared `in_window`, which re-derived the
window width (a float multiply + a round) **per element**.

This is paid per metric, per rule, per event. `m_flow_split` was the worst: `value()`
went through `totals_at()`, which rebuilt a whole `FlowTotals` from scratch, so a rule
with three `m_flow_split_window` conditions did three full scans on every tick of
every tracked token. That is why "a rule with flow-split metrics" felt distinctly
slower than one without.

The read is now genuinely O(1), resting on two invariants:

* the deque is kept **time-sorted** (`flow_window::push_sorted`) — a bare `push_back`
  in the monotone case, walking in from the tail when a regressed `block_time`
  arrives (legal: canonical order is slot → tx_index → leg);
* the running sums cover **all** of the deque, so a read starts from them and
  subtracts only the two out-of-window ends — entries the last `evict` has not dropped
  yet at the front, future-dated entries at the back. Both loops stop at the first
  in-window entry, which sortedness guarantees is also the last out-of-window one.

Correcting both ends is what makes the read exact at instants nobody evicted at —
`TokenCreated` / `FirstSlotSettled` evaluate at a time no `evict` ran on, and skipped
ticks now leave entries un-evicted by design.

`price_window` keeps its filtered walks (the monotonic deques are short) but hoists
the width out of them.

Guarded by brute-force equivalence tests in both modules: the running-sum read must
equal a naive `in_window` scan at a spread of probe instants, including out-of-order
arrivals.

---

## 3. Flow labels were re-parsed per trade

`projection::to_trade_lite` ran `serde_json::from_str::<Vec<String>>` on each row's
`ix_labels` — a parse plus one heap allocation per label, **per trade**, on corpora of
millions of rows — purely to feed `ix_hash`.

Now:

* `flow_split::ix_hash_from_labels_json` walks the stored JSON array in place. It
  handles only the shape the writers emit (a flat array of unescaped strings) and
  **falls back to `serde_json`** on any escape or anything unexpected, so its result
  is by construction whatever `ix_hash_opt(&parsed)` would have returned — including
  "unparseable ⇒ `None` ⇒ organic". Locked by
  `json_scanner_matches_the_parsed_hash`.
* `projection::FlowKeys { ix_hash, wallet_hash }` is resolved once at the row decode
  (`lake/duck.rs`, `project_pg_tail`), so the fold is a pure field move.
* `Selection::with_flow_text` gates the **raw** strings. Flow *discovery* is the only
  consumer that reports label text / groups by wallet address; every other flow
  consumer classifies from the hashes. So a flow sweep/simulate row is now *smaller*
  than before — 24 B of scalars instead of two pointers into ~85 B of heap.

A fixture that carries label/wallet text must resolve `FlowKeys` the same way the
loaders do, or the classifier sees nothing.

---

## 4. Smaller items in the same pass

* **`Tick` map churn.** The branch cloned every mint key, then `remove`d and
  re-`insert`ed each `TokenState` — two keyed `BTreeMap` operations (base58 string
  compares + rebalancing) per token per tick, purely to satisfy the borrow checker.
  The sweep never reads `tokens`, so the map is now lent out with `mem::take` and
  walked in place with `retain` (which also does the pruning). `Trade` does the same
  with a single `get_mut`.
* **Per-event arm sort.** `evaluate_token` sorted the arm list by
  `(Reverse(priority), rule_id)` on every event of every token. With all priorities
  equal the key collapses to `rule_id`, and `arms` is a `BTreeMap` — already in that
  order. `EngineState::any_priority` (recomputed on reload) skips the sort unless some
  rule actually sets a priority, which is the only case where it can reorder anything.
* **`tagged_wallets`.** A `BTreeSet<u64>` doing one pointer-chasing lookup per trade
  per fingerprint, keyed by values `hash.rs` had already hashed. Now `hash::HashedSet`
  — a flat set over an identity hasher. Membership-only, never iterated, so
  determinism is unaffected.

---

## What was NOT done, and why

**Parallelising the fold.** Caps are global per-rule counters over one `EngineState`,
so a sharded replay is only sound when `max_concurrent_tokens` and `max_total_tokens`
are both unlimited *and* the copycat guard is off. Typical rules run cap 1, so the
fast path would almost never apply — and a second decision path is exactly what the
ROOT RULE in [`../../../CLAUDE.md`](../../../CLAUDE.md) forbids.

**Sparse ticks in the replay driver.** `replay.rs` still emits a dense 200 ms grid
while any token is active. That is now cheap: an all-settled tick is an O(1) memo
check inside `reduce`, so the remaining per-tick cost is one `reduce` call. Moving the
sparseness into the driver would duplicate the horizon logic outside the engine for a
sub-second gain — the engine is the right place for it, and it is already there.
