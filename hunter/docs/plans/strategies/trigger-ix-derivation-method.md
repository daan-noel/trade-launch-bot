# Finding a wallet's trigger: the reality model and the method

Goal: work out what event makes a wallet buy, using only durable, wallet-free evidence.
Written from the 8dtx study but the logic transfers to any wallet.

## Part 1. What actually happens on chain

### An entry is a reaction to an event, and the event is other people's transactions

A wallet does not buy at a random moment. Something on the tape makes it fire. That
something is one or more transactions by other people. So the whole problem is: **which
transactions, when they appear, are the ones it reacts to?**

The only durable identity a transaction has is its `ix_labels` array, the ordered list of
instruction names. Wallets rotate constantly, so wallet identity is useless. The instruction
structure is the tool that built the transaction, and tools do not rotate.

### The latency gap, and who lands inside it

This is the part that breaks naive analysis.

```
   slot S                         slot S+1
   |                              |
   [TRIGGER]  ...................  [our wallet]
        |                              ^
        |  he sees it, decides,        |
        |  signs, sends  ~ 1 slot      |
        |                              |
        +--> [racer] [racer] [racer] --+
             other bots see the SAME trigger and are FASTER
```

Between the trigger landing and our wallet landing there is a gap of roughly one slot. Bots
faster than him land inside that gap. They are reacting to the same event he is.

Consequences:

- **A transaction landing before him is not necessarily his trigger.** The trigger lands
  before him, but so does every racer. "Landed before him" cannot separate the two.
- **The racers are the ones with latency machinery.** Pre-signed transactions using a
  durable nonce, or disposable accounts created inline. They exist to win that gap.
- **If the response window is wider than his latency, racers get credited as triggers.**
  Measured over S..S+5, harvester bots read 0.76%. Measured over S..S+1, they read 0.13%.
  The 0.76% was reactions to something that landed after the harvester.

So the response window must match his real latency, which is S to S+1.

### Bundles: several transactions in one slot

Transactions arrive in slots, not in a continuous stream. Several land in the same slot and
are simultaneous for every practical purpose.

Two different things look identical at first:

```
   one actor splitting a buy          several actors arriving at once
   -------------------------          -------------------------------
   [Photon 1.12]                      [Photon 1.16]
   [Photon 1.13]                      [Axiom  0.74]
   [Photon 1.14]                      [Trojan 1.31]
   same tool, near-identical size     different tools, comparable size
   -> he ignores it (1.1 %)           -> he fires (9.2 %)
```

The first is one person, and it carries no more information than a single buy. The second is
independent demand from independent people, which is the thing worth reacting to.

**The unit of analysis is the SLOT, not a time window.** `trades.block_time` is the ingest
clock with sub-millisecond resolution, so grouping by a time gap tears every bundle apart
and destroys the signal.

### Why unconditional counting inverts the answer

```
   a token already running                  a quiet token
   buy buy buy [BOT] buy buy buy            .    .    [ROUTER]    .
               ^                                      ^
   our wallet is ALREADY IN, for the        our wallet decides here,
   same reason the bot fired                because of this
   -> bot scores 5.45 %                     -> router scores 1.5 %
      and caused nothing                       and caused it
```

Reactive bots cluster on hot tape. So does our wallet. Counting co-occurrence measures the
tape, not causation. On this corpus the same marker reads **5.45% unconditionally and 0.13%
once tape state is held constant.** The sign of the finding flips.

## Part 2. The method

### Step 1. Build the corpus

Every buy on every mint the wallet traded, in the study window, named by `md5(ix_labels)`.
Use all of it. A 2% sample missed a 155,937-buy pattern that manual inspection found.

**Remove the wallet's own transactions from the candidate pool.** Otherwise its own buy
pattern scores as the strongest trigger in the corpus: here it read 2,737 hits out of 2,792
slots, a 98% hit rate, purely because the pattern was him.

### Step 2. Cut the tape into runs

A run starts when a buy lands with no buy on that mint in the previous 5 slots.

```
   slot   S-5  S-4  S-3  S-2  S-1   S    S+1  S+2 ...
           .    .    .    .    .   [X]   [ ]  [ ] ...
           |<------ silence ------>| |
                                     run starts here
```

