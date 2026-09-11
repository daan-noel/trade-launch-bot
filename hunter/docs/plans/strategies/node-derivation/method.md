# How a rule is derived from the traders who run it

The playbook that turns a node of profitable wallets into a shippable rule: which member pays,
what it reacts to, which of those moments pay at our seat, how it exits, which coins to refuse,
and then every slot re-derived on the sentence's own pool. Each step names the question, the
toolkit call that answers it, the rule that decides, what it returns on the hot-tape node (the
worked example, [hot-tape-rule-1.md](hot-tape-rule-1.md)), and the trap it guards against.

The basis is [_!___strategy.md](../_!___strategy.md), the loop it plugs into is
[_!___workflow.md](../_!___workflow.md) (section 8 G1), and every number is
[_!___evidence.md](../_!___evidence.md) 1.4-1.20. The code is [toolkit/](toolkit/README.md); the
hot-tape scripts, step by step, are [hot-tape/](hot-tape/README.md). A new node's working file
starts from [node-template.md](node-template.md).

## 1. The basis

- **The only profit source is other people's buying while we hold.**
- **Money = P.W - (1-P).L - toll.** The toll is 3.2-4 % a round trip (125 bps and 0.000225 SOL a
  leg, plus our own impact). Where we enter and how we exit set the break-even win rate.
