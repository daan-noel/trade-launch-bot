# The "max-buy launcher" fingerprint family

Reverse-engineering of the launch tool behind the saved fingerprint `sweep 9779e08a`, and of
the one axis inside it that carries edge. Corpus: local PG 2026-07-22 -> 08-05 (14 d, 920
matched tokens / 109,633 trades) for the tool profile, and the 26-day lake (07-22 .. 08-16)
for the band work, with fills and costs ported from `core/src/strategies/paper_fill.rs` +
`kernel.rs`. Companion to [wallet-analysis.md](wallet-analysis.md) - that file profiles
*traders*, this one profiles a *launch client*.

**Two verdicts, and the second scopes the first.**

1. **No long-entry rule on a funding tier as a whole is positive-expectancy.** The move is
   consumed inside the creation slot by the launcher's own bundle. Use the tier as an
   exclusion filter, not an entry gate.
2. **The tool's launches are not homogeneous.** Cut any tier by creation-slot bundle SOL and
   the 15-30 SOL band is positive while the rest carries the loss. The band is the tradeable
   axis; the funding tier only correlates with it.

## 1. What the fingerprint actually selects

`group 1` axes: `spendable_lamports_in = 2_000_000_000` (exact, `bucket_size_amount
IS NULL`) and `ix_labels = ["Pump.Fun: Create_v2", "Associated Token: Create",
"Pump.Fun: BuyExactSolIn"]`.

It is **not one dev**: 920 tokens across **626 distinct creator wallets**, 624 of
which launch exactly once. Only two wallets repeat —
`BBhFEWSC9x2HBqP8Uk6ig8MxzobrXUanATnuTY5oM9B5` (152) and
`CmwFWFQsK2tuNpz3HAbjoxDVSf5miY5vbP425UTRg5Rd` (144).

The real invariant is arithmetic. For all 920 rows:

```
initial_buy_lamports = 1_975_308_641   (identical to the lamport)
spendable_lamports_in / initial_buy_lamports = 1.0125   exactly
```

`1.0125` is pump.fun's **125 bps buy fee**. So the creation tx spends the wallet's
entire balance *inclusive of fee* — a **"max buy" button** in some launch client.
`cu_limit`/`cu_price` are NULL because the client emits no compute-budget
instructions at all (hence the bare 3-label `ix_labels`).

The same ratio holds at every funding preset, which is all the other sweep groups
are:

| spendable | tokens | creators | dev buy (SOL) | ratio | sweep group |
| --- | --- | --- | --- | --- | --- |
| 1.0 | 549 | 475 | 0.987654320 | 1.0125 | 2 |
| 1.5 | 143 | 119 | 1.481481480 | 1.0125 | 3 |
| 2.0 | 920 | 626 | 1.975308641 | 1.0125 | **1** |
| 3.0 | 678 | 542 | 2.962962962 | 1.0125 | 0 |
| 4.0 | 46 | 46 | 3.950617283 | 1.0125 | 5 |
| 5.0 | 185 | 185 | 4.938271604 | 1.0125 | 4 |

Family total (any preset): **2,536 tokens / 1,996 creator wallets / 15 presets**.
The grouped sweep did not find six dev crews — it found **one tool**, split by
funding tier. Group 1 = "a fresh wallet funded with exactly 2.000 SOL, max-buy".

## 2. The habit

**Launch is a bundle, not a solo dev buy.** Creation slot holds a median of 8 legs
from 8 distinct wallets totalling **9.77 SOL** of buys (p25 7.0, p75 11.0, p95
14.5). The co-buy amounts are *also* `X / 1.0125` (1.4814, 2.2716, 3.4568,
0.9877 …) — same client, same operator, several wallets in one slot.

**The bundle is the pump.** Price from the pre-dev-buy state to the last trade of
the creation slot: **+55% median** (p25 +34%, p75 +64%, p95 +94%). An outsider's
first tradable price is already the top of that markup.

**The dev exits at ~5 seconds.** Creator-wallet first sell, from creation:

| p25 | p50 | p75 | p90 |
| --- | --- | --- | --- |
| 4.0 s | 5.3 s | 6.9 s | 9.6 s |

