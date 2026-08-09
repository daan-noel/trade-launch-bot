# Launch-crew follower analysis — refuted out-of-sample (2026-07-29)

> **History.** Strategy mining over the full 7-day lake. Kept because the registry-copy + fixed-TP class it surfaced **failed out-of-sample**; the in-sample numbers below are exactly the trap a re-run would fall into.
>
> Nothing here is a current instruction. The rules that survived this work live
> in `CLAUDE.md` and `docs/plans/strategies/`.

---

Big-picture strategy mining over the full 7-day lake (07-22..28, 7.9M trades,
120k tokens, 2.65M wallet-mint visits). Goal: find strategy classes viable at a
2-3 SOL bankroll, 0.03-0.1 SOL entries, 10-50 fires/day, minutes-scale holds.
All PnL net of 125 bps/leg + 0.001025 SOL/leg fixed + own price impact
(see [execution-costs.md](../plans/strategies/execution-costs.md)). No Helius calls; lake +
scratchpad DuckDB only.

> **Second session, same day (07-29):** 5.1 and 5.2 were executed; results and
> corrections are in section 5. Two of this file's original conclusions did not survive
> re-derivation - the signal-D threshold (section 3) and the "regime dominates" reading
> (section 4). Read 5.2 before acting on either.
>
> **Lake coverage is partial on several days** - hours of tape present per day:
> 07-22 **6 h**, 07-23 23 h, 07-24 24 h, 07-25 24 h, 07-26 16 h, 07-27 21 h,
> 07-28 15 h. Per-event EV is unaffected; per-*day* counts and any SOL/day figure
> are exposure-weighted by this, and the corpus keeps growing as the EC2 box is
> re-synced (which is why a re-run of an older backtest does not reproduce its
> episode count - see 5.1).

## 1. Who actually makes money at minutes-scale (wallet census)

Per-wallet rollup of all completed visits (sold >= 99% of tokens bought),
wallets with >= 20 completed visits, net of fees:

| median-hold bucket | wallets | net-positive | median wallet PnL |
| --- | --- | --- | --- |
| < 5 s | 783 | 71% | +1.91 SOL |
| 5-30 s | 6,145 | 34% | -0.41 |
| 30-60 s | 4,102 | 24% | -0.71 |
| 1-5 min | 5,664 | 21% | -0.69 |
| 5-30 min | 781 | 16% | -1.02 |

The median wallet LOSES everywhere past 5 s. The minutes-scale winners split
into three populations (via token-age-at-entry + shared-mint overlap):

1. **Creation-slot insiders/bundlers** (median entry age 0.0 s, 90-100% win,
   3-11 SOL buys, hold 40-360 s): the top of the PnL table. Top wallet
   `7p4AkPb9` +3,038 SOL/week at 99.6% win. These are the crews that "eat
   snipers". Many multi-wallet operations with identical sizing (a whole
   cluster medians exactly 1.975 SOL/buy).
2. **Early-but-not-instant insiders** (entry age 6-20 s, ~90% win, median
   completed-visit return +1,000-2,500% i.e. 10-25x): pump-scheme
   beneficiaries. Skill cannot produce a 10x MEDIAN at 90%+ win; this is
   coordination.
3. **The known scalper family** (64hP, omego, GVVP8, JDfuh8, FYTVwP, HK3J9...):
   real but thin, sub-minute to ~10 min, heavily overlapping mints (shared
   hot-token selection, not a crew). Already covered in
   [wallet-analysis.md](../plans/strategies/wallet-analysis.md).

## 2. The pumps are detectable at t+20s (token-side)

Per-token early tape (first 20 s post-creation) vs outcome (peak in the next
10 min from the t+20s reference price), 58,391 tokens with a live tape:

- Base rate P(2x) = 5.9%, P(3x) = 2.6%. Median time-to-peak ~4 min.
- >= 32 SOL bought in first 20 s -> P(2x) 18-23%.
- >= 15 unique buyers in first 20 s -> P(2x) 14%.
- >= 5 unique buyers of >= 1 SOL AND <= 2 sells ("signal D") -> P(2x) 21.6%,
  ~53 tokens/day. **Corrected 07-29 (5.2): the >= 5 threshold does not
  reproduce** - on a clean re-derivation it selects ~215 tokens/day at P(2x)
  5.4%, i.e. no lift over the 5.0% base. The lift appears only at **>= 7**
  (P(2x) 20.6%, ~38/day) and the cliff between 6 and 7 is sharp (6.2% -> 20.6%).
  Use k >= 7; treat the k >= 5 number as an artifact of the older derivation.
