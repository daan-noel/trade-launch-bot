# Profitable-wallet mine — wallet-copying closed (2026-07-27 → 08-02)

> **History.** The 7-day mine that closed wallet-copying as a strategy class. Kept because it is a **closed search**: the ranking pass, the latency model and the negative-at-zero-latency result are the reason nobody should re-run it.
>
> Nothing here is a current instruction. The rules that survived this work live
> in `CLAUDE.md` and `docs/plans/strategies/`.

---

Follow-up to [wallet-analysis.md](../plans/strategies/wallet-analysis.md) (omego / 64hP / 63ot). Goal: find
*other* wallets running the same family of logic, rank them for a **1 SOL bankroll at
0.01–0.1 SOL/trade**, survivability first.

Source: local lake `hunter/lake-data` (7 sealed days, 9.08M trades, ~400k distinct
wallets, 110k mints). No Helius calls. Scripts in the session scratchpad
(`stage1b_conserved.py` → `stage7_f4check.py`).

---

## 0. TL;DR

1. **Your budget sets a hard floor at 0.1 SOL/trade.** The Jito tip is *fixed* per leg,
   so it becomes a percentage tax that explodes as size shrinks. At 0.01 SOL you need a
   **23.6% gross winner just to break even**. Trading 0.01–0.05 SOL is not a strategy
   problem, it is an arithmetic one.
2. **Nearly every profitable wallet's edge is latency, not logic.** Delay entry by
   **one second** and the best momentum fleet goes `+16.84% → −1.91%` median return,
   win rate `68% → 35%`. This generalises the omego verdict: it was never specific to
   omego.
3. **Exactly one fleet survives realistic latency** — `3E6Eq2…Zv2c` + `CAwkpF…14J9`
   (F4). At +2s and 0.10 SOL it still returns **+0.0082 SOL/episode, 0% ruin risk,
   1.0 → 2.64 SOL median over 200 trades**.
4. **But F4 is a right-tail lottery, not a "stable" strategy.** Its *median* episode
   is −0.0018 SOL. 16% of episodes returning ≥ +50% carry the entire expectancy.
   That is the opposite of the survivability profile you asked to optimise for.
5. **F4 was then validated mechanically and FAILED — see §9.** Applied to every token it
   loses on all 12 days, in-sample and holdout, under every exit policy, **and is negative
   even at zero latency**. Its edge is not in any observable entry condition. Nothing here
   is armable; the wallet-copying direction is closed.

---

## 1. The cost floor — read this before anything else

Two cost layers. Only one of them scales.

| layer | size | at 5 SOL | at 0.1 SOL | at 0.01 SOL |
| --- | --- | --- | --- | --- |
| curve fee (125bps × 2 legs) | proportional | 2.53% | 2.53% | 2.53% |
| tip + priority (`JITO_MIN_TIP_SOL` 0.001 × 2 + ~0.00005) | **fixed 0.00205 SOL** | 0.04% | **2.1%** | **20.5%** |

Break-even gross return per round trip:

| trade size | needs |
| --- | --- |
| 0.01 SOL | **23.57%** |
| 0.02 SOL | 13.06% |
| 0.05 SOL | 6.75% |
| **0.10 SOL** | **4.65%** |
| 0.25 SOL | 3.39% |
| 0.50 SOL | 2.97% |
| 5.00 SOL | 2.59% |

Every wallet mined here trades 0.5–5 SOL, where the tip is invisible. Their published
edge must be re-priced before it means anything to you — that is what §4–5 do.

**The one upside of small size:** at 0.1 SOL your size-vs-depth impact is ≈0 against a
~48 SOL vsol pool. The `PumpfunImpact` term that refuted the omego copy-trade thesis
does not bind on you. Your enemy is the tip, not the curve.

---

## 2. How the population was filtered

Most "profitable wallets" are not strategies. The funnel matters more than the ranking:

| gate | survivors | what it removes |
| --- | --- | --- |
| ≥25 mints, ≥4 days, gross-profitable | 2,569 | one-off punters |
| **token conservation** (`tok_out ≤ tok_in×1.02`) | 1,712 | creator/airdrop/transfer endowment |
| ≥30 clean reconstructed episodes | 1,683 | — |
| taint ≤2% (per-episode conservation) | 1,612 | wallets clean on 300 tokens, endowed on 3 |
| atomic ≤20% (buy+sell within 1 slot) | 1,538 | MEV / sandwich / atomic arb |
| **not a bundler** (entry age ≥5s) | 799 | **807 dev-bundle wallets — half the population** |
| ≥6 of 7 days active, ≥40 episodes | 496 | one-lucky-day actors |
| net-positive **at 0.10 SOL** | 375 | edges smaller than your tip |
| max drawdown ≤5 position sizes | 320 | blowup risk on a 10-position stack |
| bag rate ≤12% | 294 | the 64hP failure mode |
| median return ≥4.65% (your break-even) | **100** | — |