663 of 910 devs (73%) sell; median proceeds **4.19 SOL** on a 1.975 SOL buy ≈
**2.1×**. The other 27% recover nothing — no buyers showed up.

**Then it dies.** ATH at median **8.5 s** after creation. Median lifetime 110 s,
**89% dead**, 1.5% migrated (vs 2.1% baseline).

**Clock.** 85%+ of launches fall in 20:00–06:00 UTC; 07:00–19:00 UTC is nearly
empty.

**Why it looked promising.** Against every token created in the same window
(n = 269,579), this fingerprint selects genuinely hot launches: median 68 trades
and 45.8 SOL volume vs baseline 5 trades / 2.31 SOL — ~20× the activity. The
activity is real; the *edge* is not, because it is manufactured and pre-sold.

## 3. Why no rule works

Backtest over the 891 tokens with trade history, entry = curve state at latency
`L`, exits filled at the **actual triggering trade price** (not the theoretical
stop level), 30 s time stop, 5% round-trip cost (125 bps/leg fee + ~1.7% impact +
tip). Net return per trade:

| entry latency | best TP/SL found | avg net / trade |
| --- | --- | --- |
| p_open (perfect next-slot snipe) | any | **−20% … −26%** |
| 0.5 s | TP 300% / SL 15% | +0.5% |
| 1.0 s | TP 300% / SL 15% | +0.6% |
| 2.0 s | TP 300% / SL 40% | −2.2% |
| 3.0 s | any | −7% … −8% |

Every trailing stop (10/20/30/45%) is **negative at every latency** — the tail is
too thin and the retrace too fast to pay for the give-back.

Three things kill it:

1. **Entering at `p_open` is the worst possible trade.** `p_open` *is* the bundle's
   markup top; price retraces immediately. Being faster makes it worse, not better.
2. **A ~3 pp/second latency cliff.** The entire distribution is consumed between
   0.5 s and 3 s.
3. **No causal filter separates winners.** Quintiles of 1-second outsider buy SOL,
   distinct 1-second buyers, and 1-second price gain over `p_open` are all
   non-monotonic noise (−10% … +14% per trade, no ordering). The one split that
   *looks* decisive — first-5-second buy volume, +47%/trade in the top quintile —
   is **lookahead**: our entry is at 1 s, so it only restates "tokens that went up,
   went up". Rebuilt causally at ≤1 s, the signal vanishes.

From `p_open` the median ATH is only **1.40×**, reached at 8.5 s, against a dev who
is already selling at 5.3 s. An outsider is trading against a group holding ~10 SOL
of inventory with a five-second exit plan.

## 3b. The post-rug bounce — tested and refuted

The one hypothesis with a causal story rather than a data-mined one: at ~5 s the dev
market-sells ~2 SOL for *scripted* reasons, not demand-driven ones, so the sell should
overshoot and revert. Tested on the **full 2,536-token family** (323,059 trades), entry
gated on `dev has already sold` (observable live from the trades feed), with an
**out-of-sample split** (IS = before 2026-07-30, n≈500; OOS = 07-30 → 08-05, n≈1,100).
Cost per trade: 250 bps fee round-trip + `2 × 0.27 / vsol` impact + 0.004 SOL fixed,
≈ **5.3%** on a 0.27 SOL buy.

**The bounce is real.** From a post-rug entry at 8 s, median peak within 60 s is
**+17.6%** (OOS), 55% of tokens bounce ≥10%, and a perfect-foresight exit at the 60 s
peak nets **+38.7%** OOS. The raw material exists.

**It is not harvestable.** All 36 wide-exit cells (entry 8/10/12/15 s × trail 25/40% /
60 s stop × dip gate) are **negative out-of-sample**, −4% to −22% per trade. The few
mildly positive in-sample cells (+1% … +4%) all flip hard negative OOS — textbook
overfit. Re-tested with exits *sized to the measured effect* (TP 5/8/12/20% × SL 10/20%
× hold 5/10/20 s, 24 cells): **every cell negative in both splits**, and with
SE ≈ 0.006 these are 5–10σ losers, not noise.

