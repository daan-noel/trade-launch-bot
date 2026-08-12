# 2026-08-12 - per-cohort rule-search evidence: g8, g3, g0

Three worked searches, kept for the mechanisms they demonstrate rather than the rules they
produced. The method those mechanisms feed is
[../plans/strategies/rule-search-method.md](../plans/strategies/rule-search-method.md).

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
  acceptance is scored on data the selection never saw, and why the seed rule is never the axis set.
- **An entry band can cost more than it screens for.** `liquidity > 25` and `time > 5` both
  carry negative marginals; removing them takes 154 trades to 201 *and* raises PnL.
- **No take-profit and no stop-loss.** Both axes pick their unreachable value over 50 and 25.
- **A time gate is not a fix for a wide fill spread.** The three-exit config moves 10.8x between
  fill models. Delaying entry tightens nothing and destroys the edge: PF 1.49 with no gate, 1.27
  at `time > 5`, 1.03 at `time > 15`, 0.90 at `time > 30`. On this cohort, under a total-SOL
  objective, entering at creation is the edge and the spread is a property to accept or reject
  rather than a defect to engineer away. The verdict is g8's own: g12 and g13 both gate `time`
  and score well, so screen it per fingerprint instead of carrying this forward.
- **A spike resolves into a plateau.** `retrace` 40 beats 30 and 50 badly on the coarse grid,
  which reads as overfit; refined against 36/38/42/45 it is a smooth plateau, and 40 holds.
- **This search covers only the seed rule's own metrics, so it prunes rather than discovers.**
  Its winner is a strict subset of the seed. That is the signature the "result that adds
  nothing" trap names, and the reason the candidate pool is built from the whole catalog.

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
  it beats `off`. A drop probe over already-selected conditions is what surfaces this.
- **`m_position.held` is load-bearing**: removing it collapses PnL to +0.0021. `stall`, exit
  `liquidity` and `m_flow_split_window.nonvol_buy` never bind and park on evidence.
- **`nonvol_net` has a cliff, not a smooth optimum.** 0.5 and 0.55 hold out of sample (PF
  1.74/1.84) while 0.3, 0.4 and 0.45 go **negative** out of sample despite scoring higher
  in-sample. This is the case a plateau check alone does not catch and the split does.
- Every candidate including the seed rule decays train to validate, so the cohort's edge
  concentrates before the split: a cohort verdict, not a rejection of the shortlist.
