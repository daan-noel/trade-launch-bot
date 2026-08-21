# Price-action and token-filter signal space refuted (2026-08-18)

Full universe, 08-01..08-16: 345,869 mints, 14.3M slot-states (`wstudy.mx`), 7.48M 2-second
bars, 1.18M token-minutes, 6.5M causal decision points with honest +1 slot fills.

**Bottom line.** Oscillation is a real, strongly persistent, predictable per-token property,
and so is survival. Neither is monetizable long-only. Every causal entry and every token
filter tested is negative in-sample and out, and every filter axis is monotone in the *wrong*
direction: the more active, liquid, or fast-growing the token, the worse it is to buy. The
best cells in the entire search are the deadest tokens, returning the bare cost of trading.

---

## 1. What is predictable

Windows are 60s; bars are 5 slots (~2s), coarse enough to suppress tick noise and fine enough
to trade. `path` = sum of |bar-to-bar % move| over the window = gross amplitude on the table.

| property | measure | persistence w -> w+1 |
| --- | --- | --- |
| amplitude | `corr(ln path)` | **0.632** |
| cycling | `corr(reversal count)` | **0.498** |
| choppiness | `corr(efficiency ratio)` | 0.110 |

Both transitions are monotone across every bucket: amplitude decile 1 -> 32.3 next-minute
path, decile 10 -> 143.6; reversal count 0 -> 0.60 next minute, 8 -> 3.57.

**Amplitude and cycling are forecastable. Whether the move round-trips or trends is not.**

## 2. The amplitude is large and genuinely available

- Median token-minute `path` = **75%** at 2s resolution; p90 = 216%.
- Median bar-to-bar move = **4.15%**, above the 3.3% round trip.
- The largest single step is only 24% of the path, so this is many tradeable moves, not one
  untradeable jump.
- Perfect-exit headroom within 30s, net of cost: **+5.9%** (low amplitude) rising monotonically
  to **+15.7%** (high amplitude); median +8.4% in the top quintile.

## 3. No causal entry captures any of it

Signal at slot S, fill at `mf.pfirst` of the next printing slot, 3.3% round trip.

| test | cells | result |
| --- | --- | --- |
| dip depth x predicted-amplitude quintile, 10s and 30s | 100 | every cell negative, -2.8% to -8.0% |
| dip depth x confirmed-cycler (prev-minute reversals) | 56 | every cell negative |
| take-profit 3/5/8/10/15/20/30 + 30s time stop | 35 | worse than a flat hold at **every** level |
| hold horizon 1m/2m/5m/10m/30m x token age | 55 | negative everywhere |

- **More cycling is worse.** Reversal count 0 returns -3.97% at 30s; 6+ returns -6.56%, while
  headroom rises from +5.9% to +15.7%.
- **A take-profit inverts the payoff.** Top amplitude quintile: flat 30s hold -6.38%, TP+3
  -8.65%. 46.9% of decision points touch +10% within 30s and it still loses, so the right tail
  carries everything and truncating it is fatal.

## 4. The fill-model excuse is closed

Across 6.5M decision points, the gap between the signal price and the first reachable print of
the next slot is **+0.07% to -0.16%**, monotone in dip depth. Entry latency is not what makes
these rules lose. Do not reach for the fill model again.

## 5. A silent token does not lose its price

The bonding curve is passive: it always quotes, and it cannot move without a trade. Measured on
the first print after a silence versus the last print before it:

| silence | n | mean change | median change |
| --- | --- | --- | --- |
| 0-3 min | 241,930 | -1.59% | -0.64% |
| 3-7 min | 42,813 | -1.13% | -0.46% |
| 7-10 min | 2,893 | -1.05% | -0.28% |
| 17-20 min | 2,074 | -0.29% | -0.28% |

**"Illiquid" means nobody else is trading, not that the position is trapped.** Booking a
non-printing token as a total loss is wrong; the correct exit is the last curve price less own
impact. Every figure in this document uses that accounting. This also means the stuck-tail
correction applied to wallet 64hP's book in
`hunter/docs/history/2026-08-18-wallet-64hp-full-study.md` is too harsh, and death prediction
is not the large prize it appeared to be.

## 6. Survival is highly predictable, and filtering for it makes returns worse

Base rates: **12.6%** of tokens still print actively at 1 minute, **5.0%** at 5 minutes,
**2.9%** at 10 minutes. From first-minute features only, 5-minute survival separates hard:
print density 0.32% -> **31.7%**, pool size 0.03% -> **31.1%**, first-minute net move 1.85% ->
**32.7%**, amplitude 0.31% -> **24.8%**.

Applying the obvious quality filter (dense printing, pool growth, balanced buy/sell) at age 60s:

| group | share of universe | 5-min survival | return, 1m hold | return, 5m hold |
| --- | --- | --- | --- | --- |
| unfiltered rest | 84.9% | 1.4% | -3.83% | -4.74% |
| dense + pool + balanced | 6.7% | **42.1%** | **-10.65%** | **-16.54%** |

