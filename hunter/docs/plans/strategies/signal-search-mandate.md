# Signal search: mandate, gates, and the open queue

Read this first in any session that continues the signal search. It states what the search is
for, the operator's market model, the hard constraints, what counts as a result, the gates a
candidate passes before it is believed, the live candidate families, and what is still open.
Detail on individual rounds lives in `docs/history/`.

## The purpose

Derive the underlying logic of the pump.fun scalper wallets that actually make money - what they
see, not what they land. Copying landed transactions is closed: latency, fee and slippage
distort them past usefulness. The output wanted is **the logic itself**, stated as a mechanism
sentence that can be defended against the moments the wallet did *not* fire; a tuned rule comes
after that, never instead of it.

- **Find new signal ideas**, not new statistics over the same tape. The named exemplars are
  sell-deceleration, lull signature, single-slot buy impulse: small, mechanical, computable from
  the print stream, each expressing a belief about *what the other side of the market is doing*.
- **Do not stop at the known wallets.** They are evidence that an edge exists and a scale for
  how big it is, not the target. Reproducing a footprint is not the goal; deriving what they
  read is. See [wallet-analysis.md](wallet-analysis.md) for each one's logic in a sentence.
- **Derive discriminatively.** A decision rule is defined as much by what it rejects as by what
  it accepts. Reconstruct the tape state as of the end of the previous slot at every moment the
  wallet was plausibly watching a token, split those moments into fired and did-not-fire, and
  ask what separates them. Describing landed trades in hindsight cannot recover a decision rule.
- **Stay at signal altitude.** Funding-graph reconstruction, cross-venue dashboards and external
  data vendors are out of scope unless a signal already justifies them.
- **Iterate.** If a round of candidates fails, the answer is another round of new signals, not a
  retune of the failed ones.

## The operator's market model

Domain knowledge supplied by the operator from manual trading. Treat it as prior, not as
measured fact - but note that it has predicted several measured results here in advance.

- Token creators manufacture volume with **rotating wallets, some fresh and some not**, to
  imitate organic demand and attract real traders. Instruction structures and fee sizes vary
  per volume-making transaction, and creators move reserves between wallets specifically to
  break tracking. **Separating creator and cohort wallets from real traders is expected to be
  impossible**, and wallet-identity work built on transfer graphs is expected to fail.
- The creator decides **rug or migrate at a specific moment, by watching the token's state**.
  The most influential input is **real traders' entry SOL**: too little entered and they may
  push it to migration instead, to eat the traders who hunt migrating tokens; a large amount
  entering suddenly mid volume-making triggers the dump.
- **The dev watches the same public tape we do.** So the trigger is an aggregate tape event and
  identity is not needed to detect it. The corollary is uncomfortable and testable: the event
  that looks most bullish - a sudden large buy - may be *the cause of* the rug, which would
  explain why flow and impulse features here predict pops with the opposite sign to terminal
  return.
- **Volume-making is bundled, so its slots are not fillable.** A backtest that fills at a
  bundled slot's median price is fiction. Mark bundled slots unfillable before believing any
  entry that lands on one.
- **The stable winners' strength is the logic, not latency or parameter values.** When the
  regime changes they re-fit their thresholds; they do not change the logic. The deliverable
  shape that follows is **a fixed shape plus re-fittable thresholds** - which is also what
  round 10 measured, since re-searching the shape daily loses badly to fixing it.

## Hard constraints on any answer

| Constraint | Value | Source |
| --- | --- | --- |
| Round-trip cost bar | `2.53% + 4*sqrt(F/vsol)`, about **3.45-3.6%** at vsol 30-40 | [fill-and-cost-models.md](fill-and-cost-models.md) |
| Best gross edge constructible from all 29 observables | **+2.6%** | [inverted token search](../../history/2026-08-18-inverted-token-search.md) |
| Fill model | entry at `next_slot_median` (measured p50 = +1 slot), exit at the **next print past** the trigger, own `B/vsol` impact on both legs | same |
| Sizing | money-optimal `B* = e*vsol/4`, about **1% of pool**; `B = sqrt(F*vsol)` minimises cost percentage, not money. `F = 0.000225` SOL/leg | [execution-costs.md](execution-costs.md) |
| Usable window | **2026-08-11 onward**; `trades` chunks run 100-280 MB/day before and 1.7-2.0 GB/day after, so mixing invents feed artifacts | inverted token search |
| Measurement resolution | **one slot**; a coarser grid reports "no signal" with full confidence | [lull signature](../../history/2026-08-18-choice-set-lull-signature.md) |
| Helius spend | critically sensitive, never call without asking | root `CLAUDE.md` |
| `trades.fee_lamports` | NULL for every row, so priority-fee competition is unmeasurable | inverted token search |
| `tokens.meta` | holds only `uri`, so no off-chain social or narrative feature is testable | inverted token search |

A candidate that cannot plausibly reach **+1pp of gross above the current +2.6%** is not worth
building. The gap to break-even is the whole problem.

## A signal is not a screen

This is the distinction the search keeps losing, and it is the reason three rounds produced one
usable rule and zero signals.

| | screen | signal |
| --- | --- | --- |
| says | this token is likelier to be good | something just happened, act now |
| built from | a trailing-window aggregate | an event and the response to it |
| pays through | tail density in a basket | a timed entry |
| shape | negative median, tail-carried | positive median, body-driven |

Every candidate the search has kept is a screen. Screens on this venue produce lotteries: the
median trade loses, one token in a hundred pays for everything, and the left tail is
unfilterable because risk and return are the same axis. That shape matches the two study wallets
that clear the fee bar - but **63ot**, the one wallet with a positive median (+12.38%) and a
66.2% win rate, has a shape nothing in the search reproduces. That shape is the target.

