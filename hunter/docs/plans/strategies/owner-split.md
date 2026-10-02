# Owner split: whose money is each trade

## Summary

**What it does.** Splits every trade on a coin into **owner** (fake demand: the dev, its wallets,
the machines it hires) and **outsider** (real money). Every later reading (owner profit, outsider
buying, the dump, the owner's decisions in [owner-decisions.md](owner-decisions.md)) stands on it.
It works for any launch group and any chart shape; section 6 says what to re-tune per shape.

**The core idea.** The owner uses many wallets, and wallets change every day. But the owner cannot
hide three things:

1. **Its machinery repeats.** Its scripts keep the same transaction build and fee settings, and the
   same fee payer, while the wallets change.
2. **Its tokens move without trades.** It buys with one wallet, moves the tokens by transfer, and
   sells with another. So some wallets sell what they never bought, and some buy and never sell.
3. **Its paid machines lose on purpose.** A volume service is paid off the chart, so on the chart
   it loses a little on most coins, for weeks. No real trader can do that.

**The workflow.**

```
 1 COLLECT   the group's coins + market-wide facts (every coin, every wallet)
     |
 2 MARK      sure facts (A) and transfer ends (T)            -> first owner lists
     |
 3 GROW      shared machinery (S) and paid machines (M)      -> repeat until nothing new
     |
 4 CLEAN     judge each added set by its job                 -> throw out the wrong ones
     |
 5 VERIFY    money, chart, eye check, old days, luck         -> pass, or fix and repeat
     |
 USE LIVE    one decision order per trade, for the engine
```

**The four kinds of evidence.** Every rule belongs to one kind.

| kind | IDs | what it is | why only the owner shows it |
| --- | --- | --- | --- |
| Sure facts | A1-A4 | creator, create transaction, creation-slot buy, owner-only build | owner by definition, or by a build the rest of the market does not use |
| Transfer ends | T1-T3 | sells more than it bought; buys and never sells; the matching pair | an outsider gets tokens only by buying, and sells what it holds |
| Shared machinery | S1-S6 | private structure, owner payer, shared transaction, fresh wallet | one script or one key holder sits behind all of them |
| Paid machines | M1-M3 | a machine operator (bundle or lockstep, its sister wallets joined) that loses steadily | a machine that keeps losing is paid by someone |

---

## 1. Words

**The six core words.**

| word | meaning | example |
| --- | --- | --- |
| owner | everyone who makes fake demand on the coin: the dev, every wallet it controls, every machine it hires | a volume service paid by the dev is owner, though it is not the dev |
| outsider | everyone else: real money. Kinds in section 5 | a sniper, a person clicking a buy button, a trading bot that wins |
| trader | who really made a trade: the credited wallet, except when that wallet is a **routing wallet** (a service wallet that thousands of users trade through); then it is the fee payer | every rule that says "wallet" reads the trader |
| structure | how a trade's transaction is built. Its **core** is what it does: the labels left once the extras are dropped (compute budget, every System Program instruction, token-program and token-account instructions, memo, Lighthouse), in order, with pump.fun's verb variants merged (Buy, BuyV2, BuyExactSolIn are BUY; Sell, SellV2 are SELL; Create, Create_v2 are CREATE); an app's own instructions stay. Its **marks** are the extras: which are present (CL CU limit, CP CU price, N nonce, L Lighthouse, M memo, S seed or created account, C account close, W wrap), never their order, plus two counts (T System transfers, A token-account opens). Its **numbers** are the CU limit, CU price and tip. A script keeps its core and rotates the rest | core `Pump.Fun: BUY`, marks `CL CP N T1 A0`, CU price 167,000, tip 0; a 6Vo3 buy: core `6Vo3245 BondingCurveV3`, marks `CL CP C T1 A3` |
| transfer | tokens moved between wallets without a trade. Not on the tape; we see only its two ends (rules T1-T3) | wallet B buys 93.4 M tokens, wallet E sells 93.4 M it never bought |
| machine | a script trading through several wallets at once: a **bundle** (3 or more wallets in one slot, one app, one side) or a **lockstep pair** (two wallets whose trades match one to one within 0.5 s). All wallets of one operator are judged as one | 4 wallets buying through Terminal in one slot: one trader's multi-wallet button |

**Other terms.**

| term | meaning |
| --- | --- |
| payer | the account that signed and paid the fee (`trades.payer_id`); a **service payer** pays for thousands of unrelated users |
| sniper | a bot that buys brand-new coins in their first slots, on many unrelated coins every day. An outsider |
| creation slot | the slot of the transaction that creates the coin |
| the group's coins | the coins with the group's creation facts (create instruction list and CU price), the only set A4 measures against: it is fixed at birth, so it never depends on the split |
| the owner's coins | every coin in the market where a sure fact (A) of this owner appears, not only the group's coins |
| loses steadily | over all its coins in the market: net SOL (sells minus buys) below zero over the last four weeks (M3) |
| bag | the tokens someone holds now, as read from its trades |
| drop | a slot where the price falls 10 % or more |
| fall | one drop (a one-shot dump), or drops less than 30 s apart (a waterfall). The **main fall** is the coin's deepest |
| start value | a first guess for a threshold, tuned on data |

---

## 2. Step 1 - COLLECT

| what | from | why |
| --- | --- | --- |
| every trade of the group's coins: instructions, CU limit, CU price, tip, slot, block position, wallet, payer, routing flag | PG `trades` (payer and routing are PG only, about 30 days) | the rules read all of them |
| the creator, creation slot and create transaction of each coin | PG `tokens` | sure facts A1-A3 |
| per instruction list (fees ignored): its trades on the group's coins and on all coins, and on how many group coins | the whole market (lake) | A4: owner-only build? |
| per structure, at every match level: its trades on the owner's coins and on all other coins | the whole market (lake) | S1: private or public? |
| per payer: coins and wallets it pays for, per day | the whole market (PG) | S3: owner key or service? |
| per trader: first-slot buys over the window, on how many coins and creators | the whole market (PG) | A3: sniper or bundle? |
| per wallet, over the last four weeks: coins, buys, sells, coins ended empty, coins behind, net SOL | the whole market (lake) | T2 and M3 |
| per machine-linked wallet pair: how many market coins both trade | the whole market (lake) | M2: sisters or strangers? |

Build on the last ~2 weeks. Check on the 2 weeks before (step 5).

---

## 3. Steps 2 and 3 - MARK and GROW: the rules

Every rule is a test, a start value, and the reason only the owner passes it.

### A. Sure facts - owner with no test (step 2)

| ID | rule | why |
| --- | --- | --- |
| A1 | the creator: the wallet that signs the create transaction | it is the dev |
| A2 | every other wallet inside the create transaction | one transaction is signed by one key holder |
| A3 | every buy in the creation slot, except a sniper's | nobody can see a coin before it exists; only the dev's bundle and the snipers' bots land in its own slot |
| A4 | every trade with an **owner-only build**: an instruction list (fees ignored) that the rest of the market does not use | the owner's script or program runs mostly on the owner's coins; a public app runs on every coin |

**Sniper (A3).** A creation-slot buyer whose trader buys in the first three slots of many unrelated
coins. Start value: 20 or more coins over the window (two weeks), from 10 or more creators, 20 % or
less of them the group's coins. Breadth is counted over the window, never per day: a sniper with a
quiet day still snipes hundreds of unrelated coins (one buys the first slots of 88 coins in 13 days,
about 7 a day, and wins 102 SOL). A sniper is an outsider and stays one on that coin. Size is a
hint, never the test: many snipers buy 0.5-1 SOL.

Example: in coin X's creation slot, W4 buys 1.4 SOL (first slots of 2 coins in two weeks, both from
this creator: owner) and W5 buys 0.62 SOL (first slots of 1,690 coins a day: sniper, outsider).

