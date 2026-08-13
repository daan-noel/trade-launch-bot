# The "max-buy launcher" fingerprint family (`sweep 9779e08a · group N`)

Reverse-engineering of the token family behind the saved fingerprint
`sweep 9779e08a · group 1`. Corpus: local PG, 2026-07-22 → 2026-08-05 (14 d),
920 matched tokens / 109,633 trades. Companion to
[wallet-analysis.md](wallet-analysis.md) — that file profiles *traders*, this one
profiles a *launch client*.

**Verdict up front: no long-entry rule on this fingerprint is positive-expectancy.
The whole move is consumed inside the creation slot by the launcher's own bundle.
Use it as an exclusion filter, not an entry gate.**

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

## 3b. Per-tier verdicts

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

## Reproducing

Scratch tables from this run were dropped. The matched set is:

```sql
SELECT * FROM tokens t
WHERE (t.initial_buy_instruction->>'spendable_lamports_in')::numeric = 2000000000
  AND t.ix_labels = '["Pump.Fun: Create_v2","Associated Token: Create","Pump.Fun: BuyExactSolIn"]'::jsonb;
```

(the SQL mirror of `hunter_engine::fingerprint::matches` — see
`fingerprint_scope_clauses` / `sol_field_lamports_sql` in `creation_stats_repo.rs`).
