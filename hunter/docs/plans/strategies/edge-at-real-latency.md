# Edge at real latency — the governing constraints

Every rule this bot runs is graded at the fill it can actually reach. This file states the
constraints that decide whether an edge is real. It governs
[convexity-search-workflow.md](convexity-search-workflow.md); where the two disagree, this
one wins.

## 1. The bar

- **Bot latency is 94-115 ms** decision to fill (p50; 808 real fills over 25 days, and 76
  positions independently). Only ~8 ms is this code — the rest is network and validator.
  **It is not optimisable.** Trigger staleness is nil: the engine reacts to the print.
- **BOTH LEGS pay it.** A latency correction applied to one leg is not a latency
  correction. The exit-leg artifact is the *larger* of the two.
- **Round-trip cost is ~0.0016 SOL/trade** at 0.05 SOL. Rank expectancy against that bar,
  never against zero — a rule at −0.0005 has real selection edge and a cost problem; a rule
  at −0.0018 has neither.
- **Same-slot fills are unreachable.** Landing in the signal's slot happens ~52.6% of the
  time, but *ordering* inside a block is the leader's call. A model that assumes the next
  print is measuring an ordering privilege no latency buys. See
  [fill-and-cost-models.md](fill-and-cost-models.md).

## 2. The law — latency cost is set by which way price moves relative to you

| action | price moves | latency cost | measured fill / trigger |
| --- | --- | --- | ---: |
| buy a breakout | away from you | **expensive** | −9.87%/slot |
| buy a dip | toward you | ~free | 0.889-0.943 (pays you) |
| sell a take-profit | toward you | ~free | **1.027** |
| sell a stop or trail | away from you | **expensive** | **0.964** |

**Any trigger that waits for adverse movement is adversely selected by construction.**
Conditioning on "price just fell" selects for continuation, so the fill lands ~3.5% past the
level. This is structural, not a data artifact and not a slow-execution problem: it holds
across every threshold tested (stops 20/28/35, TPs 10/17/25) on 29,357 episodes, and
reproduces independently on the island population.

**Only "buy weakness, sell strength" is robust on both legs.** A breakout entry paired with
a trailing exit is the worst available construction — both legs pay maximum cost.

## 3. What the law permits

The law forbids *falling price as the exit trigger*. It does not mandate a clock. Four
shapes satisfy it; two of them cap the right tail and two do not.

| exit shape | latency | caps the tail? |
| --- | --- | --- |
| take-profit | favourable | **yes** — rejected |
| clock / `m_position.held` | unbiased | **yes** — rejected |
| **armed trail** (`m_position.arm_above_pct`) | fires from strength | **no** |
| **cause-based** (flow reversal before price turns) | fires before the move | **no** |

The two tail-preserving shapes are the design targets, and both already exist. A
cause-based exit pays **3× more at 115 ms than at an instant fill** — it is a latency hedge,
cheap when fast and large when slow.

**An unarmed `retrace >= N` is a hard −N% stop, not a trail.** `PositionCtx::at_fill` seeds
`peak_price = entry_price`, so retrace measures drop-from-entry until price rises. Unarmed
trails turn 21% of winners into losers and no width from 2-20 rescues it. See
[armed-trailing-stop.md](armed-trailing-stop.md).

## 4. Order of work

Order by what each answer depends on. A token that rugs to −90% loses under **every** exit
at **every** latency, so it can be settled first. "Is this moment convex?" cannot: the same
16,874 entries price at −65.44 SOL under one exit and −20.66 under another. Settle the
independent questions before the coupled ones.

1. **Fix costs first.** Size scales with pool depth (`buy_pct_of_vsol`), because impact is
   exactly `buy / vsol`. Arithmetic, not search — see
   [execution-costs.md](execution-costs.md).
2. **Cut the poison tokens.** The never-enter blacklist is the only step whose answer does
   not move when the exit changes, and the only lever with **zero execution cost** — no
   fee, no impact, no adverse fill, because the trade never happens. It is also what makes
   the moment search possible: a greedy search cannot start from a deeply negative
   universe, and rug/instant-death tokens contaminate every region equally.
3. **Then fix the exit**, on the cut population. It is worth more than the entry: on one
   island the exit alone moves expectancy by +0.0028 SOL/trade, more than the entire
   remaining gap to break-even.
4. **Then search the moment**, on top of that exit. A moment search wearing a bad exit
   reports an empty space, because a good entry with a losing exit still loses.
5. **Tune thresholds last, then re-check the cut once.** The cut and the exit are coupled;
   the moment is separable.

**Grade every step at 94-115 ms on both legs**, reporting the zero-lag column beside it —
the ratio is the artifact size.

Two invariants no cut may break:

- **Decision-time facts only.** What a token did after the decision instant is not a
  filter. 44.5 of 47 percentage points of selection edge land *after* the decision point
  and are unharvestable.
- **The runner rate must survive.** Profit is the right tail. A cut that raises expectancy
  while lowering the frequency of `>= +50%` outcomes has converted a convex book into a
  flat one — reject it. **Blacklists generalise; whitelists do not** (a top-5 identity
  whitelist scores −38.59 OOS where the blacklist pays), and cut sets are chosen on days
  that never see the day being graded.

## 5. Two search failures that manufacture false answers

- **A greedy search gated "positive at every step" cannot start from a negative universe.**
  From a −769 SOL baseline no single cut is positive, so the search reports an empty space
  while a conjunction would pass. An empty result from a greedy search is not evidence of
  absence.
- **Pin every cross-token policy.** `skip_duplicate_identity` absent inherits `app_settings`
  and is an anti-selection — it removes ~20% of trades and 40-63% of the profit, so an
  unpinned ladder is not comparable row to row.

## 6. Known bias in `LagMs` — results are optimistic pending a fix

`LagMs` selects the **first** print at or after `fire_time + lag` and prices the fill from
it. A transaction executing at wall-clock `T` actually meets the pool state left by the
**last** print at or before `T`: `vsol` on a row is the reserve *after* that trade (99.98%
agreement with the later trade's direction over 2,081,639 same-mint pairs). The selected
print landed *after* us, so its own impact cannot have touched our fill.

Measured cost: **+8 to +12 pp per trade in our favour**, and it flatters **exits into
strength** specifically — a rise-triggered exit is usually followed by another buy. The
correct baseline is the last print with `block_time <= fire_time + lag`, falling back to
the firing trade itself when none intervenes (a quiet tape costs nothing, because an AMM
price moves only when someone trades).

**What this does and does not invalidate.** A bias in our favour cannot rescue a negative
result, so every "negative at 115 ms" verdict stands and is understated. **Comparisons
between exit shapes do not stand** — they inherit the bias unevenly and need re-running.
Live paper and the grouped sweep are unaffected: they run `WorstCase`.

## 7. Status

| claim | status |
| --- | --- |
| the exit-direction law | **proven** — 3 independent corpora, mechanism is structural |
| unarmed retrace is a hard stop | **proven** — engine behaviour + counterfactual |
| pool-fraction sizing | **derivable** — impact is `buy/vsol` exactly |
| cause-based exits beat reactive ones at latency | **needs re-running** - inherits the LagMs bias (section 6) |
| ix-structure cuts | **unproven** — validated only at an unreachable fill, never regraded |
| the three islands (absorption, impulse, quiet accumulation) | **dead** at real latency |
| entry-side island search | **open** — prior nulls come from greedy searches and bad exits |
