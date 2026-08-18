# Wallet analysis — what four cracked scalper wallets actually do

The calibration source for the flow-scalper rule ladder. Four external wallets were
reverse-engineered from the local pump.fun curve firehose (PG `trades`, wallet-attributed);
this file holds the **surviving conclusions** — the mechanics, the numbers a rule is
calibrated from, and the searches that are closed.

The run-by-run investigation (rejected hypotheses, overturned verdicts, intermediate gate
readings, the primary per-episode data) is
[`@history/wallet-research-2026-07.md`](../../history/wallet-research-2026-07.md). Read it
only if you need the raw measurements — the scratch tables it was computed on are gone.

Companions: [execution-costs.md](execution-costs.md) (what a round trip costs — read
before trusting any PnL number here), [flow-scalper-findings.md](flow-scalper-findings.md)
(the exit-condition traps this work surfaced), [armed-trailing-stop.md](armed-trailing-stop.md)
(the `arm_above_pct` fix it produced).

## The family, and the one number that decides everything

Three of the four wallets are the same strategy: **dip-reversion scalping**. They buy a
sharp dip inside an otherwise-hot mid/late-curve token and exit fast. Structurally all of
them are **1 buy → 1 full sell** — never laddering. What looks like laddering on a token
page is *rapid re-entry* on the same mint.

**The pump.fun fee is 125 bps/leg = 2.53% per round trip.** Derived from our own data:
first-buy amounts cluster on `0.98765432 × round SOL`, and `0.98765432 = 10000/10125`
exactly, matching the IDL's `net_sol = spendable × 10_000 / (10_000 + total_fee_bps)`.
`amount_lamports` is the **net curve-side** SOL and excludes this fee, so a raw
`sell − buy` sum overstates PnL on both legs. Every verdict below is net of it.

That number is the whole game: a gross edge under ~2.5%/round-trip is not an edge.

## The four wallets

| | `omego` | `64hP` | `63ot` | `trunoest` |
| --- | --- | --- | --- | --- |
| family | dip-reversion | dip-reversion | dip-reversion | **momentum ignition** |
| verdict | **REFUTED** — no net edge | +2.54%/SOL cycled | **+2.3%/turnover** | ~break-even on landed |
| closed eps (window) | 2,974 (5 d) | 6,515 (4.2 d) | 1,088 (5.8 d) | 225 mints (8.7 d) |
| win rate | 59.1% | 56.5% gross / 49.6% net | **65.1%** | 63.6% |
| median episode | ~0 | +2.39% | **+11.0%** | +4.6% |
| sizing | 1.18% of vsol | 1.86% of vsol, cap 1.5 SOL | **fixed ~0.5 SOL** | 4% of vsol, tiered |
| entry age (med) | 5.3 min | 0.8 min | 1.9 min | 69 s |
| vsol at entry (med) | 73.5 | 44.5 | 69.8 | 60 |
| dip vs 30 s high | −12.6% | −22.7% | −20.8% | −19.1% |
| exit | ~3% trail | −6.8% trail | **TP +17% / SL −28%** | ~30% off-peak reversal |
| hold (med) | 22.5 s | 21.3 s | **10.6 s** | 30 s (win) |
| unclosed bags | — | 3.3% / −225 SOL | **0.9% / −3.1 SOL** | 8% / −60 SOL |
| concurrency | med 3, max 8 | med 2, max 10 | **usually 1, max 3** | **1** |
| selectivity | 0.66% of mints | 3.8% | 0.57% | **0.25%** |

### `63ot` — the one to build from

Lowest capital, highest win rate, simplest to express, and the only one whose exit needs
no trailing subtleties at all. The whole book runs on **1–2 SOL** turning ~100 SOL/day.

- **Entry:** deep dip on a very hot, deep-curve token. Age med 1.9 min, vsol med 69.8,
  price −20.8% vs the 30 s high, −27.5% off prior ATH, market heat med **224 trades /
  119 SOL gross in the prior 60 s**. Not a bottom-tick buyer (med +29% above the 30 s low).
- **He buys INTO the knife.** 56% of entries land immediately after a market *sell*, and
  prior-2 s net flow is negative at median (−2.4 SOL). **A `m_flow_window(2).net >= 0`
  bounce gate fights this entry** — do not add one.
