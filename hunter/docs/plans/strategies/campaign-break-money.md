# Campaign-break money measurement (Phase 5 v0)

Honest-money study of entering on campaign-machine silence-breaks
([tool-census.md](tool-census.md) machines `29d9aacb`, `d2c86e7a`, `828457fa`).
Tables `census.money_v2..v4`, `census.break_path`. Window 08-30 17:48 .. 09-03.
**Every break is a candidate entry (re-entry per mint), not one trade per mint** —
the unit of study is the break: which silence break is up-moving.

## Fill model (the load-bearing correction)

The curve is the counterparty: a sell needs no later print. A timeout exit is a
clock, not a print event; it fills against the last observed reserve state
(silence freezes price), with our own impact charged both legs. Scoring a
no-print exit as proceeds 0 is fiction that charges -100% instead of ~-4% and
dominates the total. Entry fills at the first print >= break_slot+2 (our ~95ms
seat); costs 125 bps + 0.000225 SOL fixed per leg; graduated frozen states cap
at vsol 114.9.

## What the breaks do (6,566 entries, 0.05 SOL clip)

- Up-move is nearly universal: median path-max within 600s is +30..65% in price,
  63-100% of breaks exceed +8% (cost bar ~4%). The signal is real and, per the
  thermometer, almost uncontested by fast money.
- Which breaks are up-moving: entry vsol >= 65 (dev has committed most of the
  runway) AND break_idx >= 4 (the machine keeps returning) gives median up
  +55-65% vs median drawdown -16..25%. Entry vsol < 45 is the death zone
  (down > up); it holds all the v2/v3 losses.
- Money with TP +40% / timeout 600s / no stop: green slice +1.28 SOL over 2,706
  trades (+0.9%/trade), 2 of 5 days negative. A wash: 68% winners capped at
  +0.018 by the TP, 32% losers average -0.036 — failed late breaks are dumps,
  not drifts. The same decision node has two branches late in a campaign: push
  higher, or harvest exit liquidity; riding both blind nets ~0.
- First follow print <= 2s and upward is the best cell (+1.76 SOL, 71% win);
  downward first follow is negative. Third-level in-sample slice — treat as a
  hypothesis, not a result.

## Frozen rule v0 (before holdout - no further tuning on the derivation window)

Door: campaign machine (29d9aacb / d2c86e7a / 828457fa, or their pre-cutover
structural equivalents). Event: the machine's buy ends a >=10-slot silence.
Permission: e_vsol in [65,100) at our entry print (break+2 slots), break_idx >= 4,
pre-break 120s tape not buy-heavy (buy_sol <= 1.5 x sell_sol), entry below 97% of
the 30-min running max vsol. Position: 0.05 SOL, re-enter on later qualifying
breaks. Exit: TP at +40% price (vsol x 1.1832) resolved on prints, else 600s
clock; frozen-state fills. Derivation window read: +3.59 SOL / 1,227 trades /
71 mints / 73.2% win; 41 of 71 mints green; total without best mint +2.22.
The tells are climax-avoidance: buy-heavy pre-tape and at-the-high breaks are
the losing branch (buying the top of the push).

## Holdout (pre-cutover 08-28 .. 08-30 17:48, frozen rule, no tuning)

Both real machines exist verbatim pre-cutover (828457fa starts post-cutover and
drops out). Tables `census.hold_breaks` / `census.hold_money`. Result:
**+2.61 SOL / 1,278 trades / 87 mints / 72.5% win** (derivation: +3.59 / 1,227 /
71 / 73.2%) - days -0.01 / +0.96 / +1.65, survives best-mint removal (+1.72),
and the mint sets of the two windows are **fully disjoint** (0 shared): the rule
generalizes across different campaigns, not just different days. ~+4% per clip
per trade at 0.05 SOL. The true test remains forward days.

## Machine-class widening: refuted (walk-forward)

A behavioral screen on trailing (pre) data - breaks/mint >= 5, mints >= 5,
median buy >= 0.1 SOL, break-mint graduation >= ~10x baseline - recovers both
known machines blindly and adds three new ones. Traded forward (post window)
under frozen rule v0 the new machines LOSE: -2.60 SOL, 33% win. A trailing
money screen fails too: the two Lighthouse machines were green pre (60-62% win)
and red post. Small machines (6-8 mints per window) flip sign within days -
their evidence is noise at this horizon.

Per-machine truth under the full rule (pre -> post):

| machine | pre | post |
| --- | --- | --- |
| 29d9aacb | +2.37 / 72.9% / big n | +4.02 / 75.0% / 61 mints |
| d2c86e7a | +0.24 / 57.9% / 38 tr | +0.01 / 70.0% / 6 mints |
| 828457fa | absent | -0.44 / 4 mints |
| Lighthouse pair | +0.58 green | -2.73 red |

