# 8dtx entry event - ix structures and templates

Phase 3 of [market-model-and-workflow.md](market-model-and-workflow.md), under
[trader-study-contract.md](trader-study-contract.md). **No set ships from this pass.**
The event unit is corrected and the tape signature is confirmed; the structures that survive
it are racer-class, so naming them as triggers would repeat the wave-node error.

Wallet `8dtx2tr4TuJsYpri2suggFu1pg3DVjFLBBVmhtDy1MEF` (`wallet_dict.id = 2720`).

## Files

[8dtx-event-templates.json](8dtx-event-templates.json) (57 grain ids) and
[8dtx-event-structures.json](8dtx-event-structures.json) (155 ordered `ix_labels` sequences)
carry the lists alone; [8dtx-event-detail.json](8dtx-event-detail.json) carries the numbers.
All three are **ordered by how many of his buys follow the structure**, most first.

**There is no rate filter.** The count is the finding: the more often his entry lands after a
structure, the more often that structure is the event. Rates are reported beside the count,
never used to keep or drop a row - a rate divides the count by how often the machine pushes,
so it reads a busy volume machine as weak. `Unknown (6Vo3245...)|CU|ATA|F` precedes **476** of
his buys, more than any other structure on the tape, at 0.217% per push because it re-pushes
the same token 9.4 times; ranking on that rate hides it behind structures carrying a third as
many of his entries.

Every number counts **silence-breaking pushes only** - a push is one tool's buys landing in
one slot on one token after 5+ empty buy-slots, any size, any push number. Per row: tokens,
pushes, pushes per token, SOL per push, buys per push, his follows, and the same broken out by
push size. `pct_per_token` is the fair rate; the all-tool baseline is 1.69%.

## Frame

| term | value |
| --- | --- |
| window | 2026-08-30 17:48:14 UTC (decoder cutover) .. 09-04 12:00 |
| universe | full tape - 5,548,132 buy txs, 100,482 mints, 15,720 exact structures, 953 grains |
| his anchors | 1,852 first-buy-per-mint, from 2,494 buy txs |
| response | his entry lands in `S..S+1` **and** strictly after the event's last print |
| candidate | every silence-breaking push; size and push number are reported, never filtered |
| floors | `he_followed >= 20` pushes, rate `>= 1.25x` the all-tool rate |

His own legs stay out of the candidate pool. Everything orders by `(slot, tx_index)`;
`block_time` is the ingest clock and bounds scans only.

## The event unit

**An event is one tool firing N times in one slot on one mint.** Count and size are measured
over that in-slot run only, so nothing after the decision point is counted. The fire point is
the run's last print in chain order. Racers and other tools may land between the fire point
and his buy without disqualifying it - on average **2.37 prints sit ahead of him**, and 57%
of his entries have two or more.

**Slot shape is not a cell.** Filtering to `ntx = 1` filters the SLOT, not the event: an event
that draws a swarm into its own slot leaves the cell, so that spelling excludes exactly the
events strong enough to provoke a crowd and reaches only 47.8% of his entries. The same-tool
unit reaches 97.7%.

## The signature - gap and count together

Response rate by silence before the run and same-tool count inside it, over 4,380,685 events:

| same-tool N | gap 0 | gap 1-4 | gap 5-20 | gap 21+ |
| --- | --- | --- | --- | --- |
| 1 | 0.043 | 0.064 | 0.105 | 0.142 |
| 2 | 0.045 | 0.109 | 0.211 | **0.416** |
| 3 | 0.054 | 0.175 | 0.226 | **0.472** |
| 4+ | 0.040 | 0.100 | 0.226 | 0.372 |

Neither term works alone: count over the whole tape runs 0.94 / 1.28 / 1.58 / 1.23 / 0.96 and
turns over by N=7, gap alone at N=1 reaches only 0.142. At a long silence, 1 -> 3 same-tool
buys **triples** the rate. A mint's first observed buy slot reads 0.000 at every count - he
never enters on it.

## Ranked inside the cell (gap >= 5, N >= 2)

51,696 grain candidates, baseline 0.358%; 48,997 exact, baseline 0.365%.

