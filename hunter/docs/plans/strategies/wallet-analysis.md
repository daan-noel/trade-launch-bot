# Wallet analysis - what seven studied scalper wallets actually do

The index and comparison SSOT for every wallet reverse-engineered from the local pump.fun
curve firehose (PG `trades`, wallet-attributed). It holds the **surviving conclusions**: each
wallet's logic in one sentence, its net book, the mechanics a rule is calibrated from, and
the searches that are closed.

Per-wallet detail lives one tier down: [wallet-8dtx-logic.md](wallet-8dtx-logic.md) is the
only full mechanism spec; the rest are in `docs/history/` and linked from their sections. The
run-by-run investigation for the first four is
[`@history/wallet-research-2026-07.md`](../../history/wallet-research-2026-07.md).

Companions: [execution-costs.md](execution-costs.md) (what a round trip costs - read before
trusting any PnL number here), [flow-scalper-findings.md](flow-scalper-findings.md) (the
engine traps this work surfaced), [armed-trailing-stop.md](armed-trailing-stop.md) (the
`arm_above_pct` feature it produced).

## The one number that decides everything

**The pump.fun fee is 125 bps/leg = 2.53% per round trip.** Derived from our own data: first
buy amounts cluster on `0.98765432 x round SOL`, and `0.98765432 = 10000/10125` exactly,
matching the IDL's `net_sol = spendable x 10_000 / (10_000 + total_fee_bps)`.
`amount_lamports` is the **net curve-side** SOL and excludes this fee, so a raw `sell - buy`
sum overstates PnL on both legs by ~2.5pp.

**Every wallet book here is quoted net, with the gross figure beside it, and no wallet is
promoted to a template before its net book clears zero with a day-block bootstrap CI that
excludes zero.** That gate exists because four of six early verdicts were published gross and
inverted when the fee was charged
([derivation](../../history/2026-08-18-wallet-books-were-gross-not-net.md)). A gross edge
under ~2.5%/round-trip is not an edge.

Tips are charged nowhere, so every figure below is an upper bound.

## Each wallet's logic in one sentence

| wallet | the logic |
| --- | --- |
| `63ot` | Buy a deep dip (-21% off the 30 s high) on a **hot, deep, mature** curve while sellers are still hitting it, then exit on a **fixed bracket** - TP +17% / SL -28% - never a trail. |
| `FBvx` | Buy the moment a token that has already proven a distributed crowd (10-17 active slots, 13-31 buyers, 15-30 SOL in) **absorbs** the selling that has bled it back to flat, joining the first no-seller buy cluster in that same slot, out in 1.5 s. |
| `8dtx` | Wait for the tape to go **quiet** (10 s churn 7 SOL against 37 at setups it skips) on a token 5-60% off its lifetime peak, buy the first small buy run into that vacuum, exit on an armed trail ~18% off the in-hold peak. |
| `3Xk2` | Buy **strength**: at or above the 30 s high, +36% off the 30 s low, into positive 2 s flow, one shot per mint, exit on a single wide ~25% trail. |
| `64hP` | The `8dtx` dip-reversion shape on a younger, shallower curve, but **re-entering the same mint repeatedly** (61% re-entry, improving monotonically with episode index) on a trailing stop with `peak` seeded at entry. |
| `omego` | Dip-reversion scalping that sells ~81% into the bounce and **lets ~19% ride**; the scalp round trip is a fee-paying wash and the un-closed runner tranche is the whole book. |
| `trunoest` | Does not scalp someone else's flow - **manufactures it**: one oversized ignition buy into a violently-moving young token, micro-buys painting the tape to hold screener attention, then a full-balance dump on the confirmed reversal. |
| `6LWSrd` | Buys **graduation candidates** at 0.1 SOL a shot and pays for the losers with the winners: 10.5% of its picks migrate against a 2.1% baseline, and the 202 migrators carry the entire book. |

## Net books

18 ingest-clean days 07-24..08-15 for six of them, bags charged as total loss, tips not
charged. `FBvx` is 22 days to 08-20 and `6LWSrd` 07-22..08-05, both on their own builders.