- **Exit is a fixed bracket, not a trail.** Winners: gross move med **+16.9%**, constant
  across dip-depth buckets ⇒ **fixed TP ≈ +17%**. Losers: med **−27.5%** ⇒ **hard SL
  ≈ −28…−30%**. He exits at-touch and the price keeps running (+17.6% further median in
  the next 60 s) — he deliberately leaves the right tail.
- **Sizing:** fixed ~0.50 SOL (~900 of 1,125 buys in 0.48–0.54), next to the measured
  cost-optimal 0.27–0.5 for vsol ~70. Tip drag at 0.001/leg is ~0.4%/round-trip against a
  +2.3% margin — thin but positive.

**Engine fit — everything needed already exists.** Entry: `m_price_window(30).trail`,
`liquidity` band, `m_flow_window(60).gross`, `time`. Exit: plain `take_profit` /
`stop_loss` sugar (desugars to `m_position.pnl`) — no `arm_above_pct`, no trail, no stall
dependency. The 0.9% bag rate makes a dead-flow bailout optional (still cheap insurance:
`m_flow_window(30).gross <= 3`).

Two caveats before believing any transferred number:

- **Latency.** His median winner resolves in 7.6 s and both exits fill at-touch. Live
  TP/SL evaluation is feed-driven, so expect a haircut — validate via simulate with
  `pumpfun_impact` + `worst` fill.
- **Gate recall.** A first-guess gate (liq 55–85, trail30 ≥ 15, gross60 ≥ 70, age ≥ 0.5 min)
  recalls only **27%** of his entries jointly (60–77% each) — the bands interact and need a
  sweep to place. In-gate episodes do outperform (69.7% win / +2.83% vs 63.4% / +2.03%).

### `64hP` — bigger, same family, and the bag problem

Same mechanics with every knob set differently; the first wallet whose economics clearly
clear the fee (**+2.54% per SOL cycled**, 56.5% win). His own price impact is a constant
+3.82% entry / −3.7% exit (a consequence of exact-fraction sizing), so **raw entry/exit
prices are biased** — every figure here uses the pre-trade market price.

**Exit is ONE rule, not three.** Bucketing exit-retrace by MFE gives a flat −4.9%…−7.4%
band for every bucket with MFE ≥ 5%. Episodes that never rose exit at median −7.16% vs
entry — the same trailing stop with `peak` initialised to the entry price. No take-profit,
no separate stop-loss. The −33% p10 tail is gap risk, not a wider stop.

**Re-entries improve monotonically with index** (ep1 52.0% win / +5.18% avg → ep9+ 65.0% /
+7.73%) — replicated on omego. **Do not cap `max_episodes` low.**

**The bags are the open question.** 227 episodes (3.3%) never closed, −225 SOL. The rate
is constant across days including gap-free ones, so not an ingest artifact, and only 2/227
mints ever traded on AMM, so not migrations. They are also not rugs — marked at a fixed
horizon after entry the median is +0.2% (15 s) / −1.9% (60 s) / −5.8% (300 s). The trade
stream simply stopped while price was near his entry and he never sold. A bail-out rule at
any of those horizons turns the cohort net-positive.

### `omego` — REFUTED, search closed

The original subject. Gross edge is **+1.81%/turnover, which does not clear the 2.53%
round-trip fee.** His real profit is an unclosed runner tranche the engine cannot express
(no partial exits at the time). Mechanics are still useful as family calibration — dip
−12.5% below the 30 s high, ~1.1–1.2% of vsol sizing, ~1–1.5% trailing stop off the
post-entry peak, med 17.3 s hold — but **do not build a rule from his numbers**.

Two findings from his data that generalise:

- **Winner and loser holds are identical** ⇒ the exit is price-action-driven, not time- or
  PnL-schedule-driven. There is no "no-trades-for-N-sec" exit; exits happen in *dense*
  flow (med 0.1 s since the last market trade) because the exit needs liquidity.
- **Not copy-triggered.** Only 36% of entries have a ≥0.5 SOL market buy in the prior 1 s.
  Reaction is 0.112 s median, slot delta 0 — a custom low-latency bot, not a follower.

