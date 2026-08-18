# Wallet 64hP — full study (2026-08-18)

`wallet_dict.id = 662`. Study window 2026-08-09..08-15 (7 full days) for his book, with
controls and out-of-sample work spanning 08-01..08-16. Everything below is measured from
this workstation's Postgres (`trades`, `tokens`) unless stated.

**Bottom line.** His entry behaviour is now almost fully characterised and none of it is
transferable. His profit lives in his **exit**, and after excluding five candidate
mechanisms the exit trigger is not visible in on-chain data. Every mechanical rule built
from what was derived loses money at our fills.

---

## 1. His real book

| measure | value |
| --- | --- |
| episodes | 31,162 |
| completed (sold) | 29,953 |
| **completed return, SOL-weighted** | **+5.20%** (+1,564 SOL on 30,050 deployed) |
| never sold | 1,209 episodes, 1,145.6 SOL |
| **true book incl. unsold as total loss** | **+1.34%** (+418.4 SOL on 31,196 deployed) |
| concurrent positions | median 4, p90 7, max 15 |

**Bag-holding destroys 73% of his gross profit.** The unsold episodes are spread evenly
across all seven days (145-228/day), so they are not a window artifact. Those tokens go
illiquid: 42 prints in the following 10 minutes versus 225 for positions he exits, and 7.4%
stop printing entirely versus 1.0%.

Two consequences. First, quote **+1.34%**, never +5.20% and never the +5.40% unweighted mean.
Second, any clone that does not model the stuck tail overstates itself by roughly 4x.

He earns ~418 SOL/week on ~4 SOL of working capital. The return is on turnover, not on size.

## 2. Operational profile

- **Fully automated.** Buys are flat across all 24 hours (967-1548/hr). No human rhythm.
- **Not a scanner.** Of his active seconds, 29,560 contain exactly one token, 784 contain
  two, 11 contain three. He works one token at a time.
- **Narrow, adaptive universe.** ~1,700 tokens/day out of ~25,000 launched, **2.6 buys per
  token per day**. He re-enters 71.7% of the time after a winning episode versus 50.2% after
  a losing one.
- **Young tokens.** Age at his first buy: p10 39, p50 112, p90 430 slots (median ~45 seconds).
- **Cadence.** Median 12.5s between consecutive buys; hold p10 9 / p25 21 / p50 48 / p75 123
  / p90 513 slots.
- **He is a +1 slot reactor**, not a searcher or bundler. 76.9% of his buys land exactly one
  slot after the last print; slot gap to the previous print p50 1, p90 2. This bot measures
  p50 1 slot / 55% next-slot, so **it is in the same latency regime** — see
  `hunter/docs/roadmap/latency-measurement.md`.

## 3. Entry — what it is not

Every hypothesis below was tested against a matched control and rejected.

| hypothesis | test | result |
| --- | --- | --- |
| copy-trades a wallet | wallets printing immediately before his buys, lift vs their share of prints on the same mints | top counterparty precedes 66 of 31,161 buys (0.21%); top 25 ~10% total |
| scans a list | distinct mints bought within the same second | 1 token in 29,560 of ~30,355 active seconds |
| picks creators | creators of his 11,818 tokens | 3,550 creators, 3.33 tokens each |
| picks liquidity | pool at his entry vs all prints | his p50 22.4 SOL vs 20.8 — identical |
| reacts to a trade type | side of the print immediately before him | 53.6/46.4 buy/sell vs baseline 54.4/45.6 |
| reacts to trade size | size of that print | p50 0.295 vs 0.236 SOL — marginal |
| any slot-level price/flow state | pool, dip vs rolling high, net flow, age, intra-slot recovery | his return is a flat +2-3% inside **every** bucket while the population swings -3.2% to +2.5% |

That last row is the important one. The orthogonality is not noise — it is the finding. **A
signal invisible to every absolute market feature is probably not an absolute market feature.**

## 4. Entry — what it is

**He re-buys below his own previous exit price on that token.**

61.3% of re-entries are below his last sell (median -8.9%, p25 -26.3%, p10 -43.0%), and the
payoff is monotone in depth:

| re-buy price vs his last exit | n | next episode return | win |
| --- | --- | --- | --- |
| < -20% | 6,133 | **+6.92%** | 58.3% |
| -20..-10% | 2,881 | +6.21% | 63.1% |
| -10..-3% | 1,829 | +5.09% | 62.7% |
| flat -3..+3% | 1,083 | +3.47% | 55.8% |
| +3..+20% | 2,646 | +4.86% / +4.26% | ~55% |
| > +20% | 4,010 | +4.77% | 55.6% |

Gap from his exit to his re-entry: p10 9, p50 75, p90 570 slots. Performance also improves
with visit number (1st visit +4.74% weighted, win 49.9%; 6th+ +5.84%, win 61.4%).

**This explains the orthogonality.** His reference price is one he set himself, per token.
No bucketing of market state can see it.

**It is not generic dip-buying.** Control across all tokens, forward 30s from the reachable
fill at 3.3% cost, bucketed by the prior 30s return: every bucket negative, -4.88% to -9.73%;
a 40%+ drop returns -5.77%, no better than flat. His +6.92% is a ~12pp gap the market does
not provide.

## 5. Entry — refuted as a rule

A stateful per-print simulation (5.9M prints, 67,139 tokens, 20% token sample, 08-01..08-15,
+1 slot latency on **both** legs, 3.3% round trip) implementing exactly that loop — probe a
young token, then re-buy when price falls X% below **our own** last exit — **loses in all ten
configurations**, in-sample and out:

| config | IS | OOS |
| --- | --- | --- |
| hold 5 slots, re-buy -10% | -6.61 | -6.60 |
| hold 10, re-buy -10% | -6.62 | -6.61 |
| trail 8%, re-buy -10% | -7.05 | -7.35 |
| hold 25, re-buy -10% | -7.47 | -7.59 |
| hold 25, re-buy -25% | -7.93 | -8.05 |
| hold 50, re-buy -10% | -8.61 | -8.55 |
| trail 15%, re-buy -10% | -9.03 | -9.10 |
| hold 150, re-buy -10% | -9.17 | -10.18 |
| trail 15%, re-buy -30% | -9.71 | -9.55 |
| trail 25%, re-buy -10% | -13.88 | -14.82 |

The re-buy leg is negative in every config and consistently **worse** than an indiscriminate
seed probe (-8.43% vs -6.67%).

**Therefore the +6.92% belongs to his exit, not to the re-buy condition.** Substituting any
mechanical exit removes the entire edge. The same conclusion arrives independently from the
entry side: his entries reach only **+2.69% at 10s gross**, which is under the 3.3% round
trip, so buying what he buys and holding nets about **-0.6%**.

## 6. Exit — five mechanisms excluded

| candidate | evidence against |
| --- | --- |
| fixed take-profit / stop-loss | exit-return histogram is smooth and unimodal about 0 across -40%..+40% in 2% bins, with no spike at any level |
| a clock | hold p10 9 / p50 48 / p90 513 slots |
| trailing stop | winners exit at 5.7% median retrace, losers at 19.1% — a trail fires at a **constant** retrace regardless of outcome |
| reacting to order flow | with his own legs removed from the slot, his exit rate is **highest (5.88%) when other wallets have near-zero net flow**; **49.4% of his exits occur in slots where no other wallet trades at all** |
| reacting to a counterparty | same as the entry-side counterparty test — no recurring wallet |

Retrace does have a weak association (3-6% bucket: 3.84% exit rate versus 0.19% below 1%),
but there is no threshold and it cannot be the rule given the winner/loser asymmetry.

Hold time correlates with outcome — 2-9 slots returns +7.60% at a 74.6% win rate, 750+ slots
returns -9.55% at 19.3% — but that is an **effect**, not a cause: he exits quickly when the
trade works.

**He is alone when he sells and nothing observable fires it.**

## 7. His execution quality

Calibrated on 7.09M real sells, comparing realized price
(`amount_lamports/token_amount`) to `plast` of the previous slot — the price a +1 slot
reactor sees when it decides:

| | mean | median |
| --- | --- | --- |
| all sellers | -5.03% | -1.14% |
| **wallet 662** | **-1.75%** | **-1.85%** |

His ~1 SOL sell into a ~50 SOL pool carries ~2% of own impact, so **his adverse drift is
about zero — he realizes the decision price.** Exit *execution* is therefore not the barrier;
this bot can match it. What cannot be matched is knowing *when*.

## 8. Methodology traps found while doing this

Each one produced a convincing false result before being caught. All five are now gates.

1. **Own-impact contamination in the entry anchor.** Anchoring his entry at "the first print
   after the decision slot" charges *his* 1 SOL impact (1.85%) to a 0.12 SOL hypothetical.
   Flipped his measured entry edge from -2.07% to +2.69%.
2. **Double-charged slot.** Repricing his fills one slot later assumed his signal lived in his
   own fill slot; he already reacts at S-1 and fills at S.
3. **Exit filled at the trigger price.** Slot-granularity backtests fill a trailing stop at
   the trail level in the slot it breaks. Per-print data shows the first print at or below the
   trail has usually already gapped past it. Worth ~2.5pp — it was the entire apparent profit
   of the intra-slot "turn" rule.
4. **Own trade inside the slot aggregate.** Measuring "what is happening when he sells" with
   slot flow that includes his own sell manufactured a 15.69% exit rate into "net selling of
   1-5 SOL". Excluding his legs **inverted** the result.
5. **Cost floor.** The true round trip is 3.3%, not 3.0% — see
   `hunter/docs/plans/strategies/fill-and-cost-models.md`.

Standing gate for any future candidate: **per-print resolution, +1 slot on both legs, 3.3%
cost, matched control, and an out-of-sample period.** Every phantom result in this
investigation died at one of those five.

## 9. Opinion

Four sessions of feature search excluded his entry signal, and this session excluded five
exit mechanisms. What remains is a wallet whose entries are worth less than transaction costs
and whose exits are timed by something that leaves no trace in the tape — no counterparty, no
flow, no price threshold, no clock, and usually not even another trade in the same slot.

The most likely explanations are off-chain (a private feed, a social signal, or state held in
his own process such as a model over tokens he has already probed). None of those are
recoverable from `trades`.

**Recommendation: stop cloning this wallet.** The genuinely valuable output of the exercise is
not his rule but the hardened method in section 8 and the ruled-out families in sections 3-5.
Search this bot's own signal space with those gates instead.

## 10. Data left in place

PG schema `wstudy` (~26 GB) holds the study tables: `mx`/`mf` (full-universe slot state and
first-print price, 14.3M rows, 345,869 mints, 08-01..08-16), `hep` (his reconstructed
episodes), `hold2` (per-slot state while he held, own legs excluded), `uni` (per-print
simulation universe), `st` (simulated trades), plus the refuted intra-slot work. Drop when
finished.
