# 7ix crew rule: engine implementation and simulate parity

The two 7ix rules (FINAL and BROAD, candidate 4 in
[launch-group-7ix.md](../plans/strategies/node-derivation/launch-group-7ix.md)) run in the engine,
and a `simulate` over 09-01 .. 09-24 matches the independent Python replay
(`launch-group-7ix/g7_audit4.py`) ticket by ticket. The engine gains the smallest set of pieces
the rules need; every other term reuses an existing metric.

## 1. Term map

The crew is one fingerprint tag, `volume`; the dump list is a second, `dump`.

| rule term | engine spelling | status |
| --- | --- | --- |
| 7ix create list, CU price 1000, max_cost 0.13 / 0.65 / 4.16 | fingerprint axes: ix_labels, cu_price, `max_sol_cost` (exact) | exists; one fingerprint per max_cost |
| crew Tier 1: the 9ddjzq program heads the tx | tag `volume`, matcher `program: ["Unknown (9ddjzq...)"]`, matched on `TradeLite::program_hash` through `template_grain::program_id_hash` | **added**: the `program` matcher |
| crew Tier 2: the creator; a wallet once tagged on this coin | tag `volume`, matcher `creator: true`, option `sticky: true` | exists |
| crew Tier 3: >= 3 same-slot prints, same ix, side, CU limit, CU price, tip, SOL within 10 % | tag `volume`, matcher `cluster: {"min_prints": 3, "sol_tol_pct": 10}` | **added** (causal: a print carries the tag once it is the 3rd close print of its group) |
| creation-slot buyers: neither crew nor outsider | tag `volume`, option `exclude_creation_slot: true`: such a trade counts in neither `@volume` nor `@!volume` | **added** |
| crew profit | `m_holdings.profit_sol @volume` (SOL) | **added**: the tag's bag liquidated into the curve at this print, minus its net SOL in |
| outsider buys over 3 s / 10 s | `m_flow.buy_sol @!volume [3s]` / `[10s]` | exists |
| age | `m_state.age_sec` | exists |
| vsol >= 44 at entry; vsol >= 110 | `m_state.liquidity_sol >= 14` / `>= 80` (real reserve), `m_state.on_curve = 1` | exists |
| <= 6 prints by 1 s | `m_flow.trade_count <= 7` at the one entry print | exists |
| dump-instruction sell | tag `dump` (the exact variant list as `ix_shape`, `side: "sell"`), read as `m_flow.sell_tx_count @dump [1p] >= 1` | exists (list audited by variant) |
| 1,000 s clock | `m_position.held_sec >= 1000` | exists |
| buy only at the first print at age >= 1 s | `enter.event: m_state.age_sec >= 1`, `enter.lock: "token"` | **added**: the `token` lock (the failing print spends the coin, as `slot` spends the slot) |
| signal: crew profit at target, or at 0.6 x target with outsider buys | signal `cashout`: `[profit_sol @volume >= T]` or `[profit_sol @volume >= 0.6 T, m_flow.buy_sol @!volume [3s] >= 1]` | rule grammar |
| signal before age 20 s: sell; at age >= 20 s: ride | stage `early` (`ends: {"age_sec": 20}`): `cashout` and `m_state.age_sec < 20` -> sell; at its end `cashout -> go ride`, else `late`; stage `late`: `cashout -> go ride` | rule grammar |
| guard: first 30 s of the ride | stage `ride`: `m_flow.buy_sol @!volume [10s] >= 2` and `m_position.stage_sec <= 30` -> sell | **added**: `m_position.stage_sec`, seconds since the current stage began |
| name used by an earlier coin of this build | fingerprint axis `name_reuse_count`: earlier tokens with the same create ix list and the same `(name, symbol)` identity (`identity::token_identity_hash`), trailing `NAME_REUSE_WINDOW_DAYS` | **added**: one timestamped tally (`fingerprint::identity_launches`) |

The vsol >= 110, dump and clock exits are `always` lines, read in every stage. Nothing
duplicates a metric that exists: `m_holdings.bag_share_pct` keeps per-wallet bags but no side
value, and `m_position.held_sec` is anchored on the fill, not on the stage.

## 2. Rule changes the engine spelling forces (re-booked in Python first)

| term | derived spelling | engine spelling | why |
| --- | --- | --- | --- |
| Tier 2 contagion | from a wallet's first 9ddjzq print | from any tagged print (Tier 1, 3, creator) | one `sticky` rule; a wallet in a volume cluster is crew on this coin |
| Tier 1 | "9ddjzq" anywhere in the labels | the tx's head program is 9ddjzq | `program_hash` is the head program |
| Tier 3 | SOL within 10 % of the group median | within 10 % of the group's first print | streaming: no stored group to re-median |
| name reuse | lower-cased name, 7ix CU-1000 coins since 09-01 | normalized `(name, symbol)`, same create list, trailing 30 days | the identity SSOT; the window is part of the rule |
| cut10 | at age 10 s, outsider sells >= 1.3 SOL | dropped | it moved the book < 0.4 points; the grammar states it as an `at_end` line of a stage ending at `age_sec` 10, not re-booked |
| dump sell | label contains `ix#5d583c` | exact variant list | `ix_shape` matches whole ix lists |
| dead pools | none | the engine's Dead exit (300 s quiet) | shared deadness verdict |

The re-spelt book is `launch-group-7ix/g7_engine_ref.py`; the engine is compared against it.

## 3. Build order

1. Python: re-spell section 2 in `g7_audit4.py`, book both rules, write the per-ticket reference
   (entry print, exit print, reason, pct).
2. Engine: the `program` and `cluster` matchers, `exclude_creation_slot`,
   `m_holdings.profit_sol`; unit tests against hand-built tapes, including
   `every_metric_is_live_reachable`.
3. Engine: `enter.lock: "token"`, stages with deadlines, `m_position.stage_sec`; tests for the
   stage order (a `go` takes effect from the next print or tick, so a guard cannot fire on the
   print that moves into `ride`).
4. Engine: `name_reuse_count` axis with its tally and priming (live boot, simulate).
5. Registry text, `_!___metrics.md`, `arch/strategies.md`, the fingerprint editor's tag
   matchers, the sweep.
6. Author 3 fingerprints and 6 rules; run `simulate` 09-01 .. 09-24 with `lag_115`,
   `pumpfun_impact`, clip 0.03, copycat guard **off**, no concurrency cap.
7. Parity: per ticket - entered, entry print, exit print, reason, pct; every mismatch explained
   to its cause before any number is quoted.

Definition of done: `cargo check` / clippy / tests clean on `hunter-engine`, `hunter-live`,
`hunter-lab`; parity table recorded in the case file.

## 4. Status

Steps 1-7 are built; the parity table is section 6 of the case file. The six stored rules are
the v1 converter's form (a `start` / `armed` stage pair). The form section 1 writes - the
`cashout` signal, `early` ending at age 20 s, `late`, `ride` - books the same tickets as the
stored form on all six rules over 09-01 .. 09-21: same entries, exit prints and SOL. Open:

- Store that form in the six rule rows, so the readout and the exit labels say the rule's own
  words (`crew cashout`, `ride burst`, `dev dumps`).
- `lab lake-export` through the newest day, then re-run `g7_engine_sim.py` so the 09-22 .. 09-24
  coins join the engine side.
- Paper trading, then real at 0.03 SOL.
