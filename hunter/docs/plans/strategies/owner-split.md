# Owner split: whose money is each trade

**What this file does.** On a coin, some trades are the owner's own money (the dev and the volume
wallets it controls) and some are outsiders' money (real buyers and sellers the owner trades
against). This file is the method to tell them apart, trade by trade. Every later number (owner
profit, outsider buying, the dump, the moment outsiders arrive) depends on it. The next step,
reading the owner's decisions, is [owner-decisions.md](owner-decisions.md).

The method works for any launch group (7ix, 6ix, ...). Numbers measured so far are in section 10.

## 0. Words

| word | meaning |
| --- | --- |
| trade | one buy or sell on the coin |
| structure | how a trade's transaction is built: its exact list of instructions, plus its CU limit, CU price and tip. Two trades made by the same script have the same structure |
| owner | the dev and every wallet it controls. There are only two sides: owner and outsider |
| outsider | everyone who is not the owner. Outsiders come in kinds (section 7): snipers, button buyers, other bots |
| sniper | a bot that buys brand-new coins in their first slots, on many unrelated coins every day. A sniper is an **outsider** |
| creation slot | the slot of the transaction that creates the coin |
| wallet | the account a trade is credited to (`trades.wallet_id`) |
| payer | the account that signed and paid the fee for the transaction (`trades.payer_id`) |
| routing wallet | a wallet that belongs to a trading service, not to a trader: the service routes its users' trades through it, so thousands of unrelated people share it. Marked `wallet_dict.is_proxy` (and `trades.is_proxied` per row) |
| service payer | a payer that belongs to a trading service: it pays the fee for its users' wallets, so thousands of unrelated wallets share it |
| trader | who really made the trade: the wallet, except when the wallet is a routing wallet; then it is the payer |
| owner wallets, owner structures, owner payers | the three lists this method builds |
| bag | the tokens someone holds right now |
| drop | a slot where the price falls 10 % or more |
| fall | one drop (a one-shot dump), or several drops less than 30 s apart (a waterfall) |

Numbers marked **start value** are first guesses, to be tuned on data. Every rule that names a
"wallet" reads the **trader**, so a routing wallet never ties unrelated people together.

---

## 1. The idea

The owner uses **many wallets but only a few structures and a few payers**. It runs scripts, and a
script keeps the same transaction shape and fee settings while its wallets change. A script also
pays its fees from a key it controls.

So the three lists help each other:

- a wallet that trades with an owner structure, or is paid for by an owner payer, is an owner wallet;
- a structure used mostly by owner wallets is an owner structure;
- a payer that pays mostly for owner wallets, on few coins, is an owner payer.

Example:

```
W1 is surely the owner (it created the coin).
W1 also sells with structure S7.  Almost all S7 trades come from owner wallets -> S7 is owner.
W8 is a brand-new wallet.  It sells with S7                                   -> W8 is owner.
W8's fee is paid by P3.  P3 pays for 40 wallets, all on this owner's 3 coins  -> P3 is owner.
W9 uses a public app (Axiom), but its fee is paid by P3                       -> W9 is owner.
... and so on, until nothing new is found.
```

## 2. How the owner works (why the method looks for these things)

| the owner wants | so it does | what we see |
| --- | --- | --- |
| to hold the supply cheap | buys at birth, with its own wallets, in the creation slot | big buys in the creation slot from wallets that do not snipe other coins |
| the coin to look busy | trades all the time | many trades, even when nobody else is trading |
| a steady rising chart | buys in steps, buys every dip | price goes up in steps and stays in a band |
| many holders | many small buys from many wallets | lots of fresh wallets with small, similar buys |
| to look like normal traders | trades through public apps (6Vo3, Axiom, ...) and through bot-like builds (nonce, seed accounts) | owner trades hide among thousands of real users of the same app |
| to run many coins cheaply | reuses the same scripts and the same fee key | the same structures and payers on every coin, at the same moments |
| to split the work | one script per job: buyer, seller, dumper | each structure has one job; a wallet uses a few of them |
| fake volume | trades with itself | one transaction that both buys and sells; tokens passed between its wallets |
| to hide | swaps in fresh wallets | new wallets appear, but on the old structures and the old payers |
| to cash out | sells when real buyers arrive | its sells make the price drop; real buyers sell later |

---

## 3. Step A - start from sure facts

