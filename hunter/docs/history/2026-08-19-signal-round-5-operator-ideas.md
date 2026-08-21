# Round 5 - the operator's two ideas: a large exit gain, and entry selection stays shut

2026-08-19, same day as [round 4](2026-08-19-signal-round-4-lull-impulse-chain.md) and built on
its tables. Two candidates supplied by the operator from manual observation, plus a re-scoring
of round 4's seller-inventory feature against the loss side rather than against direction.

One produced the largest exit improvement ever measured in this program. Neither produced a
profitable rule, and a 256-cell combination search over everything found in five rounds
returned nothing.

## Idea 1 - exit on silence, then a sell

**As stated:** after entry, if roughly three seconds pass with no trade and then a sell prints,
the move is over - exit.

**It works on the group nothing else reached.** The armed trail leaves 35% of positions never
reaching the arm threshold, and they lose 36% because no exit condition applies to them. An
exit on silence cuts that group to **-10.78%**, and total net on the D->L->I selection improves
from **-4.20% to -1.52%** - 2.7pp, the largest exit gain in the program.

**But the stated mechanism is not what pays.** Three controls on identical entries and fills:

| exit | median hold | net | loss group |
| --- | --- | --- | --- |
| armed trail, arm 8 / trail 4 / 300 s | 38.8 s | -4.20% | -36.04% |
| first print of any kind | 1.4 s | -2.03% | -4.29% |
| first sell print, no silence required | 3.1 s | -1.76% | -7.81% |
| **first print after a gap, no sell required** | 3.8 s | **-1.49%** | -8.59% |
| silence >= 2 slots then a sell | 5.6 s | -1.52% | -10.78% |
| silence >= 2 slots then a sell-dominant slot | 6.0 s | -1.48% | -11.05% |

Requiring a sell adds nothing over requiring a gap, and requiring a gap adds nothing over
neither. What pays is **hold length**, and it is not monotone: exiting on the very first print
(1.4 s) is worse than waiting for the first gap (3.8 s). The optimum sits at a median hold of
3 to 6 seconds against the incumbent's 38.8.

A state-dependent version - silence governs an un-armed position, the trail takes over once
armed - is **worse** (-1.87%) than applying the fast exit throughout, because an armed position
that goes quiet is better sold at once than trailed to a gapped fill.

Silence length behaves the way the loss-cutting story predicts and the profit story does not:
tightening from 25 slots to 2 takes the loss group from -31.97% to -10.78% while total net
improves monotonically, but the winners fall from +12.76% to +3.41% at the same time. The
exit is trading winner size for loss size at close to a fair rate.

**Verdict: keep the fast exit, discard the mechanism.** A trail on this venue waits for price
to fall 4% from its peak, and the fall happens inside a print gap, so the fill lands well past
the trigger. Any exit that leaves in a few seconds sidesteps that. The specific silence and
sell conditions are not doing the work.

## Idea 2 - holder concentration

**As stated:** top holder under 10% and top ten under 30% marks a safer token.

`iv.hc` computes top-1 share, top-10 share, a concentration index and holder count at the entry
slot from the running per-wallet position in `iv.wc`. On this population the stated thresholds
sit far out in the tail - median top-1 is 15.9%, median top-10 is 82.6%, median holder count
27 - so the axis is swept rather than tested at those values.

**Concentration is a strong, clean, monotone lever on the loss side, with the opposite sign.**

| top-1 holder share | tokens | loss when it fails |
| --- | --- | --- |
| < 8% | 2,847 | **-54.57%** |
| 8-12% | 2,371 | -50.38% |
| 12-18% | 2,430 | -42.58% |
| 18-28% | 2,314 | -34.07% |
| 28-45% | 1,864 | -26.28% |
| >= 45% | 1,942 | **-20.40%** |

Monotone across all six buckets, and the same shape on top-10 share and on holder count - more
dispersed ownership means a **larger** loss, not a smaller one. The reading that fits: a
concentrated token is small and quiet, with no crowd to stampede and little distance to fall. A
dispersed one has already run and has a crowd that can leave at once.

**The confound gate then takes most of it.** Top-10 share correlates **-0.774** with `vsol` and
holder count **+0.708** - these are pool-size proxies, exactly the shape that "passes every
profitability test and generalises to nothing". Inside `vsol` bands the effect survives only in
the 30-45 band (loss -41.53% / -33.76% / -21.15% across low, mid and high concentration) and is
flat above it (-54.49 / -57.12 / -54.98 in the 45-65 band).

Net is negative in every concentration bucket under both exits.

## Idea 3 - seller inventory, re-scored against the loss

Round 4 scored `e_left` against direction and killed it. Against loss magnitude it is real and
in the intuitive direction: sellers already flat leave a **-27.50%** loss group, sellers still
holding 25% or more leave **-47.67%**. It does not move net in any bucket, and it is 0.328
correlated with `vsol`.

## The combination test

The standing operator thesis is that these combine - that a narrow enough conjunction avoids
the losing tokens. Eight binary filters covering everything found in five rounds: top-1 share
>= 18%, seller inventory < 3%, lull >= 6 slots, dip <= -20%, impulse >= 4x, holder count 22-40,
cashback off, `vsol` 30-45. All **256 combinations**, scored under the best exit, one trade per
token.

| | |
| --- | --- |
| cells with at least 250 tokens | **187** |
| cells with positive net | **0** |
| best cell | -0.11% on 350 tokens, 2 of 8 days positive |

Not one profitable cell. Combining does not rescue it.

## Where this leaves the two sides

**The exit side moved a lot.** Best net on the D->L->I selection goes from -4.20% to -1.49%,
which is 2.7pp of gross recovered from fill mechanics rather than from any signal. Carry the
fast exit into every future test - the armed trail has been overstating the cost of a bad entry
in every measurement in rounds 1-4.

**The entry side is unchanged.** Concentration and seller inventory both move the loss side
strongly and monotonically, both are largely size proxies, and neither moves net. With round
4's result that no feature moves net through the direction channel, and this round's that none
moves it through the loss channel either, the entry-selection space is closed from both
directions on this trade shape.

The remaining gap is about 1.5pp against a 3.45% cost. Every shape measured here - the chain at
-1.49%, `63ot` at -0.08%, the inverted token search ceiling at +2.6% gross - lands within about
1pp of the fee, which continues to point at cost rather than signal as the binding constraint.

## Method notes worth keeping

- **Control a conditional exit against its unconditional form.** "Silence then a sell" reads as
  a mechanism and reduces to "hold for four seconds". Two controls separated them in one query.
- **Concentration measured from `trades` is defeated by bundled launches** - one operator across
  twenty wallets reads as dispersed. It fails in the unsafe direction for a safety filter. Cross
  it with the `m_bundle` veteran roster before trusting it anywhere.
- Scratch: `iv.pv` forward paths with sell activity and slot gaps, `iv.hc` concentration,
  `iv.res` per-entry outcomes under both exits, `iv.cells` the 256-cell search.
