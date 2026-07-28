# Flow-scalper `fs2-*` rule ladder - 64hP-calibrated (2026-07-28)

> **SUPERSEDED by the `fs3-*` ladder, later the same day.** `fs2-*` runs on ONE BROAD
> fingerprint, on the assumption that creation shape carries no signal. That assumption
> was only ever tested against token *selection*; against per-episode *outcome* the
> creator dev-buy size does carry signal (59.2% win / +7.11 %/ep above 12.8 SOL vs 49.1%
> / +2.44% below, mint-clustered permutation p=0.006, replicated on an untouched
> holdout and on every day of the window). See "Dev-buy size" in
> [`../plans/strategies/wallet-analysis.md`](../plans/strategies/wallet-analysis.md).
>
> Consequences: the narrow fingerprint arms ~110 tokens/day instead of ~18,000 (which
> is what made the `fs2-*` matched set too big to simulate or trade), and it is the
> first configuration here to stay PnL-positive under the adversarial `worst` fill.
> Seed [`../../scripts/seed-flow-scalper-dev13-rules.sql`](../../scripts/seed-flow-scalper-dev13-rules.sql);
> ladder plans `fp13` / `fp13b` / `fp13ctl`. Keep `fs2-*` only as the broad-universe
> control - the knob conclusions below still hold, they just apply to a worse universe.
> Two of them were revised by the `fs3-*` runs: the dip gate is best at **25**, not 18,
> and `liquidity` at **40-75**, not 36-70.

Renamed from `flow-scalper-fingerprint-rules.md`. That file's original content - a
6-rule fingerprint A/B calibrated from wallet `omego` - is **retired**:
[`../plans/strategies/flow-scalper-findings.md`](../plans/strategies/flow-scalper-findings.md)
established that omego's gross edge (+1.81% of turnover) does not clear the 2.53%
round-trip fee, and that his real profit is an unclosed runner tranche `hunter-engine`
cannot express (no partial-exit concept). Do not run
`hunter/scripts/seed-flow-scalper-rules.sql` (deleted) or otherwise revive the `fs-*`
fingerprint ladder - the reasoning trail survives in
[`../plans/strategies/wallet-analysis.md`](../plans/strategies/wallet-analysis.md)
("Proposed knob deltas for the `fs-%` seed rules") for context only.

This file is now **only** the live, unrun `fs2-*` ladder calibrated from wallet
`64hP97Bwr5PubotcTeGgfhkFrGiLVVxT2kVo9M9b4AEz`, whose closed-episode edge clears the
fee by ~2.5pp (see the `64hP` section of
[`../plans/strategies/wallet-analysis.md`](../plans/strategies/wallet-analysis.md)).

Seed: [`../../scripts/seed-flow-scalper-64hp-rules.sql`](../../scripts/seed-flow-scalper-64hp-rules.sql)
(paper, `is_active=false`). Engine capabilities assumed are all **shipped**:
`m_price_window`, `m_position` (incl. `arm_above_pct`), multi-window-per-group,
`reentry`.

**Safety.** The seed inserts every rule as `trade_mode='paper'`, `is_active=false`. The
engine loads `WHERE is_active AND is_enabled` (`rule_repo.rs`), so nothing fires until
you flip `is_active`. Params were validated against `RuleParams::parse` +
`CompiledRule::compile` before commit (throwaway test, removed).

## `fs2-*` ladder - 64hP-calibrated knob sweep

ONE broad fingerprint (`fs2-ALL broad`; creation shape carries no signal once hotness
is known - see the fingerprint-axis section of `wallet-analysis.md`) x 12 rules; each
moves exactly ONE knob off `fs2-00 base`.

Base:

```
entry (AND)   m_snapshot.time        >= 30      his p25 first-buy age is 30 s
              m_snapshot.liquidity   36 .. 70   his first-buy vsol p25-p75
              m_price_window(30).trail >= 18    dip off the 30 s high; his >-8% bucket is
                                                his ONLY losing cohort (-0.81%/ep)
              m_flow_window(60).gross_flow >= 45  his p25 gross60 = 45.6 SOL
              m_flow_window(2).net_flow   >= 0    sell-exhaustion
exit (OR)     m_position.retrace     >= 7        his measured trail is 6.8%
              m_position.arm_above_pct = 2       <- he does NOT have this; see below
              m_position.held        >= 90       his >90 s cohort is net NEGATIVE
              m_flow_window(30).gross_flow <= 3  cold-token bailout; he does NOT have this
              stop_loss              = 12        floor while the trail is unarmed
buy           0.30 SOL fixed                     pct-of-vsol sizing is unimplemented (Ph5)
```

