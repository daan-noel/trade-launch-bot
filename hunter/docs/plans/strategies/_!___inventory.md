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
- **The structure grid** (after the map) shows every ix-structure idea by what it reads and how the
  structures are grouped: the whole list together, each structure alone, or several landing in one
  slot. A cell names the row and the setting, and carries that setting's own status; the grouping
  stays a parameter of the row.
- **Nothing leaves silently.** Every idea name an earlier version of this file used is in
  **Old names** at the end, with the row, parameter or Dropped line that holds it now. A merge
  keeps the old name there; a new layout adds its names in the same edit.
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
   E1  who printed           a class of structure buys · the push · pack in one slot · buy wave · structure after structure · arrives from another coin
   E2  this structure here   first here · back after a gap · list silent, then back · adding a leg · clip step-up · buying back under its sell
   E3  the swing low         breaks the coin's silence · after sellers · a sell we buy into · capitulation · dip buy after a rise · flush stops · size into a dip · pullback depth
   E4  how loud the trigger  trigger size · price-step buy · since the last step-up · fee paid · slot company
   E5  graveyard             price-path turn · new-buyer acceleration · racer burst
P  PERMISSION
   P1  where the coin is     age · curve position · pool alive
   P2  what the tape did     net flow · tape busyness · big sells lately · a rise exists · under its peak · low holding · frenzy sells unabsorbed
   P3  who holds the supply  concentration · bundled share · public-app share · creator's position · actor still holds · crowd left · holders' PnL · operators round-tripped · top holders sold
   P4  which structures and  classes present · named structure present · structure mix · group flow · one structure's flow ·
       who have acted here   order of the last prints · record of those present · racers around · distinct wallets · sell-reactive buyers
   P5  the trigger's record  structure record on earlier days
   P6  the whole tape now    group live now · pushes elsewhere
X  EXIT
   X1  static                clock · take profit · stop · trail · bracket · gate then ride · abort · width from the coin's swing · exit picked at the buy
   X2  the tape stops        silence of X · flow turns · fees fall · buys fade · crowd under water
   X3  an actor acts         actor sells · actor leaves · sell into a buyer
   X4  the swing turns down  lower high · dip break · sudden dump · high goes stale · early pop fails · no breakout · low breaks