- **Price already holds every print that landed.** Flow, "up 20 %", zigzags and pullbacks are in
  the price. What is not: which recipes (trade-ix's) trade, how many independent ones, who is
  selling, and whether the pushers still hold (strategy 1.4). A term earns its place from that
  second list.
- **The seat.** Our fill lands 115 ms after the decision, on both legs. Losing the sequencing race
  costs -5.59 points; the 115 ms itself costs -0.77. An event anchored on a print is a FOLLOW model
  by construction; a state that has held for seconds can be reached before the crowd.
- **Direction.** Buying into a sell has the price moving toward us during the lag; buying into a
  buy has it moving away (strategy 1.5).
- **A wallet is an instrument, never a term** - agreement between wallets included (law 20).
  Every term is spelled in public tape state or instruction structure.
- **A rule is six slots**, D E P X R S. Money on the whole conjunction, day by day, is the only
  score.

## 2. The standing corrections

| rule | what it forces |
| --- | --- |
| A red number closes a sentence, never a slot (law 17) | A red result with D or P empty, or X guessed, is an unfinished sentence. A node is never written closed until every slot is searched |
| A negative result means the ideas are not enough | Red routes to a new idea for the empty slot, never to a verdict |
| None of D, E, P, X is frozen while searching | A slot is held fixed only to search the next one on its fires, and is reopened when a better filling appears (phase G) |
| A waiting time is not a reaction time | "Seconds since the last big buy" measures how often big buys happen. Latency is measured from the trigger print, by excess intensity (B1) |
| Scalpers detect a move, they do not ignite it | Their SOL is 2-5 % of the move. Their arrival marks a live move; it is not the move |
| Decision point first, then the event, then the exit | The order of the phases below |
| For re-entry scalpers, E and X carry more than D and P | E aims at the convexity and its moment; X eats as much of it as possible |
| Read an exit at the actor's own hold, on a selected pool (law 26) | An exit swept on an unselected pool returns the shortest clock, because the pool is mostly dying coins |
| Pool no members (law 27) | Six wallets under one label can be two strategies; the average hides the one that pays |
| Copy no median trade (law 28) | Imitating an actor is bounded by its median trade, never by its net. Search which of its trades win |

## 3. The keep rule and the ship bars

Every choice in phases C-G is made the same way, so it is stated once.

**Walk-forward.** The days of the current sentence's fires split into two halves. Each half (fit)
picks the value with the most SOL among those whose every fit day is positive; the other half
(test) scores it against the current value there. A change is **taken** only when both folds
move the same way and both beat the current value on their test half; the value is the mean of
the two picks, snapped to the grid, a tie to the side nearer the current value
(`walkforward.thresholds`, `converge`, `new_terms`, `axes`).

**Chance.** A gain on a test half smaller than the SD of the SOL a random share of the tickets
carries (`walkforward.cut_noise`) is chance, whatever the folds say. On rule 1 a random 5 % of the
tickets carries 0.15-0.18 SOL a half, 10 % carries 0.20-0.25.

**The ledger** (`book.ledger`) is read on every result, never a mean alone:

| bar | the ship line |
| --- | --- |
| days positive | >= 5/7 on the study tape, every day on the holdout |
| the two halves of the days | both > 0 |
| body (net without the top 1 % tickets) | > 0 |
| top 1 % share of net | <= 15 % |
| biggest coin's share of net | <= 15 % |
| capped book (every gain capped at the median take profit) | > 0, and its top 1 % share inside the bar |
| exits on the graduation print (`graduation.grad_flag`) | counted; the SOL must not depend on them |

A change that raises SOL and breaks a bar is not taken. A change that raises SOL only in the
capped-away gaps is not taken (rule 1's holders 150-200, 1.20).

**The holdout confirms, it never chooses.** Every change is booked ONCE on the holdout, one
change at a time in the order taken; a change that fails there is dropped (rule 1's 240 s
clock, 1.20). Each read wears the holdout; the next unseen days are the clean test.

## 4. The method

Each step runs on the full print tape at `lag_115`, one position per coin at a time. `S` is a
`tapes.load(name, tapes.roster(node))` session; `w` a member's wallet id (`S.wallet(prefix)`).

### Phase 0 - the frame

| # | step | how | decides |
| --- | --- | --- | --- |
| 0.1 | **Two tapes** | The study tape, where every threshold is read, and a holdout tape of later lake days in the study tape's exact format (`lake_export.export`, then an entry in `tapes.TAPES`), started a few days early as a warm-up and checked row by row where the two overlap | Nothing is chosen on the holdout |
| 0.2 | **The instruments** | The roster's wallets for the node (`tapes.roster`): they mark prints NODE so every public fact excludes them | They never enter a term |
| 0.3 | **The seat** | `kernel.py`: both legs fill at the last print landed 115 ms after the decision print, exact curve arithmetic, 125 bps + 0.000225 SOL a leg | One kernel prices everything |

### Phase A - who pays

| # | step | how | decides | hot-tape |
| --- | --- | --- | --- | --- |
| A1 | **Price a perfect copy, per member** | Each member's own round trips through our kernel (`seat.episodes`: first buy while flat to the sell that takes it to <= 2 % of its peak): win rate, median, top 1 % share | A member with a negative median and a tail carrying the net is noise (law 28) | One member wins 68.3 % at a median of +12.02 %; five carry a negative median and a 180 % top 1 % |
| A2 | **Split the node by member, at two seats** | `seat.seat_book(S, E, caps=(15,))`: RACE (sequenced before its print, the reserve it met) and FOLLOW (our fill 115 ms after its print) | Keep the members positive at RACE on most days with a positive body. Positive only at RACE: the event must be reached before it | 8fStGV +2.28 % 8/8, AbQcLH +2.29 % 7/7, 49uohd +2.24 % 6/8 at RACE; at FOLLOW -1.45 to +0.41 %. Pooled +0.50 % with a 216 % tail |

Trap: pooling. The six read red for weeks because three of them are noise (1.11).

### Phase B - the event: find the decision point first

| # | step | how | decides | hot-tape |
| --- | --- | --- | --- | --- |
| B1 | **The trigger, by excess intensity** | `trigger.excess_intensity(S, {m: [w]})`: its buys against same-coin controls, every public print in the 5 s before by (class, lag); `trigger.peak(lift, cls)` | A spike of one class at one lag band is the trigger, and its lag is the reaction time. Flat everywhere: the member fires on a state | 8fStGV: public SELL >= 1 SOL at 25-200 ms (lift 8-10), avoids burst starts. AbQcLH: burst start at 25-50 ms (8.8-9.9). sssssw (loses): burst start at 75-100 ms (9.2). Speed does not separate payers from losers; the side does |
| B2 | **The seat against the trigger** | `seat.reaction(S, E, trigger_fn)`: its lag from the trigger print, the share where our fill on that trigger lands ahead of its buy, and the book at our seat, ahead and behind | Reachable when its lag p50 is well over 115 ms and our book is not red when we land behind. A lag near 50 ms with under 10 % ahead is DELAY about zero: a race, not an event | 8fStGV: lag p50 81 ms, ahead 33.5 %, +0.21 % even behind: reachable. AbQcLH: 47 ms, ahead 5 %, -1.36 %: a race |
| B3 | **Which triggers: within-coin contrast** | On the member's coins, every print of the trigger class (`candidates.build` with that trigger), labelled by whether it acted (`contrast.label_acted`, its reaction window); `contrast.strat_rank(acted, ignored_same_coins, facts)` and the medians | The facts far from 0.50 with a mechanism become the event's terms; the acted medians are the first thresholds | The sells it buys: 15 recipes in 5 s (ignored 7-8), 3.94 SOL bought in 2 s (0.4-0.5), a new high 5.4 s ago (73-80), a seller who bought 20.5 s ago (50) |
| B4 | **Spell it publicly on the full tape** | Every term in public tape state, on every coin (`candidates.build` + `book.fires`); add the terms one at a time; run the pump-side control (the same terms on the opposite side) | Each term must lift the book monotonically; the control must be worse | The frenzy-absorbed sell: -4.26 % to -0.68 % term by term; the same frenzy on a BUY -2.64 % |

Traps: a waiting time read as latency (1.9); a feature window that contains the member's own
print (every fact is built from prints before k); a model of the moment that is learnable and
worthless - the moment reproduced out of sample at AUC 0.72 books -4 to -11 % (1.7).

### Phase C - which coin (the door), diagnostics first

| # | step | how | decides | hot-tape |
| --- | --- | --- | --- | --- |
| C1 | **Split by the member's coin list** (diagnostic only) | The event's fires on coins it trades against every other coin, and on its coins split at its FIRST buy there | A gap before its first buy is a coin property; a gap only after it is its future arrival, and D has to predict that arrival | +1.30 % on its coins, -3.96 % elsewhere; +4.46 % before its first buy, -0.68 % after: its arrival |
| C2 | **Split by its arrival inside our hold** | `candidates.build(..., actor=w)` gives `act_in` | Tells how much of the book is incoming demand | +6.04 % when it arrives, -0.62 % when it does not |
| C3 | **Search D on the frozen E's fires** | Coin facts from prints before the fire; AUC of the fires that pay against the rest, money by quintile, the best two stacked; the keep rule | A door is kept only if it survives the holdout | "Earlier frenzy-sells on this coin failed" +1.13 % 5/7 in sample, -0.53 % on the holdout: fitted, dropped |

Trap: the coin list as a door - it is a wallet term and contains every coin traded LATER.

### Phase D - the exit, at the actor's hold

| # | step | how | decides | hot-tape |
| --- | --- | --- | --- | --- |
| D1 | **Its closing trigger and hazard** | `trigger.excess_intensity(S, {m: [w]}, cases="close", controls="hold")` for a print trigger; `hazard.closing_hazard(S, w, pool)` on the pool the sentence selects: the chance its next print is the close, by profit x time held | A hazard that jumps at a profit band is a take profit; one at a loss band is a stop; a flat band waiting for time is a clock. Read at fine bins | On rule 1's pool it holds through +8..+12 %, sells hard at +15..+20 % (12-30 % a print), stops at -25..-30 %, and its clock closes land at 60-90 s |
| D2 | **Book the families against the bracket** | `exits.X(kind=...)` for bracket, scale, sellbuy, trail, ride, fade, dump; each with its own occupancy | The bracket is the default until a family beats it on money and bars | Twelve tape-driven exits all book below the bracket: a frenzy swings 5-10 % in a few prints and tight reactions cut recoveries |

Trap: sweeping X on an unselected pool, or at a horizon longer than the node's hold (1.10).

### Phase E - the permission, on the losers

| # | step | how | decides | hot-tape |
| --- | --- | --- | --- | --- |
| E1 | **The trades that stop against the trades that take profit** | Facts at the fire; AUC; a one-sided cut by the keep rule; applied BEFORE occupancy, so a refused fire frees the coin for a later one | P is the cut that removes stop-outs without shrinking the book | The stop-outs are young, thin coins: holders >= 368 x age >= 158 s, +2.07 % 7/7 |

### Phase F - hold out

| # | step | how | decides | hot-tape |
| --- | --- | --- | --- | --- |
| F1 | **The frozen sentence on unseen days** | The same booking code on the holdout tape, nothing re-fitted, the full ledger | Every bar | +1.90 % 5/5; the fitted door fails |

### Phase G - every slot re-derived on the sentence's own pool

Once a sentence holds out of sample, each slot was chosen on a pool the later slots changed. So
each is re-read on the pool the whole sentence trades, in the order E, P, X, R, S.

| # | step | how | decides | hot-tape |
| --- | --- | --- | --- | --- |
| G0 | **The room** | Split the sentence's trades by the member's pick (`act`) and its arrival (`act_in`) | If its picks pay no more than its skips, copying it has no room left: read every threshold off money | Its picks +2.00 % / +2.77 %, its skips +2.82 % / +2.49 %: money decides |
| G1 | **The candidate table** | `candidates.build(S, trigger, floor, exit, actor)` under floors loose enough for every loosening; `book.save` | **It must reproduce the current book exactly** (`book.fires(C, spec)` against the recorded ledger) before any number off it is trusted | 169,995 / 113,456 candidates; rule 1 reproduced to the ticket (840, +2.73 %, 4.58 SOL) |
| G2 | **Thresholds by money** | `walkforward.converge(C, days, base, grid, veto=bars)`; a term that lands on the grid edge means the table floor is too tight - rebuild it looser | The keep rule and the bars | "Bought >= 2 SOL in 2 s" drops out (+0.71 / +0.93 SOL); holders wants 150-200 and is vetoed by the tail |
| G3 | **Structural checks** | `graduation.grad_flag` (trades and SOL on the completing print), the capped book, a lookahead read of any term that looks too good | A book that rests on an unpriceable exit gets a term that keeps the exit priceable | 16-17 % of trades carried 46-48 % of the SOL at graduation; reserve <= 100 SOL removes it |
| G4 | **New terms** | `walkforward.new_terms(C, days, base, facts)`: each fact cut at 12 quantiles from both sides; `cut_noise` beside it | Taken only above chance | Two of 22 pass at +0.04 to +0.18 SOL, inside chance: none taken |
| G5 | **The exit on the new pool** | `exits.outcomes(S, C[mask], grid)` then `walkforward.axes`: one axis at a time | The keep rule, then the holdout | Take profit holds +15 %, stop widens to -40 %, clock to 240 s - which fails the holdout, so 90 s stays |
| G6 | **Re-entry** | `book.occupy(C, m, cool_sl=..., max_per_coin=...)`, and the book of the n-th entry and of entries after a stop | Taken only when the trades it touches are enough to beat chance | Nothing taken: 14 and 8 trades |
| G7 | **Size** | `book.reprice(F, b)` at every clip, flat and as a share of the reserve | The largest clip that keeps every bar on both tapes | 0.35 SOL flat |
| G8 | **The holdout, one change at a time** | Each change in order on the holdout (`hot-tape/r1u_holdout.py`) | A change that fails is dropped | +3.44 % 5/5, top 1 % 13.1 % |
| G9 | **The engine's target** | A print-by-print replay that shares no code with the candidate table (`hot-tape/r1_replay.py`): first with the members left out, where it must reproduce the G8 tickets one for one; then with every wallet counted, on a tape with every leg of a transaction (`lake_export --all-legs`), across a lag sweep | Two codes that agree to the ticket rule out a booking bug; the every-wallet, every-leg book is the number the engine must reproduce, and it must still pass the bars | 739 / 433 tickets identical; the engine's target is +3.55 % study, +3.26 % holdout 5/5 |
| G10 | **Every term against an exact field, every term as the engine computes it** | Each term's input compared with an independent exact field of the lake (`hot-tape/r1_terms_audit.py`); then a replay spelling every term, fill and clock the engine's way, each line citing the engine code it mirrors (`hot-tape/r1_exact.py`); the book after each correction, one at a time; the rule re-derived on a study tape at the engine's grain (every leg, its clock and its spot); a second code sharing nothing rebuilds the tickets (`hot-tape/r1_exact_check.py`) | A second code sharing an idea cannot catch the idea; a term that fails its exact field is re-spelled, and the rule is re-derived where the engine's answer differs | The holder book was float dust; the engine's buy-only entry fill prices 29 % of entries at a print our buy cannot meet; re-derived: +4.44 % 5/5 on the every-leg holdout, top 1 % 9.8 % |

## 5. The guards

- **Every fact carries a date.** A fact dated after the fire is a lookahead. A door fact
  measured at age 60 s cannot serve a fire at age 20 s.
- **Every exit branch resolves to an index; the smallest index wins** (law 24). A multi-leg exit
  is priced leg by leg (law 25).
- **An absurd win rate is a lookahead until shown otherwise** - 80 % on a scalper, or a cell green
  on every column at once.
- **A ceiling is not money in a slot** (law 23). A perfect-exit or perfect-door number bounds the
  slot; it does not say the slot can reach it.
- **A member's coin list is a diagnostic split, never a door**, and it is split at its first buy
  on the coin before any gap is read as a door.
- **Read a hazard at fine resolution before spelling a stop.** Coarse bins put this member's stop
  at -20 %; finer bins put it at -25 to -40 %.
- **A gradient is not a filling.** A term monotone in money that does not cross zero stays a
  candidate for a fitted vector, not a threshold AND.
- **Report the per-day list, never a mean floor**, and ask of every term whether it can be spelled
  without naming a wallet.
- **Features in the coin's own units.** A fact z-scored against the coin's own prior history beats
  one ruler across thousands of coins almost everywhere (1.6); a live rule computes it from the
  coin's tape with no stratum and no lookahead.
- **Report coverage and reaction cost beside every lift.** A conjunction at lift 4.2 covering
  1.2 % of the member's buys is a rare corner, not its logic; an event that costs over about 2 %
  to react to (the price move from the state a watcher held to our 115 ms fill) is unreachable
  whatever its lift.
- **Every flow fact has a node-blind twin** with the members' SOL removed, so a term can be asked
  whether it is public tape or a proxy for the members.
- **`v[k]` is the reserve AFTER print k.** Pricing an entry at `v[their buy]` charges THEIR
  displacement; the reserve they met is `v[k] - signed amount`. A state fire anchors at the print
  the decision sees (`k-1`): anchoring at `k` fills later, and on a falling tape a later fill is a
  cheaper buy.
- **A new engine reproduces the old book before it is used** (G1): a variant booked on a table
  that does not reproduce the current sentence is measuring the table.
- **The tape is curve prints only.** Count the exits on the graduation print; a sentence that
  needs them is priced at a point the tape cannot see.

## 6. Where the results go

| result | goes in |
| --- | --- |
| each step's numbers | a numbered section of [_!___evidence.md](../_!___evidence.md) |
| each step's coordinate and next action | a row of [_!___workflow.md](../_!___workflow.md) G3 |
| each idea tried | its slot in [_!___inventory.md](../_!___inventory.md), with status and evidence |
| the sentence, its book, its derivation chain, the members | the node's working file, from [node-template.md](node-template.md) |
| the scripts | `node-derivation/<node>/`, one per step, each opening with its step and question |