**A candidate that is an unconditional aggregate over a trailing window is a screen.** Say so up
front and rank it accordingly, or condition it on an event and make it a signal.

### Why the dead features are dead

Sort every feature ever built here by outcome and the split is by *form*, not by subject.

- **Dead** (permutation-certified, floor `|sep| = 0.005`): `f_age` 0.0023, `f_conc` 0.0019,
  `tx_index` 0.0018, `f_r10` 0.0015, `f_buyaccel` 0.0006, `f_r30` 0.0005. All are
  **unconditional aggregates over a trailing window** - how much of X happened in the last N
  seconds.
- **Alive** (every idea that has ever shown life): the lull signature, the single-slot buy
  impulse at S-1, `3Xk2`'s break of the 30 s high into positive 2 s flow, `8dtx`'s turn after a
  quiet, `FBvx`'s absorption, the operator's sell-deceleration. All are **the tape's response
  after a specific event**.

The reason is mechanical. On a bonding curve price is a deterministic function of net flow, so
level and flow are public and priced the instant they print and an unconditional average of them
carries nothing anyone else lacks. Information survives in two places only:

1. **Event-conditioned response.** What happens right after a trigger, measured against what
   normally happens after *that* trigger. The conditioning is the signal; the aggregate throws
   it away.
2. **Stock, not flow.** The state of the holder base - who holds, at what cost, how much is
   underwater. Flow is priced; the distribution of holder cost basis is not, because computing
   it takes an accumulation across the token's whole life.

## Candidate families

Each entry gives the mechanism first, because the mechanism is what survives when the
parameters fail. Struck rows are closed; the rest are unbuilt or unfinished.

