# Partial exits (tranched scale-out) — decision record

> The **shipped** contract for `RuleParams.scale_out`. Structure/flow lives in
> [../../arch/strategies.md](../../arch/strategies.md),
> [../../arch/position-lifecycle.md](../../arch/position-lifecycle.md) and
> [../../arch/sweep.md](../../arch/sweep.md); this file is the *why* — the semantics
> that are easy to break and the alternatives that were rejected.
>
> Motivation (measured, see [wallet-analysis.md](wallet-analysis.md) +
> [flow-scalper-findings.md](flow-scalper-findings.md)): a fixed TP costs
> −10..−18%/event because it caps the tail that carries all EV, while a pure trailing
> stop gives back 25-30% of every winner. omego is net-positive **only** via his ~19%
> runner tranche. Before this landed, every exit in `reduce.rs` closed 100%.

## Design principles (why the shape is what it is)

1. **N ordered stages, not a hardcoded 2-tranche case.** The authoring shape is a
   `Vec` of stages; "sell most into strength, trail a stub" is the 1-stage instance.
2. **Each stage reuses the FULL exit grammar** (`SideConditions` + per-stage TP sugar
   + `arm_above_pct`), compiled through the same `build_reqs`/`pnl_req` desugar. No
   parallel mini-language that can drift — so every exit family (fixed ROI, time stop,
   trailing, flow/momentum fade) is per-stage for free, and any metric added later is
   automatically a legal stage trigger.
3. **Sizes are `bps of the INITIAL bag`**, never of the remainder — fractions compose
   without compounding drift, and exec does exact integer token math. The final/global
   close is always "all remaining" so dust is swept.
4. **The portion travels on the existing `SubmitSell` effect** as a field, not a new
   effect variant — the same vocabulary serves manual partial sells.
5. **Durable truth is a per-position fills ledger** (`position_fills`), not wider
   one-row columns — any number of legs, both sides, no further migration for manual
   partials or future DCA.
6. **ONE decision kernel stays law.** It lands in `reduce`, so live-real, live-paper
   and simulate get it by construction; the grouped sweep gets a staged resolver plus
   parity guards (D5/D6 in [../sweep/sim-parity.md](../sweep/sim-parity.md)).

## Semantics (the contract)

- `scale_out: [ { sell_bps, conditions… }, … ]`, ordered. Stages are one-shot and fire
  strictly in order: stage k is the only stage evaluated while `stage == k`. Firing
  sells `sell_bps` of the initial bag and advances to `k+1`.
- **Global exits are unchanged and always close 100% of the remainder**: `Dead`
  verdict, desugared `stop_loss`, the authored `exit` side, `Migrated`, `Manual`.
  Priority per event stays `Dead > SL > … > stages`. This is the catastrophe path — a
  stub in a rug must not wait for its stage.
- After the last partial stage, the position continues under the global `exit` side —
  **unless** the ladder ends with a **remainder stage** (`sell_bps` omitted ⇒ portion
  `All`), whose conditions close it outright with their own reason. That is the only
  way the stub gets a *different* policy than the pre-banking hold (trail 25% while
  full, tighten to 8% on the stub); the global side is static across the whole hold and
  cannot express it. Stage progression is therefore stepped/dynamic trailing for free.
- `PositionCtx` (entry/peak/trough/entered_at) is **not** reset per stage — `pnl`,
  `retrace`, `held` keep anchoring on the original entry.
- Validation: `sell_bps` in `[1, 9900]`, `sum(explicit sell_bps) <= 9900` (a remainder
  must exist for whatever closes it), stage conditions non-empty, `scale_out: []` folds
  to "not set" at the wire boundary (the `configured_labels` precedent — see the
  zero-as-unbound rule in [../../../CLAUDE.md](../../../CLAUDE.md)).
- **Hard cap: at most 3 explicit stages** (+ optional remainder). Two independent
  reasons, both worth re-reading before raising it: fee cost (~1% of notional per extra
  leg at 0.1 SOL) and **in-flight-sell blindness** — no new decision is made while a
  sell is pending, and each stage multiplies that window. A global exit that becomes
  true mid-partial fires on the next event after the fill resolves; in-flight escalation
  is deliberately not built.

## Grain: episode vs leg (locked)

Re-entry multiplies **episodes**; scale-out multiplies **legs inside one episode**.
They are orthogonal — never merged into one status concept.

```text
(token, rule) ── episode 1 ── strategy_positions row A
                  buy 1  +  sell legs (partial…partial…final)  → End
                  └── arm → Cooldown → Armed
             ── episode 2 ── strategy_positions row B   (fresh next_position())
```

| Axis | Unit of truth | Owns status? |
| --- | --- | --- |
| **Episode** (re-entry) | one `strategy_positions` row = one buy → fully flat | YES |
| **Leg** (scale-out) | one `position_fills` row under that position | NO — parent stays open until flat |

Legs never open a new row, never bump the episode counter, never call
`rearm_after_close`.

**Rejected alternatives, and why (do not re-propose without new evidence):**

1. **One fat row accumulating every re-entry buy+sell** — status becomes meaningless
   ("Holding" with 5 entries?), `CLOSED_PRED` / win-loss / per-episode PnL all break,
   and omego-style 31 episodes/mint is unreadable.