Two traps worth naming, because a naive PnL sort is dominated by them:

- **Endowment.** Top of the raw ranking was a wallet at +1,664% of buy volume. It bought
  2.3e8 tokens for ~0 SOL and sold 1.7e13 — a 73,504× token ratio. Conservation must be
  enforced **per episode**, not per (wallet, mint).
- **MEV.** A 5-second hold at 95% win and +60% of turnover is not directional trading.
  Those wallets profit *from* flow like yours.

Also: `block_time` in the lake is **microseconds**, and `sol_amount` is **curve-side
(fee-exclusive)** — `0.098765 = 0.1 × 0.9875`. Both will silently corrupt any analysis
that assumes otherwise.

---

## 3. The two families that survive

Fleets detected by identical (size, entry-age band, hold band) — one operator running
several wallets.

| fleet | wallets | eps | entry age | hold | % off 60s high | ret 60s | buy frac | win | med ret |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| **F1 momentum-snipe** | `28eGXe`,`CDyhUV` | 945 | 8s | 8s | **+4.7%** | **+13.6%** | 0.68 | 75.7% | +16.8% |
| **F2 momentum-snipe** | `55pbD9`+5 | 1465 | 17s | 7s | −2.7% | +11.4% | 0.63 | 72.5% | +11.9% |
| **F9 momentum-snipe** | `H8UKNv` | 736 | 8s | 15s | +7.1% | +14.5% | 0.68 | 73.9% | +12.8% |
| **F4 pullback-snipe** | `3E6Eq2`,`CAwkpF` | 172 | 5s | 12s | −4.4% | +2.4% | **0.77** | 65.7% | +9.3% |
| **F7 dip-scalp** | `P5uiFH`+3 | 205 | 261s | 51s | −7.0% | +4.1% | 0.57 | 72.5% | +17.4% |
| **F3 dip-scalp** | `Doir5a`+3 | 216 | 125s | 55s | **−31.6%** | +4.0% | 0.51 | 65.7% | +6.4% |
| **F10 dip-scalp** | `4euyaq` | 207 | 22s | 52s | −19.6% | +5.5% | 0.59 | 57.0% | +5.9% |
| **F8 micro (0.1 SOL!)** | `A8KEE1`+2 | 454 | 19s | 7s | −12.5% | +0.9% | 0.63 | 66.1% | +8.0% |

**Momentum-ignition** (F1/F2/F9) buy *into* strength — at or above the 60s high, after a
+11–14% 60s move, at token age 5–17s, and flip in 7–15s. This is the trunoest family,
not the omego dip-reversion family. All are 7/7 positive days.

**Dip-reversion** (F3/F7/F10) buy pullbacks 20–32% off the 60s high at age 22–261s and
hold ~52s. This is the omego/64hP/63ot family. Notably **less** stable: F3 4/7 and F7 5/7
positive days.

---

## 4. Latency kills almost all of it

Same trigger, entry filled *d* seconds later, exit held fixed at the price the wallet
actually got. Median gross return:

| fleet | +0s | +1s | +2s | +3s | +5s | +10s |
| --- | --- | --- | --- | --- | --- | --- |
| F1 momentum | **+16.84** | −1.91 | −3.01 | −2.18 | +0.47 | +3.31 |
| F2 momentum | **+11.90** | −2.19 | −0.99 | −0.83 | +0.54 | +0.86 |
| F9 momentum | **+12.83** | −1.50 | −3.24 | −4.61 | −2.47 | +0.19 |
| **F4 pullback** | **+9.33** | −0.08 | **+2.76** | **+3.04** | **+3.23** | **+3.02** |
| F5 pullback | +8.13 | −2.19 | −0.84 | −0.97 | +0.89 | +2.09 |
| F3 dip-scalp | +6.37 | −1.73 | −1.16 | −1.47 | −1.92 | −1.14 |
| F7 dip-scalp | **+17.41** | −5.59 | −1.37 | 0.00 | +0.99 | +3.25 |
| F10 dip-scalp | +5.85 | **−25.96** | −25.85 | −26.08 | −26.48 | −25.86 |
| F8 micro | +7.96 | +4.63 | +2.18 | +0.98 | +0.02 | +0.15 |

Net-of-cost win rate at 0.10 SOL collapses in step: F1 `68.4% → 34.7%`, F5 `57.7% →
21.6%`, F10 `51.2% → 11.6%`.

**Read this as: their alpha is the first second.** F10 is the clearest case — its entry
is a precision knife-catch that is worth −26% to anyone who arrives late.

> This test is still **optimistic**: it delays your entry but lets you exit at the
> wallet's exact exit price. A real bot is late on both legs.

---