- Dev initial buy 8-26 SOL -> P(2x) 19.7%, P(3x) 10.2% (independent
  re-confirmation of the fs3-dev finding on a different objective; the 07-29
  re-derivation gets 12.2% / 6.3% on the same band, still ~2.5x base).

## 3. Follower simulation - what survives and what dies

Tick-level sim: enter at first print >= t0+22 s, exit by trailing stop /
hard stop -35% / time cap, 3% exit haircut sensitivity, size 0.05 SOL
(round-trip fixed+fee ~= 6.6% at that size).

**DEAD (measured, do not build):**
- **Broad early-momentum follower** (>= 30 SOL in 20 s, ~1,220/day):
  EV -7.2%/event even with NO haircut. This is the sniper trap quantified:
  the generic "hot launch" signal is exit liquidity for the crews.
- **Registry copy-trading** (buy when a top-100 winning wallet buys in first
  60 s). In-sample +5.8..+8.1%/event. **Out-of-sample (registry trained
  07-22..25, traded 07-26..28): -5.7 to -8.7%/event, negative on all 3
  days.** Winning-wallet lists do not transfer forward; this generalizes the
  earlier zero-latency copy-trade refutation.
- **Fixed take-profit exits** (TP-race +30..80% vs SL): EV -10 to -18%
  everywhere, worse than trailing on every signal and every day. The entire
  EV of launch-window entries is the right tail; capping it is fatal.

**SURVIVING, WITH CAVEATS - "signal D" crew-footprint rider:**
>= 5 distinct buyers of >= 1 SOL AND <= 2 sells within 20 s of creation;
enter ~t+22 s; trail 25% with hard stop -35%, cap 10 min; ~52 events/day.
Per-day EV at 0.05 SOL with 3% exit haircut:

| day | n | win% | EV/event |
| --- | --- | --- | --- |
| 07-22 | 33 | 33% | +3.6% |
| 07-23 | 48 | 33% | +12.5% |
| 07-24 | 52 | 42% | +25.0% |
| 07-25 | 105 | 34% | +11.5% |
| 07-26 | 40 | 18% | -9.0% |
| 07-27 | 67 | 28% | -7.3% |
| 07-28 | 21 | 24% | +7.3% |

Pooled ~+6.9%/event (~+0.18 SOL/day at 0.05 sizing), 5 of 7 days positive,
BUT day-mean t ~= 1.3 - **not established**; the edge is regime-dependent.
Median trade is -20%; the profile is 30% win / winners 3-5x losers. Any
production attempt needs (a) a discriminator for the losing days (see
creator reputation below) and (b) live-forward paper validation, per the
project rule that post-hoc selections on this dataset tend not to
generalize.

## 4. Implications / open leads

- **Partial exits remain the biggest structural gap.** Fixed TP kills the
  tail; pure trailing gives back 25-30% of every winner and rides losers.
  Every profitable reference (omego's runner tranche, the crews' own
  distribute-into-strength) uses scale-outs. A 2-tranche exit (sell most
  into strength, trail a stub) is the shape both exit families point to and
  the engine cannot express today.
- **Regime dominates tuning.** Day-level EV swings (-17..+25%/event) dwarf
  every knob difference measured. A meta-layer that scales size / pauses on
  rolling realized EV would matter more than any entry refinement.
  **Qualified 07-29 (5.2): the day swing is NOT market-wide.** Running the same
  follower over *every* token with an entry print (49,476 events) gives a dead-flat
  -6.9 / -7.6 / -7.8 / -7.5 / -7.5 / -7.9 / -8.2 %/event across the seven days,
  and a flat 6.6-7.9% P(2x), while the crew-event cohort swings +34% to -2%. The
  variance lives in *which launches the signal picks up that day*, not in the
  market. That moves the fix from a thermostat to a selection filter - see 5.2.
- **Creator reputation is the untested discriminator.** Creator = creation
  buyer; crews have stable fingerprints (identical sizing across wallets,
  cu_price signatures, funding chains). Scoring creators/crews by their PAST
  launches (pumped vs insta-rugged) is the natural way to separate signal-D's
  good days from bad ones. **Measured 07-29 - it works, but as an EXCLUSION
  (drop repeat creators that never produced a 2x), and the crew/backer-wallet
  variants do not. Full result in 5.2.**
