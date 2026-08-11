# Fingerprint metric discovery - the best metrics and values for a fingerprint

**The target is a fingerprint.** The output is the set of metrics that actually influence its
tokens, plus the value each one wants. A rule that already trades the fingerprint is an
optional *seed* and a baseline to beat - never the axis set, and never the starting assumption
about which metrics matter.

Which tokens a fingerprint selects is [fingerprint-rule-handoff.md](fingerprint-rule-handoff.md),
the phase-level research arc is [metrics-path-profitable-rules.md](metrics-path-profitable-rules.md),
and the sweep engine itself is [../../arch/sweep.md](../../arch/sweep.md).

## The four rules that make this work

**1. Every metric is a candidate, and every candidate carries an `off` value.** A metric that
is never an axis cannot appear in a winning combo, so the grid produces no evidence about it,
and reading "it never won" as "it does not help" is a false conclusion rather than a
measurement. When a seed rule exists, its conditions join the candidate pool like any other,
including whatever sits in its `params.disabled` block. A parked condition is a previous pass's
opinion, not a result.

**2. The search is a strategy, not an exhaustion.** 38 metrics x 2 sides x directions x values
x window choices is many orders past the 1,000,000 per-group `HARD_MAX_COMBOS`. Coverage comes
from re-testing every candidate each round, not from one enormous grid.

**3. A screen taken once is wrong.** A metric's value depends on what else is selected: the
same condition loses to its own `off` row under one entry gate and is worth +0.28 SOL under
another. Selection therefore re-measures every candidate against the *current* set, every
round, and stops only when the set stops changing.

**4. Selection measures money; robustness is a separate, final check.** Rank on PnL while
choosing. Ask "is this real" once, at the end. Folding the robustness checks into the selection
loop doubles the cost and starts fitting the validation window itself.

## What "best" means

Rank on **`total_pnl_sol + open_pnl_sol`** at the live notional, worst fill + `impact`.

- `total_pnl_sol` alone is **closed-only**, so it crowns configs that never exit. A config
  leaving 107 positions open at `open_pnl_sol` -3.43 outranks every honest one until the open
  mark is added back.
- Report per-trade PnL alongside, but rank on the total.
- Never rank on the sweep's `score`: it floors `win_rate` at 0.01 and carries an open-position
  drag term, so it answers a different question.
- The fixed per-leg cost is size-sensitive, so a result at one `buy_amount_sol` does not
  transfer to another. Sweep at the notional the rule trades.

Hard gates - a candidate failing any of these is rejected whatever its headline number:

| Gate | Rule |
| --- | --- |
| Coverage floor | Fires on too few tokens, or ends with too many positions open, to read as a result. |
| Plateau | Neighbouring values of an axis score far worse. A lone peak fits a handful of tokens. |
| Out-of-sample | Fails 2 of 3 split dates on per-trade PnL, or lands validate profit factor at or below 1. |

Warnings - reported in the headline, never an automatic rejection:

| Warning | Meaning |
| --- | --- |
| Wide fill-model spread | How much of the config is an execution bet. Compare worst fill + `impact` against `first` + `fee_only`. |
| Every candidate decays | A verdict on the **cohort**, not the candidates: the edge sits before the split date. Size down rather than rejecting the shortlist. |

**Why both plateau and out-of-sample.** The simulation is honest about the tokens it runs; the
*selection* is what biases the number. Keeping the best of 3,072 combos keeps a score that is
high partly because the rule is good and partly because that combo suits those tokens. Plateau
asks whether the win is real using **all** matched tokens; the split asks the same question
using only the validate window's trades. Plateau costs nothing extra because the refine grid
already computes the neighbours, and at these cohort sizes it reads the stronger of the two.
The split earns its place on the cases plateau cannot see: a value that scores highest
in-sample and turns negative forward.

## The complete metric catalog - 38 metrics, 8 groups

SSOT is `hunter_engine::metrics::REGISTRY` (`hunter/engine/src/metrics/mod.rs`); a name that
disagrees with it fails the sweep at parse rather than silently no-opping. Semantics for the
flow groups live in [metrics-reference.md](metrics-reference.md).

