# Inventory: the ideas

Every idea, as a tree: **slot → family → idea → variant**.
[_!___strategy.md](_!___strategy.md) is the basis;
[_!___derive.md](_!___derive.md) is the method, the gates and how a result is recorded;
[_!___workflow.md](_!___workflow.md) is the open queue;
[_!___evidence.md](_!___evidence.md) is the numbers;
[_!___terms.md](_!___terms.md) is every word;
[_!___metrics.md](_!___metrics.md) is what the engine measures. This file is the idea list used
**on a parent** (derive phase 10). It does not invent the parent.

Editing rules:

- **Every idea a study tries is registered here**, new or re-read, in the same edit as its step
  (derive 13). Every D/E/P/X term of a booked sentence gets a row, red ones included.
- A row is six columns. **Every row reads alone**: no shorthand for its family or its
  neighbours ("the class", "the same", "that structure", a bare "it"), and every word in it is
  plain English or a line in [_!___terms.md](_!___terms.md).
  - **Name**: 1-4 words, sayable, and unique in this file.
  - **Idea**: one short sentence: what it reads and which way is good ("holders whose loss is
    -20 % ~ 0 %: fewer is better"). **No fitted number** - the cut that separates is read per
    pool and lives in the doc the Status names. A number stays only when it is fixed, and then
    the row says so: the curve and the machine (the wall at 115, a 0.4 s slot, our 115 ms seat),
    a class the engine defines (3 first buys in one slot is a bundle), standing policy (0.2 SOL,
    one open position per coin), and a **booked** term of a shipped rule. Two marks carry a
    number that is not a law: **(booked: X)** is what ships today, **(as read: X)** is the value
    one study used, kept only where the row makes no sense without it.
  - **Meaning**: the rule in plain words. It opens with what the row does - it **fires** (it is
    the print we buy on), **filters** (it lets a fire through or not), **measures** (it is a
    number another row cuts), **groups** (it decides which coins are judged together), **sells**
    (it closes a position) or **sizes** (it sets the clip) - and when another row is close, it
    names that row and gives the difference in one clause.
  - **Why it matters**: why it could move the price (or cause the loss), and which way is good
    for us. On a red or dead row it is the reason it was expected to work; the Status reference
    holds why it failed. The E8 graveyard also says why, in a second sentence.
  - **Example**: a worked case, 2-4 numbered steps with numbers, separated by `<br>`, ending in
    what happens (we fire, the coin passes, we sell) - or in the value, on a row that measures.
  - **Status**: see below.
- **An idea, never a metric and never a measurement.** A row is a claim about a coin or a print
  that could fill a slot; the metric is only how it is spelled, and its definition lives in
  [_!___metrics.md](_!___metrics.md). A number belongs to
  [_!___evidence.md](_!___evidence.md), a candidate list to [_!___workflow.md](_!___workflow.md),
  a capture instruction to [_!___derive.md](_!___derive.md).
- **A word an idea needs is registered**, plainly, in [_!___terms.md](_!___terms.md), in the same
  edit as the idea.
- Put an idea under the family that shares its mechanism. A variant is a `└` child of its idea.
  A threshold or an exit setting is a variant, never a new idea. Open a new family only when no
  family shares the mechanism.
- One idea, one row. An axis used in several slots gets one row per slot, and each row says only
  what that slot does with it.
- Every term is spelled in ix structure or tape state. A row that needs a wallet identity is not
  an idea and is not added. A derive 5.1 class keeps its code name in the Idea (`clip_step_up`).
- Status is one word plus one reference. Numbers that score an idea live in the evidence file,
  never here; the numbers in an Example only illustrate.

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
mechanism) · **new** (never scored). A **keep** may carry one word saying what it is kept as:
**(screen)** a door refreshed daily, **(exclude)** a cut that only removes, **(control)** a
yardstick every rival is measured against, **(term)** a piece other rows are built from.

Then one reference, and one only:

| form | means |
| --- | --- |
| `ev N` | section N of [_!___evidence.md](_!___evidence.md) |
| `ev 7 (C9)` | that rule's row in the evidence ledger, by its label |
| `case N` | step N of [hot-tape-rule-1.md](node-derivation/hot-tape-rule-1.md) section 2; `case L4` / `case U5` are the lettered rows of its later sections |
| `mt1 N` / `mt2 N` / `mt3 N` | step N of [mid-tape-rule-1.md](node-derivation/mid-tape-rule-1.md) / [mid-tape-rule-2.md](node-derivation/mid-tape-rule-2.md) / [mid-tape-rule-3.md](node-derivation/mid-tape-rule-3.md) |
| `mt3 5.2 88887Q` | that step read on one named wallet |
| `derive N` | section N of [_!___derive.md](_!___derive.md) |

A row carries **no reference** only when nothing measured it: a `new` idea, or one ruled out by
argument rather than by a book. Then **Why it matters** ends with the argument that ruled it out,
because there is no doc to send the reader to.

---

## Terms

Every word these files use is in [_!___terms.md](_!___terms.md): the tape, the curve, how a
print was sent, who acts, what a coin does, the six slots, and the study code names. A word an
idea needs is registered there in the same edit.

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
   E1  a listed ix structure acts                        the lists · trigger forms · identity family
   E2  silence, then a spend                             whose silence · who breaks it
   E3  an operator's plan is unfinished                  legs · clip left · step-up · under its own sell
   E4  a count crosses a line                            buyers · structures · flow rate
   E5  after sellers                                     dip · flush · who sold · sell inside a frenzy
   E6  this print                                        slot · size · price step · fee · fresh buyer
   E7  clock                                             fixed age
   E8  graveyard
P  PERMISSION
   P1  curve position                                    age · vsol · headroom
   P2  windowed tape metrics                             flow · price · crowd
   P3  skin in                                           creator · crowd · pusher · cost basis
   P4  ix makeup of the recent tape                      seed racers · tools · buy SOL band
   P5  tape state already true                           structures · sells lately
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

| Name | Idea | Meaning | Why it matters | Example | Status |
| --- | --- | --- | --- | --- | --- |
| **Exact launch** | Group coins by the exact instruction list of the transaction that created them | Groups. Coins born from an identical create transaction are judged together. **Coarse bucket** groups the same coins more loosely. | One launch tool or one operator sends the same create transaction every time, so its coins tend to behave alike: a good group's next coin is likely good too. | 1. 300 coins are created by one identical 5-instruction create transaction.<br>2. They form one group.<br>3. The group's history (D1.2) decides whether its next coin enters the door. | keep · ev 3.1 |
| └ **First-slot buy** | The same group, split by how much SOL anyone bought in the coin's first slot | Groups. **Creator's opening buy** splits by what the creator alone put in; this splits by what everyone put in. | One launcher does quiet launches and bundled launches; they behave differently, so they should not share one history. | 1. A group's coins are split into bands by creation-slot buy SOL.<br>2. This coin's first slot bought 2 SOL, inside the 1-3 SOL band.<br>3. It is judged only against the group's other 1-3 SOL coins. | open |
| └ **Creator's opening buy** | The same group, split by how much the creator buys inside the create transaction | Groups. The creator's own stake at birth, not the whole first slot. | What the creator puts in shows how serious the launch is: a bigger stake is a creator who needs the coin to live. | 1. The create transaction also buys 0.8 SOL for the creator.<br>2. That falls in the 0.5-1 SOL band.<br>3. The coin is judged with the group's other 0.5-1 SOL coins. | open |
| &nbsp;&nbsp;└ **No opening buy** | The create transaction carries no buy at all | Groups. The empty end of **Creator's opening buy**. | A creator with no stake loses nothing when the coin dies, so nothing holds the launch together. Red on the sentences run, with no book of its own: why it failed is not recorded. | 1. The create transaction holds a Create and no Buy.<br>2. The creator owns 0 tokens at birth.<br>3. The coin is expected to die sooner. | red |
| └ **Launch settings** | The same group, split by the max cost, priority/tip fee and CU limit the launch software sets | Groups. These settings sit inside the create transaction and fingerprint the software that sent it, finer than the instruction list alone. | A finer fingerprint puts one operator in one group instead of mixing several, so the group's history describes one behaviour. | 1. Two launchers send the same create ix structure.<br>2. One sets a CU limit of 200,000 and a 0.001 SOL tip; the other 300,000 and no tip.<br>3. They become two groups, each judged on its own coins. | open |
| **Coarse bucket** | Group coins by the instruction count and the last instruction of the create transaction, e.g. `5ix:BuyV2` | Groups. Looser than **Exact launch**: it keeps only the length and the last step, so unrelated launchers land in one group. | Bigger groups carry more history, and history is what the door reads. | 1. Every 5-instruction create that ends in `BuyV2` is one group.<br>2. 40 different launchers fall inside it.<br>3. The group's history mixes all 40, so it describes none of them. | red · ev 7 (mid-tape instruments) |

#### D1.2 History test: which groups pass

A test names the move type, the minimum count of coins, the rate, and the window.

| Name | Idea | Meaning | Why it matters | Example | Status |
| --- | --- | --- | --- | --- | --- |
| **Slow-wall group** | Yesterday the group launched enough coins and enough of them made a slow wall, and it is not a bundle launch group (booked: ≥ 20 coins, ≥ 5 % in the sentences, ≥ 8 % in the shipped screen) | Filters. It keeps the coins of launchers whose coins climbed to vsol 60 **after** their first minute. **Instant-wall group** keeps the launchers whose coins get there at once. | A launcher whose coins climb slowly tends to repeat it today, and a slow climb leaves time for our fill to ride it. | 1. Yesterday the group made 40 coins.<br>2. 3 of them (7.5 %) reached vsol 60 after age 60 s.<br>3. 7.5 % clears the 5 % cut: the group's coins are in today's door. | keep (screen) · ev 3.1 |
| └ **Heavy first slot** | The same group, and this coin's own creation slot bought above a floor (booked: ≥ 2 SOL) | Filters. The group test judges the launcher; this judges the single coin in front of us. | Money in the first slot shows the launcher backs this coin, instead of spraying it and moving on. | 1. The coin comes from a slow-wall group.<br>2. Its creation slot bought 2.5 SOL.<br>3. 2.5 clears the 2 SOL floor: the coin passes. | keep (screen) · ev 3.2 |
| **Instant-wall group** | Yesterday the group launched enough coins and a high share of them reached vsol 60 at any age, and it is not a bundle launch group | Filters. Its coins reach vsol 60 by about age 15 s and decay after it, which is what separates it from **Slow-wall group**. | These coins move hard, so the group's next coin could make the same run. | 1. Yesterday the group made 40 coins.<br>2. 5 of them (12.5 %) reached vsol 60.<br>3. 40 coins and 12.5 % clear the cuts read (30 coins, 10 %): the group qualifies. | red · ev 3.1 |
| **Dump factory** | Remove groups whose coins nearly always spike once and then fall back under where they started | Filters, by exclusion. **Coarse bucket** and the group keys decide who is in a group; this removes whole groups from the door. | A launcher that rugs by design does it again on the next coin, so removing the group removes losses we can see coming. | 1. A group made 30 coins.<br>2. All 30 spiked, then fell 80 % inside a minute.<br>3. The group is out of the door, whatever its wall rate says. | keep (exclude) |
| **Other move type** | The same history test, counting +100 % hills or second hills instead of coins that reached vsol 60 | Filters. Same test, different move counted. | A launcher can be good at a move other than the wall, and a rule that trades that move needs a door that counts it. | 1. Yesterday a group made 50 coins.<br>2. 6 of them (12 %) made a second hill.<br>3. 12 % clears the cut: the group's coins are in the door. | new |
| **Group live now** | Another coin of the same creation fingerprint is trading right now | Filters. The group's history is yesterday's; this is the group's activity at this moment. | The launcher and the buyers who follow it are awake right now, so this coin can get the same attention. | 1. Coin A from the group printed 20 s ago.<br>2. Coin B from the same group reaches a fire.<br>3. The group counts as live: B passes. | red · ev 7 (C15) |

