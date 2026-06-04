The kink is not a bug in swing-low detection — it comes from how the overlay is drawn.

## One continuous line, multiple legs

`swingsToLineData` builds **one polyline** by chaining every leg’s **start** and **end** (two points per leg):

```70:78:f:\pumpfun\meme-trading\frontend-react\src\components\token-price-chart\swingOverlay.ts
  for (const leg of swings) {
    const tStart = resolveChartTime(leg.start_at, groupMode, trades);
    const tEnd = resolveChartTime(leg.end_at, groupMode, trades);
    const yStart = swingPriceToChartY(leg.start_price, metric, toValue);
    const yEnd = swingPriceToChartY(leg.end_price, metric, toValue);

    if (tStart != null) points.push({ time: tStart, value: yStart });
    if (tEnd != null) points.push({ time: tEnd, value: yEnd });
  }
```

At the high→low reversal (~13:49:21) the path is:

| Point | Source | Meaning |
|-------|--------|---------|
| A | swing high **end** | Last **buy** in the long up-leg |
| B | swing low **start** | First **sell** that opens the down-leg |
| C | swing low **end** | Last **sell** in that leg |

The purple line is **A → B → C**, not a single straight A → C.

## Why it looks like a “joint”

The steep segment is usually **A → B** (end of swing high → start of swing low). The shallower segment is **B → C** (rest of the swing low).

Those use different rules in the analyzer:

- Swing high `end_price` = price of the **last buy**
- Swing low `start_price` = price of the **first sell**

On VIRL at 2 SOL, those are only ~39ms apart but different trades:

- High end: `1.861e-13` @ 13:49:21.732  
- Low start: `1.804e-13` @ 13:49:21.771  
- Low end: `1.692e-13` @ 13:49:47.877  

The big red 1s candle can show a much larger drop than A→B, because the candle aggregates many sells while the overlay only uses **first/last sell trade prices** for B and C.

With 1s time grouping, A and B often land in the **same second**; `ensureStrictlyIncreasingTimes` then nudges one point to the next second, which makes the corner at B easier to see.

## Summary

The corner is the **handoff between swing high and swing low**, not an extra pivot inside one leg. The overlay always draws leg boundaries, so a reversal with a gap between last buy and first sell shows as two slopes. To get one straight drop you’d need different drawing (e.g. only `end` points, or candle OHLC at reversal time).