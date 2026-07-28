# Grouped sweep — re-entry (successor to grouped-sweep-phase6.md)

Phase 6 items A (fill/cost fidelity) and B (TP/SL migration + bind-time req
classification) landed and are now documented permanently in
[../arch/sweep.md](../arch/sweep.md) (*Exit-scan path* + *Pricing* rows, the
*Metric scope* section, and the *entry is exit-dependent* section). This file
carries forward only what's still open: Phase 6 item C (re-entry) plus two loose
acceptance checks A/B never ran. `grouped-sweep-phase6.md` is deleted — its design
rationale for A/B is superseded by the shipped code + arch doc; nothing here repeats
it.

Read [../plans/strategies/wallet-analysis.md](../plans/strategies/wallet-analysis.md)
for the WHY and target numbers (moved there 2026-07-28 from
`flow-reversion-scalper.md`), and [../arch/sweep.md](../arch/sweep.md) for the current
sweep architecture this work extends.

## Anatomy (so you don't re-explore)

| File | Role |
| --- | --- |
| `lab/src/sweep/generic/strategy.rs` | `GenericSweepStrategy`, `resolve_entry`/`resolve_exit`, `BoundCombo` |
| `lab/src/sweep/generic/exit_index.rs` | `ExitIndex` prefix-extrema hulls |
| `lab/src/sweep/generic/guard.rs` | scan ≡ `run_replay` parity guards — item C's episode-parity guard belongs here |
| `lab/src/sweep/engine.rs` | `fill_outcomes_with_state` (entry cache, `engine.rs:48-59`), `aggs[combo_id].record(o)` at `:527`, `combo_batch_size` |
| `lab/src/sweep/aggregate.rs` | `ComboAgg` → `RunAgg` (streaming DDSketch, O(1)/combo) → `ComboMetrics` |
| `lab/src/sweep/strategy.rs` | `TokenOutcome` (`Copy`), `Strategy`/`ParamSpace` traits |
| `engine/src/reduce.rs` (`hunter-engine`) | live re-entry reference: `RuleParams.reentry`, `ArmState::Cooldown`, per-token episode counter — mirror this, don't re-derive it |

Commands: `cargo check -p hunter-lab`, `cargo test -p hunter-lab --lib sweep::generic`,
`--target-dir "C:/Users/User/Documents/Bot/target-check"` when a bin is running. Build
test targets with `-j 2` — full parallelism OOMs this box (pagefile error 1455).

---

## Item C — re-entry in the grouped sweep (not started)

The engine has re-entry (`RuleParams.reentry`, `ArmState::Cooldown`, per-token
episode counter). The sweep does not — a swept combo scores one episode per token,
so any re-entry rule is mis-scored. Confirmed still true 2026-07-26: every `reentry`
reference under `lab/src/sweep` is a test fixture set to `None`.

### The scan is the easy part

Episodes are **sequential and non-overlapping**, so the cursor moves monotonically:
`entry → exit → cooldown_until → re-arm → resume search`. A multi-episode scan over a
token is still **one forward pass, O(n) total** — not O(episodes × n). Re-entry costs
nothing asymptotically. Mirror `reduce.rs`: re-arm on **normal exits only**
(TP/SL/Metrics — never Dead/Manual/Migrated), and honour `cooldown_sec`.

**Cap semantics.** The sweep deliberately strips `max_concurrent_tokens` / `max_total`
(they would serialize the token fan-out — documented in `compile_combo`, do not "fix").
But `max_episodes_per_token` is **not** a concurrency cap; it is part of the strategy's
identity and bounds a per-token quantity the sweep already evaluates per token. **The
sweep must honour it.** It is also what makes the memory model below computable.

### The hard part 1: it breaks the entry cache

Today `resolve_entry` is resolved once per (token, distinct entry-key) and reused
across every exit-variant combo (`engine.rs:48-59`; entry axes are the high-order
combo digits so same-entry combos are contiguous). That is a large constant-factor
win — with 100 entry × 20 exit combinations it is 20× fewer entry scans.

With re-entry, only **episode 1** is a pure function of the entry key; episode 2's
entry begins after episode 1's exit, which depends on exit params. The cache silently
stops applying.

