Two joints appear because the overlay is a **polyline through every leg endpoint**, not one segment per swing low. A single swing low leg only supplies **two** of those points; the rest come from **adjacent legs** and from **time snapping**.

## At 2 SOL (VIRL) — the main dump ~13:49

There are **6** overlay points (2 per leg × 3 legs). Around the big red candle the path is:

| # | Time (after snap) | Role | Price |
|---|-------------------|------|-------|
| 1 | 13:49:21 | Swing high **end** (last buy) | peak |
| 2 | 13:49:22 | Swing low **start** (first sell) | −3% |
| 3 | 13:49:47 | Swing low **end** (last sell) | −6% more |
| 4 | 13:49:48 | Next swing high **start** (first buy) | ~flat (−0.08%) |

So the “down” stroke is **three segments**, not one:

1. **Steep:** high end → low start (leg handoff, ~1s after snap) — **joint 1**  
2. **Shallower:** low start → low end (the actual swing low leg, ~25s) — slope change at low start  
3. **Nearly flat:** low end → next high start (next leg handoff, ~1s) — **joint 2** (horizontal step before the line turns up)

That third step is still the end of the down-move visually; it’s not inside the swing-low detector as a third pivot, it’s the **low → next high** boundary.

`ensureStrictlyIncreasingTimes` forces points that share a second (e.g. 13:49:21 and 13:49:47) onto **13:49:21 → 22 → 47 → 48**, which makes both corners easier to see.

## Why it can look like “2 joints on the swing low” at 13:51

With **2 SOL**, there are **no** overlay vertices between **13:49:48** and **13:52:20** — only one rising segment through 13:51. The pink line should **not** bend with the red candle there; it cuts through as a chord.

If you see **two kinks around 13:51:47–50** (including a **flat** step), that matches **more legs** (e.g. **1 SOL** threshold): a short swing high ends, a tiny swing low with **same start/end price** (~1.949e-13), then the next leg — four points in four seconds, including a **0% horizontal** 1s segment from duplicate price + time bumping.

## Summary

| Joint | What it is |
|-------|------------|
| **1** | Swing high end → swing low start (last buy vs first sell) |
| **2** | Swing low end → next swing high start (last sell vs first buy; often almost flat) |

The swing low leg only draws **start → end**; the extra corner is always the **next leg’s start** (and the previous leg’s end). To get one straight drop you’d need one point per reversal (e.g. only leg ends), not start+end per leg.