| grain | mints | avg N | avg SOL | lift | lag1 | days |
| --- | --- | --- | --- | --- | --- | --- |
| `Lighthouse\|CU\|N\|S\|F` | 656 | 2.1 | 1.54 | 3.41 | 1.06 | 4/4 |
| `Lighthouse\|CU\|N\|S` | 1,595 | 2.2 | 1.51 | 2.45 | 3.07 | 4/4 |
| `Terminal\|CU\|F` | 4,503 | 2.8 | 2.21 | 1.92 | 0.93 | 4/4 |
| `Pump.Fun\|CU\|N\|S\|F` | 3,269 | 2.6 | 2.81 | 1.62 | 1.71 | 3/4 |
| `Axiom Trade\|CU\|ATA\|F` | 11,827 | 3.1 | 2.28 | 1.30 | 1.24 | 2/4 |

Exact rows track the same order: `aa9e05b3` (`Lighthouse|CU|N|S|F`) 4.17 at 4/4,
`cff577c5` (`Lighthouse|CU|N|S`) 2.42 at 4/4, `e0191851` (`Terminal|CU|F`) 1.84 at 4/4.

## Why no set ships

`|N|S` is `AdvanceNonceAccount` **and** `CreateAccountWithSeed` - the seed-builder cohort the
tool census identifies as the racer layer. A racer bundle after a silence marks that somebody
else's rule has already fired; it is confirmation that an event happened, never the event. A
set keyed on it is behind the real trigger by construction, which is the shape that closed the
wave node. `Terminal|CU|F` is the one non-racer entry at 4/4, and its `lag1` is 0.93 - his
response to it is same-slot, so a seat at p50 +1 slot cannot read it either.

**The open step is upstream:** run this same unit against the racer bundle as the response
instead of against him. What precedes a `Lighthouse|CU|N|S` burst is a candidate event; what
precedes him is, on this evidence, mostly the burst.

## What the solo-slot pass got wrong

A first pass cells on `ntx = 1` and ships four exact structures
(`Pump.Fun: BuyExactQuoteInV2` x2, `6Vo3245...: BondingCurveV3` x2) plus three grains at
4/4 days and lifts of 2.0 - 8.2. On the corrected unit those four price **0.00 / 0.00 / 0.00 /
0.61**, two of them on fewer than 200 mints. The lift was real for what it measured - which
structure precedes him **when it is the only thing in its slot** - and that is not the entry
event. The sets are withdrawn from `ix_pattern_sets` and those files are deleted.

Two findings from that pass stand on their own, because neither depends on the cell:

- **Chain order is load-bearing.** 1,409 of 5,786 prints inside the `S..S+1` window (24%) land
  *behind* him in his own slot. Counting them inflates every rank.
- **The template grain drops the instruction name.** `program_owned` truncates the head label
  at the first `:`, so `Pump.Fun: BuyExactQuoteInV2` (lift 3.16, 4/4) and
  `Pump.Fun: BuyExactSolIn` (0.52, 0/4) share one grain. Any Pump.Fun-direct machine is below
  the grain's resolution.

## His own build

Two exact structures, one machine: `AdvanceNonceAccount, CU price, CU limit, ATA idem,
Pump.Fun: Buy` with (1,940) and without (554) a trailing `System Program: Transfer`. Grain
`Pump.Fun|CU|ATA|N|F` and `Pump.Fun|CU|ATA|N`. Median clip ~0.75 SOL. He buys Pump.Fun direct
off a durable nonce, and sets CU price before CU limit.

## What this does not claim

- No money. `he1` reads his send; nothing here is priced at 95 ms.
- Nothing outside 2026-08-30 17:48 .. 09-04. Instruction names differ before that instant, so
  an earlier period is a separate derivation, never a merge.
- Four full days is the stability evidence; it is not a five-week hold.

## Reproduce

Schema `e8` on the workstation PG, LOGGED: `e8.hisbuy` / `e8.anchor` -> `e8.tx` (full-tape
buys, his legs out) -> `e8.lab` (structure dictionary, grain, program) -> `e8.slot` ->
`e8.sc` (gap, prior activity, response) -> `e8.ev` (per print) -> `e8.rung` / `e8.runx`
(same-tool in-slot events, both vocabularies) -> `e8.p` / `e8.px` (silence-breaking pushes
with his response and push number). `e8.eg` / `e8.ex` are the withdrawn first-push-only cell,
kept only to reproduce what it cost. `e8.grain` mirrors
`hunter_engine::metrics::template_grain::grain`.