### D2 Creator's document (metadata URI)

What the creator publishes at birth, read from the metadata URI. It marks a coin that **lives**,
not one that spikes. A later fetch is empty on old coins, so capture is live.

| Name | Idea | Meaning | Why it matters | Example | Status |
| --- | --- | --- | --- | --- | --- |
| **Documented** | The creator's document carries a website, a telegram or a description | Filters. The loosest of the document cuts: any one of the three is enough. | A creator who writes something plans for the coin to live, and a coin that lives gives rises to catch. | 1. The document holds a `website` field.<br>2. The coin counts as documented.<br>3. It passes the door. | keep · ev 3.3 |
| └ **Website-led** | A website, and also a telegram or a description of real length (as read: over 80 characters) | Filters. **Documented** needs one of the three; this needs two, anchored on the website. | Two efforts mark a planned project rather than a field filled in to look complete. | 1. The document has a website.<br>2. Its description runs 120 characters.<br>3. Website plus a real description: it passes. | keep · ev 3.3 |
| └ **Telegram-led** | A telegram, and also a website or a description of real length | Filters. **Website-led** anchored on the channel instead of the site. | The same two-effort test, anchored on where a community would actually gather. | 1. The document has a `t.me/...` link.<br>2. It also has a website.<br>3. Two efforts: it passes. | new |
| └ **Telegram required** | A telegram link is present | Filters. The single cheapest of the document cuts. | A community channel is the cheapest sign that someone intends to keep the coin alive. | 1. The document has a `t.me/...` link.<br>2. It passes, whatever else the document holds. | open |
| └ **Live link** | The website or telegram answers when we ask, at the fire | Filters. The other document rows read what is written; this reads whether it is real. | A dead link is copy-paste, a working one took someone's effort. | 1. We fire and request the website.<br>2. It answers: the coin passes.<br>3. It times out: the coin is out. | open |
| **URI host** | Skip documents stored on a launchpad's own host; keep the ones on plain IPFS | Filters. It reads where the document lives, not what it says. | A launchpad host means the coin was made in a few clicks; a self-uploaded document took a decision. | 1. The URI sits on a launchpad's domain: skip the coin.<br>2. The URI sits on `ipfs.io`: keep it. | open |
| **Reused content** | The document's URI, site, telegram, name or symbol already appeared on an earlier coin | Filters, by exclusion. | A recycled document is a relaunch or a copycat, and those rarely live long enough to rise. | 1. Coin A launched yesterday with the telegram `t.me/abc`.<br>2. Coin B today carries the same link.<br>3. B is a copy: skip it. | new |

### D3 This coin's life before the fire

Facts true on this coin before the event. Each fact carries the time it becomes known: a fact
known at age 60 s cannot serve a fire at age 20 s.

| Name | Idea | Meaning | Why it matters | Example | Status |
| --- | --- | --- | --- | --- | --- |
| **Operators in** | How many distinct operator structures have bought this coin | Measures. The count itself; the two rows under it cut it in opposite directions. | Each operator chose this coin on its own, so the count says how many independent decisions are already in it. | 1. 9 different operator structures have bought the coin.<br>2. The count is 9: the cuts below decide what that means. | keep · ev 3.3 |
| └ **Many operators** | The count is high (as read: ≥ 8) | Filters. The high end of **Operators in**. | Many independent choices mark a coin worth trading. | 1. 9 operator structures have bought.<br>2. 9 clears the cut of 8.<br>3. The coin passes. | keep · ev 3.3 |
| └ **Operators 3-7** | The count is a handful, not a crowd | Filters. The middle band of **Operators in**, so it refuses the coins **Many operators** likes best. | A few bots show interest; a crowd of them is a swarm that can leave together. | 1. 5 operator structures are on the coin: it passes.<br>2. At 9 it is out, where **Many operators** would let it in. | new |
| └ **Early operators** | Two or more operator structures print inside the coin's first minute | Filters. **Operators in** counts them at the fire; this counts them at birth. | A bot that chooses a coin before the crowd arrives saw something in it early. | 1. An operator structure buys at age 20 s.<br>2. Another buys at age 45 s.<br>3. Two inside the first minute: the coin passes. | new |
| **Tools in** | How many distinct public apps' users have bought this coin: more is better | Measures. **Operators in** counts bots; this counts the apps real people trade through. | Many apps means the coin was found by many kinds of retail buyers, and more of them are still coming. | 1. Axiom, Photon and GMGN buyers have all bought.<br>2. The count is 3. | open |
| **Wallets on the coin** | How many distinct wallets have printed on this coin before the fire | Measures. The two rows under it cut this count in opposite directions. | It is the plainest measure of how many people the coin has reached. | 1. 300 distinct wallets printed before the fire.<br>2. The count is 300: the cuts below decide what that means. | open |
| └ **Broad crowd** | The count is high | Filters. The high end of **Wallets on the coin**. | A broad crowd, rather than a few bots, is what keeps a coin trading. | 1. The cut is 200 wallets.<br>2. 300 wallets printed before the fire.<br>3. 300 clears it: the coin passes. | new |
| └ **Few holders** | The count of distinct public buyers is low (as read: ≤ 46) | Filters. The low end of **Wallets on the coin**, and it counts buyers only. | A coin that is not crowded yet still has its buyers ahead of it. | 1. The cut is 46 buyers.<br>2. 30 public buyers have bought so far.<br>3. 30 is under it: the coin passes. | red · ev 7 (rule 3b) |
| **Quiet birth** | Few prints land in the coin's first slots: fewer is better | Filters. **Early operators** asks who came early; this asks how loud the birth was. | A sniper swarm at birth is a bag that dumps on the first rise. | 1. 3 prints land in the first 10 slots.<br>2. A sniper launch shows 30 or more.<br>3. 3 is quiet: the coin passes. | new |
| **Early life** | vsol at age 60 s sits inside a band, neither flat nor spent | Filters. It exists only from age 60 s, so a younger fire cannot read it. | A moderate first minute shows real demand that has not spent itself. | 1. The band read is vsol 50-70.<br>2. At age 60 s vsol is 58: the coin passes.<br>3. A fire at age 30 s cannot use it: the fact does not exist yet. | open · case 10 |
| **Made a hill** | The coin has completed at least one hill | Filters. **Flush recovered** asks whether it defended a rise; this asks whether it ever made one. | A coin that has risen once has buyers who can lift it, which is the whole bet. | 1. vsol went from 40 to 50 earlier.<br>2. Price rose (50 / 40)^2 = +56 %.<br>3. That is a hill: the coin passes. | keep |
| **Flush recovered** | The coin fell into a flush and climbed back to the peak it fell from | Filters. **Made a hill** counts the rise; this counts the recovery after one. | Buyers defended the coin once, so they can defend the dip we buy. | 1. The peak price is 1.00.<br>2. Price falls to 0.75, 25 % under it.<br>3. Price returns to 1.00: the coin passes. | new |
| **High peak, mid-curve** | The coin's peak vsol is already high, and vsol now sits back in the middle of the curve | Filters. Two facts at once: the proof of demand, and the room left. | The peak proves buyers exist, and the fall back leaves the room they need to do it again. | 1. The peak was vsol 90.<br>2. vsol now is 70.<br>3. Room to the wall is (115 / 70)^2 - 1 = +170 %: the coin passes. | new |
| **Floor held** | vsol has never fallen below a named floor | Filters. **Flush recovered** needs a round trip; this only needs the floor never to break. | A floor that holds means holders never gave up and sold it down, so buyers sit under the price. | 1. The floor is vsol 45.<br>2. Since age 30 s the lowest vsol is 47.<br>3. The floor held: the coin passes. | new |
| **Frenzy sells unabsorbed** | Of this coin's earlier sells ≥ 1 SOL inside a frenzy, few were bought back (as read: at most a third) | Filters, by exclusion. A sell is bought back when the coin makes a new high within 15 s of it. | If this coin's dips do not recover, the dip we buy may not recover either. | 1. The coin had 6 sells ≥ 1 SOL inside a frenzy.<br>2. Only 1 was followed by a new high within 15 s.<br>3. 1 of 6 is under a third: the coin is out. | red · case 34 |
| **Operators round-tripped** | Operator structures that bought this coin have already sold out of it: fewer is better | Filters, by exclusion. **Operators in** counts who is here; this counts who has been and gone. | Bots that already took their profit here will not pay for our rise. | 1. 7 operator structures bought this coin.<br>2. All 7 have sold out.<br>3. The coin is out. | new |

### D4 Loss door: which coin goes to -50 %

Read inside a vsol band. Below vsol 42.43 a -50 % move is impossible, so vsol alone separates
nothing.

| Name | Idea | Meaning | Why it matters | Example | Status |
| --- | --- | --- | --- | --- | --- |
| **Low bundle** | Wallets that bought in the creation slot hold a small share of live supply (as read: under 20 %) | Filters. **Low bundled buyers** reads the same kind of group in any slot, not only the creation slot. | Creation-slot wallets are one team with one trigger; with a small share they cannot drop the coin in one slot. | 1. Creation-slot wallets hold 12 % of live supply.<br>2. 12 % is under the 20 % cut: the coin passes.<br>3. At 35 % it is out. | keep · ev 3.7 |
| └ **Low bundled buyers** | Wallets whose first buy landed in a same-slot group of 3 or more on one ix structure hold no more than 28.57 % of live supply (booked) | Filters. It reads any slot, which is why rule 1 uses it: past age 158 s the creation-slot share reads 0. | A group that bought on one trigger sells on one trigger, and the price never warns first. | 1. At age 90 s, 4 wallets buy in one slot through one ix structure.<br>2. They hold 35 % of live supply.<br>3. 35 % is over 28.57 %: the coin is out. | keep · ev 1.28 |
| **Public-app majority** | Holders whose first buy of this coin went through a public app hold most of the live supply (booked: ≥ 70 %) | Filters. A holder is classed once, at its first buy: a later buy through another app does not move it, and all its tokens stay in that class. **Low bundled buyers** reads who bought together; this reads what they bought through. | The supply outside the public apps is mostly one bot swarm that sells in a single slot. A swarm program looks wide, but its wallets never come back, which is what the 2-buys half of the public-app test catches. | 1. Yesterday Axiom had 5,000 buying wallets at 6 buys each: public. A swarm program had 2,800 wallets at 1.05 buys each: not public.<br>2. Wallet A bought first through Axiom, later through the swarm program: all 300 M of its tokens count as public.<br>3. Public holders hold 800 M of 1,000 M live tokens = 80 %.<br>4. 80 % clears 70 %: the coin passes. | keep · ev 1.28 |
| **Top holders** | The biggest one, or biggest ten, holders' share of live supply: smaller is better | Measures. **Low bundle** reads a group that bought together; this reads size alone, whoever they are. | One wallet with a large share can drop the price by itself, and no one else has to agree. | 1. The top wallet holds 15 % of live supply.<br>2. Its single sell drops the price hard.<br>3. A cap on that share keeps such coins out. | red · case L4 |
| **Low creator share** | The creator holds a small share of live supply (as read: under 10 %) | Filters. **Top holders** reads the biggest holders, whoever they are; this reads the one wallet that can rug by design. | A creator with a small bag has little to dump, so the worst single outcome is smaller. | 1. The creator holds 4 % of live supply.<br>2. 4 % is under the 10 % cut.<br>3. The coin passes. | open · ev 3.7 |
| **Launchpad safety counts** | The coin passes the venue's own safety counts: few snipers, few fresh wallets, many buyers | Filters. It is the panel a human trader reads on the venue page, taken as one cut. | Each count is a way of saying the coin is not one team's setup, so the crowd on it is real. | 1. The panel shows 12 snipers and 30 % fresh wallets.<br>2. Both sit under their limits, and the buyer count is above its own.<br>3. The coin passes. | red · ev 3.7 |

### D5 Off-chain, stored at the fire

