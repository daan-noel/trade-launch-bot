# Wave node (gap + N terminal buys): refuted at our seat

Node-2 study per the decision-node plan in
[market-model-and-workflow.md](market-model-and-workflow.md). Tables
`census.wave_events` (219,190 events, top-20 mass terminal builds, post-cutover),
`census.wave_path` / `wave_path_n` (400-per-cell random sample, exact path
outcomes, true N recounted from the tape - `gap_breaks` keeps only the first
buy of a breaking slot, so N must come from trades).

## Result

- At entry break+2 slots (our ~95ms seat), median path geometry is NEGATIVE in
  every cell with usable n: med up 5-28% vs med down 6-45%, across all 20
  builds, all N buckets, all curve positions. N raises both tails, not the edge.
- Pre-registered money test on the best a-priori configuration (N>=2 same-build,
  vsol 40-100, rule-v0 exits): **-0.70 SOL / 261 events / 244 mints / 46.4%
  win**. Red on a proper population.
- Mechanism: the wave's up-move is consumed inside the breaking slots by the
  auction winners; at +2 slots we are the exit liquidity. Consistent with the
  racer census and with why profitable fast readers hold 0-2s.

## The slow-reader thread: also closed

Waves a slow reader (hold >= 10s) joins have positive TAIL asymmetry (p75 up
+77.6% vs down 45.7%), which motivated a structural-tell derivation. The tells
exist and are strong: joined waves are big-ticket (median max buy in the
breaking slot 0.99 vs 0.10 SOL), multi-party (>=2 wallets, >=2 builds),
below the 30-min running max, pre-tape not buy-heavy. Gate stack (max buy
>= 0.5 SOL, >= 2 buying wallets, vsol < 0.95 x run-max, pre-120s buy <= 1.5x
sell) reproduces reader selection at 93.8% on a balanced sample.

The pre-registered money test refutes it anyway: **-35.87 SOL / 10,057 trades
/ 4,613 mints / 37.9% win** on the full post-cutover universe (tables
`census.w1_cand/w1_gated/w1_money`). The decisive split: even the waves a slow
reader ACTUALLY joined read -18.56 SOL / 39.4% win at our entry, barely better
than unattended (-17.31 / 35.9%). Median path geometry for reader-joined gated
waves is negative (up 19.2% vs down 27.9%; the +77.6% is the p75 tail only), so
no exit scheme can rescue it. The readers' profit is their ENTRY PRICE inside
the breaking slots - consistent with the winner-population result that ~93% of
gross edge is entry price. At break+2 slots the edge does not exist.

## Racing seat: the edge is being AHEAD of the wave, not in its slot

Same gated universe repriced at every seat (`census.w1_money_race`,
`w1_money_first`, `w2_money`):

| seat | fill | pnl SOL | win |
| --- | --- | --- | --- |
| land first (ahead of every wave buy; uses end-of-slot gates = look-ahead) | pre-break state | +44.41 | 57.0% |
| land second (trigger = first buy >= 0.5 SOL after gap; nothing later used) | after first print | -24.07 | 41.7% |
| land last in breaking slot | end-of-slot state | -31.16 | 39.3% |
| slot +1 | end of slot+1 | -35.98 | 38.1% |
| slot +2 (original) | first print >= +2 | -35.87 | 37.9% |

The whole edge (~75 SOL / 10k trades) is the wave's own price impact between
its first and last buy. Beating every follower after seeing the first big buy
still loses: the first buy already carries the impact we would need. A
tape-observable trigger cannot be ahead of the print that triggers it, so the
wave node does not pay at ANY seat - it pays only to whoever IS the wave (or
sees it before the tape: mempool / leader view).

**Node 2 is closed at every seat, including the reader-selection branch.**
The remaining harvestable node on the curve is node 1 (dev campaign push),
where the up-move develops over minutes, not slots.
