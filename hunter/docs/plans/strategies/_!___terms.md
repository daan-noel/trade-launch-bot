# Terms: every word these files use, in plain language

The vocabulary of the strategy files. One word, one line, one place. If a word can be read two
ways, it is here.

[_!___strategy.md](_!___strategy.md) is the market and the laws · [_!___derive.md](_!___derive.md)
is the method · [_!___inventory.md](_!___inventory.md) is every idea ·
[_!___workflow.md](_!___workflow.md) is the open queue · [_!___evidence.md](_!___evidence.md) is
the numbers · [_!___metrics.md](_!___metrics.md) is what the engine measures and how.

Editing rules:

- **A word an idea or a finding needs is registered here in the same edit**, plainly, before the
  word is used anywhere else.
- One line each: what it is, in words a reader who has never seen this market can follow. A
  number belongs here only when it is fixed by the market or by the machine.
- No word is explained twice. A file that needs a word links here.
- A code name says where it is computed, so its definition can be checked against the code.

---

## The tape

| term | meaning |
| --- | --- |
| **print** | One buy or one sell that landed on chain. One row of the tape: coin, side, SOL, wallet, fee_payer, slot, ix structure. |
| **coin** | One token (one mint). |
| **wallet** | The address the venue credits on a print. A router can put many traders behind one wallet. |
| **fee_payer** | The account that signs the transaction and pays its fee: the real sender. It differs from the wallet when a router trades for its users. |
| **public** | Any print that is not ours and not made by the wallet a study is following. Every count a rule reads is public: the instrument's own prints stay out, or the rule would be reading itself. |
| **slot** | Chain time, about 0.4 s. Prints in one slot land together; inside a slot, order by transaction index. |
| **silent** | No print for N slots. Always say whose silence: the coin's, or one ix structure's. |
| **tape** | Every print of every coin, in time order. "On the tape" means market-wide, not on one coin. |

## The curve

Every coin trades against a bonding curve, not an order book: each buy moves the price up along
a fixed formula, each sell moves it back down. Nothing else sets the price.

| term | meaning |
| --- | --- |
| **vsol** | Virtual SOL in the curve, the one number that fixes the price. It starts at **30** and the coin graduates to a real pool at about **115** (**the wall**). Price = vsol^2 / k, so price is the square of vsol. Every band in these files is vsol. |
| **real reserve** | SOL actually deposited in the curve: vsol - 30. |
| **vres** | The same quantity under its study name: vsol at, or just after, a print. |
| **live supply** | The tokens the buyers hold: everything bought minus everything sold, per wallet, never below zero. Not the mint's total supply. |
| **headroom** | The most a buy can make before the wall: (115 / vsol)^2 - 1. A +100 % target needs vsol ≤ 81. |
| **price move of a print** | What one print did to the price on its own: (vsol after / vsol before)^2 - 1. |
| **age** | Seconds since the coin was created. |
| **toll** | The cost of one round trip: 125 bps a leg, plus 0.000225 SOL a leg, plus our own impact. 3.2-4 %. |
| **seat** | Where our fill lands: the decision print, then our order 115 ms later, filled at the next print of either side. Every number is read at that seat, because that is what we can actually get. |

## How a print was sent

