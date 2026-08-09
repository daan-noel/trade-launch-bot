# Grouped-sweep axis value candidates (empirically grounded)

Covers two levers: the **metric / TP / SL axes** (what the sweep varies within a
group) and the **fingerprint grouping set** (how the corpus is partitioned into
cohorts). Both grounded in the local lake. Fingerprint section at the bottom.

Candidate value sets for the generic sweep's metric/TP/SL axes, derived from the
local Parquet lake (curve venue only, `price > 0`): statics + future-extremes over
all good days 2026-07-01..07-08 + 07-20/21 (11.0M trades, 122K tokens), rolling
window flows over 07-03 + 07-20 + 07-21 (2.36M trades), ix structures over
07-20/21. Age-conditioned ("HOT") subsets cover the recent days only (the tokens
dimension joined 730K recent rows); recent-vs-all percentiles matched closely
everywhere they overlap, so the numbers generalize.

Query script (throwaway, session scratchpad): DuckDB over
`$SWEEP_LAKE_DIR/trades/dt=*/data.parquet`. Re-derive any time; the percentile
anchors below are the permanent record.

> **Now generated.** `lab/src/discovery/candidates.rs` (`screen_plan` →
> `collect_percentiles` → `build_menus`) derives this ladder and these menus for any
> cohort straight off the metric `REGISTRY` — measured through the engine's own
> `MetricSeries`, not re-derived in SQL. The tables below stay as the recorded
> ground truth for the 2026-07 lake and as the sanity check a generated menu is
> compared against; a **new** metric needs no hand-derivation pass. Module
> map + architecture: [../../arch/sweep.md](../../arch/sweep.md) "Metric-combo
> discovery pipeline".

**Open decision carried forward (D1 — never pinned):** the discovery objective
(`robust_profit × fire_rate × win_component × min_n_gate` in `discovery/objective.rs`)
runs on tunable constants — `OPEN_HAIRCUT` (unrealized-mark discount), the
`profit_factor` cap, `MIN_CLOSED` (the anti-overfit floor below which a combo scores
zero), and the plateau-penalty weight. They're seeded from the percentile anchors below,
not validated against real outcomes. Pin the final set here, in a new subsection, once a
discovery run's picks are checked against live/paper results.

Subsets used:
- **ALL** - every curve trade moment.
- **HOT** - `age >= 120s AND vsol in [40, 115]` (the blueprint universe gate;
  n = 283K).
- **HOT+DIP** - HOT and `trail 5..30%` (the dip-entry regime; n = 104K).

## Percentile anchors (the ground truth)

Values at trade moments; `fut_*` = extreme of the token's remaining lifetime
price path from that moment (TP/SL reachability).

| metric | subset | p05 | p10 | p25 | p50 | p75 | p90 | p95 | p99 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `time` (age s) | ALL(recent) | 1.3 | 4.9 | 22 | 98 | 357 | 938 | 1497 | 6.2K |
| `liquidity` (vsol) | ALL | 30.8 | 32.5 | 39 | 52.6 | 71.5 | 87 | 96 | 107 |
| `liquidity` | HOT+DIP | 49 | 53 | 63 | 77 | 90 | 99 | 103 | 108 |
| `trail` % | ALL | 0 | 0 | 2.6 | 17.4 | 41.5 | 65 | 76 | 88 |
| `trail` % | HOT | 0 | 0.1 | 6.4 | 22.8 | 43.4 | 61 | 69 | 79 |
| `stall` s | ALL | 0 | 0 | 1.3 | 15.3 | 94 | 421 | 1005 | 4.3K |
| `stall` s | HOT | 0 | 0.2 | 15 | 91 | 282 | 766 | 1504 | 5.7K |
| `fut_gain` % | HOT | 0 | 0.8 | 9.2 | 29 | 72 | 140 | 199 | 355 |
| `fut_gain` % | HOT+DIP | 0 | 1.6 | 11 | 30 | 67 | 124 | 178 | 309 |
| `fut_dd` % | HOT | 1.9 | 9.5 | 39 | 64 | 78 | 86 | 89 | 91 |
| `net_flow@10` | HOT | -10.3 | -6.5 | -2.3 | 0.1 | 1.8 | 4.9 | 7.3 | 14 |
| `net_flow@30` | HOT | -14.7 | -10 | -3.9 | 0.3 | 3.6 | 8.4 | 12 | 21 |
| `net_flow@60` | HOT | -18.4 | -12.5 | -4.8 | 0.9 | 5.7 | 12 | 17 | 28 |
| `gross_flow@30` | HOT | 1.1 | 2.3 | 6.0 | 15.8 | 32 | 56 | 75 | 112 |
| `gross_flow@60` | HOT | 2.5 | 4.7 | 12.5 | 31 | 62 | 107 | 141 | 208 |
| `buy@30` | HOT | 0.3 | 1.0 | 2.9 | 7.3 | 15.7 | 28 | 39 | 58 |
| `sell@30` | HOT | 0.1 | 0.4 | 2.2 | 7.7 | 17 | 30 | 40 | 58 |