- Sizing at this bankroll: fixed cost argues for 0.08-0.1 SOL entries
  (2.0-2.5% fixed+fee) over 0.03 (7%+). ~10 concurrent x 0.05-0.1 fits the
  bankroll.

## 5. Actions - 5.1 and 5.2 executed 2026-07-29, 5.3/5.4 open

### 5.1 Run `fs3-00` in paper at 0.1 SOL - EXECUTED; it was NOT "nothing to build"

**Naming, because it confused the operator once:** `fs3-dev big [12.8-25.6)`
is a **fingerprint** name (the token filter), NOT a rule name. The rules on it
are `fs3-00 dev13 base`, `fs3-01 one-shot`, `fs3-02 dip 20`, `fs3-03 dip 30`,
`fs3-04 unarmed`. All sit in local PG as `paper` + `is_active=false`. Seeded by
`hunter/scripts/seed-flow-scalper-dev13-rules.sql`; evidence in
[wallet-analysis.md](../plans/strategies/wallet-analysis.md) "Dev-buy size".

What `fs3-00` expresses (dip-buy inside a creator-backed pump): watch only
tokens whose creator's own first buy was 12.8-25.6 SOL; require age >= 45 s,
pool 40-75 SOL, >= 45 SOL gross flow in the last 60 s, price >= 25% below its
30 s high, and 2 s net flow >= 0 (dump paused). Exit on armed trail
(`retrace >= 7`, `arm_above_pct 2`) OR `held >= 90` OR 30 s gross flow <= 3.

**Required change before running:** `buy_amount_lamports` is **0.5 SOL** (as of
07-29 the `fs3-00` row in local PG is already 0.1 SOL, the other four are 0.5);
the operator's bankroll is 2-3 SOL at 0.03-0.1 per entry. Set it to **0.1 SOL**.
Cost impact vs the 0.30 SOL the ladder measured at: fixed 2x0.001025/B goes
0.68% -> 2.05%, own impact 2B/vsol (vsol~57) goes 1.05% -> 0.35%, net **+0.67
pp/round trip**. So the measured `first`-fill +5.42%/ep -> ~+4.8% and the
`worst`-fill +3.48% -> ~+2.8%: still positive at both bounds. Expect **~12
episodes/day** (below the 10-50 target's midpoint - the `huge [25.6-38.4)`
band or a second dev-buy band is the throughput lever).

#### What re-running it actually found (07-29)

**The seeded rule is not the rule the numbers came from.** `seed-flow-scalper-dev13-rules.sql`
shipped `stop_loss = 12.0`; the ladder row those numbers come from (`N1 dev13 dip 25`,
plan `fp13` in `scripts/flow-scalper-ladder.ps1`) has **no stop overlay** -
`Use-64hpGeometry` calls `Set-StopLoss $null`. Simulated on 07-22..28, `first`
fill / `pumpfun_impact` / 0.30 SOL / conc 4 (identical to the recorded row):

| params | n | win | PnL | PF |
| --- | --- | --- | --- | --- |
| as recorded in the ladder CSV (07-28) | 74 | 59.5% | +1.203 | 1.67 |
| **no `stop_loss`** (re-run 07-29) | 82 | **59.8%** | **+1.226** | **1.65** |
| as seeded, `stop_loss 12` | 113 | 36.3% | -0.689 | 0.82 |

So the documented edge reproduces, and the 12% floor **destroys** it - it cuts
exactly the deep dip-entries whose recovery carries the tail, the same result the
`stop` ladder found on 64hP's own book. Fixed in the seed file, and applied
directly to local PG on 07-29: `fs3-00 dev13 base` now carries no `stop_loss`,
0.1 SOL, conc 10, 40 episodes, and is **armed** (`paper` + `is_active`). The four
parked ladder variants (`fs3-01..04`) and the whole superseded `fs2-*` ladder were
deleted along with their orphaned fingerprints - `seed-flow-scalper-dev13-rules.sql`
recreates them if a knob ladder is ever needed again (note it deletes `fs3-%`
first, so re-running it disarms the live rule).

**Sizing, measured rather than extrapolated** (no `stop_loss`, `pumpfun_impact`,
07-22..**29** so 07-28 is included; `first` fill is the optimistic bound, `worst`
is what live paper books):

