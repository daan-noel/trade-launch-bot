# Inventory: the ideas

Every idea, as a tree: **slot → family → idea → variant**.
[_!___strategy.md](_!___strategy.md) is the basis;
[_!___derive.md](_!___derive.md) is the method, the gates and how a result is recorded;
[_!___workflow.md](_!___workflow.md) is the open queue;
[_!___evidence.md](_!___evidence.md) is the numbers. This file is the idea list used
**on a parent** (derive phase 10). It does not invent the parent.

Editing rules:

- Put an idea under the family that shares its mechanism. A variant is a `└` child of its idea.
  Open a new family only when no family shares the mechanism.
- One idea, one row. An axis used in several slots gets one row per slot, and each row says only
  what that slot does with it.
- Every term is spelled in ix structure or tape state. A row that needs a wallet identity is not
  an idea and is not added.
- Status is one word plus one reference. Numbers live in the evidence file, never here.

```
  D  Door        which coins we watch
  E  Event       the print we fire on
  P  Permission  state already true when that print lands
  X  Exit        how we leave
  R  Re-entry    tickets per coin (standing: unlimited, one position open)
  S  Size        the clip (standing: 0.2 SOL)
```

Status: **keep** (use when the sentence needs it) · **open** (not scored on a whole sentence at
lag_115) · **red** (red on the sentences already run; not closed) · **dead** (closed by a
mechanism) · **new** (never scored). `ev N` = [_!___evidence.md](_!___evidence.md) section N;
`ev 7 (C9)` = that rule's row in the evidence ledger; `case N` = step N of
[node-derivation/hot-tape-rule-1.md](node-derivation/hot-tape-rule-1.md) section 2.

---

## Terms

### The tape

| term | meaning |
| --- | --- |
| **print** | One buy or one sell that landed. One tape row: coin, side, SOL, wallet, fee_payer, slot, ix structure. |
| **coin** | One token (one mint). |
| **wallet** | The address the venue credits on a print. A router can put many traders behind one wallet. |
| **fee_payer** | The account that signs the transaction and pays its fee: the real sender. It differs from the wallet when a router trades for its users. |
| **slot** | Chain time, about 0.4 s. Prints in one slot land together; inside a slot, order by tx index. |
| **silent** | No print for N slots (usual N = 10, about 4 s). Always say whose silence: the coin's, or one ix structure's. |

### The curve

| term | meaning |
| --- | --- |
| **vsol** | Virtual SOL in the curve. Starts at 30; the coin graduates at about 115 (**the wall**). Price = vsol^2 / k. Every band in this file is vsol. |
| **real reserve** | SOL actually deposited: vsol - 30. |
| **headroom** | The most a buy can make before the wall: (115 / vsol)^2 - 1. A +100 % target needs vsol ≤ 81. |
| **age** | Seconds since the coin was created. |
| **toll** | The cost of one round trip: 125 bps a leg + 0.000225 SOL a leg + our own impact. 3.2-4 %. |

### ix structures

