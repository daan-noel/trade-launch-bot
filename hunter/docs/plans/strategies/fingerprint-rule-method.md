# Fingerprint rule method: from raw trades to two stage-tree rules

How to take one launch group (one fingerprint) from raw trades to a complete, tested rule.
A fingerprint group can contain several recurring ways the owner runs a coin. The workflow
discovers those ways from behavior; it does not assume their names or number. Nothing here
belongs to one fingerprint: a new group starts at W0 ("The steps").

| Read with | For |
| --- | --- |
| [stage-tree-plan.md](stage-tree-plan.md) | the engine's rule format (`phases`, `situations`, `cells`, `enter.cells`, `always`) |
| [backtest-audit.md](backtest-audit.md) | the bar every reported number passes |
| [execution-costs.md](execution-costs.md) | what one round trip costs |
| [lake-pg-read-paths.md](../database/lake-pg-read-paths.md) | which store the lab reads (the Parquet lake) |
| [fingerprint-case-6ix.md](fingerprint-case-6ix.md), [fingerprint-case-5ix.md](fingerprint-case-5ix.md) | worked cases: their numbers, rules and mistakes |

---

## 1. What we build

The workflow follows this order. It discovers recurring owner playbooks first, describes each
with live-readable phases and situations, then finds actions and evaluates the whole rule.
Outcome names such as "fast dump" and "migration" label results; they do not define the games
or live branches in advance.

```text
FINGERPRINT GROUP
  one creator / launch shape; possibly several recurring owner playbooks
       |
       v
DISCOVER behavior from token histories      (W0-W2)
  owner actions + outsiders + pool + price; propose recurring playbooks
       |
       v
DESCRIBE each playbook live                 (W3-W4)
  phases (where in the token's life) x situations (what is happening now)
       |
       v
FIND the complete rule                      (W5-W6)
  AMM exit -> curve exits -> streak gate -> entry -> exits again -> stable rule
       |
       v
CHECK it under changing mixes and periods   (W7-W10)
  per-playbook results + whole-rule results + untouched forward data + engine parity
```

The second picture shows how that rule reads one token:

```text
each new engine step (print or tick)
       |
       +--> update group history when a coin's gate result settles
       v
read current phase (moves forward) + current situation (first matching condition)
       |
       v
look up that phase x situation cell; check always lines
       |
       +--> no line true --> wait for the next event; read the tree again
       |
       +--> buy line true --> check streak gate; blocked: wait; allowed: buy once
       |
       +--> part-sell / sell line true --> execute; continue reading remaining position
       |
       +--> migration --> enter AMM phase; use the separately studied AMM exit
       |
       +--> no further exit --> use the rule's defined fallback / end-of-data treatment
```

The diagrams are research and runtime maps, not extra engine features. In the runtime tree,
phases move forward, situations are re-read on each engine step, and the cell supplies its
lines. The streak gate reads the group's prior coins before a buy; the AMM phase handles the
post-migration position.

```
  RULE A  max PnL          any cells, any lines
  RULE B  hold migrated    max PnL, on one condition: of the coins that migrate and that B
                           bought, at least H % are still held at migration. H is found
                           in W6 from the frontier of PnL against H. After migration: the
                           AMM exit.
  GATE    streak gate      an optional veto above both rules: no buy while recent group history
                           matches the studied adverse-session pattern. Found in W5 step 3 and
                           re-checked with the final entry (W5 step 6).
```

The trade shape, both rules:

```
  ONE buy per coin    at the first moment a buy cell holds   (no buy at launch by default)
  ANY part sells      each in a cell where selling part pays (a % of the first bag)
  ONE final sell      a sell cell, the owner's dump print, or the AMM exit
```

### What counts as success

Do not hide competing goals inside one PnL number. Report the rule against these priorities:

1. **Preserve migration upside.** Rule B targets the highest PnL subject to a held-share floor
   H for bought coins that migrate. Find H from the measured PnL-versus-held-share frontier;
   show how many migrating coins and how much AMM value each choice keeps or cuts.
2. **Avoid soon-dead coins at entry.** Show how many such coins the rule buys, skips, and how
   many migrations it also skips. The streak gate and entry conditions are candidates for this
   job; neither is assumed to work.
3. **Exit before harmful owner actions when the evidence supports it.** Report warning lead
   time before the dump-start label (owner bag sold at least 30%) separately from damage-control
   exits that fire after it.
4. **Improve total SOL after costs and execution effects**, with stability across days and
   without dependence on a few exceptional coins.

Rule A reports the unconstrained best-PnL candidate; Rule B reports the migration-preserving
tradeoff. If the evidence cannot meet a higher priority without harming another, show the
tradeoff and leave the choice explicit. Do not silently trade away migrations or accept a
weaker result.

Why this shape: a fingerprint's coins share a launch identity, but the owner may run more than
one recurring playbook. Our money comes from recognizing the current playbook and where the
owner is within it, then acting only when a measured trigger pays. One buy keeps the rule simple
and the slot count fixed; part sells can bank a gain and keep a runner for migration.

Two inputs the user fixes before the work:

- **Buy-size ladder.** The size now and the larger sizes the user may move to once the rule is
  stably profitable (5ix: 0.05, 0.1, 0.3 SOL). Rules are built at the first size and re-tested
  at each larger one (W2 step 7, W5, W7, W8). Size does not change phases or situations: they
  read the market, not our trade.
- **One rule per coin.** Several rules buying the same coin in its first second multiply our
  own money in the early pot and the loss on every bad coin (Key concepts, "Our own money is part of the game").
  The work assumes one live rule per coin.

---

## 2. Key concepts

These ideas carry the method. Everything else in the file serves them.

### The core idea: discover the games, then split each coin's life by the owner's playbook

A fingerprint identifies a launch group; it does not guarantee that the owner behaves the same
way on every launch. A group can contain several recurring playbooks, and the workflow must
find them rather than assume a list such as fast dump, mid dump, and migration. A playbook is a
recurring pattern in the owner's choices and responses to the pool and outsiders. Its name is a
plain-language summary, not a field the rule reads.

The method studies owner action sequences across coins to propose playbooks, then splits each
coin's life into meaningful steps and situations. The rule recognizes the current path only
from information available at that moment. Later outcomes evaluate whether the recognition and
action were useful; they never define a live state. If the data does not support a distinct,
recurring playbook, the workflow does not force one.

Two changes must remain separate:

- **Mix change:** the owner launches more coins through one already-recognized playbook. This
  changes its frequency, not its meaning. Report results per playbook and recompute the total
  under alternate playbook proportions.
- **Policy change:** the owner's choices or responses inside a playbook change, so its behavior
  sentence is no longer true. Re-check the playbook logic; do not hide this change by moving
  only a threshold. A change in outcome or rule PnL by itself does not prove that the owner's
  policy changed.

```
  MEANING FIRST, METRIC SECOND
     every phase, situation and trigger starts as a sentence about the owner or the crowd
     ("he has nearly paid the pool, his payday is close"), and only then gets the exact
     metric that reads it. A metric with no sentence behind it is not a rule part.
```

Three layers, each answering one question:

| Layer | Question | What it is | Example (5ix) |
| --- | --- | --- | --- |
| **Phase** | where in his game are we | a step after which his goal and his options change; never goes back | opening (he builds the coin), contest (he and the outsiders trade), near migration (payday is close), AMM (payday) |
| **Situation** | what is happening now in that step | one human act, his move or the crowd's answer | he pushes; he sells into outsiders; outsiders rush in; outsiders leave; quiet; falling; dead |
| **Trigger** | act now? | a live condition made of 1 to 3 metrics, tied to one story | "outsiders just arrived AND he has not sold yet AND the pool is near the top -> he is about to sell into them: sell" |

**Phase x situation = a cell.** A cell is a context. It says where he is and what he is doing;
it never says "act". A cell with no trigger takes no action: "everything else" (REST) is such
a cell, and no rule buys or sells because a coin merely sits in it.

### The loop

Inside one coin, each discovered playbook has a behavior loop. The loop is a hypothesis to test,
not a universal sequence that every coin must follow:

```
  OWNER PUSHES --> OUTSIDERS BUY --> OWNER SELLS ON THEM --> OUTSIDERS LEAVE --+
       ^           (fresh money)     (shake-out or THE DUMP)                   |
       +---------------------------------------------------------------------+
```

- Outsiders (snipers, traders, us) bring money the owner may trade against.
- Most coins are **bait**: he nets a few SOL from outsiders and lets the coin die.
- **Migration is his payday**: the curve fills, the coin moves to the AMM, and he sells into the
  AMM buyers.
- We test where an action is useful in each path; buying before a push or selling into the
  crowd is a candidate story, not an assumed profitable rule. W2 measures behavior first; W3
  and W4 define the live phases and situations from the supported findings.

### Stage tree: the rule format

The engine's format: a coin moves through **phases**; on every row one **situation** is true
(the first whose condition holds); each **cell** (phase x situation) owns its lines: buy, part
sell, sell, hold. A line is a trigger.

- **Phase.** Cut on a measure that never falls (age, peak pool), so a coin only moves
  forward. A cut is chosen where his behaviour measurably changes (W3), and the cut's metric is
  only the live reader of that step. The AMM is the last phase.
- **Situation.** One human act, in exact metrics, ordered so the most urgent matches first
  (dead, falling, then the flows). Two situations that act alike in a phase are merged (W4).
- **Trigger.** Inside a cell, a combination of 1 to 3 metrics that fires the line on the first
  row where it is true. It is found and judged in W5, in SOL, inside the whole rule.
- **Cell value.** In SOL with real fills, for each trigger: what buying, part-selling or
  selling at its moment is worth. The rule is read off these values (W5, W6).

### A trigger is a combination of metrics, with a story

One metric rarely says a story. "He is about to sell into the crowd" needs the crowd's arrival,
his silence and the height of the pool together. So a trigger is built from the story, not
from the single best metric:

1. Write the story and name its pieces (before any test).
2. Each piece gets one exact metric. Pieces are added one at a time; a piece stays only if the
   trigger's gain in SOL falls without it (W5, "Grow").
3. At most 3 pieces. A longer list is a fit.

### The rule loop: why exits come before the entry

The parts of a rule (gate, entry, curve exits, AMM exit) change each other's value, so none is
settled alone and none is frozen early. They are found in one loop (W5), in an order that
follows what depends on what:

```
  1 AMM EXIT     selling after migration does not depend on how we bought       -> first, fixed
  2 EXITS        under a reference entry (the snipe: a position in every state)  -> last phase back to the first
  3 GATE         its value is the loss it avoids under the real exits           -> after the exits
  4 ENTRY        a buy is worth what its exits make, on the coins the gate lets in -> after both
  5 EXITS AGAIN  on the positions the entry really opens
  6 STABLE       repeat 4-5; re-check the gate with the final entry; cross-week number
```

- **Selling does not depend on the entry price.** At any row, "sell now or hold" compares
  what the bag fetches now with what it fetches later; what we paid is spent. So the best
  exit at a row is the same for every entry, except through WHICH coins the entry picks.
  This is optimal stopping: the value of holding first, from the end of the life backwards;
  the entry where that value beats the price.
- **The snipe is the reference entry**, not a proposal to snipe: it holds a position in every
  state of every coin, so every exit is measured. Step 5 corrects for the coins the real entry
  picks.
- **The gate comes after the exits** because its value is the loss it avoids under the real
  rule, and **is re-checked after the entry** because it protects early buys: under a later
  entry it can only skip coins that were never in danger (5ix: with an entry at a peak pool of
  40 SOL the gate lost 0.15-0.94 SOL a week).
- **The AMM exit is not re-tuned** in the loop (it does not depend on the entry); it is re-run
  only when the data grows.

### The AMM exit

When to sell a migrated coin: W5 step 1, the first step of the loop, because it sets what
holding to migration is worth. Without AMM trade data the loop runs on a placeholder (the
bag at migration is worth the last curve price minus the AMM fee) and every number carries
that mark; rule B is "not measured" until the AMM tape is backfilled and step 1 runs for real.

### Held share H (rule B)

Rule B's share of bought migrating coins still held at migration. The user does not pick it:
it is read from the frontier of PnL against H (W6). The user may overrule the pick.

### Streak gate

A veto above every rule, read across the group's coins rather than inside one coin.

```
  EVENT   a per-coin condition list that marks a studied adverse game/session
          (for example, a fast pool death; the event is found for this fingerprint)
  GATE    no buy while the last N eligible, settled coins of this group, within M minutes,
          all match the event
  RESUME  by itself: the first recent coin without the event breaks the streak
```

Why: some owners alternate between a normal loop and a **farming session** in which they
launch coin after coin and kill each within seconds, taking the early outsiders' money. In a
session no entry condition and no exit helps: the whole loss is one owner sale at 2-4 s, and
migration does not happen (it would cost him the money he is farming). Sessions are visible
only across coins, so the gate reads the group, not the coin.

- Discover the gate event from the group's recurring adverse behavior and its live coin
  conditions; do not assume one event for every fingerprint. For a fast-dump session, pool
  death within a group-appropriate time is a candidate because it measures the harm directly.
  Owner actions, outsider behavior, and pool conditions are also candidates when their story
  explains why repeated results predict a bad current session. Reject conditions that are
  common on healthy or migrating coins, fire too late to protect entry, or merely restate an
  outcome without a useful decision-time meaning.
- A coin counts only after its T s have passed (live only), and only within M minutes (a
  streak ends with its session).
- A condition alone is not a useful gate event merely because it names a bad outcome. Check
  its prevalence on healthy, losing, and migrating coins and whether it settles early enough
  for the gate to help the next buy.
- Event conditions and their thresholds, N, and M are found per group in W5 step 3, judged in
  SOL inside the whole rule, and re-checked with the final entry (W5 step 6). For a fast-death
  event, test time horizon T and any justified peak limit X; other event stories use their own
  meaningful threshold grid.
- The gate is a rule layer, not a metric: a metric is one coin's state at one moment, the gate
  reads the group's recent coins. Engine plan:
  [streak-gate-plan.md](../../roadmap/streak-gate-plan.md).

### Our own money is part of the game

The early outsider money on a coin is partly ours. When our rules (and any copy-trading
wallets of the user's team) buy in the first second, the early pot grows, and an owner who
farms big pots dumps faster. So: one rule per coin, a size ladder tested for impact, the
owner's reaction to our buy size measured (W2 step 7), and a gate that stops us feeding a
session.

---

## 3. Words

### Who

| Word | Meaning | Engine term |
| --- | --- | --- |
| **Fingerprint** | one creation shape (instruction labels, compute budget, spend). Its coins are one owner or one launch tool | `fingerprints` row |
| **Owner** | the wallets carrying the fingerprint's owner tag: the creator and his trading wallets | `@volume` (or the group's tag) |
| **Bundle** | wallets that buy in the creation slot together with the creator (5ix: 4 wallets, about 19.8 SOL in total) | found from the tape |
| **Outsider** | every trade without the owner tag | `@!volume` |
| **Us** | our trader wallets. Never tagged: they count as outsiders | - |
| **Dump** | the owner's dump print | `m_flow.trade_count @dump >= 1` |

The spend in a fingerprint's name ("spend=1") is the creator's creation buy, **not our buy
size**. Our size is the rule's own buy amount.

### Where

| Word | Meaning |
| --- | --- |
| **Curve** | the pump.fun bonding curve: the coin's first market. Fee 1.25 % a leg |
| **Migration** | the curve fills (real pool about 85 SOL) and the coin moves to the AMM |
| **AMM** | the coin's second market. Fee 0.95 % a leg. New wallets, bigger and faster swings |
| **Pool** | real SOL in the pool, `m_state.liquidity_sol` (on the curve: virtual SOL - 30) |
| **Death** | the pool falls far below its peak and never comes back; the live test is "70 % or more below the peak, or pool under 3 SOL" |

### When and what

