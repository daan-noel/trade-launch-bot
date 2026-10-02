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
| creation-slot buyers are crew (the dev's birth bundle, chain 33) | no spelling: a tag has no creation-slot matcher, and the bundle wallets are fresh on each coin | **open** |
| crew profit | `m_holdings.profit_sol @volume` (SOL) | **added**: the tag's bag liquidated into the curve at this print, minus its net SOL in |
| outsider buys over 3 s / 10 s | `m_flow.buy_sol @!volume [3s]` / `[10s]` | exists |
| age | `m_state.age_sec` | exists |
| vsol >= 44 at entry; vsol >= 110 | `m_state.liquidity_sol >= 14` / `>= 80` (real reserve), `m_state.on_curve = 1` | exists |
| <= 6 prints by 1 s | `m_flow.trade_count <= 7` at the one entry print | exists |
| dump-instruction sell | tag `dump` (the exact variant list as `ix_shape`, `side: "sell"`), read as `m_flow.sell_tx_count @dump [1p] >= 1` | exists (list audited by variant) |
| 1,000 s clock | `m_position.held_sec >= 1000` | exists |
| buy only at the first print at age >= 1 s | `enter.event: m_state.age_sec >= 1`, `enter.lock: "token"` | **added**: the `token` lock (the failing print spends the coin, as `slot` spends the slot) |
| signal: crew profit at target, or at 0.6 x target with outsider buys | signal `cashout`: `[profit_sol @volume >= T]` or `[profit_sol @volume >= 0.6 T, m_flow.buy_sol @!volume [3s] >= 1]` | rule grammar |
| signal before age 20 s: sell; at age >= 20 s: ride | `always`: `cashout` and `m_state.age_sec < 20` -> sell; `cashout` and `m_state.age_sec >= 20` -> go `ride`. Age stays on the line. The position starts in `open` | rule grammar |
| guard: first 30 s of the ride | signal `burst` = `m_flow.buy_sol @!volume [10s] >= 2`; stage `ride`: `burst` and `m_position.stage_sec <= 30` -> sell | **added**: `m_position.stage_sec`, seconds since the current stage began |
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
2. Engine: the `program` and `cluster` matchers,
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

Steps 1-7 are built; the parity table is section 6 of the case file. The rule spelling is
section 1: `cashout` and `burst` as signals, the age split on `always`, `open` then `ride`.
Exit labels are `crew cashout`, `ride burst`, and `dev dumps`. Each max_cost keeps its own
target (0.13 0.833, 0.65 1.178, 4.16 4.908 SOL; 0.6 x for the burst clause). FINAL keeps the
print-count door; BROAD keeps the pool door. No rule is stored: the old-list rules are deleted
(the crew left that list on 09-28); the six old-list fingerprints `7ix crew <door> <max_cost>`
and the twelve test fingerprints stay.

**Parity on the full owner split (10-02).** The crew tag is `program`, `creator`, `cluster`,
sticky (migration 0023 removes `creation_slot`). Unsaved drafts on 12 test fingerprints (`7ix
owner <door> <max_cost> <old|new>-list`: the stored criteria, the new create list without
`ExtendAccount` for `new`, tags `volume`, `dump` and the owner split's `owner`), 09-02 .. 10-01,
`lag_115`, `pumpfun_impact`, 0.03 SOL, copycat guard off, curve prints only. Reading A is the stored
params; reading B reads outsider buys on `@!owner` at 0.75 / 1.5 SOL, the `owner` tag holding the
owner split's structures as core-level and exact `ix_shape` rows. The reference is a Python
replay re-spelt from the engine's definitions (lake trades, closed millisecond windows, the
cluster counting only trades no other matcher took, a latched ride guard read on the next 200 ms
tick, the 300 s dead pool, the close fee).

| | engine tickets / reference | SOL engine / reference | %/trade engine | days in profit |
| --- | --- | --- | ---: | --- |
| A BROAD | 585 / 585 | +4.374 / +4.382 | +24.74 | 23/26 |
| A FINAL | 223 / 223 | +3.302 / +3.307 | +48.99 | 20/24 |
| B BROAD | 726 / 726 | +4.158 / +4.172 | +18.95 | 24/26 |
| B FINAL | 262 / 262 | +3.245 / +3.251 | +40.98 | 19/24 |

Every ticket exists on both sides. Of 1,796, 99 differ: 41 by the reported exit time only (same
fill), 48 by an exit fill one print away (the 115 ms boundary at millisecond rounding, or tick
against print timing; -0.034 SOL together), 10 by an entry fill one print away (+0.003 SOL), and
one coin where the reference sells on the latched guard and the engine does not (-0.005 SOL in
each reading). Reading B does not beat A: the stored reading stays.

Open:
- **The crew moved to the new create list on 09-28.** The stored fingerprints match the old list
  and take no coin after 09-28; on the new list, A BROAD books 31 tickets, +0.355 SOL, +37.9 % a
  trade over 09-28 .. 10-01. Paper trading needs the rules saved on new-list fingerprints.
- **FINAL on the new list is nearly empty** (3 tickets): `name_reuse_count` counts earlier coins of
  the same create list, and the crew reuses names across both 7ix lists. An identity tally across
  a group's lists is the extension the door needs.
- Paper trading, then real at 0.03 SOL.
- The lake keeps 30 days: 09-01 is gone, so a re-run starts 09-02.
