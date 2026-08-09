# Creator-reputation + launch-crew screen — refuted (2026-08-03)

> **History.** The mechanical validation run that closed the creator-reputation screen. Kept because the approach was **refuted**, not merely unfinished: without this record the `p=0.0002` result it started from reads as a live lead and gets re-mined.
>
> Nothing here is a current instruction. The rules that survived this work live
> in `CLAUDE.md` and `docs/plans/strategies/`.

---

Follow-on to `wallet-mine-2026-08-03.md`, which closed wallet-copying. That doc's
closing recommendation was to test the creator-reputation screen — the one signal in
this repo that had survived a permutation test and a holdout (`p=0.0002`, 07-29). This
document is that test, run with the same harness that refuted F4.

**Verdict: the creator screen is real but redundant, the crew footprint is one day of
data wearing a mean, and the dev-buy fingerprint does not replicate as net-profitable.
Nothing here is armable. One sub-band is worth more data, not money.**

---

## 0. TL;DR

1. **The creator screen's headline effect is 94% dust removal.** "Repeat launcher, never
   made a 2x" separates P(2x) by **+26.1pp** raw — but **82% of the dropped cohort has
   fewer than 5 trades in its first 20s**. Only 3,072 of 48,534 dropped tokens survive a
   minimum-tape filter. Inside a tradeable universe the screen is worth
   **+0.0005 to +0.0010 SOL/episode** — correct in sign on both splits, an order of
   magnitude inside the noise band.
2. **`bigbuyers >= 10` is refuted.** Apparent +35.1% holdout gross, but **19 of its 32
   holdout episodes fall on 07-25 alone, contributing 98% of holdout PnL**. Day-block
   bootstrap CI `[-25.9, +56.3]`. It is one good day, not a strategy.
3. **`dev_buy >= 12.8 SOL` does not replicate as net-profitable.** Net/ep
   `+0.0011 [-0.0024, +0.0050]` over 12 days, `-0.0007` on holdout. Straddles zero
   everywhere. The 07-28 claim that it is a *monotone threshold family* **fails on this
   window** — see §4.
4. **Only `dev_buy ∈ [12.8, 16)` is net-positive with a CI clearing zero** (12-day
   `+0.0071 [+0.0013, +0.0132]`), and on the 4-day holdout alone it straddles
   (`[-0.0064, +0.0109]`). It is a post-hoc single bucket — the exact failure mode that
   killed three earlier gates.
5. **Survivability fails your own bar.** Best candidate ruins a 1 SOL stack **21.9%** of
   the time in 30 days; the others 69–80%. Median max drawdown 1.27 SOL on a 1 SOL stack.
6. **Size runs the wrong way.** Depth impact (`2·size/vsol` against a ~45 SOL pool) grows
   faster than the fixed tip amortizes. At 2 SOL/trade every gate loses 5–10× more per
   episode than at 0.1. **This trade class does not scale** — relevant to your stated
   plan to extend capital later.

---

## 1. Method

Same discipline that refuted F4, so the results are directly comparable.

| | |
| --- | --- |
| Corpus | 12 lake days, 2026-07-22..08-02, 12.57M trades in the 0–900s window |
| Creator identity | derived — pump.fun's dev buy rides in the creation tx, so the first print at age≈0 is the creator. **Reconciles against `fp_initial_buy_sol` for 95.3% of tokens** (159,192 of 167,046); `unknown:*` wallets excluded |
| Creator history | **point-in-time** — a token's score uses only launches that finished strictly before its own `created_at` (SQL window `ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING`) |
| Entry | observation window closes at t+20s, +2s latency ⇒ fill at the **first real print at or after t+22s** |
| Exit | +1s latency on every trigger |
| Cost | 125bps/leg curve fee + `0.00205` SOL fixed (tip×2 + priority) + `2·size/vsol` depth impact |
| Portfolio | explicit slot limit; concurrency swept 1→20 |
| Split | **holdout 07-23..07-26** (never touched by the wallet mine), in-sample 07-27..08-02 |
| Control | liquidity-matched random entry, same age and vsol band, no behavioural condition |

Scripts: `c1_devwallet` → `c12_survive` in the session scratchpad.

---

## 2. The creator screen is mostly a dust detector

Raw separation is enormous and completely misleading:

| cohort | n | P(2x in 10m) | median peak | median trades in 20s | % with <5 trades |
| --- | ---: | ---: | ---: | ---: | ---: |
| no history | 40,190 | 30.29% | 1.209 | 7.0 | 40.4% |
| **repeat, never 2x → DROP** | 48,534 | **2.24%** | **1.000** | **2.0** | **82.2%** |
| repeat, has 2x → KEEP | 68,702 | 27.17% | 1.508 | 18.0 | 17.8% |

`median peak = 1.000` is the tell: the dropped cohort's typical token **never printed a
second price**. These are spam launches you could not have traded, not bad trades you
avoided.

Conditioning on a tradeable universe collapses the effect:

| universe | kept | P2x keep | dropped | P2x drop | lift |
| --- | ---: | ---: | ---: | ---: | ---: |
| all tokens | 108,892 | 28.32% | 48,534 | 2.24% | **+26.08pp** |
| has real tape (n20≥15) | 54,095 | 50.81% | 3,072 | 28.91% | +21.90pp |
| **tape + liquidity (vsol 40–60)** | 32,760 | 62.13% | 1,253 | 55.71% | **+6.43pp** |

Only **6.3%** of the dropped cohort survives `n20>=15`. The screen's unique contribution
is confined to those tokens, and once liquidity is also controlled the lift is +6.4pp on
an *outcome proxy* — which §3 shows does not convert to money.

**Marginal value on net expectancy** (conc 5, 0.1 SOL, hold 60s):

| universe | without screen | with screen | delta |
| --- | ---: | ---: | ---: |
| devbuy≥12.8 · HOLDOUT | −0.0012 | −0.0007 | **+0.0005** |
| devbuy≥12.8 · in-sample | +0.0009 | +0.0019 | **+0.0010** |