| Name | Idea | Meaning | Why it matters | Example | Status |
| --- | --- | --- | --- | --- | --- |
| **Coin is visible now** | The coin sits high on the venue's feed, or carries many replies or a livestream, when the print lands: more attention is better | Filters. **Rank crossing** is the moment visibility changes; this is the state it leaves behind. | Attention is how strangers find the coin, and those strangers are the buyers who lift the price after our fill. | 1. At the fire the coin is #5 on the feed with 40 replies.<br>2. Both sit above the cut: the fire is allowed. | open |
| └ **Rank crossing** | The coin crosses into a feed rank, or becomes king-of-the-hill, on this print | Fires. **Coin is visible now** is the standing state; this is the change itself. | Visibility jumps at that moment, so a wave of new buyers can follow. | 1. The coin sits at #12 on the feed.<br>2. It becomes king-of-the-hill.<br>3. We fire on that change. | new |

---

## E - Event

### E1 A listed ix structure acts

Traders who decide tend to fire right after specific ix structures act: 2-4 Axiom buys, 2 Photon
buys, one nonce buyer. Keep named lists of those ix structures and fire when a listed one acts.
The list is the asset. Build a list from real decision prints; score it on the full tape, never
on the trader's own coins.

#### E1.1 The lists

| Name | Idea | Meaning | Why it matters | Example | Status |
| --- | --- | --- | --- | --- | --- |
| **8dtx list** | Fire when one of the 155 ix structures that 8dtx buys right after prints (`8dtx-event-structures.json`) | Fires. One trader's list, read off its own reactions, so it describes 8dtx rather than the market. | A trader that makes money has already decided whose buy is worth following; the list is that decision, reusable by us. | 1. A structure on the list buys the coin.<br>2. We fire on that buy. | open · mt3 6.1 |
| **Tools** | Fire when a tool buy lands: Axiom, Photon, GMGN, Bloom, Trojan, Terminal | Fires. **Operators** fires on one owner's bot; this fires on the apps thousands of people trade through. | Retail arrives through public apps, so a tool buy is the crowd showing up. | 1. An Axiom buy lands on the coin.<br>2. We fire. | open · mt3 6.1 |
| └ **Tool + nonce** | The same, and the transaction also carries a nonce, e.g. `Axiom Trade\|CU\|ATA\|N\|F` | Fires. A tool order that was signed before it was sent. | A pre-signed order was prepared, so it is a decision taken earlier rather than a reaction to the last print. | 1. An Axiom buy carries `AdvanceNonceAccount`.<br>2. We fire on it. | open · mt3 6.1 |
| **Nonce buyers** | Fire when a pre-signed buy lands that creates no throwaway account | Fires. **Tool + nonce** needs a tool as well; this takes any pre-signed buy. | Preparing a transaction in advance is deciding in advance, which is the opposite of racing someone else's print. | 1. A buy carries `AdvanceNonceAccount`.<br>2. It carries no `CreateAccountWithSeed`.<br>3. We fire. | open |
| **Direct by instruction** | Fire on direct pump.fun buys, kept as one list per instruction name | Fires. Each instruction name is scored as its own list, never pooled. | A different instruction is different trading software, so it is a different trader with different odds. | 1. `BuyExactQuoteInV2` buys are one list.<br>2. `BuyExactSolIn` buys are another.<br>3. Each is scored on its own. | open · mt3 6.1 |
| **Operators** | Fire when an operator structure buys | Fires. **Tools** fires on a crowd's app; this fires on one owner's bot. | One owner's bot is one decision, not many people happening to arrive together. | 1. A structure used by 12 wallets has printed 900 times this week: it counts as an operator structure.<br>2. It buys.<br>3. We fire. | open · mt3 6.1 |
| **Campaign structure** | Fire on a buy that sets its priority fee before its compute-unit limit | Fires. That order is unusual, and it marks one pusher's own bot. | If the bot that pushes a coin is starting, a coordinated push is starting with it. | 1. A buy carries `SetComputeUnitPrice` before `SetComputeUnitLimit`.<br>2. We fire. | red · ev 7 (campaign-break) |
| **Never listed** | Seed racers and aggregator routes never go on a list | Filters, by exclusion. It applies to every list above. | They react to someone else's buy, so their print is already behind the decision we want to follow. | 1. A `CreateAccountWithSeed` buy lands: not a trigger.<br>2. A Jupiter route lands: not a trigger. | keep (exclude) |

#### E1.2 Trigger forms

| Name | Idea | Meaning | Why it matters | Example | Status |
| --- | --- | --- | --- | --- | --- |
| **Silence, then K** | The coin is silent for a stretch, then one listed structure lands several buys in one slot | Fires. Silence and the count work together: neither half fires on its own. | A listed trader breaking a quiet tape is deciding now, not continuing something already priced. | 1. The coin is silent 10 slots.<br>2. One Axiom structure lands 2 buys in one slot.<br>3. Both halves hold: we fire. | open · mt3 6.1 |
| **Two lists agree** | Two different listed structures buy in the same slot | Fires. **Silence, then K** needs one structure and a quiet tape; this needs two structures and no quiet. One structure splitting a buy into near-equal parts counts once. | Two independent actors deciding in the same 0.4 s is harder to explain as noise than one. | 1. A Photon buy and a nonce buy land in one slot.<br>2. We fire.<br>3. Three equal buys from one structure would count as one, and not fire. | open · mt3 6.1 |
| **First run here** | A listed structure's first run on this coin, not its return | Fires. **Structure restart** is the same structure coming back; this is it arriving for the first time. | A first buy is a fresh decision about this coin, where a return can be a top-up of one already made. | 1. A nonce buyer has never printed on this coin.<br>2. It buys now.<br>3. We fire. | red · ev 7 (C16) · mt3 6.1 |
| └ **Any new structure** | Any ix structure's first print on this coin, listed or not (`struct_first_here`) | Fires. The same event without the list. | A new kind of buyer arriving is new money, whoever sent it. | 1. The coin has seen 14 distinct ix structures.<br>2. A 15th prints.<br>3. We fire. | open · mt3 6.1 |
| **Identity family** | Fire on any print of the "who printed" classes: `structure_burst`, `clip_step_up`, `buy_after_sells`, `two_struct_slot`, or a tool | Fires. One family, read as one term, never ANDed together. | The five overlap, and together they say a known kind of printer just acted, which is what 8aaRWu buys right after. | 1. A tool buy lands: we fire.<br>2. Or a structure buys again after 10 silent slots: we fire.<br>3. Either one counts as the same family. | open · mt3 6.1 |

### E2 Silence, then a spend

Name whose silence (the coin's, or one ix structure's) and who breaks it. A listed breaker is E1.

| Name | Idea | Meaning | Why it matters | Example | Status |
| --- | --- | --- | --- | --- | --- |
| **Burst start** | A tool or router structure's first buy above a floor after at least 2 slots of its own silence, on a mid-life coin (booked: ≥ 0.5 SOL) | Fires. **Structure restart** wants 10 slots of silence and takes any structure; this wants 2 and only a tool or a router. | A structure that pauses and comes back with size is making a new decision, not continuing one. | 1. A Photon structure's last print here was 3 slots ago.<br>2. It buys 0.8 SOL.<br>3. Both halves hold: we fire. | keep (with slow-wall) · ev 6.4 |
| └ **Structure restart** | Any ix structure silent 10 or more slots on this coin buys again (`structure_burst`) | Fires. 8aaRWu's lead class. Among identity-family prints it is not a separating term. | The same bot returning to a coin it left is a second look at it. | 1. A structure last printed here 40 slots ago.<br>2. It buys again.<br>3. We fire. | open · mt3 6.1 |
| └ **Live return** | The structure was silent 10 or more slots here while the coin kept printing (`live_return`) | Fires. **Structure restart** does not care what the coin did meanwhile; this needs the coin to have stayed alive. | Coming back to a tape others are still trading is a second look at a live coin, not a poke at a dead one. | 1. The structure is gone 20 slots.<br>2. Other wallets keep buying through those slots.<br>3. The structure buys again: we fire. | open · mt3 6.1 |
| └ **Silence band** | The structure's silence falls inside a band, cut on both sides, rather than "silent or not" | Fires. **Structure restart** cuts one side only; this refuses a gap that is too short **and** one that is too long. | Too short is a bot continuing, and already in the price; too long is a bot that has forgotten the coin. | 1. The band read is 15-40 slots.<br>2. The structure is silent 25 slots, then buys: we fire.<br>3. At 5 or at 100 slots, no fire. | open · mt3 6.1 |
| └ **Returning operator** | An operator structure buys after 10 or more slots of its own silence | Fires. **Burst start** takes tools and routers; this takes the single-owner bots instead. | One owner coming back with size is one decision, where a tool's return is many people's. | 1. An operator structure is silent 10 slots on this coin.<br>2. It buys 0.5 SOL.<br>3. We fire. | red · ev 7 (C5) |
| **Silent-coin breaker** | The first buy above a floor after the whole coin has been silent 10 or more slots (as read: ≥ 0.5 SOL) | Fires. The rows above read one structure's silence; this reads the coin's. | Size landing on a quiet tape is a decision, and it can wake the buyers who left. | 1. No print on the coin for 10 slots, about 4 s.<br>2. A 0.6 SOL buy lands.<br>3. We fire. | red · ev 7 (C5) |
| └ **New breaker** | The buy that breaks the coin's silence comes from an ix structure the coin has never seen | Fires. **Silent-coin breaker** takes any breaker; this takes only a first-time one. | Fresh money waking a quiet coin is a new decision, not a bot circling back to its own position. Red on the sentences run, with no book of its own: why it failed is not recorded. | 1. The coin is silent 10 slots.<br>2. A structure new to this coin buys.<br>3. We fire. | red |

### E3 An operator's plan is unfinished

An operator executing a position in legs still has SOL to spend. The leftover is that ix
structure's remaining spend on this coin. One fire per (coin, ix structure).