| Group | Kind | Scope | Sides | Strict params | Metrics (unit) |
| --- | --- | --- | --- | --- | --- |
| `m_snapshot` | static | token | entry + exit | - | `time` (s), `liquidity` (SOL) |
| `m_price_lifetime` | static | token | entry + exit | - | `stall` (s), `trail` (%), `rise` (%) |
| `m_price_window` | **dynamic** | token | entry + exit | `window_size_sec` | `trail` (%), `rise` (%) |
| `m_flow_lifetime` | static | token | entry + exit | - | `gross_flow`, `net_flow`, `buy`, `sell` (all SOL) |
| `m_flow_window` | **dynamic** | token | entry + exit | `window_size_sec` | `gross_flow`, `net_flow`, `buy`, `sell` (SOL), `unique_wallets` (count) |
| `m_flow_split` | static | token | entry + exit | fp `volume_ix_patterns` | `vol_buy`, `vol_sell`, `vol_net`, `vol_gross`, `nonvol_buy`, `nonvol_sell`, `nonvol_net`, `nonvol_gross` (SOL), `vol_share` (%) |
| `m_flow_split_window` | **dynamic** | token | entry + exit | `window_size_sec` (+ fp patterns) | same nine as `m_flow_split` |
| `m_position` | static | **position** | **exit only** | `arm_above_pct` | `retrace` (%), `bounce` (%), `pnl` (%), `held` (s) |

Plus two non-metric axes: `take_profit` and `stop_loss` (both %, desugar into `m_position.pnl`).
Both belong in the candidate pool even when the seed rule carries neither.

Legality traps:

- **`m_position` is exit-only.** The sweep rejects it on the entry side - it reads `NaN` before
  a fill, so it could never fire.
- **`m_flow_split*` needs `volume_ix_patterns`** on the request *and* in the fingerprint's
  `metric_config`. Unconfigured, every flow-split metric is `NaN`, which satisfies nothing: the
  conditions read as present but never fire. Rule save warns; it does not reject.
- **A dynamic group needs `window_size_sec`.** Two axes on the same side+group with different
  windows become independent clauses, which is how you sweep a window.
- **`take_profit` / `stop_loss` axes reject `null`.** To test "no take-profit", either omit the
  axis entirely or pass an unreachable value (`1000` for TP, `100` for SL).

Combination semantics: **entry conditions AND together, exit conditions OR together.** Adding
an exit axis can only make exits fire earlier or as early, never later.

## Stage 0 - cohort health

```powershell
cd hunter
cargo run --release -p hunter-lab -- lake-export --include-today   # lake must cover the whole span
```

Then answer two questions before spending any compute.

1. **How many tokens does the fingerprint match, and over what span?** This is the hard ceiling
   on sample size. Under ~100 matched tokens, treat every result as directional only.
2. **Is the cohort still behaving the way it does earlier in the span?** Split the matched
   tokens by day and compare median curve life and median peak `liquidity`. A launcher that
   changes tactics turns a validated rule into one that never fires, and it shows up here
   rather than in the search. A break narrows the date window for every later stage.

```sql
-- both answers, one query; substitute the fingerprint's own axes
WITH fp AS (
  SELECT mint_address, created_at, date_trunc('day', created_at)::date AS day
  FROM tokens
  WHERE ix_labels = '["..."]'::jsonb
    AND (initial_buy_instruction->>'max_cost_lamports') = '43200000')
SELECT f.day, count(DISTINCT f.mint_address) AS tokens,
  round(percentile_cont(0.5) WITHIN GROUP (
    ORDER BY EXTRACT(EPOCH FROM (t.block_time - f.created_at)))::numeric, 0) AS med_curve_life_s,
  round(percentile_cont(0.5) WITHIN GROUP (ORDER BY t.reserve_lamports/1e9)::numeric, 1) AS med_vsol
FROM fp f JOIN trades t ON t.mint_address = f.mint_address AND t.venue = 'curve'
GROUP BY 1 ORDER BY 1;
```

Compare the pinned cohort against the same `ix_labels` *without* the continuous pins. When only
the pinned rows change behavior, the fingerprint is the thing that changes, not the market.

## Stage 1 - the candidate pool

A candidate is one `(side, group, metric, operator, window)` with a short value menu. Build the
pool from the **whole catalog**, not from the seed rule.

- **Both sides** for every group except `m_position` (exit only).
- **Both directions** where the metric is two-sided: a lower bound and an upper bound are
  different candidates, and either may be the one that pays.
- **Windows** for the three dynamic groups: one candidate per window in a small set such as
  `{2, 3, 5, 10, 30}` seconds. The window is part of the search, not a constant chosen up front.
- **`take_profit` and `stop_loss`**, each against an unreachable value.
- **Everything in the seed rule**, active and `disabled` alike.

