# Anatomy of a cohort entry rule

How to read the rule a cohort search produces, and how to derive one for a new cohort.
The machinery is [convexity-search-workflow.md](convexity-search-workflow.md); this file is
the **shape of the answer** — what the terms mean, which questions they answer, and the traps
that make a wrong answer look right.

Worked example throughout: the 6ix `Buy`+fee launch cohort's 60-second rule
([6ix-instant-crowd-launch.md](6ix-instant-crowd-launch.md)).

## The rule, term by term

```
fire once per token, on the first print where ALL hold:
  m_snapshot.time                     >= 60 s      (1) past the death window
  m_flow_window(60).gross_flow        >= 15 SOL    (2) alive at all
  m_flow_window(60).trade_count       >= 10
  m_flow_window(5).net_flow           >= 0.38 SOL  (3) money going in NOW
  trade_count(3) / trade_count(60)    >= 0.078     (4) faster than its own pace
  fp_first_slot_buy_sol               >= 6.31 SOL  (5) the launch was real
  trade_count(10) / unique_wallets(10) <= 2        (6) a crowd, not one wallet

exit on the first of:  retrace 10 %  |  held 60 s  |  m_snapshot.liquidity >= 75
```

In one sentence: **a token that survived its first minute, is being bought right now, faster
than its own recent pace, by many different people.**

## The six questions — the part that transfers

Each term answers one question. A new cohort keeps the questions and re-fits the numbers.

| # | question | metric family | why it is there |
| --- | --- | --- | --- |
| 1 | is it past the death window? | `m_snapshot.time` | most tokens die in the first minute; an age floor buys survival, and survival is the cheapest edge on the board |
| 2 | is it alive at all? | windowed `gross_flow` + `trade_count` | a liveness floor is what holds the stuck-bag rate down — a bag you cannot sell is a total loss, not a small one |
| 3 | is money going in *now*? | short-window `net_flow` | the instantaneous direction. Depth change **is** net flow, so this needs no price metric |
| 4 | is it accelerating against itself? | short/long `trade_count` ratio | a **ratio is scale-free**, so it does not silently re-read size. Two absolute bounds are not a substitute |
| 5 | was the launch real? | a static cohort axis (`fp_first_slot_buy_sol`) | separates a funded launch from a dust one, and it is known at creation, so it can never re-trigger |
| 6 | a crowd, or one wallet churning? | `trade_count / unique_wallets` | **a count, never an identity.** Operators rotate wallets and bundle trades, so identity is worthless and the count is not |

Question 6 is the wallet-free replacement for wallet analysis. It survives rotation because
it never asks *who*.

The exit answers a seventh: **what ends the trade** — it gave back, it ran out of time, or it
arrived. Persistent-state exits (clock, give-back) beat barriers, because a barrier is
adversely selected at its own fill while a clock is not.

## What must be held constant, or the result is circular

**Entry depth.** `price ~ vsol^2`, so the depth you enter at fixes your payoff. Any gate that
raises P(outcome) also tends to raise entry depth, and the two cancel exactly — one cohort axis
lifts P(migration) **32x** and earns nothing. **Report every candidate inside a depth stratum.**
Pooled, a gate only re-reads depth.

**The fire moment.** Fire on the false->true **transition**, not while the condition holds. A
standing condition stamps a stale trigger, and `entry_slot - target_slot` is 98.7 % trigger
staleness rather than execution time — a metric-gated rule that waits structurally cannot land
near its own trigger.

**One fire per token**, or a single runner is counted many times.

## Six traps, each of which produces a confident wrong answer

0. **A fire count is a claim, and it is the cheapest one to check.** Every number in a
   result is per-fire, so a fire set that is silently truncated makes the whole table
   describe a subsample. Count the fires **two independent ways** before reading any money
   column: a standalone `SELECT count(DISTINCT mint)` over the full cohort, and `simulate`
   on one short window (a separate implementation driving the live `reduce`). The two 6ix
   rules reported 17/day and were 84/day and 279/day — the search harness was dropping
   candidate prints, and *nothing else in the pipeline noticed*
   ([2026-08-26](../../history/2026-08-26-6ix-cohort-rules-are-intra-slot-impact.md)).
   A per-day rate that looks comfortably tradable is exactly the shape a truncation
   produces; suspicion is cheaper than a re-derivation.