| wallet | gross | **net** | net median ep | win | net SOL | day-block bootstrap 95% CI |
| --- | --- | --- | --- | --- | --- | --- |
| `FBvx` | +3.68% | **+1.175%** | -2.47% | 39.4% | +398 | **22/22 days positive** |
| `3Xk2` | +5.22% | **+2.62%** | -6.31% | 40.5% | +110 | [1.28, 3.99] - 100% draws + |
| `8dtx` | +4.31% | **+1.73%** | -4.87% | 27.2% | +93 | [1.03, 2.47] - 100% draws + |
| `6LWSrd` | - | **+2.7 .. +8%** | 0.72x | 41.7% | +5.2 .. +15.5 | not computed |
| `63ot` | +2.45% | **-0.08%** | **+12.38%** | 66.2% | -1 | [-1.20, 0.93] - 43% draws + |
| `64hP` | -0.03% | **-2.50%** | -1.77% | 45.6% | -1,652 | [-3.08, -1.96] - 0% draws + |
| `omego` | -2.83% | **-5.23%** | -1.39% | 45.2% | -1,439 | [-6.00, -4.48] - 0% draws + |
| `trunoest` | -6.46% | **-8.77%** | -2.98% | 41.7% | -250 | [-12.43, -4.39] - 0% draws + |

`63ot`'s sign is the one that is assumption-sensitive: the 125 bps is measured on the buy side
and taken from the IDL on the sell side. Even at a buy-only fee every other negative wallet
stays negative.

## Payoff shape decides how much capital and how many trades a copy needs

The median column above is the discriminator, not the mean.

| | convex (tail-carried) | body-driven |
| --- | --- | --- |
| wallets | `3Xk2`, `8dtx`, `FBvx`, `6LWSrd` | `63ot` only |
| median episode | negative, -2.5% to -28% | **+12.4%** |
| win rate | 27-41% | **66%** |
| book with top 1% of episodes removed | `3Xk2` +0.01%, `8dtx` -1.07% | unchanged |
| episodes needed to zero the book | 59 (1.01%) / 29 (0.43%) | - |
| a take profit | **destroys it** - the trail is the mechanism | is the mechanism |
| cost and latency sensitivity | maximal: the body is ~zero by construction, so any added cost comes straight out of the 99% and the 1% cannot carry it | low |
| trades before the edge is visible | thousands | dozens |

The tail is *reliably produced* rather than lucky - `3Xk2` and `8dtx` are both 4 of 4 weeks
positive and their top episodes are clean single round trips returning +247..+586%. But a
convex wallet copied at low trade count is a lottery, not a strategy: **the archetype choice is
a capital-and-throughput decision before it is an edge decision.**

## Per-wallet mechanics

The behavioural measurements below carry no fee term and are unaffected by the gross/net
correction: dip depth, entry age, vsol bands, hold times, exit-retrace-by-MFE shape, re-entry
monotonicity, sizing as a fraction of vsol, the latency table.

| | `omego` | `64hP` | `63ot` | `trunoest` |
| --- | --- | --- | --- | --- |
| family | dip-reversion | dip-reversion | dip-reversion | momentum ignition |
| closed eps (window) | 2,974 (5 d) | 6,515 (4.2 d) | 1,088 (5.8 d) | 225 mints (8.7 d) |
| median episode | ~0 | +2.39% | **+11.0%** | +4.6% |
| sizing | 1.18% of vsol | 1.86% of vsol, cap 1.5 SOL | **fixed ~0.5 SOL** | 4% of vsol, tiered |
| entry age (med) | 5.3 min | 0.8 min | 1.9 min | 69 s |
| vsol at entry (med) | 73.5 | 44.5 | 69.8 | 60 |
| dip vs 30 s high | -12.6% | -22.7% | -20.8% | -19.1% |
| exit | ~3% trail | -6.8% trail | **TP +17% / SL -28%** | ~30% off-peak reversal |
| hold (med) | 22.5 s | 21.3 s | **10.6 s** | 30 s (win) |
| unclosed bags | - | 3.3% / -225 SOL | **0.9% / -3.1 SOL** | 8% / -60 SOL |
| concurrency | med 3, max 8 | med 2, max 10 | **usually 1, max 3** | **1** |
| selectivity | 0.66% of mints | 3.8% | 0.57% | **0.25%** |

