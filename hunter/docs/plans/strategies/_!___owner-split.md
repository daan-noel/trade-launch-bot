# Owner split: whose money is each trade

Every trade on a coin is **owner** or **outsider**.

- **Owner** is fake demand: the dev, the dev's wallets, and the machines the dev hires.
- **Outsider** is real money: a person or a bot trading for itself.

Owner profit, outsider buying, the dump, and the owner's decisions in
[owner-decisions.md](owner-decisions.md) stand on this split.

Names in the tables are short on purpose. Each one is spelled out in [Terms](#terms) at the end.

## At a glance

Two times. [Build](#build) runs once, offline, over the launch group's coins: Mark, Grow and Check label every trade, and the passing rows and the wallets that come back are stored on the fingerprint. [While a rule runs](#while-a-rule-runs) only reads that stored tag. Nothing writes it while rules run. A stored row does not store the wallet, and a stored wallet does not add its other lists. A hand edit (a chart click, the Flow Discovery cart, the tags editor) changes the stored tag at the next rules reload and lasts until the next build replaces the tag whole. The build command is open work: [owner-split-builder.md](../../roadmap/owner-split-builder.md).

The pictures restate the rules below in one place. A number here and a number in a rule table are the same number; the table is the definition.

### Build

Workstation and lake only, never the live box.

```text
INPUT
  launch group = coins born from one creation ix structure, plus one CU price
                 or max_sol_cost
  every coin has ended, so the dump is on the tape
  every market trade of the build window (about two weeks)
  no owner wallet is known yet
        |
        v
+-------------------------------------------------------------------------------+
| STEP 1. MARK: label every trade on every coin                                 |
|   this coin = the wallet is owner on that coin only, never stored             |
|   stored    = written onto the fingerprint                                    |
+-------------------------------------------------------------------------------+
| LAUNCH  Creator ............ signer of the create                 owner       |
|         Same transaction ... other wallets in the create          owner       |
| BIRTH   Instant buy ........ buy 15 ms or less after the create   owner       |
|         Creation slot ...... later buy in that slot, wallet                   |
|                              under the sniper counts              owner       |
|         Creation-slot ...... 2+ wallets, buys 100 ms or less                  |
|           bundle             apart, in that slot                  owner       |
|         Bought twice ....... one wallet, 2+ buys in that slot     owner       |
|         Sniper ............. 20+ coins, 10+ creators, 20% or                  |
|                              less in the group, two weeks         OUTSIDER    |
|         Rise continues ..... big early buy, creator buys again    owner       |
|         Rise stops ......... big early buy the creator sells into OUTSIDER    |
| BUNDLE  Several sells ...... 2+ Pump.fun sells in one tx          owner       |
|         Several lists ...... 2+ buys within 100 ms on 2+                      |
|                              ix structures                        owner       |
|         Private list ....... 2+ wallets within 100 ms on one                  |
|                              private ix structure                 owner       |
|                              -> that ix structure is a row candidate          |
| REPEAT  Dominates .......... ix structure + CU price is 20%+ of               |
|                              trades or SOL over its run, 10+      owner       |
|         Many wallets ....... ix structure, 20+ trades, 2+ wallets owner       |
|         One wallet ......... one wallet, 20 uses                  OUTSIDER    |
| PAIR    Same slot .......... 1x buy and 2x buy in one slot, the               |
|                              smaller sells about 60 s later       owner       |
|                              -> on 20+ coins both wallets come back           |
| SCRIPT  Mostly here ........ 30%+ of the structure's trades in                |
|                              the group, 5+ coins, 2+ creators     owner       |
| WAKE    After silence ...... 2 s silence, then 5+ buys of one                 |
|                              structure by repeat buyers           owner       |
|         Used once .......... bought that structure once           OUTSIDER    |
| HIDDEN  Below zero ......... a sell takes the bag below zero                  |
|                              by more than 1% of the sell          owner       |
|                              -> on 2+ coins the wallet comes back             |
|         Same amount ........ A buys X and never sells, B sells                |
|                              X (0.5%) and never bought            owner       |
|         Never sells ........ 10+ coins, 20+ buys, no sell         owner       |
|                              -> the wallet comes back                         |
|         Matching holder .... the one wallet holding the                       |
|                              missing amount                       owner       |
|         Bag balances ....... bought what it sold                  OUTSIDER    |
| LOYAL   Stays in group ..... 3+ group coins, 80%+ of its coins                |
|                              in the group                         owner       |
|                              -> the wallet comes back                         |
|         Fresh cash-out ..... 4 coins or fewer ever, 3+ empty                  |
|                              their bag in one slot                owner       |
|         Wide service ....... too wide for 80%, old wallets        OUTSIDER    |
| VICTIM  Sold into .......... owners sell after its buy, it                    |
|                              exits under half its cost            OUTSIDER    |
+-------------------------------------------------------------------------------+
        |
        v
+-------------------------------------------------------------------------------+
| STEP 2. GROW: start from the marked wallets, find the next one                |
|   Backfill  Earlier buys ........ a marked wallet's earlier buys   owner      |
|             The list stays put .. its app list is not copied to anyone        |
|   Spread    Beside an owner ..... same tx as an owner wallet       owner      |
|             First trade ......... 14 days quiet, first trade on this          |
|                                   dev's coin through an owner ix              |
|                                   structure or ix core             owner      |
|   repeat until a round adds nobody (Stop)                                     |
|                                                                               |
|   NOT IN THE BUILD: Kept template, On a timer (need an ix template),          |
|                     Fee payer, Service payer (need the fee payer)             |
+-------------------------------------------------------------------------------+
        |
        v
+-------------------------------------------------------------------------------+
| STEP 3. CHECK: judge only a new ix structure or ix core Grow reuses           |
|   Stay  Mark's answers ...... stay as Mark left them                          |
|         This coin ........... a stored wallet stays; its list is not added    |
|   Test  a set = one ix structure, one ix core, or one rule's wallets          |
|         C1  40% or less of its sold tokens sold after the big drop starts     |
|         C2  10% or less of its biggest bag left at the drop's bottom          |
|         both on 60%+ of at least 5 coins with a drop                          |
|           pass ............ Cashed out ..... stays owner                      |
|           under 5 coins ... Too few coins .. stays owner, untested            |
|           fail ............ Held the bag ... OUTSIDER, Grow runs again        |
+-------------------------------------------------------------------------------+
        |
        v
+-------------------------------------------------------------------------------+
| STEP 4. CHOOSE THE ROWS: every ix structure read at seven match levels        |
|                                                                               |
|   level  row            matches on                                            |
|   1      ix core        core, side                                            |
|   2      ix core        core, side, marks                                     |
|   3      ix core        core, side, CU price                                  |
|   4      ix core        core, side, tip                                       |
|   5      ix core        core, side, CU price, tip                             |
|   6      ix core        core, side, marks, CU price, tip                      |
|   7      ix structure   exact list, CU limit, CU price, tip                   |
|                                                                               |
|   a row passes at 90%+ owner SOL on the group and 10+ trades                  |
|   one-coin rows pass too: Dominates (structure + CU price), Many wallets      |
|   keep the fewest rows that still cover every passing trade, biggest first    |
|   a plain shared core (a bare Pump.Fun: BUY) fails and stays out              |
+-------------------------------------------------------------------------------+
        |
        v
+-------------------------------------------------------------------------------+
| STEP 5. CHOOSE THE WALLETS: only wallets that come back                       |
|   Same slot (20 coins), Below zero (2 coins), Never sells, Stays in group     |
|   and latest trade on the group inside the recent window                      |
|   (the gap that covers 90% of returns, counted to the build's last coin)      |
|   never stored: a this-coin wallet, a birth-bundle wallet, a proxy            |
+-------------------------------------------------------------------------------+
        |
        v
+===============================================================================+
| WRITE tags.volume ON THE FINGERPRINT, ONCE, REPLACED WHOLE                    |
|   match.ix_shape  ix structure rows and ix core rows (level core)             |
|   match.wallet    the wallets that come back                                  |
|   match.creator   true (the flag; the address is not stored)                  |
|   sticky          true                                                        |
+===============================================================================+
```

### While a rule runs

The live bin and lab replay run the same engine.

```text
fingerprint loaded -> tag compiled once (ix_shape rows, wallet set, creator)
        |
        v
EACH TRADE, ONE PRINT
        |
        v
+-------------------------------------------------------------------------------+
| 1. The creator, a stored wallet, or a wallet already sticky on this coin?     |
+-------------------------------------------------------------------------------+
        | yes ---> OWNER
        | no
        v
+-------------------------------------------------------------------------------+
| 2. Its ix structure matches a stored row?                                     |
|    exact row  the same ordered list, plus its fee pins                        |
|    core row   the same ix core, plus its side, marks and fee pins             |
+-------------------------------------------------------------------------------+
        | yes ---> OWNER, and sticky starts for this wallet on this coin
        | no
        v
     OUTSIDER

Sticky covers this wallet's later trades on this coin only.
A trade already printed never changes.

Rules read the tag:  m_flow.buy_sol @volume   = owner money
                     m_flow.buy_sol @!volume  = outsider money (real demand)

Nothing is written: no row is added, no wallet is replaced.
Sticky lives in memory, for this coin only.
A stored tag ages; a rebuild reruns the Build offline.
```

### Flow Discovery auto-tag

A hand edit, not the Build. On the `volume` tag, Flow Discovery runs the [Birth](#birth) and [Hidden](#hidden) rules over the run's coins (`lab/src/strategies/owner_split.rs`, the thresholds below as its constants) and offers one toggle per rule, in two tabs:

| Tab | Toggle | Marks |
| --- | --- | --- |
| Creation slot | Instant buy | Instant buy |
| Creation slot | Creation slot | Creation slot, snipers left out |
| Creation slot | Bundle | Creation-slot bundle |
| Creation slot | Bought twice | Bought twice |
| Transfers | Below zero | Below zero; the group scope keeps only wallets on **2** or more coins |
| Transfers | Transfer pair | Same amount and Matching holder, both wallets |
| Transfers | Never sells | Never sells |

- A toggle adds its rule's wallets to the draft's `match.wallet` list, and a second click takes them out. A wallet another toggle that is on also marks stays. Apply saves the list with the sticky and creator switches.
- **This token** takes the picked coin's marks, **Group** every coin's. A this-coin mark lands in the stored wallet list on purpose: the user chooses to store it, and a toggle takes it out again.
- The sniper counts and Never sells read the market over the **14** days ending at the run's last trade. When that lake read fails, the Creation slot and Never sells toggles are off for the run, with a notice.
- The tooltips quote the thresholds the run applied, sent with the result.
- **Gap:** the [recent window](#terms) is not applied. A toggle stores a wallet whatever the date of its latest trade on the group.

## 1. Mark

This is the [Build](#build). Every coin has ended, so the dump is on the tape. A this-coin rule marks the wallet owner on that coin and does not store it. A row that passes [Match levels](#match-levels), or a wallet that comes back, is stored. No owner wallet is known at the start.

### Launch

The dev's wallets sit in the transaction that creates the coin.

| Rule | Side | Why | Explanation | Example |
| --- | --- | --- | --- | --- |
| Creator | **owner** | The wallet that creates the coin is the dev. | The wallet that signs the create is **owner** on that coin. | On DdTt, the wallet that signs the create is **owner** from the first trade through the dump. |
| Same transaction | **owner** | The dev packs a helper wallet into the create. It is the same person. | Every other wallet in that create transaction is **owner** on that coin. | The DdTt create also holds a second wallet. That wallet never trades DdTt again. It is **owner** on DdTt too. |

### Birth

In the first seconds the dev buys their own coin. A real buyer is either too late to see it, or so large that the dev sells into them and the rise stops.

| Rule | Side | Why | Explanation | Example |
| --- | --- | --- | --- | --- |
| Instant buy | **owner** | A real buyer cannot see the coin 15 ms after it is created. That buy is the dev. | A buy within **15 ms** of the create is **owner**. Size and **ix structure** are ignored. | A buy lands 10 ms after the create, in the creation slot, through a public `Pump.Fun: BUY`. That buy and that wallet are **owner** on this coin. |
| Creation slot | **owner** | Apps show the coin only after the creation slot. A buy inside that slot is the dev filling the launch. | A later buy in the creation slot is **owner** when the wallet is under the sniper counts in the next row. | A buy lands later in the same slot as the create. The wallet has opened 6 coins in two weeks. The buy is **owner**. |
| Creation-slot bundle | **owner** | The dev packs the first buys with the create so a stranger cannot land between them. | Two or more wallets buy in the creation slot, and each buy lands within **100 ms** of another buy in that pack. Every wallet in the pack is **owner** on this coin. Sticky covers the rest of each wallet here. The wallets are not stored. The **ix structures** are not stored. | On DL744, seven buys land within 84 ms in the creation slot, on five ix structures. All seven wallets are **owner**. |
| Bought twice | **owner** | A sniper fires once at the open. A second buy in that same slot is the dev. | The same wallet buys more than once in the creation slot. That wallet is **owner** on this coin, and sticky covers the rest of its trades here. The wallet is not stored. A wallet that buys many times after the creation slot stays unmatched here. | A wallet sends two buys in the creation slot. Both buys and that wallet are **owner**. 8dtx and omego buy many times after the creation slot, so those later buys stay unmatched here. |
| Sniper | **outsider** | This bot opens many launches from many creators. It hunts the market. It is not running this coin. | A bot that buys the opening of **20** or more coins, from **10** or more creators, with at most **20%** of them in this **launch group**, over two weeks, is **outsider**. A buy inside a creation-slot bundle, and a second buy in the creation slot, stay out of this row. | A bot buys the creation slot of 40 coins in two weeks, from 15 creators. Only 2 of those coins are this launch group. Those single buys stay **outsider**. |
| Rise continues | **owner** | The dev plants a big early buy that looks like a sniper, then keeps buying. A real buy that large is the profit the dev sells into. | A big buy in the first seconds after the creation slot is **owner** when the creator buys again after it. The **ix structure** is ignored. | On DdTt, slot 453178755, 1.5 s after the create, two buys of 1.66 SOL and 1.48 SOL use `AdvanceNonceAccount`, `Axiom Trade: ix#05`, `Axiom Trade: ix#00`, `Transfer`. The creator buys again after them and the coin rises to 78.6 SOL. Those buys are **owner**. |
| Rise stops | **outsider** | The dev sells into a big early buy and the rise ends. That buyer was real money. | A big buy the creator sells into is **outsider**. | A 2 SOL buy lands 2 s after the create. The creator sells into it and the rise ends. That buy is **outsider**. |

### Bundle

The dev packs buys so a stranger cannot land between them and eat the volume. He mixes the wallets, the ix structures, and the order inside the pack so one wallet or one list cannot be followed. He packs sells so the dump looks like a crowd.

| Rule | Side | Why | Explanation | Example |
| --- | --- | --- | --- | --- |
| Several sells | **owner** | One person packed those sells. Real sellers each land in their own transaction. | Two or more Pump.fun sells in one transaction are one person. Every wallet in it is **owner**, and each wallet's earlier buys on this coin are **owner** too. Two sells or ten is the same. | One transaction holds 12 `Pump.Fun: SELL` from 12 wallets, plus a SOL transfer and a compute-budget instruction. All 12 wallets are **owner** on this coin, including the buys they made earlier here. One sell in a transaction stays unmatched. |
| Several lists | **owner** | One person sent the pack. He uses more than one ix structure so a watcher of one list sees only a slice. He also changes which wallet goes first and which wallet carries the large size. | Two or more buys land within **100 ms** and use more than one **ix structure**. Every wallet in that pack is **owner** on this coin. Sticky covers the rest of each wallet here. The wallets are not stored. The plain list is not stored. A private list in the pack is stored by the next row. A pack that uses one ix structure stays unmatched here. A tool that packs many wallets onto one public list stays unmatched here. | On 3fm2, slot 455796752, three buys of 0.0988 SOL on one list and one buy on a private list land within 50 ms. The order of those wallets is not fixed from one pack to the next. On 5hkAo, a `9ddjzq` buy and `9nH7`'s plain Pump buy land on the same millisecond. Those buys are **owner** on this coin. The plain list is not stored. |
| Private list | **owner** | A normal trader does not send this list, and does not send a second wallet with it. | Two or more wallets buy within **100 ms** on one **ix structure**, and that list is a private tool: a nonce, a seed account, an unknown program, or a Pump.fun instruction with no public name. Those buys are **owner**. The **ix structure** is stored. The wallets are not stored. A public button, including Axiom, stays unmatched here. | On DL744, slot 455417285, `CZ7Ho6` and `3ZkLVZ` buy 2 ms apart on `AdvanceNonceAccount`, `CreateAccountWithSeed`, `InitializeAccount3`, `Pump.Fun: ix#e1f7501ed5b38488`. That list is stored. The two wallets are not. |

### Repeat

The dev's volume is one **ix structure**, many times, from many wallets. One person repeating their own list is a trader.

| Rule | Side | Why | Explanation | Example |
| --- | --- | --- | --- | --- |
| Dominates | **owner** | The dev's script is a large share of this coin while that script is active. The share is the trades. One use does not make the person the dev. | The **ix structure** plus its CU price is at least **20%** of the trades, or **20%** of the SOL, from its first trade on this coin to its last, with at least **10** trades. Sniper buys in the creation slot are left out of the share. Those trades are **owner** on this coin. The wallet is not stored. A later trade on a different list is judged again. A public app takes the same test. | One ix structure plus its CU price trades 40 times between its first trade and its last. Those 40 are 25% of the trades in that stretch. Those 40 trades are **owner**. A wallet that used it once is not stored. |
| Many wallets | **owner** | The dev rotates wallets through one script so the volume looks like a crowd. The crowd is the trades of that list. A public app takes the same test. The app name does not move the list to outsider. | The same **ix structure** trades **20** or more times from more than one wallet. CU price is ignored. Those trades are **owner** on this coin. The wallets are not stored. A list that starts in the first seconds after the create and keeps firing is this rule. | On 8AB1, `Axiom Trade: ix#05`, `Axiom Trade: ix#00`, `Transfer` trades 97 times from 94 wallets. Those 97 trades are **owner**. The 94 wallets are not stored. On qH4u the same list trades 51 times from 50 wallets, the first about 1 s after the create. Those trades are **owner**. The 50 wallets are not stored. |
| One wallet | **outsider** | One person trading their own size many times is a trader. The dev spreads the same list across wallets. | One wallet using an **ix structure** 20 times stays **outsider**. | omego trades 20 times on one coin, almost all through one ix structure or his program `bDZu`. One wallet, so those trades stay **outsider**. |

### Pair

One habit uses two wallets, so the buy looks like two people.

| Rule | Side | Why | Explanation | Example |
| --- | --- | --- | --- | --- |
| Same slot | **owner** | One person splits one buy across two wallets and leaves on a clock. One coin can be luck. The habit, repeated, is the person. | In one slot, one wallet buys a fixed size and another wallet buys twice that size. The smaller wallet sells those same tokens **60 seconds** later. Before this has happened on **20** coins, the two buys are **owner** on that coin and the wallets are not stored. Once it has happened on **20** coins, both wallets are stored when their latest trade on this launch group is inside the [recent window](#terms). | EARLY buys 0.987654320 SOL. In the same slot another wallet buys exactly twice that, 1.975308640 SOL. EARLY sells that bag 60 seconds later. On BWEPP the other wallet is Hd9c. On DL744 it is J9bt. The pair is on 188 coins, from EARLY and 11 double-size wallets. Both wallets are stored on those coins. |

### Script

The dev's **ix structure** shows up mostly on this launch group's coins, from more than one creator wallet.

| Rule | Side | Why | Explanation | Example |
| --- | --- | --- | --- | --- |
| Mostly here | **owner** | The dev reuses one script on their own launches. One trade on that script is the trade. It is not yet the person. | **30%** or more of this **ix structure**'s trades sit on this launch group's coins, on at least **5** of those coins, from at least **2** creators. CU price is ignored. Those trades are **owner**. The wallet is not stored. | An ix structure has 100 trades in the market. 40 sit on this launch group's coins, across 6 coins and 3 creators. A new wallet that uses it once leaves that trade **owner**. The wallet is not stored. |

### Wake

When the tape goes quiet, the dev buys with the same **ix structure** to make the coin look alive.

| Rule | Side | Why | Explanation | Example |
| --- | --- | --- | --- | --- |
| After silence | **owner** | The dev restarts a dead tape. A person clicks once. The wallet that repeats the list is doing the restart. | The same **ix structure** is bought **5** or more times, and the first of those buys ends **2 seconds** with no trade. Wallets that buy it more than once are stored on this coin. The **ix structure** is not stored. | The coin has no trade for 2.4 s. Then one ix structure is bought 5 times by two wallets, and each wallet buys it more than once. Both wallets are stored on this coin. |
| Used once | **outsider** | One buy after a pause is a person clicking. | A wallet that buys that **ix structure** only once stays **outsider**, and that buy stays out of the count of 5. | A third wallet buys the same ix structure once inside that stretch. That wallet stays **outsider**. |

### Hidden

The dev moves tokens from one wallet to another with no buy and no sell. The **token bag** is buys minus sells. A transfer is not a trade, so the bag ignores it, and the sell shows tokens the wallet never bought.

| Rule | Side | Why | Explanation | Example |
| --- | --- | --- | --- | --- |
| Below zero | **owner** | The dev sent tokens to this wallet off the tape, then this wallet sold them. | A sell that takes the token bag below zero, by more than **1%** of the sell, is **owner** on that coin. The same sell on **2** coins marks the wallet on its other coins. The address is stored when its latest trade on this launch group is inside the [recent window](#terms). | A wallet sells 1,000 tokens and bought none on the tape. The bag goes to -200. The wallet is **owner** here. The same gap on a second coin marks it on the other coins it trades. |
| Same amount | **owner** | The dev buys in one wallet and sells from another. Each wallet shows only one side. | One wallet buys tokens and never sells them. Another wallet sells that same amount, within **0.5%**, and bought none of it. Both wallets are **owner** on this coin, and sticky covers the rest of each wallet here. The wallets are not stored. Any count is enough. The buy may carry an Approve, and the sell may carry a TransferChecked of those tokens. The **ix structures** are not stored. | On 4FYn, `5qsuh` buys 15,429,758,455,861 tokens at 15:25:12.333 and never sells. `Fzwbh` sells that exact amount at 15:25:12.850 and never bought. Both wallets are stored on 4FYn. The same pair repeats 33 s later for 7,634,011,728,159 tokens. On GbkC, Azcvj buys 6,612,357,687,012 tokens in the creation slot and never sells. 2Bjxx sells that exact amount 0.47 s later and never bought. The sell carries TransferChecked. Both wallets are stored. On 5hkAo, `9nH7` buys 80,778,309,479,650 tokens at 23:35:55.976 and never sells. `D3op` sells that exact amount at 23:35:56.544 and never bought. The sell carries TransferChecked. Both wallets are stored. Five smaller pairs on 5hkAo do the same. |
| Never sells | **owner** | This wallet's job is to create demand. It buys and never takes profit. | A wallet with **10** or more coins, **20** or more buys, and no sell anywhere is **owner** on every coin it buys. The address is stored when its latest trade on this launch group is inside the [recent window](#terms). | A wallet buys 12 coins, 25 buys, and has no sell on any coin. It is **owner** on all 12. |
| Matching holder | **owner** | The wallet still holding the missing tokens is the one that sent them. | A wallet sells tokens it never bought, so the bag goes below zero. Exactly one other wallet on this coin holds that same amount, within **0.5%**. That holder is **owner** too. | B sells 1,000 tokens and bought none. A still holds 1,000. No other wallet holds about 1,000. A sent the tokens to B with no trade. A is **owner**. |
| Bag balances | **outsider** | This seller bought the tokens on the tape. There is no hidden send to find. | The seller bought these tokens on this coin, then sold them. The bag ends at zero or above. The seller stays **outsider**. | B buys 1,000 tokens here and sells 1,000. The bag ends at 0. B stays **outsider**. |

### Loyal

The dev's wallets trade this launch group and little else. Brand-new wallets that all cash out in one moment are the dev leaving.

| Rule | Side | Why | Explanation | Example |
| --- | --- | --- | --- | --- |
| Stays in group | **owner** | A real trader's coins are spread across the market. This wallet lives on this dev's launches. | **3** or more launch-group coins, and **80%** or more of the wallet's coins are in the **launch group**. The wallet is **owner** on every coin it trades. The address is stored when its latest trade on this launch group is inside the [recent window](#terms). | A wallet trades 8 coins and 7 are this launch group. It is **owner** on all 8. The 8th coin is outside the launch group, and it is still **owner** because this wallet is. |
| Fresh cash-out | **owner** | The dev cashes out through new wallets in one moment, then abandons them. | A fresh wallet has traded **4** coins or fewer in the whole market. **3** or more of them each sell their whole bag in the **same slot**, then never buy this coin again. They are **owner**, and so are their earlier buys here. | Three wallets have each traded only 3 coins ever. In one slot, each sells every token it holds on this coin, and none buys this coin again. All three are **owner**, including the buys they made here before that sell. |
| Wide service | **outsider** | A hired service works many devs, so its coins are spread out and its wallets are old. | A volume service that trades many devs' coins is too wide for the 80% test, and its wallets are above the fresh-wallet count. Loyal leaves it **outsider**. | A volume service trades hundreds of creators. Its share of this launch group is far under 80%, and its wallets have traded far more than 4 coins. Loyal leaves those trades **outsider**. |

### Victim

The dev's profit is a real buyer. The dev sells after that buy. The buyer is still holding, and later sells at a loss.

| Rule | Side | Why | Explanation | Example |
| --- | --- | --- | --- | --- |
| Sold into | **outsider** | The wallet wanted the coin. The dev sold into that buy, and the wallet took the loss. An ix list does not change that. | A buy is **outsider** when wallets already named **owner** sell after it, and this wallet later sells those tokens for less than half the SOL it paid. The **ix structure** is ignored. The fingerprint does not store that list from this buy. | On Ezxo, 4Z1e buys 8.20 SOL at age 302 s, with the curve at 107.9 SOL. Two seconds later, 11 wallets sell between 2 SOL and 4 SOL each and the curve falls. 4Z1e sells the same tokens at age 484 s for 0.72 SOL. That buy and that sell are **outsider**. |

## 2. Grow

This is still the [Build](#build). It starts from wallets already marked. Their other trades on those coins are **owner** because the wallet is stored. Grow then names the next wallet. It does not copy an **ix structure**. Repeat until a round adds nobody.

### Backfill

A wallet already named **owner** bought earlier on this coin. Those buys are the same dev, at any count.

| Rule | Side | Why | Explanation | Example |
| --- | --- | --- | --- | --- |
| Earlier buys | **owner** | The buy that built the bag is the same dev as the later sell. | A marked wallet's buys on this coin are **owner**. | On DdTt the sell ix structure `AdvanceNonceAccount`, `Axiom Trade: ix#00`, `Transfer` is already **owner**. The same wallets bought at slot 453178755. Those earlier buys are **owner**. |
| The list stays put | **owner** | The stored wallet is the dev. The app list it clicked is how anyone buys. | A stored wallet's trades on the coins it was stored for are **owner**. The **ix structure** it used is not stored, and it matches no other wallet. A public list still marks trades on a coin where it passes the 20-trade test or the 20% test. | Those buys use `AdvanceNonceAccount`, `Axiom Trade: ix#05`, `Axiom Trade: ix#00`, `Transfer`. The wallets are stored, so their buys on this coin are **owner**. The list is not copied onto the fingerprint. |

### Spread

A known dev wallet points at the next wallet. They share one transaction, or the next wallet's first trade is the dev's tool.

| Rule | Side | Why | Explanation | Example |
| --- | --- | --- | --- | --- |
| Beside an owner | **owner** | Two wallets in one transaction are one person. | A wallet in the **same transaction** as an **owner** wallet is **owner** on this coin. | A is already **owner**. One transaction holds A's sell and B's sell. B is **owner** on this coin. |
| First trade | **owner** | The dev opens a quiet wallet. Its first trade is already the dev's tool, on the dev's coin. | The wallet has no trade for **14** days. Its first trade is on **this dev's coins**, through an owner **ix structure** or **ix core**. That wallet is **owner**. | A wallet has no trade for 14 days. Its first trade is a buy on one of this dev's coins, through the dev's private ix structure. That wallet is **owner**. |

### Not in the build

These rows read an **ix template** (the tool name plus a few markers, such as `Axiom Trade|CU|ATA|N|F`) or the **fee payer**. The stored tag holds neither, so the build does not run them and nothing they find reaches the fingerprint. They stay here as measured rules.

| Rule | Side | Why | Explanation | Example |
| --- | --- | --- | --- | --- |
| Kept template | **owner** | The dev keeps one private tool shape. The next wallet on that shape is the same dev. | **5** or more of this dev's wallets use one **ix template**. Of its trades, **80%** are on **this dev's coins** and **95%** are trades those wallets already made. The next wallet that uses it once is **owner**. One extra wallet on fewer than **100** coins in the month can use it too. | One Axiom ix template is used 100 times. 75 times are this dev's wallets on this dev's coins. 20 times are this dev's wallets on other coins. 5 times are other people on this dev's coins. The coin count is 80 (75+5). The wallet count is 95 (75+20). Five of this dev's wallets use it. Wallet B uses it once. B is **owner**. |
| On a timer | **owner** | The dev's script fires at a set age of the coin. A person does not hit that same second on most coins. | The first trade of this **ix template** lands within **1 s** of its usual time after creation, on **60%** of its coins. The next wallet is **owner**. The same mark applies when half its wallets already use another owner **ix template**. | The first trade is about 30 s after creation. On 6 of 10 coins it lands within 1 s of 30 s. A new wallet trades it at 30.4 s and is **owner**. |
| Fee payer | **owner** | The dev pays the fee for many wallets from one account. That account reveals every wallet it pays for. | The **fee payer** spends **80%** or more of what it pays on **this dev's coins**, and it pays on fewer than **50** coins a day. Every wallet it pays is **owner**. One trade is enough. | One fee payer pays the fee for 30 wallets. 85% of that is on this dev's coins, across 20 coins a day. A new wallet uses it for one buy and is **owner**. |
| Service payer | **outsider** | A public app pays the fee for thousands of strangers. The payer is the app. | A **fee payer** that pays the fee for thousands of different people leaves those wallets **outsider**. | One fee payer pays trades on 4,000 coins a day for thousands of users. A wallet that uses it for one buy stays **outsider**. |

### Stop

A round finds no new wallet, **ix structure**, or **ix core**. Grow ends.

## 3. Check

This is the last step of the [Build](#build). Only a new **ix structure** or **ix core** that Grow wants to reuse on other wallets. The dev sells out when the price falls. A seller who holds through the drop is real money. The numbers are [C1 and C2](#checks).

### Stay

Mark's answers, and a script copied from a wallet already **owner** on this coin, stay **owner**.

| Rule | Side | Why | Explanation | Example |
| --- | --- | --- | --- | --- |
| Mark's answers | **owner** | Mark already watched the whole coin, including the dump. Asking again would drop dev buys that are real. | Certain rules stay stored wallets. Dominates, Many wallets, Mostly here, and a pair under 20 coins stay trade marks on that coin. Sold into stays **outsider**. | Repeat names the 97 trades on 8AB1. Check leaves those trades **owner**. The wallets stay unstored. |
| This coin | **owner** | The wallet is already stored. Its other trades here are the same dev. Its app list is not a new script. | A stored wallet stays stored. Check does not copy its **ix structure**. | The wallets already stored on DdTt stay stored. Their Axiom buy list is not added. A plain `Pump.Fun: BuyV2` stays under the 20-trade test. |

### Test

Did those sellers cash out on the way down. A set is one ix structure, one ix core, or the wallets one rule added.

| Rule | Side | Why | Explanation | Example |
| --- | --- | --- | --- | --- |
| Cashed out | **owner** | The dev sells out when the price falls. They take the profit and leave a small bag. | The set stays **owner** when **40%** or less of its sold tokens are sold after the **big drop** starts, and **10%** or less of its biggest bag is left at the end of that drop, on **60%** of at least **5** coins with a drop. A sell while that drop is still under halfway counts as early. A set with no sells passes the first half. | On 8 coins with a drop, the wallets sell on the way down and hold under 10% of the bag at the bottom, on 5 of the 8. The ix structure stays **owner**. |
| Too few coins | **owner** | There are too few dumps to judge the exit. Leave it until more coins exist. | Fewer than **5** coins with a drop. Check leaves the set **owner** and untested. | An ix core shows up on 2 coins. It stays **owner** until more coins exist. |
| Held the bag | **outsider** | These sellers held through the drop. That is a real buyer, or a bot that did not get out. | The sellers still hold most of the bag after the drop, so that **ix structure** or **ix core** is **outsider**. Grow runs again without it. | The wallets that use this Axiom ix structure still hold most of the bag after the drop, on most of their coins. It is **outsider**, and Grow continues without it. |

## 4. While a rule runs

This is [While a rule runs](#while-a-rule-runs). A trade is **owner** when it matches a stored row, comes from a stored wallet or the creator, or comes from a wallet already sticky on this coin. Every other trade is **outsider**. A match starts sticky: it covers that wallet's later trades on this coin and does not relabel a trade that already printed. Nothing is written while rules run. Mark, Grow and Check do not run on a print, so a this-coin rule never fires live; a stored row is what carries its finding onto a new coin. A wallet is never a term in a rule ([_!___strategy.md](_!___strategy.md) T5).

## Outsider

Real money, once Mark, Grow, and Check have passed. Birth names the sniper. The other kinds:

- **Button buyer.** Buys a size an app offers as a button: 0.1 / 0.5 / 1 SOL after the venue fee (0.099, 0.494, 0.988), or a dollar button at the day's SOL price, or a size 15 or more traders used in the same hour.
- **Timer.** Buys about the same size on thousands of coins and sells on a fixed clock. It trades for itself. A clock shows a machine. The breadth shows whose machine. On 18,223 coins, 9bwHb buys 0.50 SOL and sells 19 to 23 seconds later on 18,115 of the 18,208 coins it closes. On GbkC the buy is at age 216 s and the sell is 21 s later, ahead. That wallet is **outsider**.
- **Bottom-fisher.** One buy of 1-3 SOL on many coins, about 30 a day, out within minutes, ahead on most coins.
- **Herd bot.** Many coins a day, joins within seconds of a big buy, ends empty, market result around even.
- **Racer.** Pays a high priority fee to land first.

## Match levels

The owner rotates small variants of one ix structure, so the build judges each ix structure at seven levels, loosest first. A row is stored at any level where the market tests hold, and the loosest passing level catches every variant.

The **ix core** is the labels left once compute budget, every System Program instruction, token-account instructions, memo, and Lighthouse are dropped. Order stays. Pump.fun verbs merge: Buy, BuyV2, and BuyExactSolIn are BUY; Sell and SellV2 are SELL; Create and Create_v2 are CREATE. An app's own instructions stay. The build and the engine reduce a list to the same core: both assert `hunter/engine/fixtures/ix_core_parity.json`.

The **marks** are which extras are present (CL CU limit, CP CU price, N nonce, L Lighthouse, M memo, S seed or created account, C account close, W wrap), never their order, plus two counts (T system transfers, A token-account opens).

The **numbers** are the CU limit, the CU price, and the tip.

| level | the trade matches when it has the same |
| --- | --- |
| 1 | core and side |
| 2 | core, side, and marks |
| 3 | core, side, and CU price |
| 4 | core, side, and tip |
| 5 | core, side, CU price, and tip |
| 6 | core, side, marks, CU price, and tip |
| 7 | exact ix structure, CU limit, CU price, and tip |

A private program and a public app take the same test. Asking which one a list came from decides nothing. Level 1 passes when that core is 90% or more owner SOL on the group, with 10 or more trades. A shared core such as a plain `Pump.Fun: BUY` fails that test and stays out. Repeat on one coin uses the same numbers for both. The 20% share is the **ix structure** plus its CU price, from its first trade on the coin to its last, with at least 10 trades. The 20-trade count is the **ix structure** from more than one wallet, and the CU price is left out. A list that passes marks the trades on the coin where it passed. The wallet is not stored from that pass, and a stored wallet does not add its own lists. Stored rows hold level 7 as an exact ix structure, and levels 1-6 as a core row: the core's labels, the side, and whichever of marks, CU price, and tip that level pins. A pin the trade lacks is left off, so that row matches any value there.

## Checks

This is step 3. These rows keep or drop an **ix structure** or an **ix core** that Grow wants to reuse on other wallets. They leave a single trade unmarked.

Certain rules stay stored wallets. Dominates, Many wallets, Mostly here, and a pair under 20 coins stay trade marks on that coin. Sold into stays **outsider**. Check leaves them unread. A set is one ix structure, one ix core, or all the wallets one rule added. A stored wallet's own lists are not a set.

Only the seller is judged by how it exits. A buyer that never sells is judged by Hidden.
A set that trades both ways and ends empty is judged at the ix structure or ix core that brought it in.

| check | stays when | what it reads |
| --- | --- | --- |
| C1 | 40% or less of its sold tokens are late. A set with no sells passes | a sold token is late when sold after the main fall begins, except a sell while that drop had fallen less than half its way |
| C2 | at the end of the main fall, 10% or less of its biggest bag before the fall | the owner has cashed out |

A set stays when C1 and C2 both pass on 60% or more of its coins with a fall, over at least 5
coins. Fewer coins, and the set stays untested. Tune the two so the creator passes and a wide bot
that wins fails: a wallet on 100 or more coins, net SOL above zero, behind on fewer than half its
coins, that never used the owner's program and never created a coin. Snipers are not judged here.

Mark holds for every chart shape. C1, C2, and the verify checks below are retuned per shape.

| shape | the fall | C1 and C2 |
| --- | --- | --- |
| steady rise, one-shot dump | the one dump | on the main fall |
| waterfall | the drops less than 30 s apart, read as one fall | C1 on every step; C2 at the bottom |
| many swings | each fall | C1 on every fall; C2 only on the last, since the owner may buy again |
| slow bleed, no dump | none | untested; the set stays on the facts alone |
| heavy outside trading | as above | each group sets its own values from the coins that pass |

| check | good when |
| --- | --- |
| V1 owner money, per coin, bags included | the owner ends behind the outsiders on 30% of coins or fewer, and its usual loss is 10% of its money or less |
| V2 share of price movement from owner trades | 80% or more |
| V3 share of seconds where outsiders move 0.5 SOL or more | 20% or less. Money, not the presence of a tiny trade |
| V4 share of outsider SOL in their busiest 10% of seconds | 50% or more |
| V5 an outsider wallet or shape trading like a script, 10 or more 3 s windows in a row | none. Each one found is reviewed as a missed owner |
| V6 eye check, price and both flows on one axis | the owner line has the chart's shape. A stretch where the outsider line follows the chart is a real swing when the owner does not sell into it and the tokens sold there were bought there |
| V7 the lists, used on the 2 weeks before | they still find the owner, and less than 2% of outsider money is marked owner |
| V8 shuffle who traded inside each coin, keep the times | the fact finds far more on the real tape |

V1 reads the owner side as a whole. A buyer wallet loses while a seller wallet collects. Wallets change
fast, shapes and paying keys slowly, so a stored tag ages: rebuild it offline from the last two
weeks, never on the live box.

A wallet that buys an **ix structure** once and sells what it bought stays **outsider** while that **ix structure** stays under 20% of the stretch from its first trade to its last, fewer than 20 trades come from more than one wallet, and the wallet buys that ix structure once. Every app takes that test.


## Appendix A - tested and not used

The rows below keep the names used when they were measured. The names in the rules above are spelled out in [Terms](#terms).

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
| losing machine operators as owner (M1-M3) | a machine sign (a bundle of 3+ wallets in one slot and app, or a lockstep pair), sister wallets trading the same market coins joined into one operator, and the operator below zero over the market: owner | the wallets it adds lose to the owner on the group's coins, so they are its prey: on 3ix 0.54 they put in 2,125 SOL net while the creation crew takes out 1,887, and they buy late (median slot +314 .. +523); on 7ix they end -349 SOL (bags counted) while the owner ends +3,344. On a group whose builds are all public, 3 wallets in one slot through one app is any crowd |
| heavy loser on wide breadth | a wallet on 20+ market coins losing 1 SOL or more a coin: a paid volume service | adds 27 SOL on 3ix 0.54; on 5ix it adds 262 SOL, of which the known owner holds only 28 % |
| build loyalty on the family | A4 against every coin with the group's create instruction list (all `max_sol_cost` or CU price slices), 20 times chance | finds the 3ix 0.54 owner's buy script (49 % of its trades on the family, 9 % on the slice) but assumes one family is one owner; the same create list carries several owners split by CU limit, CU price, `max_sol_cost` or spendable SOL. D finds the same script on each coin it dominates (3ix owner 93.8 % against 94.3 %) |
| D on the bare instruction list, or at 10 % | the pattern without its CU price, or a 10 % share | at 10 % it takes 1.9-3.5 % of the winning wide bots' money on 5ix and 7ix; without the CU price, plain public builds dominate small coins |
| a tag of exact lists only | the owner tag holds whole instruction lists (with a CU price pin where needed) | on new 5ix coins it catches 84.0 % of owner SOL with 2,952 rows; core and exact rows catch 88.2 % with 663 ("How a split becomes a tag") |
| wallet loyalty at 30 % | W with 30 % of the wallet's market coins in the group | on 5ix the split's outsiders hold 40 such wallets with 41 SOL (0.03 % of the volume), small losers; W at 80 % keeps the owner and leaves them |
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

On 7ix, built on 09-15 .. 10-01, checked on 09-02 .. 14:

| what | result |
| --- | --- |
| the split | 1,333 coins (the old instruction list on any day, the new one with CU price 1000 from 09-28); owner 93.6 % of SOL; owner ahead on 97 % of coins; V2 passes on 82 % of coins (median 94 %); V3 median 1.6 % of seconds |
| old days (V7) | owner 93.1 % against 94.2 % for a split built there; 1.77 % of outsider money marked owner (186 SOL) |
| outsider reference | wide bots that win over the month marked owner on 1.1-2.1 % of their money, none of it by M |
| calibration of C1-C2, judged as sets | crew program wallets 66 %, creators 90 %, creation-slot owner buys 70 %; snipers 44 %, wide bots 4 %; T1 wallets 35-45 % (they sell all through the coin's life, so T sets are never judged by C) |
| match levels (S1) | a list of exact structures built on one half of the month finds 81 % of owner SOL on the other half one way and 53 % the other; the core with all 7 levels finds 93 % and 89 %, adding 0.00-0.05 % of outsider money. The crew program rotates up to 201-549 exact variants of one core. Owner structures passing on 09-15 .. 10-01: level 1: 6, level 2: 16, level 3: 135, level 4: 12, level 5: 272, level 6: 367, exact: 448 |
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
| winning wide bots' money in the final split (sniper breadth over the window) | 3.0 % (the answer key: 4.1 %) | 1.4 % |
| A4 built on one week, used on the other | 86 %, 0 SOL outsider | |
| owner instruction lists found | 22 (19 carry `9ddjzq`, one is the create transaction) | 41 programs, 40 plain lists |
| final split: owner ahead of outsiders / V2 median / V2 >= 80 % | | 99 % of coins / 0.95 / 92 % of coins (without A4: 98 % / 0.70 / 34 %) |

**How a split becomes a tag.** One build serves every group. Every structure of the group's trades
is read at each match level (core levels 1-6, the exact list alone, the exact list with its CU price,
level 7), and a row passes when it is 90 % or more owner SOL on the group and has 10 or more trades
there. Owner SOL is the split's own labels. An instruction list is not dropped because its wallets are absent from the stored list. A one-coin shape at 20 % of the trades or SOL inside its own run passes on that coin, including a public app. A list that passes Many wallets stays, including one that starts in the first seconds after the create. Same amount sticks both wallets on that coin and stores neither wallet nor either list. A this-coin wallet is not copied onto the fingerprint. The fingerprint wallet list holds the wallets that come back, inside the recent window at build time. The run is the stretch from that shape's first print on the coin to its last.
An instruction list with at least 20 prints from more than one wallet on that coin passes too. The CU price is left out of that count, and one wallet's prints do not pass. The passing one-coin row is that exact ix structure, plus its CU price when the 20 % share is what passed. The row marks those trades. It does not store the wallets, and a stored wallet does not add the lists it used. The 20-trade test and the 20 % test stay for a list no certain rule has named. The tag holds the fewest passing rows that still
cover every trade the passing rows cover (biggest row first), the creator, and the wallets that come back. A this-coin wallet is not on that list. A row applies on every coin of
the fingerprint, while D reads one coin, so the tag can carry some outsider money the split leaves
out. The measured rows in the table below are each under 10,000 market coins. A wider list stays in when the one-coin tests pass.

| fingerprint (tag) | rows (core / exact) | wallets | agrees with the split on SOL | coins within 5 points | rows alone carry |
| --- | ---: | ---: | ---: | ---: | ---: |
| 7ix, both create lists, 224 coins (`volume` and `owner`) | 54 / 5 | 170 | 99.8 % | 99 % | 99.3 % of owner SOL |
| `5ix owner split` (`volume`) | 1,159 / 53 | 13,631 | 99.6 % | 99 % | 90.0 % |
| `3ix 0.54 owner split` (`volume`) | 11 / 7 | 127 | 99.3 % | 91 % | 98.4 % |

**Core rows reach new coins.** Rows chosen on the first 60 % of a group's coins by birth and read on
the other 40 %:

| group | rows | owner SOL caught on the new coins | outsider SOL caught | rows needed |
| --- | --- | ---: | ---: | ---: |
| 5ix | exact lists only | 84.0 % | 1.6 % | 2,952 |
| 5ix | core and exact | 88.2 % | 2.6 % | 663 |
| 7ix | exact lists only | 98.2 % | 0.0 % | 49 |
| 7ix | core and exact | 98.2 % | 0.1 % | 38 |
| 3ix 0.54 | either | 98.0 % | 25.8 % | 16-17 |

Core rows alone, without the exact levels, catch less (5ix 85.1 %, 7ix 95.0 %): some owner builds
pass only as one exact list. The owner bar (80-95 %) moves none of these by more than half a point.
A wallet list adds under one point on new coins, since the owner's wallets rotate. On 3ix 0.54 the
outsider SOL caught is the owner's own buy script (CU price 950,000, 664 market coins) on coins where
it stays under D's 20 %; the split leaves those trades outsider. On 7ix the exact row `ATA Create,
Pump.Fun Buy` (no CU instructions, 229 market coins, 84 creation-slot buys of 1,038 SOL, all owner)
catches the owner's creation-slot bundle buy on a new coin before any wallet list knows the wallet.

With W and L in place of M1-M3 the 5ix split
keeps 100 % of the answer key, and the owner share moves from 91.7 % to 90.5 %; D adds 4 SOL.

The 5ix outsiders are 9.5 % of the SOL (median coin 5.1 %, 81 % of coins under 10 %). They are wide
traders: 64 % of their SOL comes from wallets on 100 or more market coins in two weeks, and wallets
seen on one coin only carry 84 SOL. The largest unknown program among them, `6Vo3245e`, is
a public app (115,735 coins, 101,397 wallets, 0.4 % of its trades on 5ix). Measured the way the
chart reads the tag, a median coin has 5 % outsider SOL on 5ix against 7 % on 7ix.

A tag reads forward: sticky marks a wallet's trades only from its first tagged trade on. The split
labels the wallet's whole history on the coin. An ix structure is on the shape list because a rule
marked that ix owner. Every wallet that uses it is owner, and every trade of that wallet is owner.
A stored wallet does not add the other lists it used. The wallet list holds the wallets that come back, and only those whose latest trade on this launch group is inside the recent window at build time: a pair after 20 coins, a wallet below zero on 2 coins, a wallet that never sells, a wallet that stays in the group. A birth-bundle wallet is new on the next coin, so only a stored row catches it there. The exit burst is on the shape list, one core
row per pack size, so the sell matches on a wallet the list has not seen.

On `Ad8zvok8gZebkyLYCWP63Jj9xeopiVTR5773Tndwpump` the birth-bundle wallets buy in the creation
slot and sell in the 12-sell burst at 19:21:36. The buys and the burst are volume. A single-wallet
app sell in that same second stays outside the burst, and a sniper later in the creation slot
stays outside the birth bundle. On
`4u5L6sfpur3PreYvirLxJiKbuBaqFtGcmS6G7tXMpump` the three plain buys beside the create are volume
through the wallet list, and the slot-+242 dump is volume through the 12-sell core.

**The stored tag, coins born 09-15 .. 10-01.** `5ix owner split` holds 816 core rows, including
one pure `Pump.Fun: SELL` core for every pack size from 2 to 12 (size 1 stays pinned to its CU
price), and 17,677 wallets, 4,046 of them the birth bundle. `3ix 0.54 owner split` holds 222
wallets, 95 of them the birth bundle, and no pack-size row. On 7ix the tag holds 13 rows plus a `dump` tag. Birth-bundle wallets on top of the shared 170 are 12, 28, 14,
40, 0, 4, 2, 28, 0 and 20 across the five max-cost bands, new list then old list.
3ix and 7ix dumps are a pinned single sell and, on 7ix, the `9ddjzq` dump instructions.

**3ix 0.54: a group whose builds are all public.** 3ix:Buy (`Create_v2, ATA CreateIdempotent,
Buy`, no CU instructions) with `max_sol_cost` 0.54 SOL: 80 coins born 09-17 .. 30, 13,618 SOL,
median coin 78 SOL, 36 % of coins rise with no real drop and fall in one slot. The groups with
`max_sol_cost` under 0.054 SOL are not tradable: someone buys the whole curve in the creation slot.

| what | M1-M3, no D, W, L | no M1-M3, D, W, L | + W and L | + D (final) |
| --- | ---: | ---: | ---: | ---: |
| owner share of SOL | 92.3 % | 50.7 % | 90.5 % | 93.8 % |
| owner ahead of outsiders (V1) | 93.8 % of coins | 90.0 % | 95.0 % | 95.0 % |
| V2 median / coins >= 80 % | 0.94 / 85 % | 0.75 / 44 % | 0.92 / 79 % | 0.93 / 89 % |

D finds 81 dominant (coin, pattern) pairs; the main one is the owner's buy script (`CU Price,
CU Limit, ATA CreateIdempotent, Buy` at CU price 950,000, on 664 market coins, 4,077 SOL here).
Read as the chart reads it, a median coin has 5.2 % outsider SOL (75 % of
coins at or under 8.3 %, the highest 31 %), against 7 % on 7ix and 5 % on 5ix. The tape carries
curve trades only, so a coin's life after migration (35 coins, all pushed there by the W wallets)
is not in any of these numbers.

On 7ix (the 162 coins of the exact create list born 09-17 .. 30) the same method gives owner
74.7 % of SOL (69.1 % without D, 81.8 % with M1-M3), owner ahead on 95.1 % of coins, V2 median
0.85; D adds the `9ddjzq` lists A4 misses on this coin set (980 SOL).

On 6ix: one network of 2,136 wallets trades 84 % of all 6ix coins and makes 58 % of their volume;
107 creators are its own wallets. It ends about even (-0.01 SOL a coin); the creator is ahead on
83 % of coins, outsiders on 9 %.

On the whole market, 09-26 (28,819 coins created):

| what | result |
| --- | --- |
| creation-slot money | creators buy 80,861 SOL; other transactions in the creation slot 77,772 SOL (55,787 buys on 15,627 coins) |
| snipers in it | buyers on more than 50 coins that day make 34,135 of those buys (33,707 SOL, median 0.62 SOL); buyers on 50 or fewer make 21,724 (44,132 SOL, median 1.3-1.4 SOL) |
| payers | 234,149 trades paid by another account; three service payers pay on 3,405-4,999 coins each; 3,678 payers pay for 6-20 wallets on a median of 1 coin (one operator each) |

## Terms

Each row above uses these names. **Why** is why the dev behaves that way. **Explanation** is the test. **Example** is one case with the numbers.

**Owner.** Fake demand. The dev, the dev's wallets, and the machines the dev hires.

**Outsider.** Real money. A person, or a bot trading for itself.

**Dev.** The person who launches the coin and fakes the chart.

**Tape.** The buys and sells. A token transfer is off the tape. The rules can see only what the tape shows.

**Token bag.** Buys minus sells for one wallet on one coin. A transfer is not a trade, so it leaves the bag unchanged. A sell of tokens that arrived by transfer takes the bag below zero.

**Recent window.** The build's clock for a stored wallet. Count this launch group's own coins, newest first, up to the build's last coin. The width is the gap within which 90% of wallets that come back do come back. A gap of half of those returns drops an address that is about to trade again. A gap that covers nearly every return keeps addresses the dev has finished with. An address is stored when it passes its test and its latest trade on the group is inside that width. A running rule never moves the window.

**Stored and sticky.** Stored is on the fingerprint, written by the build: rows, wallets, the creator flag. A hand edit to the stored tag lasts until the next build replaces it. Sticky is this coin only: a wallet with one owner trade here, from that trade on. Sticky is never written back.

**Ix structure.** The exact instruction list of one transaction, in order. Order and repeats count. `AdvanceNonceAccount`, `Axiom Trade: ix#05`, `Axiom Trade: ix#00`, `Transfer` is one ix structure. A different order is a different ix structure.

**Ix core.** An ix structure with compute budget, System, token-account, memo and Lighthouse instructions dropped, and Pump.fun's Buy / Sell / Create variants merged. Order stays. One core covers every variant the dev rotates. The exact rule is in [Match levels](#match-levels).

**Core marks.** Which extras a trade's ix structure carries beside its core (CL CP N L M S C W), plus its system-transfer and account-open counts (T, A). A core row may pin them.

**Match level.** How a stored row matches. An **exact** row is one ix structure and may pin CU limit, CU price and tip. A **core** row is one ix core and may pin side, core marks, CU price and tip. Levels 1-6 are core rows, level 7 is an exact row.

**Fee payer.** The account that pays the transaction fee. Also called the fee key. When the credited wallet is a routing wallet that thousands of users share, the rules read the fee payer.

**Launch group.** Coins born from the same creation ix structure, and the same CU price or `max_sol_cost`. Fixed at birth. The Script rule and the Loyal 80% test read this group.

**This dev's coins.** Every coin already counted as this dev's. The list starts as the launch group: Mark finds the owner wallets there. A coin outside the group joins when one of those wallets is already **owner** on it. Loyal does this when a wallet has 3 or more launch-group coins and 80% of its coins are in the group: that wallet is owner on every coin it trades, including the one outside the group. Hidden does this when the same below-zero sell happens on 2 coins: the wallet is owner on its other coins. The 80% in Kept template and Fee payer ([Not in the build](#not-in-the-build)) counts trades on this list.

**CU price.** The priority fee on the transaction. Repeat's 20% share includes it. Repeat's 20-trade count leaves it out.

**Slot.** One moment on the chain. Several transactions can land in the same slot. The creation slot is the slot that holds the create.

**Fresh wallet.** A wallet that has traded 4 coins or fewer in the whole market.

**Big drop.** The coin's deepest price drop. A drop is a slot where the price falls 10% or more. Drops less than 30 s apart count as one drop. Also called the main fall. Check reads this drop.

**Run.** From an ix structure's first trade on a coin to its last trade on that coin.