Silence means zero buys of **any** size. There is no dust floor and sells are not counted.
Both choices are safe here: raising the floor to 0.01 SOL leaves the ranking unchanged, and
23.6% of silent windows contain a sell with no effect on the ordering.

Silence is a **measuring device**, not a claim about the wallet's rule. 8dtx is the first
buy out of silence only 37.6% of the time; the rest of the time he joins a run already
moving.

**The device is not neutral, and scoring only silence-breaks is a defect.** On 8dtx,
5,060 of 11,018 buys (45.9%) land nowhere near a silence-break, so every structure whose
role is to trigger mid-run stays invisible: on a dip, on a re-acceleration, on a second
wave. Score **every slot that carries a buy**, and use token state as a control instead of
as a filter. Restricting the corpus and controlling a covariate are not the same operation.

### Step 3. Describe the shape of the slot that breaks the silence

For that first slot, counting everyone except our wallet:

| field | meaning |
| --- | --- |
| `ntx` | how many buys landed together |
| `npid` | how many **distinct** ix structures among them |
| `tot` | total SOL |
| `spread` | largest buy divided by smallest |

### Step 4. Ask one question with the right window

**Did our wallet buy this mint in slot S or S+1?**

Not S+5. The window must match his latency or racers get the credit.

### Step 5. Control before comparing

Two things must be held constant before comparing one structure to another:

1. **Slot shape.** Compare a structure only against structures appearing in slots with the
   same `ntx` and the same total SOL. Without this, "which tool" silently measures "how big
   was the bundle".
2. **Buy size.** Inside the shape strata, keep a fixed size band.

Lift = observed response rate divided by the rate expected from that slot's shape.

## Part 3. What the method found on 8dtx

### The shape of the slot is the primary term

First slot after silence, total 0.3 to 6 SOL, response in S..S+1, his own buys excluded:

```
   1 tx    1.58 %
   2 tx    2.62 %
   3 tx    4.01 %
   4-5 tx  5.73 %
   6+ tx   9.21 %
```

### Count and size only work together

| total SOL | 1 tx | 4 tx |
| --- | --- | --- |
| 0.3-0.6 | 0.43 | 0.31 |
| 2.5-6 | **0.29** | **11.91** |

A single large buy is the worst cell on the board, worse than a small one. Several buyers
reaching the same total is the best. Size alone is nothing. Count alone is nothing.

### They must be different actors

Bundles of 2 to 5 transactions:

| amount spread | same structure | different structures |
| --- | --- | --- |
| within 1.2x | 1.48 | 4.52 |
| 1.2 to 2x | 1.12 | **9.16** |
| 2 to 5x | 1.09 | 5.10 |
| over 5x | 0.64 | 3.18 |

Same tool with near-identical amounts is one actor splitting a buy, and he ignores it.
Different tools at comparable size is genuine simultaneous demand.

Gate `ntx>=2 AND npid>=2 AND tot>=1.2`: **7.13 / 7.56 / 7.72 / 7.98 / 8.16** by week against
0.75 to 1.06 for everything else. Five weeks out of five.

### Tool identity is the secondary term, and it only bites when one buy breaks the silence

Lift after controlling for slot shape:

| class | single tx breaks silence | bundle breaks silence |
| --- | --- | --- |
| target router | **1.22** | 1.00 |
| other | 1.13 | 1.07 |
| direct | 0.72 | 0.85 |
| aggregator | 0.58 | 0.68 |
| harvester | **0.27** | **1.35** |
| launch buy | - | **0.00** |

When one transaction breaks the silence, **who** it is decides everything. A harvester alone
is a non-event. A retail router alone is a signal.

When several arrive together, **how many and how much** decides it, and identity mostly
stops mattering. Harvesters even invert and become mildly positive, which is consistent: a
pile of bots arriving confirms other machines also see the event.

The mechanism behind the single-tx column: a retail router transaction means a person opened
an app and paid a fee to buy. That is a decision. A harvester transaction means a program
noticed something. That is a reaction, and a reaction to an event you have not seen tells you
nothing.

### Identity is tertiary for PREDICTION and first-order for MONEY

These are two different questions and a term is routinely flat on one and dominant on the
other. Score every candidate gate BOTH ways.

