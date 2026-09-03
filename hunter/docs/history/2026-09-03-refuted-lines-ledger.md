# Refuted search lines, 2026-07 to 2026-09 — the consolidated ledger

**One file replaces 44 per-study records.** Every line below was derived under the
pre-2026-09-03 methodology: statistical feature search over a space chosen without a
causal model of who pays for a price move. On 08-31 all 36 pre-registered gates from that
program ran on one honest basis and 32 of 32 entry gates were negative in both halves —
the program's own verdict on itself.

The methodology that replaced it is
[market-model-and-workflow.md](../plans/strategies/market-model-and-workflow.md). Read
this ledger for **what has already been tried**, never as a starting point: a refuted
hypothesis about a trader closes the hypothesis, never the trader.

Full text of every entry is in git history before 2026-09-03.

---

## What survives, and where it now lives

The only durable products of the whole program. Each is a law, not a result:

| Law | Now lives in |
| --- | --- |
| On a curve `price = vsol^2/k`, so windowed flow **is** the price move (corr 0.98) | [curve-flow-is-price.md](../plans/strategies/curve-flow-is-price.md) |
| Both legs pay the fill lag; a stop or trail is adversely selected at its own fill | [edge-at-real-latency.md](../plans/strategies/edge-at-real-latency.md) |
| Impact is charged on the priced reserve `vsol`, never the real one | [execution-costs.md](../plans/strategies/execution-costs.md), `CLAUDE.md` |
| A fill is the LAST print at or before the deadline, never the first | `paper_fill.rs`, [fill-and-cost-models.md](../plans/strategies/fill-and-cost-models.md) |
| Silence freezes a price: a no-print hold exits at entry, not -100% | [curve-honest-pricing.md](../plans/strategies/curve-honest-pricing.md) |
| Rank on total net SOL; mean, median and win rate are description | [market-model-and-workflow.md](../plans/strategies/market-model-and-workflow.md) T10 |
| Never build a factor on wallet identity — the durable axis is ix structure | [market-model-and-workflow.md](../plans/strategies/market-model-and-workflow.md) T5 |
| A derivation's candidate table is not a universe | [market-model-and-workflow.md](../plans/strategies/market-model-and-workflow.md) T10 |
| Position inside a slot is a tip auction, not a latency parameter | [market-model-and-workflow.md](../plans/strategies/market-model-and-workflow.md) §A |

## The one live consequence

**Every `sim_results` run stored before 2026-08-31 is priced under two pricing defects of
opposite sign** — impact on the real reserve, and fills booked off a counterparty's
execution price — and does not compare with anything after. RCA kept separately:
[2026-08-31-backtest-price-basis-and-impact-denominator.md](2026-08-31-backtest-price-basis-and-impact-denominator.md).

---

## Signal-search rounds 2-11

| Round | Verdict |
| --- | --- |
| 2 — stock and fresh wallets | First positive net in the program; it was a bet on the tip. Superseded by round 3 |
| 3 — participation breadth | Breadth was a tie-break, not a signal (`a5_uwshare` 99.2% ties); the fresh-wallet screen alone was the rule |
| 4 — the D->L->I chain | Mechanism confirmed (+7.70pp vs matched control), rule refuted on cost. A lull is a slot gap |
| 5 — operator ideas | Silence-exit reduces to "hold 3-6 s not 39 s"; concentration real but backwards; 256 combos, 0 positive |
| 6 — the ix-pattern channel | Campaign mechanism confirmed; `ix_labels` closed at every speed (0/417 cells); MFE selection unharvestable |
| 7 — fresh-wallet forward | Fails on the fresh day (placebo z 9.46 -> 0.38); venue-state gate refuted, `P(arm)` invariant (0/329 cells) |
| 8 — cost and entry depth | 125 bps immovable; money-optimal buy ~1% of pool; the 3.45% bar is correct |
| 9 — entry depth forward | Depth cut held on a second day (8/9), both halves required — never regraded after the 08-31 pricing fix |
| 10 — exhaustive combination search | 17,744 cells + OR-portfolios refuted by walk-forward: searching lost to not searching |
| 11 — island search | No multi-window region beats cost; 0/320 marginal cells; exits dominate entries by 40-50pp |

## Wallet studies (every clone attempt refuted)