**A 30x survival lift comes with a return nearly three times worse.** The tokens that survive
are the ones that just pumped, and buying after the pump is the losing side: inside the
filtered group, a first-minute net move of +231% gives 49.8% survival and a **median -56%**
five minutes later.

## 7. Every filter axis is monotone in the wrong direction

Eight first-minute features x eight deciles x two horizons x in-sample (08-09..08-16) and
out-of-sample (08-01..08-08) = 256 cells. **All 256 are negative, and in-sample and
out-of-sample agree in every cell.**

| feature | best decile | worst decile |
| --- | --- | --- |
| print density | 1.0 bars: -3.57% | 24.9 bars: -7.60% |
| pool size | 30.0 (no growth): -3.25% | 57.9: -7.64% |
| launch burst (prints in first 12s) | 1.0: -3.49% | 88.0: -5.99% |
| first-minute net move | -50%: -3.24% | +56%: -7.23% |

The best cells are the tokens where **nothing happened** - they return about -3.3%, which is
exactly the cost of trading against a gross forward return of approximately zero. Every unit of
activity, liquidity, launch size or price appreciation makes the forward return worse.

## 8. Why

On a bonding curve, price is a deterministic function of reserves, so price *is* cumulative
signed order flow. There is no market maker and no valuation anchor.

- Mean reversion is not a price property; it requires a population that reliably buys dips.
- High amplitude therefore means a large flow imbalance, which here is predominantly the dump:
  path deciles 7-9 have median net displacement **-17% to -24%**.
- **Activity is the extraction.** Prints, pool growth and price appreciation are the mechanism
  by which earlier participants are paid, so conditioning on them is adverse selection.
- Inactive tokens are honestly zero-sum, and the 3.3% cost is the whole loss.

The pincer: quiet tokens do not move enough to clear cost, and moving tokens move against a
late entrant. There is no long-only region between the two.

## 9. Standing conclusion

Market-data feature space is exhausted for a late entrant, both as an entry signal and as a
token filter. What is not refuted:

- **Creation-time participant identity.** The one replicated positive result in this codebase
  is the maxbuy bundle band (15-30 SOL creation bundle, blind 60s hold, +18.3%/trade, +20.5%
  on n=408 with CI [8.1, 33.3], positive 4 of 4 weeks). It is an entry *at* launch selected by
  *who* is launching, which is the only position this study finds that is not structurally
  adverse. The crude proxy tested here (peak virtual reserves in the first 12s) is not that
  signature and does not bear on it.
- **Cost reduction.** With gross forward return near zero on quiet tokens, the 3.3% round trip
  is the entire loss in the best cells.

## 10. Entering at launch instead of late does not rescue it

292,802 launches, entry at the first print after the creation slot, blind 60s hold, same gates.
Bucketed by creation bundle (`vsol` at the creation slot less the 30 virtual):

| bundle | n | return | median | win | IS | OOS |
| --- | --- | --- | --- | --- | --- | --- |
| <5 | 156,990 | -3.24% | -3.30% | 8.8% | -3.30 | -3.18 |
| 5-10 | 81,011 | -15.11% | -31.81% | 13.2% | -15.18 | -15.04 |
| 10-15 | 36,544 | -22.26% | -46.81% | 14.1% | -22.22 | -22.31 |
| 15-20 | 11,517 | -16.13% | -56.95% | 23.3% | -15.23 | -16.90 |
| 20-30 | 3,898 | -5.63% | -10.57% | 40.0% | -6.31 | -4.98 |
| **>30** | **2,842** | **+2.86%** | -1.08% | 48.0% | **+1.14** | **+4.43** |

The single positive cell in the whole study **fails the standing gates**: median -1.08%, only
10 of 16 days positive (08-16 = -16.4%), **77.9% of P&L from the top 20 of 2,841 trades**, and
the finer sweep across 35/45/55/65/75/85 SOL has no stable ridge with IS and OOS disagreeing in
4 of 6 buckets. It is a class-B tail artifact, not a rule.

The sign does **not** flip with entry age either: for the 15-30 bundle cohort, entering at
launch returns -13.48% (median -55.21%) against -10.22% (median -5.85%) entering at 60s.
Entering early into a mid-size bundle is worse, not better.

**This reconciles with the shipped `5SOLIN liq>12 held60`.** That rule's liquidity gate is not
evidence that liquidity is good - globally it is monotonically bad (section 7). The gate works
because the rule is scoped to the `5ix:BuyExactSolIn` fingerprint first. **The ix-shape
fingerprint is not one signal among many; it is the precondition that makes any market variable
usable.** Without an identity scope, no market variable works anywhere in 345,869 tokens.

## 11. Data

`wstudy` additions: `bar` (7.48M 2s bars), `osc` (1.18M token-minutes with path/net/amplitude),
`cyc` (reversal counts), `bx`/`sx` (per-bar and per-slot trailing and forward paths), `sq`
(6.5M honest-fill decision points), `hz`/`hz2` (minute-horizon forward returns), `life`/`life2`
(last live and last actively-printing window per token), `lch` (launch aggregates), `tq0`
(per-token first-minute features and labels), view `v_ret`.