**Owner-only build (A4).** An instruction list is the owner's when 30 % or more of its trades in the
whole market are on the group's coins, on 5 or more group coins, from 2 or more creators. The group's
coins are about 0.14 % of the market, so 30 % is about 200 times what chance gives; a public app's
build sits on every coin and scores under 1 %. A4 needs no owner found first, so it starts the split
on a group with no known program, where S1 has nothing to grow from.

- **Fees are left out.** CU limit, CU price and tip are config the owner changes at will; with fees
  in the key, fee variants of plain builds mix in public users.
- **30 %, not 80 %.** One owner's tools serve several launch builds, so its share on any one group
  stays well under 80 %: on 7ix the crew program works 706 coins in two weeks, 418 of them 7ix coins.
- It finds the owner's programs without being told their names: on 7ix the `9ddjzq` lists, on 5ix
  41 unknown programs (each about 58 % on the group's coins, run through 52,540 throwaway wallets)
  plus the plain Pump.Fun sell lists that sell for 4 or 12 wallets in one transaction.

Example: a build used 10,000 times in two weeks, 5,800 of them on 300 of the group's coins from 280
creators, is the owner's; an app's default buy, used on every coin of the market, is not.

### T. Transfer ends - owner because the tokens moved (step 2)

| ID | rule | start value | why |
| --- | --- | --- | --- |
| T1 | **sells more than it bought** on the coin | a sell takes its bag below zero (more than 1 % of the sell) | an outsider gets tokens only by buying; the rest came by transfer from the owner's buyer wallets |
| T2 | **buys on many coins and never sells** anywhere in the market | 10 or more coins, 20 or more buys, no sell, in the window | nobody buys that much and never sells; it is a hired buying machine whose tokens leave by transfer |
| T3 | **the other end of a T1 transfer**: a wallet that only bought on the coin and still holds, whose bag equals one T1 wallet's shortfall | within 0.5 %, and it is the only wallet that matches | it handed its tokens to the seller |

- T1 is a fact about the coin: the wallet is owner there. It joins the owner wallet list (for other
  coins) only when it repeats on 2 or more coins.
- T2 is owner on every coin it buys.

Examples. T1: wallet E has no buy on coin X and sells 4.5 SOL of it in three sells, at 1.5 s, 8.6 s
and 13 s. T2: one wallet makes 25,270 buys on 16,141 coins in a month, and not one sell. T3: E sold
93.4 M tokens it never bought; B bought 93.4 M and never sold.

### S. Shared machinery - grows from known owner (step 3)

Start from the owner found by A and T. Repeat S1-S6 until a round adds nothing.

| ID | rule | start value | why |
| --- | --- | --- | --- |
| S1 | **a private structure** becomes an owner structure when (a) its trades are mostly owner, AND (b) the rest of the market does not use it, AND (c) it runs on a timer OR its wallets also use another owner structure | (a) 95 % or more of its trades, from 5 or more owner wallets, or every trade owner except those of one narrow wallet (under 100 market coins in the month); (b) 80 % or more of its market trades on the owner's coins; (c) first seen within 1 s of its usual age on 60 % of its coins, or 50 % of its wallets use another owner structure | a script keeps its build and fees while its wallets change |
| S2 | a wallet that uses an owner structure | one trade is enough | the structure is the owner's script |
| S3 | **an owner payer**: pays for an owner wallet, and is not a service | 80 % or more of the trades it pays for are on the owner's coins, and fewer than 50 coins a day | a key that pays only for the owner's wallets is the owner's key |
| S4 | a wallet paid for by an owner payer | one trade is enough | finds owner wallets hiding in public apps |
| S5 | a wallet in one transaction with an owner wallet | one transaction is enough | one transaction is signed by one key holder |
| S6 | a fresh wallet whose first trade ever is on the owner's coins, with an owner structure or payer | no trade in the 14 days before | a wallet born into the owner's job |

**Match levels (S1).** A dev rotates small variants of one build (a memo added, the CU
instructions swapped, BuyV2 instead of Buy, a computed CU limit that differs on every
transaction), so S1 judges every structure at seven levels, loosest first:

| level | the trade matches when it has the same |
| --- | --- |
| 1 | core and side |
| 2 | core, side and marks |
| 3 | core, side and CU price |
| 4 | core, side and tip |
| 5 | core, side, CU price and tip |
| 6 | core, side, marks, CU price and tip |
| 7 | exact instruction list, CU limit, CU price and tip |

A structure is owner at any level where S1 (a)-(c) hold, so the loosest passing level catches every
variant the script rotates. A private program passes at level 1. A public app's core fails there (its
users share it) and passes only at a level that carries the owner's own fee, or never. Example: on
FohR the owner's buys carry computed CU limits (96,591 and 100,139, never twice the same), so level 7
never repeats; level 3 (`Pump.Fun SELL`, CU price 167,000) holds 15 trades of 9 wallets on two
weeks of the group's coins, 83 % of its market trades on the owner's coins.

