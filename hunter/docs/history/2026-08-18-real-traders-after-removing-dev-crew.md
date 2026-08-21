# Real traders, found by removing the dev crew (2026-08-18)

Full universe 08-01..08-16, 345,869 mints, 684,208 wallets. Honest fills (`mf.pfirst` of the
next printing slot after the signal), 3.3% round trip, blind time exit, walk-forward roster
built strictly from 08-01..08-08 and tested on 08-09..08-16.

**Bottom line.** The previous session's "winner population" was mostly launch crew, and that
contamination inverted its central result. After stripping bundlers, creator-linked wallets
and any wallet that sells tokens it never bought, a small population of genuinely independent
traders remains, their edge replicates out of sample, it is **latency-flat**, and a blind
60-second hold on the tokens they buy is **positive**. Token selection is the lever after all;
it was invisible while the extractors were in the sample.

---

## 0. CORRECTION (same day, later): the headline was an entry-weighting artifact

Every return below that averages **per entry** is inflated. Roster wallets re-buy into a
token they already hold, so a token that rises inside the 60-second window contributes one
row per re-buy, and winners are counted many times. The 820 OOS "entries" are 254 tokens
bought by 22 wallets — 3.2 entries per token.

Re-measured one entry per token, at the first roster buy, which is the only thing a copier
can actually trade:

| basis | n | return |
| --- | --- | --- |
| per entry (as first reported) | 820 | +10.03% |
| per token, first roster buy | 254 | **-8.23%** |
| per token, neutral entry at age 30 | 253 | **-0.80%** |
| market control, same days, age 30 | 34,110 | -7.43% |

The same correction applies to the in-sample selection test: strict12 reads +11.40% per entry
and **+4.93% per token**; clean51 reads +9.94% per entry and **-1.81% per token**.

**What survives.** Token selection is real but small. Activity-matched at a neutral age-30
entry, roster-picked tokens beat the market in every crowd bucket:

| buyers by age 30 | roster tokens | market |
| --- | --- | --- |
| 1-10 | +5.68% (28) | -4.86% (22,038) |
| 11-25 | -7.47% (42) | -10.98% (6,692) |
| 26-60 | +1.72% (117) | -14.38% (3,968) |
| 61+ | -3.79% (66) | -11.29% (1,412) |

That is a ~9pp relative edge on a ~3.3% cost base, landing at roughly zero net. Win rate
36.4% vs 12.0% is the largest honest difference in the study.

**What does not survive.** "Their picks return +10% on a blind hold", "latency-flat +8.62% at
+4 slots", and the day-count stability all rest on the per-entry average and are withdrawn.

**Selector correction.** Ranking the 11,487 walk-forward independent wallets by in-sample
return on spend is monotone against OOS pick quality (IS < -30% gives -7.81%, IS >= +25%
gives +15.11% per entry). Ranking them by *days positive* is not: 8/8 gives -10.20%. Return
on spend replaces the every-day criterion. Per token the best cut still only reaches +0.53%.

---

## 1. The prior population was crew

`2026-08-18-winner-population-inverted-search.md` selected wallets profitable every day and
removed only those matching `tokens.creator_wallet`. That test catches the wallet that signs
the create instruction and nothing else. Measured against a baseline of all wallets with 30+
tokens:

| group | entries in the creation slot | tokens from creators traded 3+ times | entries in first 5 buyers |
| --- | --- | --- | --- |
| baseline (48,541 wallets) | 6.7% | 23.1% | 7.6% |
| "winners" (803) | **48.8%** | **65.3%** | **44.0%** |
| "pure traders" (526) | 27.7% | 51.5% | 25.1% |

Nearly half of the winners' entries land **inside the atomic launch transaction**. The
"pure trader" set was still 4x baseline on the same measure. The population was the dev crew,
which is why its selection measured worse than random: those tokens go down *because that
population is the seller*.

## 2. Gates that separate a reachable trader from crew

Four properties, all computable from `trades` + `tokens`, none defeated by wallet rotation.

| gate | crew signature | cut used |
| --- | --- | --- |
| creation-slot share | bundle is atomic and unreachable | `<= 2%` |
| slot-1 share | catches bundling services that miss by a slot | `<= 4%` |
| top-creator share | crew serves one operator | `<= 8%` |
| repeat-creator share | crew follows a creator across launches | `<= 25%` |
| token conservation | crew receives inventory by transfer | sells <= buys on every observed position |

The last one is the sharpest: any wallet selling tokens it never bought is crew regardless of
how it looks otherwise. It is not a window-boundary artifact - checked against the full feed,
1,640 positions and 28.4% of the receipts of the surviving set came from tokens with no
matching buy.

**Being early is not the same as being crew.** A bundler sits in the creation transaction; a
fast public sniper reacts to it and is in the same race we are. The creation slot is the line.

## 3. The surviving population

25,469 wallets pass the independence gates; 7,266 are active enough to judge (6+ days, 50+
legs). Applying the every-day-profitable criterion to those:

| roster | n | OOS cash | OOS median | still positive | still every-day |
| --- | --- | --- | --- | --- | --- |
| independent active base | 6,840 | -52,017 SOL | -2.22 | 13.2% | 1.3% |
| every-day winners | 73 | +3,441 SOL | +20.19 | **95.9%** | 47.9% |
| + no free inventory | 51 | +1,468 SOL | +24.50 | 98.0% | 56.9% |
| walk-forward roster | 23 | +755 SOL | +30.42 | 95.7% | - |