## 5. Bankroll survival — 1 SOL, 0.10 SOL/trade, +2s late, 200 trades

4,000 bootstrap paths (right-skewed returns; the mean path and median path differ wildly).

| fleet | n | win% | mean/ep | med/ep | P(ruin) | med end | p10 end | med MDD |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| **F4 pullback** | 172 | 45.3 | **+0.0082** | −0.0018 | **0.0%** | **2.64** | 2.10 | 0.16 |
| F7 dip-scalp | 205 | 42.9 | +0.0032 | −0.0059 | 1.3% | 1.62 | 0.83 | 0.45 |
| F3 dip-scalp | 216 | 36.6 | +0.0013 | −0.0057 | 1.1% | 1.24 | 0.64 | 0.41 |
| F8 micro | 454 | 42.5 | −0.0016 | −0.0024 | 1.4% | 0.68 | 0.34 | 0.45 |
| F1 momentum | 945 | 28.0 | −0.0053 | −0.0075 | **77.5%** | 0.10 | 0.08 | 0.91 |
| F2 momentum | 1465 | 35.0 | −0.0042 | −0.0055 | **43.3%** | 0.16 | 0.08 | 0.90 |

Copying the *best-looking* wallets (F1/F2) blows up a 1 SOL stack 43–78% of the time.

**F8 is the reality check that matters most:** a real operator already trading at your
size, 0.09–0.13 SOL, is **net negative** once you are 2s late. Somebody is running your
exact plan and losing.

---

## 6. F4 in detail — the only realistic candidate

`3E6Eq28ZzG2gE1wna6BoDFaPVgbY8edJqQNF5mqcZv2c`
`CAwkpFhxxkw8vYNNhBt7HyPVsndHtjaRyys73KVr14J9`

Entry gate (medians at +2s, p25 / med / p75):

| gate | p25 | med | p75 |
| --- | --- | --- | --- |
| token age | 1.4s | **5.3s** | 22.5s |
| trades in prior 60s | 14 | **28** | 67 |
| buy fraction prior 60s | 0.69 | **0.77** | 0.89 |
| % off prior-60s high | −15.6% | **−4.4%** | +2.3% |
| vsol at entry | 39.8 | **48.2** | 58.7 |

Shape: a *shallow* pullback (−4%, not −30%) on a very young token with **lopsided buy
pressure (77%) but modest flow (28 trades)**. It is a momentum-continuation entry, not a
reversion entry. Exit after ~12s.

By day at +2s / 0.10 SOL: `+0.344, +0.036, +0.171, −0.015, +0.529, +0.266, +0.077` →
**6/7 positive, +1.408 SOL total.**

Tail concentration — drop the best winners:

| drop top | net | |
| --- | --- | --- |
| 1 | +1.310 | positive |
| 3 | +1.119 | positive |
| 5 | +0.932 | positive |
| 10 | +0.520 | positive |
| **20** (of 172) | **−0.069** | **negative** |

So ~12% of episodes carry it. Robust to a handful of lucky trades, **not** robust to the
tail regime disappearing.

Realistic return distribution (+2s): p05 −22.7%, p25 −8.5%, **med +2.8%**, p75 +24.0%,
p90 +59.4%, p95 +87.9%. Mean +13.0%.

**Median +2.8% is below your 4.65% break-even.** You lose slowly on most trades and are
paid by the 16% that run ≥+50%.

---

## 7. Recommendations

**On sizing — do this regardless of strategy:**

- **Never trade below 0.1 SOL.** At 0.01 SOL the tip alone is a 20.5% tax.
- With 1 SOL, run **0.1 SOL × 1 concurrent position**. 10 positions of ammo.
- The highest-leverage change available is **not a better rule — it is a bigger unit**.
  Going 0.1 → 0.5 SOL cuts your break-even from 4.65% to 2.97%, which is worth more than
  any entry-gate tuning in this document. If the bankroll can reach 5 SOL, do that before
  optimising anything else.

**On strategy — combining what actually holds up:**

- **Entry (from F4, the only latency-robust gate):** token age ≤30s; buy-fraction ≥0.70
  over prior 60s; entry within −15%..+2% of the prior-60s high; prior-60s trade count
  15–70; vsol 40–60 SOL. Note this is *momentum continuation on a shallow dip* — it is
  **not** the deep-dip reversion of omego/63ot, and the deep-dip fleets (F3, F10) are the
  ones that die worst on latency.
