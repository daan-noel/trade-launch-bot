# Rule value tuning playbook — best metric values for a fingerprint

Given a saved fingerprint and (optionally) a rule already trading it, this finds the best
`RuleParams` values for it. It is the *values* half of the handoff: which tokens to trade is
[fingerprint-rule-handoff.md](fingerprint-rule-handoff.md), the phase-level research arc is
[metrics-path-profitable-rules.md](metrics-path-profitable-rules.md), and the sweep engine
itself is [../../arch/sweep.md](../../arch/sweep.md).

## The one rule that makes this work

**Every condition you might keep or drop is an axis with an `off` value.** A metric that is
not an axis cannot appear in a winning combo, so a grid that omits it produces no evidence
about it — and reading "it never won" as "it does not help" is a false conclusion, not a
measurement.

This is the whole reason for Stage 2. It is also the failure the g13 worked example below
records: a grid that omits `m_flow_split_window.nonvol_buy` parks that condition as useless
when it is in fact the single highest-value exit in the rule (median **+0.217 SOL** with it,
**-0.002** without — the strategy is break-even without it).

Corollary: when tuning a rule that already exists, **every condition already in it is a
mandatory axis**, at minimum `[null, <its current value>]`.

## The complete metric catalog — 38 metrics, 8 groups

SSOT is `hunter_engine::metrics::REGISTRY` (`hunter/engine/src/metrics/mod.rs`); a name that
disagrees with it fails the sweep at parse rather than silently no-opping. Semantics for the
flow groups live in [metrics-reference.md](metrics-reference.md).

| Group | Kind | Scope | Sides | Strict params | Metrics (unit) |
| --- | --- | --- | --- | --- | --- |
| `m_snapshot` | static | token | entry + exit | — | `time` (s), `liquidity` (SOL) |
| `m_price_lifetime` | static | token | entry + exit | — | `stall` (s), `trail` (%), `rise` (%) |
| `m_price_window` | **dynamic** | token | entry + exit | `window_size_sec` | `trail` (%), `rise` (%) |
| `m_flow_lifetime` | static | token | entry + exit | — | `gross_flow`, `net_flow`, `buy`, `sell` (all SOL) |
| `m_flow_window` | **dynamic** | token | entry + exit | `window_size_sec` | `gross_flow`, `net_flow`, `buy`, `sell` (SOL), `unique_wallets` (count) |
| `m_flow_split` | static | token | entry + exit | fp `volume_ix_patterns` | `vol_buy`, `vol_sell`, `vol_net`, `vol_gross`, `nonvol_buy`, `nonvol_sell`, `nonvol_net`, `nonvol_gross` (SOL), `vol_share` (%) |
| `m_flow_split_window` | **dynamic** | token | entry + exit | `window_size_sec` (+ fp patterns) | same nine as `m_flow_split` |
| `m_position` | static | **position** | **exit only** | `arm_above_pct` | `retrace` (%), `bounce` (%), `pnl` (%), `held` (s) |

Plus two non-metric axes: `take_profit` and `stop_loss` (both %, desugar into `m_position.pnl`).

Legality traps:

- **`m_position` is exit-only.** The sweep rejects it on the entry side — it reads `NaN`
  before a fill, so it could never fire.
- **`m_flow_split*` needs `volume_ix_patterns`** on the request *and* in the fingerprint's
  `metric_config`. Unconfigured, every flow-split metric is `NaN`, which satisfies nothing —
  the conditions read as present but never fire. Rule save warns; it does not reject.
- **A dynamic group needs `window_size_sec`.** Two axes on the same side+group with different
  windows become independent clauses, which is how you sweep a window.
- **`take_profit` / `stop_loss` axes reject `null`.** To test "no take-profit", either omit
  the axis entirely or pass an unreachable value (`1000`).

Combination semantics: **entry conditions AND together, exit conditions OR together.** Adding
an exit axis can only make exits fire earlier or as early, never later.

## Running it fast

This playbook's shape is many small runs, not one huge grid: Stage 2 is ~38 runs of a handful
of combos each. That cost is **corpus load + per-token `MetricSeries` build + exit scan**, not
the fold — 38 separate runs rebuild the same per-token series 38 times.