1. **Window degeneracy.** A trailing `m_flow_window(W)` is clipped by the token's own age, so
   on a young token every window returns the same number. On a fire set built as
   "first qualifying print per token", **48 % of fires have all five windows identical** — a
   search over 5 metrics x 5 windows is then 5 features wearing 25 names, and it reports the
   multi-window vocabulary as worthless when it was never populated. **Offer a rung only the
   windows `W <= its age floor`.**
2. **Unsellable bags priced as zero.** Where a token has no print after entry, `last/entry - 1`
   is 0 %, not a small loss. At the depth floor **46.6 %** of tokens are in that state, and
   booking them at 0 turns the whole band positive. Book them as a total loss.
3. **Hold-to-death as the failure exit.** Grading failures at the token's last print is the
   worst exit available and moves a surface ~20 pp. A refutation measured that way is not a
   refutation.
4. **P(outcome) as the objective.** Predicting migration better means buying later. Break-even
   P tracks actual P to within **0.2 pp** at the top of the range. Rank on money at matched
   depth, never on the probability.
5. **The fire rule biases the feature space.** First-qualifying-print puts nearly every fire in
   a token's first seconds. Run an explicit **age ladder** so late-firing rules get a fair test.
6. **Your own derived feature is not a metric.** A hand-rolled window quantity that differs from
   the engine's by a boundary convention (`first_value` inside the frame vs the frame's sum)
   correlates 0.98 and is still a different number. **Re-fit on the engine's exact semantics
   before implementing** — shipping a threshold validated on a private variant is a silent
   refutation ([root CLAUDE.md](../../../CLAUDE.md), "The finding sets the metric").

## The gates a candidate passes before it is believed

1. **Every greedy step improves out-of-sample too**, or the search stops there.
2. **All weeks the same sign.**
3. **Still positive with its top five trades deleted.** This is the one that kills most
   candidates; a rule whose profit is five trades is not a rule.
4. **A permutation null that replicates the WHOLE procedure** — shuffle outcomes inside the
   cohort and re-run the full multi-start search, not one greedy pass. A search that tries
   many starts must be nulled against a null that tries as many.
5. **A latency ladder that starts at 0 and steps in milliseconds.** Price the entry at
   0 / 10 / 25 / 50 / 115 / 400 / 800 / 2000 ms, and **report the entry price paid at each
   rung as a percentage of the fire print**. Flat means the rule is deployable; collapsing
   between 115 ms and one slot means it is an execution bet.

   Collapsing between **0 and 25 ms** means something worse: the edge is the price impact
   of the trade that triggered the gate, and no execution buys it. A ladder whose first
   rung is 115 ms cannot see that — it reads the post-impact price at every rung and looks
   flat. The tell is the entry-price column: when the gain at 0 ms equals the entry-price
   rise by the first rung, the rule is reading its own trigger
   — the copy-edge result (a copied wallet's apparent edge IS its own `1 + buy/pool`
   impact) and the 6ix cohort reach the same place from different directions. Any
   `m_flow_window(W).buy >= X` gate is a candidate, because the condition is satisfied
   *by* a buy landing.

A rule that clears 1-4 and fails 5 is not wrong — it is a different, harder product.

## Reading the ladder

Running the same search at several age floors is what separates the two kinds of answer:

- **Money peaks at the launch** and dies within one slot -> a race.
- **Money is lower but flat across the ladder** -> a signal. Prefer it; the ~1 pp of gross it
  costs buys immunity to every latency question.
- **Money peaks at 0 ms and is gone by 25 ms** -> not a race either: the gate is reading the
  impact of its own trigger, and there is no latency at which it pays.
- **Nothing clears its null at any rung** -> the cohort has no drift to redistribute. A rule
  moves money between trades; it cannot create it.

## Rank on the unsellable rate before ranking on money

Where most of a cohort dies, the dominant term in mean PnL is not selection among
survivors — it is how often the position is a bag with no bid. Book those at **-100 %**
(trap 2), then read the unsellable rate as its own column at every step of the greedy
build. On the 6ix cohort it runs 64.7 % -> 2.0 % across four gates and moves the mean
85 pp, which is larger than every other effect measured there put together. A gate that
does not move it is not competing for the thing that pays.
