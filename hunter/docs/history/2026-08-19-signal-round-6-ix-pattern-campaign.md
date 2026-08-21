# Round 6 - the ix-pattern channel: campaign mechanism confirmed, latency eats the confirmation, the exit question closes

2026-08-19, built on round 4's tables. Three operator ideas: exit on a sell above a static
SOL threshold just after entry; entry only when the lull-breaking buy carries a specific
`ix_labels` pattern; entry when the breaking buy is repeated (the creator "decided to start
volume making"). Plus the operator's correction that hold time should be dynamic on the
token's up/down rather than bounded short.

The round confirms the operator's volume-bot mechanism with the cleanest control result in
the program, kills the confirmation entry on one slot of latency, refutes the sell-threshold
exit, and closes the exit search with a structural insight: the first-gap exit already is
the dynamic exit.

## The new channel: `ix_labels` as buyer identity

Every trade row carries an ordered `ix_labels` jsonb - the instruction shape of the
transaction, which names the client that sent it. In the 8-day window: 100% coverage,
8,247 distinct buy patterns. Client taxonomy by the first non-boilerplate label:
Axiom Trade 36.4%, direct Pump.Fun 31.3%, Terminal 5.5%, GMGN Bot 5.5%, one unknown router
5.3%, Jupiter 2.5%, DFlow 2.3%, long tail of unknown programs. Top pattern is only 17.9% of
buys - the distribution gate passes, unlike every stock feature of round 3.

Scratch: `iv.pd` (pattern dictionary with class + rarity), `iv.tp` (8.8M per-buy rows with
`pid`), `iv.eb`/`iv.eb2` (per-entry breaker identity: first-buy and modal-pattern versions),
`iv.mx` (per-slot max single sell/buy), `iv.rf` (per-entry fast-exit outcomes for all 94,260
superset entries), `iv.sc` (13,808 strict-chain one-per-token pool), `iv.cf`/`iv.k5`/`iv.q5`/
`iv.q6` (confirmation slots and forward paths with cumulative pattern counts).

## Entry results, in the order the gates killed or kept them

**Breaker client class alone: nothing.** On the chain selection under the fast exit the
spread is -0.94% (Jupiter) to -1.65%, all negative.

**Breaker absolute size alone: nothing.** Best bucket >= 3 SOL at -0.27% on 136 tokens,
median -2.76, vsol-confounded.

**In-slot same-pattern repetition - the survivor candidate.** `mode_ct` = the largest count
of any single pattern among the break slot's buys. On the strict-chain pool (one per token,
fast exit, all costs):

| mode_ct | n | net | win |
| --- | --- | --- | --- |
| 1 | 8,620 | -1.66% | 32.6% |
| 2 | 2,414 | -1.41% | 36.5% |
| 3-4 | 2,176 | -1.25% | 36.8% |
| 5-6 | 389 | +0.49% | 42.2% |
| 7-9 | 139 | -3.03% | 38.8% |
| 10+ | 70 | +1.00% | 45.7% |

`mode_ct >= 5` nets -0.27% on 598 tokens vs the pool's -1.57%, and beats **all 20** placebo
draws of the same size (draw mean -1.27%, sd 0.48, z = 2.12). Win rate and MFE rise
monotonically with repetition; `vsol` is flat across buckets. Within the cell the
multi-wallet burst (+0.04%, n=501) beats the single-wallet spam (-1.88%, n=97): the paying
shape is a coordinated bundle across wallets, the way launch crews actually operate.

**The mid-rarity refinement is a tie-break artifact - killed by its own gate.** Requiring
pattern global frequency 50k-500k lifted the cell to +1.80% (231 tokens), but 20 random
231-token draws from the same 598-token parent span -2.37% to **+2.23%** (z = 1.61), and
shifting the rarity boundaries collapses the number (+0.64 / -0.08 / +0.45 at neighboring
windows). The rarity axis carries no information beyond the repetition it rides on.

**Cross-slot confirmation: the information is real and latency eats it.** Waiting for the
k-th same-pattern print after the break, entering at the confirmation print:

| entry | n | net | win | days+ |
| --- | --- | --- | --- | --- |
| break slot (k irrelevant) | 13,807 | -1.57% | 34.4% | 0 |
| confirm k>=3 | 8,832 | -0.71% | 39.1% | 2 |
| confirm k>=5 | 6,514 | +0.08% | 41.8% | 5 |
| confirm k>=8 | 4,749 | +0.07% | 42.6% | 4 |
| blind delay 7s (control) | 13,634 | **-4.64%** | 22.3% | 0 |