**The structural lever is to not run them separately.** The metric-discovery pipeline's
Layer 1 is exactly Stage 2, run as ONE `AdditiveStrategy` pass: every `(side, metric,
operator)` is a segment, all segments share one per-token precompute, and the engine builds
each token's series once for the whole screen. Cost is `Σ menu_len`, not `Σ runs`. Stage 2
below gives the endpoint; the per-metric runs stay as the audit route.

Four knobs on top, in order of payoff — they matter for Stages 3-4 either way:

1. **Keep the selection byte-identical across a stage.** The corpus cache is a *single slot*
   keyed on the selection hash (mints + trade counts + lake version), so all 38 Stage 2 runs
   pay one DuckDB load — but only while `fingerprint_id`, the date range, `token_cap` and
   `curve_only` stay fixed. Any change is a miss, which evicts and reloads for that run *and*
   every run after it. Vary axes inside a stage; vary selection only between stages.
2. **Run the lab in release.** `cargo run -p hunter-lab` is a debug build, where the vector
   exit scan is ~**2.3× slower** than scalar — the dev profile does not inline the intrinsics.
   Sweeping stages want `cargo run --release -p hunter-lab` (or the built `hunter-lab.exe`).
3. **AVX-512 On.** The sweep form's *AVX-512* radio sends `use_avx512: true`, which runs the
   per-`(combo × token)` exit scan on an 8×`f64` kernel: **2.2×** on the pnl-bound shape that
   every `take_profit`/`stop_loss` baseline in Stages 2-4 carries. It is byte-identical to
   scalar and is never persisted on the run row — *how the box computed*, not part of the
   analysis, so it can never move a result. Host-gated on `avx512f`: a box without it is
   forced to scalar with a toast. Exit shapes that are not a pure `pnl` bound delegate to the
   index path, so the win shrinks as a grid leans on flow/price conditions.
4. **Tighten the RAM reserve** to 512M/256M for these runs. Bigger resident series wave ⇒
   fewer precompute rebuilds; it costs desktop headroom, never fidelity.

Stage 5 is the same story one layer up: simulate is **cold scan + cold load** bound
(~53% + ~42%), and the engine fold is ~3% — a warm re-run of the same rule finishes in
milliseconds. So run one finalist's three checks back to back rather than interleaving
finalists. Measured numbers and the driver/sizing detail:
[../../arch/sweep.md](../../arch/sweep.md) and [../sweep/ram-sizing.md](../sweep/ram-sizing.md).

## Stage 0 — corpus and cohort health

```powershell
cd hunter
cargo run -p hunter-lab -- lake-export --include-today   # lake must cover the fingerprint's whole span
```

Then answer two questions before spending any compute:

1. **How many tokens does the fingerprint match, and over what span?** This is the hard ceiling
   on sample size. Under ~100 matched tokens, treat every result as directional only.
2. **Is the cohort still behaving the way it did?** Split the matched tokens by day and compare
   median curve life and median peak `liquidity`. A launcher that changes tactics turns a
   validated rule into one that never fires — and it shows up here, not in the sweep.

```sql
-- both answers, one query; substitute the fingerprint's own axes
WITH fp AS (
  SELECT mint_address, created_at, date_trunc('day', created_at)::date AS day
  FROM tokens
  WHERE ix_labels = '["..."]'::jsonb
    AND (initial_buy_instruction->>'max_cost_lamports') = '43200000')
SELECT f.day, count(*) AS tokens,
  round(percentile_cont(0.5) WITHIN GROUP (
    ORDER BY EXTRACT(EPOCH FROM (t.block_time - f.created_at)))::numeric, 0) AS med_curve_life_s,
  round(percentile_cont(0.5) WITHIN GROUP (ORDER BY t.reserve_lamports/1e9)::numeric, 1) AS med_vsol
FROM fp f JOIN trades t ON t.mint_address = f.mint_address AND t.venue = 'curve'
GROUP BY 1 ORDER BY 1;
```

Compare the pinned cohort against the same `ix_labels` *without* the continuous pins. When only
the pinned rows change behavior, the fingerprint is the thing that changed, not the market.

