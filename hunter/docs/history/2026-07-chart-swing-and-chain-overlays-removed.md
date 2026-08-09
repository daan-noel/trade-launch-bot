# Token-chart swing overlay + chain highlight — removed

Two chart overlays that shipped and were later deleted along with the `swing_1`
strategy stack. Recorded because the feature docs described them as live long after
the code was gone, and because the geometry is non-obvious if it is ever rebuilt.

**What they were.** Both hung off the token price chart
(`hunter/frontend/src/shared/components/token-price-chart/`) and rendered output of the
backend swing scan.

- **Swing overlay** (`swingOverlay.ts`, toolbar **Swings** / **Connect**) — detected
  swing legs drawn as colored line series over the price chart. Swing-high cyan
  `#0eb5ff`, swing-low magenta `#e879f9`, 3 px, no crosshair markers or price line.
  Leg key `swingLegKey(leg) = ${type}-${start_at}-${end_at}`. Three segment modes:
  `connected` (one continuous reversal path, colored per leg), `perLeg` (isolated
  start→end segments), and `connectedSequential` — connected **only within runs of legs
  adjacent in the full ledger** (`groupSequentialLegChains`), the one that mattered,
  because a visibility-filtered gap otherwise got bridged with a line that described no
  real move. Clicking resolved the leg under the pointer
  (`resolveSwingLegAtChartInteraction`) and highlighted its bars;
  `SwingCrosshairTooltip` showed duration, start/end time & price, Δ%,
  inflow/outflow/net flow, trade count.
- **Chain highlight** (`chainHighlightPlugin.ts`, toolbar **Chain link**) — a
  full-height amber translucent band across the longest swing chain's span, solid amber
  edges, top-centered chip `Longest chain · N pairs`. Chip hit-tested via
  `containsLabelPoint`; `ChainHighlightTooltip` showed pair count, in/out/net flow,
  price Δ + %, and in-band trade counts from `computeChainTradeCounts` (trades whose
  `block_time` falls inside `[startAt, endAt]`).

**Why they went.** They were the visualisation half of the swing-detection scan that
belonged to the retired `tpsl1`/`tpsl2`/`swing1` per-strategy stack. When that stack was
replaced by the one generic engine (`hunter-engine::reduce`), nothing produced the legs,
so the parent stopped passing `swingOverlay` / `highlightChain` and both plugins were
deleted. `swing-detection-logic.md` — the algorithm doc they referenced — went with it.

**What survived.** The other canvas plugins (`rangeSelectPlugin.ts`,
`walletMarkersPlugin.ts`) and the whole rest of the chart. The `chartBars.ts` comment
about canonical trade order still names "the Rust swing scan" as one of the orderings it
had to agree with.

**If rebuilt:** `connectedSequential` is the mode to start from, not `connected` — a
filtered chart with `connected` draws legs that never happened.