Return on money actually spent, no marks: **+14.4% in-sample, +19.0% out-of-sample.** The
out-of-sample figure is the higher one under every filter strength.

## 4. Their behaviour

Entry age p25 42 / p50 140 slots (~56s) against a base of p50 264. Median buyer rank 67
(base 139). Median buy 1.068 SOL against 0.309. Creation-slot share 0.16%. Repeat-creator
share 6.8% against a 23.1% baseline. Roughly 370 legs across 101 tokens per 8 days.

They are mid-size traders entering about a minute after launch, with no relationship to any
creator, who buy every token they sell.

## 5. Blind hold on their picks - the selection result

Signal is their buy; fill is the first print of a later slot; exit is a blind 60-second clock.
No exit skill, no oracle.

| group | n | 60s return |
| --- | --- | --- |
| market control | 205,739 | **-8.70%** |
| clean 51 | 9,290 | -0.95% |
| strict 12 | 1,346 | **+2.95%** |

Age-matched, the edge concentrates in a narrow band of **11-50 slots (4-20 seconds old)**:

| entry age | strict set | clean set | market control |
| --- | --- | --- | --- |
| <= 10 slots | -3.66% | +3.59% | -12.00% |
| 11-50 | **+11.40%** | **+9.94%** | **-11.57%** |
| 51-150 | -3.47% | -5.93% | -9.71% |
| 151-600 | -5.06% | -6.26% | -5.95% |
| 600+ | -1.82% | -2.11% | -2.80% |

The band itself is not the edge - the market loses 11.57% in it. Their picks inside it gain.

## 6. Walk-forward, with no roster leak

The section 5 features were computed over the whole window, so the roster peeked. Rebuilt from
08-01..08-08 tokens only (23 wallets) and applied to 08-09..08-16 entries:

| test | n | return | median | win |
| --- | --- | --- | --- | --- |
| OOS copy, all ages | 1,485 | **+4.56%** | -22.15 | 39.1% |
| OOS copy, age 11-50 | 820 | **+10.03%** | -59.69 | 38.7% |

Positive on **6 of 8 days** (+13.2, +25.0, +32.0, -27.1, +14.9, -13.8, +3.1, +3.8).

## 7. Latency-flat - the property that makes it reachable

| fill delay after their buy | return |
| --- | --- |
| +1 slot | +10.03% |
| +2 slots | +10.53% |
| +4 slots | +8.62% |

Total price slippage across four slots is **1.13%**. The bundler population lost 9.87% to a
single slot. **This edge does not live in the fill**, which is what separates it from every
copy target refuted so far.

## 8. Payoff shape

Fat right tail: the top 20 of 820 trades carry **54.1%** of total P&L; median is -59.69 on a
+10.03 mean, win rate 38.7%.

| exit variant | return |
| --- | --- |
| blind 60s hold | +10.03% |
| take-profit +30 | **-27.00%** |
| take-profit +50 | -19.66% |
| take-profit +100 | -3.13% |
| stop-loss -40 | **+22.61%** |

**Never cap the upside; cap the downside.** The stop figure is a threshold-price fill and is
optimistic - it needs per-print resolution with honest gap fills before it can be quoted.

## 9. Token or wallet?

Other wallets buying the same tokens in the same age band, excluding the roster:

| group | n | return |
| --- | --- | --- |
| market at age 11-50 | 47,049 | -11.57% |
| other wallets, roster tokens | 12,613 | **+2.21%** |
| roster wallets | 820 | **+10.03%** |

The token carries **+13.8pp** of the lift and their precise moment adds **+7.8pp**. Both are
real, and the token half is net positive on its own - which makes this a **token filter**, not
only a copy signal.

## 10. Why the earlier refutations missed it

`2026-08-18-price-action-space-refuted.md` found all 256 token-filter cells negative and
monotone the wrong way, and read that as efficient pricing. The two diagnoses separate on the
data already collected: efficiency predicts gross return near zero everywhere (net = -cost),
while manufactured volume predicts gross going strictly negative as the faked signal
strengthens. Measured: deadest tokens -3.3% (gross ~0), most active tokens -7.6%
(gross ~-4.3%). The activity axes were poisoned, not priced - operators manufacture the
volume that those filters key on, so the filters were reading the bait.

Selecting on who is buying, net of the crew, is not poisoned, because an operator can fake
volume and rotate wallets but cannot fake a genuinely unaffiliated buyer.

## 11. Open

- The -40% stop needs an honest per-print backtest with gap fills before use.
- Roster refresh cadence is unmeasured; these wallets may rotate or decay.
- What the roster is detecting is underived - replacing the copy with a computable feature
  would remove the dependence on 23 specific wallets.

## 12. Data

`wstudy` additions: `mc` (mint to creator id), `twr` (10.5M buy entries with buyer rank),
`wcr` (6.96M wallet-creator pairs), `wf`/`wf2` (crew features), `indep` (25,469 independent
wallets), `cw`/`cw2`/`cw3`/`cw4` (every-day winners at four filter strengths), `tbal`/
`tbal_all` (token conservation), `wd2`/`wsum2` (OOS cash flow), `wfA`/`tbA`/`rosA`
(walk-forward roster), `selt`/`selt2` (selection test), `oosc` (walk-forward copy), `lat`
(latency), `othr` (token-vs-wallet split).
