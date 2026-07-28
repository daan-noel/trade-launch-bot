# Flow scalper — build plan (2026-07-28)

The execution plan that turns the omego analysis into a shipped rule. Companions:

| Doc | Role |
| --- | --- |
| [flow-reversion-scalper.md](flow-reversion-scalper.md) | the analysis + measurements (WHY / target numbers) |
| [flow-scalper-fingerprint-rules.md](flow-scalper-fingerprint-rules.md) | the 6-fingerprint A/B rule spec + recalibrated knobs |
| [grouped-sweep-reentry.md](grouped-sweep-reentry.md) | sweep item C (re-entry) — a dependency this plan decides whether to fund |

**Standing decision (user, 2026-07-28): sizing is a fixed `buy_amount_lamports`.**
Percent-of-vsol sizing is deferred until measured to be worth it. See *Deferred* — the
measurement says it costs us almost nothing here, so this is a cheap deferral, not a
compromise.

---

## 1. What the engine can express today

Verified against source on 2026-07-28, not against roadmap claims. **Everything the
strategy needs for a first honest backtest already ships.**

| Strategy element | Expressed as | Status |
| --- | --- | --- |
| dip trigger | `entry.m_price_window(30).trail >= 12` | ships |
| liquidity band | `entry.m_snapshot.liquidity >= 55, <= 100` | ships |
| age floor | `entry.m_snapshot.time >= 150` | ships |
| hot gate | `entry.m_flow_window(60).gross_flow >= 11` | ships |
| reversal gate | `entry.m_flow_window(2).net_flow >= 0` (multi-window, same group) | ships |
| trailing exit | `exit.m_position.retrace >= 3` | ships |
| cooling exit | `exit.m_price_lifetime.stall >= 15` | ships |
| hard stop | `stop_loss` (top-level sugar, desugars into `m_position.pnl`) | ships |
| re-entry | `reentry {cooldown_sec, max_episodes_per_token}` | ships **in the engine** |
| concurrency ration | `max_concurrent_tokens` | ships |
| fixed size | `buy_amount_lamports` | ships |

Full metric inventory: `m_snapshot{time,liquidity}`, `m_price_lifetime{stall,trail,rise}`,
`m_price_window(w){trail,rise}`, `m_flow_lifetime{...}`, `m_flow_window(w){gross_flow,
net_flow,buy,sell}`, `m_flow_split[_window]{vol_*,nonvol_*,vol_share}`,
`m_position{retrace,bounce,pnl,held}` (exit-only).

### Four structural facts that shape everything below

1. **Simulate has re-entry; the grouped sweep does not.** `lab/src/strategies/replay.rs`
   folds through `hunter_engine::reduce`, so it inherits `ArmState::Cooldown` and the
   per-token episode counter for free. The sweep is the sanctioned parallel
   re-implementation and re-entry is [item C, not started](grouped-sweep-reentry.md) —
   **a swept re-entry combo is silently mis-scored at one episode per token.**
   Consequence: **all re-entry calibration goes through simulate, one rule at a time.**
   No grid search until item C is funded.
2. **Simulate takes a full draft over HTTP.** `POST /api/strategies/simulate` accepts
   `{draft: {fingerprint_id, params, buy_amount_sol, max_concurrent_tokens,
   max_total_tokens, trade_mode}, since, until, fill_model, cost_model}`. No DB rule
   row, no migration, no UI. A backtest ladder is a script.
3. **The lake already covers the analysed window** — `hunter/lake-data/trades/dt=2026-07-22`
   through `dt=2026-07-27`, the exact 5 days the re-derivation used. No export step.
4. **Caps are honoured and global.** Replay merges every token into ONE time-ordered
   stream against one `EngineState`, so `max_concurrent_tokens` rations entries across
   tokens exactly as live does. This matters more than usual here — see §2.

---

## 2. What the goal actually is

The [gate-payoff measurement](flow-reversion-scalper.md) put a ceiling on selection:
conditioned on the liquidity band and a 12% dip, the best available gate reaches
**~5.0% precision** against a 4.38% base — a 1.15x lift. Combined with the earlier
finding that entered vs skipped moments *inside* the gated pool are statistically
identical, **omego's token pick is not reproducible from window features.**

So the design goal is not "find his tokens". It is:

> Trade a **superset** of his universe with his **mechanics**, and let
> `max_concurrent_tokens` ration the entries.

That reframes where the effort belongs. The binding parameters are the mechanics —
dip depth, trail width, cooldown, episode cap, concurrency — not the selection gates.
Phases A and B spend everything there. No new metric is built until a measurement says
the mechanics are already right and selection is what is left.

### The fee threshold — the hypothesis that drives phase A

omego's gross edge is roughly `+108.4 SOL / 2,974 episodes / ~0.84 SOL avg size` ~=
**+4.3% per episode gross**, but the per-episode-index medians are far thinner: ep1
**+0.52%**, ep9+ **+2.76%**. pump.fun charges ~1%/leg, so a round trip costs ~2% before
any slippage.

That predicts something specific and testable: **episode 1 is net-negative and only
later episodes clear the fee.** If true it explains why every one-shot backtest so far
looked marginal — a one-shot backtest samples nothing but the losing episode. It also
means the trail must be wide enough that the median winner clears 2%, which is an
argument against tightening it.

---

## 2a. READ THIS BEFORE THE RESULTS TABLES — the noise floor

Per-episode PnL has a standard deviation of **9-15%**. At the sample sizes these
ladders produce that puts the standard error at **±0.27 to ±0.66 pp**, which is
*larger than most of the differences the tables below rank*. Ranking by mean alone
over-reads the data; always check `n` and the spread.