**The validated edge is one machine: `29d9aacb`.** Everything else is noise or
negative at current n. Consequences: (1) the Door is per-machine, and a machine
earns real allocation only by sustained green trailing money at adequate
effective n (mints, not trades) - candidates run on paper until then;
(2) template identity must be structure-normalized - the decoder rename
(Unknown L2TE.. -> Lighthouse: ix#05) silently split two machines' histories at
the cutover; a label-hash Door goes blind on every decoder upgrade.

## Engine reconciliation (Phase 6) - PASSED

The rule is authored in engine vocabulary by
[`scripts/seed-campaign-break-rule.sql`](../../../scripts/seed-campaign-break-rule.sql):
a **wildcard** fingerprint whose `m_flow_ix.ix_patterns` carries the machine's
exact ordered build (`wallet_contagion` and `creator_is_tagged` OFF - the claim
is that THIS print is that build, not that a wallet once matched); `entry_event`
= that print (`m_flow_ix_window`, `window_size_prints: 1`) AND a 10-slot buy
silence (`m_flow_window`, `window_size_slots: 10, window_lag: 1`); permissions =
`m_state.liquidity` 35..70 (real reserve = vsol - 30), `m_flow_ix.tagged_buy_count
>= 15`, `m_flow_window.buy_share <= 60` over 120 s, `m_price_window.trail > 5.9`
over 1800 s; exit = `take_profit 36` (a +40% price move net of costs) or
`m_position.held >= 600`; `reentry` per break.

Two SQL-vs-engine parity corrections came first, both narrowing the study:
`break_idx` does not exist as a metric (the engine counts lifetime machine buys,
`tagged_buy_count`; >= 15 selects the same population), and the engine holds
**one position per token at a time** while the study allowed overlapping
entries. Sequential, single-position SQL reads +0.65 SOL / 142 trades / 63 mints
(post) and +0.23 / 173 / 86 (pre).

Engine simulate, worst-case fills + `pumpfun_impact` costs, same windows:

| | SQL (sequential) | engine simulate |
| --- | --- | --- |
| post: trades / mints | 142 / 63 | **154 / 66** |
| post: net SOL / win | +0.65 / 73.9% | **+0.46 / 74.7%** |
| pre: trades / mints | 173 / 86 | **215 / 94** |
| pre: net SOL / win | +0.23 / 71.7% | **+0.55 / 75.3%** |

Win rate agrees within ~1 pp on the derivation window and ~4 pp on the holdout;
counts agree within 8-24%; both windows green in both measurements. The engine
books less money post (worst-case fill is adverse on both legs where the study
prices at last-known state) and more pre (it takes ~40 entries the sequential
SQL skipped). Exits: 108/154 and 147/215 take-profit, the rest clock or dead -
the +40% barrier is doing the work, as designed. This is the first rule of the
rebuilt workflow to pass Phases 5 and 6.

## The rule as the screen: every build, both periods (09-04)

The full rule v0 (event + permissions + exit, honest fills) run on every build
with >= 200 silence-breaks on >= 20 mints per period - 88 builds post, 59 pre;
tables `census.screen_post` / `census.screen_pre`. Result:

| | builds | trades | net SOL | builds green |
| --- | --- | --- | --- | --- |
| post-cutover | 68 | 11,820 | -34.2 | 1 |
| pre-cutover | 59 | 9,304 | -21.3 | 1 |

**Exactly one build is green on both periods with real n: `29d9aacb`**
(+2.46 / 84 mints / 73% pre; +4.13 / 61 mints / 75% post). `d2c86e7a` is green
both sides on 3 and 6 mints (noise). Every terminal, router and aggregator build
loses 4-7% per trade under the same rule.

Why that build: its instruction set has a generic-ordered twin, `80b0515a`
(CU-limit first) with the same 52k buys - spread over 19,632 mints (~2.7 per
mint, a spray: snipers and retail) and red. `29d9aacb` puts its 52k buys on
2,535 mints (~21 per mint): concentrated campaigns. The CU-price-first order is
the fingerprint of one client whose users campaign; concentration (buys per
mint) is the species, the order is how the tape spells it. The other
price-first variants (`BuyExactSolIn`, `BuyExactQuoteInV2`, ATA Create) are
sprays at 2-7 per mint.

Consequences: (1) the silence-break rule is client-specific, not a generic
curve edge - "more operators" means more *campaign clients*, and today the tape
has one with volume; (2) the weekly re-screen is this pass (rule money per
build, both blocks, n in mints), not a look-alike score; (3) the direct
extraction tell (the client's own sells) is refuted - the client is a tool used
by ~8.6k wallets, its sells precede 99.9% of qualifying breaks, and its net
direction flips sign across periods.

## Open

1. **Extraction tell** (the branch splitter): what marks a harvest-break before
   entry — the operator's own sells on the mint before the break, sell-template
   pairing from `census.tool_census_sell`, wall distance vs campaign age.
2. Exit redesign: p75 up-move is +90..105%, the +40% TP caps it; armed trail at
   engine latency is the candidate.
3. Everything above is derived in-sample on 3.7 days. Holdout = pre-cutover days
   (same machines under old label names) + forward days as they accumulate.
   Nothing ships before that and per-trade engine reconciliation.
