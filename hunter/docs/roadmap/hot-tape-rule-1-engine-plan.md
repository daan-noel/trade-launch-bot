# Hot-tape rule 1 in the engine

Rule 1 ([hot-tape-rule-1.md](../plans/strategies/node-derivation/hot-tape-rule-1.md), evidence
1.22) goes into the ONE kernel so that simulate reproduces the Python book ticket by ticket before
any paper run. A rule dies between Python and the engine when a term, a fill or a clock means one
thing in each code. This plan removes that by construction: every term is spelled in the reference
exactly as the engine computes it (`hot-tape/r1_exact.py`, each line citing the engine code it
mirrors), the rule is re-derived under that spelling, and the engine must book the frozen
reference's tickets on the same prints before paper starts.

Governing rules: the finding sets the metric, extend it don't complicate it, one metric ships
explained once (root `CLAUDE.md`); ONE decision kernel ([arch/strategies.md](../arch/strategies.md));
the flow-group mechanics ([_!___metrics.md](../plans/strategies/_!___metrics.md)).

## 1. The rule the engine implements

```
E  a sell >= 1 SOL (this leg), from a wallet whose last buy of this coin is <= 30 s old, while
   >= 15 distinct build_core recipes printed in the 5 s up to and including it, and the spot
   made a new high <= 20 s ago
P  age >= 158 s, >= 368 distinct wallets other than the creator have bought the coin, reserve
   after the sell <= 100 SOL (liquidity <= 70), the print is on the curve
X  take profit +20 %, stop -60 %, clock 90 s
R  one position per coin; re-entry after the exit fill        S  0.35 SOL (0.2 for parity)
seat  both legs: the last print of either side landed by decision + 115 ms, in the decision
      print's slot or the next observed slot at most 3 on
```

Holdout at the engine's grain: +4.44 %/trade 5/5, top 1 % 9.8 %, 95 % interval +2.19..+6.58 %,
+2.76 % at a 500 ms seat (evidence 1.22).

## 2. The contract: every term in both codes

| term | reference (`r1_exact.py`) | engine today | work |
| --- | --- | --- | --- |
| trigger: a sell of 1 SOL or more | this leg's SOL | `m_flow.sell_sol [1p] >= 1` | reuse |
| seller bought 30 s ago or less | now minus the wallet's last buy of this coin | `m_print.since_buy_sec` | built (1c) |
| 15 or more recipes in 5 s | distinct `build_core` in the closed window `[now_ms - 5000, now_ms]`, the print included, floor-ms positions | `m_crowd.unique_ix_shapes [5s]` over `trade_keys::build_hash` | built (1a, 1b); partitions all 25,842 lake label sequences exactly as `build_core` does |
| new high 20 s ago or less | `max(0, now - time of the last strictly higher spot)` | `m_price.stall_sec` | reuse; the registry text now says spot (1e) |
| age 158 s or more | now minus `created_at` | `m_state.age_sec` | reuse |
| 368 buyers or more | distinct wallets, not the creator, with a buy at or after creation | `m_crowd.buyer_count [age0s]` | reuse; the capped scan at 369 costs nothing measurable (a 132,992-coin replay folds in minutes) (1d) |
| reserve 100 or less | `vsol` after the print | `m_state.liquidity_sol <= 70` | reuse |
| on the curve | the tapes are curve rows | `m_state.on_curve`, fed by the venue on all three adapters (the lab row carried no venue and hard-coded the curve) | built (1f) |
| entry fill | the exit leg's rule (below) | `LagMs`: one helper, `paper_fill::lag_fill_idx`, on both legs | fixed (1g); before it the entry took the last BUY and priced 29 % of rule 1's entries at a print our buy cannot meet |
| exit fill | last print of either side by fire + 115 ms, fire slot or next observed slot at most 3 on | `LagMs` exit, the same | reuse |
| take profit, stop | `pnl %` against the entry print's spot, each print after the entry fill | `take_profit 20` / `stop_loss 60` | reuse |
| clock | `held >= 90` on a print or on the 200 ms tick grid (first event + 200 ms); a tick fires from the last folded print | `m_position.held_sec >= 90`, the replay's tick | reuse; parity aligns the grid's first event |
| re-entry | a print after the exit fill, at or after its time | `reentry {cooldown_sec: 0, max_per_coin: 1000}` | reuse |
| never on a tick | only prints decide | a 1-print window holds its print through ticks | `m_print.since_buy_sec` reads NaN on a tick, so a tick cannot fire; a test pins it |
| caps, guard, universe | none | concurrency cap, copycat guard, fingerprint | cap 0, `skip_duplicate_identity` off, a wildcard fingerprint |
| cost | the engine kernel: impact `B/vsol` at each leg's print, 125 bps on notional + proceeds, 0.000225 SOL a leg | `pumpfun_impact` | reuse |
| a coin's history | coins born before the tape's first print are left out | simulate starts at its corpus | parity compares coins born inside the corpus |
| a coin that dies | no death: a drained, quiet coin that revives can fire later | a coin quiet 300 s with liquidity under 30 is retired, and its later trades are ignored, live and simulate alike | engine semantics stand; 3 of 1,054 reference tickets sit on revived coins and cannot happen live |