| term | meaning |
| --- | --- |
| **ix structure** | The ordered instruction list of a buy or sell transaction. Order and repeats count. It is how a print was sent. |
| **core ix structure** | An ix structure without token-account create/close and memo, plus whether fee_payer = wallet. The key when counting distinct ix structures, unless a row says otherwise. |
| **creation ix structure** | The instruction list of the transaction that creates the coin. It always holds a Create, so it never equals a trade ix structure. |
| **creation fingerprint** | A creation ix structure plus optional launch params (creator's first buy, first-slot buy, max cost, priority/tip fee, CU limit). Coins that share one form a **launch group**. |
| **tool** | A public trading app whose program sits in the ix structure: Axiom, Photon, GMGN, Bloom, Trojan, Terminal. Thousands of wallets share one tool structure. |
| **ix template** | A tool name plus markers: CU (compute budget), ATA (token account), N (nonce), S (seed), F (fee transfer), e.g. `Axiom Trade\|CU\|ATA\|N\|F`. Coarser than an ix structure. A direct pump.fun buy keeps its instruction name (`BuyExactQuoteInV2` and `BuyExactSolIn` are different traders). |
| **aggregator** | A routing program other bots pass through: Jupiter, DFlow. |
| **operator** | A person or team running its own bot. |
| **operator structure** | An ix structure with ≤ 50 wallets and ≥ 200 prints this week: one operator's bot, not a public tool. |
| **seed racer** | An ix structure that creates a throwaway account (`CreateAccountWithSeed`). It sprays many coins and reacts to someone else's print. |
| **nonce buyer** | An ix structure that sends a pre-signed transaction (`AdvanceNonceAccount`) and no seed account. Prepared in advance: the opposite signal to a seed racer. |
| **priority/tip fee** | What a print pays to land sooner: the compute-unit price, plus a tip to a Jito validator. Read it against that ix structure's own median. |
| **run** | One ix structure's prints on this coin inside one slot. The event unit; never the whole slot. |
| **burst** | One ix structure's prints on this coin until that ix structure is silent ≥ 10 slots. |

### Actors and coin life

| term | meaning |
| --- | --- |
| **creator** | The wallet that created this coin. |
| **bundle share** | The share of live supply held by wallets that bought in the creation slot. |
| **pusher** | The ix structure that bought the most in the last hill. |
| **crowd** | The wallets that bought in the last hill. |
| **hill** | A completed rise of ≥ 50 % from a low to a peak. |
| **flush** | Price ≥ 20 % below the coin's peak so far. |
| **slow wall** | A coin reaches vsol 60 with its peak ≥ 60 s after birth. |
| **frenzy** | ≥ 15 distinct ix structures printed in the last 5 s, and ≥ 2 SOL bought in the last 2 s. |
| **absorption** | A sell whose drop is bought back: the coin makes a new high within 15 s of the sell. On a curve every buy moves price, so absorption means that sequence, never buying that fails to move price. |
| **up door / loss door** | A door that picks coins that go up / a door that removes coins that go to -50 %. |

Do not use: recipe, trade-ix, build, machine, client, professional, or a bare "racer" (say seed
racer or nonce buyer).

---

## Map

```
D  DOOR
   D1  creation fingerprint, chosen by its own history   key · history test
   D2  creator's document (metadata URI)                 documented · host · reuse
   D3  this coin's life before the fire                  structures in · early life · hills · absorption
   D4  loss door                                         bundle · holders
   D5  off-chain, stored at the fire                     attention
E  EVENT
   E1  a listed ix structure acts                        the lists · trigger forms
   E2  silence, then a spend                             whose silence · who breaks it
   E3  an operator's plan is unfinished                  legs · clip left · under its own sell
   E4  a count crosses a line                            buyers · structures · flow rate
   E5  after sellers                                     dip · flush · sell inside a frenzy
   E6  this print                                        slot · size · fee
   E7  clock                                             fixed age
   E8  graveyard
P  PERMISSION
   P1  curve position                                    age · vsol · headroom
   P2  windowed tape metrics                             flow · price · crowd
   P3  skin in                                           creator · crowd · pusher
   P4  ix makeup of the recent tape                      seed racers · tools · buy SOL band
   P5  tape state already true
X  EXIT
   X1  static                                            clock · trail · target · bracket
   X2  the tape stops (a state cut)
   X3  an actor sells
R  RE-ENTRY
S  SIZE
```

---

## D - Door

### D1 Creation fingerprint, chosen by its own history

Group coins by how they are launched, then keep only the groups whose past coins made the kind of
move we want, often enough. Two parts: the **key** (which params make a group) and the **history
test** (which groups pass). Refreshed daily; a group stays in the door one to two days.

#### D1.1 Key: what makes a group

| idea | meaning | status |
| --- | --- | --- |
| exact creation ix structure | the ordered instruction list of the create transaction | keep · ev 3.1 |
| └ + first-slot buy | SOL bought in the creation slot, in a band | open |
| └ + creator's first buy | the creator's own buy inside the create transaction, in a band | open |
| &nbsp;&nbsp;└ no opening buy | the create transaction carries no buy | red |
| └ + launch settings | max cost, priority/tip fee, CU limit the launch software sets | open |
| coarse bucket | instruction count + last instruction, e.g. `5ix:BuyV2` | red · ev 6.9 |

#### D1.2 History test: which groups pass

A test names the move type, the minimum count of coins, the rate, and the window.

| idea | meaning | status |
| --- | --- | --- |
| slow-wall | previous UTC day: ≥ 20 coins, ≥ 5 % made a slow wall, not a bundle launch group (`3ix:Buy`, `4ix:Buy`, `3ix:BuyExactSolIn`) | keep (screen) · ev 3.1 |
| └ + first-slot buy ≥ 2 SOL | the same group, and this coin's creation slot bought ≥ 2 SOL | keep (screen) · ev 3.2 |
| instant wall | ≥ 10 % of the group's coins reach vsol 60 inside 15 s | red · ev 3.1 |
| dump factory → exclude | groups whose coins dump once and die | keep (exclude) |
| another move type | the test counts +100 % hills, or second hills, instead of walls | new |
| group live now | the same creation fingerprint is trading on another coin right now | red · ev 7 (C15) |

### D2 Creator's document (metadata URI)

What the creator publishes at birth, read from the metadata URI. It marks a coin that **lives**,
not one that spikes. A later fetch is empty on old coins, so capture is live.

| idea | meaning | status |
| --- | --- | --- |
| documented | the document has a website, a telegram or a description | keep · ev 3.3 |
| └ website-led | website AND (telegram OR description > 80 chars) | keep · ev 3.3 |
| └ telegram-led | telegram AND (website OR description > 80 chars) | new |
| └ telegram required | a telegram link is present | open |
| └ link still resolves | the site or telegram answers, checked at the fire | open |
| URI host | skip documents on launchpad hosts; keep generic IPFS | open |
| reused content | the URI, site, telegram, name or symbol already appears on an earlier coin | new |

### D3 This coin's life before the fire

Facts true on this coin before the event. Each fact carries the time it becomes known: a fact
known at age 60 s cannot serve a fire at age 20 s.

| idea | meaning | status |
| --- | --- | --- |
| operator structures already in | ≥ 8 distinct operator structures have bought this coin | keep · ev 3.3 |
| └ early | ≥ 2 operator structures print in the first 60 s | new |
| tools already in | count of distinct tools that have bought this coin | open |
| distinct wallets | ≥ N distinct wallets have printed on this coin | new |
| quiet birth | few prints in the first N slots | new |
| early life | vsol at age 60 s is 50-70 | open · ev 1.11 (known only from age 60 s) |
| already made a hill | at least one completed hill | keep |
| recovered from a flush | price fell into a flush and came back | new |
| high peak, not at the wall | peak vsol already ≥ X, and the coin is still mid-curve | new |
| never under a floor | vsol never fell below a named floor | new |
| earlier frenzy sells not absorbed | of this coin's earlier sells ≥ 1 SOL inside a frenzy, at most a third were absorbed | red · ev 1.15 (fails out of sample) |
| operator structures already round-tripped | many operator structures have bought and sold this coin | open · case 28b |

### D4 Loss door: which coin goes to -50 %

Read inside a vsol band. Below vsol 42.43 a -50 % move is impossible, so vsol alone separates
nothing.

| idea | meaning | status |
| --- | --- | --- |
| bundle share < 0.20 | creation-slot wallets hold under 20 % of live supply | keep · ev 3.7 |
| creator share < 0.10 | the creator holds under 10 % of live supply | open · ev 3.7 |
| snipers, fresh wallets, buyer count | safety-panel counts; they track vsol, not the loss | red · ev 3.7 |

### D5 Off-chain, stored at the fire

| idea | meaning | status |
| --- | --- | --- |
| attention now | feed rank, replies, livestream, stored at the fire | open |
| └ rank or KOTH crossing | the coin crosses a feed rank or king-of-the-hill at the fire | new |

---

## E - Event

### E1 A listed ix structure acts

Traders who decide tend to fire right after specific ix structures act: 2-4 Axiom buys, 2 Photon
buys, one nonce buyer. Keep named lists of those ix structures and fire when a listed one acts.
The list is the asset. Build a list from real decision prints; score it on the full tape, never
on the trader's own coins.

#### E1.1 The lists

| idea | meaning | status |
| --- | --- | --- |
| 8dtx decision structures | the 155 ix structures that 8dtx buys right after (`8dtx-event-structures.json`); built on "silent coin, then an ix-gated burst", so one trader's list, not a general one | open |
| tools | tool structures: Axiom, Photon, GMGN, Bloom, Trojan, Terminal | open |
| └ tool + nonce | a tool structure that also carries a nonce, e.g. `Axiom Trade\|CU\|ATA\|N\|F` | open |
| nonce buyers | nonce, no seed account | open |
| direct buys by instruction | direct pump.fun buys, split by instruction name | open |
| operator structures | ≤ 50 wallets, ≥ 200 prints this week | open |
| campaign structure | priority fee set before CU limit: a pusher's own bot | red · ev 7 (campaign-break) |
| never listed | seed racers and aggregators: they land after the decision | keep (exclude) |

#### E1.2 Trigger forms

| idea | meaning | status |
| --- | --- | --- |
| silent coin, then K buys of one listed structure | the coin is silent ≥ N slots, then one listed structure's run holds K buys. Silence and count work together, neither alone | open |
| K buys from ≥ 2 listed structures in one slot | different actors in one slot. One structure buying near-equal amounts is one actor splitting: count it once | open |
| first run on this coin | a listed structure's first run here, not its return | red · ev 7 (C16) |
| └ any new structure | an ix structure this coin has not seen before | new |

### E2 Silence, then a spend

Name whose silence (the coin's, or one ix structure's) and who breaks it. A listed breaker is E1.

| idea | meaning | status |
| --- | --- | --- |
| burst start | an ix structure's first buy ≥ 0.5 SOL after that structure is silent, on a mid-life coin; tool or router structures | keep (with slow-wall) · ev 6.4 |
| └ silence ≥ 10 slots, any structure | the same, any ix structure, ≥ 10 silent slots | red · ev 7 (C5) |
| first buy after a silent coin | first buy ≥ 0.5 SOL after the coin is silent ≥ 10 slots | red · ev 7 (C8) |
| new structure breaks the silence | the breaker is an ix structure this coin has not seen | red |

### E3 An operator's plan is unfinished

An operator executing a position in legs still has SOL to spend. The leftover is that ix
structure's remaining spend on this coin. One fire per (coin, ix structure).

| idea | meaning | status |
| --- | --- | --- |
| first leg | first buy ≥ 0.5 SOL of an operator structure that buys in 2+ bursts on ≥ 25 % of its coins | red · ev 7 (C9) |
| └ later leg | that structure's second-or-later burst on this coin | red · ev 7 (C11) |
| this coin's second burst | any ix structure already burst once on this coin, silent >= 10 slots, starts again; one fire per (coin, structure); not a cross-coin class | red · ev 7 (C12) |
| clip left | the structure has spent ≥ 0.3 SOL here and is still under 60 % of its median spend per coin | red · ev 7 (C9) |
| under its own sell | coin silent, vsol below this structure's last sell here, no buy back yet; the structure sells-then-buys on ≥ 25 % of its coins | red · ev 7 (C10) |
| └ operator structures only | the same, operator structures only | red · ev 7 (C9) |
| clip step-up | this buy is larger than the structure's last buy here | new |
| first print here this hour | the structure's first print on this coin in the current UTC hour | new |
| arrives from another coin | the structure is buying another coin, then prints here | red · ev 7 (C13) |

### E4 A count crosses a line

| idea | meaning | status |
| --- | --- | --- |
| second outside buyer | the second non-creator buyer, age 5-300 s | red · ev 7.0 |
| first outside buyer after the creator | the first non-creator buy after the creator has bought | new |
| operator structures cross K | the count of operator structures on this coin rises through K | open · ev 5.10 occupancy red; leftover unread |
| recipes in 2 s cross K | distinct recipes in the last 2 s rises through K | open · ev 5.10 occupancy red; leftover unread |
| first operator structure after only creator and seed racers | first non-creator, non-seed operator-structure buy >= 0.5 SOL; prior prints are only the creator and seed racers; one fire per coin | red · ev 7 (C15) |
| buy flow spike | a buy lands at ≥ 3x the coin's trailing 30-slot buy rate | open |

### E5 After sellers

| idea | meaning | status |
| --- | --- | --- |
| buy in the dip after a hill | a tool or operator structure buys while price is still down after a hill | red · ev 7 (machine print in the dip) |
| first buy after a flush stops | first buy ≥ 0.5 SOL (not a seed racer) after a flush, once vsol makes no new low for ≥ 10 slots; one fire per flush | red · ev 7 (C9) |
| sell inside a frenzy | a public sell ≥ 1 SOL inside a frenzy, a new high in the last 20 s, by a seller who bought ≤ 30 s ago; fire on the sell | keep · ev 1.20, 1.22, 1.26, 1.27 (rule 1's event; the frenzy's "≥ 2 SOL bought in 2 s" half drops out; +4.44 % 5/5 out of sample at the engine's grain; every term sits at its break-even margin, loosening adds no money; derive 5.2 passes it, cost 1.41 %, peak +7.46 %) |
| └ 9999hu sell >= 1 | the same print class on a younger one-shot; leftover on the fires it takes is live; occupancy of every fire at its age is red; working X on the acted pool is tp15 sl40 t70 | open · ev 5.11, 5.12 (parent); do not copy age <= 16 occupancy (launch first-sell); do not copy its close |
| capitulation cascade | a public sell ≥ 1 SOL from a seller at a loss, with the price down ≥ 5 % in 10 s, a second big sell in 10 s and a busy tape | red · ev 1.16. The second operator's dip leg books +3.50 % 8/8 on its own picks at our seat; no public spelling reaches it |
| big buy after a dip | a buy ≥ 0.5 SOL opening a burst after ≥ 0.4 s of silence, the price down over 10 s | dead at our seat · ev 1.16: the operator that trades it reacts in 47 ms |
| first buy after a run of sells | several sells in a row, then a buy | red · ev 7 (C14) |
| first buy after the crowd left | the last hill's crowd holds under 50 % | new |
| first buy after a structure sold | anyone's first buy after an ix structure sold this coin | new |
| top holders sold | the largest holders have sold | new |

### E6 This print

| idea | meaning | status |
| --- | --- | --- |
| two ix structures in one slot | two different ix structures print in the same slot | keep (term) · ev 7 (zigzag turn) |
| alone in its slot | no other print in the same slot | new |
| high priority/tip fee | this print pays at or above the 75th percentile | keep (term) · ev 7 (zigzag turn) |
| └ against its own structure | ≥ 5x its ix structure's median priority/tip fee | open |
| size buy ≥ 1 SOL | a public buy ≥ 1 SOL on a mid-life coin | red · ev 7 (G1) |
| large among recent prints | this print's SOL ≥ the 75th percentile of recent prints | new |

### E7 Clock

| idea | meaning | status |
| --- | --- | --- |
| fixed age | fire when the coin reaches age T | open |
| └ sell >= 1 at age <= 16 s occupancy | first public sell >= 1 SOL while the coin is still young | red as 9999hu's E · ev 5.11 (fire age p50 2 s, top 1 % 71 %; its own 8-16 s band is -3.74 % 0/7) |
| first size buy after age T | first buy ≥ 0.5 SOL after age 60 s, no silence cut | new |

### E8 Graveyard

Do not rebuild these as the event.

| idea | meaning | status |
| --- | --- | --- |
| zigzag turn | first print off the low after a 15 % bounce | dead |
| new-buyer acceleration | K first-time buyers in W slots | dead |
| seed-racer burst after silence | it confirms a decision already made | dead |
| several tools in one slot | the wave; our fill lands behind it | dead |
| copy a wallet's buy | its impact and the swarm behind it are in the price first | dead |
| swing pullback | price gave back d % of its swing high; bought r % off the low | red · ev 1.11 |
| up-move portrait | up m % in 60 s, a recent new high, small giveback, busy tape | red · ev 1.11 |
| feed top-10, creator re-buy, cadence | "now" tells | red |

---

## P - Permission

### P1 Curve position

Primary permissions. They do not explain why a move happens; they put the fire where a move can
pay and where a loss is bounded.

| idea | meaning | status |
| --- | --- | --- |
| age band | age inside a window | keep |
| └ established coin | age ≥ 158 s and ≥ 368 public wallets holding; a frenzy on a young, thin coin dies | keep · ev 1.14, holds out of sample 1.15; holders re-read in 1.20 and kept |
| room under the wall | vsol after the fire ≤ 100, so a +15 % target fits well under graduation (115); above 107.2 a trade closes on the completing buy, at a price the curve no longer offers | keep · ev 1.20 (safety term, not a fit) |
| sell-reactive buyers on the coin | distinct wallets on this coin that bought within 300 ms of a public sell ≥ 1 SOL | red · case 41 (not the arrival of the member's kind) |
| vsol band | vsol inside a window | keep |
| headroom | vsol ≤ 81 for a +100 % target | keep |
| pool alive | real reserve > 0 and the coin still prints | new |

### P2 Windowed tape metrics

Each metric has its own meaning and concentrates the pool: it buys "this coin is not decaying",
worth about the toll. None is a cause or an event, because windowed flow is the price path. The
window is seconds, slots or prints, with a lag so it cannot read the event. Definitions:
[metrics-reference.md](metrics-reference.md).

| idea | meaning | status |
| --- | --- | --- |
| flow | gross, net, buy and sell SOL, buy share, trade count over a window | keep |
| price | trail (below the window high), rise (the bounce above the window low) | keep |
| lifetime price | trail below the all-time high; stall (seconds since it) | keep |
| crowd | distinct wallets and new buyers in a window; outside buyers after age T | keep |
| quiet before | buy SOL in the prior N slots ≤ X | open |
| no new low | vsol has made no new low since the event | new |

### P3 Skin in

| idea | meaning | status |
| --- | --- | --- |
| creator has not sold | the creator still holds. A survival term: it also raises the loss rate | keep · ev 3.4 |
| crowd left | the last hill's crowd holds under 50 % | keep |
| └ crowd still holds ≥ 80 % | the last hill has not distributed | red |
| pusher still holds | the last hill's pusher has not sold | red |
| firing structure still holds | the ix structure we fire on has not sold | new |
| creator bought recently | the creator bought in the last N slots | new |

### P4 ix makeup of the recent tape

The permission side of E1.

| idea | meaning | status |
| --- | --- | --- |
| not a seed racer | this print is not a seed racer | keep |
| only this structure in the slot | no other ix structure in the same slot | new |
| no seed racer lately | no seed racer in the last N slots | new |
| tools only | all buy SOL in the run (or the last N slots) comes from tool structures, none from seed racers | open |
| buy SOL band | the run's buy SOL, or the tools' buy SOL over this slot and the one before, sits above noise and below "already priced" | open |

### P5 Tape state already true

| idea | meaning | status |
| --- | --- | --- |
| distinct structures in 5 s | the count of distinct ix structures printing in the last 5 s, with no single one dominating | open · ev 1.11 |
| coin already silent | the coin is silent when the print lands | new |
| operator structures 3-7 | several operator structures here, not a crowd | new |
| last print was a sell | the print before this one is a sell | new |
| slow re-entry state | quiet ≥ 60 s, ≥ 2 hills, a returning structure, pullback 40-70 %, vsol 55-80 | red · ev 7 (zigzag turn) |

---

## X - Exit

### X1 Static

| idea | meaning | status |
| --- | --- | --- |
| clock | close at T, 15-600 s | keep (control) |
| └ clock 25 on 9999hu acted sell >= 1 | copies its 20-30 s close; that close is the give-back | red · ev 5.12 |
| take profit | close at +25 / +40 / +100 % | keep (control) |
| unarmed trail | 20-40 % off the peak, counted from entry, cap 600-1800 s | keep |
| armed trail with a stop | trail only after +arm, a hard stop until then; shipped as arm +21 %, trail 36 %, stop -43.75 %, cap 1200 s | keep · ev 4.7 |
| └ no stop | a position that never arms has no stop | red |
| tp100 / trail50 / cap1200 | keeps the tail; fits a trough entry | keep |
| bracket | take profit +20 %, stop -60 %, clock 90 s (rule 1 at the engine's grain; +15 % / -40 % on the pool before it, -25 % before the wall term, +10 % / 60 s on the wider pool) | keep · ev 1.22, 1.24 |
| └ tp15 sl40 t70 on 9999hu acted sell >= 1 | working X: +1.61 % 7/7 body +4.47; top 1 % 43 %, capped red | open · ev 5.12 |
| └ clock 240 s | the same bracket held longer | red · ev 1.20 (study +0.6 SOL, holdout -1.2 SOL) |
| └ breakeven after +N | once up +3 / +5 / +7 %, close back at the fill | red · case 31 |
| └ half out at the target | sell half at the target, ride the rest | red · ev 1.24 (half at +20 %, the rest to 0.6 x the wall: fails a fold) |
| └ trail after the target | once up +10 %, trail 5-10 % off the peak | red · ev 1.24 |
| └ stepped trail | the higher the peak, the set trail off it: peak +10 % -> 5 %, +30 % -> 10 %, ... | red · ev 1.24 (tight: sells the winners' dips; wide: top 1 % 25 %) |
| headroom target | take profit at a share of the room to the wall: f x ((115 / vsol)^2 - 1) | open · ev 1.24, 1.25 (0.4 x with rule 1's 90 s clock beats rule 1 on every holdout line, read after selection; 0.3 x with 180 s loses it). Rule 1b = `m_position.room_taken >= 40`, engine books the Python tickets |
| plain hold | a fixed hold, no stop, no target | open |
| exit by payoff shape | pick the family from the group's own payoffs: a positive median wants a small target, a tail wants no target | open |
| static abort | not up X % by T → sell | dead |

### X2 The tape stops (a state cut)

A cut that can fire while the position is up is a clock. Every cut here fires only below the
fill.

| idea | meaning | status |
| --- | --- | --- |
| new buyers stop arriving | the crowd stops showing up | red · ev 1.24 on rule 1 (open · ev 4.7 elsewhere) |
| flush resumes | vsol makes a new low after an after-flush entry | keep |
| back to the pre-event low | vsol revisits the low before the event | red · ev 7 (C14) |
| firing structure goes silent | the ix structure we fire on stops printing | red · ev 7 (C8) |
| operator structures stop arriving | no new operator structure arrives | red · ev 7 (C15) |
| priority/tip fees fall | later prints stop paying to land | new |
| give back this burst's peak | trail against this ix structure's own peak, not the coin's | new |

### X3 An actor sells

| idea | meaning | status |
| --- | --- | --- |
| firing structure sells | the ix structure we fire on sells | keep · ev 7 (C11) |
| pusher sells | the last hill's pusher sells | open |
| creator sells | the creator prints a sell | new |
| ride the creator | hold while the creator holds, trail after the creator sells | new |
| firing structure leaves | the ix structure we fire on prints on another coin | red · ev 7 (C13) |
| sell into a buy | once up ≥ 10 %, sell on the first public buy ≥ 1 SOL or +3 % print | red · ev 1.24, 5.12 (does not beat the wide-stop bracket on 9999hu's acted pool) |

---

## R - Re-entry

| idea | meaning | status |
| --- | --- | --- |
| one open per coin, no cap | the standing rule | keep |
| below our own last exit | re-enter only under our last exit on this coin, e.g. 30 % under at vsol < 42.43 | open · ev 1.11 |
| round trips already taken | earlier round trips on this coin as a gradient | open · ev 1.11 |
| cool-down after a stop | no re-entry on the coin for N s after a stop-out | open · ev 1.20 (+0.33 SOL on 14 trades, inside chance) |
| entries a coin, capped | at most N entries on one coin | open · ev 1.20 (rests on 8 trades) |

## S - Size

| idea | meaning | status |
| --- | --- | --- |
| 0.2 SOL | the standing clip | keep |
| cost minimum | sqrt(F x vsol), about 0.126 SOL at vsol 70 | open |
| fraction of vsol | the clip scales with the pool | open · ev 1.20 (equal to flat on the study tape, better on the holdout) |
| └ 0.35 SOL flat for rule 1 | the largest flat clip that keeps every bar on both tapes | keep · ev 1.20 |