**A level is a tag row.** The volume list (and the fingerprint the engine runs) holds each owner
structure at its own level: level 7 as an exact `ix_shape` row (labels, CU limit, CU price, tip),
levels 1-6 as a core row (`"level": "core"`, the core's labels, the side, and the marks, CU price
and tip the level pins). A pin on a reading the trade lacks (no CU price, no tip) is left off, so
that row matches any value there. On 7ix: 167 core rows and 85 exact rows (the program's own
structures are the `program` matcher), with the creator and 6,861 owner wallets, sticky. It
matches the split on 99.64 % of SOL on 09-15 .. 10-01 and 99.83 % on 09-02 .. 14, and the engine
folds it to the same owner SOL as the Python model on all 1,300 coins (to 0.000001 SOL; the lab's
`metric-series`, `m_flow.buy_sol` / `sell_sol` @volume and @!volume): owner 93.39 % of SOL
against the split's 93.56 % on 09-15 .. 10-01, 94.28 % against 94.24 % on 09-02 .. 14.

**One narrow wallet does not block S1 (a).** The wallet under question is itself one of the
structure's users, so when it is the only non-owner user, its own trades would keep the structure
below 95 %. Example: on FohR, 14 of the 15 level-3 trades above are owner; the 15th is wallet 4CPn
(one coin in the month, bought 1.96 SOL at 5 s, sold 6 s later). A wide trader never gets this pass:
a winning bot on 9,259 coins that used an owner fee once stays outsider.

**Private or public (S1 b).** A structure that shows up on thousands of other coins is a public app
setting (Axiom, GMGN, a plain Pump.Fun trade with the default fee) and says nothing about who traded.

| structure | on the owner's coins | on all other coins | share | verdict |
| --- | ---: | ---: | ---: | --- |
| Pump.Fun Sell, CU 160,000, price 621,000 | 2,700 | 300 | 90 % | private: owner script |
| Axiom default buy | 4,800 | 550,000 | 0.9 % | public: says nothing |

**Owner settings inside a public app.** An app's users pay the app's live default fee, which moves
during the day for everyone together. A script inside the same app often keeps its own fixed fee.
Because the structure includes the fee, such a script is its own structure, and S1 judges it.
Example: most 6Vo3 buys pay tip 3,063 at 14:05 and 2,101 at 14:20; a set of wallets pays CU price
80,000 and tip 0 all day: its own structure.

**Service or owner (S3).** A service pays for thousands of wallets on thousands of coins and says
nothing. A routing wallet is the opposite case: thousands of payers trade through it, so its payers
are the traders.

### M. Paid machines - owner with no tie to this owner (step 3)

**Why.** A volume service sells volume to many devs at once. The dev pays it off the chart (a plain
SOL transfer). Its trading pays the venue fee and slippage, so on the chart it loses a little on
most coins, for weeks. A real trading bot that does this goes broke; a machine that keeps losing is
paid by someone, so its trades are fake demand.

Example for one coin: the dev pays the service 2 SOL. The service trades 50 SOL and loses 0.8 SOL
in fees and slippage, so it nets +1.2 SOL. The dev sells his bag into the busy coin. On the chart
we see only the service's -0.8.

