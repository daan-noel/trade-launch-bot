# Attention-arrival node: readers, event, seat, doors, exit

Phase 3-5 study of the attention-arrival node under
[_!___market-model-and-workflow.md](_!___market-model-and-workflow.md) and
[trader-study-contract.md](trader-study-contract.md). Two readers are the instruments
(`wallet_dict.id` 796 and 1416); the universe is every token created after the decoder
cutover, 2026-08-30 17:48 UTC .. 09-06 12:00, with both readers' own prints out of the
tape. Schema `aa` on the workstation PG (LOGGED). Every money number is net of 125 bps a
leg, own impact on `vsol`, and the fixed leg cost, at a 0.2 SOL clip.

The story: *on a curve with headroom, when several independent machines buy in the same
slot, attention has arrived and budget is still being spent; the dev has not begun to
extract; flow continues for seconds to minutes and fails fast when it fails.*

---

## 1. The instruments

| | reader 796 | reader 1416 |
| --- | --- | --- |
| build | terminal-style, no nonce or seed | pre-signed: `AdvanceNonce` + `CreateAccountWithSeed` |
| trades / day, clip | ~180, 0.19-0.29 SOL | ~350, 0.72 SOL fixed |
| one buy, one sell | 97.6 % | 98.0 % |
| round trips per token | 1.00 | 1.14 |
| lands in the same slot as the previous print | 93.0 % | 87.3 % |
| prints ahead of him in that slot, median | 3 | 2 |
| the slot before his is empty | 48.7 % | 53.1 % |
| entry age, vsol (medians) | 45 s, 45.3 | 46 s, 43.8 |
| entry at the 30 s high | 53.8 % | 51.7 % |
| tape in the prior 30 s: prints, wallets, builds | 27, 22, 15 | 28, 22, 15 |
| hold, median | 46 s | 54 s |
| return on spend, days positive | +2.68 %, 59 % (19 of 32) | +2.21 %, 75 % (24 of 32) |

**Decision point.** Both land inside the burst slot, behind ~3 prints, and the prints of
that slot are spread over ~130 ms on the ingest clock (not one batch). Half the time the
slot before theirs is empty, so the event is inside their own slot and they react to its
first prints within ~100-150 ms. They are same-slot reactors, which is the seat we have.

**Exit shape.** Winners peak at 64-77 % of the hold and are sold 14-21 % below the peak
after 78-112 s; losers are cut at 30-40 s, 18-25 % down, with the peak 2-3 s after entry.
Every SOL of both books is in holds longer than 60 s; holds under 30 s lose heavily.

**Doors they walk through.** Against all tokens created in the window, both over-select
two creation structures 8-12x (`5ix:ix#6f`, `3ix:BuyV2`), under-select no-dev-buy and
sub-0.2 SOL launches at 0.08-0.22x, and touch the ~49 SOL bundler launch (`3ix:Buy`) at
0.04-0.5x its base rate.

---

## 2. The event and its thermometer

Unit: the k-th distinct buyer inside one slot; fire = that print; count, size and racer
presence measured over the run up to it. 1.27 M fires at k = 2 on 72,004 tokens.

Response = a reader lands in the fire's slot behind the fire print, or in the next slot.

| term | response, per mille (796 / 1416) |
| --- | --- |
| k = 1 / 2 / 3 | 0.37 / 0.81 / 1.22 ; 0.41 / 0.81 / 1.24 |
| at the 30 s high vs a >10 % dip (k = 2) | 1.07 vs 0.75 ; 1.02 vs 0.76 |
| `vsol` 35-40 / 46-55 / 55-70 / 70+ | 1.78 / 1.20 / 0.17 / 0.00 ; 1.91 / 0.94 / 0.13 / 0.00 |
| a racer already in the slot: 0 / 1 / 2 | 0.65 / 1.28 / 1.44 ; 0.63 / 1.26 / 1.78 |
| the two buyers moved the pool >= 5 % | 2.55 ; 2.97 |
| first buyer a terminal human, second a racer | 2.43 ; 3.48 |
| distinct buyers in the prior 60 s: 3-5 / 25+ | 1.87 / 0.57 ; 2.15 / 0.54 |
| slots since the previous two-buyer slot: 1-2 / 76+ | 0.55 / 2.23 ; 0.53 / 2.91 |
| first two-buyer slot of the token / 31st+ | 0.87 / 0.40 ; 0.47 / 0.21 |