The control is the sharpest in the program: an unconditional delay of the same median
length (7s) on the same pool loses -4.6%, so the 5th print carries **+4.7pp** of selection
information over pure timing. But the +0.08% books the fill at the confirmation slot
itself. Charging the standing one-slot entry latency (fill = next active slot) gives
**-2.75%, 0 of 8 days** - the 3Xk2 lesson again: an entry into a running impulse pays its
edge to the slot boundary. The confirmation entry is dead at our latency.

**The mechanism itself is confirmed (lookahead diagnostic).** Grouping break-slot entries by
how many same-pattern buys follow in the next 75 slots: zero continuation -6.7%, 1-5 prints
-4.3%/-2.0%, 6-20 prints +0.5%/+5.9%, >20 prints +5.4%/+4.7% - in both rarity groups, win
rate up to 69%, arm rate up to 92%. The payoff lives exactly where the campaign continues.
The operator's story - repetition means the creator started making volume - is right; the
tradeable problem is that confirmation and price arrival are the same event.

## Exit results - the question closes

All on identical entries and fills, against the incumbent first-gap exit
(`gap_b >= 2`, hold cap 300s, fill at trigger+1) at -1.49%:

| exit | net | note |
| --- | --- | --- |
| first single sell >= 0.3 / 0.5 / 1.0 SOL | -2.10 / -2.46 / -3.35% | losers held waiting for a sell that comes after the collapse |
| two-state: gap unarmed, big-sell armed (0.3 SOL) | -1.48% | wash; armed net decays 2.30 -> -1.31 as hold grows |
| two-state: gap unarmed, activity-ride armed | -0.98 vs -0.81 base | sign flips between subsamples; no transfer |
| pattern-silence (exit when modal pattern quiet m slots) | -0.12 vs +0.08 base | wash on the confirmed selection |
| pattern-state (gap until k reached, then pattern-silence) | -1.49 to -1.56% | wash at every (k, m) |

Seven exit families across rounds 5-6 land within 0.1pp of the first-gap exit or lose.
The structural reason, and the answer to the dynamic-hold idea: **on this venue the
first-gap exit already is the dynamic exit.** Continuous printing is the up-move; the first
pause is the top. Its hold stretches automatically while a campaign runs (the trigger never
fires without a gap) and cuts in ~1.5s on a dead entry. A hold rule keyed on any other state
variable - price arming, sell size, pattern activity - coincides with the gap when it
matters and only adds tail when it does not. The sell-threshold exit fails for the mirror
reason: the informative sell prints inside the same gap that already triggers the exit.

## Where this leaves the search

- Entry: `mode_ct >= 5` on the strict chain is the closest any D-L-I-family entry has come
  (-0.27% vs a 3.45% cost bar, placebo-clean, mechanism-confirmed) and is still ~0.3-0.6pp
  short. ~75 tokens/day.
- The ingest sync is running again (data reaches 08-19 ~11:00 UTC): the first genuinely
  untouched OOS window since round 1 is forming. Validating the repetition cell there needs
  an `ev`/`en2`/`pv`/`tp` rebuild on the new days.
- Untested and cheap: does `mode_ct >= 5` stack with the fresh-wallet rule's selection
  (the one rule that clears the fee)? Both are entry-time observables on disjoint channels.

## Method notes worth keeping

- **Charge one slot of entry latency on any entry that waits for confirmation.** The
  optimistic fill flips the sign; it has now done so twice (3Xk2, k>=5 confirmation).
- **A blind-delay control separates information from timing** in one query, and it is far
  stronger than a placebo draw: same pool, same delay distribution, only the condition
  differs.
- **Boundary-shift the bins of any frequency/rarity filter before believing it** - the
  mid-rarity cell died that way; the placebo draw confirmed.
- psql `-c` with multiple statements runs one transaction: an error at the tail rolls back
  a `create table` at the head. Split builds from reads.
- Scope scratch builds to the selection that needs them - the all-pool campaign-path build
  ran 2h18m before being killed; the scoped equivalent took seconds.
- A terminated-but-forgotten backend from a timed-out session query ran 4h and throttled
  the whole instance; check `pg_stat_activity` before diagnosing slow builds.