R  RE-ENTRY
S  SIZE
```

---

## Structure grid

Every ix-structure idea at a glance: what is read about the structures on this coin, and how they
are grouped. **Whole list**: every listed structure counted together (Axiom, Photon, 6Vo3245 and
the rest of one list). **Each structure**: one structure on its own line. **Landing together**:
several buys landing in one slot, like 2-4 Axiom buys at once. A cell is `**row** (setting) ·
status`.

| read | whole list | each structure | landing together |
| --- | --- | --- | --- |
| SOL flow | **Group flow** · open | **One structure's flow** · red | **Pack in one slot** (its SOL) · new |
| first appears | **First here** (the list's first print) · new | **First here** · red | **Pack in one slot** (a buyer new to the coin) · new |
| back after a gap | **List silent, then back** · new | **Back after a gap** · keep | **The push** · keep |
| how many land together | **Slot company** (listed structures in the slot) · open | **The push** (K from one structure) · keep | **Pack in one slot** (count, pure, back to back) · new |
| the run of slots | **Buy wave** (listed buys) · new | **Buy wave** (one structure) · new | **Buy wave** (packs in the wave) · new |
| bigger clip | **Clip step-up** (the list as one buyer) · new | **Clip step-up** · red | **Pack in one slot** (bigger than the last pack) · new |
| sells or still holds | **Operators round-tripped** (a listed set) · new | **Actor still holds** · red | **Pack in one slot** (a pack of sells) · new |
| order after another | **Order of the last prints** · new | **Structure after structure** · new | **Structure after structure** (the first print a pack) · new |
| earlier-day record | **Record of those present** · red | **Structure record** · keep | **Structure record** (per pack shape) · new |

---

## D - Door

Facts fixed at birth or inside the coin's first seconds. A door is optional (derive 7): empty is a
value, and a door earns its place only when it raises money and keeps the bars.

### D1 Launch build

The create transaction is an ix structure like any other, and the history of the coins it made is
the door's evidence. Refreshed daily; a group stays in the door one to two days.

| Name | Idea | Meaning | Why it matters | Example | Parameters | Status |
| --- | --- | --- | --- | --- | --- | --- |
| **Create fingerprint** | Group coins by the ix structure of the transaction that created them | Screens, by grouping. Coins born from one create structure are judged together. The unit is the instruction list; CU limit and CU price are knobs one launcher turns without changing its software, so they never make a group. | One launch tool or one operator sends the same create transaction every time, so its coins tend to behave alike: a good group's next coin is likely good too. A coarse key mixes unrelated launchers and describes none of them. | 1. 300 coins are created by one identical 5-instruction create transaction.<br>2. Two of them set different CU prices; they stay in the same group.<br>3. The group's history decides whether its next coin enters the door. | exact list vs coarse bucket (count + last instruction); max-cost and tip as a split inside a list | keep · ev 3.1 · 6ix · ev 7 (mid-tape instruments) · 3ix: the Create_v2, ATA, Buy list is four launch kinds split by the creation-slot SOL; two of them complete the curve at birth |
| **Group history test** | Yesterday the group launched enough coins and enough of them made the move we trade | Screens. It keeps the launchers whose coins repeat a move type; the move counted is a parameter, so one test serves a slow-wall rule and a hill rule. **Dump-factory exclusion** is the same test run to remove. | A launcher whose coins made the move yesterday tends to repeat it today, and a slow climb leaves time for our fill to ride it. A launch group decays week to week, so a short window predicts better than every earlier day. | 1. Yesterday the group launched 40 coins.<br>2. 3 of them (7.5 %) reached vsol 60 after age 60 s.<br>3. 7.5 % clears a 5 % cut: today's coins from this group are in the door. | move type (slow wall after the first minute, instant wall, hill, BIG rate at our seat), minimum coins, rate, window (one day, three, rolling) | keep (screen) · ev 3.1 · mt3 d8 · mt3 2b wide: inside his cell his picks come from groups with LOWER earlier BIG and DEAD rates (0.41-0.42): calm groups · the group's template (spike-and-collapse, steady rise) on earlier days: his picks come from quiet groups (0.42) · the steady-rise share of the group's tokens (earlier days + last 3 h) is monotone in money on both halves; with size and 60 s activity +0.65 % over twelve days, 84 % of draws (mt3 2b wide) · mt3 2b wide (relative Door): the group's big and spike share against the market lifts runners 6/6 days and lowers money · 3ix: the last five crew coins' outcome does not split came / nobody on both periods |
| **Dump-factory exclusion** | Remove groups whose coins nearly always spike once and fall back under where they started | Screens, by exclusion. **Group history test** keeps; this removes whole groups whatever their wall rate says. | A launcher that rugs by design does it again on the next coin, so removing the group removes losses we can see coming. | 1. A group made 30 coins yesterday.<br>2. All 30 spiked, then fell under their start inside a minute.<br>3. The group is out of the door, whatever its wall rate says. | spike and fall-back definition, rate | keep (exclude) · ev 3.1 · mt3 2b wide: the group's earlier DEAD rate <= 18.7 % is a layer of the best walk |
| **Launch flood** | The launch group is not launching many tokens right now: its launches in the last hour stay under a bar | Screens. The last hour of the group's creations, read at the fire. **Group history test** reads how its tokens did; this reads how fast it is launching them. | A tool that floods launches is farming the launch itself, and its tokens die once the farm moves on. | 1. The bar is 143 launches in the last hour.<br>2. A group launched 150 tokens in the last hour: its new coin is out of the door.<br>3. A group that launched 54 stays in. | window, bar | keep · mt3 2b wide (inside the steady rule DEAD 92 against runners 54 launches an hour; <= 143 takes the rule from +0.65 to +1.29 % over twelve days) · part of R2, red on the holdout · mt3 2b wide |
| **Steady template** | The launch group's tokens usually rose steadily through their first five minutes, on earlier days and over its last hours | Screens. Earlier days and a trailing window, never the coin itself. The dev's template read as a path, not as a fee or a preset. | A dev whose launches climb steadily runs a real launch; one whose launches spike and collapse is selling into its own crowd. | 1. 30 % of the group's last 300 tokens rose steadily through their first five minutes.<br>2. Across the market over 24 h the share is 15 %, so the group sits at 2x.<br>3. 2x clears a 1.68x cut: the group's next coin is in the door. | window, share cut | keep · mt3 2b wide (money monotone over the fifths on both halves; the Door of R2) · red on the holdout as R2's Door (-1.14 %, 1/9 days); a fixed share cut drifts with the market · mt3 2b wide(relative Door): over the group's last 300 tokens at 1.68x the market's 24 h share, above the same day's size-cell pool 6/6 and 6/6 days, not positive alone · keeps a fifth of his size-cell picks (22 a day), which lose on test (-0.18 %) · mt3 2b wide, combined: at 1.676x in all 12 conjunctions that pass the written fit rule; as one book fit -0.10 % 4/6, test +1.93 % 3/6 carried by one day · mt3 2b wide, R3 (21 days): a term of R3 (relative, >= 1.164x), frozen |
| **Creator record** | Inside one launch build, keep the coins of creators whose own earlier launches survived or moved often enough | Screens. **Group history test** judges the software; this judges the person sending it, inside one build. A creator address costs nothing to replace, so rotation is not recoverable from chain data and the door sees only creators who stand still. | One build carries thousands of unrelated creators whose coins do not behave alike, so the build's history describes none of them; the creator's own record separates them and keeps predicting forward. It selects coins, not an entry. | 1. Inside one launch build, a creator launched 8 coins in the last 7 days.<br>2. 5 of them reached 20 trades.<br>3. Today's coin from that creator is in the door; a creator with 1 of 8 is not. | window of days, minimum prior coins, survival or move definition (the wall, 20 trades), inside one build or on the creator wallet alone | keep (screen) · ev 3.8, 3.9 · mt3 3Xk2Eu |

### D2 Birth money

| Name | Idea | Meaning | Why it matters | Example | Parameters | Status |
| --- | --- | --- | --- | --- | --- | --- |
| **Creator's opening buy** | How much SOL the creator buys inside the create transaction, zero included | Screens, as a split of the launch group or a cut on its own. **First-slot money** reads everyone's buying in that slot; this reads the creator's alone. | What the creator puts in shows how serious the launch is. A creator with no stake loses nothing when the coin dies; frozen as a door alone that reads red, because the one green week was one launch machine. | 1. The create transaction also buys 0.8 SOL for the creator.<br>2. That falls in the 0.5-1 SOL band.<br>3. The coin is judged with the group's other 0.5-1 SOL coins; a create with no buy is its own band. | band; zero as its own band | open · ev 7 (no-initial-buy door) · 3ix: an opening buy that fills the curve completes it in the create transaction (half the group) |
| **First-slot money** | How much SOL, and how many wallets, bought in the coin's creation slot | Screens. **Creator's opening buy** is the creator's part of it. **Quiet birth** counts prints across the first slots, not SOL in the first. | Money in the first slot shows the launcher backs this coin instead of spraying it, and one launcher's quiet and bundled launches should not share one history. | 1. In the creation slot, 4 wallets buy 2.5 SOL in total.<br>2. The floor is 2 SOL.<br>3. 2.5 clears it: the coin passes. | SOL band, wallet count | keep (screen) · ev 3.2 · 3ix: the small creation-slot band loses on every exit; the rest is the door of the crew-sell sentence |

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
| **Documented** | The creator's document carries a website, a telegram or a description of real length | Screens. The loosest form needs one field; a stricter form needs two, anchored on the website or the channel. | A creator who writes something plans for the coin to live, and a coin that lives gives rises to catch. Two efforts mark a planned project rather than a field filled in to look complete. | 1. The document holds a website and a 120-character description.<br>2. Two fields are filled, one of them the website.<br>3. The coin passes. | which fields, how many, description length | keep · ev 3.3 · mt3 d8 · mt3 2b wide, his picks against their lookalikes: socials, description and keys 0.50-0.55 on the half of the coins with a document |
| **URI host** | Where the document is stored: a launchpad's own host is out, plain IPFS stays | Screens, by exclusion. It reads where the document lives, not what it says. | A launchpad host means the coin was made in a few clicks; a self-uploaded document took a decision. Read on omego's structure event it removes, it does not pick. | 1. The URI sits on a launchpad's own domain: the coin is skipped.<br>2. The URI sits on `ipfs.io`: the coin stays. | host list | open · ev 3.4g (omego E2f) |
| **Reused content** | The document's URI, site, telegram, name or symbol already appeared on an earlier coin | Screens, by exclusion. | A recycled document is a relaunch or a copycat, and those rarely live long enough to rise. | 1. Coin A launched yesterday with the telegram `t.me/abc`.<br>2. Coin B today carries the same link.<br>3. B is a copy: it is skipped. | which field, look-back | new · 3ix: name reuse flat on both periods |

---

## E - Event

The print we fire on, and only that. An event is graded by leftover existence behind the trigger at
our fill (derive 5.2), never by one exit's book. Seed racers and aggregator routes are never a
trigger: they react to someone else's buy, so their print is already behind the decision we want.

### E1 Who printed

Which ix structure sent the print. The structure is the actor, never the wallet and never the app
name: one app covers structures that act in opposite directions. The same axis read as a standing
state is P4, and across the whole tape P6.

| Name | Idea | Meaning | Why it matters | Example | Parameters | Status |
| --- | --- | --- | --- | --- | --- | --- |
| **A class of structure buys** | Fire when a buy lands from an ix structure of a named class | Fires. The class is the parameter and the list is the asset: a list built from one trader's reactions describes that trader, a list of public tools describes the crowd, a list of operator structures describes single owners. Each list is scored on the full tape, never on the trader's own coins. | A trader that makes money has already decided whose buy is worth following; the list is that decision, reusable by us. Retail arrives through public apps; one owner's bot is one decision; a pre-signed order was decided before the last print; an unusual instruction order marks one pusher's own bot. | 1. The list holds an Axiom buy structure and a nonce buyer.<br>2. The nonce buyer buys this coin.<br>3. It is on the list: we fire on that buy. | class: trader-derived list (8dtx's 155 structures), public tool, operator structure, nonce buyer, direct pump.fun buy by instruction name, campaign order (fee set before CU limit); the buyer new to the coin | keep · mt3 d8 · mt3 6.1 · ev 7 (campaign-break) · 3ix: one plain buy structure (one machine) is the outside money the launch crew waits for; firing on its fill and leaving on the crew's first sell pays until the machine stops, and any other tool's big buy is flat; mt3 2b wide, mid-tape event (21 days): without the quiet slots it covers 86 % of his openings, 180,693 a day; mid pool -2.26 % a trade in the book, no conjunction passes |
| **The push** | The coin is silent for a stretch, then one listed structure lands several buys in one slot | Fires. Silence and the count work together: neither half fires alone. **Breaks the coin's silence** (E3) needs one buy of size from anyone; this needs one listed structure landing several buys in one slot. It is 8dtx2t's event. | A listed trader breaking a quiet tape is deciding now, not continuing something already priced, and several buys in one slot say it means it. | 1. The coin is silent for 10 slots.<br>2. One Axiom structure lands 2 buys in one slot.<br>3. Both halves hold: we fire. | slots of silence, K buys in the slot, which list (trader-derived, tools, operators), size floor, one floor per structure | keep · mt3 d8 |
| **Pack in one slot** | Several buys from the listed structures land on the coin together, in one slot | Fires, on the print that completes the pack. **The push** needs the coin silent first and one structure; this reads any pack, from one structure or several. The engine spells it `m_burst_slot`. | A bot, or a group of bots, sending 2-4 buys into one slot is spending in one decision, which is harder to fake than one print; a pack all from the list and back to back in the block is one operator's order. | 1. In one slot two Axiom buys and one Photon buy land, back to back.<br>2. All three are on the list: a pure pack of 3.<br>3. The third completes it: we fire. | K buys, the pack's SOL, distinct structures in it, share from the list (a pure pack), back to back in the block, a buyer new to the coin in it, bigger than the coin's last pack, buys or sells, one structure or the whole list | new |
| **Buy wave** | A run of back-to-back slots with buys, after empty slots, and how many listed buys it holds | Fires, on the print where the wave's listed buys cross K. **Pack in one slot** reads one slot; this reads the run of slots it belongs to. The engine spells it `m_burst_wave`. | A burst lands across a few slots, not one: the empty slots before it say the coin was quiet, and the listed buys inside it say who started it. | 1. The coin has no buy for 4 slots.<br>2. Buys land in three back-to-back slots, two of them Axiom buys.<br>3. The listed count crosses 2: we fire. | empty slots before the wave, listed buys in it (K), its wallets, all buyers new to the coin, a tip band repeating, one structure or the whole list, packs inside the wave | new · unscored: needs a hit-rate read (derive 5.0) |
| **Structure after structure** | A listed structure buys right after another named structure printed on this coin, a few slots earlier | Fires, on the second print of the pair. **Order of the last prints** (P4) reads the same sequence as a filter on any trigger. Two structures in one slot needs the slot to close and is dropped; this pair spans slots, so it is known at the fill. | Bots that trade together land in an order: one operator's program answering a retail app's buy is a pattern that repeats, and the second print is the decision to follow. | 1. An Axiom buy lands on the coin.<br>2. Two slots later a Token-2022 program buys.<br>3. That pair is on the list: we fire on the second buy. | the pair (which first, which second), side of the first print, gap in slots, pairs picked on earlier days only; the first print a pack | new · unscored: needs a hit-rate read (derive 5.0) |
| **Arrives from another coin** | A structure that printed on a different coin seconds ago now prints here | Fires. **Actor leaves** (X3) is the same move read as an exit. | A bot moving from one coin to another is moving its money, and the money arrives here. | 1. A structure bought coin A 2 s ago.<br>2. It buys this coin now.<br>3. We fire. | how long ago it printed elsewhere, side of that print, class of the structure | red · ev 7 (C13) |

### E2 This structure's record on this coin

One structure's own history here is what turns "a buy landed" into a decision. One fire per
(coin, structure) where the row says so.

| Name | Idea | Meaning | Why it matters | Example | Parameters | Status |
| --- | --- | --- | --- | --- | --- | --- |
| **First here** | A structure's first print on this coin | Fires. **Back after a gap** is the same structure returning; this is it arriving. | A first buy is a fresh decision about this coin, where a return can be a top-up of one already made. | 1. A nonce buyer has never printed on this coin.<br>2. It buys now.<br>3. We fire. | listed structures only, or any; the whole list's first print on the coin | red · ev 7 (C16) · mt3 6.1 |
| **Back after a gap** | A structure buys again after a stretch of its own silence on this coin | Fires. **Breaks the coin's silence** (E3) reads the whole coin's silence; this reads one structure's. The gap can be cut on one side or both, and the coin may be required to have kept printing meanwhile. | A structure that pauses and comes back with size is making a new decision, not continuing one. Too short a gap is a bot continuing, already in the price; too long is a bot that has forgotten the coin. | 1. An Axiom structure last printed on this coin 25 slots ago.<br>2. The coin kept printing meanwhile, and the structure now buys 0.6 SOL.<br>3. We fire. | gap length (slots), band or one-sided, coin kept printing meanwhile, size floor, K buys in the returning slot, class of the structure (tool, router, operator, any), first return only; read per wallet on this coin (how long ago, net SOL here) | keep · ev 6.4 · mt3 d8 · ev 7 (C5, C12) · mt3 6.1; mt3 2b wide, event logics (21 days): the short-gap side (same structure inside 10 - 30 s) is red, see **Same bot again** |
| **List silent, then back** | Every listed structure has been silent on the coin for a stretch, then one of them buys | Fires. **Back after a gap** reads one structure's silence and **Breaks the coin's silence** (E3) the whole coin's; this reads the list's, while other prints may keep landing. | When the bots a trader follows all step away and one comes back, the return is a fresh decision by the group he watches, even while retail keeps printing. | 1. No listed structure has printed on the coin for 30 slots; retail buys kept landing.<br>2. A Photon buy lands.<br>3. We fire. | slots of the list's silence, which list, size floor, K buys on the return | new |
| **Adding a leg** | A structure that usually buys a coin in several bursts makes its next burst here | Fires. **Clip step-up** reads the size of the next buy; this reads only that the plan has another leg. | A bot that buys in legs still has SOL to spend here, and its own next legs push the price we bought at. | 1. A bot buys most of its coins in two or more bursts.<br>2. Its first burst here lands at age 40 s.<br>3. Its second lands at age 60 s: we fire on it, ahead of any third. | first leg or later leg; share of its coins bought in legs; how much of its usual spend is still unspent here | red · ev 7 (C9, C11) · mt3 6.1 |
| **Same bot again** | A listed structure buys a coin it already bought a few seconds ago; the wallet may differ | Fires, on the second buy. **Back after a gap** needs a long silence first; this is the short gap. | A runner has started: a volume maker, the dev or a trader's bot keeps buying, and the next buyers ride its flow. | 1. A bot pushes 1 SOL.<br>2. 6 s later the same structure buys 0.5 SOL.<br>3. The runner is on: we fire. | window, repeats, same wallet or any, after a push or any | red · mt3 2b wide, event logics (21 days): money a trade mid -3.69 against -1.99 % without it, young -7.01 against -3.93 %, every week |
| **Bot rhythm** | The same structure buys this coin at a steady beat | Fires. **Same bot again** reads one repeat; this reads a schedule. | A scheduled volume maker keeps going for a while, and its volume draws buyers. | 1. One structure buys every 3 - 4 s.<br>2. Its last three gaps are 3.1, 3.8, 3.4 s.<br>3. A schedule: we fire. | gaps, their spread, count | red · mt3 2b wide, event logics (21 days): last three gaps <= 10 s with CV <= 0.5: money a trade mid -4.38 against -3.01 % |
| **Bot churns** | The structure that buys has also sold this coin inside the window | Filters. **Actor sells** (X3) is the same actor after our fill. | A bot that buys and sells the same coin is making volume, not taking a position; its buys carry no new money. | 1. A structure sold 0.4 SOL 12 s ago.<br>2. It buys 0.5 SOL now.<br>3. It churns: skip. | window | keep (as an exclusion) · mt3 2b wide, event logics (21 days): money a trade mid -3.98 against -2.75 %, young -6.67 against -6.20 % |
| **Clip step-up** | A structure buys more than its own last buy on this coin | Fires. **Trigger size** (E4) compares the buy with the coin; this compares it with the structure's own last buy here. | A setup that comes back with a bigger buy is adding to a position, not closing one. | 1. A structure buys 0.3 SOL here.<br>2. Later the same structure buys 0.5 SOL.<br>3. The second buy is bigger than its last: we fire on it. | step size (ratio), plus the price step and the buyer being new to the coin as extra terms; the whole list read as one buyer | red · ev 7 (rule 3b) · mt3 6.1; mt3 2b wide, event logics (21 days): money mid -3.26 against -2.97 % |
| **Buying back under its sell** | The coin is quiet under a price this structure sold at, it has not bought back, and it usually does | Fires. The only E2 row read off the structure's sells. | A bot that sells high and buys back lower is likely to buy back here, and its buy lifts the price. | 1. A structure sold here at vsol 70 and has not bought back.<br>2. The coin sits quiet at vsol 60.<br>3. It rebuys after selling on most of its coins: we fire. | class (tools vs operators), how often it rebuys elsewhere, how far under its sell | red · ev 7 (C10, C9) |

### E3 The swing low: who acts at the dip

The swing low is the largest prize measured on this tape and the hardest to reach: a fire at the
true low of a swing pays, and the same fire once the price has already risen off that low loses
more than the whole edge (ev 3.8, the trough against a confirmed rise). So a row here fires on
**who is acting** at the dip or at the silence - a seller giving up, a buyer absorbing, a quiet
tape broken - never on what the chart already did. A price-path fact standing at the print is P2;
the chart's shape alone is the graveyard (E5).

| Name | Idea | Meaning | Why it matters | Example | Parameters | Status |
| --- | --- | --- | --- | --- | --- | --- |
| **Breaks the coin's silence** | The first buy above a floor after the whole coin has been silent | Fires. **Back after a gap** (E2) reads one structure's silence; this reads the coin's. **Coin silent now** (P2) is the same fact standing, as a filter. | Size landing on a quiet tape is a decision, and it can wake the buyers who left. | 1. No print lands on the coin for 10 slots, about 4 s.<br>2. A 0.6 SOL buy lands.<br>3. We fire. | slots of silence, size floor, K buys in the breaking slot, the breaker's class (listed, new to the coin, any) | red · ev 7 (C5) · mt3 d8 · mt3 6.1 |
| **After sellers** | The first buy after selling | Fires. The buy is the trigger and the selling is its context; **Net flow** (P2) is the windowed form of the same fact, as a filter. | Buyers come back once a run of sellers is spent, and the first of them buys the low; a buy that takes a seller's tokens is absorbing supply, not chasing. | 1. Three public sells land in a row.<br>2. A 0.5 SOL buy that is not a seed racer lands next.<br>3. We fire on that buy. | how many sells in a row, a known structure as the seller, a sell in the same slot, size floor on the buy, not a seed racer | red · ev 7 (C14) · mt3 6.1 |
| **A sell we buy into** | A public sell of size by a seller of a named kind; we buy the dip it makes | Fires. Rule 1's event. The seller's kind is the parameter; the young-coin form is the same print without **Age** (P1). | The dip a scheduled or forced seller makes is a discount rather than news: a flipper's sell is an exit, not an opinion; a seller under water is giving up; a seller who is all out cannot hit us twice; the frenzy's next buyers absorb the drop. | 1. The coin made a new high 10 s ago.<br>2. A wallet that bought 20 s ago sells 1.5 SOL.<br>3. We buy the dip that sell made. | seller kind (bought within 30 s, under its cost, all out, the pusher, a top holder, a listed structure), size floor, inside a frenzy (new high in 20 s), the coin's first big sell | keep · ev 1.22 (rule 1's event) · ev 1.27 · case U5 · mt2 6.2 · mt3 5.2 88887Q · ev 1.16 · mt3 d8 |
| **Capitulation** | A public sell of size from a seller under its cost, inside a burst of selling, into a price already falling | Fires. **A sell we buy into** fires on one seller of a named kind; this needs the selling to be a panic: a loser, inside a burst, into a fall. | Panic overshoots what the coin is worth, and the snap back is the move we want to be in. | 1. The price is down 10 % over 10 s.<br>2. A wallet 4 % under its cost sells 1 SOL inside a burst of sells.<br>3. We buy the dip it makes. | size floor, the seller's loss, burst length, the fall before it (window, depth), tape busyness | red · ev 1.16 |
| **Dip buy after a rise** | A real buy of size lands while the price is still down after the coin's last hill | Fires, never on a seed racer. **Flush stops** waits for the fall to end; this fires while the coin is still down. **A rise exists** (P2) is the fact it stands on. | Someone buying the pullback of a coin that just proved it can rise is buying the same coin cheaper, and the buyer, not the chart, is the signal. | 1. The coin makes a +60 % hill.<br>2. It pulls back 20 %.<br>3. A 0.6 SOL Axiom buy lands: we fire. | size floor, the buyer's class, depth of the pullback, hill size | red · ev 7 (machine print in the dip) |
| **Flush stops** | The first real buy after a flush, once vsol has made no new low for a stretch | Fires, once per flush, never on a seed racer. **Dip buy after a rise** fires while the price may still fall; this waits for the low to hold. **Low holding** (P2) is the standing form. | A low that holds means the sellers are done, so the buy we follow is not catching a knife. | 1. The price is 30 % under its peak.<br>2. Ten slots pass with no new low.<br>3. A 0.7 SOL buy lands: we fire. | slots with no new low, flush depth, size floor, the buyer's class | red · ev 7 (C9, S12) |
| **Size into a dip** | A buy of size that opens a run while the price is falling | Fires. **Dip buy after a rise** needs an earlier hill; this needs the buy to open a run. | Size arriving into a falling price looks like conviction and the start of the recovery. Dead at our seat: the buyers who answer it land inside about 50 ms, so the dip is gone before our fill. | 1. The price is down 5 % over 10 s.<br>2. A 1 SOL buy opens a run.<br>3. The row would fire here; it is dead. | - | dead · ev 7 (AbQcLH burst start) |
| **Pullback depth** | Buy once the price has given back a set share of its last swing, a set distance off the low | Fires, on the chart alone. **Price-path turn** (E5) reads the same shape at the turn and is dead; the depth stays a P2 term (**Under its peak**). | A measured pullback looks like a cheap entry into a coin that is still rising; on price alone it waits for the confirmation that costs the edge. | 1. The price is 40 % under its swing high.<br>2. It is 5 % off the low.<br>3. We fire. | depth off the high, distance off the low, at the knife, at the turn, after stillness | red · ev 7 (hot-tape price-path) |

### E4 How loud the trigger is

Facts of the one print we answer, knowable at our fill.

| Name | Idea | Meaning | Why it matters | Example | Parameters | Status |
| --- | --- | --- | --- | --- | --- | --- |
| **Trigger size** | How big the buy is against a denominator: a band, not a floor | Fires, or is a term of another E row. The denominator is the parameter; the same SOL means something on a quiet coin and nothing on a busy one. **Price-step buy** reads what it did to the price. A very large buy has already moved the price before our fill lands, so the fact turns around above about 2 SOL. | Size is conviction, the simplest form of it to read; size against the structure's own habit is a change in behaviour; the price step says whether the actor is placing or chasing. | 1. The coin's last 20 prints have a 75th percentile of 0.2 SOL.<br>2. This buy is 0.8 SOL, 4x that.<br>3. It sits inside the band: the fire stands. At 3 SOL it would not. | denominator: absolute SOL; the coin's last public buy; the 75th percentile of the last 20 prints; the coin's buying rate over 30 slots; the structure's usual clip elsewhere; floor per structure or one floor; band; read past a set age with **Age** (P1) | keep (term) · mt3 d8 · ev 7 (G1) · mt3 6.1 · ev 3.4g (omego E2b) · mt3 2b wide: 0.80 on his pick, 0.77 within the coin; the strongest fact and never a rule · mt3 2b wide, matched: 0.77 against pushes he skips on the same coin, 0.80 against coins of his launch group in the same minute; as a sentence with the other matched facts it keeps his picks and 35x as many losers · mt3 2b wide, R3 (21 days): a term of R3 (>= 1.5x the coin's mean buy over 30 s), frozen · 3ix: a big early outsider buy of any tool is flat once the one machine is removed |
| **Price-step buy** | How far this one print moves the price on its own: its price step, not its SOL | Fires, or is a term. **Trigger size** reads the SOL against a denominator; this reads what that SOL did to the pool, which depends on how deep the pool is. It is the up side of the swing: a buyer paying up. | On a thin pool a buy that jumps the price is someone paying up; a print that barely moves it is placing, and what we pay is close to what it paid. Which side is good is the parameter. | 1. vsol is 50.<br>2. A 1.1 SOL buy takes it to 51.1.<br>3. The price moves (51.1 / 50)^2 - 1 = +4.4 %: over the cut, we fire. | the price step (`mvk`), band, floor or cap, big side or small side, the buyer's class | open · ev 3.4g (omego E2b, E2d, E2e) · mt3 6.1 |
| **Since the last step-up** | This step-up read against the coin's previous one: how long ago it was, how many came before, and how much louder this one is | Fires, or is a term. **Clip step-up** (E2) compares one structure's buys; this compares the coin's step-ups whoever made them. | A trader can pick on a change rather than a level: two step-ups close together are one operator adding in quick steps, and a louder one than last time is an escalation. | 1. The last step-up came 1 s ago with 0.4 SOL bought in the 2 s before it.<br>2. This one comes with 1.0 SOL.<br>3. It is louder and fresh: the fire stands. | gap in seconds (`clip_gap`), count so far (`clip_n`), change in step, buy SOL, prints or price move since the last one | red · derive 6.1 · mt3 6.1 |
| **Fee paid** | The print pays a priority and tip fee at the high end | Fires, or is a term. Compared with the tape, or with what this structure usually pays. | Paying extra to land sooner is urgency; urgent for that bot in particular is a change, not its habit. | 1. Most prints on the tape tip 0.0005 SOL.<br>2. This one tips 0.002 SOL.<br>3. It sits in the top quarter: the fire stands. | vs the tape's distribution; vs the structure's own 20 or more earlier prints | keep (term) · ev 7 (zigzag turn) · mt3 6.1 · mt3 2b wide: against the coin's own recent buyers 0.58 on his pick · mt3 2b wide, matched: a fee over the coin's last ten buyers' 0.58 on the same coin; no conjunction passes |
| **Slot company** | How many prints, distinct structures and tipped prints landed in the trigger's slot before it | Fires, or is a term. Only what landed before the trigger is knowable at our fill; "alone in its slot" needs the slot to close and is dropped. | Several buys queued into one block are a decision many actors made at once; several bots paying a tip to race the slot is intent rather than appearance. | 1. Before the trigger, two other buys landed in its slot.<br>2. Both paid a Jito tip.<br>3. Two tipped prints already in the slot: the fire stands. | count of prints, count of distinct structures, count of tipped prints, two listed structures in the slot, position in the block (tx index) | open · ev 3.4g (omego E1e) · mt3 d8 · mt3 6.1 · mt3 2b wide: counted without our own prints; tipped ahead 0.545, prints ahead 2.4x at >= 2 · mt3 2b wide, conjunctions inside the relative Door: the trigger first in its slot joins the frozen conjunction (+0.25 % fit, -0.28 % seen test): red |
| **The slot after** | Public buyers keep landing in the slot right after the push | Fires, or is a term. **Slot company** reads the trigger's slot before it; this reads the next slot, so it needs an entry that waits one slot. | A push that the market answers at once has buyers behind it; one that lands alone is a bot talking to itself. | 1. A bot pushes 1 SOL in slot 100.<br>2. In slot 101 three public wallets buy.<br>3. We buy after slot 101. | slots to wait, buys, SOL, wallets new to the coin | new · mt3 2b wide, funnel: his picks against their lookalikes 0.60, the lookalikes' runners against their FLAT and DEAD 0.58; mt3 2b wide, wait one slot as a real entry: -3.66 % against -2.53 % on the funnel pool, conditioned on the next slot -3.2 to -4.6 %: red |

### E5 Graveyard

Dead by mechanism. Do not rebuild these as the event.

| Name | Idea | Meaning | Why it matters | Example | Parameters | Status |
| --- | --- | --- | --- | --- | --- | --- |
| **Price-path turn** | The first print after the price turns up from a low | Fires. It reads the shape of the chart and nothing else. **Pullback depth** (E3) is the depth form, red. | From the price alone a turn and a falling knife look the same: the detector is right 5.6 % of the time where it needs about 45 %. The price path is a P fact (P2), never a trigger. | 1. Price falls 30 %.<br>2. It bounces 15 % off the low.<br>3. The row would fire here; it is dead. | - | dead · ev 7 (hot-tape price-path) |
| **New-buyer acceleration** | Several wallets buy the coin for the first time inside a few slots | Fires. **Tape busyness** (P2) is the same count standing. | They are the move, so by the time the count rises the price has paid for it. | 1. Five new buyers land inside 10 slots.<br>2. The row would fire here; it is dead. | - | dead |
| **Racer burst** | Seed racers pile into a coin | Fires. | A seed racer only reacts to someone else's print, so it confirms a decision already in the price. | 1. The coin is silent 10 slots.<br>2. Four seed racers buy.<br>3. The row would fire here; it is dead. | - | dead |

---

## P - Permission

State standing true when the print lands. P does not explain why a move happens; it puts the fire
where a move can pay and a loss is bounded, and it cuts before occupancy so a refused fire frees
the coin (derive 9). Every row names its time basis.

### P1 Where the coin is

| Name | Idea | Meaning | Why it matters | Example | Parameters | Status |
| --- | --- | --- | --- | --- | --- | --- |
| **Age** | The coin's age sits inside a band when the print lands | Filters. At the print. An E row never carries an age word; it comes from here. | A rule is only tested inside the part of a coin's life it was read on; a frenzy on a young coin dies with the seller who started it and on an old one the crowd absorbs it; sniper-eating rugs die inside 10 s. | 1. The band is age 158 s and older.<br>2. The coin is 200 s old when the print lands: the fire is allowed.<br>3. At 30 s old it is not. | band (booked on rule 1: age >= 158 s) | keep · ev 1.22 · mt2 9.1 · mt3 2b wide, inside the relative Door: a push on a coin 10-38 s old, with a busy tape, carries the pairs under the fit winner (fit +1.9 to +2.0 %, 4/6 days); unconfirmed · mt3 2b wide, R3 (21 days): a term of R3 (<= 37.9 s), frozen |
| **Curve position** | vsol sits inside a band when the print lands | Filters. At the print. On the curve one number is price, market cap, progress, real reserve (vsol - 30) and headroom to the wall at 115, so every spelling is this row. The upper cut can be set from the target: a +20 % target needs vsol under about 105, a +100 % target vsol 81 or under. | Where on the curve the fire sits decides both how far the coin can rise and how far it can fall; a target above the wall cannot be reached at any price. | 1. The band is vsol 45-100.<br>2. The print lands at vsol 60: the fire is allowed.<br>3. At vsol 105 a +20 % target no longer fits under the wall: refused. | band, or headroom for the target (booked on rule 1: vsol <= 100) | keep · ev 1.20 · ev 6.4 · mt2 9.1 |
| **Pool alive** | The coin still holds SOL and is still printing | Filters. At the print. | A pool nobody trades has no buyer to sell our tokens to, whatever the price says. | 1. The last print landed 3 s ago.<br>2. The reserve is 12 SOL.<br>3. The coin is alive: the fire is allowed. | last-print gap, reserve floor | new |

### P2 What the tape did

Net SOL over a window, the price change over it and the vsol change over it are one number on a
bonding curve, so windowed flow is the price path. What stays separate is count against SOL, and
public against all.

| Name | Idea | Meaning | Why it matters | Example | Parameters | Status |
| --- | --- | --- | --- | --- | --- | --- |
| **Net flow** | Buy minus sell over the window before the print, with a sign: which side is good is the parameter | Filters. Window. Read as SOL it is the price move; read as a count of prints it is not. On the push event the sold-off side is the one door term that survives both folds; on rule 1's pool buying leading is what pays. | A coin where buying leads is not decaying, so our fill is not the last one in; size stepping into a tape everyone else is leaving is absorption. | 1. In the last 30 s the coin is bought 4 SOL and sold 8.<br>2. Net is -4 SOL.<br>3. On a rule that wants the sold-off side, the fire is allowed. | window (seconds, slots, prints), SOL or count, public or all, sign and cut, the sell-side shape (down over 10 s); the move before the print: up hard over 10 s to 5 min (already running, the up-move portrait), fell hard inside a minute (a fresh fall) | keep · ev 1.3 · mt3 d8 (both signs) |
| **Tape busyness** | How many prints, wallets or structures landed in the window, or none at all | Filters. Window, or right now. Few prints, many first-time buyers still arriving, many distinct structures with none dominating, and the coin silent right now are all readings of it. | A busy tape is a pile-in that has already moved the price; new buyers are who pays for the next rise; a print on a quiet tape is a fresh decision. | 1. The cap is 25 prints in 5 s.<br>2. 12 prints land in the last 5 s.<br>3. Under the cap: the fire is allowed. | what is counted (prints, wallets, first-time buyers, returning buyers' share of the SOL, mean buy size), window, silent now, the race (share of buys landing within our latency of the one before) | keep (term) · ev 1.11 · ev 7 (rule 3b) · mt3 6.1 · mt3 2b wide, inside the relative Door: 12 or more structures in 60 s alone lifts the Door pool 6/6 fit days (+0.95 points), -0.13 % fit · mt3 2b wide, conjunctions inside the relative Door: with the trigger first in its slot and sells in each of 30, 10, 5 s, the frozen conjunction: +0.25 % fit 4/6, -0.28 % seen test: red |
| **Calm before the push** | On a coin past its launch minute, few public buys land in the minute before the push | Filters. **Tape busyness** counts the crowd as a plus early in a coin's life; mid-tape the same count is a minus. | Once a coin is minutes old, a busy last minute is people chasing a move that already happened; a push into a quiet coin is the first money in a new leg. | 1. A coin is 4 minutes old.<br>2. The last 60 s held 5 public buys, under the bar.<br>3. A bot pushes 1 SOL: we take it. | window, bar | new · mt3 2b wide, mid-tape (21 days): busy last minute -2.0 / -2.5 / -2.3 points by week; in no passing conjunction; mt3 2b wide, mid-tape event (21 days): on listed buys into a busy tape -4.1 / -4.8 / -5.2 points by week (calm wins), his AUC 0.23 - 0.28 |
| **Time off the peak** | The coin's best price is long past when the push lands | Filters. **High goes stale** reads the same age after our fill; this reads it before. | A coin still near a fresh high is being chased; one that topped long ago has cleared its sellers, and a new push starts a new leg. | 1. The coin's best price was 4 minutes ago.<br>2. It is 40 % under it.<br>3. A bot pushes: the peak is old enough. | seconds since the best, depth under it | new · mt3 2b wide, mid-tape (21 days): +2.3 / +2.1 / +1.8 points by week; red in conjunction; mt3 2b wide, mid-tape event (21 days): +1.7 / +1.0 / +0.5 points by week on listed buys; red in conjunction |
| **Previous push paid** | The last push on this coin, already closed, made money | Filters, for re-entry. It reads our own exit's result on the same coin. | A coin whose last push paid has buyers who answer pushes; one whose last push lost has taught them not to. | 1. A push on this coin 3 minutes ago was sold at +8 %.<br>2. A new push lands.<br>3. The last one paid: we take it. | the result bar, how long ago | new · mt3 2b wide, mid-tape (21 days): +1.1 / +2.8 / +1.4 points by week; red in conjunction |
| **Big sells lately** | How many public sells above a size landed in the window | Filters. Window. | A large holder on the way out sells more than once, and the rest of that exit would land on our position. | 1. The cap is 4 sells of 1 SOL or more in 30 s.<br>2. Two such sells landed in the last 30 s.<br>3. Under the cap: the fire is allowed. | size (down to any sell), window (down to the print just before), cap | red · ev 7 (rule 3b) |
| **A rise exists** | The coin has completed at least one hill | Filters. Lifetime. **Under its peak** reads where the price sits after it. | A coin that has risen once has buyers who can lift it, which is the whole bet. | 1. Earlier, vsol went from 40 to 50.<br>2. Price rose (50 / 40)^2 - 1 = +56 %: a hill.<br>3. The coin has made a hill: the fire is allowed. | hill count, hill size, second hill | keep · ev 6.4 |
| **Under its peak** | How far the price sits under its peak, and for how long | Filters. Lifetime or window. Read at both ends: at the high the move is spent, far under it the coin is falling; a coin far under for minutes has lost its buyers. A hill whose fall is fully closed is a recovered flush. | The peak proves buyers exist, and the fall back leaves the room they need to do it again. | 1. The coin's peak price is 1.00.<br>2. The price is 0.75, 25 % under it, for 40 s.<br>3. Inside the depth and duration band: the fire is allowed. | depth, duration, peak basis (lifetime, last hill, the window's high), recovered or not | keep (term) · mt3 d8 (omego pullback red) |
| **Low holding** | vsol has made no new low for a stretch, or a named floor never broke | Filters. Window. **Low breaks** (X2) is the same test after we are in. | A low that holds means the sellers did not take control, so the reason we fired still stands. | 1. The last low is vsol 52.<br>2. For 12 slots vsol has not gone under 52.<br>3. The low held: the fire is allowed. | slots without a new low, floor | new |
| **Frenzy sells unabsorbed** | Of this coin's earlier big sells inside a frenzy, how many were bought back: more is better | Filters, by exclusion. Lifetime. A sell is bought back when the coin makes a new high soon after it (**absorption**). **Sell-reactive buyers** (P4) counts who did the buying back. | If this coin's dips do not recover, the dip we buy may not recover either. | 1. The coin had 6 sells of 1 SOL or more inside a frenzy.<br>2. Only 1 was followed by a new high within 15 s.<br>3. 1 of 6 is under the cut: the coin is out. | sell size, the new-high window, share cut | red · case 34 |

### P3 Who holds the supply

Read inside a vsol band: below vsol 42.43 a -50 % move is impossible, so vsol alone separates
nothing here. The loss door of rule 1 lives in this family.

| Name | Idea | Meaning | Why it matters | Example | Parameters | Status |
| --- | --- | --- | --- | --- | --- | --- |
| **Concentration** | The biggest one, or biggest ten, holders' share of live supply: smaller is better | Filters. At the print. **Bundled share** reads a group that bought together; this reads size alone. | One wallet with a large share can drop the price by itself, and no one else has to agree. | 1. The top wallet holds 15 % of live supply.<br>2. The cap is 10 %.<br>3. Over it: the fire is refused. | top-1 or top-10, cap | red · case L4 · ev 3.7 · mt3 2b wide, at the pause: the top ten's share 0.45-0.50 · mt3 2b wide, his picks against their lookalikes: the top ten's share 0.84 on his picks against 0.98 (0.72 fit, 0.66 test); at most 0.95 matches his bucket mix and loses -2.84 % |
| **Fresh holders** | Holders first seen on the tape in the last few minutes hold a share of the float | Filters, either way. **Concentration** reads how big the biggest bags are; this reads how new their owners are. | Wallets made minutes ago to buy one coin are a launcher's own supply or a buyer who just arrived; which one it is decides who sells next. | 1. Of 40 holders, 6 were first seen in the last 10 minutes.<br>2. They hold 12 % of the float.<br>3. The share sits over the bar. | minutes, share of holders or of the float | new · mt3 2b wide, his picks against their lookalikes: 0.61 fit, 0.59 test; money untested · mt3 2b wide, combined: in 10 of the 12 conjunctions that pass the written fit rule; the winner is red on test (-2.13 %) · mt3 2b wide, R3 (21 days): a term of R3 (>= 3.4 % of the float), frozen; +4.47 % a trade, 16/21 days |
| **Pusher new to the coin** | The wallet that sent the trigger has never printed on this coin before | Fires, or is a term. **Actor still holds** reads its bag; this reads whether the push is its first touch. | A wallet's first buy on a coin is a fresh decision to enter, while a wallet adding to a bag is managing a position it already has. | 1. A bot pushes 1 SOL.<br>2. Its wallet has no earlier print on this coin.<br>3. The push is a first touch. | - | new · mt3 2b wide, his picks against their lookalikes: 0.58 fit and test; money untested; mt3 2b wide, mid-tape event (21 days): the trigger buyer new to the coin +0.9 / +2.4 / +3.1 points by week, his AUC 0.67 - 0.70; in no passing conjunction |
| **Bundled share** | Wallets whose first buy landed in a same-slot group hold a small share of live supply | Filters. At the print. The creation slot alone reads 0 past age 158 s, so rule 1 reads any slot. | A group that bought on one trigger sells on one trigger, and the price never warns first. | 1. At age 90 s, 4 wallets bought in one slot through one ix structure.<br>2. They hold 35 % of live supply.<br>3. 35 % is over 28.57 %: the fire is refused. | creation slot vs any slot, group size (3 or more on one structure), cap (booked on rule 1: <= 28.57 %) | keep · ev 3.7 · ev 1.28 · mt3 2b wide: the first slot's buyers' remaining share 0.47 on his pick |
| **Public-app share** | Holders whose first buy went through a public app hold most of the live supply | Filters. At the print. A holder is classed once, at its first buy. | The supply outside the public apps is mostly one bot swarm that sells in a single slot; a swarm's wallets never come back, which is what the two-buys half of the public-app test catches. | 1. Holders whose first buy came through a public app hold 800 M of 1,000 M live tokens.<br>2. 80 % clears 70 %.<br>3. The fire is allowed. | floor (booked on rule 1: >= 70 %) | keep · ev 1.28 |
| **Creator's position** | What the creator holds, whether he has sold, whether he has bought again | Filters. At the print, or the last slots for the rebuy. A survival term: it keeps coins alive longer and raises the loss rate, because it keeps us in coins that fall slowly. | A creator still holding has not rugged yet; a small bag has little to dump; a creator adding is betting on the coin. | 1. The creator bought 1 SOL at launch.<br>2. He has sold nothing since.<br>3. The fire is allowed. | share cap, has not sold, rebought in the last N slots | keep · ev 3.4 · ev 3.7 · mt3 2b wide: the birth buyer's bag 0.52 on his pick, its selling as an exit +1.4 purpose on his picks only |
| **Actor still holds** | The structure that lifted the last hill, or the structure we fire on, has not sold | Filters. At the print. **Actor sells** (X3) is the same actor after we are in. On the pusher it reads inverted: its selling is a better sign than its holding. | Whoever paid most to lift the price has the most reason to want it higher; the one we follow still wanting the price up is the reason we buy. | 1. The pusher bought 3 SOL during the last hill.<br>2. It has sold nothing.<br>3. The fire is allowed. | which actor (pusher, firing structure) | red · ev 7 (the extraction tell) · mt3 2b wide: the trigger's structure already holding 0.46 on his pick · mt3 2b wide, at the pause: the trigger's structure still holding 0.48-0.50 · 3ix: the crew holding plus outside money since birth as a later-entry gate: every later age books below the first-second entry |
| **Crowd left** | The buyers of the last hill hold less than half of what they bought | Filters. At the print. The opposite reading, buyers still in, is refuted and inverted: still-held supply is the next sellers. | Their selling is already spent, so it will not land on top of our rise. | 1. The last hill's buyers bought 10 M tokens.<br>2. They hold 4 M, 40 % of it.<br>3. Under half: the fire is allowed. | share held | keep · ev 6.4 · ev 1.14 · mt3 2b wide, at the pause: the new buyers' kept share 0.39-0.43 (the ones that churn run), the old holders' 0.42-0.47 |
| **Holders' PnL** | The share of supply held by wallets in a named profit band right now | Filters. At the print. Near-even holders sell to get out even inside the rise we trade for; the SOL they would get against the SOL a +20 % rise needs is the get-out-even wall. On a frenzy the losing side is the younger, thinner coin, so more holders in profit reads as maturity. | Holders' selling into a rise is what the rise has to absorb, and where they sit against their cost says how much. | 1. The price is 1.00.<br>2. Holders who paid 1.00-1.25 (down 0-20 %) hold 18 % of supply.<br>3. A +20 % rise brings them back to even, and 18 % is over the cap: the fire is refused. | band (-20..0 %, any loss, +20 % or more, far up), as share or as the wall ratio | red · case U5 · ev 1.14 · mt3 2b wide: holders in profit, the float's gain, the last seller's profit read 0.44-0.50 on his pick and on BIG · mt3 2b wide, at the pause: the share of holders in profit 0.43-0.49 · 3ix: the crew's paper profit as a target is red; no crew wallet sells at a fixed multiple |
| **Operators round-tripped** | Operator structures that bought this coin have already sold out of it: fewer is better | Filters, by exclusion. At the print. | Bots that already took their profit here will not pay for our rise. | 1. 7 operator structures bought this coin.<br>2. All 7 have sold out.<br>3. The fire is refused. | count; which set (operator structures, a listed set) | new |
| **Top holders sold** | The biggest holders have sold out of the coin | Filters. At the print. **Concentration** reads what the top holders hold now; this reads that the ones who held most are gone. | The largest single dump risk leaves with them, so what is left cannot drop the price the same way. | 1. The top 3 holders held 30 % of supply.<br>2. All 3 have sold out.<br>3. Nobody left can drop it that far: the fire is allowed. | how many top holders, the share they once held, sold all or most | new |

### P4 Which structures, and who, have acted on this coin so far

The ix structure axis read as a standing state: which kinds printed, which named one, how the
buying splits across them, and how much a listed set or each structure sent inside a window. The
engine spells a listed set's flow `m_flow_ix`, over a fingerprint's pattern list. Wallet counts sit
at the end.

| Name | Idea | Meaning | Why it matters | Example | Parameters | Status |
| --- | --- | --- | --- | --- | --- | --- |
| **Classes present** | Which kinds of ix structure have printed on this coin, and how many | Filters. Lifetime or window. Read as a count of operator structures, a count of public tools or a count of all distinct structures; **Named structure present** reads one structure by name. | Each operator chose this coin on its own, so the count says how many independent decisions are in it; a structure a paying wallet stays away from is as much a decision as one it answers, and the two are not the same list. | 1. 9 different operator structures have bought the coin.<br>2. The cut is 8 or more.<br>3. 9 clears it: the fire is allowed. | kind (operators, tools, all), count band, window (lifetime, 2 s, 5 s, 30-60 s), read as a crossing (the Nth arrives, the first real bot after the creator and racers) | keep · ev 3.3 · ev 3.4g (omego E1a) · mt1 5.2d · ev 3.7 · ev 7 (C15) · mt3 2b wide: listed structures holding 0.40 on his pick, structures selling in 30 s 0.46 |
| **Named structure present** | One specific ix structure has printed on this coin inside the window, or has not | Filters. Window. **Classes present** counts kinds; this reads which one. The unit is the exact instruction list, never the app name. | Different structures are different actors with different latency and different followers: one is the start of a slow wave, another the end of a fast one. | 1. The window is 5 s.<br>2. A listed pump.fun buy structure printed 2 s ago.<br>3. It is on the allowed list: the fire is allowed. | which structure, present or absent, window, list picked on earlier days only | open · ev 3.4g (omego E1a, E1b) |
| **Structure mix** | Which kinds of structure the recent buying came through, and whether one of them dominates | Filters. Window. **Classes present** counts kinds; this weighs their share of the buying. **Racers around** is the racers-only reading. | Retail arriving through apps keeps arriving while racers dump what they just bought; many independent senders at once is real demand, and the same count from one busy bot is not. | 1. 1.2 SOL was bought in the last 5 slots.<br>2. All of it came through Axiom and Photon, and no structure sent more than 30 % of the prints.<br>3. The fire is allowed. | share by class (tools only, no racers, operators' share), share of the largest structure, count of distinct structures, window | open · ev 1.11 |
| **Group flow** | The SOL and prints a listed set of structures sent, added up as one group over a window | Filters. Window. **One structure's flow** keeps a line per member; this adds the list up. | A trader standing behind a set of structures answers their combined buying, not any single one, so the set's flow is how much of that buying is on the coin now. Too little is noise; too much means the move is already made. | 1. The list holds Axiom, Photon and Terminal buy structures.<br>2. Together they bought 1.2 SOL over the last 2 slots.<br>3. Inside the band: the fire is allowed. | which list (tools, trader-derived, operators), buy SOL, sell SOL, net, prints, band or floor, window (slots, seconds, prints) | open · mt3 d8 |
| **One structure's flow** | For each structure on its own: how many prints it sent in the window, how much SOL, and how many in one slot | Filters. Window. **Group flow** adds a list up; this keeps a line per structure, so the cut can differ by structure. **Named structure present** is its presence-only form. | A wallet that answers a structure answers a certain amount of it, and the amount can differ by structure. | 1. A Jupiter V6 buy structure printed 3 buys in the last 5 s.<br>2. They carry 3.4 SOL, two of them in the trigger's slot before it.<br>3. That clears this structure's own line: the fire is allowed. | per structure: prints, SOL, same-slot count, repeats; window; cut per structure | red · ev 3.4g (omego E1e, E2c) · mt3 d8 |
| **Order of the last prints** | Which structures printed just before the trigger, in order and with their side | Filters. Window of the last one to three prints. **Classes present** counts them; this reads the sequence, so "a retail app bought, then a Token-2022 program bought" is one value. | The bots that trade together land in an order, and a wallet that follows them reads that order. | 1. Two prints back, a retail-app structure bought.<br>2. One print back, a Token-2022 program bought.<br>3. That sequence is on the allowed list: the fire is allowed. | one to three prints back, side kept or not | new · mt3 2b wide (his pick 5-12x, BIG and DEAD move together) · in the layers: its earlier-day his rate 0.56-0.59 inside his cell |
| **Record of those present** | The earlier-day record of the structures already on this coin, averaged | Filters. Lifetime. **Structure record** (P5) reads the one structure firing; this reads the company it is in. | The "many smart wallets on one token" idea, read through structures instead of wallets, which rotate. | 1. Five structures have printed on this coin.<br>2. Their earlier-day answer rates average 0.55.<br>3. Above the bar of 0.50: the fire is allowed. | mean, best, count above a bar | red · mt3 d8 · mt3 2b wide, his picks against their lookalikes: the pusher's own earlier pushes that ran BIG 0.56, over the last hour 0.54 · mt3 2b wide, the pushing wallet's own money on earlier days rises -4.18 to -2.19 % over its fifths (test -4.44 to -2.10 %), never positive alone |
| **Racers around** | No seed racer has printed in the slots just before ours | Filters. Window. **Structure mix** weighs every class; this asks only for no racer. The trigger itself never being a racer is E's standing exclusion. | If the racers have not arrived yet, the move they react to is still ahead of us, and racers dump what they just bought. | 1. The window is the last 10 slots.<br>2. No seed racer printed in it.<br>3. The fire is allowed. | window | new |
| **Distinct wallets** | How many distinct wallets have bought this coin: a band | Filters. Lifetime. Rule 1 pairs it with **Age**. | It is the plainest measure of how many people the coin has reached; a coin that is not crowded yet still has its buyers ahead of it, and an old broad one absorbs a sell. | 1. 400 wallets other than the creator have bought the coin.<br>2. The floor is 368.<br>3. 400 clears it: the fire is allowed. | band (booked on rule 1: >= 368 non-creator buyers), buyers only or all, read as a crossing (the second wallet other than the creator) | keep · ev 1.22 · ev 7 (rule 3b) |
| **Sell-reactive buyers** | How many wallets on this coin bought right after an earlier big public sell here | Filters. Lifetime. **Frenzy sells unabsorbed** (P2) reads whether the coin's dips recovered; this counts who did the buying back. | Dip-buying bots that already caught this coin's sells will catch the next one, and their buying lifts the price after our fill. | 1. Five wallets here bought within 300 ms of a sell of 1 SOL or more.<br>2. The floor is 4.<br>3. Five clears it: the fire is allowed. | reaction time, sell size, count floor | red · case 41 |

### P5 The trigger structure's record on earlier days

| Name | Idea | Meaning | Why it matters | Example | Parameters | Status |
| --- | --- | --- | --- | --- | --- | --- |
| **Structure record** | The share of this structure's earlier pushes that were answered by size, or what they earned at our seat, on strictly earlier days | Filters. Earlier days only. The one fact on the push node that predicts the outcome and carries no slip, because it is history and not tape; the money form is weaker than the answer form and adds nothing on top of it. | Everything else that predicts incoming size is size that already landed, which is what we pay for; this one is free. | 1. On earlier days, 60 % of this structure's pushes were answered by a size buy.<br>2. The bar is 50 %.<br>3. It clears the bar: the fire is allowed. | answered or earned, window of days (expanding, rolling), cut, per pack shape | keep (term) · mt3 d8 |

### P6 The whole tape right now

State of every coin at the print, kept as one counter for the whole tape. A ranking across coins
is dropped: it is a cost on every token.

| Name | Idea | Meaning | Why it matters | Example | Parameters | Status |
| --- | --- | --- | --- | --- | --- | --- |
| **Group live now** | Another coin of the same launch build is trading right now | Filters. At the print. **Group history test** (D1) reads the build's yesterday; this reads its activity this moment. | The launcher and the buyers who follow it are awake now, so this coin can get the same attention. | 1. Coin A from the build printed 20 s ago.<br>2. Coin B from the same build reaches a fire.<br>3. The build counts as live: the fire is allowed. | how recent, how many coins, which build key | red · ev 7 (C15) |
| **Pushes elsewhere** | How many pushes by listed structures land on other coins in the trigger's slot | Filters. At the print. **The push** (E1) is one coin's event; this counts the same event across the tape at that moment. | When listed bots push many coins at once, the market's buyers are awake and chasing, and our push lands on a tape that answers. | 1. The trigger lands in slot S.<br>2. Six other coins get a push in slot S.<br>3. Six clears the floor of 5: the fire is allowed. | count floor, same slot or a window, which list | open · mt3 d8 |

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
| **Trail** | Sell once the price falls a set share from its best since our fill | Sells. Unarmed it trails from the first second; armed it waits for a gain first, and without a stop under the arm four fifths of entries hold to the cap with nothing cutting them. The width can widen as the peak grows - the curved width, growing with the best gain and flattening as the run gets big, is the shape 8dtx2t's own sells follow - and the peak can be the burst we joined instead of the coin's. | It rides a move as far as it goes and leaves when the move gives back, without guessing a target; a big winner swings harder, so one width is wrong somewhere. | 1. The trail arms at +21 % and is 36 % wide.<br>2. The trade peaks at +50 %.<br>3. It falls 36 % from that peak: we sell. | arm level; width shape (fixed, stepped, widening from a floor with the run, curved); what is kept (the fall off the best, or a share of the best gain); cap; peak basis (coin, the burst); with or without a stop (booked: arm +21 %, trail 36 %, stop -43.75 %, cap 1200 s) | keep · ev 4.7 · ev 7 (armed trail with no stop) · ev 1.24 · case U6 · mt3 xfirst · 3ix: a tight trail is flat, wider trails lose on study days |
| **Bracket** | A target, a stop and a clock, whichever comes first | Sells. Rule 1's exit. Half out at the target, a trail after the target and a longer clock are its spellings, all red on rule 1. The tail-keeping shape (wide target, wide trail, long cap) is the same three numbers set for the rare large winner, and it is unreachable on a pool where four fires in five never run. | An event that bounces quickly pays a small sure target, and the wide stop is there for the rare collapse. | 1. The target is +20 %, the stop -60 %, the clock 90 s.<br>2. The trade reaches +20 % at second 40.<br>3. That comes first: we sell. | the three numbers (booked on rule 1: +20 %, -60 %, 90 s), half out, trail after target | keep · ev 1.22 · ev 1.24 · ev 1.20 · ev 3.4i (omego E5) |
| **Gate then ride** | At a set age, sell at once unless the trade is up enough; if it is, hand it to a trail and hold | Sells. A bracket caps every trade at the same age; this gates, so a dead trade is cut and a live one is let go. The gate belongs where the losers die, not before: on 8dtx2t that is 8 s, not 5. | A trader whose median trade loses and whose top 1 % carries the net earns from the right tail, and a flat clock cuts every tail it has. | 1. The gate is at 8 s.<br>2. At 8 s the trade is up 45 %, over +40 %: it goes to a trail capped at 300 s.<br>3. Another trade is up 3 % at 8 s: we sell it now. | gate age, ride condition, trail and cap after the gate; per structure or one for all | keep · mt3 d8 · mt3 2b wide: 26 checkpoint facts at 5-120 s read 0.46-0.55 on the next move; the checkpoint questions that work are unbuilt |
| **Abort** | Sell if the trade is not up by a set amount after a set time | Sells. **Clock** closes everything at the time; this closes only what has not moved. | Three to twenty seconds in, a future winner and a dud read the same on price and a clock, so the grid has no positive cell. | 1. The rule is +5 % by 30 s.<br>2. At 30 s the trade is +2 %.<br>3. We sell. | T, G | dead · ev 4.4 · ev 3.4g (omego E2d) |
| **Width from the coin's swing** | The trail's width is set by how hard the coin itself swung before our fill: a wild coin gets a wide one, a calm coin a narrow one | Sells. **Trail** sizes its width by the run since our fill; this sizes it by the coin's own movement before it. | A wild coin dips hard on its way up, so one fixed width sells its winners in a dip, while the same fall on a calm coin means the move is over. | 1. Coin A swung hard in its last minute: the width is 24 %.<br>2. Coin B moved smoothly: the width is 12 %.<br>3. Both fall 15 % from their best: we sell B and keep A. | the swing measure (range, speed), window, width scale, floor | red · mt3 xbook |
| **Exit picked at the buy** | A fact at the buy picks the exit: a tight one for a risky-looking buy, the rule's own for a clean one | Sells. Every other X row sells on what happens after the fill; this chooses the exit before it. | A trader keeps a shaky-looking buy on a short leash and lets a clean one run, so a bad buy's loss is capped without cutting the good ones. | 1. At the buy the price sits 20 % under the coin's high: risky.<br>2. This trade sells at 45 s instead of 90 s.<br>3. A buy 5 % under its high keeps the rule's exit. | the fact at the buy, the tight exit (stop, clock) | red · case U9 |

### X2 The tape stops

| Name | Idea | Meaning | Why it matters | Example | Parameters | Status |
| --- | --- | --- | --- | --- | --- | --- |
| **Silence of X** | No print from X for a stretch, on either side of the fill | Sells. Whose silence is the parameter. First-time public buyers stopping replaces the clock rather than adding to it, fires on 92 % of the book and holds a plateau, and is blocked on one engine metric. Operators stopping beats the clock and fails the fold test. The coin's whole tape stopping is the best state cut the lab reads. The firing structure alone falling quiet is noise: a tool prints on one coin among hundreds. An arm on a gain first is inert. | The queue behind us is what lifts the price; when it empties there is nobody left to pay more, as true of a winner as of a loser. | 1. X is first-time public buyers; N is 6 s.<br>2. The last new buyer arrived at second 40, and second 46 passes with none.<br>3. We sell, whether we are up or down. | X: first-time public buyers, operator structures, a listed group, any print, the firing structure; N; gate (ungated, loser-only, winner-only: a bank when buyers stop) | keep · case K15-K22 · ev 4.7 · ev 7 (C15) · mt3 2b wide: no public buy for 10 s under the cap curve is the one logic that holds on both halves of his picks · conditional on the price at every pause (buyers stopped 10 s while under -2.7 %): +5.45 % on his picks over twelve days against +3.97 %, 82 % of draws · the pause sells 71 % of his BIG picks at +12 % (mt3 2b wide, where the exit loses); a pause that sells only in the first seconds and later only under the fill moves money from the losers to the runners, fit +6.23 % test +3.19 % |
| **Sellers eat the buys** | Since our fill, the public has sold at least half of what it bought | Sells. **Flow turns** reads the balance over a sliding window; this adds up everything since our fill, so a quiet stretch does not reset it. Early it is a loser's question; after 30 s, with the trade in profit, it is a take-profit that sells runners. | The buyers who came after us are the ones who would lift the price; when half their money is already walking back out, the move has no fuel left. | 1. Since our fill the public bought 4 SOL.<br>2. It sold 2.5 SOL in the same time, over half.<br>3. The price is under our fill at 10 s: we sell. | share of the buys, the checkpoint, gate (under the fill / any) | new · mt3 2b wide, his sells: 0.71-0.76 at 2-10 s; at our seat red: under the fill it is the price again (mt3 2b wide, his questions at our seat) |
| **Buys slow** | The pace of public buys drops under the pace just before our fill | Sells. **Silence of X** waits for no buy at all; **Buys fade** reads their size; this reads their count against the coin's own pace at the entry. | A move is carried by the rate of new buys; once that rate falls under what brought us in, the crowd has stopped arriving. | 1. In the 10 s before our fill the public bought 12 times, 6 per 5 s.<br>2. At 10 s the last 5 s hold 3 buys, half that pace.<br>3. The high is 7 s old: we sell. | window, share of the entry pace, the high's age | new · mt3 2b wide, his sells: at 10 s with the high >= 5 s old, 54 % sold against 27 %; at our seat red: it cuts runners with the losers (mt3 2b wide, his questions at our seat) · 3ix: no outsider buy for a window loses; the crew's dump lands first |
| **Busy coin may pause** | At the first pause in buying, keep the position when the coin has been busy since our fill | Sells or holds. **Silence of X** sells on every pause; this lets a pause pass when many new wallets and much SOL came in since the fill. | A coin that drew a crowd breathes between waves, while a coin nobody joined is simply over, and both go quiet for 10 s. | 1. Since our fill 30 first-time buyers came and 14 SOL was bought.<br>2. Then 10 s pass with no buy.<br>3. The coin was busy: we keep it on the trail. | first-time buyers, SOL bought, the best gain so far, bars | red · mt3 2b wide, at the pause: +5.72 % 5/6 fit, +4.63 % 6/6 test against the incumbent's +4.28 / +4.87 % |
| **Flow turns** | Public sell SOL over the last window is at least the public buy SOL | Sells. **Net flow** (P2) is the same reading before the fire. | The balance of the tape carries the price; but the balance turns in normal chop, so it fires early and often and cuts the winners. | 1. The window is 20 s.<br>2. In it the crowd sells 4 SOL and buys 3.<br>3. We sell. | window, gate (under the fill: sellers take over), public or a listed group | red · case K18 |
| **Fees fall** | The prints after ours stop paying extra to land quickly | Sells. **Fee paid** (E4) is the same reading before the fire. | Urgency is what makes a move run; when nobody pays for speed any more, the hurry is over. | 1. The buyers around our fill tipped 0.002 SOL.<br>2. The last 5 s of buyers tip 0.0005 SOL, a quarter of that.<br>3. We sell. | ratio to the entry level, window | new · mt3 2b wide: purpose -16 to -18 on every push |
| **Buys fade** | The last few public buys after ours are small against the trigger | Sells. **Trigger size** (E4) before the fire, this after it. | The buyers still arriving are the small ones, so the move has no money left behind it. | 1. The trigger was a 1 SOL buy.<br>2. The next three public buys are 0.05, 0.08 and 0.04 SOL.<br>3. Each is under a tenth of the trigger: we sell. | how many buys, the ratio to the trigger | red · mt3 2b wide (purpose -12 to -29) |
| **Crowd under water** | While we are under the fill, the price falls under what the last minute's buyers paid on average | Sells. **Actor sells** (X3) waits for a loser to sell; this reads that the recent buyers as a whole are losing, before they sell. | Recent buyers who are losing are the next sellers, so once the crowd behind us is under water the supply that pushes the price down is already in their bags. | 1. Wallets that bought in the last 60 s paid 1.05 on average.<br>2. We filled at 1.00 and the price is now 0.97.<br>3. We sell. | window of buyers, margin under their cost, gate | new |

### X3 An actor acts

| Name | Idea | Meaning | Why it matters | Example | Parameters | Status |
| --- | --- | --- | --- | --- | --- | --- |
| **Actor sells** | A named actor sells this coin; we react | Sells. Who sells and how we react are the parameters. Loser-only is the spelling that pays: a sell is the counterparty a rising coin needs, so leaving on one while ahead hands the move away. The creator is inert past the age rule 1 fires at, because he has no bag left. The first big sell whoever sent it is red on 8dtx2t too: in 57 % of his exits the sell he leaves on is smaller than one he sat through. | The one we followed knows its own reason better than we do; the buyer who lifted the price is the one whose selling drops it furthest; a creator selling can be the first leg of a rug. | 1. We fired on a structure's buy and are 8 % under the fill.<br>2. That structure prints a sell.<br>3. Loser-only: we sell. Had we been up, we would hold. | actor (firing structure, pusher, creator, anyone above a size, a winner selling half, a top holder at our fill, two or more wallets of one bundle in one slot, several quick flippers together, any wallet under its cost), reaction (out now, out if losing, trail from here) | red · case K18-K20 · case U8 · ev 7 (C11) · mt3 5.1 sells · mt3 2b wide: a loser dumping half at -40 %, the birth buyer, the trigger's structure, a listed structure: each judged alone; only the loser dump keeps runners · 3ix: the launch crew (creation-slot `ATA Create, Buy` wallets plus the creator), out now: its first sell is usually the coin's running peak, the best exit read there, short of the day bar; the crew sells in buy order, cheapest first |
| **Actor leaves** | The ix structure we fired on starts buying a different coin | Sells. **Silence of X** reads its silence here; this reads its activity elsewhere. | Its attention and its money have moved on, so it will not buy this coin again. | 1. We fired on a structure's buy.<br>2. That structure starts buying a different coin.<br>3. We sell. | - | red · ev 7 (C13) |
| **Sell into a buyer** | Once we are up enough, sell on the next big public buy or hard price step | Sells. | A big buy is both a high price and a counterparty, which is exactly what a seller needs. | 1. We are up +12 %, over the +10 % arm.<br>2. A 1.2 SOL public buy lands.<br>3. We sell into it. | gain before it arms, buy size or price step | red · ev 1.24 |

### X4 The swing turns down

The price path after our fill, read as a swing: the top, the dip, the bounce. Every row reads the
price alone, so a winner's early dip and a loser's fade can look alike at our seat.

| Name | Idea | Meaning | Why it matters | Example | Parameters | Status |
| --- | --- | --- | --- | --- | --- | --- |
| **Lower high** | After the top the price dips, bounces short of the top, and turns down again | Sells. **Dip break** waits for the dip's low to go; this fires earlier, when the bounce rolls over. | A bounce that cannot retake the top is the first sign the buyers are done. | 1. The top is +50 %, then a dip to +30 %.<br>2. A bounce reaches +42 %.<br>3. It turns down to +40 %: we sell. | how far the bounce turns down, how short of the top | red · mt3 xfirst |
| **Dip break** | After the top the price dips and bounces; sell when it breaks under that dip's low | Sells. **Low breaks** reads a low from before our fill; this reads the swing after our own top, so its line is wherever this coin's dip was. | A broken dip low is the chart saying the move is over, and its level differs on every coin, which a fixed percent cannot follow. | 1. The top is +50 %, the price dips to +35 %.<br>2. It bounces to +45 %.<br>3. It falls to +34 %, under the dip: we sell. | dip depth under the top, bounce size | red · mt3 xfirst |
| **Sudden dump** | Once the trade has been a winner, the price drops hard over the last few seconds | Sells. **Trail** reads the fall from the top whenever it happened; this reads only how fast the price is falling now. | A fast drop is the moment holders run for the door, and waiting a few more seconds is what costs a winner. | 1. The best is +30 %.<br>2. The price drops 5 % inside 3 s.<br>3. We sell. | drop size, window, the gain first needed | red · mt3 xfirst |
| **High goes stale** | Once the trade has been a winner, no new high prints for a while | Sells. **Silence of X** waits for no prints at all; this counts from the best since our fill, and prints may keep landing. | A move that has stopped making highs has stopped being a move, whatever the percent off the top. | 1. The best is +40 %.<br>2. 20 s pass with no print above it.<br>3. We sell. | seconds without a new high, the gain first needed, a giveback added | red · mt3 xfirst · mt3 2b wide, his sells: ungated at 5-30 s, a high >= 5 s old marks positions reaching +100 % 7-14 % against 17-29 % |
| **Early pop fails** | The trade pops up in its first seconds, then falls back to the fill before a set time | Sells. A **Stop** moved to the fill closes whenever the trade was up a little; this fires only when the pop comes early and is lost early. | On a dip bought inside a frenzy the losers pop early and fall, while the winners often dip first and peak later, so an early pop given back is the loser's shape. | 1. The trade reaches +10 % 4 s after the fill.<br>2. By 15 s the price is back at the fill.<br>3. We sell. | pop size, the time it must hold by | red · case U6 |
| **No breakout** | We bought near the coin's all-time high, and while we are under the fill no new all-time high prints for a while | Sells. **High goes stale** counts from the best since our fill once the trade was a winner; this counts from the coin's all-time high and fires under the fill. | The dip was bought on the bet that the frenzy retakes the high, so a coin that cannot while we are losing has lost the reason we bought. | 1. We fill 3 % under the all-time high.<br>2. We are under the fill and 30 s pass with no new high.<br>3. We sell. | how near the high we bought, seconds without a new high | red · ev 1.24 |
| **Low breaks** | vsol makes a new low, or returns to the low before the event | Sells. **Low holding** (P2) is the same test before the fire; **Dip break** reads the dip after our own top. On an event that fires on a seller the fill is still falling, so a new low lands 0.5 s later on every trade and the clause is a same-tick exit. | The bounce we bought has failed and the selling that made it is not finished; everything the event did has been undone. | 1. Before the event the low was vsol 55.<br>2. After our fill vsol returns to 55.<br>3. We sell. | new low vs pre-event low, gate | red · ev 7 (C9, C14) · case K20 |

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
| silence | Quiet birth | The push · Back after a gap · Breaks the coin's silence | Tape busyness (silent now) | Silence of X · High goes stale | - |
| net flow / price path / swing | - | After sellers · Capitulation · Dip buy after a rise · Flush stops · Pullback depth | Net flow · A rise exists · Under its peak · Low holding · Frenzy sells unabsorbed | Flow turns · X4, all rows | - |
| curve position | Life at a fixed age | - | Curve position | Take profit (headroom) | - |
| which ix structure | Create fingerprint | A class of structure buys · The push · Pack in one slot · Buy wave · List silent, then back · Structure after structure · Arrives from another coin | Classes present · Named structure present · Structure mix · Group flow · One structure's flow · Order of the last prints · Racers around · Structure record · Pushes elsewhere | Silence of X · Flow turns · Actor sells · Actor leaves | - |
| a structure's own history | Group history test · Creator record | This structure's record here (E2) | Record of those present · Structure record · Group live now | Actor leaves | - |
| who holds the supply | First-slot money · Creator's opening buy | A sell we buy into (seller kind) · Capitulation | P3, all rows | Actor sells · Crowd under water | - |
| size of a print | - | Trigger size · Price-step buy · Since the last step-up | Big sells lately | Sell into a buyer | - |
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
| following a copied wallet, selling to its copiers, a roster wallet printing, a leading-wallet door | wallet identity or a follower graph; the ix structure is the only stable identity layer (strategy law 20) |
| wallet age, prints elsewhere, take-out record, buy against the wallet's own habit, rotation per wallet, launch buyer back, long-hold buyers' share, the buyer winning on its other coins | per-wallet history across the whole tape: global per-wallet state on the hot path, and the same identity objection |
| first print this UTC hour · best step-up of the moment across coins | no mechanism; a clock boundary means nothing to a bot, and cross-coin ranking is a cost on every token |
| alone in its slot, two structures in one slot, two lists agree, tools in one slot | the slot must close to know it, up to 400 ms after the trigger. Only what landed before the trigger survives, inside **Slot company** |
| a maximum cost on the buy, a trail fitted to our own fill | execution settings, not ideas: they belong to the seat and to the exit's reference price (mt3 legs, xrefit) |
| his quiet picks, his coin not his moment, the crowd called in advance, alone at the fill and crowded right after | study findings on one trader's picks, labels rather than rules; they live in mid-tape-rule-3 (xpick, coin, crowd, xwait) |
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

---

## Old names

Every idea name an earlier version of this file used, and where it lives now: a row (in bold), a
parameter of a row, a Dropped line, a conjunction of rows, or a method step, finding or label that
lives in [_!___derive.md](_!___derive.md) or its case file.

| old name | now |
| --- | --- |
| "Now" tells | Dropped (feed position); **Creator's position** (a creator buying again); **Tape busyness** (the cadence of prints) |
| 0.2 SOL | **Fixed clip** |
| 8dtx list | **A class of structure buys** |
| A loser gives up | **Actor sells** |
| A maximum cost on the buy | Dropped (an execution setting) |
| A moment we choose | derive 5.2: the random-moment control, a yardstick rather than an event |
| A named structure printed | **Named structure present** |
| A structure's window shape | **One structure's flow** |
| After a sell run | **After sellers** |
| After a structure sold | **After sellers** |
| Age band | **Age** |
| Alone at the fill, crowded right after | Dropped (a finding on one trader's picks) |
| Alone in its slot | Dropped (the slot must close) |
| Already ran up | **Net flow** (the move before the print) |
| Already running | **Net flow** (the move before the print) |
| Another roster wallet is printing | Dropped (wallet identity) |
| Answers a sell | **After sellers** |
| Armed trail + stop | **Trail** |
| Arrives from another coin | **Arrives from another coin** |
| Bank when buyers stop | **Silence of X** (winner-only) |
| Below our last exit | **Price vs last exit** |
| Best of the moment | Dropped (a ranking across coins) |
| Big buys | **Tape busyness** (mean buy size) |
| Big for this wallet | Dropped (per-wallet history) |
| Big price step | **Price-step buy** |
| Big winners | **Holders' PnL** |
| Block position | **Slot company** (position in the block) |
| Bracket | **Bracket** |
| Bundle sells together | **Actor sells** |
| Burst start | **Back after a gap** |
| Buy after the crowd | **Crowd left** |
| Buy flow spike | **Trigger size** |
| Buy SOL band | **Group flow** |
| Buyers stop | **Silence of X** |
| Buying outruns selling | **Net flow** |
| Buys in the trigger's own slot | **Slot company** |
| Calm tape | **Tape busyness** |
| Campaign structure | **A class of structure buys** |
| Capitulation cascade | **Capitulation** |
| Clip left | **Adding a leg** |
| Clip step-up | **Clip step-up** |
| Clock | **Clock** |
| Coarse bucket | **Create fingerprint**; Dropped as a door on its own |
| Coin already silent | **Tape busyness** (silent now) |
| Coin is visible now | Dropped (off-chain) |
| Coin step-up | **Trigger size** (the coin's last public buy) |
| Coin tape stops | **Silence of X** |
| Cool-down after a stop | **Cool-down** |
| Copied buyer | Dropped (wallet identity) |
| Copy a wallet | Dropped (wallet identity) |
| Cost minimum | **Cost minimum** |
| Count crossing | **Classes present** and **Distinct wallets** (read as a crossing) |
| Coverage at his latency | mt3 d8: a finding |
| Creation-level facts | mt3 d8: a finding, read through the D rows |
| Creator holds | **Creator's position** |
| Creator rebought | **Creator's position** |
| Creator sells | **Actor sells** |
| Creator's prior wall rate | **Creator record** (the creator wallet alone) |
| Creator's trailing record | **Creator record** |
| Crowd behind the print | **Tape busyness** |
| Crowd left | **Crowd left** |
| Crowd under water | **Crowd under water** |
| Crowded frenzy | **Tape busyness** (wallets) |
| Curved trail | **Trail** (curved width) |
| Dip break | **Dip break** |
| Direct by instruction | **A class of structure buys** |
| Documented | **Documented** |
| Door + race, the chain | mt3 d8: a sentence |
| Dump factory | **Dump-factory exclusion** |
| Early life | **Life at a fixed age** |
| Early pop fails | **Early pop fails** |
| Exact launch | **Create fingerprint** |
| Feed tells | Dropped (feed position); **Creator's position** (a creator buying again) |
| Fees fall | **Fees fall** |
| Fill delay | derive 5: DELAY, a setting of every event |
| Firing structure holds | **Actor still holds** |
| Firing structure leaves | **Actor leaves** |
| Firing structure sells | **Actor sells** |
| Firing structure silent | **Silence of X** |
| First big sell out | **Actor sells** |
| First leg | **Adding a leg** |
| First operator after snipers | **Classes present** (read as a crossing) |
| First run here | **First here** |
| First size after T | **Trigger size** read past a set age with **Age** |
| Fixed age | derive 5.2: a control, not an event |
| Flat on the coin | **One open per coin** |
| Flippers cash out | **Actor sells** |
| Floor held | **Low holding** |
| Flow turns to selling | **Flow turns** |
| Flush recovered | **Under its peak** |
| Flush resumes | **Low breaks** |
| Flush stops | **Flush stops** |
| Follow him and clock it | Dropped (wallet identity) |
| Fraction of vsol | **Fraction of vsol** |
| Frenzy flip sell | **A sell we buy into** |
| Frenzy sells unabsorbed | **Frenzy sells unabsorbed** |
| Fresh buyer | **A class of structure buys** (the buyer new to the coin) |
| Fresh fall into the fire | **Net flow** (the move before the print) |
| Gap since last step-up | **Since the last step-up** |
| Group live now | **Group live now** |
| He follows this fire | mt3 d8: a label |
| Headroom | **Curve position** |
| Headroom target | **Take profit** |
| High fee | **Fee paid** |
| High goes stale | **High goes stale** |
| High peak, mid-curve | **Under its peak** |
| His coin, not his moment | Dropped (a finding on one trader's picks) |
| His pick at our seat | mt3 d8: a finding |
| His picks against the chain's, inside each bucket | mt3 d8: a finding |
| His quiet picks | Dropped (a finding on one trader's picks) |
| Holders in profit | **Holders' PnL** |
| Identity family | **A class of structure buys** |
| Instant-wall group | **Group history test** |
| Keep a share of the gain | **Trail** (a share of the best gain kept) |
| Large among recent | **Trigger size** |
| Last round trip's result | **Last result carried** |
| Launchpad safety counts | Dropped (the venue's safety panel) |
| Leading wallet door | Dropped (wallet identity) |
| Listed structure sells | **A sell we buy into** |
| Long past its best | **Under its peak** |
| Long-hold buyers | Dropped (per-wallet history) |
| Louder than last time | **Since the last step-up** |
| Low bundle | **Bundled share** |
| Low creator share | **Creator's position** |
| Lower high | **Lower high** |
| Made a hill | **A rise exists** |
| Many structures | **Structure mix** |
| Mid-range price | **Under its peak** |
| Near-even holders | **Holders' PnL** |
| Never listed | E's standing exclusion (seed racers and aggregator routes are never a trigger) |
| New buyers still arrive | **Tape busyness** |
| New-buyer acceleration | **New-buyer acceleration** |
| No big sells lately | **Big sells lately** |
| No breakout | **No breakout** |
| No new low | **Low holding** |
| No race after the fire | **Tape busyness** (the race) |
| No racer lately | **Racers around** |
| No sells just before | **Big sells lately** (any sell, the print just before) |
| Nonce buyers | **A class of structure buys** |
| Not a seed racer | E's standing exclusion (seed racers and aggregator routes are never a trigger) |
| One open per coin | **One open per coin** |
| Operator behind the addresses | Dropped (joining rotating creator addresses) |
| Operator count | **Classes present** |
| Operators | **A class of structure buys** |
| Operators in | **Classes present** |
| Operators round-tripped | **Operators round-tripped** |
| Operators stop | **Silence of X** |
| Other move type | **Group history test** |
| Pool alive | **Pool alive** |
| Post-hill dip buy | **Dip buy after a rise** |
| Pre-event low | **Low breaks** |
| Profile by outcome, both tails | derive: a method step |
| Proven launcher, unloaded launch | a conjunction of **Group history test** and **First-slot money** |
| Public-app majority | **Public-app share** |
| Pusher holds | **Actor still holds** |
| Pusher sells | **Actor sells** |
| Quick flip sell | **A sell we buy into** |
| Quiet birth | **Quiet birth** |
| Quiet tape, loud buy | a conjunction of **Trigger size** and **Structure mix** |
| Rank crossing | Dropped (feed rank) |
| Reused content | **Reused content** |
| Ride the creator | **Actor sells** (trail from here) |
| Room by the coin's swing | **Width from the coin's swing** |
| Room under the wall | **Curve position** |
| Round trips taken | **Tickets per coin** |
| Same wallets buy again | **Tape busyness** (returning buyers' share) |
| Same-structure follow | **Order of the last prints** |
| Second outsider | **Distinct wallets** (read as a crossing) |
| Seed-racer burst after silence | **Racer burst** |
| Sell into a buy | **Sell into a buyer** |
| Sell to the copiers | Dropped (wallet identity) |
| Sell-reactive buyers | **Sell-reactive buyers** |
| Seller at a loss | **A sell we buy into** |
| Sellers take over | **Flow turns** (under the fill) |
| Silence, then K | **The push** |
| Silent-coin breaker | **Breaks the coin's silence** |
| Size buy | **Trigger size** |
| Size into a dip | **Size into a dip** |
| Slow re-entry state | a conjunction of **A rise exists**, **Under its peak** and **Back after a gap** |
| Slow-wall group | **Group history test** |
| Static abort | **Abort** |
| Step-ups so far | **Since the last step-up** |
| Structure count | **Classes present** |
| Sudden dump | **Sudden dump** |
| Swing pullback | **Pullback depth** |
| Tail keeper | **Bracket** |
| Take profit | **Take profit** |
| The assembled rule | mt3 d8: a sentence |
| The buyer wins elsewhere | Dropped (per-wallet history) |
| The coin doubles from here | Dropped (a label) |
| The coin is hot | **Net flow** |
| The creator has not dumped | **Creator's position** |
| The crowd called in advance | Dropped (a finding on one trader's picks) |
| The exit ceiling | derive: a method step |
| The second leg, not the ignition | a conjunction of **A rise exists**, **Under its peak** and **Price-path turn** |
| The slot race | **Slot company** |
| The staircase exit | **Gate then ride** and **Silence of X** |
| The toll bar, asked first | derive: a method step |
| The trail's shape, measured | **Trail** |
| The trigger print's own step | **Price-step buy** |
| This coin has already run | **Tickets per coin** |
| Tight exit for a risky buy | **Exit picked at the buy** |
| Time out of the coin | **Cool-down** |
| Tools | **A class of structure buys** |
| Tools in | **Classes present** |
| Tools in one slot | Dropped (the slot must close) |
| Tools only | **Structure mix** |
| Top holders | **Concentration** |
| Top holders sell | **Actor sells** |
| Top holders sold | **Top holders sold** |
| Trader mode split | derive: a method step |
| Trail fitted to our own fill | Dropped (an execution setting) |
| Two lists agree | Dropped (the slot must close); across slots it is **Structure after structure** |
| Two structures, one slot | Dropped (the slot must close); before the trigger it is **Slot company** |
| Unarmed trail | **Trail** |
| Under its own sell | **Buying back under its sell** |
| Up-move portrait | **Net flow** (the move before the print) |
| URI host | **URI host** |
| Vsol band | **Curve position** |
| Wallet age | Dropped (per-wallet history) |
| Wallet back here | **Back after a gap** (per wallet on this coin) |
| Wallet takes money out | Dropped (per-wallet history) |
| Wallets on the coin | **Distinct wallets** |
| What the token says about itself | **Documented** |
| What this ix structure did before | **Structure record** |
| Widening trail | **Trail** (widening width) |
| Young big sell | **A sell we buy into** |
| Zigzag turn | **Price-path turn** |