Neither reader keys on once-per-token transitions (N-th distinct buyer, first ATH after
30 s, `vsol` thresholds): 2-3 per mille in S..S+1, and most of those transitions happen
inside the launch scramble, 20-40 s before the readers act.

---

## 3. The seat, measured - and the ceiling, corrected

The ingest-clock model `lag_115` charges every print that reaches the feed within 115 ms
of the trigger. On 506 real fills the leader sequences us ahead of many of those: median
one print behind the trigger, none at all in 50 % of fills. The density table and the
rule are in [fill-and-cost-models.md](fill-and-cost-models.md).

On the readers' own picks the seats disagree by the whole result, and the disagreement
is the swarm's own impact (section 4):

| set (k = 2, 120 s clock) | n | d = 0 | density-weighted | `lag_115` | slot end | behind the reader's print |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| whole pool | 1,268,395 | -8.2 % | -9.2 % | -10.3 % | -10.3 % | - |
| reader-responded fires | 2,015 | +14.3 % | +7.6 % | +2.4 % | +1.6 % | **+0.7 %** (4 of 8 days) |

The responded set is defined by a reader landing behind the fire, and the reader is one
of a swarm of bots that lands in the same slot. A fill at d = 0..2 sits ahead of most of
that swarm and rides its impact into the exit: the seat cost from the fire print to
behind the reader's print is 13.6 %. Priced behind the reader, the 120 s clock is flat.
What survives behind the swarm is the readers' exit: the wide armed trail (arm +15 %,
trail 25 %, cap 600 s) reads **+6.7 % a trade, 7 of 8 days** on the same 2,015 fires at
the slot-end fill. Their money is the tail of their own picks, harvested behind the
swarm; a clock cannot reach it.

## 4. What the readers are, and what the swarm fires on

**A farm and a swarm.** Reader 796 buys in the same slot as wallets 797, 7756166, 1151
and 1152 in most of its entries (802 of 1,128 with 797 alone), the five in either order;
they are one operator's wallet farm firing together. Reader 1416 is independent of that
farm (35 co-buys, chance level) but not of the crowd: the wallets ahead of either reader
in the slot are the other same-slot bots (racer builds and other multi-wallet farms), 35
distinct wallets covering half of 1416's entries against 174 for a random buy, and no
single wallet above 3 %. Neither reader copies a short list. Both are members of a swarm
that fires on the same print within one slot.

**What raises the swarm's response.** Each term below multiplies the readers' response;
every one of them leaves money negative at the honest seat. Response is per mille, money
is the density seat with a 120 s clock, age 10-300 s, `vsol` < 55 unless stated.

| term | response (796 / 1416) | money |
| --- | --- | ---: |
| feed placement: token ranked 1st by prints in 60 s / 21st+ / off the feed | 0.30 / 0.20 ; 1.84 / 2.33 ; 1.17 / 0.77 | -8 % / -9 % / -6 % |
| prints in the prior 60 s: 0 / 1-2 / 6-10 / 51+ | 7.9 / 5.2 ; 4.5 / 7.1 ; 3.8 / 5.0 ; 0.6 / 0.6 | +4 % / -2 % / -4 % / -13 % |
| `vsol` 30-36 / 46-54 / 56+ | 3.4 / 3.7 ; 1.0 / 1.1 ; 0.0 / 0.0 | -2 % / -15 % / -14 % |
| first buyer's size < 0.05 / 0.5-1 / 1-2 SOL | 0.25 / 0.43 ; 3.1 / 4.5 ; 4.3 / 4.7 | -15 % / -10 % / -9 % |
| first buyer's prior record (08-30..09-02) for fires 09-03..: 15-74 buys and net > +3 SOL / unseen | 14.7 / 9.5 ; 0.4 / 1.1 | -7 % / -18 % |
| first buyer after real silence (k = 1, gap >= 10 s) | 0.2 / 0.3 | - |

The swarm reads a feed of wallets: a buy of real size by a wallet with a record, on a
young token below a hard `vsol` wall near 56, not at the top of the activity feed. The
first print after silence is not the trigger; the second independent buyer is. The
readers' feed placement is a tilt, not a definition: 55-60 % of their entries are on
tokens with 51+ prints in the prior 60 s, and their own books are positive in every band
except the very top of the feed.