These are the owner without any test:

1. **The creator**: the wallet that signs the create transaction.
2. **Every other wallet inside the create transaction.** One transaction is signed by one key holder,
   so a wallet that buys inside the create transaction is the creator's.
3. **Every buy in the creation slot, except a sniper's.** A brand-new coin is not visible to the
   public until it exists, so the money that lands in its own creation slot is the owner's bundle,
   apart from snipers, whose bots watch every new coin and buy it at once.

   **A creation-slot buyer is a sniper** when its trader buys in the creation slot or the next two
   slots of **many unrelated coins**. Start value: 20 or more coins that day, from 10 or more
   different creators, with 20 % or less of those coins being this owner's coins. A sniper is an
   outsider, and stays one on that coin.

   Example: in the creation slot of coin X, W4 buys 1.4 SOL and W5 buys 0.62 SOL. W4 bought in the
   first slots of 2 coins today, both from this creator: owner. W5 bought in the first slots of 1,690
   coins today, from hundreds of creators: sniper, so outsider. Size is a hint (owner bundle buys run
   larger) but never the test: many snipers buy 0.5-1 SOL.
4. **Every trade through a program only the owner uses** (7ix: the `9ddjzq` program).

---

## 4. Step B - grow the three lists

Repeat these rules until a round adds nothing new.

### A structure becomes an owner structure when rules 1 AND 2 hold, plus rule 3 OR rule 4

**Rule 1 - mostly owner.** Almost all trades with this structure come from owner wallets.
Start value: 95 % or more of its trades, from at least 5 different owner wallets.

**Rule 2 - not used by the rest of the market.** Look at every coin in the market, not only this
group. If the structure shows up mostly on the owner's coins, it is the owner's private script.
If it shows up on thousands of other coins, it is a public app setting (Axiom, GMGN, a plain
Pump.Fun trade with the default fee) that anybody uses, and it says nothing about who traded.
"The owner's coins" means every coin where a sure fact of step A appears, not only this launch
group. Start value: 80 % or more of its trades land on the owner's coins.

| structure | trades on the owner's coins | trades on all other coins | share | verdict |
| --- | ---: | ---: | ---: | --- |
| Pump.Fun Sell, CU 160,000, price 621,000 | 2,700 | 300 | 90 % | private: owner script |
| Axiom default buy | 4,800 | 550,000 | 0.9 % | public: never owner by itself |

**Owner settings inside a public app.** A public app's users get the app's **live default** fee: at
any minute most of that app's trades pay the same CU price and tip, and that value moves during the
day for everyone together. A script using the same app often keeps its **own fixed** CU price and
tip instead. So a public app's instruction list with a fee that differs from the app's default at
that minute is kept as its own structure and judged by rules 1-4 like any other.

Example: at 14:05 most 6Vo3 buys pay tip 3,063; at 14:20 most pay 2,101. A set of wallets buying
through 6Vo3 pays CU price 80,000 and tip 0 all day, whatever the default is. That (6Vo3 list, CU
80,000, tip 0) is its own structure, and if it lands mostly on the owner's coins it is an owner
structure.

**Rule 3 - runs on a timer.** On each coin it first appears at about the same age (a script started
by the launch). Start value: within 1 s of its usual age, on 60 % or more of its coins.

**Rule 4 - sister of an owner structure.** Most wallets that use it also use an owner structure
(for example the sell structure of a known buy structure). Start value: 50 % or more of its wallets.

### A payer becomes an owner payer when rules 5 AND 6 hold

Many services pay fees for their users, so a payer is a link only when it is **not** a service.

**Rule 5 - pays for owner wallets.** It paid for at least one trade of an owner wallet.

**Rule 6 - not a service payer.** A service payer pays for wallets on thousands of unrelated
coins; an owner payer pays for its own wallets on the owner's few coins. Start value: 80 % or more
of the trades it pays for land on the owner's coins, and it pays on fewer than 50 coins a day.

| payer (09-26) | wallets it paid for | coins | verdict |
| --- | ---: | ---: | --- |
| `AgmLJBMD...` | 10,071 | 3,405 | service: says nothing |
| `FHpcNSe6...` | 6,769 | 4,999 | service: says nothing |
| `gasTzr94...` | 645 | 853 | service: says nothing |
| `SPBXSWwo...` | 409 | 2 | one operator on 2 coins: owner payer when either coin is the owner's |