| config | n | mean/ep | SE | t |
| --- | --- | --- | --- | --- |
| X3 — stall 15, re-entry (≈ the shipped rule) | 445 | −1.884% | 0.435 | **−4.33** |
| L7 — arm 3 / trail 1.5, re-entry | 1854 | −0.669% | 0.269 | −2.49 |
| L9 — arm 5 / trail 3, re-entry | 1654 | −0.859% | 0.343 | −2.50 |
| R3 — stall 120, re-entry | 1190 | −0.887% | 0.431 | −2.06 |
| L10 — one-shot | 451 | −0.084% | 0.659 | −0.13 |
| V0 — one-shot, trail 2 | 451 | **+0.133%** | 0.654 | **+0.20** |

**What actually clears the bar:**

- **The current rule loses money** — t = −4.33. Solid.
- **The aggregate move from −1.88% (t = −4.33) to ≈ 0 (t ≈ 0)** is a real regime
  change, and it is corroborated by *mechanism* evidence (max hold 16 s, the
  exit-reason mix) rather than resting on PnL alone.

**What does NOT clear the bar — do not quote these as results:**

- **V0 is not "profitable."** t = +0.20, 95% CI [−1.15, +1.42]. Indistinguishable
  from zero, and indistinguishable from L10.
- **Trail 2 vs trail 3** (V0 vs L10): t ≈ 0.2. Not a result.
- **One-shot vs re-entry** (L10 vs L9): Δ 0.78 pp, joint SE 0.74 ⇒ **t ≈ 1.0**.
  Suggestive only, despite how large the mean gap looks.
- **The `stall` fix** (X3 vs R3): Δ 1.0 pp, joint SE 0.61 ⇒ t ≈ 1.6. The strongest
  secondary claim, still short of significance on PnL alone — it is the mechanism
  evidence that carries it.
- Every within-ladder ordering (trail monotonicity, arm optimum, stop width, leash
  length) sits inside ±1 SE. Treat as hypotheses.

These are not independent samples — same market, different rules — so a **paired or
bootstrapped** comparison over matched tokens would be far tighter than these naive
independent-sample t-stats, and is the right way to settle the secondary claims.
Until that is done, the naive numbers are the honest prior.

## 2b. RESULTS 2026-07-28 — the dominant defect is `stall`, not selection

First real engine numbers (3-day window 07-25..07-28, broad control fingerprint,
`first` fill + `pumpfun_fee_only`, 1.0 SOL fixed, concurrency 4). Full rows in
`docs/roadmap/data/flow-scalper-ladder.csv`.