2. **A new status `PartialExited` / `ScalingOut`** — FE partitions, attention lane,
   reaper predicates and `status_partition_guard` all explode for no gain; a brief
   `ExitPending` flash already says "sell in flight".
3. **Each partial as `End` + immediate re-buy of the stub** — burns an extra fee round
   trip, double-counts episodes, resets `PositionCtx` (peak/retrace), and fights the
   point of trailing a stub on the *same* entry.
4. **Per-stage re-entry** (re-arm while still holding a stub) — concurrency /
   token-account / cap semantics become undefined. One open position per (token, rule)
   arm stays law.

### Status transitions (domain unchanged)

```text
BuySubmitted -> Holding
Holding      -> ExitPending   (any sell starts — partial OR full)
ExitPending  -> Holding       (PARTIAL fill confirmed: stage advances, bag remains)
ExitPending  -> End           (FULL / remainder fill confirmed — ONLY then may rearm)
ExitPending  -> ExitStuck | ExitUnconfirmed   (as before; bag = remainder)
```

- Caps: `open` stays held until `End` (mid-ladder keeps its concurrency slot); `total`
  increments per Enter (per episode).
- Re-entry: `rearm_after_close` / episode++ / `Cooldown` fire **only** on the final
  `End`. Partial fill reasons are ledger-only and never qualify as a close;
  `reason_allows_reentry` reads the **last** leg's reason.
- Boot: `count_closed_by_rule_mint` seeds `episodes`, and a mid-ladder `Holding` row is
  not closed — so a restart cannot inflate the budget. An adopted mid-ladder position
  resumes `stage`/`sold_*` from the row + ledger.

## Two layers of truth (within one episode)

| Layer | Stores | Read by |
| --- | --- | --- |
| **`strategy_positions`** (1 row/episode) | lifecycle status + entry snapshot + running aggregates (`sold_token_amount`, `exit_sol_lamports_total`, `scale_stage`) + on `End` the weighted-average exit stamp so existing PnL SQL keeps working | Console list, `CLOSED_PRED`, portfolio summaries, win/loss, episode counts |
| **`position_fills`** (N rows/episode) | every leg, entry included: `seq, side, price, sol_lamports, token_amount, at, reason, stage, tx_signature` | row-dialog ledger, per-leg chart markers, multi-leg cost kernel |

Charts must read the ledger layer, not the aggregate one: the `End` stamp is a
**weighted-average** exit price, so a single arrow at it marks a price no leg traded.
The paged position reads therefore carry `exit_legs` (`StrategyRepo::sell_legs_for_positions`
→ `attach_exit_legs`, one bounded batch per page beside token enrichment — never per row,
since every row on the page can render as a chart card). Legs ship wherever `exit_price`
alone cannot describe them — a ladder, or a still-open position that has banked a leg and
has no stamp yet. Only a **closed single-leg** close is omitted: it is already exactly its
`exit_*` stamp, so shipping it would be the same fact twice. `sell_bps` on a leg and the
position's `sold_bps` rollup both read
`models::bps_of_bag`, so a leg's share and the aggregate can never scale differently.

Per-leg **PnL% and hold time are never stored columns** — they derive at read time from
the ledger + the position's entry (same pattern as `strategy_position_pnl`, extended to
N legs):

```text
leg_pnl_pct     = (fill.price - entry_price) / entry_price * 100
leg_hold_secs   = epoch(fill.at - entry_time)
leg_pnl_sol     = fill.sol_lamports/1e9 - entry_sol * (fill.token_amount / entry_token_amount)
position_realized_sol (closed) = Σ exit_fill.sol_lamports - entry_lamports
position_mtm_sol (open)        = Σ banked exit_sol + mark * remaining_tokens - entry_sol
```

**The ledger is authority; the aggregates are a cache** — guarded by a writer-owned
test asserting `exit_sol_lamports_total == Σ position_fills.sol_lamports` (sell legs)
and `sold_token_amount == Σ sell token_amount`. Same class of guard as the fingerprint
sentinel bugs: two writers of one fact is how they drift.

**Unique-signature note.** `uq_strategy_positions_exit_sig0` (unique on
`exit_tx_signatures->>0`, the real-mode double-sell guard) is **not** sufficient once
the array holds N sigs — a later leg's sig could collide with another position's first.
There is no such constraint: uniqueness lives on `position_fills.tx_signature` (partial
unique, real sells). Entry-side `uq_strategy_positions_entry_sig0` stays — still one buy.

## Cost kernel

`round_trip_multi_leg` (entry leg + N exit legs) charges each leg fee bps + fixed
per-leg + impact(`leg_size / reserve_at_leg`); the single-exit wrapper is unchanged.
**Fixed cost scales with leg count** — that is the real economic bound on stage count
(~1% of notional per extra leg at 0.1 SOL). Surface it, never hide it. See
[execution-costs.md](execution-costs.md).

## Extension points left open (deliberately not built)

Per-stage position metrics (`since_stage` anchors), scale-in/DCA buys, in-flight sell
escalation, per-stage re-entry, and a continuous dynamic trail (`trail_pct = f(pnl)`,
or dynamic `arm_above_pct`) — the last is approximated arbitrarily well by adding
stages, so build it only if measurement demands it.

Pending measurement runs (paper smoke, `fs3-00` re-measure with a banked tranche):
[../../roadmap/pending-measurement-runs.md](../../roadmap/pending-measurement-runs.md).
