# 7ix crew rule: engine implementation and simulate parity

The two 7ix rules (FINAL and BROAD, candidate 4 in
[launch-group-7ix.md](../plans/strategies/node-derivation/launch-group-7ix.md)) run in the engine,
and a `simulate` over 09-01 .. 09-24 matches the independent Python replay
(`launch-group-7ix/g7_audit4.py`) ticket by ticket. The engine gains the smallest set of pieces
the rules need; every other term reuses an existing metric.

## 1. Term map

| rule term | engine spelling | status |
| --- | --- | --- |
| 7ix create list, CU price 1000, max_cost 0.13 / 0.65 / 4.16 | fingerprint axes: ix_labels, cu_price, `max_sol_cost` (exact) | exists; one fingerprint per max_cost |
| crew Tier 1: the 9ddjzq program heads the tx | `m_flow_ix.tagged_programs: ["Unknown (9ddjzq...)"]`, matched on `TradeLite::program_hash` through `template_grain::program_id_hash` | **extended**: a classifier field |
| crew Tier 2: the creator; a wallet once tagged on this coin | `creator_is_tagged: true`, `wallet_contagion: true` | exists |
| crew Tier 3: >= 3 same-slot prints, same ix, side, CU limit, CU price, tip, SOL within 10 % | `m_flow_ix.volume_cluster: {"min_prints": 3, "sol_tol_pct": 10}` | **extend** the classifier (causal: a print is tagged once it is the 3rd close print of its group) |
| creation-slot buyers: neither crew nor outsider | `m_flow_ix.creation_slot_buyers: "excluded"` (default `"untagged"`) | **extend** the classifier: an excluded trade counts on neither side |
| crew profit | `m_flow_ix.tagged_pnl` (SOL) | **new metric**: tagged bag liquidated into the curve at this print, minus tagged net SOL in |
| outsider buys over 3 s / 10 s | `m_flow_ix_window.untagged_buy`, `window_size_sec` 3 / 10 | exists |
| age | `m_state.time` | exists |
| vsol >= 44 at entry; vsol >= 110 | `m_state.liquidity` >= 14 / >= 80 (real reserve), `on_curve = 1` | exists |
| <= 6 prints by 1 s | `m_flow_lifetime.trade_count <= 7` at the one entry print | exists |
| dump-instruction sell | `m_dump_ix` exact variant list + `m_dump_ix_window` (`window_size_prints: 1`) `dump_sell_count >= 1` | exists (list audited by variant) |
| 1,000 s clock | `m_position.held >= 1000` | exists |
| buy only at the first print at age >= 1 s | `entry_event: {m_state.time >= 1}`, `entry_lock: "token"` | **extend** `EntryLock` (the failing print spends the token, as `slot` spends the slot) |
| ride latch: the signal at age >= 20 s | root `arm` key, the exit side's grammar: a clause that holds latches `m_position.armed` instead of closing | **extended**: the existing latch gains a clause trigger |
| guard: first 30 s of the ride | `m_position.since_armed <= 30` | **new metric**: seconds since `armed` latched, NaN before |
| name used by an earlier coin of this build | fingerprint axis `prior_identity_launches`: earlier tokens with the same create ix list and the same `(name, symbol)` identity (`identity::token_identity_hash`), trailing `PRIOR_IDENTITY_WINDOW_DAYS` | **new axis**, one timestamped tally (`fingerprint::identity_launches`) |

Two new metrics (`tagged_pnl`, `since_armed`), one new axis, four extensions of existing
config or grammar. Nothing duplicates a metric that exists: `m_holder_book` keeps per-wallet bags
but no side value, and `held` is anchored on the fill, not the latch.

## 2. Rule changes the engine spelling forces (re-booked in Python first)

| term | derived spelling | engine spelling | why |
| --- | --- | --- | --- |
| Tier 2 contagion | from a wallet's first 9ddjzq print | from any tagged print (Tier 1, 3, creator) | one contagion rule; a wallet in a volume cluster is crew on this coin |
| Tier 1 | "9ddjzq" anywhere in the labels | the tx's head program is 9ddjzq | `program_hash` is the head program |
| Tier 3 | SOL within 10 % of the group median | within 10 % of the group's first print | streaming: no stored group to re-median |
| name reuse | lower-cased name, 7ix CU-1000 coins since 09-01 | normalized `(name, symbol)`, same create list, trailing 30 days | the identity SSOT; the window is part of the rule |
| cut10 | at age 10 s, outsider sells >= 1.3 SOL | dropped | no one-moment spelling without a new anchored window; it moved the book < 0.4 points |
| dump sell | label contains `ix#5d583c` | exact variant list | patterns match whole ix lists |
| dead pools | none | the engine's Dead exit (300 s quiet) | shared deadness verdict |

The re-spelt book is `launch-group-7ix/g7_engine_ref.py`; the engine is compared against it.

## 3. Build order

1. Python: re-spell section 2 in `g7_audit4.py`, book both rules, write the per-ticket reference
   (entry print, exit print, reason, pct).
2. Engine: `tagged_programs`, `volume_cluster`, `creation_slot_buyers`, `tagged_pnl`; unit
   tests against hand-built tapes, including `every_metric_is_live_reachable`.
3. Engine: `entry_lock: "token"`, arm clauses, `since_armed`; tests for the latch order (an arm
   clause latches after the exits of that print are read, so a guard cannot fire on the latch
   print).
4. Engine: `prior_identity_launches` axis with its tally and priming (live boot, simulate).
5. Registry text, `_!___metrics.md`, `arch/strategies.md`, the fingerprint form's config fields,
   the sweep (rejects arm clauses rather than mis-scoring them).
6. Author 3 fingerprints and 6 rules; run `simulate` 09-01 .. 09-24 with `lag_115`,
   `pumpfun_impact`, clip 0.03, copycat guard **off**, no concurrency cap.
7. Parity: per ticket - entered, entry print, exit print, reason, pct; every mismatch explained
   to its cause before any number is quoted.

Definition of done: `cargo check` / clippy / tests clean on `hunter-engine`, `hunter-live`,
`hunter-lab`; parity table recorded in the case file.

## 4. Status

Steps 1-7 are built; the parity table is section 6 of the case file. Open:

- `lab lake-export` through the newest day, then re-run `g7_engine_sim.py` so the 09-22 .. 09-24
  coins join the engine side.
- `sweep::generic::guard::scan_matches_replay_flow_ix_entry_and_window_exit` fails on the base
  commit `f80fe2cb` as well as on this change: the lab replay does not enter where the scan does,
  while the engine enters on the same event stream. Unrelated to this rule; not yet diagnosed.
- Paper trading, then real at 0.03 SOL.