## Round 6b - the operator's actual idea: a learned whitelist of specific structures

Same day, after the operator clarified that the original idea was never "any pattern
repeated" but "a **pre-defined set of specific ix structures**: when a lull-breaking buy
carries one of them, enter". Round 6 tested class, size, repetition and rarity - not this.

**Design, fixed before any OOS read.** IS = 08-11..15, OOS = 08-16..18 (round 3's split).
On IS strict-chain entries, book every breaker pattern with >= 20 tokens; whitelist = the
ones with positive IS mean under the fast exit, all costs. Primary key = the breaking buy's
own pattern (`iv.eb.pid`); modal-pattern key secondary. Entry is decidable at the breaking
print itself, so the whitelist pays **no confirmation-latency tax** - `net_fast` already
charges the next-slot entry fill and the trigger+1 exit fill.

**Result: 53 patterns qualify, 13 are IS-positive; blind OOS = +1.13% net on 282 tokens
(~94/day) vs the pool's -1.26%** - the first absolute-positive entry cell in the program.
Per-day -1.83 / +1.71 / +2.75. Median -2.26%, win 40.4%: the familiar lottery shape, with
the top 13 tokens (4.6%) carrying 2.46pp of the mean.

Gates, all run:

| gate | result |
| --- | --- |
| size-matched placebo (20 random ~282-token OOS draws) | mean -1.22, sd 0.77, max +0.25 - whitelist beats all 20, z ~ 3.1 |
| complement | the 40 IS-negative patterns net **-1.47%** OOS - IS sign separates by 2.6pp out of sample |
| persistence | IS->OOS rank correlation +0.36 across 49 patterns |
| boundary sweep | every (min_n, min_net) variant positive OOS: +0.57 to +2.63 |
| redundancy vs `mode_ct >= 5` | 11 of 282 tokens overlap; whitelist without the repetition cell still +0.86% |
| nonce control | durable-nonce breakers alone net -1.57% - the flag is not the edge, the specific structures are |
| confound | whitelist vsol median 38.2 vs pool 42.6 - not a size proxy |
| leave-one-IS-day-out | 4 of 5 positive (+0.50..+1.01), one -0.25 - membership mostly stable |
| token bootstrap CI on the OOS mean | **[-0.88, +3.11]** - spans zero |

**Verdict: the operator's version beats everything round 6 found, and it is the crew-filter
verdict again - selection confirmed, profit CI spans zero.** 3 OOS days and 282 tokens
cannot close that CI. The genuinely fresh days now syncing are the test: re-learn the book
on all 8 days, score blind on 08-19+, no re-tuning.

What the 13 structures are: 8 direct Pump.Fun program variants (sub-shapes class alone
cannot see - class tested -0.94 to -1.65 in round 6), 2 unknown custom programs, and 4 of
13 carry `AdvanceNonceAccount` - durable-nonce = bot infrastructure, consistent with the
volume-bot mechanism. Pattern churn is real but survivable on this horizon: whitelist
coverage decays 7.8% of the IS pool -> 5.7% of the OOS pool, and two patterns nearly vanish.

Implementation note: `pd.pid` is a scratch-build artifact - a live rule must key on the
**pattern content** (the ordered `ix_labels` array), not the id.

Scratch added: `iv.wb` (strict-chain entries x breaker pattern ids), `iv.wl` (the 13-pattern
book), `iv.wl2` (modal-key variant).

## Round 6c - the fresh-day verdict: both ix entry cells are refuted forward

2026-08-20, on the first genuinely untouched day. The ingest sync reaches 08-19 20:54 UTC;
the whole round-4/6 pipeline is rebuilt for it (suffix-9 tables: `sp9` warm-up from 08-18
00:00, `dp9` full-universe - the final `dp` build carries NO `msamp` sample filter, verified
by the 30.1% bucket share of its mints - `ev9`/`ch9`/`en29` capped 10 min before tape end,
`pd9` with pids mapped to `iv.pd` by pattern content, `tp9`/`eb9`/`eb29`/`pv9`/`rf9`/`sc9`).
Pipeline validated on the shared day 08-18: row counts match once the old build's partial
tape (ended 21:03) is accounted for. Fresh strict-chain pool: 1,679 tokens, net -1.76% -
consistent with the 8-day window.

Blind scores, no re-tuning:

| cell | n | net | vs pool -1.76% |
| --- | --- | --- | --- |
| whitelist, re-learned on all 8 days (pre-registered primary) | 124 | **-4.11%** | -2.35pp WORSE |
| whitelist, original 13 patterns (2nd OOS read) | 122 | -3.69% | -1.93pp worse |
| `mode_ct >= 5` | 67 | -1.79% | at pool |
| rolling 1-day book (08-18 only) | 285 | -0.90% | +0.86pp, z=0.98 vs 20 placebo draws - NOT clean |
| rolling 2-day book | 283 | -1.10% | not clean |

**The failure mode is diagnostic.** Fresh-day performance by 8-day book tier: book-good
-4.11%, book-bad -3.60%, unremarkable middle -1.47%, rare/new -1.49%. **Both extremes of
the book underperform the middle** - a pattern that was extreme in either direction was
extreme because one operator's campaign dominated it, and operators rotate within a day.
Pattern-level persistence (+0.32) is carried by the mid mass, not the tails, so it is
useless for selection AND for exclusion. The `mode_ct` monotone ladder does not reproduce
either (3-4 bucket best at -0.31%, 10+ at -3.91%).

**Verdict: the ix-pattern entry channel is closed.** The +1.13% blind-OOS result of round
6b was same-regime continuation - 08-16..18 still carried the campaigns that 08-11..15
learned. The channel's information horizon is shorter than one day, and no learnable book
fits inside it: >= 20 tokens per pattern needs days of tape, and a 1-day book is already
inside placebo range.

**Gate earned: an OOS window adjacent to the IS window shares its regime.** Any selection
learned on operator identity (patterns, wallets, fingerprints) must be validated across an
operator-rotation horizon - a genuinely later day - before it is believed. Adjacent held-out
days answer "is this real in the window", never "will this pay tomorrow".

What survives of round 6: the exit conclusion (unchanged - the first-gap exit is structural,
not operator-dependent), the campaign mechanism as a fact about the venue (volume campaigns
are real; their identity churns), and the method gates.

## Round 6d (08-20): the online-learning variant - refuted at every update speed

The operator's follow-up: maybe the book fails only because it learns too SLOWLY - relearn
the good patterns per hour / per 4 hours / event-driven ("enter after several up-moves of
the same structure"). The daily-book ladder even supports the intuition: 8-day -4.11%,
5-day -3.69%, 1-day -0.90% - monotone toward faster.

**Design - one study covers every update speed.** Every online variant (hourly, 4-hourly,
per-event, k-wins-then-enter) is the same rule: *act on firing #k of a pattern using the
outcomes of firings 1..k-1*. Table `iv.oc2`: one row per strict-chain token (15,487 across
9 days incl. fresh 08-19), first-buy pid + modal pid, with occurrence index and RESOLVED
prior outcomes only (a prior firing counts once its exit fill time `ts+el` has passed - a
live learner cannot see an unresolved outcome; outcomes here resolve in <= 5 min so
intraday learning is mechanically feasible).

**Learnability is NOT the bottleneck**: 68.4% of pool tokens carry a pattern firing 30+
times that day; only 4.9% are one-shot patterns. **Payoff is NOT front-loaded**: net by
occurrence index inside campaigns (size >= 5) is flat noise (k=1 -1.31, k=3 +0.69,
k=4 -2.34, k=12+ -1.60) - the learner is not structurally late.

**The information is just too small.** Learner surface (resolved prior count x resolved
arm-rate, first-buy pattern, day-scoped):

| rule | n | net | median | win |
| --- | --- | --- | --- | --- |
| pool | 15,487 | -1.52% | -2.35% | 34.4% |
| best surface cell: 3-9 priors, arm-rate >= 80% | 682 | -0.55% | -1.55% | 36.8% |
| perfect record, >= 4 priors all armed | 261 | -1.14% | -1.52% | 38.7% |
| near-perfect (>= 4 priors, <= 1 miss) | 813 | -0.46% | -1.45% | 38.7% |
| arm >= 80% AND win >= 50%, 3-29 priors | 529 | -0.77% | -1.52% | 38.8% |

Every strict form converges to -0.5..-1.1%: a ~+1.0pp lift over pool that never crosses
zero, ~1.5-2 SE at sd 14.46%, before any multiple-comparison discount. The gradient is
real and monotone in the specific-pattern band (3-29 priors: <40% arm-rate -2.25, 80%+
-0.55) - same information, same size, as `mode_ct` and the whitelist. Best cell day-by-day:
positive 3/9 days, fresh day -1.45% = at pool. **The trailing-4h recency window - the
literal proposal - is WORSE than day-scoped** (same cell -1.58%): recent outcomes carry no
more information than the day's, there is just less of them.

**Verdict: the ix-identity channel is closed at every update speed.** Slow books die of
operator rotation (6c); fast books die of information size (6d): a pattern's own same-day
record predicts ~1pp, the bar is ~3.5pp. The +4.7pp selection information (round 6
confirmation control) remains real but sits behind the slot boundary and cannot be
harvested at our fill. Scratch: `iv.oc`, `iv.oc2`.

Operator reference recorded for the exit side (parked): buy and sell txs normally carry
DIFFERENT ix structures (buy-ix vs sell-ix); only tool-routed flow (e.g. Axiom) can show
the same structure on both sides. A sell print whose structure matches the campaign's buy
structure = the tool itself leaving - the queued seller-side same-token trigger.

## Round 6e (08-20): ix JOINTLY with the chain, ranked on MFE - the operator's two objections

Two fair objections: (a) was ix ever evaluated *combined* with the other signals rather than
alone, and (b) the objective should be **maximize MFE at entry, then build the exit that
harvests it**. (a) is half-answered by construction - every round-6 ix cell was measured on
top of the D-L-I chain pool, never standalone - but a *joint search* over ix x chain
thresholds was never run, and net, not MFE, was always the ranking target. Both now run on
`iv.jt` (105,263 superset entries, 9 days incl. fresh 08-19, path MFE + realized net).

**MFE selection WORKS - and it is the one ix result that survives the fresh day.** Cells
ranked on MFE put `mode_ct >= 5` in every top slot: `dip<=-.20 & imp>=2 & mode_ct>=5` lifts
mean path MFE 47% -> 68% (deepest cell 72%) and its net is **stable** across segments
(-1.08 IS / -1.14 OOS / -0.63 FRESH). The operator's principle is confirmed: combining the
ix burst with a deep dip does select the entries with the most upside available.

**The upside is not harvestable, and that is why net stays negative.** Of the 47pp mean path
MFE only **5.3pp (median 2.0%) occurs before the first-gap exit; 44.5pp (median 23.0%)
occurs after it**, across a mean 146 further prints. Every attempt to reach it loses:

| exit | net |
| --- | --- |
| first-gap (current) | -1.76% |
| first-gap + TP at 5/10/15/25/50/100% | -1.73 .. -1.76% (unchanged - the gap fires in ~5 s, before any TP) |
| TP-only, no gap exit, 300 s cap | -4.73% (TP15) / -7.29% (TP50) / -8.46% (TP100) |
| pure 300 s hold | -9.65% |

The TP-only and hold rows fill timeouts at the last print, which books the dead-token peak
and is therefore OPTIMISTIC - they lose anyway. High MFE is a volatility measure here, not
a reachable target: the same paths that spike 23% after our exit end far below it.

**Exhaustive joint search settles (a).** 14 binary conditions (6 chain, 4 context, 4 ix), all
1-3 term combinations, 417 cells with n >= 300 in IS, blind OOS and FRESH: **2 of 417
positive in IS, 0 positive across all three segments**, best IS cell +0.87% collapses to
-2.87% OOS. Note `net_fast` is already net of cost, so positive = clears the bar; nothing does.

**What ix contributes jointly is real, stable and ~15x too small.** Adding ix terms to a
chain cell helps 54.1% of the time in IS, 57.5% OOS, 57.9% FRESH, mean lift **+0.13 / +0.12 /
+0.22pp**. The consistency matters: these are generic ix FLAGS (multi-wallet burst
`mode_uw>=3`, durable-nonce infra, prior-repeat `rep_pri>=1`), not pattern identities, and
unlike identities they do NOT rotate away - they read the same on the fresh day. The channel
therefore contains a genuine ~+0.2pp, permanently available; the bar is ~3.45pp.

**Consequence for the program: the entry side is exhausted as a source of net edge, and the
MFE result says why the exit cannot rescue it.** 105k entries x 417 joint cells produce no
positive out-of-sample cell; the available upside sits after an exit that cannot be widened
without losing more than it gains. Scratch: `iv.jm`/`jm9`/`jt`/`jf`/`jmask`/`jres`/`tpl`,
helper `iv.dec(int)`.