Values come from **this fingerprint's own percentiles**, never global ones: a gate below a
metric's floor is a no-op, and one above its p95 selects noise. Two values per candidate
(around p50 and p75) is enough for selection - Stage 3 tunes the winner.

```sql
WITH fp AS (SELECT mint_address, created_at FROM tokens WHERE /* fingerprint axes */),
tr AS (SELECT t.reserve_lamports/1e9 - 30 AS liq,
              EXTRACT(EPOCH FROM (t.block_time - fp.created_at)) AS age_sec
       FROM trades t JOIN fp ON fp.mint_address = t.mint_address
       WHERE t.venue = 'curve' AND t.reserve_lamports IS NOT NULL)
SELECT 'liquidity' AS metric,
  round(percentile_cont(0.10) WITHIN GROUP (ORDER BY liq)::numeric,1) p10,
  round(percentile_cont(0.25) WITHIN GROUP (ORDER BY liq)::numeric,1) p25,
  round(percentile_cont(0.50) WITHIN GROUP (ORDER BY liq)::numeric,1) p50,
  round(percentile_cont(0.75) WITHIN GROUP (ORDER BY liq)::numeric,1) p75,
  round(percentile_cont(0.90) WITHIN GROUP (ORDER BY liq)::numeric,1) p90
FROM tr
UNION ALL SELECT 'time', /* same five over age_sec */ 0,0,0,0,0;
```

Structural anchors that hold across fingerprints: `m_snapshot.liquidity` is the **real** SOL
reserve, not the virtual one - the engine feeds `TradeLite::reserve_sol` from `real_reserve_sol`
(`live/src/strategies/engine/producers.rs`), which is `vsol - 30` on the curve. So it floors at
**0** (empty curve) and tops out near **85** (migration): a gate written against the virtual
30/115 scale is ~30 too high, and `liquidity >= 85` fires only on tokens that actually migrate.
The SQL above already subtracts 30; `reserve_lamports` itself is virtual. `stall` is seconds
since the **last all-time high**, so an exit below ~60 fires on ordinary chop.

## Stage 2 - forward + backward selection

The core. Start from a set `S` of selected conditions and repeat until `S` stops changing:

```
ADD    measure every candidate not in S, in the presence of S
DROP   measure every condition already in S, by removing it
       -> keep the best add; discard anything whose removal does not hurt
```

Both directions come from **one grid per batch**: put the currently selected conditions in as
axes of `[null, current_value]` *and* a batch of candidates as axes of `[null, v1, v2]`. Reading
each axis's `on` against `off` marginal answers "should this be added" and "should this be
dropped" from the same run.

Batch so each run lands at **2k-4k combos** - roughly 11 axes at 2 values, or 8 at 3. Round 1
sweeps the whole pool in batches; later rounds re-sweep only the current `S` plus the strongest
~30 candidates, which is where the cost saving lives.

```json
{
  "strategy_id": "generic",
  "fingerprint_id": "<uuid>",
  "group_by": [], "curve_only": false, "min_tokens": 1,
  "min_fired_abs": 100, "fire_frac": 0.3, "method": "grid",
  "buy_amount_sol": 0.01, "fill_model": "worst", "cost_model": "pumpfun_impact",
  "token_cap": 100000, "max_combos": 20000, "ram_reserve_mb": 1024, "use_avx512": true,
  "volume_ix_patterns": [["..."]],
  "axes": { "axes": [
    { "kind": "metric", "side": "exit", "group": "m_position", "metric": "retrace",
      "operator": ">=", "values": [null, 40.0] },
    { "kind": "metric", "side": "exit", "group": "m_flow_split_window", "metric": "nonvol_buy",
      "operator": ">=", "window": 2, "values": [null, 1.0, 1.9] }
  ] }
}
```

Read every axis's marginal from one query. The `on` minus `off` difference in mean net PnL is
the main effect across every combo either way, which is far more stable than a single top row:

```sql
WITH x AS (
  SELECT r.total_pnl_sol + r.open_pnl_sol AS net, r.profit_factor AS pf, c.params AS p
  FROM grouped_sweep_results r
  JOIN grouped_sweep_combos c ON c.run_id = r.run_id AND c.combo_id = r.combo_id
  WHERE r.run_id = '<run>'),
f AS (
  SELECT 'exit retrace' AS cond, (p->'exit'->'m_position'->'retrace') IS NOT NULL AS on_, net, pf FROM x
  UNION ALL SELECT 'exit nonvol_buy', (p->'exit'->'m_flow_split_window'->'nonvol_buy') IS NOT NULL, net, pf FROM x
  /* one line per axis in the run */)
SELECT cond,
  round(avg(net) FILTER (WHERE on_)::numeric, 3) AS net_on,
  round(avg(net) FILTER (WHERE NOT on_)::numeric, 3) AS net_off,
  round((avg(net) FILTER (WHERE on_) - avg(net) FILTER (WHERE NOT on_))::numeric, 3) AS marginal,
  round(avg(pf) FILTER (WHERE on_)::numeric, 2) AS pf_on,
  round(avg(pf) FILTER (WHERE NOT on_)::numeric, 2) AS pf_off
FROM f GROUP BY cond ORDER BY marginal DESC;
```