Decomposition of the best cell (entry 8 s, TP 20 / SL 20, 5 s hold):

| split | n | avg gross | avg cost | net |
| --- | --- | --- | --- | --- |
| IS | 436 | 1.0236 | 0.0539 | **−3.2%** |
| OOS | 979 | 1.0148 | 0.0528 | **−3.9%** |

That is the whole story: **the bounce is worth ~1.5–2.4% gross and the round trip costs
~5.3%.** It clears the spread only under perfect foresight. The gap between the oracle
(+38.7%) and every realizable rule (−3.9%) is an exit-timing problem no stop can solve —
the bounce is fast and its peak is unpredictable.

Combined with §3, **60 pre-registered rule variants across both entry regimes produce
zero positive out-of-sample cells.** Treat the long side of this fingerprint as closed.

## 3c. Per-tier verdicts

The blacklist above pools all six funding tiers. Searched one tier at a time, they differ.
**Group 1** (the 2 SOL preset, this document's cohort) is an exclusion filter, not an
entry fingerprint: any long-entry rule on it is an execution bet (authority vs
optimistic fill), not an edge. **Group 2** (the 1 SOL preset, fp `e6299eac`) is a
different tier. A verdict on one tier says nothing about another.

## 4. Use it as an exclusion filter

The positive-expectancy roles in this structure are (a) being inside the creation
slot, which means being the operator, or (b) being short, which the bonding curve
does not offer. Neither is available.

What the fingerprint *is* good for: a **blacklist**. It identifies, at
`TokenCreated` and with no deferred axes, a launch whose next 10 seconds are
pre-scripted and whose 89% terminal state is dead. Matching it is a reason for the
engine not to arm, not a reason to buy.

**Do not widen the axes to "fix" it.** The 1.0125 ratio means `spendable_lamports_in`
and `initial_buy_lamports` are the same fact — a fingerprint pinning both is pinning
one axis twice, and the exact-mode `spendable` axis alone already identifies the
client.

## 5. The bundle band is the tradeable axis

`spend=5` is a **proxy for a creation-slot bundle of 15-30 SOL**. The band is the edge; the
funding tier only correlates with it. Gate any tier of the same tool on the band and it pays;
strip the band from `spend=5` and it stops paying.

## The cohort carries the edge, the rule does not

One battery, identical values, `next_slot_median` fills, `pumpfun_impact` costs, 0.05 SOL:

| cohort | n | hold 30 s | hold 60 s | TP+8/SL-15 | trail 36 + stall 30 | LIQ>20 + trail/stall |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| spend=1 | 877 | +8.8 | -1.7 | +0.3 | -9.3 | -19.1 |
| spend=1.5 | 247 | +2.6 | +0.2 | -6.5 | -9.7 | -22.8 |
| spend=2 | 2271 | -1.2 | -10.9 | -4.0 | -17.7 | -23.9 |
| spend=3 | 934 | -1.5 | -7.6 | -6.0 | -11.8 | -16.0 |
| spend=4 | 72 | +28.4 | +49.8 | +7.1 | +52.8 | +40.3 |
| **spend=5** | **395** | **+11.5** | **+18.3** | **-4.9** | **+16.5** | **+19.4** |
| max=0.108 | 1154 | -4.1 | -8.4 | -0.2 | -10.0 | -10.6 |
| max=4.08 | 1377 | -23.7 | -31.1 | -4.5 | -22.6 | -6.3 |

(net %/trade. The full 14-cohort table lives in the run scratch.)

A **blind 60-second hold** on `spend=5` returns **+18.3%/trade**. The promoted rule's entry
gate adds ~3 pp on the same exits (+16.5 ungated -> +19.4 with `liquidity > 20`) at the cost
of 89 trades, and its engineered exit set does not beat a flat 60-second timeout. The rule is
not what makes the money; admission to the cohort is.

`liquidity > 20` reads as `vsol > 50` ([liquidity-metric-is-real-reserves]) and `vsol` at the
first reachable print is itself a function of the creation bundle, so the one gate that pays
is a weak restatement of the band below.

## The band

Creation-slot buy SOL, pooled across the six tiers of the tool, blind 60 s hold:

| bundle SOL | n | mean %/trade | median | win % | p90 |
| --- | ---: | ---: | ---: | ---: | ---: |
| < 5 | 243 | -5.1 | -27.9 | 9.9 | -13 |
| 5-10 | 2484 | -10.8 | -40.3 | 12.3 | +97 |
| 10-15 | 1285 | -7.0 | -48.7 | 17.8 | +170 |
| **15-20** | **556** | **+16.3** | -58.8 | 31.3 | +209 |
| **20-30** | **186** | **+33.0** | -67.4 | 36.0 | +287 |
| > 30 | 42 | -15.1 | -78.1 | 21.4 | +234 |

The tiers sit in different parts of it: `spend=5` puts 85% of its launches in 15-30 SOL,
`spend=2` puts 87% below 15. That is the whole difference between the two fingerprints.

**Both directions confirm it.** Gate the five *other* tiers on 15-30 SOL and drop `spend=5`
entirely: **+20.5%/trade, n=408, 95% CI [8.1, 33.3], 4/4 weeks positive** — better than
`spend=5` itself. The same tiers ungated return **-6.8%** (CI [-9.6, -3.9], n=4401). Restrict
`spend=5` to bundles under 15 SOL and it returns **-4.9%** (n=47).

Pooled over every tier the band holds 742 launches, **+20.5%/trade, CI [10.6, 30.5]**, +7.60
SOL at 0.05 size, ~31 launches/day.

`spend=2` alone inside the band is a live cohort a fingerprint-level search discards as a
loser: **+22.5%/trade, n=166, CI [3, 43], 4/4 weeks positive**.

**The axis is already expressible.** `fingerprints.first_slot_buy_lamports` is a
bucket-matched axis and a deferred first-slot gate, so the band needs no new metric — it
needs a fingerprint whose identity is the bundle, with the funding tier left `NULL`.

## The payoff is a fat tail, and that dictates the exit

Inside the band: **win rate 31-36%, median -58%, top-5 trades = 29-38% of P&L, p90 +209 to
+287%**. Every profitable configuration is a lottery whose tickets are cheap enough.

That is why a take-profit destroys it (`spend=5`: +18.3% blind -> **-4.9%** with TP+8/SL-15)
and why any selection objective that rewards win rate, penalizes variance, or trims outliers
selects against the only trades that pay.

**It is latency-flat**, which is the property that makes it worth shipping: `signal_price`
+21.3 -> `next_slot_first` +19.4 -> `next_slot_median` +18.3 -> `worst_case` +16.6. A ~22%
spread across the entire fill-model range with no sign flip, unlike the 4.08 launcher's
collapse between slot 2 and slot 3 ([fp-4sol-launcher.md](fp-4sol-launcher.md)).

## Two payoff shapes need two rule families

The cohorts split cleanly, and the split predicts which rule family can work:

| shape | signature | cohorts | rule family |
| --- | --- | --- | --- |
| **skew** | median crashes, mean positive, p90 max-up +210-290%, life ~190 s | the `BuyExactSolIn` spend tiers | no TP, wide trail or a flat timeout; harvest the tail |
| **grind** | median slightly positive, mean ~0, p90 max-up +45-110%, life 320-480 s | the `Buy · max=` tiers | small fixed TP, hit-rate; a trail bleeds it out |

Applying the wrong family inverts the result in both directions: a TP costs `spend=5`
23 pp, and a trail costs `max=0.108` 10 pp against its flat-TP baseline. A search that does
not classify the cohort before choosing a template finds nothing on roughly half of them.

**Classify first, with three numbers from the cohort's own tape** — `mean - median` of the
forward-60 s return, the p90 max-up, and median lifetime — then pick the template family.

## Screening every shape: what survives a false-discovery control

Screening **all** 424,620 tokens that have post-creation prints across the 76 ix shapes with
>= 150 tokens, cutting each shape by every fingerprint-expressible axis (`bundle band`,
creation-slot wallet count, slot-0 selling, `init_buy`, `max_cost`, `spendable_in`,
`cu_price`, `cu_limit`) gives 1,278 deduped cells. Split IS 07-22..08-09 / OOS 08-10..08-16.

**A screen without a null is not a screen.** Re-running the identical criteria 20 times with
the outcomes shuffled *inside each shape* measures how many winners chance alone produces:

| bar | survivors | by chance | false-discovery rate |
| --- | ---: | ---: | ---: |
| `IS>0, OOS>0, mean-1.96se>0, every week positive` | 34 | 17.9 | **53%** |
| + lower bound >= 5%/trade | 19 | 4.5 | 24% |
| + **median > 0** | 4 | **0.0** | **0%** |
| + win >= 45%, n >= 150 | 3 | 0.0 | 0% |

**`median > 0` is the discriminator.** Shuffled data reproduces a fat-tailed positive *mean*
easily — that is what a lucky reshuffle looks like — but it essentially never produces a group
whose *typical* token gains. Any group selected on mean alone is a coin-flip; a group whose
median clears zero is real.

That splits every candidate into two classes, and they want different treatment:

- **Class A — grind.** Median above zero, wins most trades, chance-rate zero. Trade it.
- **Class B — skew.** Positive mean carried by a tail, negative median. Roughly one in four is
  noise, so it needs forward evidence before size. The `3ix:BuyExactSolIn` family, including
  `spend=5`, is Class B.

### The Class A catalogue

Every survivor is `5ix:BuyExactSolIn`
(`CU limit`, `CU price`, `Create_v2`, `ATA CreateIdempotent`, `BuyExactSolIn`):

| group | rule | n | mean | IS | OOS | 95% CI | win | median |
| --- | --- | ---: | ---: | ---: | ---: | --- | ---: | ---: |
| **`cu_price=75210`** | hold 60 s | 287 | **+28.4** | +31.4 | +22.6 | [22.2, 35.0] | 77% | +34.2 |
| **`cu_price=75210`** | TP+10/SL-25/held 120 | 287 | +12.9 | +12.3 | +13.8 | [10.3, 15.5] | **94%** | +9.4 |
| `fs_buy` 10-20 | hold 60 s | 423 | +19.6 | +24.8 | +9.4 | [11.1, 28.5] | 52% | +9.9 |
| `fs_buy` 20-30 | hold 60 s | 140 | +30.0 | +35.7 | +17.3 | [12.8, 48.1] | 62% | +25.5 |

**`cu_price = 75210` (with `cu_limit = 301000`) is a distinct operator, and the sharpest cut
in the lake.** Its dev buy is **15.1 SOL** against 2.7-3.3 for every other `cu_price` under the
same shape, it launches ~15/day from 07-28, and 288 of its 293 launches carry this one ix
shape. The complement (`cu_price != 75210`, n=1007) returns +3.2%/trade with an 18% win rate
and a 95% CI spanning zero — the cu_price axis carries the entire edge, and the bundle bands
were partly reading the same tokens.

It is **latency-flat and deep**: hold 60 s prices at signal +51.2 / next_slot_first +28.7 /
next_slot_median +28.4 / **worst_case +27.8**, and holds +23.4%/trade at 1.0 SOL under
`worst_case`. Sizing is limited by nothing measured here below 2.0 SOL.

### 6ix / 7ix / 8ix carry no group on these axes

257,428 tokens across 138 shapes, and **zero cells survive even the loosest bar**. Every
bundle band in all three sizes is negative on a blind 60 s hold, from -2.8% to -36.4%, with
win rates of 6-42%. Their unconditional means (-12.1 / -11.5 / -12.7) are the worst of any
instruction count. This is a negative result about *these axes at whole-shape grain*, not a
proof that nothing exists there: shapes under 150 tokens are excluded, and only single-axis
cuts plus one interaction are tested.

## Habit axes only, with a volume floor: the constraint set is nearly empty

A launcher's *habit* axes are the values its software chooses and that are known at creation:
`max_cost`, `spendable_in`, `init_buy`, `cu_limit`, `cu_price`. `first_slot_buy_lamports` is neither --
it mixes the launcher's own bundle with outsiders joining, and it settles only after the
creation slot closes. Re-running the screen on habit axes alone (1,088 deduped cells, singles
and cu-pairs) with a tokens/day floor:

| filter | groups | by chance |
| --- | ---: | ---: |
| habit axes, no median test | 22 | 7.2 |
| + median > 0 | **1** | 0.0 |
| + >= 20/day | **0** | 0.0 |
| + >= 50/day | **0** | 0.0 |

The single survivor is `cu_limit=301000` / `cu_price=75210` at 11 launches/day. **Habit axes,
volume, and a positive median are not simultaneously satisfiable in 26 days of lake.** Pick two.

**A screen measures the raw cohort, not the tunable ceiling.** A cohort whose median is
negative under a blind hold can still clear zero once a gate removes the losers -- that is what
`liquidity` does below. Use the screen to rank where tuning pays, never to reject a fingerprint.

## Tuning `5ix:BuyExactSolIn` — the liquidity gate is the lever

Whole shape, no amount axis, 43.7 launches/day, blind entry + 60 s hold at 0.126 SOL:
**+5.3%/trade, IS +5.35 / OOS +5.23, win 34%, median -19.3**. A positive mean on a losing
typical token — the cohort to fix.

Sweeping one entry gate at a time (47 candidates, exit held at 60 s), the `m_state.liquidity`
band is the only lever that moves the median, and it moves it on a smooth ridge:

| gate | /day | mean | IS | OOS | win | median |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| none | 43.7 | +5.3 | +5.4 | +5.2 | 34% | -19.3 |
| `liquidity > 5` | 34.7 | +9.5 | +9.0 | +10.6 | 43% | -26.8 |
| `liquidity > 10` | 28.5 | +14.3 | +16.7 | +10.0 | 50% | +1.3 |
| **`liquidity > 12`** | **27.0** | **+14.3** | +16.7 | +10.0 | 52% | **+8.4** |
| `liquidity > 15` | 25.3 | +13.0 | +14.8 | +9.7 | 54% | +10.4 |
| `liquidity > 20` | 22.7 | +9.0 | +10.8 | +5.4 | 55% | +11.3 |
| `liquidity > 30` | 15.8 | -0.1 | +0.8 | -1.8 | 56% | +8.1 |

Dip entries are refuted again here: every `trail(10s) >= 5..30` gate scores -9.6 to -19.1.

### Take-profits look best and are the fill trap

`liquidity > 10` + `TP+30 / SL-15 / held 120` reads +10.8%/trade, 67% win, median +25.5, and
its whole TP x SL neighbourhood (tp 15-50 x sl 10-30, 35 cells) is positive — a genuine
plateau, not a spike. It still fails the honest bar:

| config | signal | next_slot_first | next_slot_median | **worst_case** |
| --- | ---: | ---: | ---: | ---: |
| `liq>10` TP+30/SL-15/held120 | +12.8 | +12.6 | +10.8 | **+2.7** |
| `liq>10` TP+20/SL-25/held120 | +7.1 | +9.4 | +7.3 | **+0.5** |
| `liq>10` held 60 s | +21.7 | +17.5 | +14.3 | **+10.0** |

A take-profit fires on a **momentary** event, so the adverse fill catches the bottom of the
window; a time stop is **persistent state** and barely moves. Live paper and the sweep both
book `worst_case`, so rank on it.

### The rule

**`5ix:BuyExactSolIn` · entry `m_state.liquidity > 12` · exit `m_position.held >= 60`**
— 703 trades, **27.0/day**, +14.3%/trade at `next_slot_median` and **+9.6% at `worst_case`**,
IS +16.7 / OOS +10.0, 95% lower bound +8.0, win 51.9%, median **+8.4**, every week positive,
+12.7 SOL at 0.126. `liquidity > 15` is the conservative sibling (25.3/day, worst +8.2,
median +10.4, win 53.6%). Shipped inactive as fp `0e49ee26`, rules `5bb46221` / `2f98a36b`.

## `4ix:BuyV2` does not tune — the liquidity gate is not portable

Same tuner, same metric vocabulary, on `4ix:BuyV2` (`CU price`, `Create_v2`,
`ATA CreateIdempotent`, `BuyV2`; 1,114 tokens, 40.7/day, blind hold-60 +4.8%/trade). **Zero of
110 configs clear worst_case > +3% with a positive median and every week positive**, whole
shape or restricted to its dominant habit value `cu_price = 500` (938 tokens, 86% of the shape).

Two diagnostics say why, and both are reusable:

- **The gate inverts.** On `5ix:BuyExactSolIn` the median rises monotonically with
  `liquidity` (-19.3 -> +11.3 across 0 -> 20). Here it *falls*: `liq>8` -11.5, `liq>10` -10.5,
  `liq>15` -9.4, and only `liq>30` reaches -0.8 — by which point volume is 14.7/day and OOS
  turns negative. A lever that works on one cohort is not a lever on another.
- **In-sample and out-of-sample disagree everywhere.** IS runs +7 to +11 while OOS runs -4 to
  +2 on essentially every gate. That gap, present across the whole grid rather than in a few
  cells, is cohort decay, not a tuning miss.

Best config found is `liq>30 + held 30 s`: 14.7/day, worst_case **+2.4%**, OOS +2.0, median
-0.8 — below the cost bar's margin of safety and at a third of the volume that made the shape
interesting. Nothing shipped.

**The transferable rule: check whether the gate's ridge points the right way before sweeping
exits.** One entry-gate sweep against a fixed exit answers it in a single pass, and an
IS-versus-OOS gap that holds across the entire grid means the cohort is decaying — stop there.

## The screen that works: recent blind return predicts tunability

Earlier screens ranked groups by properties measured *before* tuning and asserted the ranking
predicted tunability. It did not -- `4ix:BuyV2` ranked near the top and tuned to nothing. The
fix is to generate labels first, then test candidate screens against them.

**Protocol.** 21 groups spanning 3ix..8ix and the habit axes, each run through one fixed
tuning grid (8 entry gates x 6 persistent-state exits, priced at `next_slot_median` AND
`worst_case`). A group is **TUNABLE** when some config clears `worst_case > +3%`, `OOS > 0`,
every week positive, `n >= 60`. The label scores **edge only** -- volume is recorded separately
and never mixed in, and there is no median requirement (that would blind the label to
tail-harvest groups). Grid + labels: `grid.parquet` / `labels.parquet` in the run scratch.

**Result: 3 of 21 tunable.** `3ix spend=5`, `5ix:BuyExactSolIn`, `5ix:BuyExactSolIn cu_price=75210`.
The labels reproduce 5 of the 6 priors we already knew.

**One pre-tuning number separates all 21 groups perfectly** -- the mean net return of a blind
60 s hold measured on the **recent (out-of-sample) half** of the window:

| verdict | blind OOS return per trade |
| --- | --- |
| TUNABLE (3 groups) | +9.3, +13.3, +17.4 |
| NOT (18 groups) | -24.9 … +6.7 |

AUC 1.00, leave-one-out accuracy 95.2% against an 85.7% majority baseline. Whole-window blind
return is *not* sufficient (AUC 0.98): `4ix:BuyV2` averages +11.4% over the full window but only
+6.7% recently, which is exactly the group that fooled the earlier screen. **Measure the recent
half, not the whole window.**

Why it works: a rule redistributes a cohort's existing drift, it does not create drift. If the
unconditional recent return already clears the ~3.5% round-trip bar with surplus, a gate has
something to shape; if it does not, no gate can invent it.

**Known false negative.** `3ix:Buy max=0.108` labels NOT (blind OOS +2.3%), yet a rule gated on
`m_flow_window(1s).unique_wallets <= 9` does work there
([fp-3ix-buy-max0108 study](../../history/README.md) notwithstanding, see the fingerprint's own
notes). `unique_wallets` and `trail` were dropped from this grid for speed, so a NOT verdict
means "not tunable with **this** vocabulary", never "untunable". Treat the screen as a
priority order, not a gate.

**Caveat on power.** Three positives is thin. Perfect separation on three points is encouraging,
not established -- it wants more positives before being trusted as a filter.

### The three tunable groups, verified per trade (0.126 SOL, `next_slot_median`)

| group | rule | trades | /day | mean | median | win | IS -> OOS | worst_case | top-5 share |
| --- | --- | ---: | ---: | ---: | ---: | ---: | --- | ---: | ---: |
| `3ix spend=5` | `liq>30` + held 60 s + `stall>=30` | 201 | 7.7 | **+36.3%** | +3.3 | 50.7% | +27.6 -> **+58.9** | **+32.1** | 24.2% |
| `5ix:BuyExactSolIn` | `liq>10` + held 120 s + SL-30 | 741 | 28.5 | +15.2% | -34.6 | 35.9% | +19.5 -> +7.5 | +10.4 | 29.0% |
| `5ix:SOLIn cu_price=75210` | `liq>10` + held 60 s + `retrace>=35` | 280 | 10.8 | +20.0% | **+31.7** | **76.1%** | +22.8 -> +14.7 | +19.5 | **10.3%** |

- **`3ix spend=5` beats `g4` on a far simpler rule** and its weekly path *rises*
  (+4.1 / +20.4 / +42.2 / +58.9). Per-trade return is flat from 0.05 to 0.5 SOL (+35.05 SOL at
  0.5). Caution: OOS n is only 56 and OOS > IS by 2x, so the level is high-variance even though
  the sign is stable.
- **`cu_price=75210` is the highest-quality distribution** -- 76% win, positive median,
  top-5 trades only 10.3% of P&L, and latency-flat (signal +28.2 -> worst +19.5). Its weekly
  path decays (+28.7 / +19.5 / +14.7) and it only has 3 weeks of history.
- **`5ix:BuyExactSolIn` is a tail harvester** (median -34.6) and is decaying hardest
  (+38.0 -> +7.5). Highest volume of the three.
- Every 6ix / 7ix / 8ix group tested lands at -4% to -25% blind OOS. Nothing there.

## Forward evidence

`-- promoted g4 c213838 --- v2` (`07b27aef`) runs in paper 08-12..08-16 and takes **57
positions**: **+30.7% gross/trade, median -11.3%, 42% up, 22.8 s median hold**. That window
is entirely after its tuning cutoff, and the shape matches the backtest. Its
`exit_sol_lamports_total` is 0 on all 57 rows, so every stored PnL reads -100% — the
zero-proceeds accounting defect, not the trade outcome. Read forward P&L on this rule from
`exit_price / entry_price` until that is fixed.

## How to apply

- **Search the bundle axis, not the funding axis.** A fingerprint identifies launch
  *software*; the tool's own launches are heterogeneous, and pooling them dilutes a real band
  to zero. Cut every candidate cohort by `first_slot_buy_lamports` before concluding it is
  dead.
- **Rank on total SOL with the tail intact.** On a skew cohort the mean is five trades; a
  bootstrap CI is the honest gate, not a t-stat.
- **Check the reverse direction.** A gate that pays is only a gate if removing it from the
  winning cohort kills the edge and adding it to a losing one revives it.
- The band's threshold is not tuned — bins are fixed a priori and the effect is monotone
  across three adjacent bins — but it is chosen on this window, so it wants an out-of-time
  re-measure before size.
- Everything here is backtest. Nothing in this family has traded forward.
## Reproducing

Scratch tables from this run were dropped. The matched set is:

```sql
SELECT * FROM tokens t
WHERE (t.initial_buy_instruction->>'spendable_lamports_in')::numeric = 2000000000
  AND t.ix_labels = '["Pump.Fun: Create_v2","Associated Token: Create","Pump.Fun: BuyExactSolIn"]'::jsonb;
```


Related: [fill-and-cost-models.md](fill-and-cost-models.md), [execution-costs.md](execution-costs.md),
[rule-search-method.md](rule-search-method.md).

[liquidity-metric-is-real-reserves]: metrics-reference.md