| Wallet / study | Verdict |
| --- | --- |
| Research journal 07-21..07-31 | Run-by-run reverse engineering of the first four scalpers; all superseded |
| Books were gross, not net | 4 of 6 wallets do not clear 125 bps/leg; two verdicts inverted. Only 3Xk2 and 8dtx cleared |
| `FBvx` | Net positive 22/22 days; the entire edge is a +6.15% gap consumed inside one slot |
| `3Xk2` | Momentum breakout, not dip; +1 slot costs 9.87%. A derived first-launch rule read OOS 4/4, never regraded post-fix |
| `8dtx` | Clone refuted; his edge is fill position (unreachable) plus a ~4-6pp fill-invariant token selection that was never derived. **The trader is not closed — the hypotheses are** |
| `64hP` | Entry fully characterised; the edge is micro-timing, orthogonal to every feature held |
| Profitable-wallet mine | 7-day mine, negative even at zero latency; wallet-copying as a class is closed |
| Copy edge | Their +18%/leg *is* `1 + buy/pool` — their own price impact, priced before a copier can act |
| Choice-set lull | Four scalpers buy into silence; a filter, not an edge |
| Intra-slot turn | The signal buys a top; execution was not the issue |
| Dump-scalp gap | The ~6pp loss is fill dispersion, not thresholds and not latency |

## Token, crew and fingerprint searches

| Study | Verdict |
| --- | --- |
| Inverted token search | Starting from money: the whole observable universe is 1pp short of the fee |
| Winner population (599 wallets) | Their edge is entry price (93% of gross); they pick tokens worse than random |
| Real traders minus dev crew | Removal correct, the +10% was a weighting artifact; per token the OOS blind hold is -8.23% |
| Price-action / token-filter space | All 256 cells negative in and out of sample; activity *is* the extraction |
| Graduation runs + identity layer | The finish line is real but priced; identity predicts death, not success |
| Crew-share filter | Selection confirmed, profit CI spans zero |
| Creator-reputation + launch-crew screen | `p=0.0002` did not replicate; backer reputation came out inverted |
| Launch-crew follower | Registry-copy + fixed-TP strong in sample, failed out of sample |
| Token metadata (uri host) | Does not explain 8dtx; worked as an exclusion filter (+3.11pp held out) — never regraded post-fix |
| fp `5ix:Transfer 600K/160K` | 42 SOL bundle, 26% graduate against a 35% break-even — priced against us |
| fp `5ix cu_price=75210` | Entry selection empty; the one paying exit level failed placebo; cohort dying |
| FP108-VET-1 | The +5.95%/trade was a veteran-roster leak plus unpriced impact |
| Launch identity screen | One candidate (BuyV2 `mc 7.07`) reached stage 2; killed by the 08-31 pre-registered sweep |
| Profitable token groups | 10 held-out-positive launch groups found; launchers rotate, so the set means nothing without weekly re-screening |

## The island / 6ix line (superseded twice over)

| Study | Verdict |
| --- | --- |
| Island is a same-slot artifact | 95% of the money needed a fill inside a ~10 ms same-slot gap; nothing positive at +100 ms |
| Islands re-derived at real latency | Both legs pay the lag; a reactive exit is adversely selected, a clock is not; 1 of 3 islands survived, later closed |
| ix-structure cuts | +46% expectancy LODO at an unreachable fill; collapsed to +2.50 and held out -0.03 |
| Depth-band graduation thesis | Refuted at any achievable speed: 10 ms costs 8pp, the next 390 ms costs 3pp. The same run found the `LagMs` one-print look-ahead, since fixed |
| 6ix cohort closed | Negative in every age band, 130/130 deciles, 23/23 gates; oracle exit +35% but every reachable TP negative |
| 6ix rules are intra-slot impact | The whole edge was the trigger's own price impact — 80% gone in 25 ms |
| 6ix flow gates are the price path | Four multi-window gates fired on the same tokens as no gate at all |
| 6ix pass-through | Fails on pass-through, not on signal; sits on the break-even bar |
| Migration flow texture | Predictor real (4-6x `P(grad)` in every depth band) but priced to 0.2pp: raising `P(migration)` raises entry `vsol`, and `price ~ vsol^2` cancels it |
