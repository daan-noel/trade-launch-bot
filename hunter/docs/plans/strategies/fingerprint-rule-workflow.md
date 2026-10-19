# Fingerprint rule workflow: one fingerprint, one rule

How one fingerprint's coins become one rule (entry + exit). A fingerprint is one creation
shape, so its coins are one operator (or one tool) running one playbook. The operator makes
money by eating the outsiders' SOL: he pumps with his own volume, outsiders buy in, and at a
checkpoint he dumps. The rule's edge is **avoiding his threats and holding what migrates**:
sell on the curve only when a threat shows (his dump into the crowd, a coin he abandons, a dead
pool), hold every other coin through migration, and leave the AMM on one simple line. Selling
every coin at its first outsider bump takes the migrated coins' SOL with it. Every line is one
of his rules, stated first and tuned second.

Every number passes [backtest-audit.md](backtest-audit.md) before it is a result.

## Fixed setup

| What | Value |
| --- | --- |
| Universe | every coin of the fingerprint born in the window; no filter on a coin's future (life, peak, graduation) |
| Tickets | one entry per coin, no re-entry; entry at creation or at any later print |
| Size | 0.05 SOL |
| Fill | both legs: the state after the last print landed by decision + 115 ms (audit F1) |
| Cost | `pumpfun_impact`, `FEE_BPS_PER_LEG` 125, fixed leg cost from `FeeTuning` |
| Research | the most recent 14 complete UTC days |
| Eval | the days before research that PG still holds; read once, at step 6 |
| Objective | total research SOL, under a cap on migrated coins sold on the curve (3-10 % of them) |
| Data | curve and AMM tape: `sync-mints --incremental --amm-slots 54000` for every coin that completed the curve, then `lake-export` (lab stopped; `LAKE_RETENTION_DAYS` covering the eval days) |

## The factors, in order

| Tier | Factor | Reads |
| --- | --- | --- |
| 1 | the operator's payoff | `m_holdings.profit_sol @volume` - what he nets dumping his bag now (outsider SOL and his own together) |
| 1 | outsider status | `@!volume` flow (`buy_sol`, `net_sol`, `gross_sol`), burst (short spans), busyness (`buy_count`, `trade_count`, `m_crowd.unique_wallets`) |
| 1 | operator volume state | `@volume` flow and bag: pushing (`buy_sol [5s]`), quiet, selling (`bag_sold_pct`, `net_sol` < 0) |
| 1 | where in life | `m_state.age_sec`, `m_state.liquidity_sol` |
| 2 | the price path and the rest | `m_price.*`, ix-shape counts, other tags |

Tier 2 enters only after tier 1 is exhausted, and only as a line with its own story (for
example "the dump has started: price down 45 % in 30 s").

## Laws

1. **Decision-time values only.** Every condition is a registry read (`MetricRef`) taken from
   the engine's own metric series at that print.
2. **An exit fires only at or after our fill.**
3. **Eval decides nothing.** No threshold, line, stage or stopping point is chosen from it.
4. **Every line has a story.** One sentence in the operator's terms: what he sees, what he
   does next. A line that cannot be said that way is not added, whatever it gains.
5. **Every line passes the gate** on research:
   - total SOL rises by at least 1 % of capital (0.05 SOL x coins entered);
   - the gain is positive on at least 5 of the 7 research days;
   - it changes the outcome of at least 20 coins;
   - it is not true at the fill on most coins (a line true by construction carries no information).
6. **Thresholds sit on a plateau.** The SOL-vs-threshold curve is printed and read: smooth, one
   direction, the value taken in the middle of the range that keeps 90 % of the best gain.
   A one-point peak is noise.
7. **Budget.** Stage cells hold at least 20 research coins (thinner cells merge into a
   neighbour); at most 3 conditions per line.
8. **No approximated metric.** When a finding has no exact registry metric, extend the metric
   system ([CLAUDE.md](../../../../CLAUDE.md) rules); until then the condition stays out and the
   gap is written in the notes.

## Steps

