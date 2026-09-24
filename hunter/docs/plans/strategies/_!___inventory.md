# Inventory: the ideas

Every idea, as a tree: **slot → family → idea**, with the idea's parameters listed once beside it.
[_!___strategy.md](_!___strategy.md) is the basis;
[_!___derive.md](_!___derive.md) is the method, the gates and how a result is recorded;
[_!___workflow.md](_!___workflow.md) is the open queue;
[_!___evidence.md](_!___evidence.md) is the numbers;
[_!___terms.md](_!___terms.md) is every word;
[_!___metrics.md](_!___metrics.md) is what the engine measures. This file is the idea list used
**on a parent** (derive phase 10). It does not invent the parent.

## What a row is

A row is **one pure idea**: one claim about a coin, a print or a position that could fill a slot,
stated with no setting in it. The settings an idea can take - which class, which window, which
denominator, which gate - are its **parameters**, listed in the row's Parameters column. A study
that tries an idea at one setting is trying the row, not a new idea; the setting and its number go
in the case file and the evidence ledger, and the row's Status summarises every book that read it.

- **The verb picks the slot, the mechanism picks the family.** A row that *screens coins* on facts
  fixed at birth or in the first seconds is D. A row that *fires* on a print is E. A row that
  *filters* on a state standing true when the print lands is P, and it names its time basis (at
  the print, a window of seconds or slots, the coin's whole life). A row that *sells* is X.
  A row about our own tickets on the coin is R; the clip is S.
- **One axis, one row per slot.** The same fact can serve two slots (silence is an E when broken,
  a P when standing, an X when it returns). Each slot's row says only what that slot does with it,
  and the axis index at the end shows them side by side.
- **A conjunction is not a row.** A booked sentence's terms are each a row's parameter; the
  sentence itself lives in the evidence ledger (section 7). A re-read of a row on another pool is
  a Status reference, never a second row.
- **A measurement is not a row.** A metric is how a row is spelled; its definition lives in
  [_!___metrics.md](_!___metrics.md). A number belongs in [_!___evidence.md](_!___evidence.md), a
  method line in [_!___derive.md](_!___derive.md), a study's finding in its case file.
- **Every row is usable at our seat**: on-chain tape and the metadata document captured at birth,
  read at the fill without waiting for the slot to close, with no wallet identity and no per-wallet
  history across the whole tape. What fails that bar is listed under **Dropped** with its reason,
  so it is not re-invented.
- Columns: **Name** (1-4 words, unique here) · **Idea** (what it reads and which way is good; a
  number only when it is fixed by the curve, the machine, an engine class, standing policy, or a
  **(booked: X)** term of a shipped rule) · **Meaning** (opens with the verb: screens, fires,
  filters, sells, sizes; names the nearest row and the one-clause difference) · **Why it
  matters** (the mechanism that moves the price, and which way is good for us) · **Example**
  (2-3 numbered steps at one illustrative setting, ending in what happens: we fire, the coin
  passes, the fire is refused, we sell; never a study's result, which lives in the case file) ·
  **Parameters** (the settings a study may vary, each a phrase) · **Status**.
- **Status** is one word: **keep** (a booked sentence uses it) · **open** (read, not yet on a
  whole sentence at our seat) · **red** (red on every sentence run so far; not closed) ·
  **dead** (closed by a mechanism) · **new** (never read). A **keep** may carry one word in
  brackets: **(screen)** a door refreshed daily, **(exclude)** a cut that only removes,
  **(control)** a yardstick, **(term)** a piece other rows are built from. Then the books that
  read it, each in short form: `ev N` (evidence section), `ev 7 (C9)` (a ledger row), `case N`
  (hot-tape-rule-1 step), `mt1 N` / `mt2 N` / `mt3 N` (mid-tape rule files), `mt3 d8` (the
  8dtx2t push node in mid-tape-rule-3 section 2b), `omego N` (hot-tape-omego), `6ix`
  (launch-group-6ix), `derive N`. A **new** row, a **(control)** and standing policy carry none.
- **A word an idea needs is registered** in [_!___terms.md](_!___terms.md) in the same edit.
- **Old family codes.** Case files written before this layout mark coverage by the earlier
  family codes; the map at the end translates them.

```
  D  Door        which coins we watch: fixed at birth or in the first seconds
  E  Event       the print we fire on
  P  Permission  state already true when that print lands
  X  Exit        how we leave
  R  Re-entry    tickets per coin (standing: unlimited, one position open)
  S  Size        the clip (standing: 0.2 SOL)
```

---

## Map

```
D  DOOR
   D1  launch build          create fingerprint · group history · dump-factory exclusion · creator record
   D2  birth money           creator's opening buy · first-slot money
   D3  birth noise           quiet birth · early operators · life at a fixed age
   D4  document              documented · URI host · reused content
E  EVENT
   E1  who printed           a class of ix structure buys
   E2  this structure here   first here · back after a gap · adding a leg · clip step-up · buying back under its sell
   E3  the tape just before  breaks the coin's silence · after sellers · a sell we buy into · count crossing
   E4  how loud the trigger  trigger size · fee paid · slot company
   E5  the seat              fixed age · first size after T · fill delay
   E6  graveyard
P  PERMISSION
   P1  where the coin is     age · curve position · pool alive
   P2  what the tape did     net flow · tape busyness · big sells lately · a rise exists · under its peak · low holding
   P3  who holds the supply  concentration · bundled share · public-app share · creator's position · actor still holds · crowd left · holders' PnL · operators round-tripped
   P4  who has acted here    classes present · distinct wallets · record of those present · racers around
   P5  the trigger's record  structure record on earlier days
X  EXIT
   X1  static                clock · take profit · stop · trail · bracket · gate then ride · abort
   X2  the tape stops        silence of X · flow turns · fees fall · low breaks
   X3  an actor acts         actor sells · actor leaves · sell into a buyer
R  RE-ENTRY
S  SIZE
```

---

## D - Door

Facts fixed at birth or inside the coin's first seconds. A door is optional (derive 7): empty is a
value, and a door earns its place only when it raises money and keeps the bars.

### D1 Launch build

The create transaction is an ix structure like any other, and the history of the coins it made is
the door's evidence. Refreshed daily; a group stays in the door one to two days.

| Name | Idea | Meaning | Why it matters | Example | Parameters | Status |
| --- | --- | --- | --- | --- | --- | --- |
| **Create fingerprint** | Group coins by the ix structure of the transaction that created them | Screens, by grouping. Coins born from one create structure are judged together. The unit is the instruction list; CU limit and CU price are knobs one launcher turns without changing its software, so they never make a group. | One launch tool or one operator sends the same create transaction every time, so its coins tend to behave alike: a good group's next coin is likely good too. A coarse key mixes unrelated launchers and describes none of them. | 1. 300 coins are created by one identical 5-instruction create transaction.<br>2. Two of them set different CU prices; they stay in the same group.<br>3. The group's history decides whether its next coin enters the door. | exact list vs coarse bucket (count + last instruction); max-cost and tip as a split inside a list | keep · ev 3.1 · 6ix · ev 7 (mid-tape instruments) |
| **Group history test** | Yesterday the group launched enough coins and enough of them made the move we trade | Screens. It keeps the launchers whose coins repeat a move type; the move counted is a parameter, so one test serves a slow-wall rule and a hill rule. **Dump-factory exclusion** is the same test run to remove. | A launcher whose coins made the move yesterday tends to repeat it today, and a slow climb leaves time for our fill to ride it. A launch group decays week to week, so a short window predicts better than every earlier day. | 1. Yesterday the group launched 40 coins.<br>2. 3 of them (7.5 %) reached vsol 60 after age 60 s.<br>3. 7.5 % clears a 5 % cut: today's coins from this group are in the door. | move type (slow wall after the first minute, instant wall, hill, BIG rate at our seat), minimum coins, rate, window (one day, three, rolling) | keep (screen) · ev 3.1 · mt3 d8 |
| **Dump-factory exclusion** | Remove groups whose coins nearly always spike once and fall back under where they started | Screens, by exclusion. **Group history test** keeps; this removes whole groups whatever their wall rate says. | A launcher that rugs by design does it again on the next coin, so removing the group removes losses we can see coming. | 1. A group made 30 coins yesterday.<br>2. All 30 spiked, then fell under their start inside a minute.<br>3. The group is out of the door, whatever its wall rate says. | spike and fall-back definition, rate | keep (exclude) · ev 3.1 |
| **Creator record** | Inside one launch build, keep the coins of creators whose own earlier launches survived or moved often enough | Screens. **Group history test** judges the software; this judges the person sending it, inside one build. A creator address costs nothing to replace, so rotation is not recoverable from chain data and the door sees only creators who stand still. | One build carries thousands of unrelated creators whose coins do not behave alike, so the build's history describes none of them; the creator's own record separates them and keeps predicting forward. It selects coins, not an entry. | 1. Inside one launch build, a creator launched 8 coins in the last 7 days.<br>2. 5 of them reached 20 trades.<br>3. Today's coin from that creator is in the door; a creator with 1 of 8 is not. | window of days, minimum prior coins, survival or move definition | keep (screen) · ev 3.8, 3.9 |

### D2 Birth money

| Name | Idea | Meaning | Why it matters | Example | Parameters | Status |
| --- | --- | --- | --- | --- | --- | --- |
| **Creator's opening buy** | How much SOL the creator buys inside the create transaction, zero included | Screens, as a split of the launch group or a cut on its own. **First-slot money** reads everyone's buying in that slot; this reads the creator's alone. | What the creator puts in shows how serious the launch is. A creator with no stake loses nothing when the coin dies; frozen as a door alone that reads red, because the one green week was one launch machine. | 1. The create transaction also buys 0.8 SOL for the creator.<br>2. That falls in the 0.5-1 SOL band.<br>3. The coin is judged with the group's other 0.5-1 SOL coins; a create with no buy is its own band. | band; zero as its own band | open · ev 7 (no-initial-buy door) |
| **First-slot money** | How much SOL, and how many wallets, bought in the coin's creation slot | Screens. **Creator's opening buy** is the creator's part of it. **Quiet birth** counts prints across the first slots, not SOL in the first. | Money in the first slot shows the launcher backs this coin instead of spraying it, and one launcher's quiet and bundled launches should not share one history. | 1. In the creation slot, 4 wallets buy 2.5 SOL in total.<br>2. The floor is 2 SOL.<br>3. 2.5 clears it: the coin passes. | SOL band, wallet count | keep (screen) · ev 3.2 |

### D3 Birth noise

Each fact carries the age it becomes known: a fact known at age 60 s cannot serve a fire at age 20 s.

| Name | Idea | Meaning | Why it matters | Example | Parameters | Status |
| --- | --- | --- | --- | --- | --- | --- |
| **Quiet birth** | How many prints land in the coin's first slots: fewer is better | Screens. **Early operators** asks who came early; this asks how loud the birth was. | A sniper swarm at birth is a bag that dumps on the first rise. | 1. 3 prints land in the coin's first 10 slots.<br>2. A sniper launch shows 30 or more.<br>3. 3 is quiet: the coin passes. | number of slots, print cap | red · mt3 (7.3 8aaRWu) |
| **Early operators** | How many operator structures print inside the coin's first minute | Screens. **Classes present** (P4) counts them at the fire; this counts them at birth. | A bot that chooses a coin before the crowd arrives saw something in it early. | 1. An operator structure buys at age 20 s.<br>2. Another buys at age 45 s.<br>3. Two inside the first minute: the coin passes. | count, window | new |
| **Life at a fixed age** | vsol at a set age sits inside a band, neither flat nor spent | Screens. It is **Curve position** (P1) read once, at a fixed age, so it exists only from that age on. | A moderate first minute shows real demand that has not spent itself. | 1. The band is vsol 50-70 at age 60 s.<br>2. At age 60 s vsol is 58: the coin passes.<br>3. A fire at age 30 s cannot use it: the fact does not exist yet. | the age, the band | open · case 10 |

### D4 Document

What the creator publishes at birth, read from the metadata URI. A later fetch is empty on old
coins, so capture is live (derive 13).

| Name | Idea | Meaning | Why it matters | Example | Parameters | Status |
| --- | --- | --- | --- | --- | --- | --- |
| **Documented** | The creator's document carries a website, a telegram or a description of real length | Screens. The loosest form needs one field; a stricter form needs two, anchored on the website or the channel. | A creator who writes something plans for the coin to live, and a coin that lives gives rises to catch. Two efforts mark a planned project rather than a field filled in to look complete. | 1. The document holds a website and a 120-character description.<br>2. Two fields are filled, one of them the website.<br>3. The coin passes. | which fields, how many, description length | keep · ev 3.3 · mt3 d8 |
| **URI host** | Where the document is stored: a launchpad's own host is out, plain IPFS stays | Screens, by exclusion. It reads where the document lives, not what it says. | A launchpad host means the coin was made in a few clicks; a self-uploaded document took a decision. Read on omego's structure event it removes, it does not pick. | 1. The URI sits on a launchpad's own domain: the coin is skipped.<br>2. The URI sits on `ipfs.io`: the coin stays. | host list | open · ev 3.4g (omego E2f) |
| **Reused content** | The document's URI, site, telegram, name or symbol already appeared on an earlier coin | Screens, by exclusion. | A recycled document is a relaunch or a copycat, and those rarely live long enough to rise. | 1. Coin A launched yesterday with the telegram `t.me/abc`.<br>2. Coin B today carries the same link.<br>3. B is a copy: it is skipped. | which field, look-back | new |

---

## E - Event

The print we fire on, and only that. An event is graded by leftover existence behind the trigger at
our fill (derive 5.2), never by one exit's book. Seed racers and aggregator routes are never a
trigger: they react to someone else's buy, so their print is already behind the decision we want.

### E1 Who printed

| Name | Idea | Meaning | Why it matters | Example | Parameters | Status |
| --- | --- | --- | --- | --- | --- | --- |
| **A class of structure buys** | Fire when a buy lands from an ix structure of a named class | Fires. The class is the parameter and the list is the asset: a list built from one trader's reactions describes that trader, a list of public tools describes the crowd, a list of operator structures describes single owners. Each list is scored on the full tape, never on the trader's own coins. | A trader that makes money has already decided whose buy is worth following; the list is that decision, reusable by us. Retail arrives through public apps; one owner's bot is one decision; a pre-signed order was decided before the last print; an unusual instruction order marks one pusher's own bot. | 1. The list holds an Axiom buy structure and a nonce buyer.<br>2. The nonce buyer buys this coin.<br>3. It is on the list: we fire on that buy. | class: trader-derived list (8dtx's 155 structures), public tool, operator structure, nonce buyer, direct pump.fun buy by instruction name, campaign order (fee set before CU limit) | keep · mt3 d8 · mt3 6.1 · ev 7 (campaign-break) |

### E2 This structure's record on this coin

One structure's own history here is what turns "a buy landed" into a decision. One fire per
(coin, structure) where the row says so.

| Name | Idea | Meaning | Why it matters | Example | Parameters | Status |
| --- | --- | --- | --- | --- | --- | --- |
| **First here** | A structure's first print on this coin | Fires. **Back after a gap** is the same structure returning; this is it arriving. | A first buy is a fresh decision about this coin, where a return can be a top-up of one already made. | 1. A nonce buyer has never printed on this coin.<br>2. It buys now.<br>3. We fire. | listed structures only, or any | red · ev 7 (C16) · mt3 6.1 |
| **Back after a gap** | A structure buys again after a stretch of its own silence on this coin | Fires. **Breaks the coin's silence** (E3) reads the whole coin's silence; this reads one structure's. The gap can be cut on one side or both, and the coin may be required to have kept printing meanwhile. | A structure that pauses and comes back with size is making a new decision, not continuing one. Too short a gap is a bot continuing, already in the price; too long is a bot that has forgotten the coin. | 1. An Axiom structure last printed on this coin 25 slots ago.<br>2. The coin kept printing meanwhile, and the structure now buys 0.6 SOL.<br>3. We fire. | gap length (slots), band or one-sided, coin kept printing meanwhile, size floor, K buys in the returning slot, class of the structure (tool, router, operator, any), first return only | keep · ev 6.4 · mt3 d8 · ev 7 (C5, C12) · mt3 6.1 |
| **Adding a leg** | A structure that usually buys a coin in several bursts makes its next burst here | Fires. **Clip step-up** reads the size of the next buy; this reads only that the plan has another leg. | A bot that buys in legs still has SOL to spend here, and its own next legs push the price we bought at. | 1. A bot buys most of its coins in two or more bursts.<br>2. Its first burst here lands at age 40 s.<br>3. Its second lands at age 60 s: we fire on it, ahead of any third. | first leg or later leg; share of its coins bought in legs; how much of its usual spend is still unspent here | red · ev 7 (C9, C11) · mt3 6.1 |
| **Clip step-up** | A structure buys more than its own last buy on this coin | Fires. **Trigger size** (E4) compares the buy with the coin; this compares it with the structure's own last buy here. | A setup that comes back with a bigger buy is adding to a position, not closing one. | 1. A structure buys 0.3 SOL here.<br>2. Later the same structure buys 0.5 SOL.<br>3. The second buy is bigger than its last: we fire on it. | step size (ratio), plus the price step and the buyer being new to the coin as extra terms | red · ev 7 (rule 3b) · mt3 6.1 |
| **Buying back under its sell** | The coin is quiet under a price this structure sold at, it has not bought back, and it usually does | Fires. The only E2 row read off the structure's sells. | A bot that sells high and buys back lower is likely to buy back here, and its buy lifts the price. | 1. A structure sold here at vsol 70 and has not bought back.<br>2. The coin sits quiet at vsol 60.<br>3. It rebuys after selling on most of its coins: we fire. | class (tools vs operators), how often it rebuys elsewhere, how far under its sell | red · ev 7 (C10, C9) |

### E3 The tape just before the print

The print is read in the context of the prints around it. A price-path fact standing at the print
is P2, not a trigger.

| Name | Idea | Meaning | Why it matters | Example | Parameters | Status |
| --- | --- | --- | --- | --- | --- | --- |
| **Breaks the coin's silence** | The first buy above a floor after the whole coin has been silent | Fires. **Back after a gap** (E2) reads one structure's silence; this reads the coin's. **Coin silent now** (P2) is the same fact standing, as a filter. | Size landing on a quiet tape is a decision, and it can wake the buyers who left. | 1. No print lands on the coin for 10 slots, about 4 s.<br>2. A 0.6 SOL buy lands.<br>3. We fire. | slots of silence, size floor, K buys in the breaking slot, the breaker's class (listed, new to the coin, any) | red · ev 7 (C5) · mt3 d8 · mt3 6.1 |
| **After sellers** | The first buy after selling | Fires. The buy is the trigger and the selling is its context; **Net flow** (P2) is the windowed form of the same fact, as a filter. | Buyers come back once a run of sellers is spent, and the first of them buys the low; a buy that takes a seller's tokens is absorbing supply, not chasing. | 1. Three public sells land in a row.<br>2. A 0.5 SOL buy that is not a seed racer lands next.<br>3. We fire on that buy. | how many sells in a row, a known structure as the seller, a sell in the same slot, size floor on the buy, not a seed racer | red · ev 7 (C14) · mt3 6.1 |
| **A sell we buy into** | A public sell of size by a seller of a named kind; we buy the dip it makes | Fires. Rule 1's event. The seller's kind is the parameter; the young-coin form is the same print without **Age** (P1). | The dip a scheduled or forced seller makes is a discount rather than news: a flipper's sell is an exit, not an opinion; a seller under water is giving up; a seller who is all out cannot hit us twice; the frenzy's next buyers absorb the drop. | 1. The coin made a new high 10 s ago.<br>2. A wallet that bought 20 s ago sells 1.5 SOL.<br>3. We buy the dip that sell made. | seller kind (bought within 30 s, under its cost, all out, the pusher, a top holder, a listed structure), size floor, inside a frenzy (new high in 20 s), the coin's first big sell | keep · ev 1.22 (rule 1's event) · ev 1.27 · case U5 · mt2 6.2 · mt3 5.2 88887Q · ev 1.16 · mt3 d8 |
| **Count crossing** | The Nth arrival of a kind on this coin, and the crossing itself is the fire | Fires, once per crossing. **Classes present** (P4) is the standing count as a filter. | The first stranger can be the creator's second wallet; the second is a pattern. Several bots choosing the same coin inside seconds is a decision the market is making now. | 1. The creator bought, then one other wallet.<br>2. A second wallet other than the creator buys.<br>3. The count crosses 2: we fire, once. | what is counted (wallets other than the creator, operator structures, distinct structures), N, window (lifetime, 2 s, 5 s), the first real bot after only creator and racers | red · ev 3.7 · ev 7 (C15) · mt1 5.2d · ev 3.4g |

### E4 How loud the trigger is

Facts of the one print we answer, knowable at our fill.

| Name | Idea | Meaning | Why it matters | Example | Parameters | Status |
| --- | --- | --- | --- | --- | --- | --- |
| **Trigger size** | How big the buy is against a denominator: a band, not a floor | Fires, or is a term of another E row. The denominator is the parameter; the same SOL means something on a quiet coin and nothing on a busy one, and the same fact read as the price step is what it did to the pool. A very large buy has already moved the price before our fill lands, so the fact turns around above about 2 SOL. | Size is conviction, the simplest form of it to read; size against the structure's own habit is a change in behaviour; the price step says whether the actor is placing or chasing. | 1. The coin's last 20 prints have a 75th percentile of 0.2 SOL.<br>2. This buy is 0.8 SOL, 4x that.<br>3. It sits inside the band: the fire stands. At 3 SOL it would not. | denominator: absolute SOL; the coin's last public buy; the 75th percentile of the last 20 prints; the coin's buying rate over 30 slots; the structure's usual clip elsewhere; the price step (`mvk`); floor per structure or one floor; band | keep (term) · mt3 d8 · ev 7 (G1) · mt3 6.1 · ev 3.4g (omego E2b) · mt3 2b wide: 0.80 on his pick, 0.77 within the coin; the strongest fact and never a rule |
| **Fee paid** | The print pays a priority and tip fee at the high end | Fires, or is a term. Compared with the tape, or with what this structure usually pays. | Paying extra to land sooner is urgency; urgent for that bot in particular is a change, not its habit. | 1. Most prints on the tape tip 0.0005 SOL.<br>2. This one tips 0.002 SOL.<br>3. It sits in the top quarter: the fire stands. | vs the tape's distribution; vs the structure's own 20 or more earlier prints | keep (term) · ev 7 (zigzag turn) · mt3 6.1 · mt3 2b wide: against the coin's own recent buyers 0.58 on his pick |
| **Slot company** | How many prints, distinct structures and tipped prints landed in the trigger's slot before it | Fires, or is a term. Only what landed before the trigger is knowable at our fill; "alone in its slot" needs the slot to close and is dropped. | Several buys queued into one block are a decision many actors made at once; several bots paying a tip to race the slot is intent rather than appearance. | 1. Before the trigger, two other buys landed in its slot.<br>2. Both paid a Jito tip.<br>3. Two tipped prints already in the slot: the fire stands. | count of prints, count of distinct structures, count of tipped prints, two listed structures in the slot | open · ev 3.4g (omego E1e) · mt3 d8 · mt3 6.1 · mt3 2b wide: counted without our own prints; tipped ahead 0.545, prints ahead 2.4x at >= 2 |

### E5 The seat

| Name | Idea | Meaning | Why it matters | Example | Parameters | Status |
| --- | --- | --- | --- | --- | --- | --- |
| **Fixed age** | Fire when the coin reaches a set age, with no print needed | Fires, on the clock alone. The control every event is measured against: a moment nobody else is acting on costs no slip, so a signal only has to beat the toll. At a self-chosen moment no coin-state fact moves the book. | It shows what buying a coin at that age earns by itself, so an event has to beat it to be worth anything. | 1. The age is 120 s.<br>2. Every coin that reaches 120 s gets a fire, no print needed.<br>3. Its book is the floor every event must beat. | the age, or every N seconds | keep (control) · mt3 d8 |
| **First size after T** | The first buy above a floor once the coin is past a set age | Fires, once per coin. **Breaks the coin's silence** (E3) needs a quiet tape first; this needs only the clock. | It catches the first real buyer once the launch noise has cleared. | 1. The coin passes age 60 s.<br>2. At age 75 s a 0.6 SOL buy lands.<br>3. We fire, once on this coin. | T, floor | new |
| **Fill delay** | When we fill relative to the trigger: at once, or after a set wait, or only after the price came back or moved on | Fires, later. The DELAY of derive 5. Both waits read red on the push: the price keeps climbing after a push and decays later, and a delayed entry pays for the response it waited for. | The trigger's own impact is what we pay at 83 ms; a later fill could get the move without it, if the move outlasts the wait. | 1. A push lands.<br>2. We wait 1 s instead of filling at once.<br>3. We buy at whatever the price is then. | wait length; condition: price back near pre-print, or public buys landed and price held | red · mt3 d8 |

### E6 Graveyard

Dead by mechanism. Do not rebuild these as the event.

| Name | Idea | Meaning | Why it matters | Example | Parameters | Status |
| --- | --- | --- | --- | --- | --- | --- |
| **Price-path turn** | The first print after the price turns up from a low, or after a pullback of a set share | Fires. It reads the shape of the chart and nothing else. | From the price alone a turn and a falling knife look the same: the detector is right 5.6 % of the time where it needs about 45 %. The price path is a P fact (P2), never a trigger. | 1. Price falls 30 %.<br>2. It bounces 15 % off the low.<br>3. The row would fire here; it is dead. | - | dead · ev 7 (hot-tape price-path) |
| **New-buyer acceleration** | Several wallets buy the coin for the first time inside a few slots | Fires. **Tape busyness** (P2) is the same count standing. | They are the move, so by the time the count rises the price has paid for it. | 1. Five new buyers land inside 10 slots.<br>2. The row would fire here; it is dead. | - | dead |
| **Racer burst** | Seed racers pile into a coin | Fires. | A seed racer only reacts to someone else's print, so it confirms a decision already in the price. | 1. The coin is silent 10 slots.<br>2. Four seed racers buy.<br>3. The row would fire here; it is dead. | - | dead |
| **Copy a wallet** | Buy whenever a chosen wallet buys, or follow the wallet that others copy | Fires. | Its own buy and the swarm behind it are in the price before our fill lands; a copied wallet's money is its own impact (ev 3.4h). | 1. A named wallet buys a coin.<br>2. We would buy the same coin behind it; it is dead. | - | dead · ev 3.4h · ev 7 (copied buyer) |
| **Feed tells** | Feed position, a creator buying again, the cadence of prints, an up-move portrait | Fires. | Everything they read is public and slow, so the move they describe is already priced. | 1. The creator buys again, and the coin is on the feed's front page.<br>2. The row would fire here; it is dead. | - | dead · ev 7 (hot-tape price-path) |

---

## P - Permission

State standing true when the print lands. P does not explain why a move happens; it puts the fire
where a move can pay and a loss is bounded, and it cuts before occupancy so a refused fire frees
the coin (derive 9). Every row names its time basis.

### P1 Where the coin is

| Name | Idea | Meaning | Why it matters | Example | Parameters | Status |
| --- | --- | --- | --- | --- | --- | --- |
| **Age** | The coin's age sits inside a band when the print lands | Filters. At the print. An E row never carries an age word; it comes from here. | A rule is only tested inside the part of a coin's life it was read on; a frenzy on a young coin dies with the seller who started it and on an old one the crowd absorbs it; sniper-eating rugs die inside 10 s. | 1. The band is age 158 s and older.<br>2. The coin is 200 s old when the print lands: the fire is allowed.<br>3. At 30 s old it is not. | band (booked on rule 1: age >= 158 s) | keep · ev 1.22 · mt2 9.1 |
| **Curve position** | vsol sits inside a band when the print lands | Filters. At the print. On the curve one number is price, market cap, progress, real reserve (vsol - 30) and headroom to the wall at 115, so every spelling is this row. The upper cut can be set from the target: a +20 % target needs vsol under about 105, a +100 % target vsol 81 or under. | Where on the curve the fire sits decides both how far the coin can rise and how far it can fall; a target above the wall cannot be reached at any price. | 1. The band is vsol 45-100.<br>2. The print lands at vsol 60: the fire is allowed.<br>3. At vsol 105 a +20 % target no longer fits under the wall: refused. | band, or headroom for the target (booked on rule 1: vsol <= 100) | keep · ev 1.20 · ev 6.4 · mt2 9.1 |
| **Pool alive** | The coin still holds SOL and is still printing | Filters. At the print. | A pool nobody trades has no buyer to sell our tokens to, whatever the price says. | 1. The last print landed 3 s ago.<br>2. The reserve is 12 SOL.<br>3. The coin is alive: the fire is allowed. | last-print gap, reserve floor | new |

### P2 What the tape did

Net SOL over a window, the price change over it and the vsol change over it are one number on a
bonding curve, so windowed flow is the price path. What stays separate is count against SOL, and
public against all.

| Name | Idea | Meaning | Why it matters | Example | Parameters | Status |
| --- | --- | --- | --- | --- | --- | --- |
| **Net flow** | Buy minus sell over the window before the print, with a sign: which side is good is the parameter | Filters. Window. Read as SOL it is the price move; read as a count of prints it is not. On the push event the sold-off side is the one door term that survives both folds; on rule 1's pool buying leading is what pays. | A coin where buying leads is not decaying, so our fill is not the last one in; size stepping into a tape everyone else is leaving is absorption. | 1. In the last 30 s the coin is bought 4 SOL and sold 8.<br>2. Net is -4 SOL.<br>3. On a rule that wants the sold-off side, the fire is allowed. | window (seconds, slots, prints), SOL or count, public or all, sign and cut, the sell-side shape (down over 10 s) | keep · ev 1.3 · mt3 d8 (both signs) |
| **Tape busyness** | How many prints, wallets or structures landed in the window, or none at all | Filters. Window, or right now. Few prints, many first-time buyers still arriving, many distinct structures with none dominating, and the coin silent right now are all readings of it. | A busy tape is a pile-in that has already moved the price; new buyers are who pays for the next rise; a print on a quiet tape is a fresh decision. | 1. The cap is 25 prints in 5 s.<br>2. 12 prints land in the last 5 s.<br>3. Under the cap: the fire is allowed. | what is counted (prints, first-time buyers, distinct structures, share of the largest), window, silent now | keep (term) · ev 1.11 · ev 7 (rule 3b) · mt3 6.1 |
| **Big sells lately** | How many public sells above a size landed in the window | Filters. Window. | A large holder on the way out sells more than once, and the rest of that exit would land on our position. | 1. The cap is 4 sells of 1 SOL or more in 30 s.<br>2. Two such sells landed in the last 30 s.<br>3. Under the cap: the fire is allowed. | size, window, cap | red · ev 7 (rule 3b) |
| **A rise exists** | The coin has completed at least one hill | Filters. Lifetime. **Under its peak** reads where the price sits after it. | A coin that has risen once has buyers who can lift it, which is the whole bet. | 1. Earlier, vsol went from 40 to 50.<br>2. Price rose (50 / 40)^2 - 1 = +56 %: a hill.<br>3. The coin has made a hill: the fire is allowed. | hill count, hill size, second hill | keep · ev 6.4 |
| **Under its peak** | How far the price sits under its peak, and for how long | Filters. Lifetime or window. Read at both ends: at the high the move is spent, far under it the coin is falling; a coin far under for minutes has lost its buyers. A hill whose fall is fully closed is a recovered flush. | The peak proves buyers exist, and the fall back leaves the room they need to do it again. | 1. The coin's peak price is 1.00.<br>2. The price is 0.75, 25 % under it, for 40 s.<br>3. Inside the depth and duration band: the fire is allowed. | depth, duration, peak basis (lifetime, last hill, the window's high), recovered or not | keep (term) · mt3 d8 (omego pullback red) |
| **Low holding** | vsol has made no new low for a stretch, or a named floor never broke | Filters. Window. **Low breaks** (X2) is the same test after we are in. | A low that holds means the sellers did not take control, so the reason we fired still stands. | 1. The last low is vsol 52.<br>2. For 12 slots vsol has not gone under 52.<br>3. The low held: the fire is allowed. | slots without a new low, floor | new |

### P3 Who holds the supply

Read inside a vsol band: below vsol 42.43 a -50 % move is impossible, so vsol alone separates
nothing here. The loss door of rule 1 lives in this family.

| Name | Idea | Meaning | Why it matters | Example | Parameters | Status |
| --- | --- | --- | --- | --- | --- | --- |
| **Concentration** | The biggest one, or biggest ten, holders' share of live supply: smaller is better | Filters. At the print. **Bundled share** reads a group that bought together; this reads size alone. | One wallet with a large share can drop the price by itself, and no one else has to agree. | 1. The top wallet holds 15 % of live supply.<br>2. The cap is 10 %.<br>3. Over it: the fire is refused. | top-1 or top-10, cap | red · case L4 · ev 3.7 |
| **Bundled share** | Wallets whose first buy landed in a same-slot group hold a small share of live supply | Filters. At the print. The creation slot alone reads 0 past age 158 s, so rule 1 reads any slot. | A group that bought on one trigger sells on one trigger, and the price never warns first. | 1. At age 90 s, 4 wallets bought in one slot through one ix structure.<br>2. They hold 35 % of live supply.<br>3. 35 % is over 28.57 %: the fire is refused. | creation slot vs any slot, group size (3 or more on one structure), cap (booked on rule 1: <= 28.57 %) | keep · ev 3.7 · ev 1.28 · mt3 2b wide: the first slot's buyers' remaining share 0.47 on his pick |
| **Public-app share** | Holders whose first buy went through a public app hold most of the live supply | Filters. At the print. A holder is classed once, at its first buy. | The supply outside the public apps is mostly one bot swarm that sells in a single slot; a swarm's wallets never come back, which is what the two-buys half of the public-app test catches. | 1. Holders whose first buy came through a public app hold 800 M of 1,000 M live tokens.<br>2. 80 % clears 70 %.<br>3. The fire is allowed. | floor (booked on rule 1: >= 70 %) | keep · ev 1.28 |
| **Creator's position** | What the creator holds, whether he has sold, whether he has bought again | Filters. At the print, or the last slots for the rebuy. A survival term: it keeps coins alive longer and raises the loss rate, because it keeps us in coins that fall slowly. | A creator still holding has not rugged yet; a small bag has little to dump; a creator adding is betting on the coin. | 1. The creator bought 1 SOL at launch.<br>2. He has sold nothing since.<br>3. The fire is allowed. | share cap, has not sold, rebought in the last N slots | keep · ev 3.4 · ev 3.7 · mt3 2b wide: the birth buyer's bag 0.52 on his pick, its selling as an exit +1.4 purpose on his picks only |
| **Actor still holds** | The structure that lifted the last hill, or the structure we fire on, has not sold | Filters. At the print. **Actor sells** (X3) is the same actor after we are in. On the pusher it reads inverted: its selling is a better sign than its holding. | Whoever paid most to lift the price has the most reason to want it higher; the one we follow still wanting the price up is the reason we buy. | 1. The pusher bought 3 SOL during the last hill.<br>2. It has sold nothing.<br>3. The fire is allowed. | which actor (pusher, firing structure) | red · ev 7 (the extraction tell) · mt3 2b wide: the trigger's structure already holding 0.46 on his pick |
| **Crowd left** | The buyers of the last hill hold less than half of what they bought | Filters. At the print. The opposite reading, buyers still in, is refuted and inverted: still-held supply is the next sellers. | Their selling is already spent, so it will not land on top of our rise. | 1. The last hill's buyers bought 10 M tokens.<br>2. They hold 4 M, 40 % of it.<br>3. Under half: the fire is allowed. | share held | keep · ev 6.4 · ev 1.14 |
| **Holders' PnL** | The share of supply held by wallets in a named profit band right now | Filters. At the print. Near-even holders sell to get out even inside the rise we trade for; the SOL they would get against the SOL a +20 % rise needs is the get-out-even wall. On a frenzy the losing side is the younger, thinner coin, so more holders in profit reads as maturity. | Holders' selling into a rise is what the rise has to absorb, and where they sit against their cost says how much. | 1. The price is 1.00.<br>2. Holders who paid 1.00-1.25 (down 0-20 %) hold 18 % of supply.<br>3. A +20 % rise brings them back to even, and 18 % is over the cap: the fire is refused. | band (-20..0 %, any loss, +20 % or more, far up), as share or as the wall ratio | red · case U5 · ev 1.14 · mt3 2b wide: holders in profit, the float's gain, the last seller's profit read 0.44-0.50 on his pick and on BIG |
| **Operators round-tripped** | Operator structures that bought this coin have already sold out of it: fewer is better | Filters, by exclusion. At the print. | Bots that already took their profit here will not pay for our rise. | 1. 7 operator structures bought this coin.<br>2. All 7 have sold out.<br>3. The fire is refused. | count | new |

### P4 Who has acted on this coin so far

| Name | Idea | Meaning | Why it matters | Example | Parameters | Status |
| --- | --- | --- | --- | --- | --- | --- |
| **Classes present** | Which kinds of ix structure have printed on this coin, and how many | Filters. Lifetime or window. Read as a count of operator structures, a count of public tools, a count of all distinct structures, or one named structure present or absent in the window. **Count crossing** (E3) fires on the moment the count changes. | Each operator chose this coin on its own, so the count says how many independent decisions are in it; a structure a paying wallet stays away from is as much a decision as one it answers, and the two are not the same list. | 1. 9 different operator structures have bought the coin.<br>2. The cut is 8 or more.<br>3. 9 clears it: the fire is allowed. | kind (operators, tools, all, one named structure), count band or presence, window (lifetime, 5 s, 30-60 s) | keep · ev 3.3 · ev 3.4g (omego E1a) · mt1 5.2d · mt3 2b wide: listed structures holding 0.40 on his pick, structures selling in 30 s 0.46 |
| **Distinct wallets** | How many distinct wallets have bought this coin: a band | Filters. Lifetime. Rule 1 pairs it with **Age**. | It is the plainest measure of how many people the coin has reached; a coin that is not crowded yet still has its buyers ahead of it, and an old broad one absorbs a sell. | 1. 400 wallets other than the creator have bought the coin.<br>2. The floor is 368.<br>3. 400 clears it: the fire is allowed. | band (booked on rule 1: >= 368 non-creator buyers), buyers only or all | keep · ev 1.22 · ev 7 (rule 3b) |
| **Record of those present** | The earlier-day record of the structures already on this coin, averaged | Filters. Lifetime. **Structure record** (P5) reads the one structure firing; this reads the company it is in. | The "many smart wallets on one token" idea, read through structures instead of wallets, which rotate. | 1. Five structures have printed on this coin.<br>2. Their earlier-day answer rates average 0.55.<br>3. Above the bar of 0.50: the fire is allowed. | mean, best, count above a bar | red · mt3 d8 |
| **Racers around** | No seed racer has printed in the slots just before ours, or all recent buying came through public tools | Filters. Window. The trigger itself never being a racer is E's standing exclusion. | If the racers have not arrived yet, the move they react to is still ahead of us; retail through apps keeps arriving, racers dump what they just bought. | 1. The window is the last 10 slots.<br>2. No seed racer printed in it.<br>3. The fire is allowed. | window, tools-only form, SOL band through tools | new · open (tools only, buy SOL band) |
| **Order of the last prints** | Which structures printed just before the trigger, in order and with their side | Filters. Window of the last one to three prints. **Classes present** counts them; this reads the sequence, so "a retail app bought, then a Token-2022 program bought" is one value. | The bots that trade together land in an order, and a wallet that follows them reads that order. | 1. Two prints back, a retail-app structure bought.<br>2. One print back, a Token-2022 program bought.<br>3. That sequence is on the allowed list: the fire is allowed. | one to three prints back, side kept or not | new · mt3 2b wide (his pick 5-12x, BIG and DEAD move together) |

### P5 The trigger structure's record on earlier days

| Name | Idea | Meaning | Why it matters | Example | Parameters | Status |
| --- | --- | --- | --- | --- | --- | --- |
| **Structure record** | The share of this structure's earlier pushes that were answered by size, or what they earned at our seat, on strictly earlier days | Filters. Earlier days only. The one fact on the push node that predicts the outcome and carries no slip, because it is history and not tape; the money form is weaker than the answer form and adds nothing on top of it. | Everything else that predicts incoming size is size that already landed, which is what we pay for; this one is free. | 1. On earlier days, 60 % of this structure's pushes were answered by a size buy.<br>2. The bar is 50 %.<br>3. It clears the bar: the fire is allowed. | answered or earned, window of days (expanding, rolling), cut | keep (term) · mt3 d8 |

---

## X - Exit

A state cut reads the tape rather than the price, so it can be spelled three ways: only under the
fill, only above it, or with no gate on the price. **Which spelling pays follows from what the
clause reads** (case K19, K20): silence is ungated, because a tape that has gone quiet ends the
reason to hold on either side of the fill; an actor acting is loser-only, because a sell print is
the counterparty a rising coin needs. Read all three before calling a row red.

### X1 Static

| Name | Idea | Meaning | Why it matters | Example | Parameters | Status |
| --- | --- | --- | --- | --- | --- | --- |
| **Clock** | Close after a fixed time, whatever the price is doing | Sells. The control every other exit is measured against. On the push its own path asks for about 5 s; a trader's own hold time is not ours. | It shows what the event earns by itself, so any exit that cannot beat it is adding nothing. | 1. The clock is 45 s.<br>2. We sell 45 s after the fill, whatever the price is. | T | keep (control) · mt3 d8 · mt2 8.2 |
| **Take profit** | Close at a fixed gain, or at a share of the room left to the wall | Sells. A control on price instead of time; scaled to headroom it is right at both ends of the curve where one fixed number is wrong. | It shows how often each size of gain is reached, which is what a target has to be chosen from. | 1. At vsol 70 the room to the wall is (115 / 70)^2 - 1 = +170 %.<br>2. The target is 0.4 of it, +68 %.<br>3. The price reaches +68 %: we sell. | fixed gain; share of headroom (rule 1b: 0.4) | keep (control) · ev 1.25 |
| **Stop** | Close at a fixed loss | Sells. A floor under every other exit. A breakeven floor is a stop moved to the fill once ahead, and it scratches the winners, whose own trough is -5.8 % at the median. | It caps the worst single outcome. | 1. The stop is -40 %.<br>2. The price falls to -40 % from our fill.<br>3. We sell. | level; moved to the fill once up G | keep (control) · ev 7 (breakeven floor) |
| **Trail** | Sell once the price falls a set share from its best since our fill | Sells. Unarmed it trails from the first second; armed it waits for a gain first, and without a stop under the arm four fifths of entries hold to the cap with nothing cutting them. The width can widen as the peak grows, and the peak can be the burst we joined instead of the coin's. | It rides a move as far as it goes and leaves when the move gives back, without guessing a target; a big winner swings harder, so one width is wrong somewhere. | 1. The trail arms at +21 % and is 36 % wide.<br>2. The trade peaks at +50 %.<br>3. It falls 36 % from that peak: we sell. | arm level, width, stepped width, cap, peak basis (coin, the burst), with or without a stop (booked: arm +21 %, trail 36 %, stop -43.75 %, cap 1200 s) | keep · ev 4.7 · ev 7 (armed trail with no stop) · ev 1.24 |
| **Bracket** | A target, a stop and a clock, whichever comes first | Sells. Rule 1's exit. Half out at the target, a trail after the target and a longer clock are its spellings, all red on rule 1. The tail-keeping shape (wide target, wide trail, long cap) is the same three numbers set for the rare large winner, and it is unreachable on a pool where four fires in five never run. | An event that bounces quickly pays a small sure target, and the wide stop is there for the rare collapse. | 1. The target is +20 %, the stop -60 %, the clock 90 s.<br>2. The trade reaches +20 % at second 40.<br>3. That comes first: we sell. | the three numbers (booked on rule 1: +20 %, -60 %, 90 s), half out, trail after target | keep · ev 1.22 · ev 1.24 · ev 1.20 · ev 3.4i (omego E5) |
| **Gate then ride** | At a set age, sell at once unless the trade is up enough; if it is, hand it to a trail and hold | Sells. A bracket caps every trade at the same age; this gates, so a dead trade is cut and a live one is let go. The gate belongs where the losers die, not before: on 8dtx2t that is 8 s, not 5. | A trader whose median trade loses and whose top 1 % carries the net earns from the right tail, and a flat clock cuts every tail it has. | 1. The gate is at 8 s.<br>2. At 8 s the trade is up 45 %, over +40 %: it goes to a trail capped at 300 s.<br>3. Another trade is up 3 % at 8 s: we sell it now. | gate age, ride condition, trail and cap after the gate; per structure or one for all | keep · mt3 d8 |
| **Abort** | Sell if the trade is not up by a set amount after a set time | Sells. **Clock** closes everything at the time; this closes only what has not moved. | Three to twenty seconds in, a future winner and a dud read the same on price and a clock, so the grid has no positive cell. | 1. The rule is +5 % by 30 s.<br>2. At 30 s the trade is +2 %.<br>3. We sell. | T, G | dead · ev 4.4 · ev 3.4g (omego E2d) |

### X2 The tape stops

| Name | Idea | Meaning | Why it matters | Example | Parameters | Status |
| --- | --- | --- | --- | --- | --- | --- |
| **Silence of X** | No print from X for a stretch, on either side of the fill | Sells. Whose silence is the parameter. First-time public buyers stopping replaces the clock rather than adding to it, fires on 92 % of the book and holds a plateau, and is blocked on one engine metric. Operators stopping beats the clock and fails the fold test. The coin's whole tape stopping is the best state cut the lab reads. The firing structure alone falling quiet is noise: a tool prints on one coin among hundreds. An arm on a gain first is inert. | The queue behind us is what lifts the price; when it empties there is nobody left to pay more, as true of a winner as of a loser. | 1. X is first-time public buyers; N is 6 s.<br>2. The last new buyer arrived at second 40, and second 46 passes with none.<br>3. We sell, whether we are up or down. | X: first-time public buyers, operator structures, any print, the firing structure; N; gate (ungated, loser-only, winner-only) | keep · case K15-K22 · ev 4.7 · ev 7 (C15) · mt3 2b wide: no public buy for 10 s under the cap curve is the one logic that holds on both halves of his picks |
| **Flow turns** | Public sell SOL over the last window is at least the public buy SOL | Sells. **Net flow** (P2) is the same reading before the fire. | The balance of the tape carries the price; but the balance turns in normal chop, so it fires early and often and cuts the winners. | 1. The window is 20 s.<br>2. In it the crowd sells 4 SOL and buys 3.<br>3. We sell. | window, gate | red · case K18 |
| **Fees fall** | The prints after ours stop paying extra to land quickly | Sells. **Fee paid** (E4) is the same reading before the fire. | Urgency is what makes a move run; when nobody pays for speed any more, the hurry is over. | 1. The buyers around our fill tipped 0.002 SOL.<br>2. The last 5 s of buyers tip 0.0005 SOL, a quarter of that.<br>3. We sell. | ratio to the entry level, window | new · mt3 2b wide: purpose -16 to -18 on every push |
| **Low breaks** | vsol makes a new low, or returns to the low before the event | Sells. **Low holding** (P2) is the same test before the fire. On an event that fires on a seller the fill is still falling, so a new low lands 0.5 s later on every trade and the clause is a same-tick exit. | The bounce we bought has failed and the selling that made it is not finished; everything the event did has been undone. | 1. Before the event the low was vsol 55.<br>2. After our fill vsol returns to 55.<br>3. We sell. | new low vs pre-event low, gate | red · ev 7 (C9, C14) · case K20 |
| **Buys fade** | The last few public buys after ours are small against the trigger | Sells. **Trigger size** (E4) before the fire, this after it. | The buyers still arriving are the small ones, so the move has no money left behind it. | 1. The trigger was a 1 SOL buy.<br>2. The next three public buys are 0.05, 0.08 and 0.04 SOL.<br>3. Each is under a tenth of the trigger: we sell. | how many buys, the ratio to the trigger | red · mt3 2b wide (purpose -12 to -29) |

### X3 An actor acts

| Name | Idea | Meaning | Why it matters | Example | Parameters | Status |
| --- | --- | --- | --- | --- | --- | --- |
| **Actor sells** | A named actor sells this coin; we react | Sells. Who sells and how we react are the parameters. Loser-only is the spelling that pays: a sell is the counterparty a rising coin needs, so leaving on one while ahead hands the move away. The creator is inert past the age rule 1 fires at, because he has no bag left. The first big sell whoever sent it is red on 8dtx2t too: in 57 % of his exits the sell he leaves on is smaller than one he sat through. | The one we followed knows its own reason better than we do; the buyer who lifted the price is the one whose selling drops it furthest; a creator selling can be the first leg of a rug. | 1. We fired on a structure's buy and are 8 % under the fill.<br>2. That structure prints a sell.<br>3. Loser-only: we sell. Had we been up, we would hold. | actor (firing structure, pusher, creator, anyone above a size, a winner selling half), reaction (out now, out if losing, trail from here) | red · case K18-K20 · ev 7 (C11) · mt3 5.1 sells · mt3 2b wide: a loser dumping half at -40 %, the birth buyer, the trigger's structure, a listed structure: each judged alone; only the loser dump keeps runners |
| **Actor leaves** | The ix structure we fired on starts buying a different coin | Sells. **Silence of X** reads its silence here; this reads its activity elsewhere. | Its attention and its money have moved on, so it will not buy this coin again. | 1. We fired on a structure's buy.<br>2. That structure starts buying a different coin.<br>3. We sell. | - | red · ev 7 (C13) |
| **Sell into a buyer** | Once we are up enough, sell on the next big public buy or hard price step | Sells. | A big buy is both a high price and a counterparty, which is exactly what a seller needs. | 1. We are up +12 %, over the +10 % arm.<br>2. A 1.2 SOL public buy lands.<br>3. We sell into it. | gain before it arms, buy size or price step | red · ev 1.24 |

---

## R - Re-entry

| Name | Idea | Meaning | Why it matters | Example | Parameters | Status |
| --- | --- | --- | --- | --- | --- | --- |
| **One open per coin** | One open position per coin at a time, with no cap on how many tickets a coin gets | Filters. Standing policy every sentence starts from. Being flat and past our first exit is the state that carries most of a re-entering trader's signal: he answers 14 % of the pool's prints while flat and 0.18 % while he holds. | A coin can be traded again and again, but never doubled up, so one bad coin cannot take two losses at once. | 1. We hold a position on the coin.<br>2. A new fire lands on it: skipped.<br>3. After we close, the next fire is allowed. | - | keep · ev 3.4g (omego E2b) |
| **Tickets per coin** | How many round trips we have already made on this coin | Filters. As a hard cap, or as a weight. | Each round trip takes some of what the coin had left, so the next one starts from less. | 1. The cap is 3 round trips.<br>2. We have made 3 on this coin.<br>3. The 4th fire is skipped. | cap; weight | open · ev 1.11 · ev 1.20 · ev 3.4g |
| **Cool-down** | No re-entry on a coin for a while after our last exit | Filters. After any exit, or only after a stop; read as a wall or as a number. | A coin that just stopped us out is usually still falling; coming straight back means the reason we left has not changed. | 1. The cool-down is 60 s after a stop.<br>2. We stop out at 14:02:00.<br>3. No fire on that coin until 14:03:00. | T, after any exit or after a stop | open · ev 1.20 · ev 3.4f, 3.4g |
| **Price vs last exit** | Re-enter a coin only under the price we last sold it at | Filters. | Buying back cheaper than we sold keeps the same upside with less paid for it. | 1. We sold at vsol 60.<br>2. A new fire at vsol 55 is allowed.<br>3. A new fire at vsol 62 is not. | - | open · ev 1.11 |
| **Last result carried** | How the previous position on this coin ended, carried into the next fire | Filters. | A coin that paid once has shown it can move; a coin that just took a stop has shown who is in control. On omego it separates nothing: he re-enters after a loss as readily as after a win. | 1. The last position on this coin closed at +14 %.<br>2. The next fire carries +14 % as a fact a rule can cut on. | win or loss | red · ev 3.4e-g |

## S - Size

| Name | Idea | Meaning | Why it matters | Example | Parameters | Status |
| --- | --- | --- | --- | --- | --- | --- |
| **Fixed clip** | Every ticket buys the same SOL | Sizes. Standing policy at 0.2 SOL. The largest flat clip that keeps a rule's bars is that rule's clip. A bigger clip does not pay for itself on a pump.fun pool: the fixed lamports are a small part of the toll and the venue fee is proportional, so a bigger ticket buys back almost nothing while our own impact grows. | Small enough that our own buy barely moves the price we are buying at. | 1. Every ticket buys 0.2 SOL.<br>2. At vsol 50 that moves the price about 0.4 %. | the clip (standing 0.2 SOL; booked on rule 1: 0.35 SOL) | keep · ev 1.20 · mt3 d8 |
| **Cost minimum** | The clip where the fixed cost of a leg and our own price impact balance: the square root of the fixed cost times vsol | Sizes. Arithmetic, not a fit. | Under it the fixed cost eats the trade; over it our own impact does. | 1. The fixed cost is 0.000227 SOL a leg and vsol is 70.<br>2. sqrt(0.000227 x 70) = 0.126 SOL.<br>3. The ticket buys 0.126 SOL. | - | open |
| **Fraction of vsol** | The clip is a set share of the pool, so it grows with the coin | Sizes. **Cost minimum** derives a floor; this sets the whole clip from the pool. | The same SOL hurts a small pool and is lost in a large one, so a fixed clip is the wrong size nearly everywhere. | 1. The share is 0.3 % of vsol.<br>2. At vsol 60 the ticket buys 0.18 SOL.<br>3. At vsol 100 it buys 0.3 SOL. | share | open · ev 1.20 |

---

## Axis index

One fact, one row per slot. The index shows the repeats side by side so they read as one axis, not
as duplicates.

| axis | D | E | P | X | R |
| --- | --- | --- | --- | --- | --- |
| silence | Quiet birth | Back after a gap · Breaks the coin's silence | Tape busyness (silent now) | Silence of X | - |
| net flow / price path | - | After sellers | Net flow · Under its peak · Low holding | Flow turns · Low breaks | - |
| curve position | Life at a fixed age | - | Curve position | Take profit (headroom) | - |
| which ix structure | Create fingerprint | A class of structure buys · Count crossing | Classes present · Racers around · Structure record | Silence of X · Actor sells | - |
| a structure's own history | Group history test · Creator record | This structure's record here (E2) | Record of those present · Structure record | Actor leaves | - |
| who holds the supply | First-slot money · Creator's opening buy | A sell we buy into (seller kind) | P3, all rows | Actor sells | - |
| size of a print | - | Trigger size | Big sells lately | Sell into a buyer | - |
| fee | - | Fee paid | - | Fees fall | - |
| our own position | - | - | - | - | R, all rows |

---

## Dropped

Not ideas for this book. Listed so they are not re-invented; the reason is the bar they fail.

| idea | why it is out |
| --- | --- |
| a live link check, feed rank, king-of-the-hill, replies, livestream, the venue's safety panel | off-chain at the fire; the method is on-chain tape plus the document captured at birth (derive 2.0), and an HTTP call on the hot path is forbidden |
| joining rotating creator addresses into one operator | needs funder data; four chain-side link types all read at or below their null (ev 3.9) |
| "the coin doubles from here", "he follows this fire", "size answers this push", "state alone predicts nothing" | labels and findings, not rules: they read prints that have not landed. They live in the case files |
| following a copied wallet, selling to its copiers, a roster wallet printing | wallet identity or a follower graph; the ix structure is the only stable identity layer (strategy law 20) |
| wallet age, prints elsewhere, take-out record, buy against the wallet's own habit, rotation per wallet, launch buyer back, long-hold buyers' share | per-wallet history across the whole tape: global per-wallet state on the hot path, and the same identity objection |
| first print this UTC hour · best step-up of the moment across coins | no mechanism; a clock boundary means nothing to a bot, and cross-coin ranking is a cost on every token |
| alone in its slot, two structures in one slot, two lists agree, tools in one slot | the slot must close to know it, up to 400 ms after the trigger. Only what landed before the trigger survives, inside **Slot company** |
| a coarse launch bucket as the door | keeps only the length and last step of the create list, so unrelated launchers land in one group (ev 7, mid-tape instruments). It stays as a parameter of **Create fingerprint** |

---

## Old family codes

Case files written before this layout mark coverage by these codes. Read them through this map.

| old | now |
| --- | --- |
| D1 creation fingerprint | D1 launch build |
| D2 creator's document | D4 document |
| D3 this coin's life before the fire | D2, D3 for birth facts; P2 (a rise exists, under its peak, low holding) and P4 (classes present, distinct wallets) for facts read at the fire |
| D4 loss door | P3 who holds the supply |
| D5 off-chain | dropped |
| E1 a listed ix structure acts | E1 who printed; its trigger forms are E2 back after a gap and E4 slot company |
| E2 silence, then a spend | E2 back after a gap; E3 breaks the coin's silence |
| E3 an operator's plan is unfinished | E2 adding a leg, clip step-up, buying back under its sell |
| E4 a count crosses a line | E3 count crossing; P4 classes present; R for our own position facts |
| E5 after sellers | E3 after sellers, a sell we buy into |
| E6 this print | E4 how loud the trigger is; the wallet-history rows are dropped |
| E7 clock | E5 the seat |
| E8 graveyard | E6 graveyard |
| P1 curve position | P1 where the coin is |
| P2 windowed tape metrics | P2 what the tape did |
| P3 skin in | P3 who holds the supply |
| P4 ix makeup of the recent tape | P4 racers around |
| P5 tape state already true | P2, P4, P5; the 8dtx2t push-node findings are in mid-tape-rule-3 section 2b |
| X1 / X2 / X3 | unchanged |