| run | exit | episodes | win | PnL |
| --- | --- | --- | --- | --- |
| X0 | unarmed r3, sl 25 (today's authorable rule) | 496 | 29.6% | **−8.96 SOL** |
| X1 | unarmed r5, sl 12 | 480 | 31.3% | −8.41 SOL |
| X2 | armed g0, r3, sl 12 | 456 | 36.2% | −8.07 SOL |
| X3 | armed g2, r4, sl 12 | 445 | 38.0% | −8.38 SOL |
| X4 | armed g5, r5, sl 12 | 441 | 37.9% | −8.39 SOL |
| X5 | armed g2, r4, **one-shot** | 283 | 39.9% | −5.30 SOL |
| X6 | armed g2, r4, **`worst` fill** | 442 | 30.3% | **−19.71 SOL** |
| X7 | armed g2, r4, **no 2 s `net_flow` gate** | 727 | 42.9% | −17.15 SOL |

Arming lifts win rate 29.6% → 38% and does nothing for PnL. Exit knobs are saturated.
Re-entry is exactly **PnL-neutral per episode** (−1.87% one-shot vs −1.88% with
re-entry) — it repeats a trade that has no edge, which is not evidence against
re-entry, only against re-entry *under this exit*.

Two more results worth keeping:

- **Fill sensitivity is severe.** `worst` costs an extra **−2.56%/episode** over
  `first` (−4.46 vs −1.88). Live paper books `worst`, so any candidate has to clear
  that bar, not just `first`.
- **The 2 s `net_flow >= 0` gate earns its place.** Dropping it admits 63% more
  episodes (445 → 727) at −2.36%/ep vs −1.88%. Worth stating because the clause looks
  vacuous — `net_flow` is buy − sell, so an empty window reads `0 >= 0` = true — and
  a check confirmed it is *not* firing on silence: every entry had trades in its 2 s
  window and the gap to the previous trade is 0.00 s at p75. Keep the gate.

### What actually kills it

X3's exit-reason mix is the whole story:

| exit reason | episodes | mean PnL | total | mean hold |
| --- | --- | --- | --- | --- |
| `stall >= 15` | **323 (73%)** | −0.63% | −205 | 4.7 s |
| `StopLoss` | 55 (12%) | **−18.39%** | **−1012** | 4.5 s |
| `retrace >= 4` (the armed trail) | 67 (15%) | **+5.64%** | +378 | 5.0 s |

**Across all 445 episodes no position ever held longer than 16 seconds** — max hold is
14-16 s under *every* exit reason.

`m_price_lifetime.stall` is **seconds since the last all-time high**
([`price_lifetime.rs`](../../engine/src/metrics/price_lifetime.rs) is authoritative;
the `mod.rs` one-liner "seconds since the price last moved" is misleading). It resets
only on a *new* high. This rule enters on a `>= 12%` dip — by construction the price
is below its recent high — so a new ATH during the hold is rare. Entry additionally
requires `stall < 15`, because `can_enter` refuses to buy while an exit metric already
holds, and the median entry is already **8.3 s** past the ATH. That leaves ~7 s of
headroom, and the clock runs out on essentially every trade.

So `stall >= 15` is not the "cut losers early" gate the rule doc claims. It is:
- a **hard ~15 s ceiling on position lifetime**, against a reference trader whose
  median hold is 22.5 s and who makes half his money in holds over a minute; and
- a hidden **entry filter** — only tokens that peaked within the last 15 s.

The armed trail returns **+5.64%** in the ≤5 s it is permitted to work. Give it room
and it is the strategy.

The rule doc's evidence for keeping `stall` ("dropping it: win% 44 → 32") was measured
against the **unarmed** exit, where `retrace` was itself a −3% stop from entry. That
evidence does not transfer.

### What this retires

- **C2 (selection) is downstream, not the cause.** The cohort split is real — on
  mints omego traded, 110 episodes at −0.33%/ep and 44.5% win (≈ break-even net,
  ≈ +1.7% gross); on mints he never touched, 335 episodes at −2.39%/ep carrying 96% of
  the loss. But no entry selector can help while every position is force-closed in 5
  seconds. Re-measure C2 only after the exit can hold.
- **The supervised decile sweep found nothing** (§C3): at-entry unique wallets,
  liquidity, dip depth, age and buy-share all show every quintile negative with no
  monotone trend. Consistent with a defect that is uniform across entries.
- **`unique_wallets` (D2) is on hold and looks unlikely.** Even at its measured best it
  is a 1.15× precision lift, which cannot close a gap of this size.
- Encouraging: at the **mint** level 110 of the 283 mints we traded (39%) are his, far
  above the 4.4% moment-level precision. The entry gate is finding roughly the right
  tokens.

## 2c. The `stallfix` ladder — confirmed, and two more findings

Same window / fill / cost. **Rank on PnL per episode, not total** — the configs differ
in throughput by 10x, so total PnL mostly measures how often a still-negative trade
was repeated.

| run | config | eps | win | PnL/ep |
| --- | --- | --- | --- | --- |
| R3 | armed g2/r4, **stall 120** | 1190 | 54.9% | **−0.89%** |
| R4 | armed g2/r4, **`held >= 120`, no stall** | 1570 | 55.5% | **−0.92%** |
| R2 | armed g2/r4, stall 60 | 987 | 53.7% | −0.99% |
| R7 | **unarmed** r4, stall off | 775 | 35.2% | −1.12% |
| R6 | armed g2/r4, stall off, **one-shot** | 184 | 53.8% | −1.19% |
| R5 | armed g0/r3, stall off | 348 | 47.1% | −1.48% |
| X3 | armed g2/r4, stall 15 | 445 | 38.0% | −1.88% |
| R1 | armed g2/r4, stall off | 163 | 55.2% | −1.90% |
| R8 | armed g2/r4, stall off, **`worst` fill** | 160 | 51.9% | **−6.60%** |

Removing the hold cap roughly **halves** the per-trade loss (−1.88% → −0.89%) and
lifts win rate 38% → 55%. Gross goes from ≈0 to ≈+1.1% against the ~2 pp fee.

**Arming and bounding the hold are complementary; neither alone suffices.** R7
(unarmed + no cap) is −1.12% and R1 (armed + no bound) is −1.90%; only the two
together reach −0.89%. That is the honest accounting of the `arm_above_pct` build.

**`m_position.held` is the right primitive, not `stall`.** R4 matches R3's per-trade
result with 32% more throughput, because `held` bounds the hold *without* also
filtering entries. Prefer `held >= N`; keep `stall` for what it actually means.

### Two findings the ladder surfaced

**1. Re-entry is now harmful, via the concurrency cap.** R6 (one-shot) beats R1
(re-entry) on both axes — −1.19% over 184 episodes vs −1.90% over 163. With holds no
longer capped at ~5 s, re-entering a token occupies one of only 4 slots and crowds out
a fresh opportunity. This is an *interaction*, not a verdict on re-entry: concurrency
is now a first-class knob (`lock` ladder L9/L10 test conc 12).

**2. The exit geometry gives back more than it locks in.** `arm_above_pct: 2` with
`retrace >= 4` can exit **below** the arm point: arm at +2, trail 4% off a peak of
+2 ⇒ `1.02 × 0.96 = −2.1%`. Every armed run so far violated this (X2/R5 arm at 0 are
worst). For the trail to protect a gain you need **`arm_above_pct > retrace`**; the
worst armed exit is then `(1 + arm/100) × (1 − retrace/100)` — arm 5 / trail 3 floors
it at +1.85%. This is the `lock` ladder and needs no new code.

### The fill problem is unresolved and may be fatal

`worst` costs **−4.7%/episode** over `first` (R8 −6.60% vs R1 −1.90%); the earlier
armed-ladder pair showed −2.56%. Live paper books `worst`. Nothing measured so far
survives it. Before committing real money this needs its own answer — either evidence
that `first` is the realistic model for a feed-reactive bot, or an entry that is not
buying into an actively falling print.

## 2d. The `lock` ladder — re-entry is the second big defect

Base for all rows: no `stall`, `m_position.held >= 120`, `first` fill, fee-only.

| run | arm / trail / stop | extra | eps | win | PnL/ep |
| --- | --- | --- | --- | --- | --- |
| **L10** | 5 / 3 / 8 | conc 12, **one-shot** | 451 | 52.1% | **−0.084%** |
| L7 | 3 / 1.5 / 6 | | 1854 | 51.7% | −0.669% |
| L3 | 6 / 2 / 8 | | 1661 | 51.6% | −0.732% |
| L8 | 5 / 3 / 8 | held 300 | 1652 | 51.3% | −0.764% |
| L5 | 5 / 3 / 12 | | 1511 | 57.4% | −0.841% |
| L1 / L9 | 5 / 3 / 8 | (L9 = conc 12) | 1654 | 51.0% | −0.859% |
| L4 | 5 / 3 / 5 | | 1802 | 43.3% | −1.069% |
| L6 | 10 / 4 / 8 | | 1552 | 41.8% | −1.287% |
| L2 | 8 / 4 / 8 | | 1583 | 44.3% | −1.296% |

**Re-entry is the second big defect.** L10 vs L9 differ by exactly one field and score
**−0.084% vs −0.859%**/ep. The ~1,200 re-entry episodes average ≈ −1.15%; first
entries average ≈ −0.08%, i.e. break-even.

This is the *opposite* of omego, whose re-entries improve with index (+0.52% at ep1 →
+2.76% at ep9+). The reconciliation: **re-entry amplifies selection quality.** He
re-enters tokens he picked well; we re-enter whatever we entered, compounding the
error when the pick was bad. Re-entry is a multiplier on an entry edge, not a lever in
its own right — keep it **off** until one exists.

Secondary reads:

- **Tighter trail is monotonically better** at fixed everything-else: 1.5 → −0.67%,
  2 → −0.73%, 3 → −0.84/−0.86/−1.07, 4 → −1.29/−1.30. Take the small gain fast; the
  reversion is brief.
- **Arming too high backfires.** arm 3 −0.67 · 6 −0.73 · 5 −0.85 · 8 −1.30 · 10 −1.29:
  past ~6 too few trades ever arm and more run to the stop.
- **A tight stop is worse, not safer** (stop 5 → −1.07% vs stop 8 → −0.86%),
  matching C1: his eventual winners dip to −3.3% at p25.
- **A longer leash helps** at fixed geometry (held 300 −0.764% vs held 120 −0.859%).
- **Concurrency is not a lever here.** L9 (conc 12) is numerically identical to L1
  (conc 4) — with `held` recycling positions the cap never binds. Verified the knob
  does reach the engine by differential test (conc 1 → 409 eps / −4.34 SOL; conc 4 and
  12 → 484 / −2.54, identical). The earlier R1-vs-R6 cap-binding reading still holds:
  those runs had **no** `held` stop, so positions did pin the 4 slots.

## 2e. C4 — the remaining gap is entry TIMING, and only that

Our exact exit policy applied to **omego's own entry moments**, with `held >= 120`
forcing every episode to resolve under our rules (no fallback to his exits, which is
what confounded C1b):

| arm / trail / stop | his moments (net) | our moments (net) | gap |
| --- | --- | --- | --- |
| 5 / 3 / 12 | **+1.08%** | — | |
| 2 / 4 / 12 | **+0.95%** | −0.92% (R4) | 1.87 pp |
| 5 / 3 / 8 | **+0.78%** | −0.86% (L1) | 1.64 pp |
| 6 / 2 / 8 | +0.74% | −0.73% (L3) | 1.47 pp |

**All eight policies are net-positive on his entries and net-negative on ours.** Same
exits, same fee, same window — only the entry instant differs. Every exit knob in §2d
spans 0.6 pp; entry timing is worth **~1.7 pp**. The exit work is done and saturated.

### The `rise` hypothesis (unvalidated)

Supervised bucketing of L3's 1,661 episodes by `m_price_window(30).rise` (percent
above the rolling 30 s low):