**Fix: cache an entry-eligible row bitmap, not a resolved entry.** For a given
(token, entry-key), precompute a bitset over series rows where the entry conditions
hold. That *is* a pure function of the entry key regardless of episode count, so all
the expensive shared work (evaluating entry reqs across every row) survives. Each
combo's episode loop becomes "find next set bit ≥ cursor" — a word scan + `tzcnt`.

The per-combo half of `can_enter` (*exit metrics must not already hold*) depends on
exit params, so it stays per-combo — but it is now only evaluated at candidate rows,
not every row. Net: this should be **faster than today's cache**, not merely
re-entry-compatible. Memory: `n_rows / 8` bytes per entry-key, rebuilt on the same
cadence as today's cache.

### The hard part 2: the outcome transport

`TokenOutcome` is `Copy`, one per combo, consumed positionally
(`aggs[combo_id].record(&outs[combo_id])`, `engine.rs:527`) across a
producer→folder channel, and `combo_batch_size` budgets `batch × sizeof(TokenOutcome)`.
Variable episodes per combo break all three.

**Do not aggregate episodes inside the scan.** `RunAgg` is a streaming fixed-size
DDSketch (O(1) per combo), so folding **each episode as its own `record` call** yields
per-episode win rate, median and p90 for free — which is exactly what the analysis
reasons in (median gap ~31 s, up to 31 episodes/token, per-episode edge). Collapsing
to a token-level sum would silently redefine `win_rate` as "token was net positive"
and corrupt every ranked column.

Recommended shape:

1. Stamp `combo_id` on `TokenOutcome` (or ship a parallel `episode_counts` vector) so
   the channel is self-describing and the folder stops relying on position.
2. Emit N outcomes per (combo, token); folder loop becomes `aggs[o.combo_id].record(o)`.
3. Update `combo_batch_size`: the per-combo term becomes
   `sizeof(ComboAgg) + inflight × max_episodes × sizeof(TokenOutcome)`. Bounded
   **because** `max_episodes_per_token` is honoured — absent re-entry it is 1 and the
   model is unchanged.
4. Drill-in + chart markers assume one entry/exit pair per token — they need to
   render N.

### Acceptance

- Guard: a re-entry rule's scan ≡ `run_replay` **episode for episode** (same count,
  same per-episode entry/exit price and reason), on a fixture with ≥3 episodes and one
  that hits the episode cap.
- One-shot rules (`reentry: None`) produce byte-identical results to today — the
  existing `guard.rs` suite is the non-regression.
- Cooldown boundary: an exit and a re-entry signal inside the same cooldown window
  must not re-enter; the first eligible row at/after `until` must.
- Dead/Manual/Migrated must **not** re-arm.
- Bitmap cache: a combo scanned with a shared bitmap must equal the same combo
  scanned standalone (the `shared_bind_matches_per_token_bind` pattern — detach the
  cache from the token, which is the only way this class of bug surfaces).

---

## Loose ends from items A and B (shipped, but two checks never ran)

Both need the real lake, not a unit test — run once, then delete this section:

- **Item A:** a `first`-fill sweep over the eval cohort should reproduce the
  `flow_scalper_fill_sensitivity` sign (+, not −) for the anchor combo; and
  `pumpfun_fee_only` + a fill model should equal the harness's `realFee`. (The
  *wiring* half of the latter is already locked by
  `sweep_cost_selector_matches_the_realfee_column`.)
- **Item B:** benchmark `resolve_exit` before/after the classification change on a
  real grid — it's the measured hot spot and the change was never profiled.

---

## Gotchas

- **The sweep is a parallel impl of the fold.** Replay/simulate inherit engine
  changes free; this scan does not. Never claim "backtested" from a sweep until
  `guard.rs` covers the new path.
- **Scalar is the SSOT.** Every fast path (index, SIMD, bitmap) must be provably
  equal to the scalar walk, and the scalar walk must never be deleted.
- **`resolve_exit` is the measured hot spot**, not `prepare_token`. Spend effort
  there.
- **Test-build OOM:** build test targets with `-j 2`; full parallelism hits pagefile
  error 1455 on this box.
- **Don't lower the fast-path bar to make a guard pass.** If a class can't be
  recognised safely (tolerance-sensitive `=`, multi-arm DNF, `!=`), it belongs in
  **General** — a correct scalar walk always beats a clever wrong index.