### `trunoest` — a different family: momentum ignition

**Does not scalp someone else's flow — it manufactures the flow it exits into.** Included
because the distinction matters: copying its entry/exit without the ignition mechanic
samples a different, weaker distribution.

The loop, one token at a time: pick a very young, violently-moving token (age med 69 s,
vsol med 60, prior-60 s 105 trades / 70 SOL gross, entry −19.1% off the 30 s high but
+37% above the 30 s low — a ~70% 30 s range, with 30 s net flow strongly *positive*) →
**ignite** with ONE oversized buy (med 3.99% of vsol, ~+8% own impact; market net flow
flips from −0.63 to **+5.72 SOL** in the 5 s after) → **paint the tape** with 0.0097 SOL
micro-buys while holding, keeping the token on screeners → **dump on confirmed reversal**,
one full-balance sell at −29.5% off the episode peak, after a median +50% run.

- **Size hurts.** Tier PnL: 1.95 SOL is the sweet spot (76% win, +10.3 med); 2.93 drops to
  52%; 4.88 is flat. More impact = more ignition but a worse exit.
- **Loss containment is the weak spot — do not copy it.** 21 bags / 60 SOL sunk, marked
  med −48.8%. A −35…−40% catastrophe SL under the wide trail would have kept most of the
  closed-episode profile intact.
- Infrastructure: Axiom Trade router + **durable nonces**, mass-rebroadcast racing. Runs
  only 08:00–22:00 UTC — a human-scheduled operation, not a 24/7 daemon.

## Creation shape: no signal for SELECTION, real signal for OUTCOME

Two different questions, two different answers. Getting these backwards is the trap this
section exists to prevent.

**Which token a scalper picks** — creation shape carries **no** signal. Fingerprint-axis
testing gave χ²/df ≈ 1.0 on every axis. Hence the design rule: **use a maximally-broad
fingerprint for selection.**

**How an episode ends on a token already picked** — one axis does carry signal.
`initial_buy_lamports` (the dev buy) is the usable one:

| axis | groups | χ²/df |
| --- | --- | --- |
| `buy_ix_type` | 5 | 2.59 |
| **`init_buy`** (1 SOL buckets) | 19 | **2.03** |
| first-slot buy | 35 | 1.85 |
| `spendable_in` | 14 | 1.78 |
| `ix_labels` | 29 | 1.38 |
| `cu_limit` / `cu_price` | 12 / 29 | 0.82 / 0.82 |
| `is_cashback_enabled` | 2 | 0.44 |

(`buy_ix_type` is a 5-value proxy for the same thing; `cu_*` are flat.)

At the 12.8 SOL cut, over `64hP`'s closed episodes:

| cohort | eps | mints | win | net %/ep |
| --- | --- | --- | --- | --- |
| dev buy < 12.8 | 6,218 | 2,400 | 49.1% | +2.44 |
| **dev buy 12.8–25.6** | **284** | **80** | **59.2%** | **+7.11** |
| dev buy ≥ 25.6 | 13 | 9 | 76.9% | +8.51 |

**Why it survived scrutiny where bucket-derived gates did not:** it is **monotone in both
the fit and the holdout window**, i.e. a threshold *family* improving everywhere — not a
best-of-N single bucket (which is how `range`, rise-at-low and `rise <= 1` all died).
Mint-level block permutation (2,000 shuffles, preserving within-mint clustering) gives
p = 0.006 on win rate and p = 0.037 on net. It is not a liquidity proxy — the lift survives
conditioning on entry-vsol band (61.9% vs 50.7% within 40–55; 62.9% vs 52.1% within 55–75)
— but it **inverts above vsol 75**, hence the 40–75 band in the rule.

**`init_buy` is also the tractability lever.** A broad fingerprint arms ~18,000 tokens/day,
too many to simulate or trade. `init_buy ∈ [12.8, 25.6)` arms ~110/day, and a 6-day
simulate folds in **60 s** instead of ~20 min. It is an **instant** axis
(`has_instant_criterion`), so it matches synchronously on `TokenCreated` with no
`PendingFirstSlot` deferral.

