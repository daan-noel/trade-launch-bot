This isn’t a bug in your token data — it’s how the swing detector is defined versus what the chart shows.

## What the detector actually measures

Reversals use **cumulative SOL per trade** (`sol_amount` on each buy/sell), **not** price change on the bonding curve.

From the spec and code:

```276:279:f:\pumpfun\meme-trading\backend\src\analyzers\swing_analyzer.rs
                    frozen_threshold = params
                        .high_to_low_threshold_sol
                        .min(params.high_to_low_threshold_pct / 100.0 * snapshot.abs());
```

- **High → low:** need `min(fixed_sol, 50% × current swing-high net flow)` of **sell SOL** in a row before the next buy.
- **Low → high:** need `max(fixed_sol, 50% × |swing-low net flow|)` of **buy SOL** in a row before the next sell.
- If sells (or buys) stop short of that bar, a **buy (or sell) merges** the temp leg back into the current leg — the chart dip is ignored.

`start_price` / `end_price` in the table are per-token SOL prices (~`e-15`); thresholds are **flow in SOL** (tens of SOL net on legs).

## VIRL with your settings (verified on live API)

| Threshold | Swings |
|-----------|--------|
| **2 SOL** | 3 (one long high → short low → shorter high) |
| **3 SOL** | **0** |

With **3 SOL**, no reversal ever confirms over all 5,183 trades. The run ends in an active swing-high leg, which is **discarded** at end-of-history — so the API returns an empty list.

With **2 SOL**, the first confirmed reversal is at **13:49:21** (~7.6 minutes after the first trade). Everything before that is one continuous **swing high**.

Early behavior on this mint:

- First **60s:** ~**35.6 SOL** buys, ~**0.73 SOL** sells → huge **price** spike, tiny sell flow.
- Many early sell bursts (0.52, 0.16, 0.20 SOL, etc.) are **interrupted by buys** and merged back — never 2 SOL, and never 3 SOL in one temp-low cluster.
- ~**354** merge-backs vs **3** confirmations at 2 SOL; at 3 SOL, **354** merges and **0** confirmations.

So **3 SOL** is too high for how fragmented trading is here (~0.02 SOL average trade). **2 SOL** is barely low enough.

## Why the “first swing” looks missing at 2 SOL

The chart overlay draws **confirmed legs**, not every visual move.

What you’re treating as the first swing (launch vertical move, ~13:46 dip) is mostly **price on the curve**, not separate SOL legs:

1. **Launch spike** — almost all buys; not a separate swing; it opens swing high #1 at the first buy.
2. **~13:46 dip** — sell clusters stay **under 2 SOL** before buys resume → merged into the same swing high until **13:49:21**.
3. **First numbered swing** is that **7.6m swing high** (net ~39.55 SOL), not the initial wick alone.

The overlay line starts after the vertical wick because leg `start_price` / `end_price` track **last same-side trade in the leg**, while the wick is a few early trades moving the curve a lot in SOL terms.

## Practical takeaway

- **2 vs 3:** threshold is **opposite-side SOL volume without interruption**, not “3 SOL on the chart Y-axis.”
- **Missed first swing:** expected with 2 SOL on a fast, interleaved pump — use a lower fixed SOL (e.g. **0.5–1**), or accept that legs follow **flow**, not chart pivots.
- **0 swings at 3:** no leg ever fully confirmed; not that the token had no moves.

If you want detection closer to chart pivots, the algorithm would need a **price- or %-based** reversal rule in addition to (or instead of) SOL flow — that isn’t what it does today.