| Name | Idea | Meaning | Why it matters | Example | Status |
| --- | --- | --- | --- | --- | --- |
| **First leg** | The first buy above a floor of an operator structure that usually buys a coin in two or more bursts | Fires. **Later leg** fires on that bot's next burst; this fires on its first. | A bot that buys in legs still has SOL to spend here, and its own next legs push the price we bought at. | 1. The bot buys in 2 or more bursts on 30 % of its coins.<br>2. It makes its first 0.6 SOL buy here.<br>3. We fire, ahead of its next legs. | red · ev 7 (C9) |
| └ **Later leg** | That structure's second or later burst on this coin | Fires. The same plan, joined after it has proved itself. | The first leg can be a probe; a second leg is the plan continuing. | 1. The bot's first burst lands at age 40 s.<br>2. Its second burst lands at age 60 s.<br>3. We fire on the second. | red · ev 7 (C11) |
| **Second burst** | Any ix structure that already burst here, went silent 10 or more slots, and starts again above a floor (as read: ≥ 0.5 SOL) | Fires, once per coin and structure. **Later leg** needs a bot with a known habit of legging in; this takes any structure's second round. | A structure that comes back for a second round on the same coin is still interested in it. | 1. A structure bursts at age 30 s.<br>2. It goes quiet.<br>3. At age 80 s it buys 0.6 SOL: we fire, once. | red · ev 7 (C12) |
| **Clip left** | The structure has spent something here but is still well under what it usually spends on a coin | Fires. **Clip vs its median** is the same comparison as a number; this is the cut made from it. | Money still to spend here is buying that has not hit the price yet. | 1. The bot usually spends 1 SOL a coin.<br>2. It has spent 0.4 SOL here, 40 % of its usual.<br>3. Above the floor and under the cap: we fire. | red · ev 7 (C9) |
| └ **Clip vs its median** | This buy divided by the median this operator structure spends on its **other** coins (`clip_vs_med`) | Measures. It is blank for a tool, whose median is the market rather than one owner's habit. **Step size** compares this buy with its own last buy here; this compares it with its habit elsewhere. | A buy far under the bot's habit says the position here is not finished. | 1. On its other coins the bot spends about 1 SOL each.<br>2. This buy is 0.4 SOL.<br>3. The ratio is 0.4. | open · mt3 6.1 |
| **Clip step-up** | A buy bigger than that same ix structure's own last buy on this coin (`clip_step_up`) | Fires. **Step size** is how much bigger; this is the event itself. | A setup that comes back with a bigger buy is adding to a position, not closing one. | 1. A structure buys 0.3 SOL here.<br>2. Later it buys 0.5 SOL.<br>3. The second buy is bigger: we fire on it. | open · mt3 6.1 |
| └ **Known structure, new wallet** | The ix structure has printed here before, but the wallet behind this print has not (`fresh_return`) | Fires. **Fresh buyer** asks only about the wallet; this pairs a new wallet with a structure the coin already knows. | New money arriving through a printer that is already interested is a second person on the same idea. | 1. An Axiom structure has printed here before.<br>2. A wallet with no print here buys through it.<br>3. We fire. | open · mt3 6.1 |
| └ **Hard step-up, fresh buyer** | A step-up that also moves the price hard on its own, leaves the reserve small, and comes from a wallet new to the coin | Fires. 8dtx2t's first position. **Clip step-up** alone fires far more often; this is its loudest corner. | A big buy that moves a shallow pool hard, from money that was not here before, is conviction rather than maintenance. | 1. vsol is 50, so the reserve is 20 SOL.<br>2. A new wallet's 1.1 SOL step-up takes vsol to 51.1: the price moves +4.4 %.<br>3. Every cut holds: we fire. | red · ev 7 (rule 3b) |
| └ **Step size** | This buy divided by that structure's last buy on this coin (`step_x`, above 1 is a step up) | Measures. **Clip step-up** is the yes/no; this is the size of the step. | A big step is an operator adding hard; a small one is a bot ticking along. | 1. The structure bought 0.1 SOL.<br>2. Now it buys 0.8 SOL.<br>3. The step is 8x. | open · mt3 6.1 |
| **Under its own sell** | The coin is quiet under a price this structure sold at, it has not bought back, it sells here often, and it usually buys back after selling; not an operator structure | Fires. **Operators only** is the same read for the bots this one excludes. | A bot that sells high and buys back lower is likely to buy back here, and its buy lifts the price. | 1. It has 12 sells here, the last at vsol 70.<br>2. The coin sits quiet at vsol 60.<br>3. It buys back on 30 % of its coins: we fire. | red · ev 7 (C10) |
| └ **Operators only** | The same, for the operator structures the booked term leaves out | Fires. It is the excluded half of **Under its own sell**, kept because the exclusion was never tested on its own. | One owner's plan is easier to predict than a public tool's crowd. | 1. A 10-wallet operator structure sold at vsol 70.<br>2. The coin is quiet at vsol 60.<br>3. We fire. | red |
| **First print this hour** | The structure's first print on this coin in the current UTC hour (`first_this_hour`) | Fires. **Structure restart** measures the gap in slots; this measures it against the clock hour. | A bot returning to a position it has left alone for that long is deciding again, not adjusting. | 1. The structure last printed here 70 minutes ago.<br>2. It prints now.<br>3. We fire. | open · mt3 6.1 |
| **Arrives from another coin** | The structure printed on a different coin seconds ago, and now prints here | Fires. **Wallet rotating in** reads the same move per wallet; this reads it per structure. | A bot moving from one coin to another is moving its money, and the money arrives here. | 1. It bought coin A 2 s ago.<br>2. It buys here now.<br>3. We fire. | red · ev 7 (C13) |
| └ **Rotating in** | The same, among identity-family prints (`rotating`, `elsewhere_dt`) | Measures. The same story read on a print that already matters for another reason. | It says the printer is rotating rather than sitting, without needing its own fire. | 1. A structure buys coin A.<br>2. Two seconds later its tool buy lands here.<br>3. The gap is 2 s. | open · mt3 6.1 |
| └ **Wallet rotating in** | Seconds since this wallet last printed on a different coin (`w_rot`) | Measures. **Arrives from another coin** reads the ix structure; this reads the single wallet. | Money moving out of one coin and into this one is money that has just been freed. | 1. The wallet sold coin A 3 s ago.<br>2. It buys here.<br>3. The gap is 3 s. | open · mt3 6.1 |

### E4 A count crosses a line

| Name | Idea | Meaning | Why it matters | Example | Status |
| --- | --- | --- | --- | --- | --- |
| **Second outsider** | The second wallet other than the creator buys, while the coin is young | Fires, once per coin. **First outsider** fires one buy earlier. | One stranger can be the creator's own second wallet; a second is a pattern of strangers. | 1. The creator bought, then one other wallet.<br>2. A second new wallet buys at age 40 s.<br>3. We fire. | red · ev 3.7 |
| └ **First outsider** | The first wallet other than the creator buys | Fires, once per coin. | The first person to back a launch is the first evidence anyone wants it. | 1. The creator buys.<br>2. The first other wallet buys.<br>3. We fire. | new |
| **Operator count** | The number of distinct operator structures printing in the last 2 s or 5 s rises through a line (`npro2`, `npro5`; read at 1) | Fires on the crossing. **Operators in** counts them over the coin's whole life; this counts them inside seconds. | Several bots choosing the same coin inside a few seconds is a decision the market is making right now. | 1. The line is one structure in 2 s.<br>2. No operator structure printed in the last 2 s.<br>3. One prints: we fire. | open · mt1 5.2d |
| **Structure count** | The number of distinct ix structures printing in the last 2 s rises through a line (`nb2`) | Fires on the crossing. **Operator count** counts only bots; this counts every kind of sender. | A tape that suddenly broadens means many kinds of buyer arrived at once, not one bot working. | 1. The line is 6 structures in 2 s.<br>2. The 6th distinct structure prints inside the window.<br>3. We fire. | open · mt1 5.2d |
| **First operator after snipers** | The first operator-structure buy above a floor on a coin whose earlier prints are only the creator and seed racers | Fires, once per coin. **First outsider** counts any wallet; this waits for the first real bot. | The first bot to arrive after the launch noise is the first decision that was not automatic. | 1. The creator buys, then 5 seed racers buy.<br>2. An operator structure buys 0.6 SOL.<br>3. We fire, once on this coin. | red · ev 7 (C15) |
| **Buy flow spike** | A buy whose SOL is a large multiple of what the coin has been taking per slot (`flow_spike`, over the 30 slots before it) | Fires. **Large among recent** compares the buy with recent print sizes; this compares it with the coin's whole buying rate. | Demand that suddenly accelerates tends to carry on, and this buy is the acceleration. | 1. The coin took 0.1 SOL of buying a slot over the last 30 slots.<br>2. This buy is 0.25 SOL.<br>3. That is 2.5x the rate, above the 1.82x read: we fire. | open · mt3 6.1 |

### E5 After sellers