| Word | Meaning |
| --- | --- |
| **Row** | one moment the engine evaluates: every print plus a tick every 0.2 s |
| **Decision / fill** | the rule decides at row T; the trade fills on the state after the last print at or before T + 115 ms |
| **Live state** | everything known at a row: read from trades at or before it only |
| **Label** | what happens after a row: read from trades after it only. Labels judge states; they never enter a rule |
| **Onset** | the first row of an episode of a state or of a trigger. A rule acts at an onset, so states and triggers are measured there. A coin that leaves and re-enters has one onset per episode |
| **Cell** | phase x situation: the context. It never acts alone |
| **Trigger** | 1 to 3 metrics, true together inside a cell, that fire a buy / part sell / sell line |
| **Story** | the one-line owner or crowd reason behind a phase, situation or trigger, written before it is tested |
| **Reference entry** | the snipe (a buy at every coin's first print), used in W5 step 2 to hold a position in every state while the exits are found. Not the final entry |
| **Rule loop** | W5: AMM exit, exits, gate, entry, exits again, until stable (Key concepts, "The rule loop") |
| **Baseline** | a label's normal rate in the same phase and age bucket. **Lift** = the label in a state minus its baseline |
| **Value** | per cell, in SOL with real fills |
| **Size ladder** | our buy sizes: now, and the larger ones the user may move to |

### Data

| Word | Meaning |
| --- | --- |
| **Regime** | a stretch of days where the owner's habit holds still (opening speed, early deaths, trades per coin). Found in W0 |
| **Recency window** | the days that decide a number: the newest week, widened to 10 days, then the whole regime, until the cell has 20+ coins ("The data rule") |
| **Frozen end** | the last study day. Days after it are the forward test |
| **Kernel** | the study's own simulator: our fill, impact and fees on the engine's rows |
| **Parity** | the kernel and the engine give the same PnL per coin on the same rule |
| **Benchmark** | the group's existing live rule. Read once, at the end |
| **Cross-week number** | choose on one week, test on the other; the test number is the honest one (L13) |

---

## 4. Laws

Every law has a reason; a law broken in a past case is named in that case's mistakes table.

| # | Law | Why |
| --- | --- | --- |
| L1 | **Live only.** A state reads the past; a label reads the future. Nothing known only in hindsight (a swing's top or bottom, the death time, whether the coin migrates) is a state. | a rule can never read hindsight; its numbers look better than any live rule |
| L2 | **Full state.** Read short AND long windows (0.5 s to 60 s and life), silence timers (seconds since his last buy / sell), the price path both ways (rise from the low AND fall from the peak), life-long state (pool, peak, his money in, who is in profit). | a short list misses his silences and the coin's history; rise without fall cannot see a top has passed |
| L3 | **Onset and coins.** Labels and values are read at onsets. Every table counts coins: each coin counts once per state. Every number rests on 20+ coins. | a rule acts on the first row; a long-lived coin's ticks swamp a row count |
| L4 | **Baseline and cost.** A label is shown as lift against its phase-and-age baseline, next to the round-trip cost. His next move is his net SOL, not "he sold". | he trades all the time, so "he sells in 1 s" is true almost everywhere; a lift smaller than the cost is not money |
| L5 | **Recency window.** The newest days decide ("The data rule"). A choice that pays only on older days is dropped. Days before the regime are not used. | the owner changes his habit; the newest days are the market the rule trades |
| L6 | **Four checks** for every factor and every line: a one-line story, it holds in the recency window, it holds on 20+ coins (not 2-3), it beats the best of shuffled factors. | a free search always finds something |
| L7 | **Story first, then test.** A phase, situation or trigger starts as a sentence about the owner or the crowd; the test only accepts or drops it. A search may find candidate pieces for a story and move a threshold inside its plateau; it never chooses which cells act. Every search result is followed by L13. | a free search over a template converges on luck or on the benchmark's ideas; a story written after the data fits any pattern |
| L8 | **Exact metrics.** The finding sets the metric. Use the engine metric that IS the term; if none exists, propose a new one (one line: what, unit, window). Never a near metric. A factor is not dropped because the engine lacks it. | a near metric changes the number that justifies the rule |
| L9 | **Real fills and our footprint.** Fill at 115 ms with our impact and fees per venue. A buy value includes the owner's reaction to a buy of our size, for every size on the ladder. | PnL is what the wallet moves; history never saw our trade; a larger buy moves the price more and can draw a reaction a small one does not |
| L10 | **Kernel = engine.** The kernel runs on the engine's rows, matches it coin by coin on a trivial rule before any study, and on the final rules before any report. | the study must measure what ships |
| L11 | **Benchmark last.** The existing rule supplies no idea, line or threshold during the work. It is compared once, in W9. | a rule bent toward the benchmark is the benchmark again |
| L12 | **Order.** The owner's loop before rules; then the rule loop (W5): AMM exit, exits under the reference entry, gate, entry, exits again, until stable. | selling does not depend on the entry price, so exits come first; the gate's value is the loss it avoids under the real exits; a buy is worth what its exits make |
| L13 | **Honest number.** Whatever a search chose, report the cross-week number: choose on one week, test on the other (both directions). An in-sample number is never the result. | a greedy exit set scored +3.05 SOL on the data it was chosen on and +0.51 / -0.23 across weeks: most of it was fit |
| L14 | **Avoid, don't outrun.** A loss that is one owner sale (a farming session, a launch bundle sold at 2-4 s) is not fixed by a faster exit. Skip the coin (gate, entry) instead. | every exit tried lost on those coins and cut the gains on the others |
| L15 | **Meaning first.** Every phase boundary must change his behaviour (measured); every situation is one human act; every trigger has its story. A phase or situation with no meaning is dropped, and two that act alike are merged. | pool plus flow cuts without a meaning are only metric conjunctions: they add search, not sense |
| L16 | **Money in the whole rule.** A trigger is judged in SOL after costs and the 115 ms fill, played inside the whole rule (one buy per coin, the other triggers on), never by a price lift alone and never cell by cell. A trigger stays only if the rule's money rises with it. | a price lift is not money; an earlier trigger in another cell takes the coin first, so values do not add up |
| L17 | **Combinations, grown.** A trigger is 1 to 3 metrics that tell one story. Grow it from the story's pieces one at a time; every piece must pay its way, and the whole search is judged against the best of shuffles. | a story often needs several metrics, but all pairs and triples of 100 metrics is a luck search |
| L18 | **No part is frozen.** Gate, entry and exits are judged together in the whole rule; a part found under one entry is re-checked under the final one before it ships. | a gate found on a snipe lost money under a later entry; buys searched before the exits existed looked worthless |

---

## 5. The data rule

```
  REGIME        days over which the owner's behavior is comparable (W0). Older regimes do not
                define the current rule.
  FROZEN END    the last study day used for discovery and selection. Later days are the forward
                test (W9), read once and never used to retune that rule.

  For every number (a cell's lift, a cell's value, a line's gain):
     newest 7 days    -> if the cell has 20+ coins, they decide
     newest 10 days   -> otherwise
     whole regime     -> otherwise
     still < 20 coins -> the cell is too thin: no rule line there

  Every number is shown with the newest week beside it. A choice the newest week
  contradicts is dropped.
```

The newest data is the primary research window because it best reflects the owner's current
habits. Cross-week checks inside that window are robustness checks, not untouched final tests:
both weeks can influence the research choices. The forward period starts after the frozen end
and remains untouched until W9. If the forward result changes a rule, that period becomes
research data; freeze a later period for the next final check.

The study's data keeps growing while the work runs. Re-read the newest days from the server
(W0 step 11) before the final steps, and treat days added after the frozen end as the forward
test.

---

## 6. The workflow at a glance

```
  PART 1  DISCOVER THE OWNER'S PLAYBOOKS   what repeats in his behavior, without preset classes
  W0  DATA         coins, trades, labels; kernel = engine; regime; frozen end; live review
  W1  LEDGER       who pays the owner, by venue and outcome       -> incentives and constraints
  W2  LOOP         live state -> his next move, price, death      -> candidate playbooks       [C1]

  PART 2  MAKE THE PLAYBOOKS LIVE-READABLE   (meaning first, metrics second)
  W3  PHASES       meaningful steps in a coin's life; live boundaries where behavior changes
  W4  SITUATIONS   the current owner/crowd act; exact metrics; merge situations that act alike
                                                                                              [C2 phases + situations]
  PART 3  FIND THE RULE: THE RULE LOOP   (every part in SOL, in the whole rule)
  W5  step 1  AMM EXIT      own study on all migrated coins; fixed
      step 2  EXITS         under the reference entry (snipe), last phase -> first; rule B draft
      step 3  GATE          T, X, N, M judged with step 2's exits                              [C3 AMM exit + exits + gate]
      step 4  ENTRY         triggers with gate + exits + AMM exit on, latest phase -> earliest
      step 5  EXITS AGAIN   on the positions the entry opens
      step 6  STABLE        repeat 4-5 (max 3); gate on / off / neighbours; cross-week number  [C4 loop result]
  W6  RULES        A and B as sentences; H from the frontier                                  [C5 rules]

  PART 4  PROVE AND SHIP
  W7  ROBUSTNESS   plateaus, stress, cross-week, every size
  W8  ENGINE       documents, gate layer (approved plan), parity                              [C6 robustness + engine]
  W9  BENCHMARK    compare once; forward days; save when asked                                [C7 benchmark]
  W10 MAINTENANCE  weekly re-run, live vs paper, retire rule
```

At a checkpoint (C1 to C7) the user sees the result and its story and says go before the next
step. No fixed game categories enter W1-W4: evidence proposes the recurring playbooks, and
live-readable metrics make their paths. Outcome labels are added for evaluation, not used as
phase, situation, or game-recognition inputs. Part 2 comes before Part 3 because a trigger is
searched inside a cell: the cells must be meaningful first. Part 3 is one loop because its
parts change each other's value (Key concepts, "The rule loop").

| Checkpoint | After | The user decides |
| --- | --- | --- |
| C1 | W0-W2 data and candidate playbooks | coin counts, days, regime, frozen end, and behavior-based playbook descriptions look right; outcome names remain evaluation labels |
| C2 | W3 + W4 | each phase and situation as a sentence with its metric: keep, merge, drop |
| C3 | W5 steps 1-3 | the AMM exit, the exits under the reference entry, the gate (T, X, N, M) |
| C4 | W5 steps 4-6 | the entry, the final exits, the gate re-check, the cross-week number of each round |
| C5 | W6 rules | the shapes of A and B, and H (the user may overrule) |
| C6 | W7 + W8 | size ladder and engine parity |
| C7 | W9 | benchmark comparison, forward days, saving as paper |

---

## 7. The steps

### W0. Data

**Goal:** the same coins, trades and labels the engine sees; a kernel that plays like the engine;
the real record of what our rules did.

1. **Coins.** `tokens` rows whose labels equal the fingerprint's creation shape. Count per day in
   the DB, in the study cache, and in a lab simulate of a trivial rule: all three agree.
   A day with no coins is not a day with no data: check the DB holds trades that day.
2. **Trades.** Every curve and AMM leg per coin, in chain order (`slot, tx_index, leg_index`).
   Postgres stores raw token units (6 decimals) and lamports: check one coin's price both ways.
3. **AMM tape.** Migrated coins need their AMM trades for at least 1 h after migration:
   `cargo run -p hunter-live -- sync-mints --incremental --amm-slots N <mint>...` (N = 1 h of slots).
   **Helius spend needs the user's approval**: backfill one coin, show the cost, then ask. The
   user may approve a scope (for example "migrated coins of recent days").
4. **Lake.** The lab reads the Parquet lake: `cargo run -p hunter-lab -- lake-export`, then
   restart the lab (it keeps old trades in memory). The lake keeps `LAKE_RETENTION_DAYS` days and
   an export prunes older days. `.env` changes only with the user's yes.
5. **Labels.** Owner and dump labels from the engine's own metric rows
   (`/api/tokens/{mint}/metric-series` with the fingerprint): the study's owner is the engine's.
6. **Tag check.** What the owner tag really holds: its wallets' mint count across the market
   (wash wallets trade a handful of mints, outsiders dozens), whether the creator's own bag is
   in it, whether it stays on the AMM (`sticky`). An untagged read (`m_crowd.unique_wallets`)
   counts his wash wallets. Find the **bundle** (wallets buying in the creation slot).
7. **Kernel.** Rows = the engine's rows (prints and 0.2 s ticks). One buy at any row, any number
   of part sells (`sell_pct` of the first bag, bounded by `m_position.sold_pct`), the AMM leg,
   impact on the priced reserve, fees per venue, the engine's pre-entry veto (no buy on a row
   where a sell line already holds) and the engine's slot cap.
8. **Parity on a trivial rule** with part sells and an AMM leg: 90 %+ of coins within 0.003 SOL.
   Whether the engine buys on ticks or only on prints is read from this check, never assumed.
9. **Replayable days.** Count the trivial rule's entries per day in the lab. A day with 0 entries
   is outside the lake.
10. **Regime.** A habit table per day and per session (hour block): coins, deaths < 3 s and < 10 s,
    owner trades in the first 3 s, life, peak pool, migrated. The regime starts at the last
    clear change. Fix the frozen end. Write both down before any outcome is measured.
11. **Live review** (read-only; skip when the group has no live positions).
    - **Access.** The server DB is read through ssh and `docker exec` into the Postgres
      container. The ssh key's passphrase is loaded into an ssh-agent for the session and is
      never written to a file or a doc. No server write, no rule change.
    - **Positions.** `strategy_positions` joined to `strategy_rules` (`mode = 'real'`, the
      group's fingerprint): entry, exit, PnL (`exit_sol_lamports_total - entry_lamports`),
      `exit_reason`, status. Count `EntryFailed` and manual exits separately; they are not rule
      results.
    - **Tape.** Per losing coin, read the trade tape around our buy: who bought early (us, the
      user's team wallets, others), how much, and when the owner sold. Compare with the
      group's normal coins.
    - **Hourly habit** over the live days: share of coins that died fast, migrations, early
      outsider money. A change in the habit marks a new regime or a farming session (the
      gate, W5 step 3).
    - The study cache holds the same coins and days as the live record, so a live loss can be
      replayed in the kernel.

**Output:** coin table, row cache, kernel with its parity number, regime, frozen end, the
replayable days, the live record.

### W1. Ledger

**Goal:** what pays the owner, so we know what he protects and what he spends.

- His net SOL per coin, by venue (curve / AMM) and by fate (dies fast, dies later, migrates).
  Outsiders' net the same way.
- **Payday** = the coins and moments that pay him; **bait** = what he spends to bring outsiders.

**Output:** one paragraph: what rule A can take (his bait swings) and what rule B rides (his
payday).

### W2. Curve loop

**Goal:** at this moment, from what is already known, what does he do next, how soon, and
what does the price do after.

**Playbook discovery:** find recurring owner policies from the tape; do not start with a fixed
menu of outcome names or assumed game types. A playbook describes a repeated policy pattern
(what the owner does under which conditions). An outcome label describes what later happens to
a coin. These are different things.

1. Read owner and outsider activity, pool state, and price path across whole token lives and
   across launches. Compare the owner's choices and responses, not only final PnL or coin
   outcome. Look for recurring sequences and the conditions around them.
2. Write each candidate playbook as a sentence about the owner's behavior and incentive. Keep
   the wording provisional until it recurs across days and is distinguishable from other
   candidates. Do not call a token a "fast-dump game" merely because it later died quickly.
3. For every candidate, identify signs visible before the relevant owner decision. Record the
   first time those signs make the candidate recognizable. If recognition is possible only
   after the outcome or after the action being predicted, it cannot control that decision; it
   may still describe the path retrospectively.
4. Measure the later owner actions and outcomes for each live-recognized candidate. Report
   sample size, days, migrations, deaths, and money so that a rare case or one burst cannot
   masquerade as a recurring playbook.
5. Merge candidates whose owner behavior and useful response are alike. Keep distinct
   candidates separate only when they imply a meaningfully different path or action. Leave
   uncertain or thin cases unclassified rather than forcing every coin into a story.
6. Keep factor discovery open to the registry and new factors found in the tape. A missing
   exact engine read is proposed as a metric; a near metric does not stand in for it.

The candidate playbooks are a research aid, not a new runtime field. The deployed tree still
reads phases, situations, and triggers. A playbook distinction is actionable only when those
parts express it from live information. If the distinction requires memory across events or
coins, use an existing rule layer such as the streak gate or propose and approve the needed
stateful feature; do not imply the stage tree stores that memory.

**Playbook output:** behavior sentence, live precursor, recognition time, later owner response
and outcomes, recurrence across days, sample size, and whether the engine can express the
distinction exactly.

1. **Rows:** every engine row on the curve. **Dead first:** a live dead test (price 70 % or more
   below its life peak, or the pool under 3 SOL once the peak is 18 or more) splits off dead
   rows; the rest is measured on alive rows only. Dead ticks fill most rows of a coin's life.
2. **State** (past only, the full set of L2):

   | Group | Factors |
   | --- | --- |
   | his flow | buy / sell / net SOL and trade count over 0.5, 1, 2, 5, 10, 30, 60 s; last trade side and size; same-side run; seconds since his last trade, buy, sell |
   | outsiders | the same windows; largest single buy; seconds since their last buy / sell; their paper profit |
   | price path | rise from the window low AND fall from the window peak (1 s to 60 s and life) |
   | life-long | age, pool, peak pool, distance to migration, his money in, his paper profit, his bag sold, dump seen |
   | habit | the fate of his last coins, as known at this launch (the gate reads this) |

   This list is the start, not a limit: a factor found in the data joins it.
3. **Labels** (future only): his net SOL over the next 1 s and 3 s; price at our fill after
   1 / 3 / 10 s; death within 3 / 10 s; migration.
4. **Tables:** per factor, quantile bins -> labels as **lift vs the phase-and-age baseline**, at
   **onsets** (first onset per coin, phase and bin), **per coin**, the newest week beside the
   window, the round-trip cost beside the price lift. A baseline by the coin's own mean is
   wrong (a coin's first row differs from the rest); use phase x age bucket, coins weighted
   equally. Floor a cell's spread so a constant label gives no huge t.
5. **The four checks** for every factor kept:
   1. **Story:** one line on why he acts this way ("outsiders buy near the top -> he sells into them").
   2. **Recency:** the lift points the same way in the newest week.
   3. **Breadth:** 20+ coins; not carried by 2-3.
   4. **Shuffle:** the factor's series is swapped between coins of the same day (row position
      kept, so the coin's time structure stays), the whole search is re-run about 30 times, and
      the real cell beats the **best** |t| of the null runs. The number of factors tried is
      written down. A factor tied to time-in-phase also passes the shuffle's time structure:
      read it as a time effect, not a factor effect.
6. **Combined states:** the few combinations of kept factors that differ (for example his move
   now x outsiders now x fall from the peak).
7. **Our footprint:** his move after an outsider buy of **our size** (each size on the ladder,
   plus or minus 20 %) in the same state, against outsider buys outside the size groups in the
   same phase and age bucket. The difference is the reaction cost W5 charges each buy (the same
   for our sells). Our own buy also counts as an outsider buy in live flow reads: at each
   size, check that it cannot flip our own situation (thresholds are absolute SOL, for example 0.5).
8. **Payday leg:** how the coin reaches migration; the AMM goes to W5 step 1.

Swings (zigzag tops and bottoms) may describe a coin's life; they never feed a state.

**Output:** the kept factors, each with its story, lift, coins and newest-week number. These
are the pieces W3 to W5 build from.

### W3. Phases: the steps of his game

**Goal:** split the coin's life where the owner's goal and options change for good. Apply the
same phase structure across discovered playbooks when its meaning holds; create a distinct
phase only when the owner's step or available choices actually differ. A phase is a sentence
first ("he has put in enough, the next step is to harvest"), a cut second.

1. **Name the steps** from W1 and W2: what is he doing at the start (building), in the middle
   (trading against the crowd), near the end (paying the pool to the top), after migration
   (cashing out). Read 20+ coins' tapes with his ledger if the steps are not clear.
2. **Find a live reader** for each step boundary: a measure that never falls (`m_state.age_sec`,
   `m_state.peak_liquidity_sol`, a life count with an exact metric). Factors that say the same
   thing (pool, his money in, age) give one measure, not three.
3. **Test that the boundary is real:** his behaviour on each side differs (his next move, death,
   migration, the price after) in the newest week, on 20+ coins per side. A boundary that
   changes nothing is dropped; two phases that behave alike are merged.
4. **Plateau:** moving a cut by a small step moves each phase's chances by 3 points or less. A
   cut on a steep step (his behaviour changes right there) sits where the step ends.
5. A factor that still matters inside every phase (for example age) is a situation factor.

**Output:** 3-6 phases, each with its meaning, its cut, its chances and its story; the AMM is
its own last phase.

### W4. Situations: his moves and the crowd's answers

**Goal:** inside a phase, what is happening now. Each situation is one human act, then an exact
engine reading of it.

Situations are the live branch points that let the tree respond differently to behavior
patterns discovered in W2. They are not outcome buckets. A condition such as "price eventually
dies" is a label, not a situation; a live condition such as "the owner is selling while
outsiders are net buyers" can be a situation if its meaning and measured boundary hold.

1. **Name the acts** from W2: he pushes; he sells into outsiders; outsiders rush in; outsiders
   leave; nobody trades; the price falls; the coin is dead. Each is a sentence about a person.
2. **Read each act with exact engine metrics** at decision time, built only from factors that
   passed W2. A situation can combine metrics (his net and the outsiders' net and the fall).
   A kept factor with no engine metric gets a proposed metric (what, unit, window, family per
   the metric rules); the line waits for it.
3. **Order:** the most urgent first, **dead first**, then the price falling from its peak, then
   the flows. The fall is in the list: a "not falling" situation looks good only because the
   baseline holds falling rows. The last situation is "everything else" (REST): no rule acts
   there by itself.
4. **Merge** situations that act alike in a phase (same labels, W2 lifts within noise); keep
   one of overlapping factors. Too many situations leave cells with fewer than 20 coins.
5. Check the study's reading equals the engine metric row by row (for example 99.9 %+ within
   0.01).
6. Logic checks per condition: a window fits inside the age where it is read; a lifetime read
   means "since launch", not "since we bought"; a tag can fire before the decision; the
   direction matches the story; it is not true for most coins at the fill.

**Output:** the cell grid: phases x situations, each cell with its meaning, its coins and its
lifts. Cells with fewer than 20 coins in the recency rule are merged or marked thin.

**Checkpoint C2:** the user sees every phase and situation as a sentence, its metric and its
numbers.

### W5. The rule loop

**Goal:** the whole rule (gate, entry, curve exits, AMM exit), found together, because each
part's value depends on the others (Key concepts, "The rule loop"). Every part is judged by one
number: the whole rule's SOL after costs at the 115 ms fill (L16), with the cross-week
number beside it (L13).

```
  step 1  AMM EXIT      own study on all migrated coins                          fixed
  step 2  EXITS         reference entry = snipe; cells last phase -> first       rule B draft
  step 3  GATE          judged with step 2's rule                                [C3]
  step 4  ENTRY         gate + exits + AMM exit on; triggers latest -> earliest
  step 5  EXITS AGAIN   on the positions step 4 opens
  step 6  STABLE        repeat 4-5; gate on / off / neighbours; cross-week       [C4]
```

#### What a trigger is, in every step

```
  context  = cell (phase x situation)        where he is, what he is doing
  trigger  = 1-3 metrics true together       "now is the moment" (with its story)
  value    = SOL the whole rule gains with it, after costs, at the 115 ms fill
  no trigger found in a cell = no action in that cell (REST usually)
```

- **How it acts.** The rule fires on the first row inside the cell where the trigger is true
  (a buy when we hold nothing, a sell when we hold). If the coin leaves the cell and the
  trigger becomes true again, that is a new onset.
- **Story first.** For each cell, write the candidate triggers as sentences before any test
  ("he just sold, outsiders are still buying, the pool is near migration: he will push again,
  buy"). Candidates also come from the W2 kept factors and from a scan of the cell's rows; a
  scan result without a story is a lead to write a story for, and is dropped if none fits.
- **Pieces.** Live factors of every kind, including the owner's launch game (bundle size and
  wallets, creation-slot spend, the first outsider buys, his history across coins). Each
  piece is an exact engine metric or a proposed one (L8).
- **Grow.** Start with each single piece of the story at its best threshold inside its
  plateau; add the second only if the trigger's SOL gain rises and the piece is in the story;
  then the third. Stop at 3. Each piece must pay its way: drop it and the gain falls.
- **Value for one coin:**

  | Line | Value |
  | --- | --- |
  | **Sell** | proceeds now minus proceeds under holding, with the rest of the rule on |
  | **Buy** | the trade's proceeds minus what we paid, with the current exits, fees, our impact and the W2 reaction cost at the size |
  | **Part sell** | the same as sell, for a part; linear in the part, so a part sell is a choice about risk |

- **Accept.** One candidate at a time, in the order of its SOL gain; it stays only if the
  whole rule's money rises (L16) in the recency window and does not fall in the other week.
  The rule is played with all accepted lines on every time, because one buy per coin means
  values do not add up.
- **Luck.** The four checks (L6) hold for every accepted trigger; the shuffle re-runs the whole
  growth search on shuffled factors and the real trigger beats the best shuffled one. The
  count of combinations tried is written down.

#### Step 1. AMM exit

**Goal:** when to sell a migrated coin. It sets what holding to migration is worth, and it
does not depend on how we bought, so it is found first and not re-tuned in the loop (it is
re-run when the data grows).

- Rows: the curve's last 120 s plus the AMM's first hour. Sells only on AMM rows, after the last
  curve print. All migrated coins of the recency window (widened by "The data rule" when few).
- The value of an exit is its proceeds minus selling at the first AMM row, for a position worth
  a fixed stake at migration. Every exit needs a fallback; an exit with none holds to the
  end and loses everything.
- Basic metrics first: price moves, the fall from the AMM peak (`m_price.trail_pct [amm]`), the
  rise from the AMM low (`m_price.rise_pct [amm]`), short spikes and crashes
  (`rise_pct` / `trail_pct [10s..60s]`), volume and speed (`gross_sol`, `trade_count`), time
  since migration (`m_position.stage_sec` in the AMM phase). Owner and outsider flows second.
  Part sells (bank part at the first AMM row, trail the rest from the AMM peak) are candidates.
- Windows `[ws]` cross the venue: for the first w seconds after migration they still hold the
  curve's tail. The study reads the same.
- For each exit read the total, the total without the best 2 coins, coins better / worse than
  selling at migration, leave-one-coin-out, the neighbours (a region of settings, not one
  point), and the cross-week number. With hundreds of combinations, an exit that adds a few
  tenths of a SOL over 30 coins is noise.
- A crash backstop is wider than the normal post-migration shake-out.
- Every AMM line carries `m_state.on_curve = 0`: the AMM phase begins on the last curve print,
  and a line true there sells into a finished curve that cannot fill.
- **No AMM data:** the placeholder of Key concepts, "The AMM exit" (last curve price minus the AMM fee); every
  later number carries the mark until the tape is backfilled and this step runs for real.

**Output:** the AMM exit (often "sell at the first AMM row") with its numbers.

#### Step 2. Exits under the reference entry

**Goal:** the sell and part-sell triggers of every curve cell.

1. **Fixed lines:** the owner's dump print (always), the death exit, the AMM exit (step 1).
2. **Reference entry:** a buy at every coin's first print (the snipe). It holds a position in
   every state, so every exit is measured. It is not the final entry.
3. **Cells from the last curve phase back to the first.** A later phase's exits set what
   holding through an earlier phase is worth, so they come first. Per cell: stories, grown
   triggers, the acceptance test above.
4. **Coins killed by one owner sale** (a launch bundle sold at 2-4 s) lose on the reference
   entry whatever the exit. Do not tune an exit to them (L14): they are the gate's and the
   entry's job. Exits in the first cells are re-valued in step 5 on the coins the entry keeps.
5. **Rule B draft:** rank the accepted sell triggers by SOL gained per migrating coin sold. B
   is A with the cheapest of them turned to part sells or holds; H is read in W6.

#### Step 3. Streak gate

**Goal:** stop buying during a repeated adverse group pattern when the gate adds value under
step 2's exits and the final entry (Key concepts, "Streak gate").

1. **Gate event:** choose a small, story-led set of per-coin condition lists from W2's
   discovered adverse modes. Each condition uses a live engine read with its own time limit.
   For fast dumps, include pool death within candidate horizons (for example 3, 5, 10, 20, 30,
   or 60 s) and an optional low-peak condition when the story says little outsider money came
   in. Do not assume this event is right for every fingerprint.
2. **Memory per new coin:** the last N eligible coins whose event can be decided and that
   launched within the last M minutes. The gate is on when all N had the event. Keep unsettled
   coins out until the event is true or can no longer become true.
3. **Grid:** tune event thresholds, N, and M around the event's story. For the fast-death
   example, candidate values include N = 2, 3, 4 and M = 10, 30, 60 min; event horizons and
   peak limits follow item 1. Other events use their own bounded, meaningful grids.
4. **Value** = the whole rule's SOL with the gate minus without it (reference entry, step 2's
   exits). Beside it: coins skipped, **migrated coins skipped**, the skipped coins' death rate
   against the kept coins', each week alone.
5. **Pick** a setting on a plateau (neighbouring values give similar results) that saves in both
   weeks and skips no migrated coin, or at most 1 when the saving is clearly larger. No setting
   that saves: no gate.
6. **Live watch:** the gate is re-tuned when the owner's habit changes (W10).

The gate ships as a rule layer, not a metric: [streak-gate-plan.md](../../roadmap/streak-gate-plan.md).

**Checkpoint C3:** the AMM exit, the exits under the reference entry, the gate with its table.

#### Step 4. Entry

**Goal:** the buy triggers, with the gate, the exits and the AMM exit on.

- **Candidates:** the snipe with avoid conditions from the first seconds (the launch game: his
  bundle, his creation-slot spend, the first outsider buys); later triggers in each cell, each
  with its story ("a pullback after his sale while the pool is near migration").
- **One buy per coin:** an earlier trigger takes the coin before a later one sees it. Accept
  triggers from the latest phase back to the earliest: a later entry is the "wait" alternative
  of an earlier one, and an earlier trigger stays only if the whole rule's money rises with it.
  A launch buy is a trigger like any other and must win.
- **A whole cell is not an entry.** "Buy in P2" without a trigger and a story is a filter; it
  is written as a trigger (its pieces, its story) or dropped.

#### Step 5. Exits again

Re-value every exit (accepted or rejected in step 2) on the positions step 4's entry really
opens, and add or drop by the same test. The entry picks some coins, and an exit that paid on
all coins can stop paying on these.

#### Step 6. Until stable

1. Repeat steps 4 and 5 until no trigger is added or dropped, at most 3 rounds; stop earlier
   if the cross-week number does not rise (a loop also fits noise).
2. **Gate re-check:** the final rule with the gate on, off, and at its neighbours (T, X, N one
   step each way). It stays only if it still adds SOL in both weeks.
3. The AMM exit is not re-tuned.
4. **Report per round:** the rule, its SOL in the recency window without the best 3 coins, and
   the cross-week number (choose on one week, test on the other, both directions).

#### Sanity

- Cells with fewer than 20 coins in the recency rule hold no trigger.
- Expect most cells to hold no trigger: a market with no edge in a cell shows value near zero,
  and that is the finding. The edge, where it exists, comes from a few triggers.
- Where the money comes from: migrated coins against the rest, per week. A rule that is only a
  bet on migration says so.
- Every accepted trigger: its story, its pieces, SOL gain with and without the best 3 coins,
  the cross-week test, coins, newest-week number.

**Checkpoint C4:** the entry, the final exits, the gate re-check, the cross-week number of every
round.

### W6. Rules

- Rules A and B are written from the loop's final rule (W5): `enter.cells` = the cells that
  hold a buy trigger; cell lines = the part-sell and sell triggers; `always` = the dump-print
  sell; the last phase `amm` = the AMM exit (W5 step 1). The streak gate, if step 6 kept it,
  sits above both (entry veto).
- A buy trigger is more than its cell: it has no slot in `enter.cells`, so split the situation
  instead (a new situation above the old one, `when` = the old situation AND the trigger's
  pieces). The cell then is the trigger's own context.
- Every line has its one-line owner story and its pieces. A line without a story is dropped.
- After W6 the user sees C5: both rules as sentences (cell, trigger, story), numbers beside.
- **Finding H (rule B).**
  1. Rule A's exit lines that sell migrating coins are ranked as in W5 step 2 (rule B draft). Turning them to holds
     one by one, in that order, gives the **frontier**: for each held share from the A value
     up to 100 %, the best PnL.
  2. Read it in the recency window: the PnL lost on the curve by holding, the PnL gained on
     the AMM by holding, and the coins carrying each (with and without the best 3).
  3. H sits on a plateau of the frontier, just before PnL falls steeply. It must not move when
     the window moves from 7 to 10 days, and the PnL there must stay positive without the best
     3 coins.
  4. Show the user the table and the pick; the user may overrule it.

### W7. Robustness

- Each threshold moves only inside its plateau and sits in its middle (the range that keeps
  90 % of the best gain). A one-point peak is noise.
- Each line: a gain in the newest week; 40 %+ of the gain left without the best 3 coins; 8+
  trades changed; leave-one-day-out inside the window keeps it in most folds.
- **Cross-week (L13):** the rule's selection is repeated on each week alone and tested on the
  other; the report shows both test numbers.
- **Game-mix check:** freeze the live path definitions and report trades, SOL, migration
  retention, avoided losses, and missed opportunities separately for each discovered
  playbook. Recompute the aggregate under clearly stated playbook proportions, including the
  observed recent mix and balanced proportions. Reweight measured per-playbook contributions;
  do not invent trades or treat a thin playbook's estimate as reliable. This checks how
  frequency changes affect the total while holding each playbook's measured behavior fixed. It
  does not claim the rule survives changed behavior inside a playbook.
- **Behavior-stability check:** compare each playbook's live signs and owner responses across
  recent subperiods and available earlier regimes; report outcomes and rule results separately.
  If the owner's choices or responses change enough that the playbook's meaning no longer
  holds, mark a policy change and return to W2-W5. If the policy remains recognizable but a
  numeric boundary or action value moves, keep the playbook logic, retune the rule thresholds,
  and revalidate. A worse outcome by itself does not justify rebuilding the tree.
- **Recognition check:** apply the frozen live path definitions to later data using only the
  information available at each decision. Report unrecognized and ambiguous cases; never
  relabel them from their outcomes to improve apparent coverage.
- Stress: thresholds x0.9 / x1.1, latency 50 / 115 / 200 / 300 ms, fees +25 %. A rule that
  holds only at its exact values goes back to W5.
- Size: the rule at every size on the ladder, with that size's impact and reaction cost. A
  size is allowed only if the rule stays positive without the best 3 coins there; the first
  size that fails is the ceiling.
- Gate on and gate off: the rule is positive on gated coins and the gate removes losses, not
  migrations.

### W8. Engine

- Every condition is an exact engine metric or an approved new one. A new metric is its own
  plan, approved by the user, built with its definition and its test. The streak gate is a
  rule layer, not a metric ([streak-gate-plan.md](../../roadmap/streak-gate-plan.md)).
- Write the rule documents ([stage-tree-plan.md](stage-tree-plan.md) format) and replay them in
  the lab (`lag_115`, `pumpfun_impact`) on the replayable days, at each size on the ladder. Run
  this before the oldest study day leaves the lake.
- Parity per coin: 90 %+ within 0.003 SOL on the curve, 0.02 SOL on the AMM. Every gap explained,
  in this order:

  ```
  1  coin only in the study?        is its day in the lake (W0)?
  2  position Open at the end?      an AMM line without the on_curve guard, or no prints after it
  3  position Dead in the engine?   no prints for a long time: the lake is stale
  4  data gap?                      flat metric series while PG has trades -> lake-export + restart
  5  different entry row?           tick vs print entry; the pre-entry veto
  6  same entry, other exit?        the study measures something the engine does not
  ```

- Gate parity: the engine's gate decision equals the study's on every coin.

**Checkpoint C6 (after W8):** the robustness table and the engine numbers per window and per coin.

### W9. Benchmark and save

- Compare A and B with the benchmark on the same days in the engine: the window, the newest
  week, without the best 3 coins, migrated coins held. Then the forward days, read once.
- Report total SOL beside the higher-priority outcomes: bought and skipped soon-dead coins,
  migrations bought and still held at migration, migrations cut, and warning lead time before
  dump start. Break each result down by discovered playbook and include the observed-mix and
  alternate-mix totals from W7.
- Keep the development result, cross-week checks, and untouched forward result in separate
  columns. A forward result does not become a new rule-selection score; after it informs a
  change, freeze a later period for the next final check.
- Save as `paper`, inactive, named for what they are, only when the user says so. Turning a rule
  on is the user's call. Commit only when asked.
- Report in plain words: what the rule does, the number before and after, on which coins, and
  how sure. Name a test by what it does, never by an acronym.

**Checkpoint C7:** the benchmark comparison and the save decision.

### W10. Maintenance

A rule is a bet on the owner's habit, and habits change (and our team's own buys change his
game). After a rule is live:

- **Weekly:** re-run W0 step 10 (regime) and the rule's numbers on the newest week; compare
  with the forward test band. A new regime means a new W2 to W5.
- **Game mix:** track the share of launches assigned to each live-recognized playbook. A share
  change alone calls for a per-playbook and reweighted aggregate report, not a new tree.
- **Threshold versus logic:** the default response to a changed cutoff or rule result is to
  retune numeric conditions and revalidate while keeping the supported playbook structure. If
  the owner's choices or responses change so the behavior sentence no longer fits, revisit
  playbook discovery and phase/situation logic.
- **Live against paper:** the same rule's live results (W0 step 11) against its paper or
  engine numbers, per coin; a gap is read from the tape, never guessed.
- **Retire rule:** retire or re-tune the rule when the live result stays below the forward band
  for a week, or when the regime changes. The gate (W5 step 3) is re-tuned on the same rhythm.

---

## 8. Mistakes from past cases, and the rule each gives

### Logic

| Mistake | What goes wrong | Rule |
| --- | --- | --- |
| Tops and bottoms found in hindsight used as states | a state no live rule can read; numbers too good | L1 |
| Only 1-5 s windows; rise from the low without fall from the peak | his silences, the coin's history and passed tops are invisible | L2 |
| Tables averaged over every row in a state | a long coin decides the table; the first moment is not measured | L3 onset, per coin |
| "He sells within 1 s" read without its normal rate | a label true almost everywhere looks like a finding | L4 lift |
| Lift read without the round-trip cost | +2 % looks like money; it is less than the fees | L4 cost |
| Baseline by the coin's own mean | a coin's first row differs from the rest; every factor "works" | baseline = phase x age bucket |
| "Not falling" situations compared to a baseline holding falling rows | they all look good | W4: the fall is in the situation list |
| Exits valued against "hold to death" | almost every cell becomes a sell | W5 step 2: last phase back to the first |
| Buy and sell values that need each other | circular; whichever is guessed first decides | L12: exits first (selling does not depend on the entry price), then the entry, then rounds |
| Rule B checked only on the rows we held | its sells fire on migrating coins we never saw | B's held share on all bought coins |
| A cap of 0 migrated coins sold | the losers are cut late; the rule loses more than it keeps | H comes from the frontier (W6), not a fixed 0 |
| A dump print or an owner-sale read as the kill signal | early kills are pool crashes; the dump tag fires late; in some groups half the bag is sold on coins that live on | the gate event is pool death |
| A lifetime read used after a late entry | "since launch" is not "since we bought"; true at the fill | W4 logic checks |
| A line true at the creation print | the pre-entry veto skips every coin | W4 logic checks, kernel veto |
| The fingerprint's spend taken as our buy size | the reaction cost is measured for the wrong size | Words: our size is the rule's |
| History taken as if we were not there | our buy can draw his reaction | L9 footprint |
| A rule proven at one size carried to a larger one | impact and reaction grow with size | W7 size, L9 |
| Several rules buying the same coin in its first second | our money inflates the early pot and multiplies the loss on bad coins | one rule per coin |
| Trying to outrun a single big owner sale with exits | every exit lost on those coins and cut winners | L14: gate and entry |
| A cell used as the action ("sell at the start of the cell") | the cell is only the context; the real condition (several metrics) is never searched | L15, L17: cell = context, trigger = action |
| Phases and situations from pool and flow cuts with no meaning | metric conjunctions that add search, not sense | L15: meaning first, boundary tested |
| One metric at a time | a story needs several; opposite meanings cancel in a single metric | L17: combinations of 1 to 3, grown from the story |
| A trigger judged by price lift | a lift is not money | L16: SOL in the whole rule |
| Each cell valued alone | one buy per coin: an earlier trigger takes the coin; values do not add up | L16: whole rule |
| Exits valued on all coins of a cell | we hold only the coins our buys pick | W5 step 5: exits again on the entry's positions |
| A streak gate found first, on plain buy-and-hold policies, and frozen | judged on exits the rule does not have; under a later entry it lost 0.15-0.94 SOL a week | W5 step 3 after the exits; step 6 re-check; L18 |
| Buy triggers searched before the exits were final and never searched again | every buy looks worth nothing; the entry falls back to a whole cell | W5 step 4 after the exits; rounds; L18 |
| A whole cell taken as the entry ("buy in P2") | a filter with no story; the real buy moment is never searched | W5 step 4: a trigger with its story |
| An AMM exit carried over from an older, thinner window | the payday's value is guessed on 30 coins | W5 step 1 on the current window |
| A story written after the data | any pattern gets a story | L7: story first |
| REST given an action because the coin sits in it | the residual cell has no meaning of its own | W5: no trigger, no action |

### Method

| Mistake | What goes wrong | Rule |
| --- | --- | --- |
| A rule template filled by a search | the search, not the loop, decides; it lands on luck | L7 |
| A greedy exit set scored on the data it was chosen on | +3.05 SOL in-sample, +0.51 / -0.23 across weeks | L13 |
| A launch buy by default | no exit saves a coin killed in seconds | W5 step 4 |
| A kernel with one part sell | swing trading is unplayable, so never found | W0 kernel |
| A fixed factor list | a new factor is never found | W2 free search |
| Each factor tested against its own shuffle | the best of many tries passes by luck | L6 best-of-shuffle |
| Luck test by a circular time shift of the coin | it leaks the coin's future | shuffle across coins, same phase and day |
| A shuffle that loses the row-position structure | time-in-phase effects look like factor effects | shuffle keeps row position; read time effects as time effects |
| A constant label (all zero in a cell) giving a huge t | false passes | floor the cell's spread |
| Picking on all days mixed | an old habit decides | L5 |
| Newest days kept back as a held-out set | the most valuable days are not used to learn | "The data rule" |
| A line that separates coins only across a habit change | it fails on the new habit | W0 regime |
| Moves accepted on 4 changed trades or 1-3 coins | noise | W7 |
| A one-point peak taken as the threshold | it fails one step away | W7 plateau |
| A setting scored on a simple rule and carried to the final one | its gain does not transfer | re-score on the final rule |
| A "safe" version that only loses | not safer | safety = less exposure at equal results |
| Tuning against the benchmark | the work bends toward it | L11 |
| An AMM exit carried by one coin, or picked from hundreds of combinations on 30 coins | luck | W5 step 1: without the best 2, leave-one-coin-out, noise level |
| An AMM exit with no fallback | holds to the end and loses everything | W5 step 1: every exit has a fallback |
| A 40 % AMM drop line | fires on the normal post-migration shake-out | W5 step 1: backstop wider than the swing |

### Measurement and engine

| Mistake | What goes wrong | Rule |
| --- | --- | --- |
| Kernel buys on ticks, engine only on prints (or the reverse) | entries differ; parity fails | W0 step 8 |
| The study's window read AMM prints only | the engine's `[ws]` reads the curve tail too | W5 step 1 |
| A near metric shipped | the shipped rule is not the one measured | L8 |
| A metric with no engine twin won the search | it cannot ship | L8: propose the metric |
| An AMM line without `on_curve = 0` | fires on the last curve print; the sell never fills | W5 step 1 |
| Backfilled AMM prints with whole-second times | they sort before the last curve print | W0 parity: check AMM print order at migration |
| Generic tag shapes taken as the owner | wash wallets and the creator's bag land on the wrong side | W0 tag check |
| Our wallets tagged as owner | every flow read is wrong | engine rule: ours are never tagged |
| PG token amounts read as whole tokens | prices off by 10^6 | W0 units |
| A pandas datetime cast to int64 taken as seconds | times off | subtract the epoch, read seconds |
| Backfilled rows in PG, the engine still sees gaps | the lake is stale | lake-export + lab restart |
| `lake-export` pruned days the study needed | the engine cannot replay them | W8 before the lake moves; ask before `.env` changes |
| "No data that day" | it was no coins | W0: check trades exist |
| Engine results read before the run finished | a stale summary | read a summary computed after the request |
| A live loss explained from a guess | the cause was something else | W0 step 11: read the tape of each losing coin |

### Communication

| Mistake | Rule |
| --- | --- |
| Study jargon ("LOSO", "trim", "hidden sessions") | plain words and one number |
| A study number reported as the result | "the study says"; the result is the engine's number, and the cross-week number beside any search result |
| Long tables without the point | the point first, then the evidence |
| A guess about a loss presented as a finding | say what was measured and what is assumed |

---

## 9. Checklist

```
  [ ] W0  coins per day DB = cache = lab; trades in chain order; units checked
          AMM backfill (approved); lake-export + lab restart; labels from engine rows; tag check, bundle
          kernel (rows, part sells, AMM, veto, slot cap) = engine on a trivial rule, 90 %+
          replayable days; regime; frozen end; live review (positions, tapes, hourly habit)
  [ ] W1  ledger by venue and fate; owner incentives and constraints                        (C1 with W0)
  [ ] W2  discover recurring playbooks from owner behavior; no preset outcome classes
          dead first; full live state; labels; onset, per coin, lift vs phase x age, cost, newest week
          live precursor and recognition time; recurrence across days; ambiguous cases unforced
          four checks with best-of-shuffle; combined states; reaction cost at every size
  [ ] W3  phases = named steps of his game, never-falling readers, boundary tested, plateau, merged  (C2 with W4)
  [ ] W4  situations = named acts, exact metrics or proposed ones, fall inside, ordered, merged, REST = no action
  [ ] W5  rule loop, every part in SOL in the whole rule, stories first, 1-3 pieces grown, best-of-shuffle, cross-week
          1 AMM exit: all migrated coins, fallback, on_curve guard, few-coin checks (placeholder if no AMM data)
          2 exits under the reference entry (snipe), last phase -> first; rule B draft
          3 gate: event from a discovered adverse mode; tune its conditions and N / M
            (T / X for fast-death); value in SOL with step 2's rule;
            migrated skipped; plateau; both weeks                                                (C3)
          4 entry triggers with gate + exits on, latest phase -> earliest; launch-game pieces
          5 exits again on the entry's positions
          6 repeat 4-5 (max 3); gate on / off / neighbours; cross-week per round              (C4)
  [ ] W6  rules A and B as sentences, a story per line; H from the frontier                 (C5)
  [ ] W7  plateaus, without best 3, 8+ trades, leave-one-day-out, cross-week, stress, every size, gate on / off
          frozen path recognition; results per playbook; aggregate under alternate mixes; threshold vs logic drift
  [ ] W8  approved metrics and gate layer; documents; lab replay at every size; parity per coin   (C6 with W7)
  [ ] W9  benchmark once; forward days once; save paper, inactive, when asked                (C7)
  [ ] W10 weekly re-run; live vs paper; retire rule; gate re-tuned
```

---

## 10. Files

- Study scripts, cache and notes: `hunter/_local/study-<fingerprint-slug>/` (untracked), one
  `NOTES.md` holding every step's numbers. Large caches go to `D:/Bot-cold/`.
- The case reference: `fingerprint-case-<slug>.md` next to this file: identity, data, findings,
  rules, open items and the case's own mistakes.