Consistently positive in sign on both splits — the screen is not noise, it is just small.
Against a per-episode standard deviation in the tens of percent, +0.0005–0.0010 SOL/ep is
unmeasurable in any live run you could afford. This **confirms and sharpens the 07-29
verdict** (+0.9pp/ep on fs3-00's own episodes, "verdict on building it: NO") — and now
explains *why* the raw effect looked so much bigger than the usable one.

---

## 3. P(2x) is not money — the crew footprint

The crew footprint looked spectacular on the outcome proxy: `>=5 big buyers, <=2 sells in
20s` gives **91.98% P(2x)** vs a 20.28% base. That number is an artifact. P(2x) is measured
from the **creation price**, and a token with 5+ one-SOL buyers inside 20 seconds has
already made most of that move before t+22s. **You cannot buy the leg you are using to
select.**

Priced from a real t+22s fill, one slot, hold 60s:

| cohort | split | n | win% | mean gross | net/ep | fires/day |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| crew7 | HOLDOUT | 112 | 28.6 | −0.4% | −0.0049 | 33 |
| crew7 | in-samp | 106 | 19.8 | −8.4% | −0.0127 | |
| crew5 | HOLDOUT | 191 | 25.7 | −2.7% | −0.0071 | 210 |
| crew5 | in-samp | 356 | 24.4 | −7.7% | −0.0121 | |
| **CONTROL** liq-matched random | HOLDOUT | 924 | 13.0 | **−5.9%** | −0.0103 | |
| **CONTROL** liq-matched random | in-samp | 1,749 | 13.7 | **−6.0%** | −0.0104 | |

The gates **do** beat the control — win rate 13% → 26–46%, mean gross −6% → roughly flat.
That is +6 to +9pp of genuine alpha, far more than F4's +0.92pp. It is simply still short
of the cost floor, and it does not survive the split.

`bigbuyers >= 10` is the one crew cell that appeared to clear everything (+35.1% holdout
gross, episode-bootstrap CI `[+2.9, +67.6]`). It is an artifact of clustering:

| day | n | % of eps | share of holdout PnL |
| --- | ---: | ---: | ---: |
| 07-23 | 2 | 6% | −11% |
| 07-24 | 8 | 25% | +1% |
| **07-25** | **19** | **59%** | **+98%** |
| 07-26 | 3 | 9% | +13% |

Episodes inside a day share a regime, so the **day** is the independent unit. A day-block
bootstrap widens the CI to `[-25.9, +56.3]` — straddling zero, let alone the fee floor.
Dropping its 2 best in-sample episodes takes the mean from +14.4% to **−2.6%**.

> **Reusable rule:** never bootstrap these episodes as if independent. Day-block always.
> The episode bootstrap said "real"; the day-block bootstrap said "one lucky day", and the
> day-block one is right.

---

## 4. The dev-buy fingerprint does not replicate

Memory (`dev-buy-fingerprint-outcome-signal`, 07-28) records `init_buy >= 12.8 SOL` as
surviving *because* it was a **monotone threshold family**, not a best-of-N bucket. On
this window, using non-nested bands, that monotonicity is absent:

| band (SOL) | n | win% | median | mean | holdout / in-sample |
| --- | ---: | ---: | ---: | ---: | --- |
| [8, 12.8) | 1,370 | 22.6 | −6.7% | −4.1% | −4.6 / −3.4 |
| **[12.8, 16)** | 421 | 34.4 | −2.7% | **+12.5%** | **+8.9 / +14.2** |
| [16, 20) | 234 | 40.6 | −3.3% | −0.5% | +0.6 / −1.9 |
| [20, 25.6) | 133 | 41.4 | −7.0% | −2.4% | −3.9 / −2.1 |
| [25.6, ∞) | 114 | 57.9 | +1.3% | +6.7% | +7.2 / +7.6 |

The `>= 12.8` threshold's power comes almost entirely from `[12.8, 16)`; the two bands
immediately above it are flat-to-negative. That is a **U-shape, not a monotone family** —
so on this corpus the gate is exactly the "best-of-N single bucket" construction that
memory names as the failure mode which killed `range`, rise-at-low, and `rise <= 1`.

Tail concentration confirms the fragility of the aggregate threshold:

| cell | split | n | mean | −top1 | −top2 | −top5 | median |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| devbuy≥12.8 | HOLDOUT | 259 | +4.4% | +2.3% | +1.3% | **−1.4%** | −2.3% |
| devbuy≥12.8 | in-samp | 619 | +7.0% | +6.1% | +5.3% | +2.8% | −2.9% |

**Five episodes out of 259 carry the entire holdout edge**, and the median episode loses
2.3%. Per-day, only 7 of 12 days clear the fee floor and the median is negative on 9 of 12.

---

## 5. Concurrency — the one place the analysis was wrong before

The single-slot sim was materially understating these gates. With 89 fires/day and 60s
holds, one slot takes a near-random subset in arrival order. Your 1 SOL at 0.1/trade
supports ~10 slots:

`devbuy ∈ [12.8,16) + screen`, 0.1 SOL, hold 60s:

| conc | split | n | win% | gross | net/ep | net SOL | MDD |
| ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | HOLDOUT | 93 | 32.3 | +4.61% | −0.0000 | −0.00 | 0.66 |
| 3 | HOLDOUT | 111 | 28.8 | +4.22% | −0.0004 | −0.05 | 0.41 |
| **5** | **HOLDOUT** | **116** | **29.3** | **+8.40%** | **+0.0037** | **+0.42** | **0.40** |
| 5 | in-samp | 300 | 27.7 | +13.70% | +0.0088 | +2.65 | 0.69 |
| 10 | HOLDOUT | 116 | 29.3 | +8.40% | +0.0037 | +0.42 | 0.40 |

Concurrency saturates at 5 — beyond that the slot never binds. This is the **first
configuration in this line of work that is net-positive on a genuine holdout** with honest
costs, latency, and depth impact.

It still is not established. Day-block CI on net SOL/episode:

| gate | scope | days | n | net/ep | 95% CI | verdict |
| --- | --- | ---: | ---: | ---: | --- | --- |
| devbuy≥12.8 +screen | ALL 12d | 12 | 901 | +0.0011 | [−0.0024, +0.0050] | straddles zero |
| devbuy≥12.8 +screen | holdout | 4 | 258 | −0.0007 | [−0.0055, +0.0030] | straddles zero |
| **devbuy [12.8,16) +scr** | **ALL 12d** | 12 | 421 | **+0.0071** | **[+0.0013, +0.0132]** | **profitable** |
| devbuy [12.8,16) +scr | holdout | 4 | 116 | +0.0037 | [−0.0064, +0.0109] | straddles zero |
| devbuy≥12.8 no screen | ALL 12d | 12 | 931 | +0.0003 | [−0.0035, +0.0046] | straddles zero |

One cell clears zero, over the full corpus only, as a post-hoc bucket.

---

## 6. Survivability — your stated ranking criterion

1 SOL stack, 0.1 SOL/trade, 5 slots, 30 trading days, 4,000 bootstrap paths:

| gate | median end | **P(ruin < 0.3)** | P(loss) | median maxDD | p95 maxDD |
| --- | ---: | ---: | ---: | ---: | ---: |
| devbuy [12.8,16) +screen | 7.87 | **21.9%** | 22.0% | 1.27 | 2.24 |
| devbuy≥12.8 +screen | 0.29 | **69.0%** | 69.4% | 1.55 | 3.74 |
| devbuy≥12.8 no screen | 0.29 | **79.8%** | 80.3% | 1.40 | 3.76 |

The median-end 7.87 SOL is the right tail talking. The number that governs a 1 SOL stack
is **median max drawdown 1.27 SOL** — larger than the entire bankroll. You do not reach
the median outcome; you are ruined on the way to it roughly one run in five, and that risk
is front-loaded into the first weeks.

---

## 7. Size does not rescue it — and scaling makes it worse

You asked for extensibility. It runs backwards. The fixed tip amortizes with size but
depth impact `2·size/vsol` grows against a ~45 SOL pool:

`hold 60s`, net SOL/episode:

| gate | 0.1 SOL | 0.5 SOL | 2.0 SOL |
| --- | ---: | ---: | ---: |
| crew5+screen HOLDOUT | −0.0030 | −0.0146 | −0.1680 |
| crew7+screen HOLDOUT | −0.0016 | −0.0068 | −0.1276 |
| devbuy12.8+screen HOLDOUT | −0.0041 | −0.0204 | −0.1976 |

At 2 SOL the impact term alone is ~8.9% round trip — **3.5× the entire 2.53% fee budget**.
These are small-pool trades; the pool is the binding constraint, not the tip.

**Partial exits also hurt** (scale half at +25%, rest on trail 15/stop 25), on every gate
and both splits — e.g. crew7+screen holdout +0.0016 → −0.0173. This reproduces the 07-29
finding that EV here is all right tail and capping it kills the strategy.

---

## 8. What to do

**Do not arm anything in this document.** Nothing clears the bar you set (survivability
first), and the one positive cell is a post-hoc bucket whose holdout CI includes loss.

Ranked, honestly:

1. **Do nothing with real money.** No cell here is established. The best one carries a
   21.9% ruin probability on your stack.
2. **If you want to keep this alive, spend days, not SOL.** The single question worth
   answering is whether `dev_buy ∈ [12.8, 16)` is a real band or a lucky bucket. That
   needs corpus, not tuning: sync more EC2 days and re-run `c8`/`c12`. If the band holds
   its shape across 25–30 days with a holdout CI clearing zero, it becomes arguable. If
   the peak wanders to a different bucket, it was noise and this closes too.
3. **Keep the two reusable instruments**, which are the durable output of this work and
   of `wallet-mine-2026-08-03.md`:
   - the **liquidity-matched random control** (base rate −5.9%/−6.0% at this horizon), and
   - the **day-block bootstrap**. Between them they killed F4, `bigbuyers>=10`, and the
     `devbuy>=12.8` aggregate in about an hour each. Run them *before* building anything.
4. **Drop the creator screen as a build target, permanently.** Three independent looks
   (07-29, and §2 here on both the outcome proxy and net expectancy) agree it is worth
   under +1pp inside a tradeable universe. It requires a creator-history store to
   implement. That is a subsystem for an effect you cannot measure live.

---

## 9. Caveats

- **12 days, partial coverage.** Lake days are not uniform (07-22 has ~6h of tape). Day
  counts in the block bootstrap are 12 and 4 — the holdout CIs are wide *because four days
  is genuinely little evidence*, not because the method is conservative.
- **Creator history is window-limited.** "Never made a 2x" can only see prior launches
  inside the corpus, so it over-identifies high-frequency launchers and under-identifies a
  creator whose 2x happened before 07-22. A longer corpus would strengthen the screen's
  measured effect — but §2 shows its ceiling inside a tradeable universe is small anyway.
- **`peak10m` uses the first print as base**, which is the dev-buy fill. This is
  consistent across all tokens but is not the curve's pre-buy start price.
- **Entry is a fixed t+22s**, chosen to sit just past the 20s observation window. It is
  not optimised, deliberately — optimising entry timing on the same window that selects
  the gate is how the earlier bucket gates died.
- No Helius calls were made. All analysis is local lake/DuckDB.