**Family A - response signals (the tape's answer to an event)**

| # | Name | Mechanism | Definition sketch |
| --- | --- | --- | --- |
| A1 | ~~Supply-response deficit~~ | Holders who refuse to take profit into strength are withholding supply | Built as `a_deficit`. Real and **sign-inverted** - a pop drawing no selling is bearish, and it selects variance rather than direction. |
| A2 | **Pullback purity** | A shallow retrace that gives back little of the pop's own buy volume marks a strong holder base | On the first retrace after a pop: depth, and the share of the pop's buy volume sold back. Winners retrace 5.7% and losers 19.1% in the `64hP` study, measured inside trades and never used as an entry. |
| A3 | **Seller exhaustion** | The marginal seller is shrinking, so the down-leg is running out | Into a falling price, decay in sell print **sizes** across consecutive slots. The down-leg twin of A1, and the closest unbuilt relative of `FBvx`'s absorption. |
| A4 | ~~Break and hold~~ | A level breaks and no seller reclaims it | Closed: `a_brk` is bearish. Momentum signals on this venue buy a top. |
| A5 | **Response-time signature** | Organic crowds and scripted crews have different reaction latency distributions | After a trigger buy, the delay distribution of follower prints. Organic decays geometrically over slots; scripts spike at fixed delays. Sub-slot resolution via `tx_index`. |
| A6 | **Rug hazard** | The dev pulls when a large amount enters suddenly, so the bullish-looking event is the trigger | `P(terminal dump within k slots \| size of the largest recent buy, inflow burst, inflow acceleration)`. Produces an *exit* even if no entry comes out of it, and the program currently has no mechanism-driven exit at all. |

**Family B - holder state (stock, the largely untouched space)**

| # | Name | Mechanism | Definition sketch |
| --- | --- | --- | --- |
| B1 | ~~Underwater float share~~ | Underwater holders anchor to their entry and do not sell into small pops | Built in round 2. Stock space is measured and closed for the dip-bounce trade from both directions. |
| B2 | **Profit overhang** | The supply that will hit the next pop is the supply already deep in profit | Share of float held at more than X% unrealized profit. Low overhang plus a pop means room to run. |
| B3 | **Cost-basis distance** | A crowd approaching break-even from below produces a predictable relief-selling wall | Current price against the buy-VWAP shelf of the holder base. |

**Family C - fake-volume invariants (automating the manual vol/non-vol split)**

Instruction structures and fees rotate for free. These cost the operator real money or real time
to fake, so they survive evasion:

| # | Name | Mechanism | Definition sketch |
| --- | --- | --- | --- |
| C1 | ~~Retention ratio~~ | Degenerate on a curve | `delta vsol` **is** net SOL flow by construction, so `delta vsol / gross volume` reduces exactly to `f_net30`, an already-tested trailing aggregate. Sound on an AMM where wash volume leaves reserves untouched; carries no independent information here. |
| C2 | **Churn share** | Round-tripping is round-tripping however the transaction is dressed | Share of window volume from wallets whose net position change over the window is about zero. |
| C3 | ~~Fresh-wallet share~~ | Wallet age cannot be faked retroactively | Built as `c_fresh1h` and shipped in [fresh-wallet-entry-rule.md](fresh-wallet-entry-rule.md). Detects manufactured flow, not fresh demand. |

A **single-transaction wash** - a buy and a sell inside one `tx_signature` - is exact and free to
compute, and outside work on other pump.fun datasets puts it at about 21% of pre-migration
transactions. It **does not reproduce here**: 4,755 such slots across 8 days and 191,235 mints,
effectively zero. Round-trip identity across transactions (C2) is the only surviving route to a
wash measure on this venue.

## Gates a candidate passes before it is believed

Each of these exists because skipping it produced a wrong published result.

**Distribution gate - check the raw values before ranking anything.** A feature that is 97.8%
identical carries no information where it is applied, and quantile ranking hides this completely:
`pd.qcut` on `rank(method='first')` splits pure ties into "quintiles" by row order, so a
degenerate feature reports clean-looking buckets that are really an arbitrary sort. Print the
value counts and the tie fraction first.

**Tie-break gate - beat a random draw of the same size, not zero.** Any filter that removes a
fraction of the pool must be compared against 20 random draws removing the same fraction. A
result inside 1 sd of that spread is a tie-break artifact.

**Marginal gate - rank a candidate by what it adds to the rule, never by its standalone screen.**
The two disagree in both directions.

**Redundancy gate - correlate a new feature against every feature already in the rule.** A
correlation of 0.99 against an existing filter means a renamed duplicate.

**Confound gate - correlate against log age, `vsol`, holder count and market-wide volume.** A
clock or a size proxy passes every profitability test and generalises to nothing.

**Cost gate - re-solve `B = sqrt(F * vsol)` at each fee level before reading a cost sweep.** A
sweep at fixed size overstates the damage and has already killed one live signal wrongly.

**Exit gate - tune the exit on the selection it will run against, and read hold length jointly
with the take profit.** A hold that overfits at one TP improves both windows at another.

**Book gate - read the winner in SOL, not in percent.** A better per-trade number is often a
worse book once trade count and drawdown are priced.

**Collapse gate - one trade per token.** Per-entry means invert sign against per-token means.

Standing protocol: day-block bootstrap over whole days, an IS/OOS split fixed before tuning, a
placebo of random same-size draws from the candidate pool, and a latency sweep of 0 to 4 prints.

## Two search-design errors to avoid repeating

**Optimising the mean selects lotteries.** On a fat-tailed venue the mean is dominated by the
tail, so a mean-ranked search cannot find a body-driven rule even when one exists. Rank
candidates on **median and win rate** as well, and keep the two rankings separate.

**A universe-wide average is the wrong test for a mechanism.** A signal only has to pay on the
subpopulation where its mechanism applies. Judging a mechanism by its universe-wide expectation
retires live leads - the lull signature has a real slot-level trigger and was retired this way.

## What is settled - do not re-derive

- Price on a bonding curve is a deterministic function of net flow, so **level and flow are
  public and priced instantly**. Unconditional aggregates of either are dead.
- **`a_deficit` sign is inverted**: a pop drawing no selling is bearish, and it is a variance
  selector rather than a direction selector.
- **`a_brk` is bearish**: breaking the prior 30 s high predicts a worse forward return. Momentum
  signals on this venue buy a top.
- **A stop loss is inert here.** The collapse happens inside a print gap, so the fill lands below
  the stop. The left tail is an entry property, not an exit one.
- **The A1 conditioning trick does not generalise.** Ranking a response inside its own
  trigger-size bucket works for a response measured against a trigger, not as a general purifier.
- A grid coarser than one slot reports "no signal" with full confidence.
- **A lull is a gap in slot numbers, not a low-activity aggregate.** `iv.sp` holds active slots
  only, so `slot - lag(slot)` measures silence exactly and costs nothing.
- **The un-armed third is an entry property.** Under an armed trail the trade decomposes into
  positions that reach the arm threshold (+13% net, 78% win) and positions that never do (-36%
  net, 0.5% win). No exit reaches the second group: not a price stop, not a time bail, not a
  tighter arm. Every attack on it converges on "do not trade".
- **Arm rate alone is the wrong objective.** Raising `P(arm)` lowers the armed payoff and
  deepens the un-armed loss in lockstep, so break-even moves with it. Across 54 buckets on 9
  unrelated axes the arm rate spans 22% to 81%, 52 of 54 buckets are net negative, and the
  correlation of arm rate with net is **-0.39**. **Predicting the bounce is not the problem.**
- **Entry selection is closed for the dip-bounce trade**, stock space included and from both
  directions. Seller inventory predicts the bounce monotonically (arm rate 55.5% to 73.9%) and
  holder concentration predicts the loss monotonically (-54.6% to -20.4%); neither moves net,
  both are largely `vsol` proxies, and a 256-cell conjunction over all eight surviving filters
  returns **0 positive cells out of 187** with at least 250 tokens.
- **A trail is the wrong exit shape here; leave in a few seconds.** The fall happens inside a
  print gap, so a trail fills well past its trigger. Median hold 3-6 s beats the armed trail by
  **2.7pp** of gross on the same entries, and beats exiting on the very first print. Every
  entry measurement in rounds 1-4 overstates the cost of a bad entry for this reason.
- **The exit question is closed: the first-gap exit already is the dynamic exit.** Continuous
  printing is the up-move and the first pause is the top, so its hold is path-dependent by
  construction - it stretches while a campaign runs and cuts in ~1.5 s on a dead entry. Seven
  exit families across rounds 5-6 (sell-size thresholds, two-state armed legs, pattern-silence,
  pattern-state) land within 0.1pp of it or lose. Stop building exits; the lever is entry.
- **An entry that waits for confirmation pays its edge to the slot boundary.** The k-th
  same-pattern print carries +4.7pp of selection information over a blind delay of the same
  length, and one slot of honest entry latency turns +0.08% into -2.75%. Charge the next-slot
  fill on any confirmation-style entry before reading its number (round 6; same failure as the
  3Xk2 clone).
- **The `ix_labels` entry channel is closed - its information horizon is shorter than one
  day.** Buyer-identity selections (a learned whitelist of specific breaker structures, and
  in-slot repetition `mode_ct >= 5`) pass every within-window gate - placebo draws, boundary
  shifts, blind adjacent-day OOS (+1.13% net vs -1.26%) - and then fail on the first genuinely
  later day: whitelist **-4.11%** vs a -1.76% pool, repetition at pool, rolling 1-day books
  inside placebo range (rounds 6b/6c). The failure mode: both extremes of a pattern book
  underperform the unremarkable middle out of window, because an extreme pattern is one
  operator's campaign and operators rotate within a day. No book fits the horizon - scoring a
  pattern needs days of tape and the edge dies in less than one. Volume campaigns are real as
  a venue fact; their identity does not persist. Rarity refinements are tie-break artifacts,
  and the durable-nonce flag alone is negative. **Online learning does not reopen it** - every
  update speed (hourly, 4-hourly, per-event, k-up-moves-then-enter) reduces to "act on firing
  #k of a pattern from the resolved outcomes of firings 1..k-1", and that whole family is
  measured at once in round 6d: payoff is flat in occurrence index (the learner is not late)
  and a pattern's own resolved same-day record lifts payoff by at most ~+1.0pp, landing at
  -0.5..-1.1% - never positive, 3/9 days, at pool on the fresh day; the trailing-4h recency
  window is WORSE than day-scoped. Slow books die of rotation, fast books die of information
  size. **Combining ix with the chain does not reopen it either** (round 6e): an exhaustive
  1-3 term search over 14 chain/context/ix conditions on 105,263 superset entries yields 2 of
  417 cells positive in IS and **0 positive across IS, OOS and the fresh day**. What ix adds
  jointly is real, rotation-proof and far too small - generic ix FLAGS (multi-wallet burst,
  durable-nonce, prior-repeat), unlike pattern identities, lift a chain cell by a stable
  +0.13 / +0.12 / +0.22pp across the three segments.
- **MFE is a volatility measure here, not a reachable target - rank entries on realized net,
  never on MFE.** Selecting for MFE works and is the one ix result that survives the fresh day
  (`dip<=-.20 & imp>=2 & mode_ct>=5` lifts mean path MFE 47% -> 68%, net stable at
  -1.08/-1.14/-0.63 across IS/OOS/FRESH), but of the 47pp mean path MFE only 5.3pp (median
  2.0%) occurs before the first-gap exit and 44.5pp (median 23.0%) occurs after it. Reaching
  for it loses at every width: a TP layered on the gap exit changes nothing (the gap fires in
  ~5 s first), and TP-only or pure holding to 300 s runs -4.73% to -9.65% versus -1.76% - with
  timeout fills at the last print, which is optimistic. The paths that spike after the exit
  end far below the spike (round 6e).
- **An OOS window adjacent to the IS window shares its regime.** Any selection learned on
  operator identity (instruction patterns, wallets, fingerprints) must be validated across an
  operator-rotation horizon - a genuinely later day - before it is believed. Adjacent held-out
  days answer "is this real in the window", never "will this pay tomorrow". The crew-filter
  and round-6b results both passed adjacent OOS and failed forward.
- **Holder concentration reads backwards from intuition** - dispersed ownership means a larger
  loss, not a smaller one, because a dispersed token has already run and has a crowd to leave.
  Measured from `trades` it is also defeated by bundled launches, in the unsafe direction.
- **Venue state does not gate the trade: `P(arm)` is invariant to it.** Six trailing
  five-minute venue variables built at one-minute resolution over nine days - live mints,
  new launches, venue buy SOL, venue net-flow share, mean buy size, hour - correlate with
  the arm flag at |r| <= 0.013 on both the chain superset and the fresh-wallet pool, and
  the arm rate spans only 71.4-72.6% (chain) and 38.4-40.8% (fresh-wallet) across every
  bucket. Chain net is flat with it (all 30 buckets in [-1.56, -1.94] against a -1.74%
  pool) and an exhaustive two-way search returns **0 of 329 cells positive across IS, OOS
  and the fresh day**. The one surviving lead is a lead only: on the fresh-wallet selection
  the 00-05 UTC block runs +5.02% against +3.12% and is positive 9 of 9 days, but scores
  z = +0.45 on the fresh day itself from a 30-cell search (round 7).
- **Mark an exit path on `px_sell_med` alone and drop slots with no sell print.** Coalescing
  to the buy median marks an up-tick nobody could have sold into, lifts the running max,
  fires the trail at a price that never existed and fills on the buy side. It moves the
  fresh-wallet rule by **1.7pp** (+1.95% against the correct +3.67%). Entry reference is
  `e_px*(1+bsz/e_vsol)` and the exit multiplier `(1-bsz/e_vsol)*0.9875/1.0125`; that is the
  same cost as `0.0253 + 4*sqrt(F/vsol)` but applied multiplicatively (round 7).
- **The fresh-wallet rule is forward-unproven and not fundable.** On 2026-08-19 its live
  form books -0.34% against a -0.80% pool, placebo z falls 9.46 -> 0.38, and its
  venue-controlled edge (+0.46pp) is the worst of nine days, below the eight-day minimum of
  +1.31pp. One day of a 32%-win lottery refutes nothing - the fresh-day CI is [-2.43,+1.94]
  - but it confirms nothing either, and the live nightly-cutoff form is the worse form
  because `c_fresh1h` drifts faster in the older age band than a 7-day window tracks
  (p80 0.29 sealed -> 0.60 on the fresh day, costing ~0.64pp).
- **The 125 bps protocol fee is immovable and cashback does not touch it.** Over 9.6M buys
  in the window, 27.7% of non-cashback and 26.1% of cashback buys land on a round gross
  grid under the 125 bps hypothesis, against 2.3-2.6% under 100 bps - below the 4.6%
  random-hit null. No dynamic schedule, no taker rebate; cashback is a creator-side launch
  flag. That fixes 2.53pp of the 3.45% bar permanently. `trades.fee_lamports` is 100% NULL
  on all 19.6M legs, so `F` cannot be checked against the tape (round 8).
- **`B = sqrt(F*vsol)` minimises cost percentage, not money - read SOL, not net %.** SOL per
  trade is `B*e - 2B^2/vsol - 2F`, maximised at `B* = e*vsol/4`, about **1% of the pool**
  against the 0.25% that `sqrt(F*vsol)` gives. Both break even at `e > 4*sqrt(F/vsol)`, so
  **the 3.45% bar is correct**; only the magnitude was misread. On the fresh-wallet rule 1%
  sizing pays **7.48 SOL/day against 2.56**, 8 of 8 days positive, for 6.0x the drawdown
  (1.02 -> 6.17 SOL). It rescues nothing - scaling cannot change a sign, and on the fresh
  day it turns -0.07 SOL into -2.22 (round 8).
- **At money-optimal size the tip is a rounding error.** `F` enters only as `-2F`: a 40x
  range moves the result 19%, and dropping Jito for a bare priority fee is worth about +4%.
  The earlier reading that "a larger `F` buys a larger position and more SOL/day" is a
  **sizing artifact** - `F` was setting `B` (round 8).
- **Charging the exit leg's impact at entry depth is conservative, not optimistic.**
  Repricing at true exit depth gains +0.03pp, and 0% of exits exceed 10% of the exit pool at
  any size through 3% of pool. The pool grows during the hold (round 8).
- **A dominant buy print is not punished by the tape.** At 1% sizing our order is the largest
  buy in its slot 64.3% of the time and a median 133% of slot buy volume. Matched within
  mint x slot-volume decile (55,920 cells), slots where one buy is >=90% of volume are
  followed by **+6.25pp better terminal and +9.00pp better MFE**; unmatched they look worse
  only through the lull confound. Do not claim the +6.25pp - those buyers may be informed
  (round 8).
- **Entry depth is a signal, not the cost cut the queue called it, and it holds forward.**
  The cost difference between `vsol` 30 and 50 is ~0.1pp; the effect is 40x that. `P(arm)`
  is **monotone across all ten entry-`vsol` deciles, 9.9% -> 74.5%** - the wall venue state
  could not move. Inside the fresh-wallet screen, decision-slot `vsol >= 40` books **+7.24%
  over the nine complete days (n=971, `P(arm)` 75.9%, median +12.99, 8 of 9 days positive)**
  against a 66.0% break-even, and **+5.40% across the two forward days 08-19 and 08-20**
  (n=156, `P(arm)` 82.7%, placebo z=2.29) where the rule's shallow half books -0.67%.
  Ten days hour-matched: net +9.64%, bootstrap CI95 [+3.38, +16.45], placebo z=4.94.
  **Both halves are required** - depth without the screen is -3.75% forward, the screen
  without depth is -0.67%. It is the program's first **body-driven** shape: positive median,
  top token 9.2% of PnL, dropping the best token costs 0.9pp. Filter on **decision-slot**
  depth; the entry-slot form leaks 0.23pp. Round 8's [42,45) band was noise - forward it
  falls from +25.77 to +8.51 - but **every band at or above 40 is positive forward and both
  below are negative**, so the sign flip at 40 is the structure (rounds 8-9).

- **`alive` on `iv.dp`/`iv.dp9` is LOOK-AHEAD - never use it.** It counts slots carrying a
  sell over `range between 60 seconds following and 360 seconds following`, i.e. the future.
  It reads +19.71% against -4.77% consistently in every window because it is an oracle for
  "the token did not die". `uwb30` is also just `nb30` (r = +1.000) (round 9).
- **A refutation on the full pool is not a refutation inside the deep cell.** Earlier rounds
  measured every feature on a population that is 85% shallow with a 34% arm rate; the deep
  cell arms 76%. Crossing all 19 stored features with it (38 tests, n=1,034) revives the
  **lull**: `nb30 <= 3` - fewer than four buy prints in the trailing 30 s, the round-1
  silence signature that "never pays universe-wide" - is worth **+4.5pp** here. Stacked with
  `f_r10 <= 0` (no 10-second run-up) the cut books **+12.91%** (IS +16.52 / OOS +8.71 /
  FWD +7.15), median +16.51, 9 of 10 days, bootstrap CI95 [+7.56, +18.40], placebo z=2.73.
  It buys quality not throughput - 108 trades/day falls to 45, so SOL/day moves only
  2.44 -> 2.87 (round 9).
- **The slot boundary, not the fee, is what closes the copy family - measured on a wallet that
  demonstrably wins.** `FBvx` books +398 SOL and is net positive **22 of 22 days**, and its
  whole edge is a single-slot reversal burst: slot `S+1`'s median buy price is **+6.15% above
  slot `S`'s and higher 72.2% of the time**. Holding its signal, trail, size and cost model
  fixed and moving only the entry slot runs **+4.08% (22/22 days) at 0 slots to -2.44% (0/22)
  at +1** - 6.5pp, five times the entire 125 bps round trip. No exit and no sub-population
  recovers it (0 of 80 decile cells positive at +1 slot), though the feature *ordering*
  survives the delay intact. Its entry signal is **absorption**: a token with proven
  distributed participation taking real selling and holding, where `bshare = 1.00` - a pop
  drawing no selling - is the worst cell, and in-slot rank 4 outperforms rank 1.
  [`../../history/2026-08-20-wallet-fbvx-intra-slot-absorption.md`](../../history/2026-08-20-wallet-fbvx-intra-slot-absorption.md)
- **Rank cells in money, never in percent, and price the search that found them.** At the
  money-optimal size `B* = e*vsol/4` the SOL per trade is `e^2*vsol/8 - 2F` - **quadratic in
  the edge** - so a fixed-size comparison understates every high-edge cell. Re-running an
  entire 17,744-cell search on 40 within-day outcome permutations prices the procedure: the
  best money cell scores z=10.22 against the null of best-cells, but the best **per-trade
  edge** cell scores only z=5.43 because a search this size extracts **12.10% per trade from
  pure noise**. Round 9's +12.91% stack sits at that ceiling (round 10).
- **"Positive in IS, OOS and forward" is not evidence.** 8.6% of cells pass it on real
  outcomes and 7.3% +/- 3.1% pass it on shuffled ones - **z = 0.41**. Three-window agreement
  is what a three-way split of a fat-tailed venue produces by construction. It has been
  quoted as support since round 7 and it supports nothing (round 10).
- **Searching for the best combination loses to not searching.** Trained on every prior day
  and read on the next, across six held-out days: re-deriving the max-money cell daily books
  **-0.23 SOL/day (2/6 days)** and the max-edge cell **+1.21 (5/6)**, while the **fixed**
  rule books **+3.83 (5/6, z=9.49)** and the **fixed** `rule & vsol>=40` books **+2.49
  (5/6, z=8.25)**. Selecting on eight days and reading the two forward days is worse still -
  every procedure lands at -3.03 to -3.17 SOL/day, and the greedy OR-portfolio reaches
  **-9.80 at z = -3.19, significantly worse than chance**. Hard in-sample money selection
  picks the cells that break. Prefer a mechanism-chosen cell to a search-chosen one
  (round 10).
- **The in-sample-optimal position size does not transfer; size at 1-1.5% of pool.** In
  selection the deep cells peak at 2.5-3%; forward they peak at 1-1.5%, and a cell sized at
  its own measured optimum flips sign (`rule & vsol>=40 & nb30<=3`: +5.62 SOL/day in
  selection at 3%, **-0.29 forward at 3%**, +1.27 forward at 1%). Sizing on a fitted edge
  squares the fitting error (round 10).
- **The bare rule's money is one day; the depth cell's is not.** At 1% of pool over ten days
  the rule books 5.09 SOL/day, of which **08-17 alone is 44%**; on held-out days its 4.46
  falls to **0.90** once its best day is dropped, its top 1% of trades is **108% of the
  book**, and **67% of its trades lose**. `rule & vsol>=40` books 2.34 held-out falling only
  to 1.73, is positive **6 of 6 held-out days and 9 of 10 overall**, has a 0.89 SOL ten-day
  drawdown against 2.78, and is the first selection in the program where **a majority of
  trades win** (33% losers, top 1% = 43% of book). Round 9's `nb30<=3` costs money at fixed
  size (1.86 against 2.34) and buys risk instead - worst day -0.10 against -0.89 - so it is
  a **risk knob, not an edge** (round 10).
- **`a5_uwshare` is 99.2% ties** and carries no information where it is applied; the
  distribution gate kills it outright. `f_selldecel` and `f_buyaccel` are 92% and 88% NULL on
  every day (round 10).
- **`tokens.is_cashback_enabled` off is a free exclusion.** Worth +4.8pp on `net60` (-5.52%
  against -10.32%), positive on 8 of 8 days and in every age band, at 46.8% prevalence. It is a
  pre-existing binary flag, so no threshold is mined. Apply it to any candidate. It is a
  launch-mode exclusion, not a signal.
- **The early "screen against MFE, not terminal return" frame is superseded.** Flow does
  separate against MFE with the opposite sign to terminal return, but round 6e measures where
  that MFE lands: of 47pp of mean path MFE only 5.3pp occurs before the exit. MFE is a
  volatility measure here, not a reachable target.
- Helius spend is capped and needs explicit approval before any call.

## The open queue

The three named ideas are built and measured together as the **D->L->I chain** - sell
deceleration into a dip, then silence, then a single-slot buy impulse - in
[round 4](../../history/2026-08-19-signal-round-4-lull-impulse-chain.md). The mechanism is
confirmed: **+7.70pp against a same-token matched control**, holding in both time directions,
and under the armed trail it produces the target shape at last (median net +5.67%, win 59.7%).
The rule is refuted: every configuration lands between -0.4% and +0.9% net with 3 to 5 of 8
days positive, about 1pp of gross short - the same place `63ot` sits at -0.08%.

Round 4 then spent its second half attacking the un-armed third at entry, and closed that
space: see the same record. **Both items the round opened are now shut** - the lull-length
exception does not survive a larger sample, and no entry feature moves net.

What is open now:

1. **Fund `rule & vsol>=40` at 1% of pool and let it run forward.** It is the only object in
   the program that survives a walk-forward without being re-fitted, and the binding limit on
   it is now **tape, not analysis**: 100 trades/day, 6 of 6 held-out days positive, 0.89 SOL
   ten-day drawdown. Nothing in ten days of data can raise the confidence further - only more
   days can. Promote it through the SQL engine first (the round-10 model is static and reads
   ~20% optimistic), then run it in paper against the live feed.
2. **Re-derive the exit on the deep selection.** Unchanged and still the largest unclaimed
   gain: it runs the shallow rule's exit (arm 8 / trail 4 / TP 15 / timeout 300), tuned on a
   34%-arm population, while this one arms 76-83% and its un-armed branch loses about 49%.
   A stop is not the answer at any width from 10% to 35%. The space is arm height, trail
   width, and the round-6 first-gap exit. Tune it in **money at 1% sizing**, not in percent.
3. **Rebuild `a_deficit`, `b_wall`, `b_uwz` on 08-19 onward.** They exist only for 08-11..18
   and derive from `iv.wide`/`iv.f3`, so they have never been forward-tested. They are the
   only predicates found that add money to the rule without cutting its trade count
   (`rule & nb30<=5 & adef>0.5` books 7.46 SOL/day against 6.69 on the sealed days; `wall=0`
   lifts the deep cell 13.10 -> 15.31 per trade while keeping 80% of its trades). Round 10's
   own result is that in-sample numbers of this kind do not transfer, so treat these as
   hypotheses to be killed forward, not as findings.
4. **More days.** Every conclusion here rests on ten, of which two are forward and one covers
   only 00-14 UTC. Keep putting each new day through the same verified engine.
5. **Where the depth threshold actually sits.** `vsol >= 40` is confirmed as a **sign flip**,
   not an optimum. Whether it drifts with venue conditions, and whether depth should be a
   band rather than a floor, is unmeasured - but note that `vsol>=35` is within noise of it
   on every held-out measure, so this is a small prize.
6. **The sell side beyond deceleration.** `a_deficit` remains the sharpest discriminator ever
   built and the sell side the largest blind spot. Seller-side ix identity (which client
   shape dumps, whether the launch crew's own pattern is exiting) is still unread - the churn
   result caps its use at same-token, same-day state, never a learned cross-token book, and
   buy and sell legs normally carry different ix, so a sell matching the campaign's buy
   structure is the tool itself leaving.
7. **Ix-template gate is a discriminator, not a fill.**
   [ix-template-gate.md](ix-template-gate.md): Axiom/Photon `CU+ATA` after quiet on the
   cashback-off / init-buy 0.2–1 door pays at zero-lag (clock-20 +7.75%, first-gap +8.92%,
   12/12) and does not at 95 ms on both legs (first-gap +1.14% 6/12, clock −1% OOS).
   Re-entry at 95 ms is negative every day. Do not fund it; do not re-price the 20 s mark
   as a rule.
8. **Gap-then-burst kinds, then door.**
   [ix-burst-kinds.md](ix-burst-kinds.md): after a 5-slot buy-gap (not the mint's first
   slot), two families precede his fire — same working template / several wallets / family
   SOL in [0.9, 4), and mixed templates / several wallets in the same tot band. One-wallet
   bursts do not. Create templates are not in this event.
   [ix-door.md](ix-door.md): his arm list is ATA on create + init ≥ 0.2 SOL + create-slot
   buy ≥ 0.5 SOL (87.7% of him, 7.7× vs fail). Cashback-off / init 0.2–1 is 15.4% of him
   and is not the door.
   [ix-perm.md](ix-perm.md): inside those bursts, `vsol < 46` is required (same-template
   ≥ 46 is 0% response). Age < 180 s tightens; 10 s gross, trail, and net/gross do not
   add on top of the 5-slot gap.
   [ix-machine-money.md](ix-machine-money.md): on the full tape, same-work first-gap at
   0 ms is +7.69% / 12/12 / median +3.23% with hold 0.2 s (the rest of the crossing
   slot). Clock-20 at 0 ms is negative. First-gap at 95 ms is −0.98% **0/12**. Do not
   fund it; the event is real and the fill is a race.
9. **Burst wallets first-on-this-mint.**
   [ix-new-wallets.md](ix-new-wallets.md): on the same gap-then-burst events, wallets
   with no prior buy on that mint (`all_new`) are the live cell; all-repeat is ~0%
   response in every kind, age band, and vsol band. Not the ATA flag (ATA+repeat is
   dead; no-ATA+new still lives) and not `c_fresh1h`. Mixed still precedes fires;
   all-repeat is the exclusion. Solo's 36% share is almost all `all_new`. Thermometer
   only - the earlier completing print (solo `all_new` = print 1) is priced in
   the same file: `solo_new_work` first-gap at 0 ms is +3.36% / median +1.84% /
   12/12 / hold 0.1 s, and at 95 ms is −1.35% **0/12**. Clock-20 at 0 ms is
   negative. Repeat-only controls stay red. Do not fund it; print 1 of a 0.9–4
   SOL buy is still a race.
10. **Kind-gap is the old quiet.**
    [ix-kind-gap.md](ix-kind-gap.md): silence of first-on-mint working-template
    prints while other buys continue is **weaker** than all-buy quiet (1.62% vs
    2.94% resp) and below the unfiltered real-print base (1.80%). Fake volume in
    the gap is dilution, not a second family. Do not price it.
11. **Old-on-chain vs born-this-mint is not a conjunct.**
    [ix-old-wallets.md](ix-old-wallets.md): first-on-mint wallets already
    predate the mint (pre_mint ≈ old; hoppers are rare). Named `all_new`
    families do not split (multi 10.01 vs 10.40, same 5.78 vs 5.77); the
    labeled-born mass is a tape hole, and strict same-slot born is n=27 on
    multi. Solo old lifts (3.18 vs 0.92 strict born) and is still the
    unfundable solo book. Do not price it.
12. **Solo is a turn; the crowd is not. The bounce does not last past 95 ms.**
    [ix-solo-turn.md](ix-solo-turn.md): inside first-on-mint + working +
    tot [0.9, 4) + `vsol < 46`, a quiet-resume solo with `trail >= 15`
    lifts the thermometer (7.22% vs 4.81% without). Named bursts still
    need no dip. Full-tape first-gap at 0 ms is +3.43% / 12/12 (same body
    as unfiltered `solo_new_work`); at 95 ms it is **−1.03% 0/12**. The
    thermometer-best cells (trail 30–60 + gap sells; trail 15–30, no gap
    sells) stay 0/12 at 95 ms. Clock-20 at 0 ms is median-negative. Do
    not fund it; do not walk another subset of this print.
13. **The combined machine is the same race.**
    [ix-combined-machine.md](ix-combined-machine.md): door ∧ 5-slot gap ∧
    `vsol < 46` ∧ not-all-repeat ∧ working completing print ∧ (crowd **or**
    turn), with tight consecutive-`tx_index` packs marked unfillable.
    Fillable (`separated` ∨ `one` ∨ `mixed_gap`) first-gap at 0 ms is
    +7.44% / 12/12 / median +3.14% / hold 0.2 s; at 95 ms it is **−0.98%
    0/12**. Re-entry at 95 ms is −0.84% **0/12**. `separated` alone is
    −1.05% 0/12 at 95 ms. `bundle` at 0 ms is the fiction (+8.22%). Do
    not fund it; do not walk another subset of this completing print.
14. **Early fire and gap duration do not leave remainder past 95 ms.**
    [ix-early-gap.md](ix-early-gap.md): first working new print after
    the buy-gap (not the 0.9 cross), gap length as a band. 2–4 slots is
    weaker; 10–19 is the thermometer peak on [0.3, 0.9). Starter
    first-gap at 0 ms is +5.00% / 12/12 / median +0.72% / hold 0.1 s;
    at 95 ms **−1.23% 0/12**. Trail and clock-4 lose median at 0 ms and
    stay 0/12 at 95 ms. Lookahead `oracle_sep` is −1.04% **0/12** at
    95 ms: 95 ms after print 1 fills on the rest of the same-slot crowd.
    Do not fund it; do not walk another same-slot early print of this burst.
15. **This trigger has no harvest after-move on the tokens it selects.**
    [ix-harvest-path.md](ix-harvest-path.md): after the fillable
    combined-machine fill at 95 ms, gross median is 0 at 1–4 s and
    **−8.43% at 20 s** (8dtx's hold). 86% dump; 14% second-wave (lookahead).
    Clock-20 is **−7.28% 0/12**. Stay-on-wave / leave-on-dump
    (`harvest_clock`) is **−2.36% 0/12**. His-mint clock-20 at the same
    fill is +6.15% / 12/12 / median +0.19% — that list is lookahead, not
    a live gate. Do not fund a harvest exit on this event.

**Feature-combination search is retired as a queue item.** Round 10 ran it exhaustively -
17,744 conjunctions plus OR-portfolios over 55 predicates - and the answer is that no
selection procedure built on this feature set beats the fixed two-condition rule out of
sample. Adding a search stage subtracts money. Re-open it only with genuinely new features,
never with new combinations of these.

Cost is retired as a queue item: the fee is fixed, the tip is a rounding error at the right
size, and the sizing correction is measured. Forward-testing entry depth is retired too -
round 9 ran it on 08-20 and it held. What both left behind is item 1.

The exit derivation item is retired: round 6 closes it (see the settled list - the first-gap
exit is the dynamic exit, and seven families tie or lose to it). Exit parameters still do not
transfer between selections - the fresh-wallet rule keeps arm 8 / trail 4 until re-measured
under the gap exit on its own selection.

## Where the data is

Schema `iv` in `hunter_bot` holds the scratch tables: `sp` (per-slot buy/sell aggregates), `dp`
(decision points), `a5`, `f3`, and `wide` - the 45,032-row / 23,134-mint candidate pool used by
the current rule. Round 4 adds `ev` (slot-resolution primitives, one row per active
`(mint, slot)`), `en2`/`out` (superset firings and their per-entry trail outcomes) and `mc`
(matched-control pairs). Round 6 adds `pd` (ix-pattern dictionary), `tp` (8.8M per-buy pattern
ids), `eb2` (per-entry modal breaker), `rf` (per-entry fast-exit outcomes, all 94,260), and
`sc` (the 13,808 strict-chain one-per-token pool). Round 6b adds `wb` (strict-chain entries
joined to breaker pattern ids), `wl` (the 13-pattern whitelist book) and `wl2` (modal-key
variant). Round 6c adds the suffix-9 fresh-day pipeline (`sp9`/`ms9`/`ev9`/`dp9`/`ch9`/
`en29`/`pd9`/`tp9`/`eb9`/`eb29`/`pz29`/`pv9`/`rf9`/`sc9`/`wb9`/`wl8`), covering 08-19 with an
08-18 warm-up; `dp9` is full-universe (the final `dp` build has no `msamp` filter) and its
build SQL lives in the session scratchpad (`fresh*.sql`). Round 6d adds `oc`/`oc2` (one row
per strict-chain token across all 9 days with occurrence index and resolved prior outcomes
per first-buy/modal pattern). All of it is safe to drop once the
round's conclusions are recorded. Round 6e adds `jt` (105,263 superset entries across all 9
days with chain features, ix identity features, path MFE and realized net), `jm`/`jm9` (path
MFE), `tpl` (TP-ladder triggers) and `jf`/`jmask`/`jres` (the exhaustive joint search), plus
the decode helper `iv.dec(int)`. Schema `cs` and `iv` together hold roughly
20 GB of scratch and are safe to drop once a round closes.

Round 7 adds the fresh-day fresh-wallet pipeline (`p9`/`es9`/`cf9`/`wide9`, pool-definition
exact against `wide`), the verified exit engine (`q8`/`o8` on the 8 days, `q9`/`o9`/`fs9` on
the fresh day - `o8` reproduces the published +3.67/+3.32/+4.21) and the venue-state tables
(`vm` per minute, `vs` trailing five-minute state, `va` chain joined to it, `vb` fresh-wallet
pool joined to it). `iv.wfs` is **modified, not scratch**: 39,139 wallets first seen after
2026-08-18 21:03 are backfilled, which every consumer needs since first-seen is a minimum.

`dp.e_px` is the **next buy-active slot's median price**, verified on 100.0% of its 2,854,440
rows - the measured p50 latency fill, so entry latency is already charged in anything built
on `dp`.

The one rule that survives the gates is [fresh-wallet-entry-rule.md](fresh-wallet-entry-rule.md).
Its sample is 8 days, which is the binding limit on everything in it.

Round 8 adds `q8x`/`q9x` (the round-7 paths rebuilt carrying exit-slot depth), `rsel`/`psel`
and `rsel9`/`psel9` (rule and pool collapsed to one trade per token), `tro`/`tro9` (per-trade
outcomes at 1%-of-pool sizing with rule membership), `dv` (decision-slot vs entry-slot depth),
`armz` (the arm flag per entry), `dm` (the dominant-buy matched experiment), `stp` (the stop
sweep on the deep cell), and the sweep helpers `szc`/`fz`/`sz8r`/`sz8p`/`sx8r`/`tr1`/`fee1`.
All are scratch and safe to drop.