Against *does the wallet fire*, measured over **all** slots with a causal label (it fires in
S+1 having not been in S), the spread compresses to Bloom 2.05, Photon 1.68, Terminal 1.19,
Trojan 1.14, Axiom 1.09, GMGN 0.91, DFlow 0.53, launch buys 0.00, and inside a full state
gate it collapses to 8.0-13.3% and goes nearly flat.

Against *what the fire returns* the same groups spread **17 points**, stably, every week.
On 8dtx, net per trade against a -2.00% baseline, for fires whose burst carries at least
0.5 SOL of that group's flow before the gate transaction:

| group | n | net | weeks |
| --- | --- | --- | --- |
| Bloom Router | 1,204 | +7.03 | 4/4 |
| Photon | 1,554 | +2.27 | 2/4 |
| Terminal | 15,785 | +0.74 | 4/4 |
| Trojan | 2,602 | 0.00 | |
| `Token 2022: InitializeAccount3` + nonce + seed | 47,429 | -3.79 | 0/4 |
| L2TEx + nonce + seed | 19,790 | -5.59 | 0/4 |
| direct + nonce | 1,788 | -7.20 | 0/4 |

**Every group carrying `CreateAccountWithSeed` loses money every week.** The marker is the
mechanism: a throwaway account is a disposable bot echoing information already in the price,
while a named retail router is a person deciding, and decisions keep arriving after the
fire. Requiring zero seed-account flow in the burst moves the whole book from -2.00% to
+0.13% while keeping half the fires - the largest single lever on the tape.

Group structures by **build signature** - router program plus machinery flags - never by a
hand-written class list. A program name alone merges unrelated actors: on this tape
"Pump.Fun (direct)" is 815 distinct builds carrying 29% of all SOL, and scoring it as one
class hides everything inside it. On Axiom and Photon the paying signature is
`CU + ATA create` and the no-ATA variants of the same router are inert; exact hashes and
that template are the same event. Money, door, and the three-grain comparison:
[ix-template-gate.md](ix-template-gate.md). Gap-then-burst kinds (same template vs mixed,
several wallets vs one, crossing print, create slot out):
[ix-burst-kinds.md](ix-burst-kinds.md).

**A high lift does not mean a trigger. Test symmetry around the wallet's own buy.** A
trigger lands ahead of it and almost never behind; a racer reacting to the same event lands
on both sides. Ahead/behind ratio on 8dtx:

| lands ahead, rarely behind (trigger) | lands both sides (racer) |
| --- | --- |
| Trojan 17.0, Jupiter 15.9, Photon 11.3 | `Token 2022: InitializeAccount3` + nonce + seed 1.54 |
| Terminal 10.8, Axiom 8.1, DFlow 7.9 | L2TEx + nonce + seed 1.04, direct + nonce 0.71 |

The nonce-plus-seed builds top the raw lift table and are **competitors co-detecting the
same event**, not the trigger. A structure with a ratio below 1 is a follower.

### Flow, not presence

Score a group by **how much SOL it puts into the slot**, not by whether it appears. Presence
lift understates the effect several-fold: Axiom 1.09 by presence against 3.08 at 1 SOL or
more, Terminal 1.23 against 3.86, Photon 1.76 against 4.15. The condition is a **band**, not
a threshold - above about 5 SOL every group collapses toward zero, matching the result that
one large buy is the worst cell on the board. Most of the raw flow effect is slot size, so
the shape control still has to run afterwards.

**For money, composition beats size.** Flow-size thresholds inside a clean burst are weak
and non-monotone (2-3 SOL +1.95% net, 5-10 SOL +1.92% but only 2/4 weeks). The share of the
burst coming from named routers rather than seed-account bots is what pays. Ask *who is
buying*, not *how much*.

### Which programs are which

| class | programs | why |
| --- | --- | --- |
| target | Axiom, Photon, Terminal, GMGN, Trojan, Bloom, Maestro | apps a person clicks |
| aggregator | Jupiter V6, DFlow | plumbing other software calls, not a human decision |
| harvester | anything with `CreateAccountWithSeed` | disposable inline account, reactive |
| launch | anything with `Pump.Fun: Create_v2` | the creation buy, never a demand signal |

Two markers inside the array carry independent meaning and must never be merged into one
"speed" flag, because they point opposite ways:

- `System Program: AdvanceNonceAccount` - durable nonce, pre-signed, fired at a chosen
  moment. A prepared operator. **Positive.**