### `63ot` - the best SHAPE, and the bracket that does not clear the fee

The most interesting of the family because its payoff is the opposite of every other wallet
here and the one profile that survives our latency. The arithmetic is simply short: a
TP +17 / SL -28 bracket at 66% win yields ~+1.7% gross against a 2.47% fee. **A wider TP, or
the same shape somewhere cheaper, is open; the stated bracket is not.**

Lowest capital, highest win rate, simplest to express. The whole book runs on **1-2 SOL**
turning ~100 SOL/day.

- **Entry:** deep dip on a very hot, deep-curve token. Age med 1.9 min, vsol med 69.8, price
  -20.8% vs the 30 s high, -27.5% off prior ATH, market heat med **224 trades / 119 SOL gross
  in the prior 60 s**. Not a bottom-tick buyer (med +29% above the 30 s low).
- **It buys INTO the knife.** 56% of entries land immediately after a market *sell*, and
  prior-2 s net flow is negative at median (-2.4 SOL). **A `m_flow_window(2).net >= 0` bounce
  gate fights this entry** - do not add one.
- **Exit is a fixed bracket, not a trail.** Winners: gross move med **+16.9%**, constant across
  dip-depth buckets. Losers: med **-27.5%**. It exits at-touch and price keeps running (+17.6%
  further median in the next 60 s) - the right tail is deliberately left.
- **Sizing:** fixed ~0.50 SOL (~900 of 1,125 buys in 0.48-0.54), next to the measured
  cost-optimal 0.27-0.5 for vsol ~70. Tip drag at 0.001/leg is ~0.4%/round-trip.

**Engine fit - everything needed already exists.** Entry: `m_price_window(30).trail`,
`liquidity` band, `m_flow_window(60).gross`, `time`. Exit: plain `take_profit` / `stop_loss`
sugar - no `arm_above_pct`, no trail, no stall dependency. The 0.9% bag rate makes a dead-flow
bailout optional (`m_flow_window(30).gross <= 3`).

Two caveats before believing any transferred number. **Latency:** its median winner resolves in
7.6 s and both exits fill at-touch; live TP/SL evaluation is feed-driven, so validate via
simulate with `pumpfun_impact` + `worst` fill. **Gate recall:** a first-guess gate (liq 55-85,
trail30 >= 15, gross60 >= 70, age >= 0.5 min) recalls only **27%** of its entries jointly
(60-77% each) - the bands interact and need a sweep to place. In-gate episodes do outperform
(69.7% win / +2.83% vs 63.4% / +2.03%).

### `FBvx` - profitable every day, unreachable at +1 slot

51,414 closed episodes, 33,850 mints, 22/22 days positive, and the entire edge is a **+6.15%
median price gap that opens and closes inside one slot**. The same book at +1 slot latency is
**-2.44%, 0/22 days**. No exit and no sub-population rescues it (0 positive cells out of 80
over eight-way decile scans on ten features). Full study:
[`../../history/2026-08-20-wallet-fbvx-intra-slot-absorption.md`](../../history/2026-08-20-wallet-fbvx-intra-slot-absorption.md).

Four things transfer from it regardless:

- **Absorption beats momentum, and "no sellers" is the bearish tell.** `bshare = 1.00` - a pop
  nobody is selling into - is its single worst cell (-0.56%) while 0.70-0.80 is its best
  (+2.94%). Independently reproduces the inverted `a_deficit` sign.
- **Proven participation is a band, not a threshold.** Every decision-time feature is
  hump-shaped: too few buyers is dead, too many is late. The only monotone feature is **sell
  pressure ahead of it in its own slot** (+3.12% at the top decile).
- **Confirmation inside the same slot beats being first.** In-slot rank 4-5 books +2.13%
  against +0.89% at rank 1-2.
- **Its exit is copyable and independent of the entry problem:** one trail off the running peak
  with `peak` initialised at entry, no take-profit, hard ~10 s timeout, 0.26% bags. Flat retrace
  (-8% median) across every MFE bucket is what a single trail looks like.

### `64hP` - the bag problem