**The selection is spellable and still loses.** A gradient-boosted classifier on the
decision-time terms above (trained 08-30..09-02, tested 09-03..09-06) reaches AUC 0.91;
its top 0.1 % slice carries 65x the base response. At the slot-end fill with a 120 s
clock that slice reads -10 %, the top 1 % -8 %, every test day negative.
With every exit shape on the top slices' own forward paths at the slot-end fill, the
best is a cause-based exit at -3 % (top 0.1 %, 0 of 4 days), the readers' wide trail
-5 % to -6 %; ahead of the swarm (d = 1) the same slices read +3 % to +12 %. The money
of this node is the position ahead of the swarm's impact, not the pick.

**k = 3 does not sharpen it.** The third distinct buyer in a slot (596,054 fires) has the
same shape: pool -10.3 %, responded +6.7 % at the density seat.

## 4b. Doors and permissions, ranked on money

Add-one / drop-one over pre-registered terms, ranked by total SOL at the density-weighted
seat, floor 50 fires a day. Three sentences came out; each is graded on the ladder.

| sentence | n | mints | per trade | days + | d = 0 | d = 2 | `lag_115` | verdict |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| **B** `5ix:ix#6f` launch client, creator not sold | 37,329 | 946 | +6.8 % | 2 of 2 | +7.5 % | +6.2 % | +5.7 % | seat-robust; the client existed for two days (09-01/02) and is gone |
| **C** `3ix:BuyV2` launch client, creator not sold, a racer in the slot, age < 300 s | 1,882 | 650 | +11.6 % | 4 of 5 | +14.0 % | +10.6 % | +9.6 % | seat-robust; no valid holdout yet (below) |
| **A** first two-buyer slot of a token, no racer, second buyer >= 0.1 SOL, not `5ix:BuyExactSolIn`, dev buy < 10 SOL, pre-tape not buy-heavy, age < 300 s | 38,151 | 38,151 | +1.3 % | 7 of 8 | +18.6 % | -6.2 % | -17.2 % | execution bet: the money is being first behind the trigger |

`5ix:ix#6f` is `Create_v2` followed by two calls into program `pfeeUxB6...` (a fee-share
launch flow); `3ix:BuyV2` is `Create_v2, ATA CreateIdempotent, BuyV2` with a ~2 SOL dev
buy in the same transaction. In C the creator-has-not-sold term carries the money
(without it +1.3 %), the racer term concentrates it (without it +5.9 %).

**Execution floor.** Re-running the add-one search with the rule that a step must keep
half its money at `lag_115` returns no positive sentence from the pre-registered terms;
sentence A fails the floor and every quiet-side or feed term loses alone.

**Holdout for C.** The same creation structure exists before the cutover with identical
labels (tokens 08-06 .. 08-30, thin tape): C reads -6.4 % there on 506 fires. That block
is not the same population: the client ran ~110 tokens a day at a 0.81 SOL dev buy, and
the readers entered under 1 % of its tokens; from 09-02 it runs 130-350 a day at 2.1 SOL
and the readers enter 5-9 %. A launch-client door carries its regime, so C's only valid
test is forward days, and a client door is re-screened weekly.

---

## 5. Exit

Exits resolved per print on the fill's own path, both legs charged, all four seats.

| set | best exit | per trade | days + | plain 120 s clock |
| --- | --- | ---: | ---: | ---: |
| reader-responded fires (2,015) | armed trail: arm +15 %, trail 25 %, cap 600 s | +12.9 % | 8 of 8, worst day +0.35 | +7.6 % |
| sentence C (1,880) | 180 s clock | +15.0 % | 4 of 6 | +11.6 % |

On the readers' selection the tail pays and the harvester law holds: the wide armed
trail beats every clock, every abandon clock and every cause-based exit, stays 8 of
8 days positive at every seat down to `lag_115`, and is the only shape still positive
behind the swarm (slot-end fill +6.7 %, 7 of 8 days; the 120 s clock +1.6 %). On the launch-client door the tail does
not exist and clocks win. The exit is a property of the selection, which is why it is
settled after the gates. Cause-based exits (first sell of size) lose on both sets.

---

## 6. What this settles and what it leaves open

- The node is a swarm, and joining it at our seat is flat: the readers' picks behind
  their own print read +0.7 % on a clock and +6.7 % with the wide trail. The earlier
  +7.6 % to +12.9 % ceiling priced our fill ahead of the swarm and is withdrawn.