The operator is a bot: his playbook is a few simple rules ("dump when my profit reaches X at
this liquidity", "abort when no outsiders came by age T"). The work is to read that policy off
the data, then sell just before each of his rules fires. PnL tunes only the few parameters
the policy leaves open.

### 0. Freeze and kernel

- Pull the cohort's trades and the engine's metric series for every coin, research + eval.
- Kernel: for every row, the value of buying and of selling there, each filled at +115 ms.
- Check the kernel once against the engine on any lab rule (research SOL within 1 pp).
- Output: coins per day, snipe + hold, and the oracle (best exit per coin = the ceiling;
  where the best entry sits says whether the money is at creation or mid-life).
- Tag check: a `volume` tag built from ix shapes can match most of the tape. Its wallets'
  market-wide mint count (3 weeks) says whose they are: wash wallets trade a handful of mints,
  outsiders dozens.

### 0b. Threat rule order (hold to migration)

1. **Outcome classes, no PnL:** migrated, runner (peak >= 3x, no migration), fade, flop. Snipe +
   hold split by class says where the SOL is lost and what migrated coins carry.
2. **AMM exit first** - it sets what holding a migrated coin is worth. Holding to the AMM's end
   loses (the pool dumps within minutes of migration); one line on the AMM rows
   (`m_state.on_curve < 0.5`) is the exit, picked from a flat landscape.
3. **Curve threat lines against hold + that AMM exit.** Grade every (stage gate, tier-1 read,
   direction, threshold) as a first sell per coin, split by class, with the migrated coins it
   sells. The best unconstrained line sells nearly every coin at the first outsider bump; the
   migrated cap is what turns the search into threat lines.
4. **Combine and tune jointly** (OR = earliest fire) under the cap; each threshold at the middle
   of its plateau; leave-one-day-out on research; every line +/- 1-2 grid steps. A line whose
   value sits at its grid's edge, or whose one-step move sells most migrated coins, is dropped.
5. **Pre-entry veto in the kernel:** the engine does not buy on a print where a sell line holds,
   and the snipe has one chance. A line true at the creation print (for example "liquidity < 10,
   no outsider sells") skips every coin; such a line needs a gate past the opening.

### 1. Read cases (no PnL)

- Chart about 20 research coins (runners, middle, flops): price, operator `@volume` flow,
  outsider `@!volume` buys and sells, `profit_sol @volume`, liquidity, age.
- Write down in plain words what he does and when: push, wait, turn, abort.

### 2. His policy

- Mark his decisions on every coin: the **turn** (the price peak his volume bot sells into)
  and the **abort** (a coin that never gets its crowd: the peak sits early and low).
- At each decision read the tier-1 factors. A trigger is a factor that sits at a nearly fixed
  value, or a fixed function of a stage axis, at his decision on most coins: its spread across
  coins (log ratio p75/p25) is small next to the other factors at the same moment, and it is
  well below that value earlier in the same coin.
- The stage axes are the ones his trigger value depends on (liquidity, age, outsider count).
  Bands come from where the trigger value shifts; cells hold at least 20 coins.
- Output: the policy as 2-4 rules, each "factor reaches f(stage) -> he turns / aborts".

### 3. Sell lines from the policy

- One sell line per policy rule: his trigger factor at a fraction of his trigger value
  (`factor >= m x f(stage)`), so we sell just before him. A flop line: the abort state.
- Open parameters: the margin `m` per line and, where the data asks, the trigger value per
  band. Nothing else.

### 4. Tune and diagnose (loop)

- Tune the open parameters on research PnL: the plateau rule (law 6) and the gate (law 5)
  per line.
- Break the losses down by coin type and stage cell. Read the charts of the worst coins
  (step 1 again), find the part of his policy the lines miss, add it as a policy rule.
- Stop when the remaining losses carry no pattern. Tier 2 enters only here, as a safety line
  with its own story ("the dump has started").

### 5. Entry

- Exit fixed. The snipe against a later entry or a filter taken from the policy (buy while
  the outsider money he waits for is still ahead). A change passes only when total SOL rises.
- Early signals barely separate soon-dead coins from live ones (AUC 0.55-0.62; nothing at
  creation). The one strong filter is the operator's commitment: enter after his volume bot
  has sold at least X SOL (`m_flow.sell_sol @volume`) within the first seconds. It halves the
  dead share of entered coins and keeps the SOL per coin; read the threshold on a plateau.

### 4b. Improving a known rule

- Start from the best rule in hand and change one idea at a time; test each change in the
  engine, not only in the kernel. Search lines jointly: a line that pays only together with
  another fails a one-line-at-a-time gate.
- Score a day with parameters fitted on the other days (leave-one-day-out, or blocked folds).
  A free refit of every threshold on a few days loses to the known rule's own thresholds on
  unseen days.
- Exit shape: bank-and-ride beats an armed trail. A runner signal is read at the moment the
  outsiders cash in: liquidity at that moment (below 35 almost never migrates, 65 and above
  mostly does). At or above the runner level the cash-in line does not sell; the coin moves
  to a ride stage (`m_position.retrace_pct` sell, migration sell).
- The kernel mirrors the engine's pre-entry veto: a coin is not bought on a print where an
  `always` line or a first-stage sell line already holds, and the entry keeps watching. Without
  it the kernel buys and sells those coins at once (75 of 262 on the dev-sells entry) and its
  entry count and SOL do not match the engine. Check the kernel against the engine coin by
  coin (same entered set, same PnL) before any tuning.
- A gain found on the tuning days is booked only after the engine reruns the rule on the
  earlier days the search did not touch. An extra entry condition (crowd of 20 distinct wallets
  in 5 s) gained +4.6 pp on the tuning days and lost on the earlier days: rejected.
- Deliver three rules: max profit, safest that keeps the migrated count, and the one
  recommended. Compare out-of-fold numbers only with out-of-fold numbers.

### 6. Robustness, then eval once

- Research: every parameter +/-10 % and +/-20 %; latency 50 / 83 / 115 / 200 / 300 ms; cost
  +25 %; the book without its best 3 coins; the per-day list. A rule that holds only at its
  exact parameters goes back to step 2.
- Book the frozen rule on the eval week with its per-day list. It passes when total SOL is
  positive and at least half of research's SOL per coin. A failure is written down with the
  gap; the work returns to step 2, never to threshold tweaks.

### 7. Ship

- Write the rule: `enter`, one named signal per policy rule (one AND group per stage cell),
  `always` lines that sell on each signal.
- Simulate in the lab on both weeks (`lag_115`, `pumpfun_impact`); the engine book matches the
  kernel trade by trade (audit V1, F1).
- Insert as paper, inactive; then paper.

## Files

- Scripts, data and the step-by-step notes: `hunter/_local/study-<fingerprint-slug>/`
  (untracked), one folder per fingerprint, `NOTES.md` holding every step's numbers.
- A shipped rule's derivation: `node-derivation/<fingerprint-slug>.md`.