- `System Program: CreateAccountWithSeed` - throwaway account created inline, no setup cost.
  A reactive harvester. **Strongly negative when alone.** Combined with a durable nonce
  (`|N|S`) it marks a **racer**, not a target: it scores high on any co-occurrence measure
  and near baseline under a causal label, so read it with the symmetry test.
- `System Program: Transfer` - a fee is paid, so commercial software is charging for this
  buy. Mildly positive, and it is how a direct Pump.Fun buy built by a tool is recognised
  even when the tool's own program does not appear.

Instruction **order** also separates actors. The same labels in a different sequence are a
different build and can behave differently.

## Part 4. Gates a finding must clear

1. **Slot shape controlled.** Otherwise "which tool" is really "how big was the bundle".
2. **Size controlled.** Ordering holds inside every size bucket, not only pooled.
3. **Response window equals the wallet's latency.** S..S+1, never wider.
4. **The wallet's own transactions removed** from the candidate pool.
5. **Weekly walk-forward.** No crossing in any week.
6. **Mechanism stated before measuring.** A discriminator found by search and explained
   afterwards is usually an artifact.
7. **No label that depends on anything after the scored moment.**
8. **Symmetry checked.** A structure that lands behind the wallet as often as ahead is a
   co-detector, and its lift is co-occurrence rather than causation.
9. **Corpus is every slot with a buy**, with token state controlled rather than filtered.

## Part 5. Tables

```
   trades  -> pb      every buy, named by md5(ix_labels)
           -> pbx     flags: nonce, seed, fee transfer, ix count
           -> pbw     per-slot counts before / same slot / after   (EXCLUDE GROUP)
           -> pbwx    same, with the wallet's OWN buys removed
           -> bnd     one row per silence-breaking slot: ntx, npid, tot, spread, he1
           -> shape   + stratum key (ntx bucket, total SOL bucket)
           -> pmem    which ix structures were present in each such slot
           -> pscore2 per structure: lift when alone, lift inside a bundle
```

`EXCLUDE GROUP` on the slot window is required, otherwise a bundle member counts as both its
own predecessor and its own follower.

## Part 6. A trigger is not a trade

Finding when a wallet acts and finding a way to make money are separate problems, and the
second one usually fails. Run these four tests before believing a derived trigger is
tradable.

### Test 1. Apply the gate to every token, not the wallet's own

The corpus used to derive the trigger is the wallet's own token universe, which is a
hindsight label. On 8dtx that universe is 1.8% of all mints. Re-run the gate over the full
tape, priced at the gate-completing fill, net of 1.25% fee plus round-trip impact:

```
   his tokens     +5.87 % net
   other tokens   -2.00 % net      <- what a blind bot actually earns
```

A gate that only works on the tokens the wallet chose has measured the choice, not the
trigger. **That gap is the finding, not the failure** - it sizes the token screen worth
searching for.

### Test 2. Price the fill at the transaction that COMPLETES the gate

Pricing entry after the whole burst finishes charges the bot for racers who land *behind*
the wallet, and it inverts every burst size and count result. Against 8dtx's real executed
price:

```
   fill at the moment the gate completes    +0.06 %   <- correct
   fill after the last buy in the slot      +4.07 %   <- a modelling error, not his edge
```

He lands 3rd on average, with 1.06 buys behind him. **Price the fill position before
ranking any burst feature.**

### Test 3. Measure the cost of landing late with n HELD CONSTANT

The execution question is not milliseconds, it is position in the slot. Hold the fire set
fixed and move the fill to the next buy, the one after that, and so on:

```
   d late     0       1       2       3       4
   gross   15.81   10.43    6.88    4.11    1.90     <- about 3.5 pp per transaction
```

Letting n shrink as the deeper offsets run out of transactions reads the same cost as
0.5 pp in total. That is survivorship: only bursts that kept going have a 4th buy. **A
d-late curve on a shrinking sample is worthless.**

### Test 4. Check where the profit sits

A convexity harvester's mean is a fat tail by construction, so report the concentration
next to it. On the combined 8dtx gate: 16,495 mints, the top 100 (0.6%) carry 51.7% of the
profit and the median trade is negative. That is the business model rather than a defect,
but it sets how wide the confidence interval is and how much size the rule can take.

### What each outcome means