The routing case is the opposite one: `ARu4n5mF...` is a routing wallet (`is_proxy`). On 09-26 it
is the credited wallet for 4,882 different payers on 2,114 coins, so every rule reads those payers
as the traders, never `ARu4n5mF...` itself.

### A wallet becomes an owner wallet when any one rule holds

**Rule 7 - uses an owner structure.** One trade with an owner structure is enough.

**Rule 8 - paid for by an owner payer.** One trade whose fee an owner payer paid is enough. This is
the rule that finds owner wallets hiding inside public apps.

**Rule 9 - shares a transaction with an owner wallet.** One transaction that holds trades of
several wallets was signed by one key holder, so all its wallets are one operator: if one is owner,
all are. The common form is one transaction that sells for several wallets at once.

**Rule 10 - trades with itself.**
- One transaction both buys and sells this coin (once is enough), or
- hand-off: an owner wallet sells some tokens and this wallet buys the same amount (within 5 %)
  within 3 s, at least 3 times, or
- mirror: in the same slot, an owner wallet buys and this wallet sells about the same SOL
  (within 5 %), at least 3 times.

**Rule 11 - always acts together with an owner wallet, on many coins.** Either it always buys a
fixed time after an owner wallet (for example always 4-6 s after it), or it always trades in the
same slot as one. Start value: on at least 5 coins, and at least 10 times more often than chance.

"Chance" means: how often two unrelated wallets would meet by luck. Example: there are 1,000 coins;
wallet A trades 50 of them, wallet B trades 40. By luck they meet on about 50 x 40 / 1,000 = 2
coins. If they meet on 35 coins, B always 4-6 s after A, that is not luck. This matters because
popular sniper bots buy almost every new coin at the same age, and active traders meet each other
everywhere, so two unrelated wallets meet on many coins; the chance test removes them.

**Rule 12 - a fresh wallet born into the owner's job.** Its very first trade ever is on one of the
owner's coins, with an owner structure or paid by an owner payer. "Ever" means the history we hold:
at least the 14 days before that trade. A wallet with no trade in those 14 days counts as fresh.

---

## 5. Step C - throw out the mistakes

Step B can pull in wrong wallets, structures or payers (other bots, copy traders, a service payer
under its threshold). Each one it added (not the sure facts of step A) is checked on the coins where
it traded. The key: **the owner's sells make the price fall; outsiders sell after the fall.**

**Check 1 - sells first.** Most of its sells land inside a drop, before the price reaches the
bottom of that drop.
- One-shot dump: the owner's sells ARE the dump. Outsiders and fast exit bots sell after it: later
  in the same slot, or in the next slots, at the lower price.
- Waterfall: each step down is owner selling. Outsiders sell between the steps or at the end.
- Start value: 60 % or more of its sold tokens land inside drops.
- Example: a fast exit bot sells its whole bag 0.4 s after the dump, at the bottom. It ends empty
  like the owner, but its sells are after the drop: it fails check 1.

**Check 2 - empty when the fall is over.** When the fall ends (right after a one-shot dump, or at
the bottom of a waterfall), it holds almost nothing. Start value: 10 % or less of its biggest bag
before the fall.

A wallet, structure or payer stays when both checks pass on 60 % or more of its coins that had a
fall, over at least 5 coins.

Snipers are never judged by checks 1 and 2. A sniper sells early into the first buyers, so its
sells can land inside a drop too; what makes it an outsider is that it buys every new coin, not how
it sells (step A, fact 3).

**Check 3 - did the owner, as a whole, take the money home?** This check is for the whole owner
side, never for one wallet. For each coin add up all SOL that owner wallets put in and took out
(plus what their leftover tokens are worth), and the same for outsiders.
- Normal: the owner ends ahead and outsiders behind. Example: owner put in 20 SOL, took out 24
  (+4); outsiders put in 10, took out 6 (-4).
- Also normal: the owner loses a little, because smart outsiders sold early with a profit.
  Snipers are the usual case: they buy at birth and sell into the first buyers. Example: owner
  -0.6 SOL (3 % of its money), outsiders +0.6, all of it the snipers'.
- Wrong split: the owner side loses clearly on most coins. Start values: the owner ends behind the
  outsiders on more than 30 % of coins, or its usual loss is more than 10 % of its money.