| Name | Idea | Meaning | Why it matters | Example | Status |
| --- | --- | --- | --- | --- | --- |
| **Post-hill dip buy** | A buy above a floor that is not a seed racer, while the price is still down after a hill (as read: ≥ 0.5 SOL) | Fires. **Size into a dip** needs the buy to open a run; this only needs the coin to have made a hill first. | Someone buying the pullback of a coin that just proved it can rise is buying the same coin cheaper. | 1. The coin makes a +60 % hill.<br>2. It pulls back 20 %.<br>3. A 0.6 SOL Axiom buy lands: we fire. | red · ev 7 (machine print in the dip) |
| **Flush stops** | The first buy above a floor after a flush, once vsol has made no new low for 10 or more slots | Fires, once per flush, and never on a seed racer. **Size into a dip** fires while the price is still falling; this waits for the fall to stop. | Waiting for the low to hold means the sellers are done, so the buy we follow is not catching a knife. | 1. Price is 30 % off the peak.<br>2. Ten slots pass with no new low.<br>3. A 0.7 SOL buy lands: we fire. | red · ev 7 (C9) |
| **Frenzy flip sell** | A public sell of 1 SOL or more inside a frenzy, with a new high in the last 20 s, by a seller who bought within the last 30 s; we fire on the sell (booked, rule 1's event) | Fires. **Quick flip sell** is the same seller with none of the other three terms. | The frenzy's next buyers absorb this drop within seconds, so buying into it means buying just before them. | 1. The coin makes a new high.<br>2. A wallet that bought 20 s ago sells 1.5 SOL.<br>3. We buy the dip, and the frenzy's buyers lift it back. | keep · ev 1.22 (rule 1's event) |
| └ **Seller all out** | The same sell, and it leaves the seller holding nothing | Fires. It reads the seller's bag after the sell, which **Frenzy flip sell** ignores. | A seller with tokens left can sell again while we hold; one who is out cannot hit us twice. | 1. He holds 1 M tokens.<br>2. He sells all 1 M for 1.5 SOL.<br>3. He has nothing left: no more selling from him. | red · case U5 |
| └ **Young-coin flip sell** | The same sell on a much younger coin, without the established-coin permission (9999hu's class) | Fires. **Frenzy flip sell** is read on old, broad coins; this is the same print where the coin is seconds old. | Our own buy moves a young coin further, but its frenzy keeps running after our fill. | 1. The coin is 15 s old.<br>2. A 1 SOL sell lands.<br>3. We fire. | open · ev 1.27 |
| **Seller at a loss** | A public sell by a wallet that is under its average cost on this coin (`seller_loss`) | Fires. **Quick flip sell** reads how recently the seller bought; this reads whether the seller is losing. | A wallet giving up sells into any bid, and the drop it causes is a discount rather than news. | 1. A wallet bought at vsol 70.<br>2. It sells 1 SOL at vsol 60, under its cost.<br>3. We fire on the sell. | open · mt3 5.2 88887Q |
| **Quick flip sell** | A public sell by a wallet that bought this coin within the last 30 seconds (`seller_recent`) | Fires. It is **Frenzy flip sell** without the frenzy, the size or the new high. | A flipper's sell is a scheduled exit, not an opinion about the coin, so the price it dents recovers. | 1. A wallet buys at age 40 s.<br>2. It sells at age 55 s.<br>3. We fire on the sell. | open · mt3 5.2 88887Q |
| **Capitulation cascade** | A public sell of 1 SOL or more from a seller at a loss, inside a burst of selling, with the price already down over the last 10 s and a busy tape | Fires. **Seller at a loss** is one of its five parts; this needs all of them at once. | Panic overshoots what the coin is worth, and the snap back is the move we want to be in. | 1. Price is -10 % over 10 s.<br>2. A wallet 4 % under water sells 1 SOL inside a burst.<br>3. The tape is busy: we fire. | red · ev 1.16 |
| **Size into a dip** | A buy of about 1 SOL that opens a run while the price is down over the last 10 s | Fires. **Post-hill dip buy** needs an earlier hill; this needs the buy to start a run. | Size arriving into a falling price is conviction, and the run it opens is the recovery. | 1. Price is -5 % over 10 s.<br>2. A 1 SOL buy opens a run.<br>3. We fire. | dead · ev 7 (AbQcLH burst start) |
| └ **Mid-tape burst start** | The same print class on an older coin (8dtx2t's class) | Fires. The parent is read on the launch tape; this is the same print past the first minutes. | The rest of the burst lands after our fill, so a burst that is only starting still has buying left. | 1. The coin is 150 s old.<br>2. After a pause, a 0.8 SOL buy starts a burst.<br>3. We fire. | open · mt1 5.2e |
| **After a sell run** | The first buy after two or more public sells in a row (`buy_after_sells`) | Fires. The booked event is stricter: a buy of 0.5 SOL or more, not a seed racer, after three sells in a row. | Buyers come back once a run of sellers is spent, and the first of them buys the low. | 1. Two sells land in a row.<br>2. A buy lands next.<br>3. We fire. | red · ev 7 (C14) |
| **Buy after the crowd** | The first buy once the last hill's buyers hold less than half of what they bought | Fires. **Crowd left** is the same fact as a permission; this fires on the buy that follows it. | The people who would sell into our rise have already sold, so a new buyer starts with a clear path. | 1. The last hill's buyers have sold 60 % of their tokens.<br>2. They hold 40 %, under half.<br>3. A new buy lands: we fire. | new |
| **After a structure sold** | Anyone's first buy after an ix structure sold this coin | Fires. **After a sell run** counts sells whoever sent them; this waits for one known structure to be the seller. | A bot's exit is a known quantity of supply, and the first buyer after it is taking the discount it left. | 1. An operator structure sells out.<br>2. Someone buys.<br>3. We fire. | open · mt3 6.1 |
| **Top holders sold** | The biggest holders have sold out of the coin | Fires. **Top holders** measures what they hold; this is the moment they are gone. | The largest single dump risk leaves with them, so what is left cannot drop the price the same way. | 1. The top 3 holders hold 30 % of supply.<br>2. All 3 sell out.<br>3. Nobody left can drop it that far: we fire. | new |

### E6 This print

| Name | Idea | Meaning | Why it matters | Example | Status |
| --- | --- | --- | --- | --- | --- |
| **Two structures, one slot** | Exactly two different ix structures print on this coin in one slot (`two_struct_slot`) | Fires. **Alone in its slot** is the same count at one; three or more structures is neither. | Two senders deciding inside the same 0.4 s are two independent opinions, not one bot working. | 1. An Axiom buy lands in a slot.<br>2. A nonce buy lands in the same slot.<br>3. Nothing else prints in it: the print counts. | keep (term) · ev 7 (zigzag turn) · mt3 6.1 |
| **Alone in its slot** | No other ix structure prints on this coin in the same slot (`alone_in_slot`) | Fires. The sender's own repeat prints do not break it: it counts structures, not prints. | A print nobody is crowding is a decision of its own, and our fill does not land behind a wave. | 1. A structure buys three times in one slot.<br>2. No other structure prints in that slot.<br>3. It still counts as alone. | open · mt3 5.2 3Xk2Eu · mt3 6.1 |
| **Big price step** | This print moves the coin's price hard on its own (`mvk`, the square of the vsol it added) | Fires. **Size buy** reads the SOL paid; this reads what that SOL did to the price, which depends on how deep the pool is. | Someone paying well above the last price is paying up, and on a thin pool that is what a decision looks like. | 1. vsol is 50.<br>2. A 1.1 SOL buy takes it to 51.1.<br>3. The price moves (51.1 / 50)^2 - 1 = +4.4 %. | open · mt3 6.1 8aaRWu |
| **Fresh buyer** | The wallet on this print has never printed on this coin (`wal_n` = 0) | Fires. **Known structure, new wallet** adds that the ix structure has been here before. | New money, rather than a bot topping up a position it already holds. | 1. The wallet has no print on this coin.<br>2. It buys.<br>3. It counts as fresh. | open · mt3 6.1 |
| └ **Known hopper wallet** | The wallet is new to this coin but has printed a lot elsewhere (`wal_n_any`) | Measures. **Wallet age** reads how old the key is; this reads how busy it has been. | A wallet that trades constantly is someone's working account, not a one-off buyer. | 1. The wallet has 200 prints on other coins.<br>2. It makes its first print here.<br>3. The count elsewhere is 200. | open · mt3 6.1 |
| **Wallet back here** | This wallet has printed here before: how long ago, and how much SOL it has taken out minus put in (`w_gap`, `w_nethere`) | Measures. **Fresh buyer** is the opposite case, a wallet with no history here. | A wallet that comes back to a coin it already traded knows something about it, and whether it is up or down here says what. | 1. A wallet sold here 40 s ago.<br>2. It buys again.<br>3. The gap is 40 s, and it is 0.3 SOL up here. | open · mt3 6.1 |
| └ **Launch buyer back** | The wallet's first print here landed inside the coin's first 10 seconds, and it is printing again (`w_early`) | Measures. **Wallet back here** counts any return; this counts a return by someone who was there at birth. | A launch buyer coming back is an insider or a sniper adding, which is a planned move rather than a stray one. | 1. A wallet bought at age 2 s.<br>2. It buys again at age 300 s.<br>3. It counts as a launch buyer back. | open · mt3 6.1 |
| **Wallet takes money out** | The share of this wallet's other coins where it has taken out more SOL than it put in (`w_up`) | Measures. It reads the wallet's record across the tape, not its behaviour on this coin. | A wallet that usually leaves a coin with more than it brought picks coins better than the crowd does. | 1. The wallet has traded 12 other coins.<br>2. On 5 of them it took out more than it put in.<br>3. The share is 5 / 12 = 42 %. | open · mt3 6.1 |
| **Big for this wallet** | This buy divided by this wallet's mean earlier buy on the tape (`w_szrel`) | Measures. **Large among recent** compares the buy with the coin's tape; this compares it with the wallet's own habit. | A wallet buying far above its habit is surer than usual, whatever the absolute size is. | 1. The wallet's mean buy is 0.2 SOL.<br>2. It buys 1 SOL here.<br>3. That is 5x its habit. | open · mt3 6.1 |
| **Copied buyer** | A wallet buys, and on earlier days at least 3 wallets on other ix structures bought right after it, on many of its coins and rarely before it | Fires. **Slow copy-traders** adds that those followers are slow enough for us to get in front of. | If people copy this wallet, their buying lands a moment after ours and lifts the price we paid. | 1. On earlier days the same 3 wallets bought 1-4 s after this one, on a quarter of its coins.<br>2. It buys this coin.<br>3. We fire, ahead of the copies. | red · ev 7 (copied buyer) |
| └ **Slow copy-traders** | The same, and its followers arrive a second or more later with enough SOL to move the price | Fires. The parent counts followers; this counts only the ones slower than our own 115 ms seat. | A follower faster than us is already in the price we pay; only a slow one pays us. | 1. Its followers bring 1.5 SOL, 1-4 s behind it.<br>2. At vsol 70 that lifts the price about 4 %.<br>3. It buys: we fire, and their buying lands after ours. | red · ev 7 (copied buyer) |
| **Wallet age** | Seconds since this wallet's first print anywhere, capped at 6 hours (`w_age`) | Measures. **Known hopper wallet** counts its prints; this measures how long it has existed. | A key made minutes ago is a burner or a new operator; an old key is a hand the market already knows. | 1. The wallet's first print anywhere was 2 minutes ago.<br>2. Its age is 120 s: a fresh key. | open · mt3 6.1 |
| **Block position** | Where the print sits in its block, by transaction index (`txi`) | Measures. **High fee** reads what it paid to land early; this reads where it actually landed. | Landing near the front of a block means it paid or bundled to get there, which is urgency. | 1. The block holds 1,200 transactions.<br>2. This print is index 3.<br>3. It landed at the front. | open · mt3 6.1 |
| **Answers a sell** | The print before this one is a sell, or a sell landed earlier in this slot (`prev_sgn`, `ss_sell`) | Fires. **After a sell run** needs two or more sells in a row; this needs only the one before. | A buy that takes a seller's tokens is absorbing supply, not chasing a price that is already moving up. | 1. A 1 SOL sell lands.<br>2. In the same slot a 0.8 SOL buy lands.<br>3. The buy answers the sell. | open · mt3 6.1 |
| **Same-structure follow** | The print before this one came from the same ix structure (`prev_same`) | Measures. **Answers a sell** reads what the previous print did; this reads who sent it. | A bot chaining its own orders is one decision split in pieces, not two deciders agreeing. | 1. An Axiom buy lands.<br>2. The next print is another Axiom buy.<br>3. It counts as a follow. | open · mt3 6.1 |
| **Gap since last step-up** | Seconds between this step-up and the previous step-up on this coin (`clip_gap`) | Measures. **Clip step-up** is the event that fires; this only says how long ago the one before it landed. | Two step-ups close together are one operator adding in quick steps; a long gap means the last one is stale. | 1. A step-up lands at age 40 s.<br>2. Another lands at age 41 s, and Clip step-up fires on it.<br>3. The gap is 1 s, so this fire follows a fresh one. | open · mt3 6.1 |
| **Step-ups so far** | How many step-ups the coin has had before this print (`clip_n`) | Measures. **Gap since last step-up** reads the time to the last one; this counts them all. | More step-ups looked like more operators adding. It is ruled out as an event because it grows with the coin, which makes it the crowd count at another grain, so it belongs to permission. | 1. The coin has had 29 step-ups.<br>2. This is the 30th.<br>3. The count reads 29. | red (as an event) · derive 6.1 |
| **Change since last step-up** | Each tape fact minus its own value at the coin's previous step-up: the step, buy SOL, prints, bursts, the price move, the flow spike, 25 in all | Measures. Every row above reads a level at this print; this reads how much that level has moved since the last print of the same kind. | A trader can pick on a change rather than a level: louder than last time, or quieter than last time. Read against the coin's own previous step-up, every one of the 25 is flat, so the change carries nothing the level does not. | 1. The last step-up here came with 0.4 SOL bought in the 2 s before it.<br>2. This one comes with 1.0 SOL.<br>3. The change is +0.6 SOL. | red · derive 6.1 |
| **Best of the moment** | How many step-ups are printing on other coins in the last 5 or 60 s, and where this one ranks among them on step and size | Measures. Every other row reads this coin alone; this reads the whole tape's competition for the same moment. | A trader with one position takes the best print available, so a print alone in its moment should be taken more often. It is not: the rate holds across every count of rivals. | 1. Six other coins step up inside the minute.<br>2. This print's step is the biggest of them.<br>3. It ranks first among its rivals. | red · derive 6.1 |
| **Coin state alone** | The coin's own state at the print, laddered on its own: age, reserve, and how crowded it is | Measures, as a permission rather than an event. **Wallets on the coin** and **Room under the wall** are the same facts as door and permission rows; this asks whether they alone name the prints one trader takes. | Half of a pick can be the coin rather than the print, so the coin's state is derived on its own instead of being assumed. Both folds name the same three terms and both hold out of sample, but a ladder on shuffled coins pays about as much, because a coin fact partly reads how many prints a coin has. | 1. The coin is 90 s old.<br>2. Its reserve is 38 SOL and 24 step-ups have printed.<br>3. All three terms pass: the coin is one he buys on. | red · derive 9 |
| **High fee** | The print pays a priority and tip fee at the high end of what the tape pays | Fires. **High for its structure** compares the same fee with what that one sender usually pays. | Paying extra to land sooner is urgency, and urgency is someone acting on something. | 1. Most prints tip 0.0005 SOL.<br>2. This one tips 0.002 SOL.<br>3. It sits in the top quarter of the tape. | keep (term) · ev 7 (zigzag turn) |
| └ **High for its structure** | The fee this print pays, divided by the geometric mean of what this ix structure paid on its 20 or more earlier prints (`fee_rel`) | Measures. Blank under 20 earlier prints, so no cut on it can fire there. **High fee** compares against the tape, which a bot that always pays high would pass every time. | Urgent for that bot in particular, which is a change in its behaviour rather than its standing habit. | 1. The bot has 60 earlier prints and usually tips 0.0001 SOL.<br>2. It tips 0.0006 SOL.<br>3. That is 6x its own usual. | open · mt3 6.1 |
| **Size buy** | A public buy of 1 SOL or more on a mid-life coin | Fires. **Big price step** reads what the buy did to the price instead of what it cost. | Big money is conviction, and it is the simplest form of it to read. | 1. The coin is 200 s old.<br>2. A public 1.2 SOL buy lands.<br>3. We fire. | red · ev 7 (G1) · mt3 6.1 |
| **Large among recent** | This print's SOL divided by the 75th percentile size of the 20 prints before it (`large_recent`) | Measures. 1.0 is exactly that percentile, and the cut read sits below it, at 0.75x. **Size buy** uses a fixed SOL figure instead. | Big for this coin's own tape means something on a quiet coin and nothing on a busy one, which a fixed size cannot tell apart. | 1. The 75th percentile of the last 20 prints is 0.2 SOL.<br>2. This print is 0.16 SOL.<br>3. That is 0.8x, above the 0.75x read. | open · mt3 6.1 |

### E7 Clock

| Name | Idea | Meaning | Why it matters | Example | Status |
| --- | --- | --- | --- | --- | --- |
| **Fixed age** | Fire when the coin reaches a set age, with no print needed | Fires, on the clock alone. It is the control every event is measured against. | It shows what buying a coin at that age earns by itself, so an event has to beat it to be worth anything. | 1. The age is 120 s.<br>2. Every coin that reaches 120 s gets a fire.<br>3. Its book is the floor for every E row. | open |
| └ **Young big sell** | The first public sell of 1 SOL or more while the coin is still seconds old (as read: age ≤ 16.3 s) | Fires, once per coin. **Young-coin flip sell** fires on the same kind of print but needs a flipper as the seller; this takes the launch's first big exit whoever sold it. | Every launch has one, at about the same moment of its life, so it marks the same point on every coin. | 1. A coin is 9 s old.<br>2. Its first public 1 SOL sell lands.<br>3. We fire. | red · mt2 6.2 |
| **First size after T** | The first buy above a floor once the coin is past a set age, with no silence needed | Fires, once per coin. **Silent-coin breaker** needs a quiet tape first; this needs only the clock. | It catches the first real buyer once the launch noise has cleared, without waiting for the tape to go quiet. | 1. The coin passes age 60 s.<br>2. At age 75 s a 0.6 SOL buy lands.<br>3. We fire. | new |

### E8 Graveyard

Do not rebuild these as the event.

| Name | Idea | Meaning | Why it matters | Example | Status |
| --- | --- | --- | --- | --- | --- |
| **Zigzag turn** | The first print after the price turns up from a low | Fires. A price-path event: it reads the shape of the chart and nothing else. | Buying the turn means buying before the rise. Dead: from the price alone a turn and a falling knife look the same, and the detector is right 5.6 % of the time where it needs about 45 %. | 1. Price falls 30 %.<br>2. It bounces 15 % off the low.<br>3. We fire. | dead |
| **New-buyer acceleration** | Several wallets buy the coin for the first time inside a few slots | Fires. **New buyers still arrive** is the same count as a permission, which is not dead. | New buyers arriving faster looks like a move starting. Dead: they are the move, so by the time the count rises the price has already paid for it. | 1. Five new buyers land inside 10 slots.<br>2. We fire. | dead |
| **Seed-racer burst after silence** | Seed racers pile into a coin that has been silent | Fires. **Silent-coin breaker** is the same silence broken by anyone. | Racers arriving together looks like demand waking up. Dead: a seed racer only reacts to someone else's print, so it confirms a decision that is already in the price. | 1. The coin is silent 10 slots.<br>2. Four seed racers buy.<br>3. We fire. | dead |
| **Tools in one slot** | Several tool buys land in the same slot | Fires. **Two structures, one slot** is the same shape at two, and is kept as a term. | A crowd arriving at once looks like the start of a move. Dead: it is the wave itself, so our fill lands behind it. | 1. Axiom, Photon and GMGN buys land in one slot.<br>2. We fire. | dead |
| **Copy a wallet** | Buy whenever a chosen wallet buys | Fires. **Copied buyer** is the reverse idea: follow the wallet others copy, not the wallet itself. | A wallet that makes money picks well, so its pick looks worth taking. Dead: its own buy and the swarm behind it are in the price before our fill lands. | 1. A named wallet buys.<br>2. We buy the same coin. | dead |
| **Swing pullback** | Buy once the price has given back a set share of its swing, a set distance off the low | Fires. A price-path event, like **Zigzag turn**. | A pullback of a measured size looks like a cheap entry into a coin that is still rising. | 1. Price is 40 % off its swing high.<br>2. It is 5 % off the low.<br>3. We fire. | red · ev 7 (hot-tape price-path) |
| **Up-move portrait** | Buy a coin that is up over the last minute, near a recent high, with little given back, on a busy tape | Fires. It is a standing state rather than a print, which is why it reads like a screen. | Strength tends to continue, so the strongest-looking coin should be the one to buy. | 1. Price is +30 % over 60 s.<br>2. A new high 5 s ago, little given back.<br>3. We fire. | red · ev 7 (hot-tape price-path) |
| **"Now" tells** | Feed position, a creator buying again, the cadence of prints | Fires. **Coin is visible now** keeps the feed half of this as a permission. | They say the coin is hot at this moment, and hot coins move. Red: everything they read is public and slow, so the move they describe is already priced. | 1. The creator buys again.<br>2. We fire. | red |

---

## P - Permission

### P1 Curve position

Primary permissions. They do not explain why a move happens; they put the fire where a move can
pay and where a loss is bounded.

| Name | Idea | Meaning | Why it matters | Example | Status |
| --- | --- | --- | --- | --- | --- |
| **Age band** | The coin's age sits inside a window when the print lands | Filters. **Vsol band** does the same for where the coin sits on the curve. | A rule is only tested inside the part of a coin's life it was read on; outside it, nothing is known. | 1. The band is age 60-600 s.<br>2. A fire at age 200 s is allowed.<br>3. A fire at age 30 s is not. | keep |
| └ **Established coin** | The coin is well past its first minutes and a broad crowd has bought it (booked: age ≥ 158 s, ≥ 368 distinct non-creator buyers) | Filters. It is rule 1's permission, and it pairs age with crowd: either half alone lets the wrong coins through. | A frenzy on a young, thin coin dies with the seller who started it; on an old, broad one the crowd absorbs the sell. | 1. The coin is 200 s old.<br>2. 400 wallets other than the creator have bought it.<br>3. Both halves clear: the fire is allowed. | keep · ev 1.22 |
| └ **Established on 9999hu** | The same pair of cuts, applied to 9999hu's sell event | Filters. The parent is read on the hot tape; this tries it on a one-shot trader's fires, which mostly sit on young coins. | If old, broad coins absorb sells better, the same cut should help any sell event. | 1. 9999hu's fire lands on a coin 15 s old.<br>2. The coin is not established.<br>3. The fire is skipped. | red · mt2 9.1 |
| **Room under the wall** | The coin still sits well under the wall when the print we fire on lands (booked: vsol ≤ 100) | Filters, as a safety term rather than a fitted one. **Headroom** asks the same question from the target's side. | The curve ends at 115: above about 105 a +20 % target no longer fits under it, and the trade closes on the completing buy at a price the curve no longer offers. | 1. After the trigger print vsol is 95: the price can still rise (115 / 95)^2 - 1 = +47 %, so the fire is allowed.<br>2. After it vsol is 105: 105 x sqrt(1.2) is over the wall, so it is not. | keep · ev 1.20 |
| └ **Room on 9999hu** | The same cut, applied to 9999hu's sell event, whose fires already sit under vsol 100 | Filters. It changes nothing on that pool, which is what the read shows. | The same safety, tried on another event. | 1. 9999hu's fire lands at vsol 80.<br>2. 80 is under the cut: allowed.<br>3. Every one of its fires already was. | red · mt2 9.1 |
| **Sell-reactive buyers** | How many wallets on this coin have bought within 300 ms of an earlier public sell of 1 SOL or more | Measures. **Frenzy flip sell** fires on the sell itself; this counts who has been buying such sells back on this coin. | Dip-buying bots that already caught this coin's sells will catch the next one, and their buying lifts the price after our fill. | 1. Five wallets here bought within 300 ms of a sell of 1 SOL or more.<br>2. A new big sell lands.<br>3. Those five are likely to buy it back. | red · case 41 |
| **Vsol band** | vsol sits inside a window when the print lands | Filters. **Room under the wall** cuts the top end only; this cuts both. | Where on the curve the fire sits decides both how far the coin can rise and how far it can fall. | 1. The band is vsol 45-80.<br>2. A fire at vsol 60 is allowed.<br>3. A fire at vsol 90 is not. | keep |
| **Headroom** | The room left to the wall covers the target we are trading for | Filters. It is arithmetic, not a fit: the room is (115 / vsol)^2 - 1, so a +100 % target needs vsol 81 or under. | A target that sits above the wall cannot be reached at any price, so the trade is dead before it starts. | 1. At vsol 81 the room is exactly +100 %: allowed.<br>2. At vsol 90 only +63 % fits: a +100 % target is not allowed. | keep |
| **Pool alive** | The coin still holds SOL and is still printing | Filters. **Calm tape** wants few prints; this wants more than none. | A pool nobody trades has no buyer to sell our tokens to, whatever the price says. | 1. The last print landed 3 s ago.<br>2. The reserve is 12 SOL.<br>3. The coin is alive: allowed. | new |

### P2 Windowed tape metrics

Each metric has its own meaning and concentrates the pool: it buys "this coin is not decaying",
worth about the toll. None is a cause or an event, because windowed flow is the price path. The
window is seconds, slots or prints, with a lag so it cannot read the event. Definitions:
[_!___metrics.md](_!___metrics.md).

| Name | Idea | Meaning | Why it matters | Example | Status |
| --- | --- | --- | --- | --- | --- |
| **Buying outruns selling** | More SOL is bought than sold in the window before the print: more is better | Filters. Spelled from the windowed flow metrics, as net SOL or as the buy share of the gross. | A coin where buying leads is not decaying, so our fill is not the last one in. | 1. In the last 10 s, 5 SOL is bought and 3 SOL sold.<br>2. Net is +2 SOL, and the buy share is 63 %.<br>3. Both lead: the fire is allowed. | keep |
| **Calm tape** | Few prints land on the coin in the seconds before ours: fewer is better | Filters. **Buying outruns selling** reads which way the money goes; this reads how loud the tape is, either way. | A busy tape is a pile-in that has already moved the price, so our fill lands behind it. | 1. The cap read is 25 prints in 5 s.<br>2. 12 prints land in the last 5 s.<br>3. Under the cap: the fire is allowed. | red · ev 7 (rule 3b) |
| **Mid-range price** | The price sits between the window's low and its high, at neither end | Filters. Spelled as the drop below the window's high, or the rise above its low. **Long past its best** reads the coin's whole life instead of a window. | At the high the move is already spent and we buy the top; far below it the coin is falling and we catch a knife. | 1. The 30 s high is 1.00 and the low 0.80.<br>2. The price is 0.92, 8 % under the high.<br>3. It sits in the middle: the fire is allowed. | keep |
| **Long past its best** | The price is far under the coin's all-time high and has been for a long time: both smaller is better | Filters. **Mid-range price** reads seconds; this reads the coin's whole life. | A coin that has sat far under its best moment for minutes has lost the buyers who made that moment. | 1. The all-time high is 1.00, set 90 s ago.<br>2. The price is 0.60, 40 % under it.<br>3. Far under for that long: the fire is skipped. | keep |
| **New buyers still arrive** | Wallets buying this coin for the first time keep appearing in the window before the print: more is better | Filters. It counts wallets, not SOL, so one large buyer cannot fake it. | The next rise is paid for by people who are not in yet; if they stopped coming, nobody is left to lift it. | 1. In the last 30 s, 12 wallets buy the coin for the first time.<br>2. The cut is 10.<br>3. Above it: the fire is allowed. | keep |
| **No new low** | vsol has made no new low since the event we fired on | Filters. **Flush resumes** is the same test as an exit, after we are in. | The low holding means the sellers did not take control after the event, so the reason we fired still stands. | 1. At the event the low is vsol 52.<br>2. vsol has not gone under 52 since.<br>3. The fire is allowed. | new |

### P3 Skin in

| Name | Idea | Meaning | Why it matters | Example | Status |
| --- | --- | --- | --- | --- | --- |
| **Creator holds** | The creator has not sold any of his own coin | Filters. A survival term: it keeps coins alive longer and also raises the loss rate, because it keeps us in coins that fall slowly. | A creator still holding has not rugged yet, so the worst outcome has not happened. | 1. The creator bought 1 SOL at launch.<br>2. He has sold nothing since.<br>3. The fire is allowed. | keep · ev 3.4 |
| **Crowd left** | The buyers of the last hill hold less than half of what they bought | Filters. **Buy after the crowd** fires on the buy that follows this state. | Their selling is already spent, so it will not land on top of our rise. | 1. The last hill's buyers bought 10 M tokens.<br>2. They hold 4 M, 40 % of it.<br>3. Under half: the fire is allowed. | keep |
| └ **Crowd still in** | The buyers of the last hill still hold nearly everything they bought | Filters. The opposite end of **Crowd left**, kept because both stories are arguable. | Buyers who have not taken profit still believe in the coin, and belief is what lifts it again. | 1. The last hill's buyers bought 10 M tokens.<br>2. They still hold 9 M.<br>3. Nearly all of it: the fire is allowed. | red |
| **Pusher holds** | The structure that bought the most in the last hill has not sold | Filters. **Pusher sells** is the exit built on the same actor. | Whoever paid most to lift the price has the most reason to want it higher still. Red on the sentences run, with no book of its own: why it failed is not recorded. | 1. The pusher bought 3 SOL during the last hill.<br>2. It has sold nothing.<br>3. The fire is allowed. | red |
| **Firing structure holds** | The ix structure whose print we fire on has not sold this coin | Filters. **Firing structure sells** is the exit on the same actor. | The one we are following still wants the price up, which is the reason we are buying at all. | 1. We fire on an operator structure's buy.<br>2. It has no sell on this coin.<br>3. The fire is allowed. | new |
| **Creator rebought** | The creator has bought again in the slots just before our print | Filters. **Creator holds** only asks that he has not sold; this asks that he is adding. | A creator putting more of his own money in is betting on the coin, which is stronger than simply not leaving. | 1. The window is the last 10 slots.<br>2. The creator bought 0.3 SOL five slots ago.<br>3. The fire is allowed. | new |
| **Near-even holders** | The share of supply held by wallets whose loss is between -20 % and 0 % right now: fewer is better | Measures. **All losers** counts every losing holder, however deep; this counts only the ones a +20 % rise brings back to even. | A +20 % rise carries exactly these holders back to what they paid, and many of them sell there to get out even. That selling lands inside the rise we are trading for. | 1. The price is 1.00.<br>2. A holder paid 1.10, so he is 9 % down, inside the band.<br>3. At +20 % the price passes 1.10 and he sells into our rise. | red · case U5 |
| └ **Get-out-even wall** | The SOL those holders would get if they sold, divided by the SOL buyers must spend to lift the price +20 %: lower is better | Measures. **Near-even holders** is their share of supply; this weighs it against the buying our target needs. | Above 1, their selling alone can absorb the whole rise we are trading for, so the target cannot be reached. | 1. At vsol 50, a +20 % rise needs about 4.8 SOL of buying.<br>2. The near-even holders hold 6 SOL of tokens.<br>3. 6 / 4.8 = 1.25: their selling beats the buying, and the rise stalls.<br>4. With 1 SOL of tokens it is 0.2, too small to matter. | red · case U5 |
| └ **All losers** | The share of supply held by every wallet that is down, however far | Measures. A control for **Near-even holders**: if the two read the same, the -20 % ~ 0 % band adds nothing. | A deep loser cannot get back to even inside a +20 % rise, so his tokens do not meet our target the way a near-even holder's do. | 1. The price is 1.00.<br>2. A holder paid 2.00 and is 50 % down.<br>3. He counts here and not in Near-even holders. | red · case U5 |
| **Big winners** | The share of supply held by wallets sitting on a large gain: fewer is better | Measures. **Holders in profit** is the same idea at a lower band and is the one that was measured. | Holders far in front take profit into any rise, so their tokens meet our buy on the way up. | 1. The price is 1.00.<br>2. A holder paid 0.60 and is 67 % up.<br>3. As the price rises he sells into it. | new |
| **Holders in profit** | The share of held tokens that are up 20 % or more when the print lands | Measures. **Big winners** is the same fact at a higher band, untested; this one carries the read. | Read on a frenzy it runs the other way round: the frenzy that goes on to stop us out has **fewer** holders in profit, because it is the younger, thinner one. It reads as maturity, like age and reserve, so more is better. | 1. Two frenzies fire at the same age.<br>2. On the one that reaches the take profit, more of the held tokens are up 20 %.<br>3. On the one that stops out, fewer are. | red · ev 1.14 |

### P4 ix makeup of the recent tape

The permission side of E1.

| Name | Idea | Meaning | Why it matters | Example | Status |
| --- | --- | --- | --- | --- | --- |
| **Not a seed racer** | The print we fire on is not from a seed racer | Filters. **No racer lately** looks at the tape around it; this looks only at the print itself. | A seed racer only reacts to someone else's buy, so its print is never the decision we mean to follow. | 1. A `CreateAccountWithSeed` buy lands.<br>2. It is a seed racer: no fire. | keep |
| **No racer lately** | No seed racer has printed in the slots just before ours | Filters. **Not a seed racer** clears the trigger print; this clears the tape around it. | If the racers have not arrived yet, the move they react to is still ahead of us rather than behind. | 1. The window is the last 10 slots.<br>2. No `CreateAccountWithSeed` print lands in it.<br>3. The fire is allowed. | new |
| **Tools only** | All the buying in the run, or in the last slots, came through tool structures and none from seed racers | Filters. **Tools** fires on one such buy; this asks that the recent buying is all of that kind. | Retail arriving through apps keeps arriving; racers dump what they just bought. | 1. 1.2 SOL is bought in the last 5 slots.<br>2. All of it came through Axiom and Photon.<br>3. The fire is allowed. | open |
| **Buy SOL band** | The recent buy SOL sits above noise and below the point where the move is already made | Filters. Both ends matter, which is what makes it a band rather than a floor. | Too little buying is noise; too much means the price moved before our fill could land. | 1. The band read is 0.5-2 SOL.<br>2. Tools bought 1.2 SOL over this slot and the one before.<br>3. Inside the band: the fire is allowed. | open |

### P5 Tape state already true

| Name | Idea | Meaning | Why it matters | Example | Status |
| --- | --- | --- | --- | --- | --- |
| **Many structures** | Many distinct ix structures are printing in the last seconds, with no single one dominating them | Filters. **Structure count** fires on the moment that count crosses a line; this is the standing state. | Many independent senders at once is real demand; the same count from one busy bot is not. | 1. 8 structures printed in the last 5 s.<br>2. The largest of them sent 30 % of the prints.<br>3. The fire is allowed. | open · ev 1.11 |
| **Coin already silent** | The coin is silent when our print lands | Filters. **Silent-coin breaker** fires on the print that breaks the silence; this is the same silence as a standing state. | A print landing on a quiet tape is a fresh decision rather than one more in a wave. | 1. No print on the coin for 12 slots.<br>2. Our trigger print lands.<br>3. The fire is allowed. | new |
| **No sells just before** | No sell landed immediately before this print (`sell_run` = 0) | Filters. **After a sell run** is the opposite case, and fires on it. | A buy that does not follow sellers is not catching a price still on its way down. | 1. The last three prints are buys.<br>2. No sell run is open.<br>3. The fire is allowed. | open · mt3 6.1 8aaRWu |
| **No big sells lately** | Few public sells of 1 SOL or more in the last 30 s | Filters. **No sells just before** reads the prints next to ours; this reads the last half-minute for size. | A large holder on the way out sells more than once, and the rest of that exit would land on our position. | 1. The cap read is 4.<br>2. Two sells over 1 SOL landed in the last 30 s.<br>3. Under the cap: the fire is allowed. | red · ev 7 (rule 3b) |
| **Long-hold buyers** | The share of the last seconds' buy SOL that came from wallets which usually hold a coin five minutes or more: more is better | Measures. Each buyer's habit is read from its earlier coins on the tape, not from this one. | Money from long holders stays in the coin; money from flippers comes back out within seconds, straight into our position. | 1. In the last 10 s, 2 SOL is bought.<br>2. 1.4 SOL of it came from wallets that usually hold five minutes or more.<br>3. That is 70 %: most of the new money stays. | new |
| **Slow re-entry state** | A rested, proven coin: quiet for a minute, two hills behind it, a deep pullback, mid-curve, and a structure returning | Filters, as a conjunction of five states. Each part is its own row elsewhere; this is the combination. | A coin that rose twice, rested and is being picked up again by a buyer who knows it can rise again. | 1. Two hills, then a 50 % pullback.<br>2. A minute of quiet at vsol 65.<br>3. A returning structure buys: the fire is allowed. | red · ev 7 (zigzag turn) |

---

## X - Exit

### X1 Static

| Name | Idea | Meaning | Why it matters | Example | Status |
| --- | --- | --- | --- | --- | --- |
| **Clock** | Close after a fixed time, whatever the price is doing | Sells. The control every other exit is measured against. | It shows what the event earns by itself, so any exit that cannot beat it is adding nothing. | 1. The clock is 45 s.<br>2. We sell 45 s after the fill, at whatever the price is.<br>3. Its book is the floor for every X row. | keep (control) |
| └ **Clock on 9999hu** | A clock set to the hold time 9999hu itself uses | Sells. The parent picks the time from our own book; this copies the trader's. | If the trader's move is over when it leaves, its own hold time is where our edge ends too. | 1. We fill on 9999hu's event.<br>2. It usually closes in 20-30 s.<br>3. We sell 25 s after the fill. | red · mt2 8.2 |
| **Take profit** | Close at a fixed gain | Sells. A control, like **Clock**, but on price instead of time. | It shows how often each size of gain is actually reached, which is what a target has to be chosen from. | 1. The target is +40 %.<br>2. The price reaches +40 %.<br>3. We sell. | keep (control) |
| **Unarmed trail** | Sell once the price falls a set share from its best since our fill | Sells. **Armed trail + stop** waits for a gain before the trail starts; this trails from the first second. | It rides a move as far as it goes and leaves when the move gives back, without guessing a target. | 1. The trail is 30 %.<br>2. The best since our buy is +30 %.<br>3. The price falls 30 % from that peak: we sell. | keep |
| **Armed trail + stop** | A hard stop until the trade is up enough to arm a trail, then a trail from the peak (booked: arm +21 %, trail 36 %, stop -43.75 %, cap 1200 s) | Sells. **Unarmed trail** has no stop and no arming; this pairs the two. | A winner is only given room once it has proved it is one, and until then the loss is capped. | 1. Before +21 %, a fall to -43.75 % stops the trade out.<br>2. At +21 % the trail arms.<br>3. After that, a fall of 36 % from the peak sells. | keep · ev 4.7 |
| └ **No stop** | The same, without the hard stop before the trail arms | Sells. It is **Armed trail + stop** with its safety net removed. | A position that dips deep and comes back is not thrown away at the bottom. Red on the sentences run, with no book of its own: why it failed is not recorded. | 1. The position never reaches +21 %.<br>2. It holds to the 1200 s cap, whatever the loss. | red |
| **Tail keeper** | A wide target, a wide trail and a long cap, to keep the rare very large winner | Sells. **Bracket** is the opposite shape: small target, short clock. | On an entry where a few trades pay for all the others, cutting the winners early is what loses the money. | 1. The target is +100 %, the trail 50 %, the cap 1200 s.<br>2. A trade runs to +100 %: we sell there.<br>3. The rest are given room to get there. | keep |
| **Bracket** | A small target, a wide stop and a short clock, whichever comes first (booked: +20 %, -60 %, 90 s) | Sells. It is rule 1's exit. **Tail keeper** trades the other shape. | An event that bounces quickly pays a small sure target, and the wide stop is there for the rare collapse rather than for fitting. | 1. The trade reaches +20 %: we sell.<br>2. Or it falls to -60 %: we sell.<br>3. Or 90 s pass: we sell. | keep · ev 1.22 |
| └ **Bracket on 9999hu** | The same shape, sized to 9999hu's own pool | Sells. The parent's numbers are read on the hot tape; these are read where 9999hu buys. | The same exit has to be re-sized on a different pool, because its coins move at a different speed. | 1. +15 %: we sell.<br>2. Or -40 %: we sell.<br>3. Or 70 s pass: we sell. | red · mt2 8.2 |
| └ **Longer clock** | The same bracket, held for longer before the clock closes it | Sells. Only the clock changes; the target and the stop stay. | Some trades reach the target only after the short clock would have closed them. | 1. The target and stop are unchanged.<br>2. The clock is 240 s instead of 90 s. | red · ev 1.20 |
| └ **Breakeven** | Once the trade is up a little, close it at the fill rather than let it turn into a loss | Sells. It is a floor added under **Bracket**, not a replacement for it. | A gain that has shown itself is real money, and giving it all back is the easiest loss to avoid. Red on the sentences run, with no book of its own: why it failed is not recorded. | 1. The trade is up +5 %.<br>2. The price falls back to our fill.<br>3. We sell at 0 %. | red |
| └ **Half out** | Sell half the position at the target and let the rest run | Sells. It splits the parent between **Bracket** and **Tail keeper**. | It banks the part of the move that is certain and keeps a ticket on the part that is not. | 1. At +20 % we sell half.<br>2. The other half trails behind the peak. | red · ev 1.24 |
| └ **Trail after target** | Once the trade is up a little, replace the fixed target with a tight trail | Sells. **Unarmed trail** trails from the fill; this starts the trail at the gain the bracket would have taken. | A fixed target caps a run that was still going, and a tight trail keeps more of it. | 1. The trade reaches +10 %.<br>2. It peaks at +18 %.<br>3. It falls 7 % from that peak: we sell. | red · ev 1.24 |
| └ **Stepped trail** | The higher the peak goes, the looser the trail gets | Sells. **Unarmed trail** uses one width the whole way; this widens it as the trade wins. | A big winner swings harder, so a trail that fits a small move throws it away too early. | 1. The peak reaches +30 %.<br>2. The trail widens from 5 % to 10 %.<br>3. A fall of 10 % from the peak sells. | red · ev 1.24 |
| **Headroom target** | Take profit at a share of the room left to the wall, instead of a fixed gain (rule 1b takes 0.4 of it) | Sells. **Take profit** uses one number everywhere; this scales the target to where the coin sits on the curve. | Far from the wall a coin has room to run, and near it there is nothing to take, so one fixed target is wrong at both ends. | 1. At vsol 70 the room is (115 / 70)^2 - 1 = +170 %.<br>2. Four tenths of that is +68 %.<br>3. The target for this trade is +68 %. | open · ev 1.25 |
| **Payoff-shape exit** | Pick the exit family from how the event's own trades pay | Sells. It is a rule for choosing between the rows above, not an exit of its own. | An event that pays small and often needs a target; one that pays rarely and hugely needs a trail. Using the wrong family wastes the edge that is there. | 1. Most trades on an event end near +5 %, with no long tail.<br>2. That shape wants a small fixed target.<br>3. **Bracket** is chosen over **Tail keeper**. | open |
| **Static abort** | Sell if the trade is not up by a set amount after a set time | Sells. **Clock** closes everything at the time; this closes only what has not moved. | A trade that has not moved early is unlikely to move at all, and the money is better in the next one. Dead with no book of its own: what closed it is not recorded. | 1. The rule is +5 % by 30 s.<br>2. At 30 s the trade is +2 %.<br>3. We sell. | dead |

### X2 The tape stops (a state cut)

A cut that can fire while the position is up is a clock. Every cut here fires only below the
fill.

| Name | Idea | Meaning | Why it matters | Example | Status |
| --- | --- | --- | --- | --- | --- |
| **Buyers stop** | New buyers stop arriving while we are under the fill | Sells. **New buyers still arrive** is the same count as a permission before the fire. | The people who would lift the price out of a loss are the ones not in yet, so if they stop coming there is nothing left to wait for. | 1. We are under the fill.<br>2. No new buyer arrives for 20 s.<br>3. We sell. | red · ev 1.24 on rule 1 (open · ev 4.7 elsewhere) |
| **Flush resumes** | vsol makes a new low after we bought a flush | Sells. **No new low** is the same test as a permission before the fire. | The bounce we bought has failed, and the selling that made the flush is not finished. | 1. We buy after a flush whose low is vsol 50.<br>2. vsol falls to 49, a new low.<br>3. We sell. | keep |
| **Pre-event low** | vsol goes back to the low it sat at before the event we fired on | Sells. **Flush resumes** needs a new low; this only needs the event's own move to be given back. | Everything the event did has been undone, so the reason we are in the trade no longer exists. | 1. Before the event the low is vsol 55.<br>2. After our fill vsol returns to 55.<br>3. We sell. | red · ev 7 (C14) |
| **Firing structure silent** | The ix structure we fired on stops printing while we are under the fill | Sells. **Firing structure sells** waits for it to leave; this only waits for it to stop. | Its buying was the reason we bought, and a buyer that has gone quiet is no longer that reason. Red on the sentences run, with no book of its own: why it failed is not recorded. | 1. We are under the fill.<br>2. No print from that structure for 20 slots.<br>3. We sell. | red |
| **Operators stop** | No new operator structure arrives while we are under the fill | Sells. **Buyers stop** counts any new wallet; this counts only bots. | Bots find the coins worth trading first, so when none of them arrives any more, there is nothing here. | 1. We are under the fill.<br>2. No new operator structure prints for 30 s.<br>3. We sell. | red · ev 7 (C15) |
| **Fees fall** | The prints after ours stop paying extra to land quickly | Sells. **High fee** is the same reading as an event term before the fire. | Urgency is what makes a move run; when nobody is paying for speed any more, the hurry is over. | 1. We are under the fill.<br>2. Priority fees on the coin drop back to its usual level.<br>3. We sell. | new |
| **Burst give-back** | Trail against the peak of the burst we joined, rather than the coin's own peak | Sells. **Unarmed trail** trails the coin; this trails the actor we followed. | We bought one structure's burst, so the burst fading is the end of our reason, whatever the coin's chart does. | 1. The burst's last buy lands at a price of 1.00.<br>2. The price falls to 0.80, 20 % under it.<br>3. We sell. | new |

### X3 An actor sells

| Name | Idea | Meaning | Why it matters | Example | Status |
| --- | --- | --- | --- | --- | --- |
| **Firing structure sells** | The ix structure we fired on sells this coin | Sells. **Firing structure silent** leaves on its silence; this waits for the sell itself. | The one we followed is taking its money out, and it knows its own reason better than we do. | 1. We fire on a structure's buy.<br>2. That structure prints a sell.<br>3. We sell. | keep · ev 7 (C11) |
| **Pusher sells** | The structure that bought the most in the last hill sells | Sells. **Pusher holds** is the same actor as a permission before the fire. | The buyer who lifted the price is the one whose selling drops it furthest. | 1. The last hill's biggest buyer sells.<br>2. We sell. | open |
| **Creator sells** | The creator prints a sell of any size | Sells. **Creator holds** is the same actor as a permission before the fire. | A creator selling can be the first leg of a rug, and waiting to find out is expensive. | 1. The creator sells any amount.<br>2. We sell. | new |
| **Ride the creator** | Hold while the creator holds, and trail once he sells | Sells. **Creator sells** leaves at once; this keeps the position on a trail instead. | His exit says the plan is over, but the crowd may still push for a while, and a trail keeps that without giving it all back. | 1. We hold while the creator holds.<br>2. The creator sells.<br>3. From then we trail 20 % behind the peak. | new |
| **Firing structure leaves** | The ix structure we fired on starts buying a different coin | Sells. **Firing structure silent** reads its silence here; this reads its activity elsewhere. | Its attention and its money have moved on, so it will not buy this coin again. | 1. Our trigger structure buys a different coin.<br>2. We sell. | red · ev 7 (C13) |
| **Sell to the copiers** | While we hold, a wallet with slow copy-traders buys: sell into the copies that follow | Sells. **Slow copy-traders** is the same pair of actors as an entry. | Their buying arrives on a known delay, so it is a moment when someone is certain to be taking tokens. | 1. We are up 8 %.<br>2. A wallet with slow copy-traders buys.<br>3. We sell in the next few seconds, into their buying. | new |
| **Sell into a buy** | Once we are up enough, sell on the first big public buy or hard price step | Sells. **Sell to the copiers** knows who the buyer will be; this takes whoever shows up. | A big buy is both a high price and a counterparty, which is exactly what a seller needs. | 1. We are up +12 %.<br>2. A 1.2 SOL buy lands.<br>3. We sell into it. | red · ev 1.24 |

---

## R - Re-entry

| Name | Idea | Meaning | Why it matters | Example | Status |
| --- | --- | --- | --- | --- | --- |
| **One open per coin** | One open position per coin at a time, with no cap on how many tickets a coin gets | Filters, and it is the standing rule every sentence starts from. | A coin can be traded again and again, but never doubled up, so one bad coin cannot take two losses at once. | 1. A coin is traded 5 times in a day.<br>2. Never two positions on it at the same time. | keep |
| **Below our last exit** | Re-enter a coin only under the price we last sold it at | Filters. **Cool-down after a stop** waits on the clock; this waits on the price. | Buying back cheaper than we sold keeps the same upside with less paid for it. | 1. We sold at vsol 60.<br>2. A new fire at vsol 55 is allowed.<br>3. A new fire at vsol 62 is not. | open · ev 1.11 |
| **Round trips taken** | How many round trips we have already made on this coin | Measures. **Entry cap** is the hard version of the same count. | Each round trip takes some of what the coin had left, so the next one starts from less. | 1. We have made 3 round trips on this coin.<br>2. The count is 3, and it weighs against the next fire. | open · ev 1.11 |
| └ **Entry cap** | At most a set number of entries on one coin | Filters. **Round trips taken** is the same count used as a gradient instead of a wall. | It bounds what one coin can cost us, however good its tape keeps looking. | 1. The cap is 3.<br>2. The 4th fire on the coin is skipped. | open · ev 1.20 |
| **Cool-down after a stop** | No re-entry on a coin for a while after we were stopped out of it | Filters. **Below our last exit** allows a re-entry at a better price; this refuses one at any price for a time. | A coin that just stopped us out is usually still falling, and buying it again is paying twice for one mistake. | 1. The cool-down is 60 s.<br>2. We stop out.<br>3. No fire on that coin for the next 60 s. | open · ev 1.20 |

## S - Size

| Name | Idea | Meaning | Why it matters | Example | Status |
| --- | --- | --- | --- | --- | --- |
| **0.2 SOL** | The standing clip: every ticket buys 0.2 SOL | Sizes. It is policy, and every book is read at it unless a row says otherwise. | Small enough that our own buy barely moves the price we are buying at. | 1. Every ticket buys 0.2 SOL.<br>2. At vsol 50 that moves the price about 0.4 %. | keep |
| **Cost minimum** | The clip where the fixed cost of a leg and our own price impact balance: the square root of the fixed cost times vsol | Sizes. It is arithmetic, not a fit. **Fraction of vsol** scales with the pool as well, but with a chosen share. | Under it the fixed cost eats the trade; over it our own impact does. It is the floor a clip should not go below. | 1. At vsol 70 the formula gives 0.126 SOL.<br>2. A ticket there buys 0.126 SOL.<br>3. Half that would pay more in fixed cost than it saves in impact. | open |
| **Fraction of vsol** | The clip is a set share of the pool, so it grows with the coin | Sizes. **Cost minimum** derives a floor; this sets the whole clip from the pool. | The same SOL hurts a small pool and is lost in a large one, so a fixed clip is the wrong size nearly everywhere. | 1. The share is 0.3 % of vsol.<br>2. At vsol 60 the ticket buys 0.18 SOL.<br>3. At vsol 100 it buys 0.3 SOL. | open · ev 1.20 |
| └ **Rule 1's clip** | The largest flat clip that keeps every one of rule 1's bars on both tapes (booked: 0.35 SOL) | Sizes. **0.2 SOL** is the standing clip; this is the one rule 1 ships at. | A bigger clip earns more per trade, and the limit is the point where the rule's own bars start to fail. | 1. Every rule 1 ticket buys 0.35 SOL.<br>2. Above that its bars stop holding on one of the tapes. | keep · ev 1.20 |
