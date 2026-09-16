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
  - **Meaning**: the rule in plain words. It says whether the row **fires** (it is the print we
    buy on), **filters** (it lets a fire through or not), **measures** (it is a number another
    row cuts) or **groups** (it decides which coins are judged together), and when another row is
    close, it names that row and gives the difference in one clause.
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
mechanism) · **new** (never scored). `ev N` = [_!___evidence.md](_!___evidence.md) section N;
`ev 7 (C9)` = that rule's row in the evidence ledger; `case N` = step N of
[node-derivation/hot-tape-rule-1.md](node-derivation/hot-tape-rule-1.md) section 2; `mt1 N` /
`mt2 N` / `mt3 N` = step N of [mid-tape-rule-1.md](node-derivation/mid-tape-rule-1.md) /
[mid-tape-rule-2.md](node-derivation/mid-tape-rule-2.md) /
[mid-tape-rule-3.md](node-derivation/mid-tape-rule-3.md); `derive N` = section N of
[_!___derive.md](_!___derive.md).

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
| &nbsp;&nbsp;└ **No opening buy** | The create transaction carries no buy at all | Groups. The empty end of **Creator's opening buy**. | A creator with no stake loses nothing when the coin dies, so nothing holds the launch together. | 1. The create transaction holds a Create and no Buy.<br>2. The creator owns 0 tokens at birth.<br>3. The coin is expected to die sooner. | red |
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
| **Made a hill** | The coin has completed at least one rise of 50 % or more from a low to a peak | Filters. **Flush recovered** asks whether it defended a rise; this asks whether it ever made one. | A coin that has risen once has buyers who can lift it, which is the whole bet. | 1. vsol went from 40 to 50 earlier.<br>2. Price rose (50 / 40)^2 = +56 %.<br>3. That is a hill: the coin passes. | keep |
| **Flush recovered** | The price fell 20 % or more under its peak and then came back to it | Filters. **Made a hill** counts the rise; this counts the recovery after one. | Buyers defended the coin once, so they can defend the dip we buy. | 1. The peak price is 1.00.<br>2. Price falls to 0.75, 25 % under it.<br>3. Price returns to 1.00: the coin passes. | new |
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
| **8dtx list** | The 155 ix structures that 8dtx buys right after (`8dtx-event-structures.json`); built on "silent coin, then an ix-gated burst", so one trader's list, not a general one | Fire when one of the ix structures that 8dtx reacts to buys. | A trader's choice of whose buy to follow can be reused by us. | 1. One of the 155 listed structures buys.<br>2. We fire on that buy. | open · mt3 6.1 |
| **Tools** | Tool structures: Axiom, Photon, GMGN, Bloom, Trojan, Terminal | Fire when a public app's buy lands. | Retail arrives through public apps: a tool buy is the crowd coming. | 1. An Axiom buy lands.<br>2. We fire. | open · mt3 6.1 |
| └ **Tool + nonce** | A tool structure that also carries a nonce, e.g. `Axiom Trade\|CU\|ATA\|N\|F` | A public-app order signed in advance. | A pre-signed tool order is planned, not impulsive. | 1. An Axiom buy carries `AdvanceNonceAccount`.<br>2. It is a tool + nonce buy: we fire. | open · mt3 6.1 |
| **Nonce buyers** | Nonce, no seed account | Buys sent as pre-signed transactions. | Prepared in advance, so it decides rather than reacts. | 1. A buy has `AdvanceNonceAccount`.<br>2. It has no `CreateAccountWithSeed`.<br>3. It is a nonce buyer: we fire. | open |
| **Direct by instruction** | Direct pump.fun buys, split by instruction name | Direct pump.fun buys, one list per instruction name. | A different instruction is different trading software, so a different trader. | 1. `BuyExactQuoteInV2` buys are one list.<br>2. `BuyExactSolIn` buys are another.<br>3. Each list is scored on its own. | open · mt3 6.1 |
| **Operators** | Operator structures (≤ 50 wallets, ≥ 200 prints this week) | Fire when one bot owner's structure buys. | One bot owner's decision, not a crowd's. | 1. A structure used by 12 wallets printed 900 times this week: an operator structure.<br>2. It buys.<br>3. We fire. | open · mt3 6.1 |
| **Campaign structure** | Priority fee set before CU limit: a pusher's own bot | A buy whose compute-budget instructions come in the unusual order. | That order marks a pusher's own bot, so a coordinated push is starting. | 1. A buy has `SetComputeUnitPrice` before `SetComputeUnitLimit`.<br>2. It is a campaign structure: we fire. | red · ev 7 (campaign-break) |
| **Never listed** | Seed racers and aggregators: they land after the decision | Seed racers and aggregator routes never go on a list. | They react to someone else's buy, so their print is already late. | 1. A `CreateAccountWithSeed` buy lands: not listed.<br>2. A Jupiter route lands: not listed. | keep (exclude) |

#### E1.2 Trigger forms

| Name | Idea | Meaning | Why it matters | Example | Status |
| --- | --- | --- | --- | --- | --- |
| **Silence, then K** | The coin is silent ≥ N slots, then one listed structure's run holds K buys. Silence and count work together, neither alone | A quiet coin, then one listed structure lands K buys in one slot. | A listed trader breaking a quiet tape is a fresh decision. | 1. The coin is silent 10 slots.<br>2. One Axiom structure lands 2 buys in one slot.<br>3. K = 2: we fire. | open · mt3 6.1 |
| **Two lists agree** | K buys from ≥ 2 listed structures in one slot. One structure buying near-equal amounts is one actor splitting: count it once | Two different listed structures buy in the same slot. | Two independent actors deciding at once is stronger than one. | 1. A Photon buy and a nonce buy land in one slot.<br>2. Two lists agree: we fire.<br>3. Three equal buys from one structure count as one. | open · mt3 6.1 |
| **First run here** | A listed structure's first run on this coin, not its return | A listed structure buys this coin for the first time. | A new decision, not a top-up. | 1. A nonce buyer has never printed on this coin.<br>2. It buys now.<br>3. First run here: we fire. | red · ev 7 (C16) · mt3 6.1 |
| └ **Any new structure** | An ix structure this coin has not seen before (`struct_first_here`) | Any ix structure's first print on this coin. | A new kind of buyer arriving, listed or not. | 1. The coin has seen 14 distinct ix structures.<br>2. A 15th prints.<br>3. We fire. | open · mt3 6.1 |
| **Identity family** | Any print of the "who printed" classes (derive 5.1): `structure_burst`, `clip_step_up`, `buy_after_sells`, `two_struct_slot` or tool. One family, never an AND | Any print from a known kind of printer: a returning structure, a bigger repeat buy, a buy after sells, two structures in a slot, or a tool. | The five overlap; together they mark "a known kind of printer acts", and 8aaRWu buys right after it. | 1. A tool buy lands: fire.<br>2. Or a structure buys again after 10 silent slots: fire.<br>3. Either one is the same family. | open · mt3 6.1 |

### E2 Silence, then a spend

Name whose silence (the coin's, or one ix structure's) and who breaks it. A listed breaker is E1.