Mechanics are accurate and worth reading; the wallet is not a template. **Exit is ONE rule, not
three:** bucketing exit-retrace by MFE gives a flat -4.9%..-7.4% band for every bucket with
MFE >= 5%, and episodes that never rose exit at median -7.16% - the same trailing stop with
`peak` initialised to the entry price. **Re-entries improve monotonically with index** (ep1
52.0% win / +5.18% avg to ep9+ 65.0% / +7.73%), replicated on `omego`.

**The bags are the open question.** 227 episodes (3.3%) never close, -225 SOL. The rate is
constant across days including gap-free ones, only 2/227 mints ever trade on AMM, and they are
not rugs - marked at a fixed horizon the median is +0.2% (15 s) / -1.9% (60 s) / -5.8% (300 s).
The trade stream simply stops near entry. A bail-out rule at any of those horizons turns the
cohort net-positive. Full study:
[`../../history/2026-08-18-wallet-64hp-full-study.md`](../../history/2026-08-18-wallet-64hp-full-study.md).

### `omego` - refuted, and two findings that outlive it

Gross edge **+1.81%/turnover against the 2.53% fee**. Its real profit is an unclosed runner
tranche - 72 of 451 mints marked at 83.00 SOL, top-3 concentration 13.2%, 0 of 72 with zero
trades after its last leg - and the engine had no partial-exit concept when the pattern was
tuned, so the shape being tuned was structurally incapable of the thing that made the money
([partial-exits.md](partial-exits.md) closed the capability and has not been re-measured
against it).

- **Winner and loser holds are identical**, so the exit is price-action-driven, not time- or
  PnL-schedule-driven. Exits happen in *dense* flow (med 0.1 s since the last market trade)
  because an exit needs liquidity. There is no "no-trades-for-N-sec" exit.
- **Not copy-triggered.** Only 36% of entries have a >= 0.5 SOL market buy in the prior 1 s;
  reaction is 0.112 s median at slot delta 0.

### `trunoest` - momentum ignition

Included because the distinction matters: copying its entry and exit without the ignition
mechanic samples a different, weaker distribution. The loop, one token at a time: pick a very
young, violently-moving token (age med 69 s, vsol med 60, prior-60 s 105 trades / 70 SOL gross,
entry -19.1% off the 30 s high but +37% above the 30 s low, 30 s net flow strongly positive) ->
**ignite** with ONE oversized buy (med 3.99% of vsol, ~+8% own impact; market net flow flips
from -0.63 to +5.72 SOL in the next 5 s) -> **paint the tape** with 0.0097 SOL micro-buys while
holding -> **dump on confirmed reversal**, one full-balance sell at -29.5% off the episode peak
after a median +50% run.

**Size hurts:** 1.95 SOL is the sweet spot (76% win, +10.3 med); 2.93 drops to 52%; 4.88 is
flat. More impact buys more ignition and a worse exit. **Loss containment is the weak spot** -
21 bags / 60 SOL sunk at med -48.8%; a -35..-40% catastrophe SL under the wide trail keeps most
of the closed-episode profile intact. Runs 08:00-22:00 UTC only - a human-scheduled operation,
not a daemon.

### `6LWSrd` - graduation selection

`6LWSrdCghRVhFiY9a2aHgYyTC1Hn6c9X8SczawdEFkVB`, 07-22..08-05, 1,925 entries / 194.7 SOL. Fixed
**0.1 SOL spendable** (96% of buys), single full sell 86% of the time, no TP/SL cluster.

**The edge is graduation selection, not the exit.** 10.49% of its buys migrate against a 2.095%
baseline (n=269,579) - a 5x lift. 1,723 non-migrated = **-13.6 SOL**; 202 migrated = **+29.0
SOL**. It pays ~14 SOL of small losses to buy 202 lottery tickets. Median exit multiple 0.72x.
It enters on **pure bot flow** - organic (retail front-end) net flow at entry is median 0.064
SOL, 0.4% of pre-entry flow, against a median vol/sniper net of 19.3 SOL.

## Latency tolerance is a property of entry dip depth - screen on it first

The one number that predicts whether a copy target survives our fills, measured as the price
change from the wallet's pre-trade spot to the first print at **S+1** (1,500 buys each,
07-31..08-15):