## Stage 1 — value menus from this fingerprint's own percentiles

The discovery run in Stage 2 derives these itself (`candidates::collect_percentiles` walks the
corpus once, before the screen, because the menus size the sparse grid's `time`/`stall`
horizons). Reach for the SQL below to audit a menu, or when screening by hand.

Global menus mislead: a gate below a metric's floor is a no-op, and one above its p95 selects
noise. Derive p10/p25/p50/p75/p90 **per fingerprint**, per metric, and pick 3-5 values spanning
that range.

```sql
WITH fp AS (SELECT mint_address, created_at FROM tokens WHERE /* fingerprint axes */),
tr AS (SELECT t.reserve_lamports/1e9 AS vsol,
              EXTRACT(EPOCH FROM (t.block_time - fp.created_at)) AS age_sec
       FROM trades t JOIN fp ON fp.mint_address = t.mint_address
       WHERE t.venue = 'curve' AND t.reserve_lamports IS NOT NULL)
SELECT 'liquidity' AS metric,
  round(percentile_cont(0.10) WITHIN GROUP (ORDER BY vsol)::numeric,1) p10,
  round(percentile_cont(0.25) WITHIN GROUP (ORDER BY vsol)::numeric,1) p25,
  round(percentile_cont(0.50) WITHIN GROUP (ORDER BY vsol)::numeric,1) p50,
  round(percentile_cont(0.75) WITHIN GROUP (ORDER BY vsol)::numeric,1) p75,
  round(percentile_cont(0.90) WITHIN GROUP (ORDER BY vsol)::numeric,1) p90
FROM tr
UNION ALL SELECT 'time', /* same five over age_sec */ 0,0,0,0,0;
```

Structural anchors that hold across fingerprints: `m_snapshot.liquidity` is the **real** SOL
reserve, not the virtual one — the engine feeds `TradeLite::reserve_sol` from
`real_reserve_sol` (`live/src/strategies/engine/producers.rs`), which is `vsol − 30` on the
curve. So it floors at **0** (empty curve) and tops out near **85** (migration): a gate written
against the virtual 30/115 scale is ~30 too high, and `liquidity >= 85` fires only on tokens
that actually migrate. The SQL above reads `reserve_lamports`, which *is* virtual — subtract 30
before comparing it to a rule value. `stall` is seconds since the **last all-time high**, so an
exit below ~60 fires on ordinary chop.

## Stage 2 — screen all 38 metrics

The question is only *does this metric move the number at all* — not what its best value is.
Each metric is swept **alone** against a fixed baseline, its menu carrying `off` as pick 0, so
its marginal is read straight off its own curve: with-vs-without, same cohort, same baseline.

### The one-run route (default)

`POST /api/strategies/metric-discovery` — the lab **Metric discovery** page
(`/strategies/metric-discovery`). One run screens every registry metric on both sides over one
precompute, then carries the survivors through a family grid and a time-split validation.

```json
{
  "fingerprint_id": "<uuid>",
  "curve_only": false, "token_cap": 100000,
  "buy_amount_sol": 0.01,
  "take_profit_pct": 30.0, "stop_loss_pct": 15.0,
  "min_closed": 30, "split_fraction": 0.7,
  "entry_window_sec": 30.0, "exit_window_sec": 2.0,
  "volume_ix_patterns": [["..."]]
}
```

Read `screen.shortlist` (kept metrics, each with `verdict`, `lift`, `plateau`, `best_value`
and a `narrowed` 2-3 value range) and `screen.responses` for the full curves. Nothing drops
silently: `screen.skipped` and `screen.gaps` name every metric that had no menu, no baseline,
or nothing but min-N-gated picks. The verdicts are the same three the manual route reads by
eye — `Keep` (smooth, directional, plateau), `DropNoEdge` (flat), `DropSpike` (one peak, noisy
neighbours = overfit). The page's **seed sweep** button writes the shortlist to the sweep form
as prefilled `axes` + `optional_axes`, which is the Stage 3 grid.

Two things this route does *not* decide for you:

- **Its baseline is a fixed TP/SL pair, not your rule.** A metric's lift here is its marginal
  over TP/SL alone, which is not its marginal over the conditions the rule already carries.
  The mandatory-axis rule still governs Stage 3: every existing condition goes in the grid.
- **`entry_window_sec` / `exit_window_sec` are one pair for the whole screen.** It screens
  *which* dynamic metrics matter, not which window they want — sweep the window in Stage 3.

### The manual route (audit, or one metric in isolation)

One run per candidate metric: a single axis of 3-5 values plus `null`, a handful of combos,
seconds each. Use it to re-check a `Keep`/`Drop` call, to screen against the live rule's own
params as the baseline instead of a TP/SL pair, or when a metric lands in `skipped`.

Screen a metric on both sides (except `m_position`, exit only). Baseline = the rule's current
params, or a neutral `take_profit`/`stop_loss` pair if there is no rule yet.

```json
{
  "strategy_id": "generic",
  "fingerprint_id": "<uuid>",
  "group_by": [], "curve_only": false, "min_tokens": 1,
  "min_fired_abs": 15, "fire_frac": 0.1, "method": "grid",
  "buy_amount_sol": 0.01, "fill_model": "worst", "cost_model": "impact",
  "token_cap": 100000, "max_combos": 20000, "ram_reserve_mb": 1024,
  "volume_ix_patterns": [["..."]],
  "axes": { "axes": [
    { "kind": "metric", "side": "entry", "group": "m_snapshot", "metric": "time", "operator": ">", "values": [10] },
    { "kind": "metric", "side": "exit", "group": "m_flow_split_window", "metric": "nonvol_buy",
      "operator": ">=", "window": 2, "values": [null, 1.0, 1.9, 3.0] }
  ] }
}
```

Keep a metric for Stage 3 when its best value beats its own `off` value on total PnL by more
than a rounding margin. Record the *marginal*, not just the winner:

```sql
SELECT COALESCE((c.params->'exit'->'<group>'-><metric>->0->>'value'), 'off') AS val,
       count(*) n,
       round(percentile_cont(0.5) WITHIN GROUP (ORDER BY r.total_pnl_sol)::numeric, 4) AS med_pnl,
       round(max(r.total_pnl_sol)::numeric, 4) AS best_pnl,
       round(percentile_cont(0.5) WITHIN GROUP (ORDER BY r.profit_factor)::numeric, 2) AS med_pf
FROM grouped_sweep_results r
JOIN grouped_sweep_combos c ON c.run_id = r.run_id AND c.combo_id = r.combo_id
WHERE r.run_id = '<run>' GROUP BY 1 ORDER BY med_pnl DESC;
```

A metric whose `off` row wins is genuinely parked — and now you can say so from evidence.

## Stage 3 — one combined entry + exit grid

Grid the Stage 2 survivors **plus every condition already in the rule**, entry and exit
together, each with its `off` value.

Budget the combo count first: it is the product of every axis length, capped at 1,000,000 per
group (`HARD_MAX_COMBOS`), and the practical ceiling on a small fingerprint is much lower than
the engine's. Aim for **3k-10k**. With ~8 axes that means 3-4 values each; spend the extra
values on the axes Stage 2 showed the steepest response on, and drop an axis to `[null, best]`
rather than adding a ninth.

Set the coverage floor so a lucky handful cannot be crowned:
`min_fired_abs` ≈ 30% of matched tokens, `fire_frac` 0.3. Scale that to what the rule
*fires on*, not to the corpus, when the entry gate is deliberately narrow — a rule taking 77 of
756 matched tokens rejects every combo against a corpus-fraction floor. There, set
`min_fired_abs` to ~65% of the baseline's fired count and drop `fire_frac` to match.

## Stage 4 — refine and confirm a plateau

Re-grid finer around the Stage 3 winner. What you want is not a better number but a **plateau**:
neighbouring values scoring the same. A single spiky peak surrounded by worse neighbours is
overfit to a handful of tokens.

Two plateau signals worth trusting:

- Adjacent values of an axis produce identical results ⇒ that condition rarely binds, so drop
  it and keep the rule smaller.