- The swarm's trigger is a wallet feed (size, record, young token, `vsol` wall), and
  its selection is reproducible from tape terms (AUC 0.91) without reproducing its
  money. Selection is not the missing piece; the seat inside the swarm is.
- `lag_115` on the ingest clock is a floor on bursts; the position ladder is the grade;
  on a swarm event the honest fill is behind the swarm, not behind the trigger.
- Creation-structure doors are money terms, not descriptions - and they rot in days.
- Sentence C is withdrawn: `3ix:BuyV2` carries 2-4 % of the readers' trades and is a
  fitted corner. The launch-build door and the candidate rule that came out of it are in
  [machine-census.md](machine-census.md) sections 6-7.
- Lead, thin: a token that drew 10-79 buyers, fell 20-50 % from its high, went quiet
  (<= 5 prints in 60 s) and then shows two buyers in a slot reads +20 % on 643 fires at
  the density seat, +8 SOL at `lag_115`, concentrated in 09-01 and fading; not a rule.

## 7. Decomposition: is it the token or the moment?

"Positive on the readers' tokens, negative on the pool" has four candidate causes: the
token (selection), the readers' and the swarm's own buying, the exit, or information off
the tape. This test separates the first from the second. For every entry of 796, 1416 and
8dtx (2720): the same token at their moment (A), at random buy prints on the same token at
matched depth before their entry (B) and after it (C), and on a random pool token at
matched age and depth (D). Slot-end fill, 0.2 SOL, full costs.

| set | 796 | 1416 | 8dtx |
| --- | ---: | ---: | ---: |
| A their moment, 120 s clock | -6.0 % | -0.7 % | -1.7 % |
| B same token, before them, reader arrives inside the window | -3.2 / +1.1 % | **+13.7 / +13.4 %** | +8.2 / +2.9 % |
| B same, path truncated at the reader's arrival slot | -7.0 / -8.0 % | -2.3 / -4.7 % | -5.7 / -7.8 % |
| B same token, reader more than 120 s away | +2.7 / +3.8 % | -4.7 / -9.8 % | -4.6 / -4.7 % |
| C same token, after them | -10.5 % | -10.4 % | -11.3 % |
| D pool, matched age and depth | -11.9 % | -15.5 % | -10.5 % |

(B rows: lead 5-30 s / 30-120 s; third B row: 120-300 s / 300 s+.)

The whole profit on their tokens is the window that contains their arrival. Cut the path
one slot before they land and every "before" cell is negative; move more than two minutes
away from them and the token reads like the pool. After they land the token IS the pool.
**Selection is refuted as the cause. The flow on their tokens is the swarm's own buying,
and the profitable seat is ahead of it**, which is the seat T9 closes. Their books are
positive because they land inside the swarm ahead of part of it and harvest the tail
with a wide trail; at the slot-end fill their own moment is negative.

The exit does not change it. With their own wide trail (arm +15 %, trail 25 %, cap
600 s) at the slot-end fill: A reads +3.4 % for 1416 (6 of 8 days) and -2.1 % / -1.1 %
for 796 / 8dtx; B reads +17.7 % / +5.7 % / +5.6 % with the reader inside the window; C
and D are negative under every exit shape for every reader.

Consequences: a reader who is a swarm member cannot name a token-selection edge, only a
co-arrival edge; "profit on his tokens" is never evidence of selection until the path is
truncated at his arrival; and the instrument for the next node must be a machine that
MOVES the price with intent (a dev's campaign build, a volume machine), not one that
reacts to a print in 100 ms.

## 8. The event on machines

The event unit above counts wallets. Wallets are not actors: the farm behind reader 796
is five wallets on one build with one fee preset. Rebuilt on the census
([machine-census.md](machine-census.md)), the event is the k-th distinct MACHINE
(`build_core`) buying inside one slot, and **36.1 % of the wallet-based k = 2 fires were
one machine** (457,510 of 1,268,395).

Thermometer on the machine unit (age 10-300 s, `vsol` < 55; response per mille 796 / 1416):
k = 1 / 2 / 3: 0.69 / 1.71 / 2.27 ; 0.88 / 1.66 / 2.15. Fee as behavior: a fire print
paying 5x or more its own machine's median priority price draws 4.65 / 5.20 against
0.88 / 1.24 at the machine's normal price. A cohort-class machine in the run (farm,
bundler, creator): 2.66 / 4.20 at two. Role pairs: a terminal human followed by a farm
draws 7.1 / 9.0, the highest cell in the study.