| rise30 | eps | mean PnL | win |
| --- | --- | --- | --- |
| **≤1 (at the low)** | 263 | **+1.28%** | 55.5% |
| 1-3 | 257 | −0.58% | 53.7% |
| 3-6 | 337 | −0.41% | 52.8% |
| 6-12 | 347 | −2.06% | 48.1% |
| >12 | 457 | −1.20% | 49.9% |

+2.0 pp over the run average — the right size for the C4 gap, and one line of rule
JSON. Two caveats kept loudly:

1. **It is not copying omego.** His own `rise30` is *higher* than ours (median 9.6 vs
   5.6). This is an edge in our own fired set that happens to sit where his does not.
2. **It is a post-hoc pick of the best of five buckets on in-sample data** — the exact
   shape of the `range` mistake withdrawn on 07-28. The same bucketing run on X3's
   episodes (broken exit) gave the **opposite** ordering, which is itself a warning.

Hence `-Plan rise` on 07-25..07-28 and then `-Plan risevalidate` on the untouched
**07-22..07-25** window before this is believed or written into a rule.

## 2f. BOTTOM LINE 2026-07-28 — break-even, not profitable

The tuned config (**V0**: entry `time>=150`, `liquidity 55-100`,
`m_price_window(30).trail>=12`, `m_flow_window(60).gross_flow>=11`,
`m_flow_window(2).net_flow>=0`; exit `m_position{retrace>=2, arm_above_pct 5,
held>=120}` + `stop_loss 8`, **no `stall`**, **one-shot**, conc 12, 1.0 SOL):

| window | fill / cost | n | mean/ep | sd | t |
| --- | --- | --- | --- | --- | --- |
| in-sample 07-25..28 | `first` + fee-only | 451 | **+0.133%** | — | +0.20 |
| **OOS 07-22..25** | `first` + fee-only | 468 | **−0.181%** | 13.06 | −0.30 |
| in-sample | `signal` + `pumpfun_default` | 451 | −0.143% | — | −0.22 |
| OOS | `worst` + fee-only | 468 | **−3.866%** | — | — |

**Pooled across both windows (919 episodes): ≈ −0.03%/ep. Flat.**