- Most combos in the refined region are profitable ⇒ the region is real, not a lucky corner.

## Stage 5 — validate

The sweep is a **ranking screener, not a backtest** — its numbers and simulate's differ, and
only simulate's are quotable. Re-run finalists through `POST /api/strategies/simulate` (a
`draft` body stays RAM-only; a `rule_id` body persists). Results come back on **POST**
`/api/strategies/simulate/{run_id}/result/summary`.

Three checks, in order:

1. **Authority run** — worst fill + `impact` cost, `buy_amount_sol` = the live notional. The
   fixed per-leg cost is size-sensitive, so a result at 0.05 SOL does not transfer to 0.01.
2. **Fill sensitivity** — re-run under `first` fill + `fee_only` cost. A config that is only
   profitable on the optimistic fill is an execution bet, not an edge.
3. **Out-of-sample** — split on token `created_at` (~70/30) via `until` / `since` and compare
   **per-trade** PnL, not totals. Reject on a large train→validate drop, profit factor ≤ 1 on
   worst fill, or a validate window too thin to read.

Ranking rule for the final pick: **rank by `total_pnl_sol`, reject anything with profit factor
below 1.3 or firing under the coverage floor.** Do not rank by the sweep's `score` — it carries
a `win_rate` floor of 0.01 and an open-position drag term, so it answers a different question
(it ranks a 33%-win-rate lottery above a steadier config with more money).

## Stage 6 — ship

Create the rule with `POST /api/strategy-rules` rather than a SQL insert, so `RuleParams::parse`
validates every group and metric name against the registry.

- Name it `<original> --- v<n>`; keep the original untouched for comparison.
- **Park, do not delete.** Conditions the sweep rejects go under `params.disabled` in the same
  shape, so the next tuning pass can see what was tried. They are validated, never compiled.
- Ship `is_active: false`. Flipping it on is a separate, deliberate act.
- Carry `trade_mode`, `buy_amount_lamports`, `max_concurrent_tokens` and `tags` from the original.

## Traps

| Trap | The rule |
| --- | --- |
| A metric that is not an axis | It cannot win, so it produces no evidence. Never park a condition a grid did not test. |
| `score` vs money | `score` floors `win_rate` at 0.01 and penalises open positions. Rank the final pick on `total_pnl_sol` with a PF gate. |
| Sweep numbers quoted as PnL | The sweep approximates; `simulate` is the authority. Re-run before believing a number. |
| Take-profit assumed good | Capping upside is a real cost where the edge is the right tail. Always sweep TP against an unreachable value. |
| Flow-split silently dead | No `volume_ix_patterns` ⇒ all nine metrics `NaN` ⇒ conditions never fire, and nothing errors. |
| `stall` misread | Seconds since the last all-time high, not since the last trade. `m_position.held` is the time stop. |
| Unarmed `retrace` | Without `arm_above_pct` the peak seeds at entry, so `retrace` is a hard stop from entry, not a trailing stop. |
| AVX-512 under a debug build | The vector scan is ~2.3× *slower* there. Toggle it on only when the lab runs `--release`. |
| A stage that reloads the corpus | The cache is one slot on the selection hash. Changing the range, `token_cap` or `curve_only` mid-stage re-pays the DuckDB load on every remaining run. |
| Only ~365 rows persist per group | Retention keeps top/bottom 3 distinct values of 11 metrics, capped 10 per tie, plus the winner. A big grid is not fully readable afterwards — keep comparison grids small. |
| Stale scoped sweep rows | Runs with a `fingerprint_id` created before 2026-08-11 are truncated by `token_cap`; re-run them. [history](../../history/2026-08-11-scoped-sweep-token-cap-truncation.md) |
| Cost constants moved | Runs stored before 2026-07-28 price at 100 bps with no impact charge and do not compare. [execution-costs.md](execution-costs.md) |

## Worked example — g13 `897353e1`, v13 to v14

Corpus 106 matched tokens (2026-07-25 → 08-09), 0.01 SOL, worst fill + `impact`, simulate:

| Config | Trades | Win rate | PnL (SOL) | PF |
| --- | --- | --- | --- | --- |
| original rule, 6 hand-authored exits, no stop | 51 | 0.49 | +0.088 | 1.64 |
| v13 — grid omits two of its own conditions | 52 | 0.33 | +0.143 | 1.58 |
| v13 + `nonvol_buy` restored by hand | 52 | 0.42 | +0.152 | 1.75 |
| **v14 — every condition an axis** | 52 | **0.48** | **+0.171** | **1.94** |

v14's exit is three conditions plus a stop:

```json
"exit": {
  "m_flow_split":        {"nonvol_net": [{"operator": ">=", "value": 5.0}]},
  "m_flow_split_window": {"nonvol_buy": [{"operator": ">=", "value": 1.9}], "window_size_sec": 2.0},
  "m_price_lifetime":    {"stall":      [{"operator": ">=", "value": 240.0}]}
},
"stop_loss": 25.0
```

What the staged method surfaces that the first grid cannot: `nonvol_buy` is the highest-value
exit in the rule; `nonvol_net` belongs at **5**, not the promoted **9.5** (+0.171 / PF 1.94
versus +0.154 / PF 1.76); and `retrace`, `m_flow_window.buy` and `m_flow_window.sell` park on
evidence. The Stage 4 refine returns identical results for `stall` 240/270/300 and for
`liquidity >= 90` versus off — a plateau, and the reason v14 carries three conditions instead
of five.

Cohort caveat, and the reason Stage 0 exists: from 2026-08-07 this fingerprint's median curve
life drops to under 20 s against 260-1360 s before, with median peak `liquidity` pinned at the
115 migration ceiling, so the rule fires roughly 4 times per 5 days. The same `ix_labels`
without the `max_cost` pin hold a 63-154 s median across the same days.

## Worked example — g3 `d5b5c6f3`, v1 to v2: a screen that reverses under the winning entry

Corpus 554 matched tokens (2026-07-23 → 08-11), 0.03 SOL, simulate:

| Config | Trades | Win rate | PnL (SOL) | PF | PnL under `first` + `fee_only` |
| --- | --- | --- | --- | --- | --- |
| v1 — 7 conditions, promoted straight from a grouped sweep | 272 | 0.35 | +0.744 | 1.37 | +3.594 |
| **v2 — 5 conditions** | 310 | **0.47** | **+1.307** | **1.56** | +2.046 |

```json
"entry": {"m_flow_window": {"unique_wallets": [{"operator": ">=", "value": 12.0}],
                            "net_flow":       [{"operator": ">=", "value": 3.0}],
                            "window_size_sec": 3.0}},
"exit":  {"m_flow_split_window": {"nonvol_buy": [{"operator": ">=", "value": 0.9}],
                                  "window_size_sec": 2.0},
          "m_price_lifetime":    {"stall":   [{"operator": ">=", "value": 300.0}]},
          "m_position":          {"retrace": [{"operator": ">=", "value": 35.0}]}}
```

Three things this example carries that the g13 one does not:

- **Stage 2 in isolation can invert.** Screened against v1's own entry, `stall`'s `off` row wins
  (1.030 versus 0.949) and `nonvol_buy` peaks at 2.3. Under the Stage 3 winning entry both
  reverse: `stall >= 300` is worth +0.28 and `nonvol_buy` belongs at 0.9. A metric is screened
  against one baseline, so Stage 2 ranks *candidates to grid*, never final values.
- **The fill-model spread is itself a finding.** v1 scores 4.8x higher on the optimistic fill
  than the worst-case one; v2 moves 1.6x. Read that ratio as how much of a config is an
  execution bet — a wide spread is a warning even when both ends are profitable.
- **An entry band can cost more than it screens for.** v1's `liquidity > 15 AND < 20` reads as a
  narrow quality gate; dropping it takes the corpus from 423 fired to 489 *and* raises PnL.
  Anything that only ever narrows deserves an `off` value.

`unique_wallets` is the highest-value gate here (+0.50 SOL screened, PF 1.28 → 1.94) and is the
same metric an fs3-00 crowd gate refutes as anti-selecting. Both hold: it is a per-cohort
question, so screen it per fingerprint rather than carrying either verdict forward.