- Why never one wallet: the owner splits jobs. A buyer wallet loses by design while a seller wallet
  collects the profit.
- Read the outsider side by kind (section 7), so the snipers' profit never hides what the other
  outsiders lost.

**Never enough on its own:** the app used (Axiom, Terminal, GMGN), equal amounts, equal fees,
trades landing back to back, one shared bundle, a shared service payer. Other bot groups and copy
traders do all of these.

---

## 6. Step D - check the result against the chart

**Why this works.** The price on the curve moves only when SOL goes in or out. If the owner makes
most of the trades, **the owner's trades alone redraw the chart**: its buys are the climbs, its
back-and-forth trading is the flat band, its sells are the dump. Real outsiders come **rarely and in
bursts** - a few buys at one moment, panic sells after a drop - never as a steady stream all
through the coin's life.

So, put three lines on one time axis: the price chart, the owner's flow, the outsiders' flow.

- **Good split:** the owner line looks like the chart; the outsider line is flat with a few spikes.
- **Bad split, sign 1:** the outsider line is busy and smooth all the time. A script was counted
  as outsider, because real people do not trade every second for minutes.
- **Bad split, sign 2:** the chart moves where the owner line does not. Part of the owner is missing.

The same thing as numbers, per coin:

| check | question | good when (start value) |
| --- | --- | --- |
| D1 | of all the price movement, how much is made by owner trades? | 80 % or more |
| D2 | in how many seconds of the coin's life do outsiders trade at all? | 20 % or less |
| D3 | do outsiders come in bursts? (share of outsider SOL in their busiest 10 % of seconds) | 50 % or more |
| D4 | is any "outsider" wallet or structure trading like a script? (10 or more 3 s windows in a row, similar sizes) | none; each one found is reviewed as a missed owner structure |

The start values fit a group whose owner makes most of the trades (7ix). A group whose coins draw
heavy outside trading fails D1 and D2 on a correct split, so each group sets its own values from its
coins whose split passes steps C and D4 and the eye check.

The eye check uses the same three lines on a chart page, for a random sample of coins plus every
coin that fails D1-D4.

---

## 7. Outsider kinds

Outsiders are not one crowd. Three kinds behave differently, and a reading of "who arrived" names
the kind.

| kind | how it is recognised | how it behaves |
| --- | --- | --- |
| sniper | buys in the first slots of many unrelated coins every day (step A, fact 3) | buys at birth, sells into the first buyers |
| button buyer | buys a size a public app offers as a button: 0.1 / 0.5 / 1 SOL after the venue fee (0.099, 0.494, 0.988), or a dollar button at that day's SOL price ($100 = 0.416 SOL at $240). Recognised as a size that 15 or more different traders used in the same hour | a person clicking: arrives when the coin shows up in their app, sells by hand |
| other bot | trades many coins a day with its own sizes and timing; meets other bots everywhere (section 10, look-alike bots) | reacts within a slot or two, to prints and to price |

---

## 8. Where the split stays blind

One case passes every rule as an outsider: **a fresh wallet, trading through a public app, with the
app's default fee, with its fee paid by itself or by a service payer, once.** Nothing on that trade
ties it to the owner. This is the owner's best hiding place, and it is caught only from outside the
trade:

- **Step D**: an "outsider" flow that is steady instead of bursty points at it.
- **Its later trades**: a hand-off, a mirror, a shared transaction (rules 9, 10), or its sells in the
  dump (check 1) mark it once it acts again.
- **Funding**: where its SOL came from and where it went. We do not hold transfers; reading them
  costs Helius calls, which need approval first.

---

## 9. Step E - decide each trade live

Go down the list; the first line that matches decides.