A condition whose `off` side wins is genuinely parked, and that verdict now rests on evidence.
A condition whose marginal is positive on PnL but negative on profit factor buys money with
volatility: keep it, and expect the fill-spread warning to notice.

**Run two seeds.** Once from `S = {}` and once from `S =` the seed rule's conditions. Agreement
is free confirmation; disagreement is itself a finding, and the higher-ranked result wins.
Greedy selection can miss a *pair* of metrics that only pay together when neither pays alone,
and two seeds is the cheap guard against it.

**Only ~365 result rows persist per group** (retention keeps top and bottom 3 distinct values of
11 metrics, capped 10 per tie, plus the winner), so a batch grid is not fully readable
afterwards. Read each round's marginals before starting the next.

## Stage 3 - tune the values, confirm the plateau

Re-grid finely over **only** the surviving 3-6 conditions, several values each. What you want is
not a better number but a **plateau**: neighbouring values scoring the same.

- Adjacent values producing identical results means the condition rarely binds - drop it and
  keep the rule smaller.
- Most combos in the refined region profitable means the region is real, not a lucky corner.
- A lone peak whose neighbours lose is overfit, and fails the plateau gate.

Widen the menu before concluding a peak is lone: a value looks spiky against a coarse, noisy
grid and resolves into a smooth plateau once its immediate neighbours are tested.

## Stage 4 - validate

The sweep is a **ranking screener, not a backtest** - its numbers and simulate's differ, and
only simulate's are quotable. Re-run finalists through `POST /api/strategies/simulate` (a
`draft` body stays RAM-only; a `rule_id` body persists). Results come back on **POST**
`/api/strategies/simulate/{run_id}/result/summary`.

1. **Authority run** - worst fill + `impact`, `buy_amount_sol` = the live notional. The only
   quotable number.
2. **Fill sensitivity** - re-run under `first` fill + `fee_only`. The ratio against the
   authority run is the warning, not a gate: it measures how much of the config is an execution
   bet.
3. **Out-of-sample** - three split dates across the span, comparing **per-trade** PnL rather
   than totals. Failing 2 of 3, or a validate profit factor at or below 1, rejects. All
   candidates failing together is a cohort verdict instead.

Run one finalist's whole battery back to back rather than interleaving finalists: simulate is
cold scan + cold load bound (~53% + ~42%) and the engine fold is ~3%, so a warm re-run of the
same rule finishes in milliseconds.

## Stage 5 - ship

Create the rule with `POST /api/strategy-rules` rather than a SQL insert, so `RuleParams::parse`
validates every group and metric name against the registry.

- Name it `<fingerprint or original> --- v<n>`; keep any original untouched for comparison.
- **Park, do not delete.** Rejected conditions go under `params.disabled` in the same shape, so
  the next pass sees what is tried. They are validated, never compiled, and they re-enter the
  candidate pool next time.
- Ship `is_active: false`. Flipping it on is a separate, deliberate act.
- Carry `trade_mode`, `buy_amount_lamports`, `max_concurrent_tokens` and `tags` from the
  original, and re-check the tags: carrying `stable` onto an un-smoke-tested rule reads
  misleadingly.

## Running it fast

The whole search fits about an hour on one fingerprint. Four knobs, in order of payoff:

1. **Keep the selection byte-identical across a stage.** The corpus cache is a *single slot*
   keyed on the selection hash (mints + trade counts + lake version), so every run in a stage
   pays one DuckDB load - but only while `fingerprint_id`, the date range, `token_cap` and
   `curve_only` stay fixed. Any change evicts and reloads for that run *and* every run after it.
   Vary axes inside a stage; vary selection only between stages.
2. **Run the lab in release.** `cargo run -p hunter-lab` is a debug build, where the vector exit
   scan is ~**2.3x slower** than scalar. Use `cargo run --release -p hunter-lab`.
