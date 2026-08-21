# Signal round 2: the first positive net, and it is a bet on the tip

Date: 2026-08-19. The inverted token search closed the flow-feature space with a
permutation-certified null and left the project one percentage point short of the cost
bar. This round asked a different question: **what kind of thing is a signal here**, and
built candidates from the answer rather than from more statistics.

The diagnosis, recorded in
[signal-search-mandate.md](../plans/strategies/signal-search-mandate.md): every
dead feature was an unconditional aggregate over a trailing window, and every idea that
ever showed life was either an event-conditioned response or a state (stock) measure. Not
one of the 29 prior features measured a stock. Three families were built on that basis.

## Result

Entry = the prior best (token age under 10 s, buy flow concentrated in one order,
`is_cashback_enabled` off) **plus the new fresh-wallet filter**. Exit = the tuned armed
trail, arm 2% / trail 4% / TP 10% / 30 s hold, filling at the next print past the trigger.
Full universe, 8 days, 17,886 entries over 9,342 mints.

| | prior best alone | **+ fresh-wallet filter** |
| --- | --- | --- |
| mean net / trade | +0.22% | **+1.26%** |
| IS (08-11..15) | +0.23% | +1.20% |
| OOS (08-16..18) | +0.21% | **+1.35%** |
| day-block bootstrap | +0.22%, CI95 [-0.59, +1.09], P(>0) 66% | +1.25%, CI95 [-0.04, +2.62], P(>0) **97%** |
| one trade per token | +0.41%, CI95 [-0.70, +1.46] | **+1.71%, CI95 [+0.17, +3.55], P(>0) 99%** |
| trades / day | 4,502 | 2,236 |
| SOL / day at `sqrt(F*vsol)` sizing | 1.22 | **2.74** |

The filter halves the trade count, multiplies the per-trade edge by about six, and moves
the per-token bootstrap from straddling zero to excluding it. OOS exceeds IS. This is the
first honest positive the project has produced.

It is also **latency-positive**: +1.26% at zero delay, +1.20% at one price print late,
+1.62% at two, +2.13% at three. Waiting helps, so nothing here is a race.

## The catch: it is entirely a tip bet

> **Corrected 2026-08-19 by round 3.** The sweep below holds position size fixed while
> raising the fixed cost. Size is not fixed - re-solving `B = sqrt(F*vsol)` at each `F`
> gives **+0.18%** at a 0.001 tip, not -0.47%, and moves break-even from 0.0008 to about
> 0.0015 SOL/leg. The signal is marginal there, not dead. See
> [round 3](2026-08-19-signal-round-3-participation-breadth.md), which also supersedes this
> rule: ranking `c_fresh1h` inside the candidate pool rather than the whole universe, plus a
> participation-breadth filter, reaches +3.04%/trade at 434 trades/day.

The fixed per-leg cost is env-derived (`JITO_MIN_TIP_SOL + avg CU priority fee`), and at a
mean position of 0.094 SOL it dominates. Re-pricing the same trades:

| fixed cost / leg | tip | mean net | P(>0) per token |
| --- | --- | --- | --- |
| 0.000225 SOL | 0.0002 (current `hunter/.env`) | **+1.26%** | 99% |
| 0.000525 SOL | 0.0005 | +0.61% | 87% |
| 0.001025 SOL | 0.001 | **-0.47%** | 42% |

Break-even sits near 0.0008 SOL/leg. Prior incidents recorded that sub-0.001 tips failed
to land through Jito Sender, so **whether this strategy exists at all is a live-execution
question, not an analysis question.** Nothing in the backtest can settle it. That is the
single thing to resolve before any capital moves.

## What each candidate did

**C3 fresh-wallet share (`c_fresh1h`) - the winner.** Share of trailing-30 s buy lamports
from wallets whose first-ever trade anywhere in `trades` is under an hour old. Wallet age
cannot be faked retroactively, which is why it survives the instruction-pattern and
fee-size rotation that defeats hand-built volume-maker classifiers.

Strongly age-conditional. Top-versus-bottom quintile gap on `net60`, per age band:
**+7.81pp** under 10 s, **+4.67pp** at 10-30 s, then negative for everything older
(-0.92, -1.94, -2.59, -1.09). It is a signal about brand-new tokens only. Direction is
counter-intuitive and worth stating plainly: **more fresh-wallet money is better, not
worse.** Monotone in threshold (q>=3 +0.29%, q>=4 +0.60%, q=5 +1.55%), so not a knife
edge, and dropping the age bound from 10 s to 30 s barely moves it (+1.47% vs +1.55%).