| order | the trade | result |
| --- | --- | --- |
| 1 | comes from the creator, or sits inside the create transaction | owner |
| 2 | is a creation-slot buy by a trader that is not a sniper | owner |
| 3 | uses an owner structure | owner (works even on a brand-new wallet's first trade) |
| 4 | is paid for by an owner payer | owner |
| 5 | comes from an owner wallet | owner |
| 6 | is one transaction that both buys and sells the coin, or holds an owner wallet's trade | owner |
| 7 | anything else, snipers included | outsider |

Once a trader is owner or outsider on a coin, it stays that on that coin.

In the engine, the rule reads structures (lines 1, 3, 6), not wallet lists: wallets change every
day, and a wallet is never a term in a rule ([_!___strategy.md](_!___strategy.md) T5). Lines 2, 4
and 5 read lists of traders and payers (the sniper list, the owner payers, the owner wallets),
built daily; they are support while we measure how much each adds.

---

## 10. Build, check, refresh

- **Where each fact lives.** Instructions, CU limit, CU price, tip, slot, block position and wallet:
  the lake and PG `trades`. Payer and routing (`payer_id`, `is_proxied`, `wallet_dict.is_proxy`):
  PG only, which keeps about 30 days; the lake has no payer column, so rules 5, 6 and 8 run on PG.
  Funding and token transfers between wallets: not held.
- **Build on recent days, check on old days.** Grow the lists on the last ~2 weeks. Then check them
  on the 2 weeks before, which they have never seen. They pass when, on those old days: they still
  find the owner (including wallets they never saw); less than 2 % of outsider money is marked owner;
  steps C and D pass.
- **Test every rule against luck.** Shuffle who-bought-what inside each coin (keep the times) and
  run the rule again. A rule that does not find far more on the real tape than on the shuffled one
  is dropped.
- **Rebuild every day** from the last ~2 weeks. Wallets change fast, structures and payers slowly.

## 11. Measured so far

On 7ix (the case file: [node-derivation/launch-group-7ix.md](node-derivation/launch-group-7ix.md)):

- **Few structures carry the owner.** On coins of 09-01 .. 09-14 the owner uses 372 instruction
  lists. The top 10 carry 83 % of its SOL. 161 lists are used by the owner only (99 % or more), and
  they carry 95 % of its SOL.
- **Structures outlive wallets.** On 09-15 .. 09-27, those same structures bring in 859 wallets never
  seen before (16,000 SOL), and almost no outsider money (0.4 %).
- **A wallet has a few jobs.** An owner wallet uses 3 structures (median; 8 at p90); its main one
  covers 59 % of its trades.
- **Fee settings expose plain trades.** 39 plain Pump.Fun structures with a particular CU price are
  mostly owner. They carry 2,776 SOL, of which the current 7ix tag counts 805 SOL as outsider money.
- **Wallet links alone find the owner.** Rule 11, run without knowing the crew's program, builds
  groups whose trades are 60-100 % the crew's program.
- **Sells-first and empty-after work** on a rougher form of checks 1 and 2: the crew's wallet groups
  pass on 67-100 % of their coins.
- **Money.** Per coin, the crew's wallets end +3.81 SOL (median) and ahead on 69 % of coins; the
  creator +0.74, ahead on 99 %; outsiders -1.21, ahead on 19 %. One crew group alone books -4.44
  SOL a coin (its job is buying), which is why check 3 never judges one wallet.
- **Look-alike bots are not the owner.** Trades with equal sizes, back-to-back landing or shared
  fees are real coordination (86-94 % above luck), but those wallets trade a median 513 other coins
  and are still holding when the dump hits 60-67 % of the time: other bot groups.
- **Wallets go stale.** Wallet groups found on 09-01 .. 09-14 still carry 50-66 % of the crew
  program's SOL on most later days, but only 35-36 % on 09-25 and 09-27.

On 6ix: one network of 2,136 wallets trades 84 % of all 6ix coins and makes 58 % of their volume;
107 of the creators are its own wallets. It ends about even (-0.01 SOL a coin), the creator is
ahead on 83 % of coins, outsiders on 9 %.

On the whole market, 09-26 (28,819 coins created):

- **The creation slot holds as much money as the creators' own buys.** Creators buy 80,861 SOL; other
  transactions in the creation slot buy 77,772 SOL (55,787 buys on 15,627 coins).
- **Snipers are a large part of it, and buy smaller.** Creation-slot buyers that trade more than 50
  coins that day make 34,135 of those buys (33,707 SOL, median 0.62 SOL). Buyers on 50 coins or
  fewer make 21,724 buys (44,132 SOL, median 1.3-1.4 SOL).
- **Payers split into services and operators.** 234,149 trades are paid by a payer other than their
  wallet (routing wallets left out). Three service payers pay for 3,405-4,999 coins each. 3,678
  payers pay for 6-20 wallets each, on a median of 1 coin: one operator running a set of wallets.