Two deliberate departures from his measured behaviour, both measured on his own episodes:

1. **Armed trail.** His trail is unarmed (losers exit at a median -7.16% = the trail
   firing with `peak` still at entry). That mostly works because his median *pre-peak*
   drawdown is only -1.15% - price usually rises straight off his entry. But **23.6% of
   his big winners dipped >7% before peaking**, so an unarmed 7% trail cuts them.
   `arm_above_pct: 2` keeps them; `stop_loss: 12` is the floor until it arms.
   `fs2-05` reverts to his literal unarmed exit for the A/B.
2. **Dead-flow bailout.** His only defect: 227 episodes (3.3%) never sold because the
   token went cold, costing -225.3 SOL against a +168.7 SOL book. Replaying
   `m_flow_window(30).gross_flow <= 3` over those bags: **fires on 146/227, median 54.8 s
   after entry at -11.5% vs entry, taking the cohort from -225.3 SOL to -19.1 SOL.** The
   `held >= 90` cap closes the remaining 81. `fs2-07` removes it for the A/B.

**Do NOT use `m_price_lifetime.stall` for this.** It is seconds since the last *new
all-time high*, not since the last trade, so on a dip-entry rule it is true by
construction and simply caps position lifetime at the threshold (this is the defect that
capped holds at ~15 s in the old `fs-*` rules - see
[`../plans/strategies/flow-scalper-findings.md`](../plans/strategies/flow-scalper-findings.md)
finding #2).

Ladder (each row = one knob off base): `01` dip 12 / `02` dip 25 / `03` trail 4 /
`04` trail 11 / `05` unarmed / `06` no time cap / `07` no dead-flow exit / `08` liq
30-110 / `09` gross60 20 / `10` size 0.10 SOL / `11` size 0.80 SOL.

Param shapes verified against `RuleParams::parse` + `CompiledRule::compile` via a
throwaway test (entry reqs = 5; `arm_above_pct` attaches to `retrace` only, never to
`held`; dead-flow req is token-scoped at window 30; `stop_loss` desugars).

**Open: buy size is the least-grounded knob.** 64hP sizes at 1.859% of vsol capped at
1.5 SOL gross (~0.8 SOL in this band), but
[`../plans/strategies/execution-costs.md`](../plans/strategies/execution-costs.md)'s
impact-aware cost model puts the optimal *fixed* size at `sqrt(fixed_cost_per_leg *
vsol)` - ~0.21-0.27 SOL on this band, not 0.30. `fs2-10` / `fs2-11` bracket it (0.10 /
0.80); consider adding a rule nearer the computed optimum before sweeping. This is also
exactly what `scripts/flow-scalper-ladder.ps1`'s `v64` plan tests (draft simulate runs,
no DB rows, priced under `pumpfun_impact`) - run that first, it's cheaper than arming a
live paper rule.

## How to run

```powershell
psql $env:DATABASE_URL -f hunter/scripts/seed-flow-scalper-64hp-rules.sql
cargo run -p hunter-lab            # :8140, then use the lab UI simulate / grouped sweep
```

Or the draft-simulate ladder (no DB rows, faster iteration):
```powershell
./hunter/scripts/flow-scalper-ladder.ps1 -Plan v64
```

**Live paper** (once you trust a specific rule from the ladder):
```sql
UPDATE strategy_rules SET is_active = true WHERE rule_name = 'fs2-00 base';
-- to stop:
UPDATE strategy_rules SET is_active = false WHERE rule_name LIKE 'fs2-%';
```

## Known limits

1. **No `unique_wallets` metric.** Per `wallet-analysis.md`'s gate-payoff measurement,
   this is the top engine gap for entry precision generally (not 64hP-specific).
2. **No pct-of-vsol sizing** (Phase 5, deferred). A fixed size inside a 36-70 vsol band
   means impact varies materially across the band - see `execution-costs.md` section 2.
3. **Untested: entering at token age <1 min.** 64hP's median first-buy age is 0.8 min
   with 135 trades/60s already flowing - much faster adoption than the omego-calibrated
   rules assumed. Whether our arm-to-fill latency can realistically compete at that age
   is unmeasured.
4. **One 5-day window, one wallet** behind this calibration.