**Note `initial_buy_lamports` is the NET curve amount**, so dev-buy clusters sit at
`gross × 0.98765` (12.0 → 11.8519, 15.0 → 14.8148). Cuts must be placed in net terms.

## Rules that came out of this, and what is closed

Seeds live under [`hunter/scripts/`](../../../scripts/); all seeded rules are
`trade_mode='paper', is_active=false` by default — arm deliberately.

| Rule family | From | Status |
| --- | --- | --- |
| `fs4-*` | `63ot` fixed bracket (buy 0.5, TP 17, SL 28, liq 55–85, trail30 ≥ 15, gross60 ≥ 70) | seeded paper |
| `fs3-*` | `64hP` + the dev-buy ≥ 12.8 gate | seeded paper |
| `fs2-*` | `64hP` knob ladder | **broad-universe control only** — see below |
| `tru-0*` | `trunoest` (his size / impact-optimal 0.30 SOL) | seeded paper |
| `fs-*` | omego-calibrated | **retired** — omego is refuted |

**`fs2-*` is a control, not a candidate.** The ladder
([`seed-flow-scalper-64hp-rules.sql`](../../../scripts/seed-flow-scalper-64hp-rules.sql))
arms ~18,000 tokens/day against `fs3-*`'s ~110, and `fs3-*` is the first configuration that
stays PnL-positive under the adversarial `worst` fill — so `fs2` is worth arming only to
show what the same geometry does on a universe with no selection in it. Its knob
conclusions survive that demotion, but two are **revised** by the `fs3` runs and the seed
file is not: the dip gate is best at **25** (not 18) and the vsol band at **40–75** (not
36–70). Use the revised pair anywhere `fs2` is quoted.

**Closed searches — do not re-run these:**

- **omego as a template.** Gross edge does not clear the fee.
- **Creation shape as a token *selector*.** χ²/df ≈ 1.0 on every axis, repeatedly.
- **Bucket-derived single-bucket gates** (`range`, rise-at-low, `rise <= 1`). Best-of-N
  artifacts; none replicated out of sample.
- **A `flow(2).net >= 0` bounce gate on the dip-reversion family.** It contradicts the
  measured entry — these wallets buy into negative 2 s flow by design.
- **Cloning `8dtx2tr4` (`wallet_id` 2720) from creation-time or tape features.** Its edge
  is token *selection*, not the trigger: on its own picks the reconstructed rule prints
  +7.7 % mean (PF 1.53) and on everything else −32.8 %, from one simulate run at identical
  fills and costs. Every feature we ingest was scored against that +7.8 % ceiling and none
  is worth more than a few points. Full grid:
  [`../../history/2026-08-17-wallet-8dtx-clone-refuted.md`](../../history/2026-08-17-wallet-8dtx-clone-refuted.md).

**Two rules that generalise past that refutation.**

**Token death is the dominant cost on an unselected universe, and
`m_flow_lifetime.gross_flow` is the lever that controls it.** A dip-turn rule with no
liveness floor exits `Dead` on ~22 % of its fills; a `gross_flow >= 30` floor (with
`time <= 300`, `trail <= 30`) cuts that to 4.6 % and moves mean PnL −26.3 → −9.3 %.
Ablation isolates the floor as the whole effect. Put it on any rule that arms a broad
universe. Beware the shape of the tuning curve, though: past that point each further
tightening buys its gain by removing trades and converges on zero **from below** — a
config walking toward breakeven on a shrinking `n` has no edge, it has less exposure.

**Read a copy-target's exit as an *armed* trail before authoring it.** Measure the
in-hold peak gain and the retrace off that peak separately: `8dtx`'s median hold peaks
+11.6 % then exits −18.4 % off the peak, which an unarmed `m_position.retrace >= 12`
mis-renders as a −12 % hard stop from entry (the peak seeds at the entry fill). Getting
this wrong inverts the exit's effect — a trail amplifies selection quality in both
directions, improving good picks and worsening bad ones.

**Still open:** `64hP`'s 3.3% bag cohort (a timed bail-out looks profitable at every
horizon tested, but was never implemented or measured live), and whether `63ot`'s at-touch
fills survive feed-driven TP/SL evaluation at real latency.
