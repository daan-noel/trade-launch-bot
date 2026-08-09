# Pending measurement runs (operator, not code)

Work that is **code-complete and shipped** but still owes a run on the live box or a
real corpus. Kept here so the runs are not lost when the implementation plans are
folded into their deep-dives. Delete an entry once its run is done and its number is
written into the deep-dive it belongs to.

## 1. Scale-out / partial exits

Shipped end to end (engine → exec/mig → kernel → sweep → FE → manual partial) —
contract in [../plans/strategies/partial-exits.md](../plans/strategies/partial-exits.md).
Two runs never happened:

- **Paper smoke on the live box.** Arm one paper rule with a 2-stage ladder and walk a
  real position through it: partial fill keeps `Holding` + advances `stage`/`sold_bps`,
  ledger row per leg, aggregates match the ledger, final close stamps the
  weighted-average exit and only then re-arms. Covers the wiring the golden tests
  cannot (sink → SSE → Console chip → dialog ledger).
- **`fs3-00` re-measure with a banked tranche** — the primary payoff, and the reason
  this was built: `fs3-00 dev13 base` is a bare `retrace >= 7, arm_above_pct 2`, i.e.
  exactly the "gives back 25-30% of every winner" shape. Cheapest first shape to try:
  **one partial into strength + a remainder stage that is a pure time stop**
  (`held >= N`, no trail on the stub) — a direct probe of the open-at-cap vs trail-out
  split without searching trail widths. Run it through simulate (the PnL authority),
  not the sweep. The crew-rider re-measure stays optional/later.

## 2. `fs2-*` ladder (64hP-calibrated) — seeded, never run

[`../../scripts/seed-flow-scalper-64hp-rules.sql`](../../scripts/seed-flow-scalper-64hp-rules.sql)
inserts the whole ladder as `trade_mode='paper'`, `is_active=false`, so nothing fires
until someone flips `is_active`. The ladder spec + its calibration now live in
[../plans/strategies/wallet-analysis.md](../plans/strategies/wallet-analysis.md)
("`fs2-*` ladder").

**Superseded before it ran** by the narrow-fingerprint `fs3-*` ladder
([`seed-flow-scalper-dev13-rules.sql`](../../scripts/seed-flow-scalper-dev13-rules.sql)),
which arms ~110 tokens/day instead of ~18,000 and is the first configuration to stay
PnL-positive under the adversarial `worst` fill. Two `fs2` knobs were already revised by
the `fs3` runs: the dip gate is best at **25** (not 18) and liquidity at **40-75** (not
36-70). Keep `fs2-*` only as the **broad-universe control** — its knob conclusions still
hold, they just apply to a worse universe.

Cheaper than arming a live paper rule: `./hunter/scripts/flow-scalper-ladder.ps1 -Plan v64`
(draft simulate runs, no DB rows, priced under `pumpfun_impact`).

Two engine gaps this ladder wants and does not have — both are general entry-precision
gaps, not 64hP-specific:

1. **No `unique_wallets` metric** — per the gate-payoff measurement in
   `wallet-analysis.md`, the top engine gap for entry precision.
2. **No pct-of-vsol sizing** (Phase 5, deferred). A fixed size inside a 36-70 vsol band
   means impact varies materially across it — see
   [../plans/strategies/execution-costs.md](../plans/strategies/execution-costs.md) §2.