### How much of the improvement actually replicates (settled 2026-07-28, run O3)

The OOS *baseline* — today's shipped rule geometry (unarmed `retrace>=3`, `stop_loss
25`, `stall 15`) on the untouched window — measures **−0.646%/ep** (n=540, sd 6.20,
t = **−2.42**). Against it:

| | in-sample | OOS |
| --- | --- | --- |
| baseline | −1.88%/ep (t −4.33) | −0.646%/ep (t −2.42) |
| V0 | +0.133%/ep | −0.181%/ep |
| **improvement** | **≈ +2.0 pp** | **+0.465 pp**, SE 0.660, **t 0.71**, 95% CI [−0.83, +1.76] |

So: the sign replicates, **the magnitude does not.** The OOS improvement is a quarter
of the in-sample one and is not distinguishable from zero on its own. The honest
statement is that the baseline was *reliably* losing in both windows (t −4.33, −2.42)
and V0 is *not reliably* anything in either — an improvement that is real in direction
and unquantified in size. It is **not** "a 2 pp fix that replicated."

**The variance more than doubled: sd 6.20 → 13.06.** This is mechanical, not noise —
the unarmed trail and the `stall` cap were both truncating the right tail, so removing
them lets winners run. Two consequences: (a) V0 needs roughly 4× the episodes of the
baseline to establish the same t, so future ladders on this geometry need bigger
windows to say anything; (b) a break-even mean with sd 13 is a genuinely high-variance
book — position sizing matters more than the mean does, which is worth remembering
against the deferred percent-of-vsol question in §4.

### What is established

1. **Three real mechanical defects, all fixed.** The unarmed trail (needed the new
   `arm_above_pct`), `stall` capping every position at ~15 s, and re-entry. Each is
   backed by mechanism evidence, not just PnL.
2. **The exits are now genuinely good** — C4 shows every tested exit policy is
   net-**positive** on omego's own entry moments (+0.55 to +1.08%). Exit tuning is
   saturated; the ladders span 0.6 pp while entry timing is worth ~1.7 pp.
3. **`worst` is not the honest bar.** Measured against 2,553 of omego's real
   executions: his fill costs **+1.18%** vs the signal price (essentially just his own
   price impact, constant because he sizes at a fixed 1.18% of vsol). `first` would
   charge +2.50%, `worst` +3.77%. He lands 31% of the way signal→worst; only **2.0%**
   of his buys land near `worst`. A competent operator beats the `worst` model 3×.
   **Caveat: that is conditional on landing as promptly as he does — a latency
   requirement, not a modelling choice.**

### What is NOT established

- Any claim that V0 is profitable. Every realistic-fill number is within ±1 SE of
  zero. See §2a.
- **The size of the improvement.** +2.0 pp in-sample vs +0.465 pp OOS (t 0.71). The
  defects were real and their mechanisms are proven; how much fixing them is *worth*
  is not. Quote the mechanism evidence, not the pp figure.
- Re-entry-off: Δ0.78 pp in-sample and Δ0.83 pp OOS, same direction both windows.
  Pooled ≈ **0.80 ± 0.52 pp (t ≈ 1.55)** — the best-supported secondary claim, still
  short of significance.

### Refuted along the way (all measured, none shipped)

`range` (07-27), `unique_wallets` (selection ceiling ~5%, and C2's apparent selection
signal turned out to be a stall artefact), and `m_price_window.rise <= N` — which
bucketed at +1.28%/ep in-sample and measured **−0.757%/ep** live.

> **Rule of thumb earned here: post-hoc bucket selection on this dataset does not
> generalise.** Three for three. The `rise` gate did not even survive a change of
> *exit* config, let alone a new window. Always run the live ladder before believing a
> bucketing result, and never write one into a rule doc first.

### Recommendation

**Do not put this on real money.** It is flat under a realistic fill and −3.9%/ep
under the pessimistic one, so the outcome would be decided by execution quality rather
than edge.

The one lead worth pursuing is the ~1.7 pp of **entry timing** C4 isolates — and note
that four separate attempts to find it in window features have now failed, which is
itself evidence that it may not be recoverable from the data we ingest. Before more
tuning, decide whether that is worth attacking at all.

Independently of the strategy, `arm_above_pct` is a real engine capability worth
keeping, and the `stall` trap is worth documenting for every future rule.

## 2g. THE PREMISE WAS WRONG — his scalping does not beat the fee (2026-07-28)

Everything above assumed omego has a scalping edge worth copying. **He does not.**
Measured from his actual cash flow over the local window (C9–C12, 6,273 legs, 451
mints), not from price paths:

| | SOL |
| --- | --- |
| spent on buys | 2,661.17 |
| received on sells | 2,709.28 |
| **gross cash** | **+48.11** |
| protocol fee (1%/leg, 2 legs) | −53.70 |
| Jito tip + priority (0.001025/leg) | −6.43 |
| **net cash on completed round trips** | **−12.02** |

**His gross edge is +1.808% of turnover against a 2.00% round-trip fee.** That single
line explains every negative result in this document: V0 stalling at break-even, the
copy-trade decay curve being negative at *zero* latency, and four entry gates in a row
adding nothing. There was never 2 pp of edge to find — the pattern is real and worth
about 1.8%, and the house takes 2%.

Verified before believing it: `amount_lamports` is the **curve** amount, not fee-
inclusive — `|Δreserve_lamports| / amount_lamports` = **1.00000** at p25/median/p75
across 5.6M legs, so charging 1%/leg on top is correct and not a double-count.

### Where his money actually comes from: the bags he never sells

He is net positive only on **unsold residual positions** — 72 of 451 mints, marked at
**83.00 SOL**. Those marks survive scrutiny: not concentrated (top 3 = 13.2%, median
bag 1.39 SOL), and not dead (**0 of 72** have zero trades after his last leg; 78 of the
83 SOL sits in mints with >0.5 SOL of volume in the 10 min after he stopped).

| bags valued at | whole-wallet net |
| --- | --- |
| zero | +3.81 SOL |
| liquid ones only | +81.94 SOL |
| full last-spot mark | +86.81 SOL |

He sells ~81% of a position into the scalp and **lets ~19% ride**. On the winners that
residual multiplies — the bags mark ~7× their cost basis. So his edge is a *runner
tranche*, and the scalp round-trip is roughly a fee-paying wash that funds it.

### Why our engine cannot express this

`hunter-engine` has **no partial-exit concept** — no tranche, scale-out, or sell-
percentage anywhere in `arm.rs` / `reduce.rs`; every exit closes 100% of the position.
So the strategy we have been tuning is structurally incapable of the thing that makes
him money, no matter how the entry and exit knobs are set. That is not a tuning gap.

### Consequence for sizing (and the §4 deferral)

Two cost terms move in opposite directions with buy size, and the simulator prices
**neither**: impact = `buy_sol / vsol` grows with size, while tip+priority
(0.001025 SOL/leg, fixed) shrinks as a percentage. Round-trip, on the median 70 SOL
pool:

| buy size | impact | tip+priority | size-dependent total |
| --- | --- | --- | --- |
| 0.1 SOL (live rules) | 0.28% | 2.05% | **2.34%** |
| **0.27 SOL (optimum)** | 0.77% | 0.76% | **1.53%** |
| 1.0 SOL (the ladder) | 2.86% | 0.21% | **3.07%** |

Optimum is `sqrt(fixed_per_leg × vsol)` ≈ **0.27 SOL**. Both the configured 0.1 and the
tested 1.0 are on the wrong side of it. **Every ladder number in this document was run
at 1.0 SOL under `fee_only`, so all of them omit ~3 pp of real round-trip cost** — V0
included. Corrected, V0 is roughly −2.7%/ep at 1 SOL, not −0.03%.

`CostModel` has `fee_bps_per_leg`, a flat `slippage_bps`, and `fixed_cost_sol_per_leg`
— nothing that scales with size-vs-depth, and `pumpfun_fee_only` zeroes the slippage
term entirely.

**SHIPPED 2026-07-28.** `CostModel::price_impact` + `CostModelKind::PumpfunImpact`
charge `buy_amount_sol / reserve_sol` per leg from the entry pool depth, which the
sweep reads off `MetricSeries::reserve_sol` and simulate off the fill's `TradeLite`.
Depth is `Option<f64>` — `None` charges nothing rather than guessing — and the two
legacy kinds stay depth-blind so every stored run reprices identically.

**Also corrected: `FEE_BPS_PER_LEG` 100 → 125.** The real pump.fun fee is 1.25%/leg,
measured from dev-buy amounts clustering on `gross × 10000/10125` (16,544 of 56,908
dev buys land on `0.987654` vs 310 on the `0.990099` a 100 bps fee implies). Every
backtest run before this date is **0.5 pp/round-trip optimistic**; re-run anything
whose margin was inside that. Repricing omego at the true rate moves his net from
−12.02 to **−25.45 SOL**, so §2g's verdict strengthens rather than softens.

## 3. The plan

Each phase states the question, the method, and the exit criterion. Nothing is built
until a phase justifies it.

### Phase A — baseline (no code, script only)

**Question:** does the recalibrated rule work at all, does re-entry change the answer,
and does it survive an honest fill?

Anchor rule (recalibrated knobs from the 07-27 re-derivation, fixed 1.0 SOL size):

```json
{
  "stop_loss": 25,
  "entry": {
    "m_snapshot": { "time": [{"operator": ">=", "value": 150}],
                    "liquidity": [{"operator": ">=", "value": 55},
                                  {"operator": "<=", "value": 100}] },
    "m_price_window": { "window_size_sec": 30,
                        "trail": [{"operator": ">=", "value": 12}] },
    "m_flow_window": [
      { "window_size_sec": 60, "gross_flow": [{"operator": ">=", "value": 11}] },
      { "window_size_sec": 2,  "net_flow":   [{"operator": ">=", "value": 0}] }
    ]
  },
  "exit": {
    "m_position":       { "retrace": [{"operator": ">=", "value": 3}] },
    "m_price_lifetime": { "stall":   [{"operator": ">=", "value": 15}] }
  },
  "reentry": { "cooldown_sec": 5, "max_episodes_per_token": 40 }
}
```

with `buy_amount_sol: 1.0`, `max_concurrent_tokens: 4`, `max_total_tokens: 0`,
fingerprint = the broad control (`init_buy` bucket width 1000 — matches every token
with any dev buy; a fingerprint with zero configured axes matches nothing, which is why
the broad match is spelled as one very wide bucket).

Runs:

| # | Variant | Answers |
| --- | --- | --- |
| A1 | anchor, `fill=signal`, `cost=fee_only` | is there an edge at all before execution friction |
| A2 | anchor, `fill=first` | does it survive taking the next print |
| A3 | anchor, `fill=worst` | does it survive the adversarial in-slot fill live paper books |
| A4 | A2 with `reentry` removed (one-shot) | **the re-entry delta** — the headline number |
| A5 | A2 without the 2 s `net_flow >= 0` gate | he enters *at* the window low (rise60 p25 = 1.2), so requiring a started reversal may be wrong |
| A6 | overlap check: which of his 446 mints did the rule fire on | is it trading his universe or a different one |

Always pair a fill model with `cost_model = pumpfun_fee_only`. `pumpfun_default` adds
`slippage_bps` on top of a fill model and double-counts it.

**Exit criterion:** A2/A3 net PnL positive, or a clear diagnosis of which leg kills it.
If A3 is deeply negative while A1 is strongly positive, the strategy is a latency race
we cannot win, and that is a legitimate stop-here answer.

**Tooling:** `hunter/scripts/flow-scalper-ladder.ps1` — posts a draft, polls the run,
pulls `/result/summary`, appends one CSV row. Reused by every later phase.

### Phase B — mechanics ladder (no code)

**Question:** where is the edge most sensitive?

One knob at a time around the phase-A anchor, `fill=first`, `cost=fee_only`:

| Knob | Ladder | Why |
| --- | --- | --- |
| `m_position.retrace` | 2, 3, 4, 5, 7 | his measured trail drifted to ~3%; must also clear the 2% fee |
| `reentry.max_episodes_per_token` | 1, 3, 8, 20, 40 | tests the "ep1 is the worst trade" hypothesis directly |
| `reentry.cooldown_sec` | 0, 5, 15, 35 | his median gap is 34.6 s |
| `m_price_window(30).trail` | 8, 12, 16, 20 | dip depth; his p25/med/p75 = 6.1/12.6/24.5 |
| `max_concurrent_tokens` | 1, 3, 4, 8 | the rationing lever; his median 3, max 7-8 |
| `stop_loss` | 10, 15, 25, off | 25 is almost certainly too wide for a 22 s hold |
| `m_price_lifetime.stall` | 10, 15, 30, off | exit-side OR — check it is not firing early and stealing winners |

~30 runs. Then one confirmation run of the best combination (knobs interact; the ladder
is one-dimensional by design so the *sensitivity* is readable, but the winner must be
re-run jointly).

**Exit criterion:** a parameter set whose edge is stable across `first` and `worst`
fills, plus a ranked list of which knobs actually moved the number.

### Phase C — two measurements that decide what, if anything, gets built

Both are SQL against the same 5-day window. Neither needs engine changes.

**C1 — ANSWERED YES, 2026-07-28. `arm_above_pct` built.** Two thirds of his winners
(65.4%) dip more than 3% off their running peak before winning; applying an unarmed
trail to his own 2,974 episodes turns 21% of his winners into losers and cuts the
net edge from +2.75% to +0.84%/episode, and **no trail width from 2 to 20 rescues
it**. Arming the trail roughly doubles the net edge and lifts the median realised
exit from +0.14% to +2.4-2.9%. Full numbers and the design:
[../plans/strategies/armed-trailing-stop.md](../plans/strategies/armed-trailing-stop.md).
Also learned: with an unarmed trail the `stop_loss` is **dead code** (the trail always
fires first), and his winners' worst mark is only −0.81% median, so the hard stop
belongs near −8..−12%, not −25%. Original framing kept below.

**C1 (as posed) — does he hold through drawdowns?** Today's grammar ANDs entry conditions but
**ORs exit conditions across metrics**, so `retrace >= 3 AND pnl >= 2` ("trail out, but
only once the trade has cleared the fee") is **not expressible**. Whether that matters
is an empirical question: for each of his 2,974 episodes, compute the maximum retrace
from the running peak that he *held through* before eventually closing green. If he
essentially never holds through a 3% retrace, his trail is a true symmetric stop and
there is no gap. If his winners routinely dip 5-8% off peak first, then an unarmed
trailing stop is cutting exactly the trades that carry his money, and phase D1 is the
highest-value build in this document — higher than any selection metric.

**C2 — is the residual loss selection or mechanics?** Take phase B's best parameter set
and split its losing episodes into (a) tokens omego also traded and (b) tokens he never
touched. If the loss concentrates in (b), selection is binding and `unique_wallets`
earns its place. If it is spread evenly, selection is not the problem and D2 is
dropped — consistent with the ~5% precision ceiling.

### Phase D — build only what C justified

Ranked by expected value, each independently skippable:

**D1. `m_position` strict param `arm_above_pct` — SHIPPED 2026-07-28 (uncommitted).**
The trailing stop arms only once position PnL has exceeded `arm_above_pct`; below that
the hard `stop_loss` governs. A **strict param on the group**, not a new metric and not
a grammar change — absent means prior behaviour, so no migration and every stored rule
round-trips byte-identically.

Landed across engine (`metrics/mod.rs`, `metrics/position.rs`, `arm.rs`,
`rule_params.rs`), the sweep mirror (`sweep/generic/strategy.rs` — `exit_req_fires`
skip + `classify_exit_req` demotion to `General`, since arming is a conjunction the
prefix-extrema hull cannot index), and the frontend (`allows_zero` in
`registry.ts`/`validate.ts`; the row model in `ruleConditionRows.ts` now carries the
whole non-window strict bag so the editor cannot silently drop a param it has no
control for). Locked by two engine tests, one sweep parity guard
(`scan_matches_replay_armed_trailing_exit`, all fill models × gates 0/5/40) and three
FE round-trip tests. Reference:
[../plans/strategies/armed-trailing-stop.md](../plans/strategies/armed-trailing-stop.md).

One knock-on worth knowing: `StrictParamSpec` gained `allows_zero`, because every
validator (Rust and TS) enforced a blanket `> 0` on strict params and
`arm_above_pct: 0` ("arm at break-even") is a real setting. `None` = off,
`Some(0.0)` = arm at break-even — deliberately distinguishable, and **not** an
instance of the zero-as-unbound sentinel.

**Still open on D1:** no rule-editor control (author via API/SQL or the JSON view).

**D2. `unique_wallets` in `m_flow_window`** *(iff C2 says selection is binding)*
The only gate that survived measurement: at `>= 30` it keeps 72% of his entries at
4.86% precision, where the money gate tuned to comparable strictness keeps 46% at
4.24%. It strictly dominates `gross_flow` on both axes and **replaces** it — combining
the two is worse than the crowd gate alone.

Cost, honestly:
- `flow_window.rs`: widen `on_trade` to take `wallet: u64` (one call site), carry the
  hash on the existing eviction deque, add a `HashMap<u64,u32>` occurrence map
  decremented on evict so the distinct count stays exact as trades age out. A 60 s
  window holds a few hundred trades — no probabilistic estimator needed.
- Registry: one `MetricId`, one `MetricSpec` (hue inside the 278-300 flow band, which
  has room), plus the routing arm in the exhaustive `track.rs` match. Missing the arm
  falls through to `_ => f64::NAN`, so the compiler does **not** catch it — add the arm
  and a value test together.
- `Unit::Count` does not exist: 2 lines of Rust plus 2 in `frontend/src/shared/lib/
  strategy/registry.ts` (`MetricUnit` + `unitSuffix` are both exhaustive; `npm run
  build:live` catches a miss).
- Exclude the `wallet_hash == 0` unknown-wallet sentinel from the distinct set.
- **The `with_flow` trap — the real work.** `CorpusTrade.wallet` is loaded from the
  lake only when `with_flow` is set, and `engine_sim.rs::rule_needs_flow` decides that
  by checking for `MetricGroupId::{FlowSplit, FlowSplitWindow}` — a hardcoded group
  list. A `unique_wallets` metric living in `m_flow_window` would therefore replay with
  every `wallet_hash == 0` and read **1 wallet, always**, silently. There are four
  such deciders (`engine_sim.rs`, `grouped_sweep.rs::axes_json_references_flow`,
  `metric_discovery.rs`, `metric_series.rs`). Per the CLAUDE.md one-reader rule this
  must become a single registry-derived predicate — a `needs_wallet_identity` flag on
  `MetricSpec` and ONE decider in `hunter-engine` that all four call. That is the
  extensibility fix; the metric is the easy half.
- Free: `MetricSeries` is a thin wrapper over `TokenTrack` (`series.rs` — "series
  values ARE track values"), so the grouped sweep and the metric-series chart inherit
  the metric with no second implementation and no parity divergence to record.

**D3. Grouped-sweep re-entry (item C)** *(iff phase B shows re-entry dominates and the
one-dimensional ladder is too coarse to tune it)*
The largest job in this document: an entry-eligible **row bitmap** to replace the entry
cache (only episode 1 is a pure function of the entry key), a variable-episode outcome
transport, and an episode-parity guard. Full design already written in
[grouped-sweep-reentry.md](grouped-sweep-reentry.md). Do not start it before phase B
proves it is needed — and note it may come out *faster* than today's cache, since the
per-combo `can_enter` half would then be evaluated only at candidate rows.

### Phase E — paper, then real

Paper via the seed script's `fs-%` rules (already safe-by-default: `trade_mode='paper'`,
`is_active=false`), with the phase-D knobs written back into
[flow-scalper-fingerprint-rules.md](flow-scalper-fingerprint-rules.md). Note the
fingerprint drift: the top-5 IX sequences now cover only ~71% of his entries, so the
per-group floors in the seed script must be re-derived before the 6-rule A/B means
anything. Then real money at reduced size (`buy_amount_lamports = 100000000`, 0.1 SOL).

---

## 4. Deferred

**Percent-of-vsol sizing.** He sizes at exactly 1.18% of vsol (p25 = median = p75 — a
hard-coded fraction). But the entry gate already confines vsol to 55-100, so his own
sizing only ranges **0.65 to 1.18 SOL** across the whole admissible band. A fixed
~0.9 SOL sits in the middle of that. The narrow liquidity band makes proportional
sizing nearly redundant, so deferring it costs very little. Revisit only if the band
widens materially, at which point it is a `buy_amount` *mode* on the rule, not a metric.

## 5. Rejected (measured, not assumed)

- **`range` in `m_price_window`** — recommended on 07-27, **withdrawn on 07-28**. The
  original comparison (his tokens swing 58% vs 2.7%) was against all live mints, most
  of them dead. Conditioned on the base gate, precision *falls* as the floor rises:
  4.38 -> 4.27 -> 3.91 -> 3.57 -> 3.11%. A token that already swung 90% in a minute is
  a blow-off he avoids. Would have been cheap to build (zero new state, the deques
  already exist) and actively harmful.
- **`trade_count` in `m_flow_window`** — correlation **0.936** with `unique_wallets`.
  The same signal twice; build one.
- **Narrow fingerprint scoping** — conditional on hotness every creation axis tests at
  chi2/df ~= 1.0 (the null). Creation shape carries no signal for his token pick. Use
  the broadest fingerprint and let runtime metrics filter.

## 6. Gotchas carried into this work

- **Simulate is the PnL authority; a sweep result is a ranking screener.** Always
  re-run a promoted combo through simulate.
- **Never claim "backtested" from a sweep on a re-entry rule** until item C ships with
  its episode-parity guard.
- Build lab test targets with `-j 2` (full parallelism hits pagefile error 1455 on this
  box), and pass `--target-dir "C:/Users/User/Documents/Bot/target-check"` with forward
  slashes when a bin is running.
- Helius RPC spend needs explicit approval. Nothing in this plan calls Helius: the lake
  and local Postgres cover the whole window.