- **Exit — do not use a tight TP.** The expectancy lives entirely in the +50–100% tail.
  A fixed TP+17 bracket (63ot's shape) would clip exactly the winners that pay for
  everything. Use a **trailing stop armed above ~+15%** (`m_position.arm_above_pct`
  already exists) with hard SL ~−25% and a **stall/time exit at 60–90s**.
- **Keep the stall exit.** 64hP's only defect was 227 bags / −225 SOL from not having
  one. Every fleet here that lacked one shows it in the bag rate.

**On expectations — the honest read:**

You asked for stable, reliable, safe, and profitable. On this window the data supports
**at most three of those four**. The one candidate that survives realistic latency does
so with a *negative median trade*, paid for by a 16% tail. That is positive expectancy,
but it is not stability, and a 1 SOL stack feels every bit of the variance.

If the requirement is genuinely "stable and safe", the correct conclusion from this mine
is that **no wallet in a 400k-wallet, 7-day window offers it at 0.1 SOL** — the fixed tip
plus a 1-second latency handicap consumes the entire edge of every strategy that isn't
winning a speed race.

---

## 8. Caveats

- **In-sample.** Fresh 7-day window, no holdout (per the requested scope). §4/§5 are
  robustness tests, not out-of-sample validation. Prior work in this repo has seen
  registry-copy and fixed-TP results refuted OOS — assume the same risk here.
- **n=172** for F4, from **one operator** (2 wallets). Thin.
- The latency model delays entry only; exits are held at the wallet's realised price, so
  all delayed figures are **upper bounds**.
- Fill probability is not modelled — being 2s late assumes you still get filled.
- Bags marked to last in-window price; tokens that die after the window are optimistic.

## 9. VALIDATION — the F4 gate was tested mechanically and **FAILED**

Ran the gate as a real bot over all 12 lake days: every token, one global position slot,
125bps/leg + `FIXED_RT` 0.00205 + `2*(buy_sol/vsol)` impact, +2s entry / +1s exit latency.
Holdout = **07-23..07-26**, four days never touched by the mine.

**First red flag:** the gate fires on **17% of every token created — 2,000–3,700 signals
per day**. The F4 wallets took ~12 trades/day each. They were rejecting >99% of what these
conditions admit, on information not present in the tape.

Seven exit policies, both splits, all negative:

| policy | holdout net/ep | holdout net | in-sample net/ep | in-sample net |
| --- | --- | --- | --- | --- |
| f4_hold12s | −0.0067 | −35.2 | −0.0079 | −77.6 |
| tp17_sl28_s90 | −0.0068 | −36.2 | −0.0077 | −82.8 |
| tp30_sl25_s90 | −0.0079 | −39.7 | −0.0085 | −86.5 |
| trail_a10_t8_s90 | −0.0078 | −41.5 | −0.0082 | −87.0 |
| trail_a15_t6_s90 | −0.0079 | −41.5 | −0.0086 | −90.6 |
| trail_a15_t10_s90 | −0.0077 | −38.9 | −0.0086 | −87.4 |
| trail_a20_t8_s60 | −0.0075 | −38.6 | −0.0086 | −89.2 |

**All 12 days negative individually.** It fails *in-sample* — on the exact window F4 was
discovered — so this is not a regime or overfit story.

Two controls pin down why:

| variant | split | n | win% | mean ret% | net/ep |
| --- | --- | --- | --- | --- | --- |
| gate, entry **+0s** | holdout | 5713 | 38.6 | −0.40 | **−0.0049** |
| gate, entry +0s | in-samp | 10971 | 36.7 | −1.85 | −0.0063 |
| gate, entry +2s | holdout | 5355 | 37.4 | −2.36 | −0.0068 |
| **control**: random, liquidity-matched, +2s | holdout | 5886 | 35.1 | −3.28 | −0.0077 |
| control, +2s | in-samp | 11313 | 31.8 | −5.00 | −0.0094 |

1. **Negative even at zero latency.** Latency is not what killed it — the entry conditions
   carry no tradeable signal. No amount of speed engineering rescues this.
2. **The gate does beat random, but trivially.** +0.92pp mean return and +2.3pp win rate
   over a liquidity-matched control. Real signal, but you need ~+8pp to clear the base
   rate plus costs. **The gate delivers ~11% of the edge required.**
3. **The base rate is the real enemy.** A random entry on a young token held ~12s returns
   **−3.3% (holdout) / −5.0% (in-sample) gross, before any cost.** The token population is
   strongly negative-drift at this horizon; costs are added on top of that hole.

### Verdict

F4's measured performance is **not reproducible from observable entry conditions**. Their
edge lives in something the tape does not contain — coordination with the launch crew, an
off-chain feed, or a far more selective filter. Nothing in this document is armable, and
the wallet-copying direction as a whole should be considered closed: §4 showed the edge is
latency, and §9 shows that when it is not latency it is unobservable.

**Do not arm F4. Do not spend more on wallet copying.** The one durable, reusable asset
from this work is the cost model in §1 and the base-rate measurement above — any future
entry rule must clear a **−3.3% to −5.0% population drift plus a 4.65% cost floor**, and
should be tested against the liquidity-matched random control before anything else.