| term | meaning |
| --- | --- |
| **ix structure** | The ordered instruction list of a buy or sell transaction. Order and repeats count. It is how a print was sent, and it identifies the software that sent it. |
| **core ix structure** | An ix structure with token-account create/close and memo dropped, then hashed. The key when counting distinct ix structures, unless a row says otherwise. The engine computes it as `build_hash` (`flow_ix.rs`), the studies as `build_core` (`toolkit/lake_export.py`), and the two partition every label sequence identically. Both layers carry it in a column named `build`, and [_!___metrics.md](_!___metrics.md) names it a **recipe**: three spellings, one key. The payer is not in it. |
| **machine** | The software that sent a print, named by its ix structure and never by its wallet - a router puts many traders behind one wallet, so the wallet is not an identity. Used two ways, and a count says which: loosely, the software (the instruction names and their order); strictly, the **core ix structure plus the fee_payer**, which separates two operators running the same software. A count of independent machines uses the strict key. |
| **creation ix structure** | The instruction list of the transaction that creates the coin. It always holds a Create, so it never equals a trade ix structure. |
| **creation fingerprint** | A creation ix structure plus optional launch params (creator's first buy, first-slot buy, max cost, priority/tip fee, CU limit). Coins that share one form a **launch group**. |
| **app** | The program a buy goes through: the first program past compute budget, system, token, associated-token and memo. Coarser than an ix structure: one app sends many of them. |
| **public app** | An app that many people use and come back to: on the UTC day before this buy, more than 100 wallets bought through it and they made 2 or more buys each on average. A bot swarm is wide but never comes back, so it fails the second half. |
| **tool** | A public trading app whose program sits in the ix structure: Axiom, Photon, GMGN, Bloom, Trojan, Terminal. Thousands of wallets share one tool structure. |
| **ix template** | A tool name plus markers: CU (compute budget), ATA (token account), N (nonce), S (seed), F (fee transfer), e.g. `Axiom Trade\|CU\|ATA\|N\|F`. Coarser than an ix structure. A direct pump.fun buy keeps its instruction name (`BuyExactQuoteInV2` and `BuyExactSolIn` are different traders). |
| **aggregator** | A routing program other bots pass through: Jupiter, DFlow. |
| **seed racer** | An ix structure that creates a throwaway account (`CreateAccountWithSeed`). It sprays many coins and reacts to someone else's print. |
| **nonce buyer** | An ix structure that sends a pre-signed transaction (`AdvanceNonceAccount`) and no seed account. Prepared in advance: the opposite signal to a seed racer. |
| **priority/tip fee** | What a print pays to land sooner: the compute-unit price, plus a tip to a Jito validator. Read it against that ix structure's own usual fee, never against the market's. |

## Who acts

| term | meaning |
| --- | --- |
| **creator** | The wallet that created this coin. |
| **operator** | A person or team running its own bot. |
| **operator structure** | An ix structure with ≤ 50 wallets and ≥ 200 prints this week: one operator's bot, not a public tool. |
| **reader** | A trader who is profitable on most days and whose buying reacts to something public on the tape - the kind of actor worth following. Never the person at a terminal reading a safety panel. The opposite is **volume manufacture**: a wallet whose book is its own round trips, which is dropped before any event is looked for. |
| **instrument** | A wallet we study because it makes money. It names a decision worth copying. It is never a term in a rule, its coins are never a door, and its own prints stay out of every public count. |
| **bundle** | Wallets whose first buy of a coin landed in one slot through one ix structure: one trigger behind many wallets. At 3 or more such first buys, the engine calls the group bundled. |
| **bundle share** | The share of live supply held by wallets that bought in the creation slot. |
| **bundled share** | The share of live supply held by bundled wallets, at any age, not only the creation slot. |
| **crowd** | The wallets that bought in the last hill. |
| **pusher** | The ix structure that bought the most in the last hill. |

## What a coin does

| term | meaning |
| --- | --- |
| **run** | One ix structure's prints on this coin inside one slot. The event unit; never the whole slot. |
| **burst** | One ix structure's prints on this coin until that ix structure is silent ≥ 10 slots. |
| **hill** | A completed rise of ≥ 50 % from a low to a peak. |
| **flush** | Price ≥ 20 % below the coin's peak so far. |
| **slow wall** | A coin reaches vsol 60 with its peak ≥ 60 s after birth: it climbed instead of spiking. |
| **frenzy** | ≥ 15 distinct ix structures printed on the coin in the last 5 s. |
| **absorption** | A sell whose drop is bought back: the coin makes a new high within 15 s of the sell. On a curve every buy moves price, so absorption means that sequence, never buying that fails to move price. |
| **one-slot rug** | A fall of -50 % or worse inside a single slot: many wallets selling together, not a drift down. |

## A rule

| term | meaning |
| --- | --- |
| **door (D)** | Which coins we watch at all. |
| **event (E)** | The print we fire on. |
| **permission (P)** | State already true when that print lands. |
| **exit (X)** | How we leave. |
| **re-entry (R)** | Tickets per coin. |
| **size (S)** | The clip we buy. |
| **up door / loss door** | A door that picks coins that go up / a door that removes coins that go to -50 %. |
| **booked** | A number that a study fixed and a shipped rule now carries. It is not re-fitted casually: changing it changes what ships. |
| **sentence** | One complete rule: a filling for each of the six slots, empty included. It is the unit everything is measured on, so a red number closes one sentence and never a slot. |
| **term** | One clause of a sentence: a single fact cut at one value, on one side. A term is never read alone - only inside the sentence it sits in. |
| **parent** | The sentence a search starts from, with the fillings already fixed. A search adds terms to a parent; a new event or a new instrument is a new parent, not a deeper search. |
| **coordinate** | Which sentence a number belongs to, written as all six slots. A number without one cannot be checked or reused. |
| **cell** | One sentence booked on one tape: the row of numbers a walk produces. |

### How an exit is written

An exit is a family plus its settings, written as one string. `tp` is a take profit, `sl` a stop,
`trail` the give-back from the best price reached during the hold, `c` (or `cap`) the clock in
seconds from our fill, and **arm** what the position must first be up before a trail or a ride can
fire at all. Every percentage is against our own entry price.

| written | what sells the position |
| --- | --- |
| `clock 45` | 45 s after our fill, whatever the price. Nothing else fires. |
| `trail40 c600` | A give-back of 40 % from the best price in the hold, or the clock at 600 s. |
| `tp10 / sl25 / 60 s` | The **bracket**: take profit at +10 %, stop at -25 %, clock at 60 s. |
| `tp100 / trail50 / c1200` | Take profit at +100 %, else a 50 % give-back, else the clock at 1200 s. |

Every branch resolves to a print index and the smallest index wins, so a written exit reads as
"whichever of these lands first". An **unarmed** trail has no arm gate and can fire from the start;
an **armed** one sleeps until the position is up by its arm, which leaves the position with no exit
but the clock underneath it unless a stop is set there.

## Words the method uses

These say how an idea is judged. The method itself is [_!___derive.md](_!___derive.md).

| term | meaning |
| --- | --- |
| **instrument, node** | The wallet being studied, and the kind of decision it makes (hot tape, mid tape, launch). A node is picked, then derived. |
| **leftover** | How much of a rise is still ahead of us once our order fills 115 ms after the print we followed. If a move is spent before we arrive, the leftover is gone and the idea is dead however good the print looked. |
| **peak leftover** | The best price the coin reaches after our fill, measured from that fill. It says an edge could exist; it is never what a rule earns. |
| **reaction cost** | How much of the rise is already paid for by the time we fill. A cost is not a kill on its own: what matters is what is left after it. |
| **acted / ignored** | Of all the prints of one class on a trader's coins, the ones it bought right after, against the ones it let pass. The whole question of "which prints does it take" is this split. |
| **cover** | The share of a trader's own buys that a spelling catches. A rule that describes 1 % of what it does is a corner, not its logic. |
| **occupancy** | What the event earns when it fires on **every** coin, not only the ones the trader touched. Red occupancy with real leftover means the event is right and the door is missing, or the class is too wide. |
| **slice** | The share of all prints of a class that a term keeps. A tiny slice with a big lift is a corner. |
| **ladder** | Building a rule one term at a time: each step adds the single cut that most improves it, keeping the terms already taken. |
| **fold, walk-forward** | The days are split in two: a rule is built on one half and scored on the other. A term that only works on the half it was built on is a fit, not a finding. |
| **chance ladder** | The same ladder run on shuffled labels. A step counts only when it beats what shuffling alone would have produced. |
| **study / holdout** | The days a rule is read on, and the later days it is judged on. A holdout is read once. |
| **ship bar** | The fixed list a rule must clear before it trades real money, written before the run that tests it. |
| **book** | What a rule earned over a set of days, at our seat, with every cost in it. |
| **top 1 %** | How much of a book comes from its single best trade. A book carried by one ticket is not a rule. |
| **body** | A book with its top 1 % of tickets removed. It says whether a rule pays without its luckiest trades. |
| **RACE / PEER / FOLLOW** | Three seats, by where our fill lands against the trader's own buy: RACE just before it (does **his decision** pay?), PEER beside it, FOLLOW 115 ms after it (a copy of **his fill**). FOLLOW red is expected and closes nothing; only RACE answers whether the decision is worth copying. |
| **DELAY** | The gap between a public print and the SOL that print promises. It is the whole edge: when the tell and the money land together there is no trade at any seat, and that is the one failure that kills a story outright. |
| **episode** | One round trip: a first buy while flat, through to the sell that takes the position to 2 % or less of its peak. For a coin rather than a trader, a trough-to-peak leg closed when price retraces 20 % from the peak. |
| **big / playable episode** | Big is an episode that reaches **+100 %**. Playable is a big one that is not an age-0 launch ramp, so it is still running when our fill lands: the median runs **87 s**, which is what harvester exits are designed against. |
| **lift** | How much more often something happens where a rule fires than where it does not. A lift of 2 is twice the base rate. High lift over a tiny slice is a corner, not a logic. |
| **oracle** | A number computed with hindsight to bound a slot - the best a perfect door or a perfect exit could reach. It says how much room the slot has. It never says a rule can get there. |
| **client** | The unit a book's trades actually arrive in, because trades are not independent draws: the launch group behind a door sentence, the machine behind a trader-node sentence, the coin behind an event-only sentence. A book that dies when its best client leaves is that client's book, not a rule. |
| **ticket floor** | The refusal that a sentence prints at least 50 first-per-mint trades on **every** day. It is a floor, never a target, and a mean across the days hides exactly the shape it exists to catch. |
| **harvester / scalper / grinder** | Three shapes of book, by what each lives on. A harvester takes a slice of a real up-move over tens of seconds - the target here. A scalper takes 1-2 s pops. A grinder takes many small wins that one bad trade erases. |

## One thing, several names

The engine, the studies and these files grew their own spellings for the same key. A rebuilder
meets all of them, so each is recorded here rather than banned. Prose prefers the left column.

| the word here | also written | where the other spelling comes from |
| --- | --- | --- |
| **core ix structure** | recipe, build, `build_hash`, `build_core` | the engine's metric name and both layers' column name |
| **operator structure** | professional build, pro, `is_pro` | the launch-door studies |
| **machine** | - | defined above: the software, or strictly that structure plus its fee_payer |
| **creation ix structure** | creation build | the creation-side studies and the client gate |

Two spellings are not kept. Say **ix structure**, never "trade-ix"; say **seed racer** or **nonce
buyer**, never a bare "racer" - the bare word hides which of the two opposite signals is meant.

---

## Study code names

The Python studies name each fact they read. The engine's own metrics are in
[_!___metrics.md](_!___metrics.md); the names below belong to the study layer, and a rule that
ships must be re-spelled as an engine metric.

Paths are under `node-derivation/`. Every window excludes the print itself, so a fact can never
read its own event. Every count and every SOL figure is public: the wallets a study follows stay
out of it.

### The coin around this print

| name | what it measures | where |
| --- | --- | --- |
| `age` | Seconds since the coin was created. | `toolkit/facts.py:41` |
| `vres` | vsol after this print, in SOL. | `toolkit/candidates.py:81` |
| `hold_n` | How many public wallets hold a bag above zero before this print. | `toolkit/facts.py:94` |
| `nb2` `nb3` `nb5` `nb10` | Distinct ix structures that printed in the last 2 / 3 / 5 / 10 seconds. | `toolkit/candidates.py:72` |
| `buys2` `buys5` `buys10` | SOL bought in the last 2 / 5 / 10 seconds. | `toolkit/candidates.py:74` |
| `sells5` `sells10` | SOL sold in the last 5 / 10 seconds. | `toolkit/candidates.py:75` |
| `np5` | Prints on the coin in the last 5 seconds. | `toolkit/candidates.py:76` |
| `n60` | Prints on the coin in the last 60 seconds. | `mid-tape/mt_d8_all.py:279` |
| `nw5` | Distinct wallets that printed in the last 5 seconds. | `toolkit/candidates.py:76` |
| `npro2` `npro5` | Distinct operator structures that printed in the last 2 / 5 seconds. | `mid-tape/mt_p5b.py:60` |
| `nbig30` | Public sells of 1 SOL or more in the last 30 seconds. | `toolkit/candidates.py:82` |
| `stall` | Seconds since the coin last made a new high. | `toolkit/facts.py:53` |
| `dd` | How far under its running high the coin sits, in percent. | `toolkit/candidates.py:79` |
| `mv3` `mv10` `mv60` | The coin's price move over the last 3 / 10 / 60 seconds, in percent. | `toolkit/facts.py:89` |
| `quiet` | Seconds between this print and the one before it on the coin. | `mid-tape/mt_d8_all.py:276` |
| `pubbought` | Public SOL bought on the coin since it was created. | `mid-tape/mt_d8_all.py:280` |
| `sell_run` | How many public sells landed in a row immediately before this print. 0 when a buy came last. | `mid-tape/mt_p61_8aaRWu.py:85` |

### This print

| name | what it measures | where |
| --- | --- | --- |
| `ssize` | The print's own size, in SOL. | `toolkit/candidates.py:71` |
| `mvk` | What this print alone did to the price, in percent. | `toolkit/facts.py:48` |
| `large_recent` `szrel` | This print's SOL divided by the 75th percentile size of the 20 prints before it. Above 1 is bigger than that percentile. | `mid-tape/mt_p61b_8aaRWu.py:151`, `mid-tape/mt_d8_all.py:387` |
| `flow_spike` | This buy's SOL divided by the coin's SOL bought per slot over the 30 slots before it. | `mid-tape/mt_p61b_8aaRWu.py:146` |
| `bspike` | The same ratio for the whole slot's buying, this print included. | `mid-tape/mt_d8_all.py:385` |
| `slot_n` | Prints that already landed on this coin in this print's slot. | `mid-tape/mt_d8_all.py:535` |
| `ss_sell` | Yes when one of those earlier same-slot prints is a sell. | `mid-tape/mt_d8_all.py:535` |
| `prev_sgn` | The previous print's SOL, signed: positive a buy, negative a sell. | `mid-tape/mt_d8_all.py:537` |
| `prev_same` | Yes when the previous print on the coin came from the same ix structure. | `mid-tape/mt_d8_all.py:537` |
| `txi` | Where the print sits in its block, by transaction index. A position, not a time. | `mid-tape/mt_d8_all.py:424` |
| `pfee` | The transaction's priority fee, in lamports. | `mid-tape/mt_d8_all.py:413` |
| `tip` | The transaction's validator tip, in lamports. | `mid-tape/mt_d8_all.py:414` |
| `fee_rel` | Priority fee plus tip, divided by the geometric mean of the same on that ix structure's 20 or more earlier prints. Blank under 20. | `mid-tape/mt_d8_all.py:416` |
| `clip_n` | How many step-up prints the coin had before this one. | `mid-tape/mt_d8_all.py:544` |
| `clip_gap` | Seconds since the coin's previous step-up print. | `mid-tape/mt_d8_all.py:548` |

### The ix structure behind this print

| name | what it measures | where |
| --- | --- | --- |
| `silent` `bu_gap` | Slots since this ix structure's last print on this coin. | `mid-tape/mt_p61b_8aaRWu.py:137`, `mid-tape/mt_d8_all.py:529` |
| `bu_lastsell` | Yes when this structure's previous print on the coin was a sell. | `mid-tape/mt_d8_all.py:529` |
| `step_x` | This buy's SOL divided by this structure's previous buy on the coin. Above 1 is a step up. | `mid-tape/mt_d8_all.py:534` |
| `live_return` | Yes when this structure was away 10 or more slots and other wallets kept printing meanwhile. | `mid-tape/mt_p61b_8aaRWu.py:138` |
| `fresh_return` | Yes when this structure printed here before but this wallet has not. | `mid-tape/mt_p61b_8aaRWu.py:140` |
| `struct_n` | This structure's earlier prints on this coin. | `mid-tape/mt_p61b_8aaRWu.py:131` |
| `struct_wal_n` | Distinct wallets this structure has already used on this coin. | `mid-tape/mt_p61c_8aaRWu.py:117` |
| `struct_last_side` | This structure's previous print here: +1 a buy, -1 a sell. | `mid-tape/mt_p61c_8aaRWu.py:121` |
| `struct_dt_s` | Seconds since this structure last printed on this coin. | `mid-tape/mt_p61c_8aaRWu.py:122` |
| `struct_mv_since` | The price move since this structure's previous print here, in percent. Negative is a dip. | `mid-tape/mt_p61c_8aaRWu.py:125` |
| `first_this_hour` | Yes when this is the structure's first print on this coin in the current UTC hour. | `mid-tape/mt_p61c_8aaRWu.py:126` |
| `elsewhere_dt` | Seconds since this structure printed on a different coin. | `mid-tape/mt_p61b_8aaRWu.py:91` |
| `rotating` | Yes when `elsewhere_dt` is 5 seconds or less. | `mid-tape/mt_p61b_8aaRWu.py:95` |
| `clip_vs_med` | This buy's SOL divided by the median this operator structure spends on its **other** coins. Blank for a tool. | `mid-tape/mt_p61b_8aaRWu.py:97` |

### The wallet behind this print

| name | what it measures | where |
| --- | --- | --- |
| `wal_n` | This wallet's earlier prints on this coin. | `mid-tape/mt_p61b_8aaRWu.py:132` |
| `wal_n_any` | This wallet's earlier prints anywhere on the tape. | `mid-tape/mt_p61c_8aaRWu.py:87` |
| `w_gap` | Seconds since this wallet's last print on this coin. | `mid-tape/mt_d8_all.py:531` |
| `w_nethere` | This wallet's SOL out minus SOL in on this coin so far. | `mid-tape/mt_d8_all.py:531` |
| `w_early` | Yes when this wallet printed here before and its first print here was inside the coin's first 10 seconds. | `mid-tape/mt_d8_all.py:532` |
| `w_rot` `wal_else_dt` | Seconds since this wallet printed on a different coin. | `mid-tape/mt_d8_all.py:470`, `mid-tape/mt_p61c_8aaRWu.py:88` |
| `wal_rotating` | Yes when that gap is 5 seconds or less. | `mid-tape/mt_p61c_8aaRWu.py:92` |
| `w_age` | Seconds since this wallet's first print anywhere, capped at 6 hours. | `mid-tape/mt_d8_all.py:444` |
| `w_1h` | This wallet's prints anywhere in the last hour. | `mid-tape/mt_d8_all.py:447` |
| `w_newc1h` | Coins this wallet touched for the first time in the last hour. | `mid-tape/mt_d8_all.py:450` |
| `w_up` | The share of this wallet's other coins where its SOL out beats its SOL in. | `mid-tape/mt_d8_all.py:459` |
| `w_szrel` | This print's SOL divided by this wallet's mean earlier buy, tape-wide. | `mid-tape/mt_d8_all.py:466` |

### Print classes a study fires on

All are public prints only.

| name | what makes it true | where |
| --- | --- | --- |
| `structure_burst` | A buy whose ix structure has been silent 10 or more slots on this coin. | `toolkit/trigger.py:182` |
| `clip_step_up` | A buy bigger in SOL than that ix structure's previous buy on this coin. | `toolkit/trigger.py:185` |
| `struct_first_here` `new_build` | A print from an ix structure that has never printed on this coin. | `toolkit/trigger.py:186` |
| `alone_in_slot` | Exactly one ix structure prints on this coin in that slot. Its own repeat prints do not break it. | `toolkit/trigger.py:188` |
| `two_struct_slot` | Exactly two ix structures print on this coin in that slot. | `toolkit/trigger.py:189` |
| `buy_after_sells` | A buy that directly follows two or more public sells in a row. | `toolkit/trigger.py:190` |
| `seller_recent` | A sell by a wallet that last bought this coin 30 seconds ago or less. | `toolkit/trigger.py:183` |
| `seller_loss` | A sell by a wallet that is under its average cost on this coin. | `toolkit/trigger.py:184` |
| `burst_start` | A buy at least 0.4 s after the **coin's** previous print, whoever made it. It names the coin's silence; `structure_burst` names one ix structure's. | `toolkit/trigger.py:229` |
| `tool` `nonce` `direct` | Who sent the print, read off its ix structure: a public trading app, a pre-signed nonce transaction, or a direct call to the venue. | `toolkit/trigger.py:73` |
| `pro` | A print from an **operator structure**: its core ix structure has 200 or more prints this week from 50 or fewer wallets. | `toolkit/trigger.py:64` |
| `seed_racer` | A print from a **seed racer**. Diagnostic only: a trader who follows one is in a race, which is a reason to look elsewhere, not an event to fire on. | `toolkit/trigger.py:73` |