**A1 supply-response deficit - real, consistent, and the opposite sign to the
hypothesis.** Sell flow in the last 3 slots, ranked within its own pop-size bucket and
stratum, so it measures "less selling than a pop this size normally draws". Consistent in
**all 6 age bands and 8 of 8 days**, worth 1.4-3.0pp under a fixed hold and 1.25pp under
the trail - the largest of the new features on the trail.

The sign refutes the absorption intuition. A pop that draws **no** selling is bearish, not
bullish: on this venue an unanswered pop means there is no real holder base to take profit
against, so the move is manufactured. Heavy selling into a pop is the healthy case.

The methodological point is bigger than the feature. The raw, unconditioned version
(`a_resp`) is inconsistent - band gaps -0.82, +1.39, +0.33, -0.35, +0.57, +1.08. The
conditioned version is consistent across all six. **The conditioning is what carries the
information**, which is the round's thesis surviving its own test.

**B1/B3 holder cost-basis stock - real, age-independent, but trail-fragile.** Per-wallet
running position and average cost basis accumulated over each token's whole life, rolled
into weighted moments of `ln(basis)` so the crowd's aggregate P&L (`b_dist`) and a
normalized underwater score (`b_uwz`) are O(1) per slot.

`b_uwz` correlates with log age at only **0.044** - the first feature here that is not a
clock in disguise. Monotone deciles, 8 of 8 days positive, positive in 5 of 6 age bands
(failing only under 10 s, where a holder base barely exists). `b_dist` is consistent in
all 6. Direction: **the more underwater the crowd, the better the forward return** -
underwater holders anchor and do not sell into small pops.

Worth 2.0-3.4pp under a fixed 60 s hold. Under the trail it collapses to 0.4-0.6pp, and
adding it to the winning combination made things worse (IS -1.07 / OOS +2.98, divergent).
Real, but it suits a fixed hold and the trail already harvests what it predicts.

## What died

- **C1 retention ratio, killed by algebra before it was built.** On a bonding curve
  `delta vsol` **is** net SOL flow, so `delta vsol / gross volume` reduces exactly to
  `f_net30 = (b30-s30)/(b30+s30)`, an already-tested trailing aggregate. The idea is sound
  on an AMM where wash volume leaves reserves untouched; it carries nothing here.
- **C2 lifetime round-trip share (`c_rt_life`) is a clock.** Correlation with log age
  **0.685**. Its raw decile spread of 8.6pp looked like the strongest result of the round
  and inverted under age control, with the band gaps flipping sign (-0.60, -0.23, -2.36,
  +3.70, +2.96, +0.57). The unstratified screen and the stratified AUC disagreed in sign,
  which is the tell.
- **Single-transaction wash does not exist on this venue.** Outside work on other pump.fun
  datasets puts buy-and-sell-in-one-transaction at about 21% of pre-migration activity.
  Here it is **4,755 slots across 8 days and 191,235 mints** - effectively zero. The
  literature figure does not transfer; round-trip identity across transactions is the only
  route to a wash measure.
- `sd_lnb` (holder-basis dispersion): correlation with age 0.467 and sign-flipping bands.
- `c_xfer_share`, `c_fresh24h`, `a_pop`, `a_netresp`: below the noise floor.
- **The wide-trail configuration that topped the OOS ranking is noise.** arm 5 / trail 25 /
  no TP / 120 s showed IS -2.53% against OOS +2.57%, but its daily means run +6.72, -16.10,
  +3.89 - a day-level standard deviation near 9pp puts that OOS figure inside one standard
  error of zero. Not tail-concentrated, just loud. Rank by IS and check the daily series.

## Method notes worth keeping

- The permutation null on this sample put the noise floor at **0.0007**, tighter than the
  0.005 of the prior round because n is larger. Re-derive it per sample; do not inherit it.
- A raw decile table and a stratified AUC **disagreeing in sign** is the signature of a
  confound, not of a weak signal. Both `c_rt_life` and `b_uwz` showed it; age control
  killed one and vindicated the other.
- The per-entry trap bit again. On the 10% sample the combination read +1.55% per entry but
  +0.60% per token across 1.9 entries per token. Always collapse to one trade per token.
- Fixed-hold screening and trail screening rank features differently and are not
  substitutes. `b_dist` is strong on the fixed hold and near-worthless on the trail;
  `a_deficit` is the reverse.

## Where it lands

A real signal exists, it is new, its mechanism is legible, it is latency-positive, and it
survives out of sample. Whether it is tradeable is decided by the tip, not by the model,
and at 2,236 trades/day the strategy also needs a capacity and concurrency answer that no
backtest supplies. Both are execution questions and both are open.