## 3. Steps

### Step 0: done (evidence 1.22)

Terms checked against the lake's exact fields; the holder term re-spelled as distinct buyers; the
book re-read after each correction; the rule re-derived on an every-leg study under the corrected
fill; a second code (`hot-tape/r1_exact_check.py`) rebuilds every ticket. Frozen reference:
`node-derivation/data/r1_ref_{holdout_exact,study_exact}.parquet` (450 / 604 tickets).

### Step 1: done (engine extensions)

- 1a. `trade_keys::build_hash` beside `ix_hash` (the hash SSOT), drop list in `is_build_noise`;
  `TradeLite::build_hash`, set by all three adapters. `build_hash_partitions_like_the_study_build_core`
  reads `engine/fixtures/build_core_parity.json` (200 lake sequences with their `build_core`); its
  `--ignored` twin read all 25,842 sequences of lake days 09-01..09-10: 20,897 recipes, equal.
- 1b. `m_crowd.unique_ix_shapes`, over `metrics/distinct_window.rs`, the distinct-count
  mechanism shared with `m_crowd.unique_wallets`; arms in `on_trade` and `on_tick`; `needs_ix_labels`.
- 1c. `m_print.since_buy_sec`: NaN on a tick and for a wallet that never bought; the map opens
  only when a loaded rule reads it; `needs_wallet_identity`.
- 1d. The crowd cap at 369 left as is: the full replays run in minutes.
- 1e. The price metrics' registry text says spot, and so does the `TradeLite::price` doc.
- 1f. `m_state.on_curve`; `CorpusTrade` carries the venue from the lake and the PG tail.
- 1g. `LagMs` entry leg takes the exit leg's rule. It moves simulate and the grouped sweep; live
  paper books `worst_case` on both legs and is untouched. `lag_*` runs stored before 2026-09-11 price
  the entry on the last buy and do not compare.
- 1h. `_!___metrics.md`, `fill-and-cost-models.md`, `arch/strategies.md`; unit tests on each
  metric and on the fill; `every_metric_is_live_reachable` reads every metric.

### Step 2: done (the rule, authored)

`node-derivation/data/r1p_rule.json`, written from the derived rule by `hot-tape/r1_engine_parity.py prep`
in the v1 spelling; the parity example reads it through `hunter_engine::v1::parse_params_any`, which
converts it to this rule:

```json
{ "enter": { "filters": [
    { "metric": "m_flow.sell_sol", "span": "1p", "is": [{"operator": ">=", "value": 1.0}] },
    { "metric": "m_print.since_buy_sec", "is": [{"operator": "<=", "value": 30}] },
    { "metric": "m_crowd.unique_ix_shapes", "span": "5s", "is": [{"operator": ">=", "value": 15}] },
    { "metric": "m_price.stall_sec", "is": [{"operator": "<=", "value": 20}] },
    { "metric": "m_state.age_sec", "is": [{"operator": ">=", "value": 158}] },
    { "metric": "m_state.liquidity_sol", "is": [{"operator": "<=", "value": 70}] },
    { "metric": "m_state.on_curve", "is": [{"operator": "=", "value": 1}] },
    { "metric": "m_crowd.buyer_count", "span": "age0s", "is": [{"operator": ">=", "value": 368}] } ] },
  "always": [ { "if": [{ "metric": "m_position.held_sec", "is": [{"operator": ">=", "value": 90}] }], "sell": true } ],
  "take_profit": 20, "stop_loss": 60,
  "reentry": { "cooldown_sec": 0, "max_per_coin": 1000 } }
```

### Step 3: done (parity)

`hunter/lab/examples/hot_tape_rule1_parity.rs` replays the lake through the lab's lake load,
`run_replay`, `LagMs(115)` and the engine cost kernel, and names every print by
`(slot, tx_index, leg)`; `hot-tape/r1_engine_parity.py compare` matches it to the frozen tickets
(evidence 1.23).

| corpus | reference | engine | same trigger | fill, exit, reason, SOL | reference only |
| --- | --- | --- | --- | --- | --- |
| holdout_exact | 450 | 448 | 448 | all equal, SOL to 1.5e-16 | 2 |
| study_exact | 604 | 603 | 603 | all equal, SOL to 1.5e-16 | 1 |

The three reference-only tickets are on two coins the engine retired as dead (drained, quiet 300 s)
before they revived. The engine book is byte-identical with AMM prints loaded or not. The untouched
days after 09-10 are still to read, once, engine and reference together.

### Step 4: paper

Open decision first: live paper (`exec_paper`) books `worst_case` on both legs, not the `LagMs(115)`
seat the rule is validated under, so a paper day would grade a different fill. Either paper gains the
lag model or the paper book is read against a simulate run under `worst_case`. Then: every new coin
tracked from birth; a restart rebuilds the buyer set and the wallet -> last-buy map from a complete
history; the per-token maps fit the 4 GB box. A pass bar is written before the first day; a real
decide-to-fill delay is logged on every ticket; each paper day is replayed in simulate and must
reproduce the paper tickets.

### Step 5: small real SOL

The tip above 0.000225 SOL a leg and failed buys inside a frenzy are measured here.