3. **AVX-512 On.** `use_avx512: true` runs the per-`(combo x token)` exit scan on an 8x`f64`
   kernel: **2.2x** on a pnl-bound shape. Byte-identical to scalar and never persisted on the
   run row, so it can never move a result. Host-gated on `avx512f`.
4. **Size the RAM reserve to the box, not to the minimum.** A smaller reserve admits bigger
   runs, but the reserve is what keeps the host alive: 512M on a 16GB workstation also hosting
   `hunter-live` and DuckDB puts the lab within reach of an allocation abort. Prefer 1024M
   unless the desktop is idle.

Measured numbers and the driver/sizing detail: [../../arch/sweep.md](../../arch/sweep.md) and
[../sweep/ram-sizing.md](../sweep/ram-sizing.md).

## Traps

| Trap | The rule |
| --- | --- |
| A metric that is not an axis | It cannot win, so it produces no evidence. Never park a condition no grid tests. |
| A result that adds nothing | If the winner introduces no metric absent from the seed rule, the search degenerates into pruning. Check the candidate pool covers every group. |
| An empty screen read as a result | A screen returning no survivors is a **failed measurement**, not "no metric helps". Diagnose it before proceeding. |
| A one-token objective | If dropping a single fired token moves the ranking statistic by more than ~2x, the cohort is right-tail dominated and that statistic is unusable. Rank on money. |
| `score` vs money | `score` floors `win_rate` at 0.01 and penalises open positions. Rank on `total_pnl_sol + open_pnl_sol`. |
| Closed-only PnL | `total_pnl_sol` excludes open positions, so a config that never exits outranks honest ones. Always add the open mark. |
| Sweep numbers quoted as PnL | The sweep approximates; `simulate` is the authority. Re-run before believing a number. |
| Take-profit assumed good | Capping upside is a real cost where the edge is the right tail. Always sweep TP and SL against unreachable values. |
| Flow-split silently dead | No `volume_ix_patterns` means all nine metrics are `NaN`, conditions never fire, and nothing errors. |
| `stall` misread | Seconds since the last all-time high, not since the last trade. `m_position.held` is the time stop. |
| Unarmed `retrace` | Without `arm_above_pct` the peak seeds at entry, so `retrace` is a hard stop from entry, not a trailing stop. |
| AVX-512 under a debug build | The vector scan is ~2.3x *slower* there. Toggle it on only when the lab runs `--release`. |
| A stage that reloads the corpus | The cache is one slot on the selection hash. Changing the range, `token_cap` or `curve_only` mid-stage re-pays the DuckDB load on every remaining run. |
| Only ~365 rows persist per group | A big grid is not fully readable afterwards. Read each round's marginals before the next run. |
| Stale scoped sweep rows | Runs with a `fingerprint_id` created before 2026-08-11 are truncated by `token_cap`; re-run them. [history](../../history/2026-08-11-scoped-sweep-token-cap-truncation.md) |
| Cost constants moved | Runs stored before 2026-07-28 price at 100 bps with no impact charge and do not compare. [execution-costs.md](execution-costs.md) |

## Worked example - g8 `46a9df64`: what pruning alone misses

Corpus 312 matched tokens (2026-07-22 to 08-11), **0.05 SOL**, worst fill + `impact`, simulate:

| Config | Trades | Win rate | PnL (SOL) | PF | Validate PF |
| --- | --- | --- | --- | --- | --- |
| seed rule - 4 entry + 6 exit conditions | 154 | 0.48 | +1.319 | 1.48 | **1.06** |
| seed entry, exits trimmed to three | 154 | 0.42 | +1.602 | 1.51 | 1.36 |
| entry `liquidity < 60`, three exits | 201 | 0.36 | **+2.175** | 1.49 | **1.69** |

What this example carries:

- **The seed rule fails out-of-sample.** Per-trade +0.0124 train against +0.0011 validate. A
  rule promoted from a grid decays exactly where its selection luck runs out, which is why
  Stage 4 exists and why the seed rule is never the axis set.
- **An entry band can cost more than it screens for.** `liquidity > 25` and `time > 5` both
  carry negative marginals; removing them takes 154 trades to 201 *and* raises PnL.
- **No take-profit and no stop-loss.** Both axes pick their unreachable value over 50 and 25.
- **A time gate is not a fix for a wide fill spread.** The three-exit config moves 10.8x between
  fill models. Delaying entry tightens nothing and destroys the edge: PF 1.49 with no gate, 1.27
  at `time > 5`, 1.03 at `time > 15`, 0.90 at `time > 30`. Entering at creation is the edge, so
  the spread is a property to accept or reject, not a defect to engineer away.