| Name | Idea | Meaning | Why it matters | Example | Status |
| --- | --- | --- | --- | --- | --- |
| **Burst start** | An ix structure's first buy ≥ 0.5 SOL after ≥ 2 slots of that structure's own silence, on a mid-life coin; tool or router structures | A tool or router structure comes back to the coin with a big buy, after a pause of its own. | A structure coming back with size is a new decision on this coin. | 1. A Photon structure's last print here was 3 slots ago.<br>2. It buys 0.8 SOL.<br>3. 3 ≥ 2 slots and 0.8 ≥ 0.5: we fire. | keep (with slow-wall) · ev 6.4 |
| └ **Structure restart** | `structure_burst`: this ix structure silent ≥ 10 slots on this coin, then it buys (8aaRWu's lead class); not a 6.1 term among family prints | An ix structure buys this coin again after 10 or more quiet slots. | The same bot returning to a coin is a second look, not noise. | 1. A structure last printed here 40 slots ago.<br>2. It buys again.<br>3. 40 ≥ 10: we fire. | open · mt3 6.1 |
| └ **Live return** | This structure silent ≥ 10 slots on this coin, and the coin still printed in those slots | The structure was away while others kept trading the coin. | A second look on a live tape, not a poke at a dead coin. Each piece alone can say nothing; the pair is the tell. | 1. The structure is gone 20 slots.<br>2. Other wallets keep buying in those slots.<br>3. The structure buys again: we fire. | open · mt3 6.1 |
| └ **Silence band** | This structure silent 10-N slots (both-side cut), not silent-or-not | The structure was away for a middle length of time: not too short, not too long. | Too short is a bot continuing, already priced; too long is a zombie. | 1. The band is 15-40 slots.<br>2. Silent 25 slots, then it buys: fire.<br>3. Silent 5 or 100 slots: no fire. | open · mt3 6.1 |
| └ **Returning operator** | The same for an operator structure, after ≥ 10 slots of its own silence | The restart, for a single-owner bot instead of a tool or router. | More fires from the same story: a bot coming back with size. | 1. An operator structure is silent 10 slots on this coin.<br>2. It buys 0.5 SOL.<br>3. We fire. | red · ev 7 (C5) |
| **Silent-coin breaker** | First buy ≥ 0.5 SOL after the coin is silent ≥ 10 slots | A big buy wakes up a quiet coin. | Size breaking a quiet tape is a decision, and it can wake the other buyers. | 1. No print on the coin for 10 slots (4 s).<br>2. A 0.6 SOL buy lands.<br>3. We fire. | red · ev 7 (C5) |
| **New breaker** | The breaker is an ix structure this coin has not seen | The buy that breaks the silence comes from a structure new to the coin. | Fresh money wakes the coin, not a returning bot. | 1. The coin is silent 10 slots.<br>2. A structure new to this coin buys.<br>3. We fire. | red |

### E3 An operator's plan is unfinished

An operator executing a position in legs still has SOL to spend. The leftover is that ix
structure's remaining spend on this coin. One fire per (coin, ix structure).

| Name | Idea | Meaning | Why it matters | Example | Status |
| --- | --- | --- | --- | --- | --- |
| **First leg** | First buy ≥ 0.5 SOL of an operator structure that buys in 2+ bursts on ≥ 25 % of its coins | The first big buy of a bot that usually buys a coin in several steps. | A bot that buys in legs has more to spend here, and its next legs push the price. | 1. A bot buys in 2+ bursts on 30 % of its coins.<br>2. It makes its first 0.6 SOL buy here.<br>3. We fire, ahead of its next legs. | red · ev 7 (C9) |
| └ **Later leg** | That structure's second-or-later burst on this coin | The same bot's next buying step on this coin. | The next leg of the same plan; more can follow. | 1. The bot's first burst lands at age 40 s.<br>2. Its second burst lands at age 60 s.<br>3. We fire on the second. | red · ev 7 (C11) |
| **Second burst** | Any ix structure already burst once on this coin, silent ≥ 10 slots, starts again with a buy ≥ 0.5 SOL; one fire per (coin, structure); not a cross-coin class | Any structure's second round of buying on this coin. | A returning structure is still interested. | 1. A structure bursts at age 30 s.<br>2. It goes quiet.<br>3. At age 80 s it buys 0.6 SOL: we fire, once. | red · ev 7 (C12) |
| **Clip left** | The structure has spent ≥ 0.3 SOL here and is still under 60 % of its median spend per coin | The bot has started buying but has not spent its usual budget. | It has money left to spend here, and that buying lifts the price. | 1. It usually spends 1 SOL per coin.<br>2. It has spent 0.4 SOL here.<br>3. 0.4 ≥ 0.3 and 40 % < 60 %: we fire. | red · ev 7 (C9) |
| └ **Clip vs its median (family term)** | Among identity-family prints from an operator structure, this buy vs that structure's median spend on its other coins, not vs its last buy here | This buy compared with what the bot usually spends on a coin. It is blank for a tool, whose median is the market. | Only part of its budget spent means more buying to come. | 1. On its other coins the bot spends 1 SOL each.<br>2. This buy is 0.4 SOL.<br>3. The ratio is 0.4. | open · mt3 6.1 |
| **Clip step-up** | `clip_step_up`: an ix structure outbuys its own last buy here | A structure buys more than its own last buy on this coin. | The same setup coming back with a bigger buy means its operator is adding, not finishing. | 1. A structure buys 0.3 SOL here.<br>2. Later it buys 0.5 SOL.<br>3. 0.5 > 0.3: we fire on the 0.5 buy. | open · mt3 6.1 |
| └ **Fresh wallet on a return** | The structure has printed here before; this wallet has not | A known structure, but this wallet's first print on the coin. | New money arriving through a known printer. | 1. An Axiom structure has printed here before.<br>2. A wallet with no print here buys through it.<br>3. We fire. | open · mt3 6.1 |
| └ **Hard step-up, fresh buyer** | The step-up buy also moves the price ≥ 4.4 % on its own, leaves the reserve ≤ 40 SOL, and comes from a wallet with no earlier print here; age ≥ 1 s (8dtx2t's first position) | A bigger repeat buy, from a new wallet, that moves a shallow pool hard. | A bigger buy that moves a shallow pool hard, from new money, is conviction. | 1. vsol is 50, so the reserve is 20 SOL.<br>2. A new wallet's 1.1 SOL step-up takes vsol to 51.1: price +4.4 %.<br>3. Both cuts pass: we fire. | red · ev 7 (rule 3b) |
| └ **Step size** | This buy over its ix structure's last buy on this coin (`step_x`; the class is > 1) | How much bigger this buy is than the structure's last one here. | A big step is an operator adding hard; a small one is noise. | 1. The structure bought 0.1 SOL.<br>2. Now it buys 0.8 SOL.<br>3. Step = 0.8 / 0.1 = 8x. | open · mt3 6.1 |
| **Under its own sell** | Coin silent, vsol below this structure's last sell here, no buy back yet; the structure has ≥ 10 sells here and buys back after selling on ≥ 25 % of its coins; not an operator structure | A bot that sells this coin often sold here higher; the price is now under that sell and it has not bought back. | A bot that sells high and buys back lower is likely to buy back, and its buy lifts the price. | 1. It has 12 sells here, the last at vsol 70.<br>2. The coin sits quiet at vsol 60.<br>3. It buys back on 30 % of its coins: we fire. | red · ev 7 (C10) |
| └ **Operators only** | The same for operator structures, which the booked term excludes | The same read, restricted to the single-owner bots the parent leaves out. | One owner's plan is easier to predict than a public tool's crowd. | 1. A 10-wallet operator structure sold at vsol 70.<br>2. The coin is quiet at vsol 60.<br>3. We fire. | red |
| **First print this hour** | The structure's first print on this coin in the current UTC hour | A structure comes back to the coin after a long gap. | A bot coming back to an older position is a new decision. | 1. The structure last printed here 70 min ago.<br>2. It prints now.<br>3. First this hour: we fire. | open · mt3 6.1 |
| **Arrives from another coin** | The structure is buying another coin, then prints here | A bot moves from another coin to this one. | A bot rotating its money into this coin. | 1. It bought coin A 2 s ago.<br>2. It buys here now.<br>3. We fire. | red · ev 7 (C13) |
| └ **Rotating in (family term)** | Among identity-family prints, this structure printed on another coin in the last few seconds | The same, on identity-family prints. | The same money-moving story, on a print that already matters. | 1. A structure buys coin A.<br>2. 2 s later its tool buy lands here.<br>3. The gap is 2 s. | open · mt3 6.1 |
| └ **Wallet rotating in** | Seconds since this wallet's last print on another coin (`w_rot`) | The same, per wallet: how long since it traded another coin. | Money moving from one coin to this one. | 1. The wallet sold coin A 3 s ago.<br>2. It buys here.<br>3. `w_rot` = 3 s. | open · mt3 6.1 |

### E4 A count crosses a line

| Name | Idea | Meaning | Why it matters | Example | Status |
| --- | --- | --- | --- | --- | --- |
| **Second outsider** | The second non-creator buyer, age 5-300 s | The second wallet, other than the creator, to buy. | A second stranger confirms the first. | 1. The creator bought, then one other wallet.<br>2. A second new wallet buys at age 40 s.<br>3. We fire. | red · ev 3.7 |
| **First outsider** | The first non-creator buy after the creator has bought | The first wallet, other than the creator, to buy. | The first stranger to back the creator. | 1. The creator buys.<br>2. The first other wallet buys.<br>3. We fire. | new |
| **Operator count** | Distinct operator structures printing in the last 2 s or 5 s rises through K (`npro2`, `npro5`; read at K = 1) | The number of different bots trading inside a short window reaches K. | Enough bots choose the coin at once. | 1. K = 1, window 2 s.<br>2. No operator structure printed in the last 2 s.<br>3. One prints: we fire. | open · mt1 5.2d |
| **Structure count** | Distinct ix structures in the last 2 s rises through K | The number of different ix structures trading in 2 s reaches K. | The tape suddenly broadens: many kinds of buyers at once. | 1. K = 6.<br>2. The 6th distinct structure prints within 2 s.<br>3. We fire. | open · mt1 5.2d |
| **First operator after snipers** | First non-creator, non-seed operator-structure buy ≥ 0.5 SOL; prior prints are only the creator and seed racers; one fire per coin | The first real bot buy after the launch snipers. | The first real decider after the launch noise. | 1. The creator buys; 5 seed racers buy.<br>2. An operator structure buys 0.6 SOL.<br>3. We fire, once per coin. | red · ev 7 (C15) |
| **Buy flow spike** | A buy whose SOL is a multiple of what the coin has been taking per slot (`flow_spike`, over the 30 slots before it): bigger is better | Fires. **Large among recent** compares the buy with recent print sizes; this compares it with the coin's whole buying rate. | Demand that suddenly accelerates tends to keep going, and this buy is the acceleration. | 1. The coin took 0.1 SOL of buying a slot over the last 30 slots.<br>2. This buy is 0.25 SOL.<br>3. 0.25 / 0.1 = 2.5x, over the 1.82x read: we fire. | open · mt3 6.1 |

### E5 After sellers

| Name | Idea | Meaning | Why it matters | Example | Status |
| --- | --- | --- | --- | --- | --- |
| **Dip buy after a hill** | A buy ≥ 0.5 SOL that is not a seed racer, while price is still down after a hill | Someone who is not a racer buys the pullback of a coin that just rose. | Someone buys the pullback of a proven coin. | 1. The coin makes a +60 % hill.<br>2. It pulls back 20 %.<br>3. A 0.6 SOL Axiom buy lands: we fire. | red · ev 7 (machine print in the dip) |
| **Flush stops** | First buy ≥ 0.5 SOL (not a seed racer) after a flush, once vsol makes no new low for ≥ 10 slots; one fire per flush | After a big drop the price stops falling, then a big buyer comes. | The selling is exhausted and a buyer steps in. | 1. Price is 30 % off the peak.<br>2. 10 slots pass with no new low.<br>3. A 0.7 SOL buy lands: we fire. | red · ev 7 (C9) |
| **Frenzy flip sell** | A public sell ≥ 1 SOL inside a frenzy, a new high in the last 20 s, by a seller who bought ≤ 30 s ago; fire on the sell | A fast flipper sells into a hot, rising tape. | The frenzy's next buyers absorb the drop, so we buy the dip just before they do. | 1. The coin makes a new high.<br>2. A wallet that bought 20 s ago sells 1.5 SOL.<br>3. We buy the dip; the frenzy's buyers lift it back. | keep · ev 1.22 (rule 1's event) |
| └ **Seller all out** | + the sell leaves the seller with 0 tokens | The seller sells his whole bag, not part of it. | A seller with tokens left can sell again while we hold; one who is out cannot. All out is better. | 1. He holds 1 M tokens.<br>2. He sells all 1 M for 1.5 SOL.<br>3. He has 0 left: no more selling from him. | red · case U5 |
| └ **Young-coin flip sell** | 9999hu's sell ≥ 1: the same print class on a younger one-shot, without the established-coin cut | The same big sell on a young coin. | Our entry moves a young coin's price more, but its frenzy keeps running after our fill. | 1. A coin is 15 s old.<br>2. A 1 SOL sell lands.<br>3. We fire. | open · ev 1.27 |
| **Seller at a loss** | `seller_loss`: a public sell from a wallet under water on this coin | A wallet sells for less than it paid. | Someone gives up, and the drop they cause is a discount. | 1. A wallet bought at vsol 70.<br>2. It sells 1 SOL at vsol 60, at a loss.<br>3. We fire on the sell. | open · mt3 5.2 88887Q |
| **Quick flip sell** | `seller_recent`: a public sell from a wallet that bought this coin in the last ~30 s, any tape | A wallet sells a coin it bought seconds ago. | A fast flipper, not a holder, so the drop is short-lived. | 1. A wallet buys at age 40 s.<br>2. It sells at age 55 s.<br>3. We fire on the sell. | open · mt3 5.2 88887Q |
| **Capitulation cascade** | A public sell ≥ 1 SOL from a seller at a loss, inside a burst, with the price down over the last 10 s and a busy tape | Panic: a big losing sell lands inside a run of selling on a busy tape. | Panic selling overshoots, then snaps back. | 1. Price is -10 % over 10 s.<br>2. A wallet 4 % under water sells 1 SOL inside a burst.<br>3. The tape is busy: we fire. | red · ev 1.16 |
| **Big buy after a dip** | A buy of about 1 SOL opening a run, the price down over the last 10 s | Someone buys the dip with size. | Size buying a dip is conviction. | 1. Price is -5 % over 10 s.<br>2. A 1 SOL buy opens a run.<br>3. We fire. | dead · ev 7 (AbQcLH burst start) |
| └ **Mid-tape burst start** | 8dtx2t's burst start: the same class on a mid-tape one-shot | The same print on older coins. | 8dtx2t fires on it, so the rest of the burst could lift the price after our buy. | 1. A coin is 150 s old.<br>2. After silence, a 0.8 SOL buy starts a burst.<br>3. We fire. | open · mt1 5.2e |
| **After a sell run** | `buy_after_sells`: the first buy after 2 or more sells in a row | The first buy after a run of sells. The booked event is stricter: a buy ≥ 0.5 SOL that is not a seed racer, after 3 sells in a row. | Buyers return once the sellers are done. | 1. 2 sells land in a row.<br>2. A buy lands next.<br>3. We fire. | red · ev 7 (C14) |
| **Buy after the crowd left** | The first buy after the last hill's crowd holds under 50 % | A new buyer comes after the last rise's buyers sold most of their tokens. | The weak hands are out; a new buyer starts fresh. | 1. The last hill's buyers sold 60 % of their tokens.<br>2. They hold 40 %, under 50 %.<br>3. A new buy lands: we fire. | new |
| **After a structure sold** | Anyone's first buy after an ix structure sold this coin | The first buy after a bot sold. | The bot's exit is taken as a discount. | 1. An operator structure sells out.<br>2. Someone buys.<br>3. We fire. | open · mt3 6.1 |
| **Top holders sold** | The largest holders have sold | The biggest holders are out. | The biggest dump risk is gone. | 1. The top 3 holders hold 30 % of supply.<br>2. All 3 sell out.<br>3. No one left can dump that much. | new |

### E6 This print

| Name | Idea | Meaning | Why it matters | Example | Status |
| --- | --- | --- | --- | --- | --- |
| **Two structures in a slot** | `two_struct_slot`: exactly two different ix structures print in the same slot | Two different bots or apps trade the coin in the same slot, and no third. | Two independent actors deciding at once. | 1. An Axiom buy lands in a slot.<br>2. A nonce buy lands in the same slot.<br>3. Nothing else prints in it: the print counts. | keep (term) · ev 7 (zigzag turn) · mt3 6.1 |
| **Alone in its slot** | `alone_in_slot`: no other ix structure prints in the same slot | The only structure printing in its slot. Its own repeat prints do not break it. | A lone decision, not a pile-in. | 1. A structure buys three times in one slot.<br>2. No other structure prints in that slot.<br>3. It is alone. | open · mt3 5.2 3Xk2Eu · mt3 6.1 |
| **Big price step** | This print moves the spot price by ≥ X % on its own: `((v[k] / v[k-1])^2 - 1)` (derive 5.1's `up>=`, the toolkit's `mvk`) | One buy that lifts the price a lot by itself. | A buy that moves the price hard is someone paying up. | 1. vsol is 50.<br>2. A 1.1 SOL buy takes it to 51.1.<br>3. (51.1 / 50)^2 - 1 = +4.4 %. | open · mt3 6.1 8aaRWu |
| **Fresh buyer** | The wallet on this print has no earlier print on this coin | The wallet's first trade on this coin. | New money, not a bot topping up. | 1. The wallet has no print on this coin.<br>2. It buys.<br>3. It is a fresh buyer. | open · mt3 6.1 |
| └ **Known hopper wallet** | This wallet is new on this coin but already printed a lot elsewhere (`wal_n_any`) | A wallet new to this coin but busy on other coins. | A rotating hopper, not a brand-new key: it trades for a living. | 1. The wallet has 200 prints on other coins.<br>2. It makes its first print here.<br>3. `wal_n_any` = 200. | open · mt3 6.1 |
| **Wallet back here** | This wallet printed here before: seconds since its last print here, its SOL out minus in here so far (`w_gap`, `w_nethere`) | A wallet returns to a coin it already traded. | A wallet returning to a coin it knows, flat or holding, has a reason to. | 1. A wallet sold here 40 s ago.<br>2. It buys again.<br>3. `w_gap` = 40 s. | open · mt3 6.1 |
| └ **Launch buyer back** | This wallet printed on this coin in its first 10 s (`w_early`) | A wallet that bought at launch comes back. | An insider or sniper coming back is a planned add. | 1. A wallet bought at age 2 s.<br>2. It buys again at age 300 s.<br>3. `w_early` is true. | open · mt3 6.1 |
| **Wallet takes money out** | Share of this wallet's other coins, so far on the tape, where its SOL out exceeds its SOL in (`w_up`) | How often this wallet leaves a coin with more SOL than it put in. | A wallet that usually leaves with more than it put in picks coins well. | 1. The wallet traded 12 other coins.<br>2. It took out more than it put in on 5 of them.<br>3. `w_up` = 5 / 12 = 42 %. | open · mt3 6.1 |
| **Big for this wallet** | This buy over this wallet's mean buy so far on the tape (`w_szrel`) | How big this buy is for this wallet. | A wallet buying above its habit is sure this time. | 1. The wallet's mean buy is 0.2 SOL.<br>2. It buys 1 SOL.<br>3. `w_szrel` = 5x. | open · mt3 6.1 |
| **Copied buyer** | A wallet buys a coin, and on earlier days copy-traders kept buying right after it: at least 3 wallets on another ix structure that each bought within 5 s after it on 5+ of its coins and on ≥ 25 % of its coins, and rarely before it | A wallet that others copy buys. | Copy-traders repeat its buy a moment later, so their buying could push the price up after we buy. | 1. On earlier days, the same 3 wallets bought 1-4 s after this wallet on 5+ of its coins.<br>2. It buys this coin.<br>3. We fire, ahead of the copies. | red · ev 7 (copied buyer) |
| └ **Slow copy-traders** | The same, but its copy-traders buy 1 s or more after it, with enough SOL to lift the price about 4 % | The same, only when its copiers are slow and big. | Only copy-traders slower than our 115 ms can lift the price after our buy. | 1. Its copiers bring 1.5 SOL, 1-4 s after it.<br>2. At vsol 70, 1.5 SOL lifts the price about 4 %.<br>3. It buys: we fire, and their buying lands after ours. | red · ev 7 (copied buyer) |
| **Wallet age on the tape** | Seconds since this wallet's first print on the tape, capped at 6 h (`w_age`) | How long ago this wallet first traded anything. | A fresh key is a new operator or a burner; an old one is a known hand. | 1. The wallet's first print anywhere was 2 min ago.<br>2. `w_age` = 120 s: a fresh key. | open · mt3 6.1 |
| **Block position** | Its tx index in the block (`txi`) | Where in the block the print sits. | A low index means it paid or bundled to land first: urgency. | 1. The block holds 1,200 transactions.<br>2. This print is tx index 3.<br>3. It landed near the front. | open · mt3 6.1 |
| **Answers a sell** | The print before it is a sell (signed SOL, `prev_sgn`), or a sell landed earlier in its slot (`ss_sell`) | A buy that comes right after a sell. | A buy that takes a sell is absorbing it, not chasing. | 1. A 1 SOL sell lands.<br>2. In the same slot, a 0.8 SOL step-up buy lands.<br>3. The buy answers the sell. | open · mt3 6.1 |
| **Same-structure follow** | The print before it is the same ix structure (`prev_same`) | Two prints in a row from the same ix structure. | A bot chaining its own orders is not a new decider. | 1. An Axiom buy lands.<br>2. The next print is another Axiom buy.<br>3. `prev_same` is true. | open · mt3 6.1 |
| **Since the class printed** | Seconds since the last class print on this coin (`clip_gap`) | How long since the last step-up on this coin. | A second step-up soon after the first confirms it. | 1. A step-up lands.<br>2. 1 s later another step-up lands.<br>3. `clip_gap` = 1 s. | open · mt3 6.1 |
| **Class prints so far** | Class prints on this coin before this one (`clip_n`) | How many step-ups the coin had before this one. | More step-ups looked like more operators adding. It counts what the coin has accumulated, which is holders at another grain, so it is a permission and never an event. | 1. The coin had 29 step-ups.<br>2. This is the 30th.<br>3. `clip_n` = 29. | red (as an event) · derive 6.1 |
| **Change since the class printed** | Each fact minus its value at the coin's previous class print: step, buy SOL in 2/3/5/10 s, prints, bursts, move, spike, 25 in all (`d_mvk`, `d_buys2`, ...) | What changed on the coin between the last step-up and this one. | A pick can be a change rather than a level: against the coin's own earlier ignored print, the one he takes has more bursts in the minute and a quieter instant. Read against the coin's previous class print, every one is flat (2.0-3.0 % acted on a 2.30 % base) and the ladder drops them all. Read against the previous row of the 6.1 table it looks strong and is a leak: that row is his own exit on a third of the acted prints. | 1. The last step-up here came with 0.4 SOL bought in 2 s.<br>2. This one comes with 1.0.<br>3. `d_buys2` = +0.6. | red · derive 6.1 |
| **Best of the moment** | Rivals = class prints on other coins in the last 5 / 60 s: how many, and this print's rank among them on step and size (`riv_c60`, `rkmvk_c60`) | Whether this is the loudest step-up on the tape right now. | A trader with one slot takes the best of what is printing, so a print alone in its moment would be taken more often. His rate does not move with the number of rivals (2.18-2.53 % across every bucket) and the rank only repeats the level facts. | 1. Six other coins step up within the minute.<br>2. This print's step is the biggest of them.<br>3. `rkmvk_c60` = 1.0. | red · derive 6.1 |
| **The coin he is allowed to buy** | The coin's own state at the print, laddered on its own: age, reserve, and the crowd at its three grains (`age`, `vres`, `hold_n`/`pubbought`/`clip_n`; rank rho 0.97-0.99 makes the three one family, the reserve reads the room at 0.66 and the age neither at 0.70) | Which coins are open to him, asked apart from what the print does. | Half of a pick can be the coin rather than the event, so the permission is derived on its own facts on the prints the E already passes. Both folds name the same three terms (older than ~60 s, reserve under ~40-45, a thin crowd) and both hold x3.0-3.3 out of sample, but a coin-shuffled ladder on three families pays x2.7-3.1, so neither fold clears its own null at that depth. The label is not concentrated (1,008 acted prints on 880 coins, 1.1 each), so the null is strong because a coin fact reads how many prints a coin has, not because a few coins carry him. | 1. The coin is 90 s old.<br>2. Its reserve is 38 SOL and 24 step-ups have printed.<br>3. All three terms pass: the door is open. | red · derive 9 |
| **High fee** | This print pays at or above the 75th percentile priority/tip fee | A print that pays a high fee to land fast. | Paying to land first means urgency. | 1. Most prints tip 0.0005 SOL.<br>2. This one tips 0.002 SOL.<br>3. That is above the 75th percentile. | keep (term) · ev 7 (zigzag turn) |
| └ **High for its structure** | ≥ 5x its ix structure's median priority/tip fee; from the lake: priority fee + tip over that structure's geometric mean on its ≥ 20 earlier prints (`fee_rel`; `pfee`, `tip` for High fee) | A fee that is high for this bot, compared with what it usually pays. Under 20 earlier prints the term is blank and no cut on it can fire. | Urgent for that bot, not just a bot that always pays high. | 1. The bot has 60 earlier prints, usually tipping 0.0001 SOL.<br>2. It tips 0.0006 SOL.<br>3. 6x ≥ 5x: it is urgent. | open · mt3 6.1 |
| **Size buy** | A public buy ≥ 1 SOL on a mid-life coin | A big public buy. | Big money says conviction. | 1. A coin is 200 s old.<br>2. A public 1.2 SOL buy lands.<br>3. 1.2 ≥ 1: we fire. | red · ev 7 (G1) · mt3 6.1 |
| **Large among recent** | `large_recent`: this print's SOL over the 75th percentile of the coin's recent prints, at or above 0.75x | How big this buy is against the coin's own recent tape, where 1.0 is exactly that 75th percentile. | Big for this coin's own tape, not a fixed size. | 1. The recent 75th percentile is 0.2 SOL.<br>2. This print is 0.16 SOL.<br>3. 0.16 / 0.2 = 0.8x ≥ 0.75x: it counts. | open · mt3 6.1 |

### E7 Clock

| Name | Idea | Meaning | Why it matters | Example | Status |
| --- | --- | --- | --- | --- | --- |
| **Fixed age** | Fire when the coin reaches age T | Fire on time alone, no print needed. | A control: it shows what any fire at that age earns. | 1. T = 120 s.<br>2. Every coin that reaches age 120 s gets a fire. | open |
| └ **Young big sell** | The first public sell ≥ 1 SOL while the coin is still young (age ≤ 16.3 s); the launch first-sell, not 9999hu's fire | The launch's first big exit, used as a clock. | It marks the same moment of launch life on every coin. | 1. A coin is 9 s old.<br>2. Its first public 1 SOL sell lands.<br>3. We fire. | red · mt2 6.2 |
| **First size after T** | The first buy ≥ 0.5 SOL after age 60 s, no silence cut | The first big buy once the launch noise is over. | The first real buyer once the launch noise is over. | 1. The coin is past age 60 s.<br>2. At age 75 s a 0.6 SOL buy lands.<br>3. We fire. | new |

### E8 Graveyard

Do not rebuild these as the event.

| Name | Idea | Meaning | Why it matters | Example | Status |
| --- | --- | --- | --- | --- | --- |
| **Zigzag turn** | The first print off the low after a 15 % bounce | Buy when the price turns up from a low. | Buy the turn before the rise. Dead: a price path cannot tell a turn from a falling knife. | 1. Price falls 30 %.<br>2. It bounces 15 % off the low.<br>3. We fire. | dead |
| **New-buyer acceleration** | K first-time buyers in W slots | Buy when new buyers speed up. | New buyers speeding up looks like a move starting. Dead: they are the move itself, already in the price. | 1. 5 new buyers land in 10 slots.<br>2. We fire. | dead |
| **Seed-racer burst after silence** | A seed-racer burst after the coin is silent | Seed racers wake a quiet coin. | Racers piling in looks like demand. Dead: they only confirm a decision already made. | 1. The coin is silent 10 slots.<br>2. 4 seed racers buy.<br>3. We fire. | dead |
| **Several tools in one slot** | Several tool buys in one slot | Many public-app buys land together. | A crowd arriving at once looks like the start of a move. Dead: it is the wave, and our fill lands behind it. | 1. Axiom, Photon and GMGN buys land in one slot.<br>2. We fire. | dead |
| **Copy a wallet** | Copy a wallet's buy | Buy whenever a chosen wallet buys. | A good wallet's pick looks worth copying. Dead: its impact and the swarm behind it are in the price first. | 1. A named wallet buys.<br>2. We buy the same coin. | dead |
| **Swing pullback** | Price gave back d % of its swing high; bought r % off the low | Buy a measured pullback. | A pullback of a set size looks like a cheap entry into a rising coin. | 1. Price is 40 % off its swing high.<br>2. It is 5 % off the low.<br>3. We fire. | red · ev 7 (hot-tape price-path) |
| **Up-move portrait** | Up m % in 60 s, a recent new high, small giveback, busy tape | Buy a coin that looks strong right now. | Strength tends to continue. | 1. Price is +30 % in 60 s.<br>2. A new high 5 s ago, little given back.<br>3. We fire. | red · ev 7 (hot-tape price-path) |
| **"Now" tells** | Feed top-10, creator re-buy, cadence | Signals of what is happening at this moment. | They show the coin is hot right now. Red: they are already priced. | 1. The creator buys again.<br>2. We fire. | red |

---

## P - Permission

### P1 Curve position

Primary permissions. They do not explain why a move happens; they put the fire where a move can
pay and where a loss is bounded.

| Name | Idea | Meaning | Why it matters | Example | Status |
| --- | --- | --- | --- | --- | --- |
| **Age band** | Age inside a window | Fire only in a set part of a coin's life. | Fire only in the part of a coin's life the rule was read on; outside it the rule is untested. | 1. The band is age 60-600 s.<br>2. A fire at age 200 s is allowed.<br>3. A fire at age 30 s is not. | keep |
| └ **Established coin** | Age ≥ 158 s and ≥ 368 distinct non-creator buyers | An older coin with a broad crowd. | A frenzy on a young, thin coin dies; on an old, broad one it is absorbed. | 1. The coin is 200 s old.<br>2. 400 wallets other than the creator have bought.<br>3. 200 ≥ 158 and 400 ≥ 368: allowed. | keep · ev 1.22 |
| └ **Established on 9999hu** | The same cut on 9999hu's sell ≥ 1 fire | Rule 1's cut, tried on 9999hu's event. | If older, broader coins absorb sells better, the same cut can help another sell event. | 1. 9999hu's fire lands at age 15 s.<br>2. The coin is not established.<br>3. The fire is skipped. | red · mt2 9.1 |
| **Room under the wall** | vsol after the print we fire on ≤ 100; above 105 a +20 % target no longer fits under the wall and the trade closes on the completing buy, at a price the curve no longer offers; a safety term, not a fit | The coin must sit at vsol 100 or less when the trigger print lands. | Graduation at 115 caps the upside and breaks the exit, so a fire near the wall cannot pay. | 1. After the trigger print vsol is 95: allowed; from there the price can still rise (115 / 95)^2 - 1 = +47 %.<br>2. After it vsol is 105: not allowed, because 105 x sqrt(1.2) is over the wall. | keep · ev 1.20 |
| └ **Room on 9999hu** | vres ≤ 100 on 9999hu's sell ≥ 1 | The same cut on 9999hu's fires, which all already sit under vsol 100. | The same safety cut, on another event. | 1. 9999hu's fire lands at vsol 80.<br>2. 80 ≤ 100: allowed. | red · mt2 9.1 |
| **Sell-reactive buyers** | Distinct wallets on this coin that bought within 300 ms of a public sell ≥ 1 SOL | How many wallets on this coin buy a big sell back within 300 ms. | Dip-buying bots on this coin catch the next dip, and their buying lifts the price after ours. | 1. 5 wallets here bought within 300 ms of a sell ≥ 1 SOL.<br>2. A new big sell lands.<br>3. They are likely to buy it back. | red · case 41 |
| **Vsol band** | vsol inside a window | Fire only in a set part of the curve. | Where on the curve the fire sits sets both the room to rise and how far it can fall. | 1. The band is vsol 45-80.<br>2. A fire at vsol 60 is allowed.<br>3. A fire at vsol 90 is not. | keep |
| **Headroom** | vsol ≤ 81 for a +100 % target | The +100 % target still fits under the wall. | A target above the wall cannot be reached. | 1. At vsol 81, (115 / 81)^2 = 2.0: +100 % still fits, allowed.<br>2. At vsol 90, (115 / 90)^2 = 1.63: only +63 % fits, not allowed. | keep |
| **Pool alive** | Real reserve > 0 and the coin still prints | The coin still has SOL in it and still trades. | A dead pool has no buyers to sell to. | 1. The last print is 3 s old.<br>2. The reserve is 12 SOL.<br>3. Alive: allowed. | new |

### P2 Windowed tape metrics

Each metric has its own meaning and concentrates the pool: it buys "this coin is not decaying",
worth about the toll. None is a cause or an event, because windowed flow is the price path. The
window is seconds, slots or prints, with a lag so it cannot read the event. Definitions:
[_!___metrics.md](_!___metrics.md).

| Name | Idea | Meaning | Why it matters | Example | Status |
| --- | --- | --- | --- | --- | --- |
| **Buying outruns selling** | More SOL is bought than sold in the window before the print: more is better | Filters. Spelled from the windowed flow metrics: net SOL, or the buy share of gross. | A coin where buying leads is not decaying, so our fill is not the last one in. | 1. In the last 10 s, 5 SOL is bought and 3 SOL sold.<br>2. Net is +2 SOL, buy share 63 %.<br>3. Both lead: the fire is allowed. | keep |
| **Calm tape** | Few prints on the coin in the seconds before the print: fewer is better | Filters. **Buying outruns selling** reads the direction of the money; this reads how busy the tape is, whichever way it goes. | A busy tape is a pile-in that has already moved the price, so our fill lands behind it. | 1. The cap is 25 prints in 5 s.<br>2. 12 prints land in the last 5 s.<br>3. 12 ≤ 25: the fire is allowed. | red · ev 7 (rule 3b) |
| **Price inside its recent range** | Where the price sits between the window's low and its high: a middle reading is better than either end | Filters. Spelled as the drop below the window high, or the rise above the window low. | At the high the move is already spent and we buy the top; far below it the coin is falling and we catch a knife. | 1. The 30 s high is 1.00 and the low 0.80.<br>2. The price is 0.92, 8 % under the high.<br>3. It sits in the middle: the fire is allowed. | keep |
| **Long past its best** | The price is far under the coin's all-time high, and has been for a long time: both smaller is better | Filters. **Price inside its recent range** reads a window of seconds; this reads the coin's whole life. | A coin that has been far under its best moment for minutes has lost the buyers who made that moment. | 1. The all-time high is 1.00, set 90 s ago.<br>2. The price is 0.60: 40 % under it.<br>3. 40 % under for 90 s is a coin past its best: the fire is skipped. | keep |
| **New buyers still arrive** | Wallets buying this coin for the first time keep appearing in the window before the print: more is better | Filters. Counts wallets, not SOL, so one whale cannot fake it. | The next rise is paid for by people who are not in yet; if they stopped coming, nobody is left to lift it. | 1. In the last 30 s, 12 wallets buy the coin for the first time.<br>2. The cut is 10.<br>3. 12 ≥ 10: the fire is allowed. | keep |
| **No new low** | vsol has made no new low since the event | The low at the event still holds. | The low holding means the sellers are not in control. | 1. At the event the low is vsol 52.<br>2. vsol has not been under 52 since.<br>3. Allowed. | new |

### P3 Skin in

| Name | Idea | Meaning | Why it matters | Example | Status |
| --- | --- | --- | --- | --- | --- |
| **Creator holds** | The creator has not sold. A survival term: it also raises the loss rate | The creator still owns his tokens. | A creator still in has not rugged yet. | 1. The creator bought 1 SOL at launch.<br>2. He has sold nothing.<br>3. Allowed. | keep · ev 3.4 |
| **Crowd left** | The last hill's crowd holds under 50 % | The last rise's buyers sold most of what they bought. | Their selling is spent, so it does not hit our rise. | 1. The last hill's buyers bought 10 M tokens.<br>2. They hold 4 M (40 %).<br>3. 40 % < 50 %: allowed. | keep |
| └ **Crowd still in** | The crowd still holds ≥ 80 % | The last rise's buyers still hold almost all of it. | The last rise's buyers still believe in the coin and have not cashed out. | 1. The last hill's buyers bought 10 M tokens.<br>2. They hold 9 M (90 %).<br>3. 90 % ≥ 80 %: allowed. | red |
| **Pusher holds** | The last hill's pusher has not sold | The biggest buyer of the last rise is still in. | The one who pushed the price still wants it higher. | 1. The pusher bought 3 SOL in the last hill.<br>2. It has sold nothing.<br>3. Allowed. | red |
| **Firing structure holds** | The ix structure we fire on has not sold | The one we follow is still in. | The one we follow still wants the price higher. | 1. We fire on an operator structure's buy.<br>2. It has no sell on this coin.<br>3. Allowed. | new |
| **Creator rebought** | The creator bought in the last N slots | The creator just added to his bag. | A creator adding is betting on his own coin. | 1. N = 10 slots.<br>2. The creator bought 0.3 SOL 5 slots ago.<br>3. Allowed. | new |
| **Near-even holders** | Holders whose loss is -20 % ~ 0 % now: their % of supply | Holders who paid up to 20 % above today's price. | A +20 % rise brings them back to what they paid, and many sell there to get out even. That selling lands inside our rise. Fewer is better. | 1. The price is 1.00.<br>2. A holder paid 1.10: loss -9 %, inside -20 % ~ 0 %.<br>3. At a +20 % rise the price passes 1.10, and he sells. | red · case U5 |
| └ **Get-out-even wall** | The SOL that holders at -20 % ~ 0 % get if they sell ÷ the SOL buyers must spend to lift the price +20 % | Their selling compared with the buying our target needs. | Above 1, their selling can cancel the whole rise. Lower is better. | 1. At vsol 50, a +20 % rise needs about 4.8 SOL of buying.<br>2. Holders at -20 % ~ 0 % hold 6 SOL of tokens.<br>3. 6 ÷ 4.8 = 1.25: their selling beats the buying, the rise stalls (bad).<br>4. With 1 SOL of tokens: 1 ÷ 4.8 = 0.2, too small to matter (good). | red · case U5 |
| └ **All losers** | Holders whose loss is below 0 % now, any depth: their % of supply | Every holder who is losing, however deep. | Comparison only: deep losers cannot get back to even inside +20 %. If this reads the same as Near-even holders, the -20 % ~ 0 % band adds nothing. | 1. The price is 1.00.<br>2. A holder paid 2.00: loss -50 %.<br>3. He counts here, but not in Near-even holders. | red · case U5 |
| **Big winners** | Holders whose profit is +50 % or more now: their % of supply | Holders sitting on a big gain. | They take profit into any rise, so their tokens meet our buy. Fewer is better. | 1. The price is 1.00.<br>2. A holder paid 0.60: profit +67 %.<br>3. As the price rises he sells into it. | new |
| **Holders in profit** | The share of held tokens whose profit is +20 % or more at the fire | How much of the supply is already up +20 % or more. | Read on a frenzy it runs the other way: the frenzy that goes on to stop us out has fewer holders in profit, not more. It is a maturity fact, like coin age and the reserve, so more of it is better. | 1. Two frenzies fire at the same age.<br>2. On the one that reaches the take profit, more of the held tokens are up +20 %.<br>3. On the one that stops out, fewer are: the fact leans the way maturity does. | red · ev 1.14 |

### P4 ix makeup of the recent tape

The permission side of E1.

| Name | Idea | Meaning | Why it matters | Example | Status |
| --- | --- | --- | --- | --- | --- |
| **Not a seed racer** | This print is not a seed racer | Skip prints from seed racers. | Seed racers react to someone else's buy; they are never the decision. | 1. A `CreateAccountWithSeed` buy lands.<br>2. It is a seed racer: no fire. | keep |
| **No racer lately** | No seed racer in the last N slots | No seed racer has bought recently. | The racers have not piled in yet, so the move is still ahead. | 1. N = 10 slots.<br>2. No `CreateAccountWithSeed` print in the last 10 slots.<br>3. Allowed. | new |
| **Tools only** | All buy SOL in the run (or the last N slots) comes from tool structures, none from seed racers | The recent buying is all from public apps. | Retail buying, not bots racing: retail keeps coming, racers dump. | 1. 1.2 SOL is bought in the last 5 slots.<br>2. All of it through Axiom and Photon.<br>3. Allowed. | open |
| **Buy SOL band** | The run's buy SOL, or the tools' buy SOL over this slot and the one before, sits above noise and below "already priced" | Recent buying is big enough to mean something, but not so big that the move is gone. | Too little is noise; too much has already moved the price before our fill. | 1. The band is 0.5-2 SOL.<br>2. Tools bought 1.2 SOL over this slot and the one before.<br>3. Inside the band: allowed. | open |

### P5 Tape state already true

| Name | Idea | Meaning | Why it matters | Example | Status |
| --- | --- | --- | --- | --- | --- |
| **Many structures** | The count of distinct ix structures printing in the last 5 s, with no single one dominating | Many different actors trade the coin now. | Many independent actors, not one bot, is real demand. | 1. 8 structures printed in the last 5 s.<br>2. The largest has 30 % of the prints.<br>3. Allowed. | open · ev 1.11 |
| **Coin already silent** | The coin is silent when the print lands | The print breaks a quiet tape. | A print on a quiet tape is a fresh decision, not part of a wave. | 1. No print for 12 slots.<br>2. The print lands.<br>3. Allowed. | new |
| **No sells just before** | No sell right before this print (`sell_run` ≤ 0) | The prints right before are not sells. | The buy is not catching a falling price. | 1. The last 3 prints are buys.<br>2. `sell_run` = 0: allowed. | open · mt3 6.1 8aaRWu |
| **No big sells lately** | ≤ N public sells ≥ 1 SOL in the last 30 s | Few big sells in the last 30 s. | No large holder is on the way out. | 1. The cap is 4.<br>2. 2 sells over 1 SOL in the last 30 s.<br>3. 2 ≤ 4: allowed. | red · ev 7 (rule 3b) |
| **Long-hold buyers** | Buyers in the last 10 s who usually hold a coin 5 min or more: their % of that buy SOL | Each buyer's habit, read from their earlier coins on the tape. | Long holders' money stays in the coin, while fast flippers sell back into us within seconds. More is better. | 1. In the last 10 s, 2 SOL is bought.<br>2. 1.4 SOL of it is from wallets that usually hold 5+ min.<br>3. 1.4 ÷ 2 = 70 %: most of the new money stays. | new |
| **Slow re-entry state** | Quiet ≥ 60 s, ≥ 2 hills, a returning structure, pullback 40-70 %, vsol 55-80 | A rested, proven coin being picked up again. | A coin that rose twice and rested can rise again when a known buyer returns. | 1. Two hills, then a 50 % pullback.<br>2. 60 s quiet at vsol 65.<br>3. A returning structure buys: allowed. | red · ev 7 (zigzag turn) |

---

## X - Exit

### X1 Static

| Name | Idea | Meaning | Why it matters | Example | Status |
| --- | --- | --- | --- | --- | --- |
| **Clock** | Close at T, 15-600 s | Leave after a fixed time. | A control: it shows what holding alone earns. | 1. T = 45 s.<br>2. We sell 45 s after the fill, whatever the price. | keep (control) |
| └ **Clock on 9999hu** | Clock 25 on 9999hu's sell ≥ 1: copies its 20-30 s close | Copy 9999hu's own hold time. | Its 20-30 s close can be where its move ends. | 1. We fill on 9999hu's event.<br>2. We sell 25 s later. | red · mt2 8.2 |
| **Take profit** | Close at +25 / +40 / +100 % | Leave at a fixed gain. | A control: it shows how often each gain is reached. | 1. The target is +40 %.<br>2. The price reaches +40 %: we sell. | keep (control) |
| **Unarmed trail** | 20-40 % off the peak, counted from entry, cap 600-1800 s | Sell once the price falls a set % from its best since our buy. | It rides the move and leaves when the move gives back. | 1. The trail is 30 %.<br>2. The peak since our buy is +30 %.<br>3. The price falls 30 % from that peak: we sell. | keep |
| **Armed trail + stop** | Trail only after +arm, a hard stop until then; shipped as arm +21 %, trail 36 %, stop -43.75 %, cap 1200 s | The trail starts only after a set gain; until then a hard stop protects us. | A winner runs only once it is a winner; a loser is capped until then. | 1. Before +21 %, a fall to -43.75 % stops out.<br>2. At +21 % the trail arms.<br>3. After that, a fall of 36 % from the peak sells. | keep · ev 4.7 |
| └ **No stop** | A position that never arms has no stop | The trail without the safety net. | A position that dips deep and comes back is not stopped out. | 1. The position never reaches +21 %.<br>2. It holds to the 1200 s cap, whatever the loss. | red |
| **Tail keeper** | tp100 / trail50 / cap1200 | Wide settings that keep the rare big winner. | It fits a trough entry, where a few trades pay most of the money. | 1. Target +100 %, trail 50 %, cap 1200 s.<br>2. A trade runs to +100 %: we sell there. | keep |
| **Bracket** | Take profit +20 %, stop -60 %, clock 90 s (rule 1 at the engine's grain) | A small target, a wide stop and a short clock: whichever hits first. | A small sure target with a wide stop suits an event that bounces fast. | 1. +20 %: sell.<br>2. Or -60 %: sell.<br>3. Or 90 s pass: sell. | keep · ev 1.22 |
| └ **Bracket on 9999hu** | tp15 sl40 t70 on 9999hu's sell ≥ 1 | The same kind of bracket, on 9999hu's event. | The working bracket, read on the pool 9999hu buys. | 1. +15 %: sell.<br>2. Or -40 %: sell.<br>3. Or 70 s pass: sell. | red · mt2 8.2 |
| └ **Longer clock** | The same bracket held 240 s | More time to reach the target. | Some trades reach +20 % only after 90 s. | 1. +20 % or -60 %: sell.<br>2. Or 240 s pass: sell. | red · ev 1.20 |
| └ **Breakeven** | Once up +3 / +5 / +7 %, close back at the fill | Once a small gain shows, never let it turn into a loss. | It protects a small gain. | 1. The trade is up +5 %.<br>2. The price falls back to our fill.<br>3. We sell at 0 %. | red |
| └ **Half out** | Sell half at the target, ride the rest | Bank half, keep a lottery ticket. | It banks the sure part and still catches a big run. | 1. At +20 % we sell half.<br>2. The other half trails. | red · ev 1.24 |
| └ **Trail after target** | Once up +10 %, trail 5-10 % off the peak | Replace the fixed target with a tight trail. | A tight trail keeps more of a run than a fixed target. | 1. The trade reaches +10 %.<br>2. It peaks at +18 %.<br>3. It falls 7 % from that peak: we sell. | red · ev 1.24 |
| └ **Stepped trail** | The higher the peak, the looser the trail: peak +10 % -> 5 %, +30 % -> 10 %, ... | A bigger winner gets more room. | Big winners swing more; a loose trail keeps them. | 1. The peak reaches +30 %.<br>2. The trail widens to 10 %.<br>3. A fall of 10 % from the peak sells. | red · ev 1.24 |
| **Headroom target** | Take profit at a share of the room to the wall: f x ((115 / vsol)^2 - 1); rule 1b is 0.4 x, `m_position.room_taken >= 40` | The target scales with how far the wall is. | Far from the wall a coin can rise more, so the target can be bigger. | 1. At vsol 70 the room is (115 / 70)^2 - 1 = +170 %.<br>2. 0.4 x 170 % = +68 %.<br>3. The target is +68 %. | open · ev 1.25 |
| **Payoff-shape exit** | Pick the family from the group's own payoffs: a positive median wants a small target, a tail wants no target | Match the exit to how the event pays. | An event that pays small and often wants a small target; one that pays rarely and big wants a trail. | 1. Most trades on an event end near +5 %.<br>2. Use a small target. | open |
| **Static abort** | Not up X % by T -> sell | Cut the trades that do not move. | A trade that does not move early is likely a loser. | 1. X = +5 %, T = 30 s.<br>2. At 30 s the trade is +2 %.<br>3. We sell. | dead |

### X2 The tape stops (a state cut)

A cut that can fire while the position is up is a clock. Every cut here fires only below the
fill.

| Name | Idea | Meaning | Why it matters | Example | Status |
| --- | --- | --- | --- | --- | --- |
| **Buyers stop** | New buyers stop arriving | No new buyer comes for a while. | No one new is coming to lift the price. | 1. We are under the fill.<br>2. No new buyer in 20 s.<br>3. We sell. | red · ev 1.24 on rule 1 (open · ev 4.7 elsewhere) |
| **Flush resumes** | vsol makes a new low after an after-flush entry | The drop we bought continues. | The bounce failed; the selling is not over. | 1. We buy after a flush whose low is vsol 50.<br>2. vsol falls to 49, a new low.<br>3. We sell. | keep |
| **Pre-event low** | vsol revisits the low before the event | The price goes back to where it was before the trigger. | The event is undone. | 1. Before the event the low is vsol 55.<br>2. After our fill vsol returns to 55.<br>3. We sell. | red · ev 7 (C14) |
| **Firing structure silent** | The ix structure we fire on stops printing | The one we followed has stopped. | Its buying was the reason we bought. | 1. We are under the fill.<br>2. No print from that structure for 20 slots.<br>3. We sell. | red |
| **Operators stop** | No new operator structure arrives | No new bot comes to the coin. | Bots have lost interest. | 1. We are under the fill.<br>2. No new operator structure in 30 s.<br>3. We sell. | red · ev 7 (C15) |
| **Fees fall** | Later prints stop paying to land | Buyers stop paying extra to land fast. | The urgency is gone. | 1. We are under the fill.<br>2. Priority fees drop back to the coin's median.<br>3. We sell. | new |
| **Burst give-back** | Trail against this ix structure's own peak, not the coin's | Leave when the burst we joined fades. | The burst we joined was the reason; its fade is the end of it. | 1. The burst's last buy lands at price 1.00.<br>2. The price falls to 0.80, 20 % under it.<br>3. We sell. | new |

### X3 An actor sells

| Name | Idea | Meaning | Why it matters | Example | Status |
| --- | --- | --- | --- | --- | --- |
| **Firing structure sells** | The ix structure we fire on sells | The one we followed is leaving. | Its buying was the reason we bought; its sell ends it. | 1. We fire on a structure's buy.<br>2. It prints a sell.<br>3. We sell. | keep · ev 7 (C11) |
| **Pusher sells** | The last hill's pusher sells | The biggest buyer of the last rise is leaving. | The engine of the move is leaving. | 1. The last hill's biggest buyer sells.<br>2. We sell. | open |
| **Creator sells** | The creator prints a sell | The insider exits. | A creator selling can be the start of a rug. | 1. The creator sells any amount.<br>2. We sell. | new |
| **Ride the creator** | Hold while the creator holds, trail after the creator sells | Stay as long as the insider stays. | The creator knows the coin's plan; his exit is the signal. | 1. We hold while the creator holds.<br>2. The creator sells.<br>3. From then we trail 20 %. | new |
| **Firing structure leaves** | The ix structure we fire on prints on another coin | Its attention has moved on. | It will not buy here any more. | 1. Our trigger structure buys a different coin.<br>2. We sell. | red · ev 7 (C13) |
| **Sell to the copy-traders** | While we hold, a wallet with slow copy-traders buys: sell in the next 1-4 s | Sell into the buying its copiers bring. | Their buying arrives on a timetable, so it is a good moment to sell to them. | 1. We are up 8 %.<br>2. A wallet with slow copy-traders buys.<br>3. We sell in the next 1-4 s, into their buying. | new |
| **Sell into a buy** | Once up ≥ 10 %, sell on the first public buy ≥ 1 SOL or +3 % print | Exit into someone else's big buy. | A big buy gives us a high price and someone to take our tokens. | 1. We are up +12 %.<br>2. A 1.2 SOL buy lands.<br>3. We sell. | red · ev 1.24 |

---

## R - Re-entry

| Name | Idea | Meaning | Why it matters | Example | Status |
| --- | --- | --- | --- | --- | --- |
| **One open per coin** | One open position per coin, no cap on tickets | The standing rule. | A coin can be traded many times but never twice at once, so one coin cannot double a loss. | 1. A coin is traded 5 times in a day.<br>2. Never two positions on it at once. | keep |
| **Below our last exit** | Re-enter only under our last exit on this coin, e.g. 30 % under at vsol < 42.43 | Buy back only cheaper than we sold. | Buying back cheaper keeps the same upside at a better price. | 1. We sold at vsol 60.<br>2. A new fire at vsol 55 is allowed.<br>3. A new fire at vsol 62 is not. | open · ev 1.11 |
| **Round trips taken** | Earlier round trips on this coin as a gradient | How many times we already traded this coin. | The more we traded a coin, the less is left. | 1. We made 3 round trips on this coin.<br>2. The next fire is skipped. | open · ev 1.11 |
| **Cool-down after a stop** | No re-entry on the coin for N s after a stop-out | After a stop, wait before buying this coin again. | Do not buy the same falling knife twice. | 1. N = 60 s.<br>2. We stop out.<br>3. No re-entry on the coin for the next 60 s. | open · ev 1.20 |
| **Entry cap** | At most N entries on one coin | A limit on entries per coin. | It limits exposure to one coin. | 1. N = 3.<br>2. The 4th fire on the coin is skipped. | open · ev 1.20 |

## S - Size

| Name | Idea | Meaning | Why it matters | Example | Status |
| --- | --- | --- | --- | --- | --- |
| **0.2 SOL** | The standing clip | The default size. | Small enough that our own impact stays low. | 1. Every ticket buys 0.2 SOL. | keep |
| **Cost minimum** | sqrt(F x vsol), about 0.126 SOL at vsol 70 | The clip where fixed cost and impact balance. | Below it the fixed cost eats the trade; above it our own impact does. | 1. At vsol 70 the formula gives 0.126 SOL.<br>2. Every ticket at vsol 70 buys 0.126 SOL. | open |
| **Fraction of vsol** | The clip scales with the pool | The clip grows with the pool. | A deeper pool takes a bigger clip for the same impact. | 1. The clip is 0.3 % of vsol.<br>2. At vsol 60 it buys 0.18 SOL.<br>3. At vsol 100 it buys 0.3 SOL. | open · ev 1.20 |
| └ **0.35 SOL for rule 1** | The largest flat clip that keeps every bar on both tapes | Rule 1's clip, sized to its own bars. | A bigger clip earns more per trade as long as every bar still holds. | 1. Every rule 1 ticket buys 0.35 SOL. | keep · ev 1.20 |