| size / conc | fill | window | n | win | PnL | %/ep |
| --- | --- | --- | --- | --- | --- | --- |
| 0.30 / 4 | first | 22..29 | 104 | 55.8% | +1.553 | +4.98% |
| 0.10 / 10 | first | 22..29 | 104 | 54.8% | +0.470 | +4.52% |
| 0.10 / 10 | first | 22..28 | 82 | 58.5% | +0.371 | +4.53% |
| 0.30 / 4 | worst | 22..28 | 79 | 50.6% | +0.417 | +1.76% |
| 0.30 / 4 | worst | 22..29 | 101 | 46.5% | -0.134 | -0.44% |
| 0.10 / 10 | worst | 22..29 | 101 | 45.5% | -0.097 | **-0.96%** |

Downsizing costs ~0.46 pp/round trip (predicted +0.67). Concurrency never binds -
the taken set is identical at 4 and at 10 - so conc 10 is purely a bankroll guard
(peak exposure ~1.0 SOL vs 1.2 at 0.30x4). Two things move the `worst`-fill
bottom line away from the recorded +3.48%/ep: it is roughly halved on the same
window by the grown corpus (+1.76%), and **adding 07-28 alone flips it negative**
(-0.44% at the same 0.30 SOL). Size then takes it to **-0.96%/ep**.

**The honest bottom line: at 0.1 SOL the rule is +4.5%/ep under `first` and about
-1%/ep under `worst`,** i.e. the whole result now sits inside the fill-model
spread. That is exactly what a paper run measures - live paper books `worst`,
while the 07-28 wallet work measured 64hP's real fills at +1.18% vs `signal`
(near the optimistic bound). Run it to find out which bound reality sits near;
do not run it expecting +5%/ep, and do not promote it to real money on a
`first`-fill number.

**Throughput:** 104 episodes over 7 partial lake days, not the ~12/day estimate -
~15/day, still under the 10-50 target's midpoint.

**Operator step (still open):** the local DB is done, but the **live box's** DB is
not - `db-incremental-sync.ps1` is server->local and server-wins, so the local
arming neither propagates nor survives the next sync. To run it on EC2, apply the
same four statements there (drop `stop_loss` from `params`, `buy_amount_lamports
= 100000000`, `max_concurrent_tokens = 10`, `is_active = true` on `fs3-00`); ssh
from this session was blocked by the permission gate.

### 5.2 Creator / crew reputation index - EXECUTED 2026-07-29. Answer: yes, as a NEGATIVE screen

The question was: does a prior-launch-behaviour label move the day spread, or is
the variance purely market-wide? **It is not market-wide** (section 4 note: the
universe-wide follower EV is flat at -6.9..-8.2%/event on all seven days), and a
creator label moves the spread by **34 pp**.

**Method.** Everything re-derived from the sealed lake, no Helius. Per token:
creator = the wallet of the creation-slot buy; first-20 s tape; outcome labels
vs the t+20 s price. Follower sim = the section 3 mechanics (enter at the first print
>= t+22 s, trail 25% off the since-entry peak, hard stop -35%, 10 min cap, 3%
exit haircut, 125 bps/leg + 0.001025 SOL/leg + own impact, 0.10 SOL) run over
**every** token with an entry print (49,476 events), so every split below is a
filter on one priced table rather than a re-run.

**The event set had to be re-fit** (see the section 2 correction): the crew footprint is
`k >= 7` distinct wallets with a single >= 1 SOL buy inside 20 s, `<= 4` sells.
433 events / 7 days (~62/day), P(2x) 27.9%, follower EV **+15.9%/event**
(mean-of-day-means +11.1, day-mean t = 2.4) against the -7.7% universe baseline.
All of that EV is in the tail: the 77 events still open at the 10-min cap average
**+92%**, the 362 that trail out average **-0.8%**.

**Reputation keys tested** (all scored *causally* - a prior launch counts only if
its own 10-minute outcome window closed before the launch being scored was
created, so there is no look-ahead):