| wallet | entry vs 30 s high | +1 slot slippage (p50) |
| --- | --- | --- |
| `63ot` | -20.8% | **+0.82%** |
| `omego` | -12.6% | +2.37% |
| `64hP` | -22.7% | +3.93% |
| `FBvx` | ~0% (absorption turn) | **+6.15%** |
| `3Xk2` | **-1.1%** | **+9.24%** |

A dip entry buys into falling price, so a slot of delay is nearly free; a breakout or a turn
entry buys into rising price and pays for it. **Read the dip-depth column before simulating
anything** - a target that buys within a few percent of the local high needs *same-slot*
landing to be worth attempting, and this bot lands at +1 slot at p50.

**Corollary: an edge one slot wide is only reachable by same-slot landing.** That is why
`3Xk2`'s realized book is flat over days on which its own entries carry 37% MFE, and why `FBvx`
inverts across a single slot boundary.

## Resolution is a gate: a grid coarser than ONE SLOT cannot see a reactive trigger

Before concluding "no signal", state the grid resolution and check it against the width of the
event being hunted. A reactive trigger one slot wide is averaged away by any coarser bucket and
the search reports a confident false negative.

This rule was produced at 10 s resolution, backed by a gradient-boosted model reading the
wallets' behaviour at **OOS AUC 0.862** over 40+ features. A 10 s bucket spans ~25 slots, so a
single-slot 2-4x impulse becomes a 5-10% bump in the aggregate - below noise.

## Measuring what a wallet SELECTS: choice-set AUC, flow-weighted

For each bucket, reconstruct every token alive at that instant and rank the wallet's pick among
them; the mean percentile **is** an AUC (0.50 = indifferent). It assumes no fill model, fee,
latency or exit rule, which is what makes it safe where copy-the-trade methods are not.

**Always compute it twice.** Against a random *token* all four early wallets read as momentum
chasers (volume AUC 0.74-0.92); against a random *buy* (flow-weighted by buy count) the same
wallets on the same data invert to contrarians (0.28-0.46). Only the second controls for
everyone crowding into active tokens. Quote the flow-weighted figure; the raw one is a
statement about the crowd, not the wallet.

Participant composition comes from `ix_labels`: retail-UI (Axiom ~39%, GMGN, Photon/Terminal,
Bloom), nonce-bot, custom-bot, aggregator, direct. All four **avoid retail flow** (0.25-0.33)
and **favour nonce-bot presence** (0.54-0.73).

## Creation shape: no signal for SELECTION, real signal for OUTCOME

Two different questions, two different answers. Getting them backwards is the trap this section
exists to prevent.

**Which token a scalper picks** - creation shape carries **no** signal. Fingerprint-axis
testing gives chi2/df ~ 1.0 on every axis. Hence the design rule: **use a maximally-broad
fingerprint for selection.**

**How an episode ends on a token already picked** - `initial_buy_lamports` (the dev buy) is the
usable axis, at chi2/df 2.03 over 19 buckets against 0.82 for `cu_limit`/`cu_price` and 0.44
for `is_cashback_enabled`. At the 12.8 SOL cut, over `64hP`'s closed episodes: dev buy < 12.8
gives 49.1% win / +2.44%/ep on 6,218 eps; **12.8-25.6 gives 59.2% / +7.11%** on 284; >= 25.6
gives 76.9% / +8.51% on 13.

It survives scrutiny where bucket-derived gates do not because it is **monotone in both the fit
and the holdout window** - a threshold *family* improving everywhere, not a best-of-N single
bucket. Mint-level block permutation (2,000 shuffles, preserving within-mint clustering) gives
p = 0.006 on win rate and p = 0.037 on net. It is not a liquidity proxy (the lift survives
conditioning on entry-vsol band) but it **inverts above vsol 75**, hence the 40-75 band.

`init_buy` is also the tractability lever: a broad fingerprint arms ~18,000 tokens/day,
`init_buy` in [12.8, 25.6) arms ~110/day, and a 6-day simulate folds in 60 s instead of ~20 min.
It is an **instant** axis (`has_instant_criterion`), matching synchronously on `TokenCreated`.
**`initial_buy_lamports` is the NET curve amount**, so dev-buy clusters sit at `gross x
0.98765` (12.0 -> 11.8519) and cuts must be placed in net terms.

## Sell deceleration is the best loss-avoider found - and it is not an edge

