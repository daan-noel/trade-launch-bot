# 2026-08-22 — instruction-structure cuts: what they are worth

A trade's ordered `ix_labels` name the **tool** behind it (Axiom, GMGN, Terminal, a
bundler, a raw pump.fun call). Blacklisting the structures that lose money is a real
discriminator. It is recorded here rather than in a plan because the rule it improves is
refuted on execution — see
[`2026-08-22-island-is-a-same-slot-artifact.md`](2026-08-22-island-is-a-same-slot-artifact.md).
The cuts themselves are reusable and should not be re-derived from scratch.

## Two corrections to the earlier read

**The sets were fitted on the days they were graded on.** The original selection kept a
structure only if it lost money on the in-sample *and* the out-of-sample days:

```python
neg = [k for k, v in rows if v[0][1] < 0 and v[1][1] < 0]
```

so the reported out-of-sample figure (+65.91 SOL, "+57% expectancy") was contaminated by
construction. Every number below fits on days that never see the day being scored.

**`ix_launch` was two actors in one column.** Defined as "the ix structure of the biggest
trade in the token's first slot", it names the dev's own create+buy on some tokens and the
first outside sniper on others, whichever spent more. The launcher's tooling is directly
readable as `tokens.fp_ix_labels` — the creation transaction's own ordered instruction
list, populated on 746,372 of 746,372 tokens — so the proxy is unnecessary. Measured
head-to-head, `ix_launch` contributes **+0.00** once the other axes are present.

## The four axes

| axis | question | actor | known |
| --- | --- | --- | --- |
| `ix_create` | which tool minted this token? | launcher | at creation, static |
| `ix_first_buy` | which tool took the first outside position? | first sniper | early, static |
| `ix_dp` | which tool made the print we react to? | our trigger's counterparty | at the fold step |
| `ix_top_buy` | which tool made the biggest buy in the 0.4 s window? | impulse driver | trailing window |

## Worth, leave-one-day-out

Blacklist fitted on six days, graded on the seventh, seven times. Base rule is
`net_flow(0.4) >= 0.5 AND rise(3) <= 9 AND liquidity <= 21.56 AND bundle < 24.9`, exit
`stop 3 / trail 5`, full re-entry, next-print fill.

| combination | SOL | expectancy | days + |
| --- | ---: | ---: | ---: |
| no cut | +114.33 | 0.000857 | 7/7 |
| `top_buy` | +133.44 | 0.001129 | 7/7 |
| `create + top_buy` | +136.97 | 0.001216 | 7/7 |
| **`create + dp + top_buy`** | **+138.22** | **0.001254** | 7/7 |
| all four | +137.61 | 0.001285 | 7/7 |

Marginal worth, each removed from the winning trio: `ix_top_buy` **+7.58**, `ix_create`
+3.22, `ix_dp` +1.25, `ix_first_buy` **−0.61** (it costs money on top of the others).

## Three properties that make them trustworthy

* **Exit-robust.** The `top_buy` cut fitted under `stop 3 / trail 5` pays +7.27 / +7.90 /
  +6.74 out-of-sample under `stop 3 / trail 5`, `stop 8 / trail 8` and `no stop / trail 20`.
  It is fitted to the counterparty, not to the exit.
* **Stable membership.** Across the seven folds `ix_top_buy` draws on 15 distinct ids: 11
  appear in all seven folds, 14 in at least five, none in only one.
* **Blacklist, never whitelist.** Every "keep only the structures that pay" variant loses
  out-of-sample — `ix_create` top-5 by expectancy is **−38.59**. Structure ranks rotate;
  only the losers stay losers.

## Share beats argmax — and is the cheaper thing to build

"Which tool made the biggest buy in the window" needs a sliding-window maximum and can flip
on one lamport when two buys are near-tied. The alternative reading is the **share** of
window buy SOL from blacklisted structures, which needs only running sums.

| reading | SOL | vs base |
| --- | ---: | ---: |
| argmax: biggest buy is blacklisted | +133.44 | +19.11 |
| **share: zero blacklisted buy SOL in the window** | **+136.19** | **+21.86** |
| share ≤ 25% | +135.11 | +20.78 |
| share ≤ 50% | +131.85 | +17.52 |
| share ≤ 99.9% | +121.63 | +7.30 |

The share is both better and monotone — no cliff — so any metric built for this reads a
filtered running sum, never a windowed maximum. `share <= 0` is simply "no blacklisted buy
in the window at all", which is a sum compared against zero.

## If this is picked up again

The cuts need a metric group only because the engine cannot express "the token's creation
`ix_labels` are **not** one of these". The fingerprint matches `ix_labels` by exact ordered
**inclusion** of a single sequence, so exclusion over a set has no spelling today. The
cheapest shape found: `ix_create` is a token-static fact available on `TokenCreated`
(`TokenFingerprint::ix_labels`), so it costs nothing per trade; `ix_dp` reads the current
`TradeLite::ix_hash`; the window term is a filtered running sum in the shape
`m_flow_window` already uses. The pattern set belongs in the fingerprint's `metric_config`
under its own key, compiled once at `RulesReloaded` via `flow_split::ix_hash` — reusing the
hash, not the classifier, and never `volume_ix_patterns`.

Any such metric must also answer a `needs_ix_identity` predicate that both lab loaders
consult, or the lake omits `ix_labels`, every `ix_hash` is `None`, and the gate silently
never fires.