Per-token: life p50 110s / p75 250s / p90 645s; max stall p50 74s / p90 416s.

**Structural facts the values must respect:**
- `vsol` floor is ~30 (an empty curve holds 30 virtual SOL): a `liquidity >`
  gate below 35 is a no-op; migration sits ~115 (p99 = 107).
- Half of all trade moments sit >= 17% below the lifetime peak; `trail` entry
  gates below ~5 barely filter, above ~40 select mostly-dying tokens - pair any
  deep-dip gate with a `gross_flow` liveness gate.
- From a HOT moment the median remaining upside is +29% but the median eventual
  drawdown is -64%: uncut positions die. TP > 150% is past p90 reachability;
  SL wider than ~40 mostly rides tokens to death.
- `stall` is the time since the last NEW HIGH (peak clock), not since the last
  trade. HOT median is already 91s - exit `stall >` values below ~60 fire on
  ordinary chop.
- Windowed flows at HOT moments are balanced (net@30 p50 = +0.3): net-flow
  cutpoints only a few SOL from 0 already select the p25/p75 tails.

## Candidate value menus (per axis)

Percentile-spaced; pick 3-5 per axis per run. `off` = the null sentinel (sweep
with-vs-without). Windows ARE sweepable within one run: two axes on the same
(side, group) with different `window_size_sec` assemble into separate
`GroupConditions` instances (the engine's multi-window-per-group model), so e.g.
`m_flow_window.buy` at both 30s and 60s can run in one grid. Distinct groups
(`m_flow_window` vs `m_price_window`) always carry independent windows.

| side | axis | operator | candidates (full menu) | anchors |
| --- | --- | --- | --- | --- |
| entry | `m_snapshot.time` | `>` | off, 30, 60, 120, 300, 600 | p25..p90 of trade age; blueprint gate 120, omego med entry 780 |
| entry | `m_snapshot.liquidity` | `>` | off, 35, 45, 55, 70 | p10/p25/p50/p75; 35 ~ "any traction" |
| entry | `m_snapshot.liquidity` | `<` | off, 90, 100, 110 | p90/p95/p99 - pre-migration cap |
| entry | `m_price_lifetime.trail` | `>` | off, 5, 8, 15, 25, 40 | HOT p25..p75; blueprint dip depth 8-25 |
| entry | `m_price_lifetime.trail` | `<` | off, 35, 60 | cap vs dead-dump (HOT p75/p90) |
| entry | `m_price_lifetime.stall` | `<` | off, 5, 15, 60 | momentum variant only (recent new high) |
| entry | `m_flow_window.gross_flow@30` | `>` | off, 5, 10, 25, 50 | HOT p25/p40/p70/p90; blueprint hot gate 10 |
| entry | `m_flow_window.net_flow@30` | `>` | off, -5, 0, 3, 8 | HOT p20/p50/p75/p90 |
| entry | `m_flow_window.buy@30` | `>` | off, 3, 8, 15, 30 | HOT p25/p50/p75/p90 (momentum variant) |
| exit | `m_price_lifetime.stall` | `>` | off, 60, 120, 300, 600 | HOT p40/p60/p75/p90; deadness verdict is the true floor |
| exit | `m_price_lifetime.trail` | `>` | off, 20, 35, 50, 65 | must exceed the entry dip gate or it fires at entry |
| exit | `m_flow_window.net_flow@10` | `<` | off, -2, -6, -12 | HOT p35/p10/p05 - dump-detector exit |
| exit | `m_flow_window.sell@10` | `>` | off, 7, 18, 34 | HOT p75/p90/p95 of sell@10 (alt dump-detector) |
| - | `take_profit` | - | 20, 30, 60, 100, 150 | fut_gain HOT ~p40/p50/p75/p85/p90 |
| - | `stop_loss` | - | 10, 15, 25, 40 | fut_dd HOT ~p10/p15/p25/p40; blueprint catastrophe SL 25 |

## Combo cap + search behavior (read before sizing a grid)

`MAX_COMBOS` default is **100k**; `HARD_MAX_COMBOS` is **1,000,000** (raise via
the form's *Max combos/group*). A full **Grid runs exactly as chosen and is
evaluated exhaustively** — the old "auto-convert to LHS+refine past 200k" is
**gone** (`registry.rs` ~L700: a grid over the cap now *bails* with actionable
guidance; coarse+refine is opt-in via `refine:N:K`). So a bigger cap = a finer
*complete* grid, no silent sampling. RAM pressure degrades (threads→fold budget),
never refuses, except a true-floor overflow.

## Recommended primary grid (dip-reversion themed, ~187K combos)

Entry 648 x exit 288. Set *Max combos/group* >= 200k (default 100k would bail).

```text
entry  time            > {off, 120, 300}
entry  liquidity       > {off, 45, 60}
entry  liquidity       < {off, 100}
entry  trail           > {off, 8, 15, 25}
entry  gross_flow@30   > {off, 10, 25}
entry  net_flow@30     > {off, -5, 0}
exit   stall           > {off, 60, 300}
exit   trail           > {off, 20, 35, 50}
exit   net_flow@10     < {off, -3}
       take_profit       {30, 60, 100, 150}
       stop_loss         {15, 25, 40}
```

## Fine-resolution grid (1M-budget, 777,600 combos, exhaustive)

Extra budget spent on resolution where the edge lives (dip trigger + TP/SL), not
new axes. Set *Max combos/group* = **1,000,000**. Entry 1,080 x exit 36 x TP.SL 20.

```text
entry  time            >= {off, 120, 300}          # 3
entry  liquidity       >= {off, 45, 60, 70}        # 4
entry  liquidity       <  {off, 100}               # 2
entry  trail           >= {off, 8, 15, 25, 40}     # 5
entry  gross_flow@30   >= {off, 10, 25}            # 3
entry  net_flow@30     >= {off, -5, 0}             # 3
exit   stall           >= {off, 60, 300}           # 3
exit   trail           >= {off, 20, 35, 50}        # 4
exit   net_flow@10     <  {off, -3, -6}            # 3
       take_profit        {20, 35, 50, 100, 150}   # 5
       stop_loss          {10, 15, 25, 40}         # 4
```

Trim to ~389k by dropping entry-liquidity `70` and exit-net-flow `-6` if RAM/time
drags. 4G RAM-reserve radio makes it slower; 1G default is fine for a dedicated
lab run. The sweep won't refuse it — it degrades and toasts each step.

Variant B (momentum, swap-in): entry `trail < {off, 5, 10}` + `stall < {off, 5,
15}` + `net_flow@30 > {0, 3, 8}` + `buy@30 > {off, 8, 15}`; drop the dip axes.
Fast-scalper TP/SL sub-grid: TP {10, 20, 35}, SL {8, 15, 25} (omego profile:
med win +7.6%, med loss -5.3%, catastrophe -25).

Notes:
- Include `off` on every metric axis: the marginal value of each gate is read
  directly off the ranked table (combo with vs without).
- A deep entry `trail >` pick combined with a low exit `trail >` pick yields
  instant exits (~0 PnL rows) - expected; the ranking discards them.
- `m_flow_split` / `m_flow_split_window` axes need `volume_ix_patterns` configured
  per run; corpus-wide percentiles are pattern-dependent, so no fixed menu.
  Principled `vol_share` cutpoints: `<` {25, 50, 75}. The 07-20/21 ix-structure
  scan flags nonce-pumped spam shapes (e.g. AdvanceNonceAccount sell structures
  with 92 wallets / 3.5K trades) as natural volume-side patterns; the top
  routers by gross are Axiom (~37% across its shapes), Terminal, GMGN, direct
  Pump.Fun.
- The blueprint's rolling-window dip metric and since-entry-peak retrace have since
  shipped as `m_price_window`/`m_position`. See
  `docs/plans/strategies/wallet-analysis.md`.

## Fingerprint grouping set (the partition, not the sweep)

The grouped sweep partitions the corpus by fingerprint fields and finds the best
combo **per cohort**. Two facts dominate the choice, both measured on the lake.

### Fact 1 — the sweep corpus IS the fingerprint dimension (coverage cap)

`resolve_candidates` (`lab/src/lake/duck.rs` ~L288) selects candidate mints
**from `tokens/tokens.parquet`**, not from the trades. A traded token with no
dimension row is never a candidate. Measured:

- tokens dimension total: **9,792** rows.
- distinct curve-traded mints in the lake: **121,970**.
- traded mints WITH a fingerprint row: **7,636**.
- tokens worth sweeping (>=30 curve trades): **32,365**; of those WITH a
  fingerprint: **2,322 (7%)**.

So grouping (and the whole grouped sweep) runs on ~7% of the tradable universe
before any partition. This is a **dimension-backfill gap** (`tokens`/`tokens_info`
in local PG only covers recently-captured launches) — no field choice fixes it.
The metric-axis values above operate on the full trade corpus and are unaffected;
grouping is the weaker lever until the dimension is backfilled.

### Fact 2 — the default field set over-fragments even that 7%

Group-size on the 2,322 busy fingerprinted tokens:

| set | groups | groups >=20 tok | singletons | tokens lost @ min_tokens=20 |
| --- | --- | --- | --- | --- |
| 7 fields (ix + cu_limit + cu_price + max_sol_cost + spendable + fslot_buy + fslot_sell) | 1,119 | 11 | 867 (77%) | 84% |
| ix_labels only | 66 | 15 | 20 | 10% |
| ix + first_slot_buy /5 SOL | 177 | 24 | 67 | 24% |

Per-field cardinality among traded tokens (distinct / coverage):

| field | distinct | coverage | verdict |
| --- | --- | --- | --- |
| instruction labels (`ix_labels`) | 95 | 100% | **KEEP** — launch platform/pattern identity; the one strong, fully-covered field |
| first-slot buy SOL | 3,036 (54 @1SOL) | 100% | **KEEP coarse** — launch heat; use bucket width 5 (p50 2 / p75 7 / p90 12 / p99 45) |
| first-slot sell SOL | 263 (9 buckets) | 100% | optional — modest sniper-dump signal, adds little over first-slot-buy |
| initial buy SOL | 827 | 96% | drop — correlated with first-slot-buy, high cardinality |
| CU limit | 1,552 | 70% | DROP — near-unique tooling noise, 30% null |
| CU price | 537 | 69% | DROP — high cardinality, 31% null |
| max SOL cost | 826 | 72% | DROP — near-unique, fragments hard |
| spendable SOL in | 101 | **25%** | DROP — 75% null |
| cashback | 2 | 100% | drop — weak boolean, not worth the x2 |
| mayhem mode | 1 | 100% | DROP — constant (already the corpus filter) |

### Recommended set

- **Safest:** `instruction labels` only. 66 clean groups, biggest 498 tokens,
  10% waste. Stratifies by the field that correlates with trajectory without
  shattering the population.
- **With launch-heat split:** `instruction labels` + `first-slot buy SOL`, and
  set **Bucket width = 5 SOL** (not 1). 177 groups, 15 holding >=50 tokens —
  separates cold launches (fslot-buy <5) from hot/sniped (>25).
- Set **min_tokens = 20**; uncheck every other field (they're off, not just
  lower-priority). Keep `instruction labels` as priority #1.

Bucket-width sweep on ix + first_slot_buy (waste @ min_tokens=20): /1 = 50%,
/2 = 35%, /3 = 28%, /5 = 24%, /10 = 19%, /25 = 12%. 5-10 is the sweet spot
(coarse enough to fill groups, fine enough to keep the heat signal).