`sell_decel` = (sell lamports in the last 10 s / 10) / (sell lamports in the prior 30 s / 30).
Universe-wide it is **monotone across all ten deciles**: forward-30 s return +1.27% in the
lowest decile (selling dried up) to -8.20% in the highest. On young active tokens it moves
forward-60 s return from -16.0%/-14.7% to -4.5%/-1.7% (IS/OOS).

Use it as an **exclusion gate**, never as an entry reason. Stacking it with the rest of the
"lull" family walks return from -9% toward -1% but never positive, while max favourable
excursion collapses 16% -> 5% and share-up falls 40% -> 21%. It is a *silence filter*: it
selects tokens that stop moving, so a trailing exit on it earns nothing. Buy deceleration is
the weaker, U-shaped cousin - prefer the sell side.

## Measurement rules earned here

**A wallet's bag rate is a claim about our ingest before it is a claim about the wallet.** Plot
unsold-episode rate per day against that day's global print count. `64hP`'s 3.3% is flat across
days and survives; `3Xk2`'s reads 23.9%/27.3% on the two days our feed runs at half rate against
a 2.2% clean-day baseline - our missing sells, not its bags. Counting them moves its book from
+2.62% to +0.48%, a refutation manufactured out of a feed outage. **Restrict any wallet book to
ingest-clean days.**

**A simulated exit must land on a print that has liquidity after it.** A trail that never fires
and gets marked at the last observed print books the *peak* of a token that stopped trading: on
`3Xk2`, 83.6% of such timeouts have zero prints in the following 5 minutes, and pricing them
that way reads a clone at ~0% instead of -1.67..-20.79%. The distortion **grows with trail
width**, so it flatters exactly the configurations a search promotes. Charge an unfillable
timeout as a bag.

**Read a copy-target's exit as an *armed* trail before authoring it.** Measure the in-hold peak
gain and the retrace off that peak separately: `8dtx`'s median hold peaks +11.6% then exits
-18.4% off the peak, which an unarmed `m_position.retrace >= 12` mis-renders as a -12% hard stop
from entry (the peak seeds at the entry fill). Getting this wrong inverts the exit's effect.

**Token death is the dominant cost on an unselected universe, and `m_flow_lifetime.gross_flow`
is the lever that controls it.** A dip-turn rule with no liveness floor exits `Dead` on ~22% of
its fills; a `gross_flow >= 30` floor (with `time <= 300`, `trail <= 30`) cuts that to 4.6% and
moves mean PnL -26.3 -> -9.3%. Ablation isolates the floor as the whole effect. It is the safe
**replacement** when a windowed hot gate is dropped, and load-bearing from the start on any
entry that wants a *quiet* tape. Past that point each further tightening buys its gain by
removing trades and converges on zero **from below** - a config walking toward breakeven on a
shrinking `n` has no edge, it has less exposure.

**Identity is a conditioner, not a signal.** A 155-wallet roster of who buys the same mint
within 60 s *before* `3Xk2`, built on days 1-12, separates its days 13-18 (+4.68% with the
roster present vs -0.28% without) and survives both a stratification by precursor count and a
buy-count-matched placebo. But buying when those wallets buy **loses 7.11%** - they are
co-racers of the same impulse, not leaders to follow.

## Closed searches - do not re-run these

- **Copying any wallet's landed transactions.** Negative at every reachable latency for every
  wallet studied, and negative at *zero* latency for `omego`. Our feed sees only confirmed
  trades, so same-block is impossible.
- **Cloning `8dtx` (`wallet_id` 2720).** Its edge is token *selection*, not the trigger: on its
  own picks the reconstructed rule prints +7.7% mean (PF 1.53) and on everything else -32.8%,
  at identical fills and costs. Mechanism spec: [wallet-8dtx-logic.md](wallet-8dtx-logic.md).
  Grid: [`../../history/2026-08-17-wallet-8dtx-clone-refuted.md`](../../history/2026-08-17-wallet-8dtx-clone-refuted.md).
- **Cloning `3Xk2` (`wallet_id` 1416).** Its edge is *entirely* the ~9.9pp the price moves in
  the one slot between its landing and ours: a mechanical trail on its exact entries earns
  +8.00% at zero latency and -1.67% at +1 slot.
  [`../../history/2026-08-18-wallet-3xk2-momentum-breakout.md`](../../history/2026-08-18-wallet-3xk2-momentum-breakout.md).