| result | reading |
| --- | --- |
| positive on all tokens at the gate-completing fill | tradable; size it |
| positive only on the wallet's tokens | a token screen exists and is undiscovered |
| positive only at a fill nobody can reach | latency race, unreachable |
| the wallet's own entries go flat under a mechanical exit | its exit is discretionary |

### Where the money actually is on 8dtx: two independent gates

A gate that fires 264,289 times to catch 4,321 of the wallet's own is 24x too loose, and
"the trigger loses money" measured on it is a statement about the loose gate. **Concentrate
first, then price.** Concentrating the same trigger produces two near-independent gates,
each positive in all four weeks on tokens the wallet never touches, at the gate-moment fill:

| gate | n | net | weeks |
| --- | --- | --- | --- |
| M1 all burst flow from named routers, zero seed-account flow, `vsol_in <= 36` | 7,971 | +2.32 | +2.26 +2.08 +2.46 +2.51 |
| M2 creator initial buy in [0.2, 1.0) SOL | 27,369 | +3.15 | +4.70 +0.79 +2.52 +3.89 |
| union | 36,471 | +2.99 | 4/4 |

M1 carries no launch term and M2 carries no ix term, so neither is a restatement of the
other; the overlap is 4% of fires. Creator initial buy is monotone and each sub-band holds
4/4: 0.2-0.5 +2.29, 0.5-1.0 +3.50, 1-2 -0.27, **2-5 -3.40** (the bulk of the tape), 5-10
-4.03.

Concentration does not remove the execution constraint, it raises the intercept:

| fill on the union rule | net | weeks |
| --- | --- | --- |
| immediately behind the gate transaction | +3.12 | 4/4 |
| one buy later | +0.61 | 3/4 |
| last buy in the slot | -2.50 | 0/4 |

The money lives at d=0 and d=1. On the busy subset - fires with four or more buys behind the
gate transaction - the curve reads +17.18 / +11.91 / +8.13 / +4.99 / +2.56 for d=0..4, so
concentration buys latency tolerance exactly where the burst keeps running, and nowhere
else. In 39.6% of fires nothing lands behind the gate transaction at all.

**Screens that do not survive a leave-one-week-out test** and must not be reused: a
`cu_price` value set chosen by return; a launch-build blacklist learned on three weeks and
applied to the fourth (+0.94 / -1.27 / +0.03 / -0.06); and a token-age screen, which is a
proxy for creator initial buy - young tokens lose only because most carry a large creator
buy, and young plus a 0.2-1.0 SOL creator buy is the best cell on the board (+5.39, 4/4).

### The finished rule, and why the exit is no exit rule

Sweeping the remaining state axes by MONEY inside the ix gate - all 32 subsets of five
terms - the winner adds only two: **quiet** (prior 30 slots carry <= 3 SOL of buys) and
**age** (token is >= 75 slots old). 3,229 fires / 2,950 mints / **+5.96% net** / weeks
6.19 / 5.67 / 5.24 / 6.62 / **50.8% win** against a 31.9% baseline / 110-128 trades a day.

The liquidity cap that ranks near the top for *predicting the fire* COSTS money and has to
be dropped. Run both scorings on every candidate term or this error repeats.

**No stop and no take-profit beats every stop and every take-profit**, monotonically, once
the entry is priced at the gate-completing transaction: none +6.67, stop 0.85 +6.35,
0.90 +6.13, 0.95 +5.89; TP 1.60 +6.33, 1.30 +5.49, 1.15 +4.94; trail 40% +6.59, 25% +6.29,
15% +5.64. A plain timeout hold is flat across 15-50 slots and peaks near 20 slots (8 s).

The reason is the shape of the trade: 2.3% of fires average +147% and carry 56% of all
profit, 8.3% average +58.6% and carry 77%. Truncation is pure cost. **Any exit result
derived at a late fill on a loose gate inverts here** - a 5% stop sits inside a mean MAE of
-24%, so the stop-out rate swings roughly 10 points for a quarter-point of entry price, and
that sensitivity is what produced the opposite conclusion.

Check concentration before believing a rule: a broad rule has half its tokens profitable and
its top 1% of tokens holding about a third of the profit; a lottery has its top 1% holding
MORE than 100% of the profit, with the remainder net negative. The two must not share a book.