- **A spike resolves into a plateau.** `retrace` 40 beats 30 and 50 badly on the coarse grid,
  which reads as overfit; refined against 36/38/42/45 it is a smooth plateau, and 40 holds.
- **This search covers only the seed rule's own metrics, so it prunes rather than discovers.**
  Its winner is a strict subset of the seed. That is the signature the "result that adds
  nothing" trap names, and the reason Stage 1 builds the pool from the catalog.

## Worked example - g3 `d5b5c6f3`, v1 to v2: a screen that reverses under the winning entry

Corpus 554 matched tokens (2026-07-23 to 08-11), 0.03 SOL, simulate:

| Config | Trades | Win rate | PnL (SOL) | PF | PnL under `first` + `fee_only` |
| --- | --- | --- | --- | --- | --- |
| v1 - 7 conditions, promoted straight from a grouped sweep | 272 | 0.35 | +0.744 | 1.37 | +3.594 |
| **v2 - 5 conditions** | 310 | **0.47** | **+1.307** | **1.56** | +2.046 |

```json
"entry": {"m_flow_window": {"unique_wallets": [{"operator": ">=", "value": 12.0}],
                            "net_flow":       [{"operator": ">=", "value": 3.0}],
                            "window_size_sec": 3.0}},
"exit":  {"m_flow_split_window": {"nonvol_buy": [{"operator": ">=", "value": 0.9}],
                                  "window_size_sec": 2.0},
          "m_price_lifetime":    {"stall":   [{"operator": ">=", "value": 300.0}]},
          "m_position":          {"retrace": [{"operator": ">=", "value": 35.0}]}}
```

- **v2's entry group is absent from v1.** `m_flow_window` reaches the winning rule only because
  the candidate pool covers metrics the seed rule does not carry.
- **A single screen inverts.** Measured against v1's entry, `stall`'s `off` row wins (1.030
  against 0.949) and `nonvol_buy` peaks at 2.3. Under the winning entry both reverse:
  `stall >= 300` is worth +0.28 and `nonvol_buy` belongs at 0.9. This is rule 3 in one example.
- **An entry band can cost more than it screens for.** v1's `liquidity > 15 AND < 20` reads as a
  quality gate; dropping it takes the corpus from 423 fired to 489 *and* raises PnL.
- **The fill-model spread is a finding.** v1 scores 4.8x higher on the optimistic fill; v2 moves
  1.6x. A wide spread is a warning even when both ends are profitable.

`unique_wallets` is the highest-value gate here (+0.50 SOL, PF 1.28 to 1.94) and is the same
metric an fs3-00 crowd gate refutes as anti-selecting. Both hold: it is a per-cohort question,
so screen it per fingerprint rather than carrying either verdict forward.

## Worked example - g0 `7f796a5a`: a parked metric is the best exit, and a value cliff

Corpus 832 matched tokens (2026-07-22 to 08-11), 0.01 SOL, worst fill + `impact`:

```json
"entry": {"m_snapshot": {"time": [{">", 3}, {"<", 15}], "liquidity": [{">", 12}, {"<", 18}]}},
"exit":  {"m_position": {"held": [{">", 240}]}, "m_flow_split": {"nonvol_net": [{">=", 0.5}]}}
```

86 trades / +0.2581 SOL / PF 2.45 against the seed rule's 44 / +0.0708 / PF 1.58.

- **`m_flow_split.nonvol_net` sits in the seed rule's `params.disabled` block** and is the single
  best exit available. A parked condition is an opinion from a previous pass, so it re-enters
  the candidate pool every time.
- **`m_position.retrace >= 40` is negative**: dropping it alone gains +0.067 SOL, and no value of
  it beats `off`. The DROP half of Stage 2 is what surfaces this.
- **`m_position.held` is load-bearing**: removing it collapses PnL to +0.0021. `stall`, exit
  `liquidity` and `m_flow_split_window.nonvol_buy` never bind and park on evidence.
- **`nonvol_net` has a cliff, not a smooth optimum.** 0.5 and 0.55 hold out of sample (PF
  1.74/1.84) while 0.3, 0.4 and 0.45 go **negative** out of sample despite scoring higher
  in-sample. This is the case a plateau check alone does not catch and the split does.
- Every candidate including the seed rule decays train to validate, so the cohort's edge
  concentrates before the split: a cohort verdict, not a rejection of the shortlist.