- **Cloning `FBvx` (`wallet_dict.id` 1092149).** -2.44% and 0/22 days at +1 slot; no exit and no
  sub-population recovers the 6.5pp.
- **The slot-1 buy impulse as a standalone rule.** `3Xk2` and `8dtx` both react to a large buy
  landing at **S-1** (buy volume 1.9-4.4x the trailing baseline, sell flow flat) and fire to
  land at **S**. The same-slot follow-on is **61% nonce-bots** - a crowd of latency bots racing
  one visible trigger, not private information. Universe-wide the impulse is informative and
  never pays: reacting at the next slot's close runs -5.24% (<2x) to -0.80% (>=16x), and the
  full signature yields a stable +1.00% pop within 1-3 slots that decays negative by +30.
  **The value is consumed inside the firing slot.**
  [`../../history/2026-08-18-choice-set-lull-signature.md`](../../history/2026-08-18-choice-set-lull-signature.md).
- **The "lull" signature as an entry** (sell deceleration + low position in range + small pool +
  concentrated buying). Walks return to ~-1% and kills MFE; a silence filter.
- **Identity rosters as a followable signal.** -7.11% standalone.
- **Off-chain sources to explain `3Xk2`/`8dtx`.** The trigger is on-chain and one slot wide.
  Durable-nonce infrastructure is itself the evidence: you do not pre-sign transactions to trade
  a tweet.
- **`omego` as a template.** Gross edge does not clear the fee.
- **Creation shape as a token *selector*.** chi2/df ~ 1.0 on every axis, repeatedly.
- **Bucket-derived single-bucket gates** (`range`, rise-at-low, `rise <= 1`). Best-of-N
  artifacts; none replicated out of sample.
- **A `flow(2).net >= 0` bounce gate on the dip-reversion family.** It contradicts the measured
  entry - these wallets buy into negative 2 s flow by design.
- **`6LWSrd`'s organic-flow premise.** An `org_wal >= 3` gate is positive in sample and does not
  replicate out of sample; the wallet enters on pure bot flow.

## Seeded rules and their status

Seeds live under [`hunter/scripts/`](../../../scripts/); all seeded rules are
`trade_mode='paper', is_active=false` by default - arm deliberately.

| Rule family | From | Status |
| --- | --- | --- |
| `fs4-*` | `63ot` fixed bracket (buy 0.5, TP 17, SL 28, liq 55-85, trail30 >= 15, gross60 >= 70) | seeded paper - **source wallet nets ~0; re-derive the TP before arming** |
| `fs3-*` | `64hP` + the dev-buy >= 12.8 gate | seeded paper - **source wallet nets -2.50%** |
| `fs2-*` | `64hP` knob ladder | **broad-universe control only** |
| `tru-0*` | `trunoest` (its size / impact-optimal 0.30 SOL) | seeded paper |
| `fs-*` | omego-calibrated | **retired** - omego is refuted |

**`fs2-*` is a control, not a candidate.** The ladder
([`seed-flow-scalper-64hp-rules.sql`](../../../scripts/seed-flow-scalper-64hp-rules.sql)) arms
~18,000 tokens/day against `fs3-*`'s ~110. Its knob conclusions survive that demotion, but two
are **revised** by the `fs3` runs and the seed file is not: the dip gate is best at **25** (not
18) and the vsol band at **40-75** (not 36-70). Use the revised pair anywhere `fs2` is quoted.

## Still open

- **`63ot`'s bracket at a wider TP**, and whether its at-touch fills survive feed-driven TP/SL
  evaluation at real latency. This is the only body-driven shape in the set.
- **`64hP`'s 3.3% bag cohort.** A timed bail-out looks profitable at every horizon tested and
  was never implemented or measured live.
- **`omego`'s runner tranche under the shipped scale-out ladder.** The capability now exists and
  the pattern has not been re-measured against it.
- **`FBvx`'s absorption shape at a reachable horizon.** Its 1.5 s trade is closed; whether
  absorption predicts continuation over 30-60 s universe-wide is a different, untested question.