Money on the machine event, open pool, slot-end fill, no reader in the money path:
938,789 fires on 68,453 tokens, -7.6 % / -10.2 % / -12.7 % at 60 / 120 / 300 s. Every
machine term is negative on every one of 8 days: role of either machine, every role
pair, fee ratio, cohort count, wallets so far, clip against the machine's own median.
The story cell (age 10-300 s, `vsol` < 55, mainstream door, creator not sold) reads
-8.1 % at 120 s with response 3.7 / 3.7 per mille; adding the fee-ratio term lifts
response to 9.5 / 9.8 and money to -8.7 %; the least negative refinement is a quiet
prior minute at -5.9 %.

**The second machine in a slot is the swarm.** Named on machines and priced behind it,
the burst event carries no money at any spelling. What it does carry is the readers'
response, which is why they were found on it. Section 9 tests the events that precede flow rather than react to it.

## 9. Events that precede flow, and the feed itself

Four events written on the census, none reacting to a burst, all priced at the slot-end
fill on the open pool, 0.2 SOL, full costs, 08-30 .. 09-06 (`scratchpad/ev.sql`,
`feedev.sql`; tables `aa.evf`, `aa.fef`):

| event | n | 120 s clock | 300 s clock | peak within 300 s | runner rate | best cell |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| A campaign machine's first print on the token (farm / bundler / volume swarm) | 27,856 | -7.0 % | -8.6 % | +46 % | 29 % | story + dip -3.2 % (318) |
| B creator's wallet buys again after 30 s of silence | 20,315 | -5.4 % | -7.9 % | +12 % | 6 % | size >= 1 SOL -2.4 % (631) |
| C terminal human commits >= 0.5 SOL to a quiet young token, first in slot | 29,542 | -9.6 % | -12.2 % | +43 % | 26 % | story + dip -3.3 % (873) |
| D the token enters the top 10 of the activity feed from outside the top 20 | 27,781 | -12.8 % | -15.7 % | +46 % | 26 % | story -7.4 % (2,300) |

Every permission cell, every exit shape (clocks, wide and abandoning trails, cause-based)
and every hour band is negative on every reader-free event, 0 of 8 days positive except
cells under 700 fires. The feed-entry event (D) is followed by a reader within 60 s at
10-22 per mille, ten to twenty times the base, and still loses: by the time the token is
visible the swarm has already priced it. The two lifts that hold everywhere are "the
creator has not sold" (about +3 pp) and "below 80 % of the life high" (about +3 pp), and
together they reach gross zero, which is the cost bar.

The shape behind all of it: peaks of +43 to +58 % within 300 s on a quarter to a third
of the tokens, and -8 to -17 % at the 300 s mark. The peaks are the swarm's spike and
reversal inside one or two slots, which a trail cannot sell into; the sustained runner
that a harvester book lives on is one token in twelve in the readers' universe and no
event tested here raises that rate.

## Reproduce

`aa.px` (every print, in-slot position, pre-decision state) -> `aa.buy` / `aa.sl` ->
`aa.fire` (k-th distinct buyer) -> `aa.resp` (reader response) -> `aa.g2` (forward paths
and fills for every k = 2 fire) -> `aa.m` (money per fire per seat per exit; `w0..w3` are
the density weights) -> `aa.f2` (velocity, feed, build terms) -> `aa.tr` (once-per-token
transitions) -> `aa.h` (pre-cutover block) -> `aa.seat` (real fills) -> `aa.feed` / `aa.mf` (feed placement per 5 s bin) -> `aa.lead` / `aa.farm` (wallets ahead of the readers) -> `aa.wstat_h1` (first-half wallet records) -> `aa.m3` (k = 3) -> `aa.mlk` (classifier top slice) -> `aa.dent` / `aa.dcand` / `aa.dfwd` (decomposition sets). Decomposition: `scratchpad/decomp.sql`, `decomp2.sql`. Machine event: `fire2.sql`, `mach_money.sql` (tables `aa.buy2`, `aa.fire2`, `aa.resp2`, `aa.mm`). Search:
`scratchpad/aa_search3.py` (execution floor); exits: `scratchpad/exit_sim_d3.py`; seat:
`scratchpad/seat_real2.sql`; classifier: `scratchpad/ml2.py`.