A paid machine works for many devs, so it has **no tie to this owner** (no shared program,
structure or payer, and a small share of its coins are this owner's). It is found by what it is.

**Many real traders use machines too.** Trading apps offer multi-wallet buttons (one click buys from
3-10 wallets), snipers spread one buy over several wallets to land in the first slots, and traders
split size so scanners and copy-traders do not see one big holder. So a machine proves **one
operator**, never the owner. Whose money it is shows in the operator's result: a paid machine
loses, a real trader wins or breaks even.

Three steps, in order:

| ID | step | start value | why |
| --- | --- | --- | --- |
| M1 | **machine sign**: the wallet is in a **bundle** (3 or more wallets in one slot, one app, one side) or a **lockstep pair** (two wallets whose trades match one to one: same count of 2 or more, same sides in order, each within 0.5 s) on a group coin | snipers and the coin's first 3 slots left out | one script sent them |
| M2 | **one operator**: two machine-sign wallets that acted together and trade nearly the same market coins are sister wallets of one operator; a wallet with no sister is its own operator | the coins both trade are 80 % or more of the larger wallet's coins | one operator runs its wallets on the same coins; strangers who meet in one slot do not share their coin lists |
| M3 | **loses steadily**: the operator's wallets, added together, end below zero in the market | combined net SOL below zero over the last four weeks | a paid machine loses over time; a real bot wins over time, even with a bad fortnight |

An operator that passes M3 is owner: all its wallets, on all their coins.

- M1 groups a bundle by **app**, not exact structure: a service rotates small variants of one build
  (with or without a nonce or an extra instruction).
- M1 leaves out snipers: they bundle at birth and often lose too, and A3 already makes them outsiders.
- M2 judges the operator as one: its wallets win and lose by turns, so one wallet alone says nothing.
- An operator that **wins** is never owner: it is a real trader (section 5).

Examples.
- **Owner:** wallets 124302 and 124303 trade 6 times each on coin 9eLn on the same seconds (M1),
  trade the same ~110 market coins (M2), and lose 19-25 SOL each (M3).
- **Owner:** 3 wallets buy through Axiom in one slot of 9eLn (M1) and trade the same ~456 market
  coins (M2). Together they lose 35.8 SOL in one fortnight and win 10.2 in the other: -25.6 over the
  month (M3).
- **Outsider:** 4 wallets buy through Terminal in one slot (M1) and trade the same ~305 market coins
  (M2), but together they end +12.5 SOL (M3 fails). One trader with a multi-wallet button.
- **Outsider:** 3 sniper wallets buy 0.198 SOL each at slot +2 and trade the same ~3,600 coins, ending
  +575 SOL together.

---

## 4. Step 4 - CLEAN: judge each added set by its job

S and M can pull in wrong things (other bots, copy traders, a service payer under its threshold).
Everything step 3 added, not A or T, is checked as a **set**: one structure, one payer, or all the
wallets one rule added. Never one wallet: the owner splits jobs, and a buyer wallet passes its
tokens to a seller wallet by transfer, so alone it looks like it still holds.

**The owner has three jobs. Only the seller is judged by how it exits.**

| job | what the set does | judged by |
| --- | --- | --- |
| seller | sells, and makes the fall | C1 and C2 |
| buyer | buys and hands the tokens on; never sells | T rules only |
| volume machine | trades both ways, ends empty, loses steadily | M rules only |

| ID | check | start value | why |
| --- | --- | --- | --- |
| C1 | **sells first, never late.** A sold token is late when sold after the main fall begins, unless it is sold while its drop slot had fallen less than half its way (those first sells make the drop) | 40 % or less of its sold tokens are late; a set with no sells passes | the owner's sells make the fall; outsiders sell after it, at the lower price |
| C2 | **empty when the main fall ends** (right after a one-shot dump, at the bottom of a waterfall) | 10 % or less of its biggest bag before the fall | the owner has cashed out; an outsider is still holding |

A set **stays** when C1 and C2 both pass on 60 % or more of its coins with a fall, over at least 5
coins. A set with fewer stays untested. When a rule's set fails, the rule is dropped, its wallets
leave the owner side, and step 3 runs again without it.

Example: a fast exit bot sells its whole bag 0.4 s after the dump. It ends empty like the owner,
but all its sold tokens are late: it fails C1.

Snipers are never judged by C1 and C2: they sell early into the first buyers, so their sells can
land in a drop too. A3 makes them outsiders.

**A one-key link is judged at its source.** The wallets S2 (users of an owner structure), S4 (paid
for by an owner payer) and S5 (inside an owner wallet's transaction) add do the buyer and volume
jobs, so as a set they never pass C1-C2, though one key holder sits behind each. The structure (S1)
and the payer (S3) that bring them in are judged; the wallet sets are not. Measured: on 5ix the S4
and S5 sets pass 14-17 % while their wallets carry the multi-wallet sellers' money, and judging them
drops the split from 100 % to 59 % of that money; on 7ix the structure users pass 47 %.

**The calibration sets.** Tune C1 and C2 so that the sure owner (A wallets) passes and a known
outsider fails. The known outsider is **a wide bot that wins**: a wallet on 100 or more coins that
ends the window ahead (net SOL above zero, behind on fewer than half its coins), leaving out every
wallet that ever used the owner's program or created a coin. A wide bot that loses is no reference:
it may be a paid machine.

---

## 5. Outsider kinds

Real money comes in kinds that behave differently. A reading of "who arrived" names the kind. A
machine operator that loses steadily is not here: it is M, owner. Any of these kinds may trade
through several wallets at once; that makes it a machine, not the owner.

| kind | how it is recognised | how it behaves |
| --- | --- | --- |
| sniper | buys in the first slots of many unrelated coins every day (A3) | buys at birth, sells into the first buyers |
| button buyer | buys a size an app offers as a button: 0.1 / 0.5 / 1 SOL after the venue fee (0.099, 0.494, 0.988), or a dollar button at the day's SOL price; a size 15 or more traders used in the same hour | a person clicking: arrives when the coin shows up in their app, sells by hand |
| bottom-fisher | one buy of 1-3 SOL on many coins (about 30 a day), out within minutes, ahead on most coins | a buy after a quiet spell can start a wave of others, which it sells into |
| herd bot | many coins a day, joins within seconds of a big buy, ends empty, market result around even | follows prints and price within a slot or two |
| racer | pays a high priority fee to land first (1.3 SOL on one buy) | a trader buying a moment it believes in; no owner pays that to make volume |

---

## 6. What changes per chart shape

The rules A, T, S and M read who traded and how, never the chart, so they hold for every shape.
Two things read the chart and are re-tuned per group: the clean checks C1-C2 (they read falls) and
the chart checks V2-V5 (their start values assume an owner who makes most of the trades).

| shape | what a fall is | C1 and C2 | chart checks V2-V5 |
| --- | --- | --- | --- |
| steady rise, one-shot dump (7ix) | the one dump | on the main fall | start values as written |
| waterfall dump | the drops less than 30 s apart, read as one fall | C1 on every step down; C2 at the bottom | as written |
| many up-down swings | each fall | C1 on every fall; C2 only on the last fall, since the owner may buy again after a fall (a second pump) | lower V2 and raise V3 on coins with heavy outside trading |
| slow bleed, no dump | none | untestable; the set stays on A, T, S, M and V1 alone | lower V2 |
| heavy outside trading | as above | as above | each group sets its own values from its coins that pass C1-C2, V5 and the eye check |

A group with many swings also shows **real swings**: stretches where real traders take over (a big
buy at the bottom, a herd of bots behind it). There the outsider line follows the chart, and that is
not a split error (V6).

---

## 7. Step 5 - VERIFY

| ID | check | good when (start value) | why |
| --- | --- | --- | --- |
| V1 | **owner money, per coin**: all SOL the owner side put in and took out, plus its leftover tokens at the last price; the same for outsiders, read by kind | the owner ends behind the outsiders on 30 % of coins or fewer, and its usual loss is 10 % of its money or less | the owner runs the coin to take outsiders' money |
| V2 | share of all price movement made by owner trades | 80 % or more | the owner's trades alone redraw the chart |
| V3 | share of the coin's seconds in which outsiders move 0.5 SOL or more | 20 % or less | real outsiders come rarely and in bursts |
| V4 | share of outsider SOL in their busiest 10 % of seconds | 50 % or more | bursts, not a stream |
| V5 | an "outsider" wallet or structure trading like a script (10 or more 3 s windows in a row, similar sizes) | none; each one found is reviewed as a missed owner | real people do not trade every few seconds for minutes |
| V6 | **eye check**: price, owner flow and outsider flow on one time axis, for a random sample plus every coin that fails V1-V5 | the owner line has the chart's shape; the outsider line is flat with a few steps | the user's check |
| V7 | **old days**: the lists built on recent days, used on the 2 weeks before | they still find the owner (wallets they never saw included); less than 2 % of outsider money is marked owner; V1-V5 pass | rules must hold on days they never saw |
| V8 | **luck**: shuffle who traded inside each coin (keep the times) and run each rule again | the rule finds far more on the real tape | a rule that luck reproduces is dropped |

Notes:
- **V1 never judges one wallet.** A buyer wallet loses by design while a seller wallet collects.
  Normal: the owner ends ahead. Also normal: the owner loses a little because smart outsiders (often
  snipers) sold early.
- **V3 counts money, never presence**: many tiny retail trades fill the seconds while carrying little.
- **V6, three readings.** Bad split: the outsider line is busy and smooth all the time (a script
  was counted as outsider), or the chart moves where the owner line does not (part of the owner is
  missing). **Not a bad split: a real swing**, where the outsider line follows the chart for a
  stretch. Tell it from a missed owner by the money: the owner does not sell into it, and the
  tokens sold in it were bought in it.

**Rebuild every day** from the last ~2 weeks: wallets change fast, structures and payers slowly.

---

## 8. Use live - one decision per trade

Go down the list; the first line that matches decides. Once a trader is owner or outsider on a coin,
it stays that, except where a line says it changes from then on.

| order | the trade | result | rule |
| --- | --- | --- | --- |
| 1 | from the creator, or inside the create transaction | owner | A1, A2 |
| 2 | a creation-slot buy by a trader that is not a sniper | owner | A3 |
| 3 | with an owner-only build, or with an owner structure | owner (even on a brand-new wallet's first trade) | A4, S1 |
| 4 | a sell that takes the trader's bag on this coin below zero | owner, and its later trades on this coin | T1 |
| 5 | from a wallet on the buy-only machine list | owner | T2 |
| 6 | paid for by an owner payer | owner | S3 |
| 7 | from an owner wallet (owner wallets, transfer pairs, wallets of losing machine operators) | owner | S2, S4, S6, T3, M3 |
| 8 | in one transaction with an owner wallet's trade | owner | S5 |
| 9 | anything else, snipers included | outsider | |

In the engine, a rule reads structures (lines 1, 3, 8) and the bag (line 4), which the engine
already keeps per wallet, never wallet lists: a wallet is never a term in a rule
([_!___strategy.md](_!___strategy.md) T5). Lines 2, 5, 6 and 7 read lists built daily (snipers,
buy-only machines, owner payers, owner wallets including losing machine operators); they are support
while we measure what each adds. Live, line 4 turns a trader owner only from its first sell below
zero.

---

## 9. Where the split stays blind

- **A one-time hidden wallet**: fresh, through a public app, at the app's default fee, paid by itself
  or a service payer, that buys once and sells what it bought. Nothing on the trade ties it to the
  owner. It is caught only by V5 (a steady outsider flow), by its later trades (S5, or an owner
  structure or payer), or by funding (where its SOL came from), which we do not hold: reading it
  costs Helius calls, which need approval.
- **A buyer whose tokens go to a seller that also bought** on the coin: the seller's sells look
  covered, so T1 and T3 miss both.
- **A buying machine kept under 10 coins** (T2).
- **A paid machine's solo wallet**: with no bundle and no lockstep partner (M1) it looks like a herd
  bot in every market number (coins, ends empty, net result).
- **A public structure carries no ownership**: when the owner's machine and real users share one
  app build (a plain Axiom buy), the build cannot tell them apart; only M1-M3 on the wallet can.
- **A group whose builds are all public**: A4 finds nothing, and the split rests on A1-A3, T, S3-S5
  and M.

---

## Appendix A - tested and not used

| idea | what it is | why not |
| --- | --- | --- |
| hand-off | an owner wallet sells tokens and this wallet buys the same amount (within 5 %) within 3 s, 3 or more times | luck: 81 wallets on the real tape, 73-81 on shuffled tapes |
| one transaction buys and sells the coin | a wash inside one transaction | its wallets fail C1-C2 as a set (10 % / 50 % of coins) |
| mirror | in one slot an owner wallet buys and this wallet sells about the same SOL, 3 or more times | fails C1-C2 as a set (50 %) and pulls in wide bots on old days |
| acts together with an owner wallet | always a fixed time after an owner wallet, or in the same slot, on 5 or more coins, 10 or more times chance | real coordination, but followers: pass C1-C2 as a set on 28-32 % of coins against 66 % for the crew's wallets. Two outsiders in exact lockstep with each other are M2, a different test |
| trades only the owner's coins | 80 % or more of its market coins are the owner's | followers again: pass on 15 % of coins |
| loses in the market, alone | 10 or more coins, ends empty, behind on most, but no bundle and no lockstep partner | real herd bots look the same: it marks 13 % of a real herd's money |
| a bundle operator focused on the owner's coins | 3 % or more of its coins are the owner's (10 times the market rate) | a paid service works many devs, so its focus on one is low (1 % for 9eLn's service); loss is the test |
| a machine judged one wallet at a time | each wallet's own market result | one operator's wallets win and lose by turns: 4 Terminal sister wallets came out 2 owner, 2 outsider |
| operators joined by any shared bundle or lockstep | link two wallets whenever they meet in a machine, or meet on 2 or more coins | strangers who trade thousands of coins meet by chance and chain into one group of 300-900 wallets; on old days 58 % of the winning-bot reference was marked owner |
| one loser decides the group | the group's summed result over a group of strangers | one wallet at -377 SOL turns 297 wallets owner |
| loses in one half only | combined net below zero in the last two weeks | real bots have bad fortnights: on old coins the recent lists then mark 7.4 % of outsider money owner, 650 SOL of it from wallets that won there |
| loses in both halves | combined net below zero in each two-week half | too strict: a machine that rode one good fortnight escapes (on 9eLn 81 SOL, 23 % of the coin) |
| sister by coin list alone | a wallet whose market coins match an owner operator's wallet, with no shared bundle or lockstep | a service does not keep its wallets on one coin list: 2-5 wallets found in a window |
| per-coin structure share | on one coin, a build whose money is 80 %+ owner makes its other trades there owner | 0.1 % of all money; on 9eLn 42 % to 44 % |
| the machines' client list | a wallet whose market coins are mostly coins the losing machines also work | the machines touch about 40,000 coins, every active one: winning bots score as high as the owner |
| pool wallet: shares coins with this coin's machines (20x chance or more) and loses over the month | per coin | sweeps in 6,576 wallets that look like retail (7.5 coins a day, 0.08 SOL buys, 25 % button sizes, daily sleep of 8 h); they lose 217 SOL on the crew's coins, and the outsiders' typical loss halves. Volume makes coins trend and retail chases trending coins, so retail shares the machines' coins too |
| trades while the coin is quiet | a trade far from other trades and price moves | on a busy coin there is no quiet: on 9eLn every group trades 0.1-0.4 s after another trader |
| a buy after 2 or more empty slots | a buy that reacts to no print (a bot reacts 1-2 slots after one), so the dev buys to wake the coin | people click 1-5 s after the chart moves, and many bots fire on age or market cap: 27 % of small wallets' buys land after 2+ empty slots, against 34 % of the 9ddjzq program's; 78 % of quiet buys are owner against 75 % of all buys |
| restart kick | a buy after 2+ empty slots at a bottom (price 30 %+ under the coin's high so far), after the owner has sold | at a bottom a quiet buy is owner as often as any buy (25 % against 26 %); the outsider kickers are 1,192 wallets, 31 of them on 3 or more coins |
| build loyalty with fees in the key | A4 on the exact structure (ix list + CU price + tip, with or without the CU limit) | 7ix with `9ddjzq` hidden, at 30 %: marks 6.1 % (exact) and 10.3 % (CU limit free) of outsider money, against 0.00 % for the ix list alone: fee variants of plain builds mix in public users |
| build loyalty at 80 % | A4 with 80 % of the build's market trades on the group's coins | 7ix with `9ddjzq` hidden: finds 1-15 % of owner SOL, against 86 % at 30 %; the crew program also works coins of other launch builds (706 coins, 418 of them 7ix). Between 20 % and 50 % the result is flat |
| a buy in slot +1 by a non-sniper | the creation slot widened by one: apps show a coin only from slot +2 (app buyers among non-snipers: 1 % in slot 0, 9 % in slot +1, 24 % in slot +2) | adds 0 SOL on 7ix (the crew's bundle sits in slot 0); on 5ix those buyers pass C1-C2 on 31 % of coins: outsiders |
| sniper breadth counted per day | A3 with 20 coins and 10 creators that day | a sniper with quiet days passes as owner: on 5ix 16 winning wide bots, 19 % of their money marked owner, against 1.4 % with breadth over the window |
| C1 as "most sells land inside drops" | | volume wallets sell all through the coin's life; the crew's own wallets meet it on 24 % of coins |
| C1-C2 on one wallet | | 54 % of the crew's own program wallets fail alone |
| V3 counting presence | seconds in which outsiders trade at all | tiny retail trades fill half the seconds |
| any of these alone | the app used, equal amounts, equal fees, back-to-back landing, one shared bundle, lockstep, a shared service payer | other bot groups and copy traders do all of these. A bundle or lockstep is owner only with a steady loss (M1, M2) |

**A human sleeps.** Over two weeks, people leave a daily gap of several hours with no trade; trading
bots do not. Median daily silence: 9eLn's plain-Axiom outsiders 7 h, retail swept by the pool idea
8 h; winning bots 2 h, the Eu8n herd 3 h, multi-wallet traders 0 h. Paid machines rotate wallets
across the day, so their single wallets sleep too (median 6 h): silence tells a person from a bot,
but not a paid machine from a person.

"Chance": how often two unrelated wallets meet by luck. With 1,000 coins, wallet A on 50 and wallet
B on 40 meet on about 50 x 40 / 1,000 = 2 coins. Meeting on 35 is not luck, but copy bots that follow
the owner meet it on 35 too. Beating chance proves coordination, not ownership.

---

## Appendix B - measured

On 7ix (case file: [node-derivation/launch-group-7ix.md](node-derivation/launch-group-7ix.md)),
built on 09-15 .. 10-01, checked on 09-02 .. 14:

| what | result |
| --- | --- |
| the split | 1,333 coins (the old instruction list on any day, the new one with CU price 1000 from 09-28); owner 93.6 % of SOL; owner ahead on 97 % of coins; V2 passes on 82 % of coins (median 94 %); V3 median 1.6 % of seconds |
| old days (V7) | owner 93.1 % against 94.2 % for a split built there; 1.77 % of outsider money marked owner (186 SOL) |
| outsider reference | wide bots that win over the month marked owner on 1.1-2.1 % of their money, none of it by M |
| match levels (S1) | a list of exact structures built on one half of the month finds 81 % of owner SOL on the other half one way and 53 % the other; the core with all 7 levels finds 93 % and 89 %, adding 0.00-0.05 % of outsider money. The crew program rotates up to 201-549 exact variants of one core. Owner structures passing on 09-15 .. 10-01: level 1: 6, level 2: 16, level 3: 135, level 4: 12, level 5: 272, level 6: 367, exact: 448 |
| calibration of C1-C2, judged as sets | crew program wallets 66 %, creators 90 %, creation-slot owner buys 70 %; snipers 44 %, wide bots 4 %; T1 wallets 35-45 % (they sell all through the coin's life, so T sets are never judged by C) |
| few structures carry the owner | 372 instruction lists; the top 10 carry 83 % of its SOL; 161 used only by the owner carry 95 % |
| structures outlive wallets | the 09-01 .. 14 structures bring in 859 unseen wallets on 09-15 .. 27 (16,000 SOL) and 0.4 % outsider money |
| a wallet has a few jobs | an owner wallet uses 3 structures (median; 8 at p90); its main one covers 59 % of its trades |
| fee settings expose plain trades | 39 plain Pump.Fun structures with a particular CU price are mostly owner: 2,776 SOL |
| transfers (T1) | wallets selling more than they bought move 6.2-6.3 % of all SOL; 88-95 % already owner by other rules; the rest (550-1,000 SOL) is the crew's plain sell script at new CU prices |
| buying machines (T2) | 283 market wallets buy 10 or more coins and never sell (28,600 SOL in two weeks); 30-39 trade the crew's coins (94-200 SOL there) |
| transfer pairs (T3) | a shortfall finds a matching holder on the same coin 21-35 % of the time; on a random other coin 1.5-1.9 % |
| machine operators (M) | 09-15 .. 10-01: 1,599 operators with a machine sign; 1,138 lose steadily (1,263 wallets), 461 do not and stay outsiders |
| sister wallets (M2) | of 5,947 machine-linked pairs, 1,596 trade 80 % or more of the same market coins |
| test coin 9eLn (09-25) | M moves its owner share from 9 % to 42 %. The Axiom money left outsider (81 wallets, 74 SOL) looks like app users: button sizes (median 0.20 SOL), first buy at 52 s (owner Axiom wallets: 0.78 SOL at 30 s), 2 trades, about 14 coins a day, -0.02 SOL a coin, 15 % with a machine sign |
| test coin Eu8n, second swing (09-20, age 146-568 s) | two outsiders buy 7.9 and 3.0 SOL at the bottom, 58 bots follow within a minute, the owner sells 1.3 SOL; M marks 6 % of it owner: a real swing |
| test coin 9wyA (09-24) | 4 Terminal sister wallets (+12.5 SOL together) and two early flip bots (+236 and +485 SOL on 6,055 and 8,853 coins) stay outsiders |
| test coin FohR (09-28, old instruction list) | outside the coin set until the list filter took the old list on every day; now 94.7 % owner. Wallet 4CPn (one coin in the month) is owner through its fee setting (S1 level 3 with one narrow wallet) |
| test coin 2FW3 (09-20) | two sniper operators at slot +2 (3 wallets, +575 SOL; 5 wallets, +311 SOL) stay outsiders |
| money per coin (09-01 .. 14, earlier split) | crew wallets +3.81 SOL median, ahead on 69 %; creator +0.74, ahead on 99 %; one crew group -4.44 a coin (its job is buying) |
| wallets go stale | wallet groups from 09-01 .. 14 carry 50-66 % of the crew program's SOL on most later days, 35-36 % on 09-25 and 09-27 |

**A4 on a group with no known program.** Coins born 09-17 .. 30, market facts from every trade of
those days. "Answer-key owner" is the owner named without the split: on 7ix the full split built
with `9ddjzq`; on 5ix (`Create_v2, ATA CreateIdempotent, BuyV2` at CU price 6,666,666, 2,313 coins,
150,248 SOL) the traders of sell transactions carrying 2 or more wallets, on 2 or more coins, plus
the creator and the create transaction.

| what | 7ix, `9ddjzq` hidden | 5ix |
| --- | ---: | ---: |
| answer-key owner SOL found by MARK without A4 | 17 % | 56 % |
| ... with A4 | 86-92 % | 90 % (the answer-key wallets' buys: 49 % -> 96 %) |
| ... after GROW without A4 | 23 % | 100 % (through S3-S5) |
| ... final split without A4, one-key links judged by C1-C2 | 23 % | 59 % |
| ... final split with A4, one-key links judged at the source | 99.8 % | 100 % |
| outsider money A4 marks | 0.00 % (7ix answer key) | 0.00 % of the winning wide bots' money |
| winning wide bots' money in the final split (sniper breadth over the window) | 3.0 % (the answer key: 4.1 %) | 1.5 % |
| A4 built on one week, used on the other | 86 %, 0 SOL outsider | |
| owner instruction lists found | 22 (19 carry `9ddjzq`, one is the create transaction) | 41 programs, 40 plain lists |
| final split: owner ahead of outsiders / V2 median / V2 >= 80 % | | 98 % of coins / 0.95 / 93 % of coins (without A4: 98 % / 0.70 / 34 %) |

The 7ix split is the `7ix owner split` fingerprints (one per instruction list): the `volume` tag
holds `9ddjzq`, the creator, the owner structures as 167 core rows and 85 exact rows ("A level is
a tag row", section 3) and 6,861 owner wallets (sticky).

The 5ix split is the `5ix owner split` fingerprint: its `volume` tag holds the 41 programs, the 40
lists, the creator and 18,447 owner wallets (sticky). It agrees with the split on 99.7 % of SOL
(100 % of the owner SOL, 3.9 % of the outsider SOL), and 99 % of coins are within 5 points of the
split's owner share.

A tag reads forward: sticky marks a wallet's trades only from its first tagged trade on. The split
labels the wallet's whole history on the coin. So the wallet list holds every owner wallet with an
owner trade before the first trade a build matcher catches. A wallet the list leaves out keeps
those trades on the outsider side while its later sells go to the owner. Example: on
`Ad8zvok8gZebkyLYCWP63Jj9xeopiVTR5773Tndwpump`, four bundle wallets buy 5 SOL in the creation slot
and later sell in the owner's batch. Without them in the list, the outsider side has 14.5 SOL of
46.4 SOL; with them, 2.1 SOL. Then the two net lines look alike, though the owner trades most of
the money. The list holds the creation-slot owner wallets, so the snipers in that slot stay
outside the tag.

On 6ix: one network of 2,136 wallets trades 84 % of all 6ix coins and makes 58 % of their volume;
107 creators are its own wallets. It ends about even (-0.01 SOL a coin); the creator is ahead on
83 % of coins, outsiders on 9 %.

On the whole market, 09-26 (28,819 coins created):

| what | result |
| --- | --- |
| creation-slot money | creators buy 80,861 SOL; other transactions in the creation slot 77,772 SOL (55,787 buys on 15,627 coins) |
| snipers in it | buyers on more than 50 coins that day make 34,135 of those buys (33,707 SOL, median 0.62 SOL); buyers on 50 or fewer make 21,724 (44,132 SOL, median 1.3-1.4 SOL) |
| payers | 234,149 trades paid by another account; three service payers pay on 3,405-4,999 coins each; 3,678 payers pay for 6-20 wallets on a median of 1 coin (one operator each) |
