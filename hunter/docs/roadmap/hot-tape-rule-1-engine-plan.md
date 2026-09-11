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
the flow-group mechanics ([metrics-reference.md](../plans/strategies/metrics-reference.md)).

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
| trigger: a sell of 1 SOL or more | this leg's SOL | `m_flow_window {window_size_prints: 1}.sell >= 1` | reuse |
| seller bought 30 s ago or less | now minus the wallet's last buy of this coin | nothing | new: `m_print_wallet.since_buy` (1c) |
| 15 or more recipes in 5 s | distinct `build_core` in the closed window `[now_ms - 5000, now_ms]`, the print included, floor-ms positions | nothing (the template grain is a different key) | new: `build_hash` (1a) + `m_build_window.unique_builds` (1b) |
| new high 20 s ago or less | `max(0, now - time of the last strictly higher spot)` | `m_price_lifetime.stall` | reuse; its registry text says EXECUTION prices while the fold reads `fill_basis` spot: fix the text (1e) |
| age 158 s or more | now minus `created_at` | `m_state.time` | reuse |
| 368 buyers or more | distinct wallets, not the creator, with a buy at or after creation | `m_crowd_after_age {after_age_sec: 0}.non_creator_buyers` | reuse; fold cost at a cap of 369 (1d) |
| reserve 100 or less | `vsol` after the print | `m_state.liquidity <= 70` | reuse |
| on the curve | the tapes are curve rows | nothing: on a PumpSwap pool `liquidity <= 70` holds whenever the pool drains 15 SOL, so the rule would fire on graduated coins the reference never saw | new: `m_state.on_curve` (1f) |
| entry fill | the exit leg's rule (below) | `LagMs` entry: the last BUY by the deadline, sells ignored; prices 29 % of rule 1's entries at a print our buy cannot meet, 6.9 % dearer on average, 1.8 % inside an unfinished transaction | fix: the entry leg takes the exit leg's rule (1g) |
| exit fill | last print of either side by fire + 115 ms, fire slot or next observed slot at most 3 on | `LagMs` exit, the same | reuse |
| take profit, stop | `pnl %` against the entry print's spot, each print after the entry fill | `take_profit 20` / `stop_loss 60` | reuse |
| clock | `held >= 90` on a print or on the 200 ms tick grid (first event + 200 ms); a tick fires from the last folded print | `m_position.held >= 90`, the replay's tick | reuse; parity aligns the grid's first event |
| re-entry | a print after the exit fill, at or after its time | `reentry {cooldown_sec: 0, max_episodes_per_token: 1000}` | reuse |
| never on a tick | only prints decide | a 1-print window holds its print through ticks | `since_buy` reads NaN on a tick, so a tick cannot fire; a test pins it |
| caps, guard, universe | none | concurrency cap, copycat guard, fingerprint | cap 0, `skip_duplicate_identity` off, a wildcard fingerprint |
| cost | the engine kernel: impact `B/vsol` at each leg's print, 125 bps on notional + proceeds, 0.000225 SOL a leg | `pumpfun_impact` | reuse |
| a coin's history | coins born before the tape's first print are left out | simulate starts at its corpus | parity compares coins born inside the corpus |

## 3. Steps

### Step 0: done (evidence 1.22)

Terms checked against the lake's exact fields; the holder term re-spelled as distinct buyers; the
book re-read after each correction; the rule re-derived on an every-leg study under the corrected
fill; a second code (`hot-tape/r1_exact_check.py`) rebuilds every ticket. Frozen reference:
`node-derivation/data/r1_ref_{holdout_exact,study_exact}.parquet` (450 / 604 tickets).

### Step 1: engine extensions

- 1a. `build_hash`: one hasher beside `flow_ix::ix_hash` (the hash SSOT), its drop list one const
  (`Associated Token: Create*`, `*: CloseAccount`, `Memo Program*`); on `TradeLite`, set by all
  three adapters. A shared fixture of label sequences and their equal classes is read by a Rust
  test and by a Python test of `lake_export.build_core`.
- 1b. `m_build_window` (dynamic): `unique_builds`, distinct build recipes printed in the window,
  every window unit, an occupancy map for O(1) reads, arms in `on_trade` and `on_tick`, the
  `ix_labels` load obligation declared on the metric.
- 1c. `m_print_wallet` (static): `since_buy`, seconds since the wallet behind this print last
  bought this token; NaN on a tick and for a wallet that never bought it. One wallet -> last-buy
  map per token, allocated only when a loaded rule names the group; `needs_wallet_identity`.
- 1d. `m_crowd_after_age` at a cap of 369 scans its set linearly on each buy: measure, and store a
  hash set above a size if it costs; the meaning does not change.
- 1e. The price metrics' registry text says what the fold reads (spot).
- 1f. `m_state.on_curve`: 1 when the last print traded on the bonding curve (`TradeLite::on_curve`).
- 1g. `LagMs` entry leg: the last priced print of either side by the deadline, the same window as
  the exit leg. Every rule's simulate and live paper price entries this way from the change on;
  runs stored before it are marked as priced under the old entry leg.
- 1h. `metrics-reference.md` gains the new groups; unit tests on a hand-computed tape;
  `every_metric_is_live_reachable` covers them; `cargo check` on hunter-live and hunter-lab,
  clippy, no new warnings.

### Step 2: the rule, authored

```json
{ "entry": {
    "m_flow_window": { "window_size_prints": 1, "sell": [{"operator": ">=", "value": 1.0}] },
    "m_print_wallet": { "since_buy": [{"operator": "<=", "value": 30}] },
    "m_build_window": { "window_size_sec": 5, "unique_builds": [{"operator": ">=", "value": 15}] },
    "m_price_lifetime": { "stall": [{"operator": "<=", "value": 20}] },
    "m_state": { "time": [{"operator": ">=", "value": 158}],
                 "liquidity": [{"operator": "<=", "value": 70}],
                 "on_curve": [{"operator": "=", "value": 1}] },
    "m_crowd_after_age": { "after_age_sec": 0,
                           "non_creator_buyers": [{"operator": ">=", "value": 368}] } },
  "exit": { "m_position": { "held": [{"operator": ">=", "value": 90}] } },
  "take_profit": 20, "stop_loss": 60,
  "reentry": { "cooldown_sec": 0, "max_episodes_per_token": 1000 } }
```

### Step 3: parity

- Simulate over the `holdout_exact` corpus (lake 09-03..09-10) and the `study_exact` corpus (lake
  09-01..09-06), every token, `LagMs(115)`, `fill_delay_ms` 0. Bar: every ticket of
  `r1_ref_*.parquet` matches on trigger, entry-fill and exit print (slot, transaction, leg) and
  reason, and the SOL matches. Every mismatch is explained and fixed in the engine or the reference
  before moving on.
- Then the untouched days after 09-10, read once, engine and reference together.

### Step 4: paper

Every new coin tracked from birth; a restart rebuilds the buyer set from a complete history; the
per-token maps fit the 4 GB box. A pass bar is written before the first day; a real
decide-to-fill delay is logged on every ticket; each paper day is replayed in simulate and must
reproduce the paper tickets.

### Step 5: small real SOL

The tip above 0.000225 SOL a leg and failed buys inside a frenzy are measured here.