| key | on crew events |
| --- | --- |
| **K1 creator, no prior launch** (n=300) | **+24.3%/event**, 40% win, P(2x) 33.7% |
| **K1 creator, repeat, never 2x** (n=86) | **-11.5%/event**, 19% win, P(2x) 8.1% |
| K1 creator, repeat, has a 2x (n=47) | +12.6%/event, 34% win |
| K2 creation-tx signature (cu_price, cu_limit) | no separation (+3.3% vs +3.1% baseline OOS) |
| K3 dev-buy-size signature (the "everyone buys 1.975" tell) | weak: >=10% prior-pump rate = +27.2% vs +12.8%, but it rides on K1 |
| K4 early-backer cohort (>=1 SOL buyers' own history) | **inverted** - "no veteran backers" is the BEST cohort (+30.2% OOS); veteran-backed is worse, universe-wide too (-6.4% vs -2.9%). Same lesson as the refuted registry copy-trade: a wallet's past wins do not transfer |

**The screen that works is exclusion, not selection:** drop a launch whose creator
wallet has launched before and has **never** produced a 2x. It is a
non-transferable-registry-free label (it needs only the creator's own history) and
it is the one that separates the days:

| day | kept | EV kept | dropped | EV dropped |
| --- | --- | --- | --- | --- |
| 07-22 | 26 | +16.7% | 3 | -35.0% |
| 07-23 | 62 | +14.4% | 20 | -15.6% |
| 07-24 | 58 | +19.8% | 4 | +15.9% |
| 07-25 | 115 | +37.6% | 13 | +2.7% |
| 07-26 | 26 | +10.5% | 19 | -10.7% |
| 07-27 | 43 | +12.2% | 11 | -10.8% |
| 07-28 | 17 | +16.1% | 16 | -21.6% |

Kept is positive on **7 of 7 days** (unfiltered: 6 of 7, falling to 5 of 7 at one
print of exit latency and 2 of 7 at three - below). Held-out 07-26..28: kept
**+12.4%** on 86 events
vs dropped **-14.5%** on 46. Within-day label permutation (10k): observed gap
**34.1 pp**, **p = 0.0002**. Universe-wide the same label separates outcomes on
49k events (P(2x) 8.3% new-creator / 4.1% repeat-never-2x / 7.6% repeat-with-2x),
which is what rules out a small-sample artifact - though note the screen is NOT a
standalone edge: applied to non-crew launches every cohort is still -5 to -10%.

**Exit latency is the binding constraint, not the signal.** Pricing the exit at
the *next* print after the trigger instead of the trigger print itself:

| exit lag | crew rider | + creator screen |
| --- | --- | --- |
| 0 prints (idealised) | +15.9% / OOS +3.1% | +23.6% / OOS +13.2% |
| 1 print | +9.3% / OOS **-2.0%** | +16.6% / OOS +7.8% |
| 2 prints | +6.1% / OOS -4.4% | +13.2% / OOS +5.4% |
| 3 prints | +4.0% / OOS -6.1% | +11.1% / OOS +3.9% |

So the unscreened rider dies at any realistic latency and the screened one
survives to ~3 prints. It also survives trail 20/25% and caps of 300/600/900 s;
it weakens at trail 35% (OOS +1.9%, 5/7 days).

**What it would take to build.** Neither half is expressible today:
1. `k` (distinct wallets above a SOL threshold inside a trailing window) is not a
   metric - `m_flow_window` carries only summed SOL (`gross_flow`/`net_flow`/
   `buy`/`sell`). A `n_big_buyers`-style counting metric is new engine work.
2. The creator label needs a durable per-creator table (launches, and whether each
   made 2x) maintained by ingest, plus a fingerprint/entry axis that reads it.
   The lake already proves the label is computable; nothing produces it live.

**Is it worth BUILDING? On the evidence, no - not as a rider.** The cheap version
of the question is: does the creator label help the rule that is actually armed?
Joining the label onto `fs3-00`'s own 104 simulated episodes (0.10 SOL, `first`):

| creator cohort | eps | PnL | %/ep | win |
| --- | --- | --- | --- | --- |
| A new creator | 22 | +0.293 | +13.3% | 59% |
| B repeat, never 2x | 21 | +0.022 | +1.1% | 57% |
| C repeat, has 2x | 61 | +0.155 | +2.5% | 53% |
| **screened (drop B)** | 83 | +0.448 | **+5.4%** | 54% |
| all | 104 | +0.470 | +4.5% | 55% |

+0.9 pp/ep on n=104, against a per-episode sd of 9-15% - inside the noise floor.
The dev-buy fingerprint already selects funded creators, so the screen has little
left to remove there. Its large effect (+34 pp) exists only on the crew-footprint
cohort, and reaching that cohort costs two new engine subsystems (the counting
metric + the creator-history store) for an edge whose OOS estimate is +7.8%/ep at
one print of exit lag and +3.9% at three. The same effort spent on 5.3 lifts every
strategy, `fs3-00` included. Recommendation: keep the label as a **known,
re-derivable screen** to apply if and when a launch-window rule is built; do not
build the rider now.

**Caveats.** 433 crew events over 7 partial days; the k >= 7 threshold and the
screen were both chosen on this window (the held-out 26-28 numbers are the honest
ones); the sim prices its own impact but not queue position; and the strongest
single cohort ("creator has never launched before") is exactly the cohort a crew
can manufacture for free by rotating wallets, so treat it as a floor on how long
the edge can last, not as a moat.

### 5.3 Partial exits (tranched exit) - the structural engine gap

> **07-29 evidence, unchanged verdict, sharper number.** On the crew rider the
> split is stark: the 77 events still open at the 10-min cap average **+92%**, the
> 362 that trail out average **-0.8%**. A single all-or-nothing exit has to choose
> between banking the 362 and keeping the 77; a first tranche into strength plus a
> trailed stub is the only shape that does both.

Section 3 measured both degenerate cases of a single all-or-nothing exit:
fixed TP is -10..-18%/event (caps the tail that carries all the EV), pure
trailing is positive but returns 25-30% of every winner. Both profitable
references resolve this the same way - omego is net-positive ONLY via the ~19%
he lets ride ([[flow-scalper-wallet-64hp]] / armed-trailing-stop memory), and
the crews distribute into strength rather than at one moment. `arm.rs`/
`reduce.rs` have no scale-out concept; every exit closes 100%. Target shape: a
first tranche banked into strength (covers fee + gives the high hit rate the
operator wants) plus a trailed stub for the 5x tail. Engine work, not tuning.

### 5.4 Regime thermostat - cheap drawdown insurance

> **07-29 evidence weakens the rationale.** The premise was that the day spread is
> a market regime. Measured, it is not: the universe-wide follower EV is flat
> (-6.9..-8.2%/event) and P(2x) is flat (6.6-7.9%) across all seven days, while the
> crew cohort swings +34..-2%. A thermostat reacting to *realized* EV would have
> been reacting to which launches the signal picked, one day late - the creator
> screen in 5.2 addresses the same variance at the source and on the same day. The
> case for a thermostat is now bankroll protection (a 2-3 SOL account cannot sit
> through a -20% streak) rather than edge recovery, so it should be sized as
> insurance, not as a source of return.

Day-level EV spread (-17..+25%/event) is an order of magnitude larger than any
knob delta ever measured here, and the operator's 2-3 SOL bankroll makes a bad
streak existential rather than merely unpleasant. A supervisory layer over the
rules - full size while trailing realized EV is healthy, half size when it
turns, pause after a deeper streak, re-probe small - does not raise good-day
profit; it truncates the bad-day tail. Cheaper than any entry refinement and
independent of which strategy ends up live.

Scripts (scratchpad, session 2026-07-29 #1): a1_wallet_mining / b1_classify /
c1_token_side / d1_signals / e1_follower_sim / f1_validate. Intermediates:
visits.parquet, wallets.parquet, tokfeat.parquet, sig_{B,E,I}.parquet.
Session #2 (5.1/5.2) re-derived everything from scratch in six steps, which is
the recipe to repeat: `s1_base` (per-token creation features + 20 s tape + outcome
labels -> tok.parquet, ~3 min) -> `s2b_sim_all` (the follower sim over all 49k
entry-print tokens -> allsim.parquet + big20.parquet, ~4 min) -> `s3_rep` (causal
creator / cu-sig / dev-size / backer histories -> features.parquet) -> `s4_eval`
(regime test, fit-vs-held-out filter table, bootstrap CIs) -> `s5_robust`
(exit-lag and knob sensitivity) -> `s6_strict` (the no-look-ahead creator label:
a prior launch counts only once its own 10-min outcome window closed). The lab
simulate runs used `run-fs300*.ps1` (POST `/api/strategies/simulate` with an
inline draft, same shape as `flow-scalper-ladder.ps1`).
Scratchpad is session-scoped - a later session re-derives them from the lake
(keep DuckDB at `memory_limit='3-5GB'`; run nothing heavy while a lab fold is
in flight, and note `block_time`/`created_at` are **microseconds** and that all
same-slot trades share one `block_time`, so "age > 0" excludes the whole creation
slot rather than just the create tx).
