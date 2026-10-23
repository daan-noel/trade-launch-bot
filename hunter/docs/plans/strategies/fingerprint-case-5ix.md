# Fingerprint case: 5ix 400K (`--- cu_limit=400K · cu_price=250K · spend=1 · 5ix:BuyExactSolIn`)

The 5ix group's reference: identity, data, what the owner does, the rules in the DB, the four
studies so far and why each fell short, and the current run. The method is
[fingerprint-rule-method.md](fingerprint-rule-method.md).

---

## 1. Identity

| What | Value |
| --- | --- |
| Fingerprint | id `14a62dab-ec0d-4d26-a7f9-868c0225ab23` |
| Owner tag | `volume`, `sticky`, with `"creator": true` (the creation-tx buyer's bag is his) |
| Dump tag | `dump`: `m_flow.trade_count @dump >= 1` |
| "spend=1" | the creator's creation buy, not our size |
| Our buy | size ladder 0.05 SOL now, 0.1 if the rule is stably profitable, 0.3 at most; the studies use 0.05; the DB rule T3 buys 0.025 SOL |
| Benchmark | T3 (`194c9b96`), read only in W9 |
| Study folders | `hunter/_local/study-5ix-400k/`, `study-5ix-threat/` (`tree/`, `fresh/`) |

Tag facts:

- The `volume` tag's generic shapes match about 90 % of SOL. Their wallets trade a median 3 mints
  across the market in 3 weeks (outsiders 68.5; 77 % trade 20+): disposable wash wallets.
  `@volume` = owner, `@!volume` = real outsiders.
- On the AMM `sticky` keeps the curve-tagged wallets tagged: they carry 40.5 % of AMM sell SOL,
  so `@!volume` there = wallets not tagged on the curve.
- `m_crowd.unique_wallets` and `buyer_count` are untagged and count his wash wallets.

---

## 2. Data

| What | Value |
| --- | --- |
| Coins | 1182 on 09-29..10-20, 111 migrated |
| Cache | `D:/Bot-cold/study-5ix-threat/fresh` (rows: 4.28 M engine rows, 531 K trades); tape `../trades2.csv`; engine series `ms2` |
| AMM tape | backfilled with `sync-mints --incremental --amm-slots 54000` (about 0.1 credit a tx; 3 coins have no AMM trades) |
| Regime | from **10-07** the opening is busier: owner trades in the first 3 s 13-18 (8-11 before), deaths within 3 s 8-18 % (0-9 % before) |
| Frozen end | 10-20; 10-21 onward is the forward test |
| Recency window | newest week 10-14..10-20 -> 10 days 10-11..10-20 -> regime 10-07..10-20; days before 10-07 are not used |
| Lake | `LAKE_RETENTION_DAYS=14` in `.env`; an export prunes older days, so the replayable range moves forward daily. Engine replays run before 10-07 leaves |

---

## 3. What the owner does

### Ledger (10-11..10-20)

- Curve coins are bait: he nets +1 to +5 SOL a coin from outsiders.
- Migration is his payday: on a migrated coin he is -81 SOL on the curve and +330 SOL on the AMM.
- He pays the pool himself: about 30 SOL of his money in per 10 SOL of peak pool; outsiders bring
  3-25 SOL a coin.

### Outcomes (09-29..10-17)

| Class | Coins | Path |
| --- | --- | --- |
| migrated | 98 | 5.3x at completion, median 70 s |
| runner (peak >= 3x, no migration) | 114 | 4x at 65 s, then 0.4x |
| fade | 586 | 1.6x at 9 s, falls at 12-20 s |
| flop | 218 | never rises, falls at 3-8 s |

- At the moment of the fall a runner's turn and a migrating coin's dip look alike (he pushes
  10-15 SOL, sells 7-9 within 2 s, outsiders net about 0).
- His volume sells start about 2 s into every class. Dump start (his bag sold 30 %) median: flop
  3.5 s, fade 8 s, runner 47 s.
- His reaction to an outsider buy of 1 SOL or more: a sell within 0.3 s on 60 % (30 % otherwise),
  median 0.22 s. The reaction to buys of our size (0.05 SOL) is not measured yet.

### AMM

- Price against the last curve price: median 1.11 at 1-60 s, 0.94 at 120 s, 0.12 at 600 s; peak
  median 1.97x at 71 s. He sells into the AMM buyers about 1 : 1.
- Holding to the AMM end loses.

## 3b. Fast dump and the streak gate

Read from the server DB (coins 10-14..10-22, 422 coins, 42 migrated) and the real positions of
the 5ix rules.

- **The launch.** The creator and 4 bundle wallets buy about 19.8 SOL in the creation slot. A
  **fast dump** is the bundle selling half of that within 4.5 s (26-50 % of coins a day since
  10-14; about 25 % overall, more in streaks from 10-18).
- **Big early money.** Outsiders buying 3 SOL or more in the first 1 s: 95 % of those coins are
  dumped early (21 coins). When the last 3 coins were all dumped early, the next one is too in
  81 % of cases (base 26 %).
- **The live loss.** 10-21 22:30 to 10-22 00:30: -1.9 SOL over 41 coins and 116 positions.
  At 22:53 seven launches in 70 s were all dumped at 2-4 s (about -0.6 SOL); the rest were
  small coins (peak pool 20-35) that died 7-30 s after the bundle sale, with 3 migrations (7 %,
  against 10-16 % on normal days). Our 5 real rules each bought within 0.1 s, about 0.35 SOL
  per coin: a quarter to a third of all early outsider money.
- **No exit saves a snipe from the bundle sale.** On the 133 early-dumped coins, holding loses
  -2.0 SOL and the best exit (owner's first sale, take-profit, drop from the peak, time) loses
  -0.5, and every exit cuts the winners on the other coins.
- **Gate result** (event = pool died within T s, last 3 coins, 30 min): T 5 s skips 17 coins,
  76 % vs 21 % died fast next, 0 of 42 migrated skipped, +0.51 SOL on a 0.05 snipe; T 10 s
  skips 37 coins, 1 migrated skipped, +0.86. On the real positions the "last 3 dumped early"
  gate would have skipped 35 positions worth -0.71 SOL.
- **Gate in money** (422 coins 10-14..10-22; value = minus the PnL of the skipped coins under six
  plain buy-and-hold policies): T 30 s, N 2, peak limit 30 SOL skips 70 coins (2 migrated) and
  saves 0.75-1.6 SOL; T 30 s, X 25, N 2 skips 35 (0 migrated) and saves 0.35-1.0; T 5 s, N 3
  saves 0.1-0.5. Coins that die at a peak of 25 SOL or less die at a median of 4 s; 25-30 SOL
  94 % within 30 s; above 40 SOL median 65 s. Streaks exist mainly from 10-18 on, so the
  numbers are one regime.
- **Sessions.** A streak is a farming session (for example 10-19 03:12-03:22: 20 coins in 10
  minutes, every one dumped within seconds); no coin of such a session migrated.
- Wallets of the user's copy-trading team (YSH, MSI, KSI) are in the early pot on most coins
  since 10-20; they are a reference, not a spec.
- Scripts and notes: `hunter/_local/study-5ix-threat/dump21/` (NOTES.md).

---

## 4. Rules in the DB (paper, inactive)

| Rule | Id | What it is |
| --- | --- | --- |
| T1 safest | `40c49e6b` | snipe; threat lines; AMM exit `net_sol @!volume [10s] <= -1` |
| T2 recommended | `fac3c071` | the same, middle thresholds |
| T3 max | `194c9b96` | the same plus `operator_cash_out`; **the benchmark** |
| v3 | `491a9710` | the first staged rule (snipe + 4 lines) |
| round 3 rule A | `bdb5a187` | liquidity-cell lines |
| round 5 | `22f0fbd4` | 3 thresholds: outsider net 2 / 3 by pool, `trail_pct [30s] >= 50` |

T-rule lines (each with its story):

- `outsiders_underwater`: `m_holdings.profit_sol @!volume <= X`: he has dumped below their entries.
- `early_crowd_dumped`: age < 5 s and `m_flow.net_sol @!volume [5s] >= 1.9`: he dumps into an early crowd.
- `operator_stopped_pushing`: age 20-40 s and `m_flow.net_sol @volume [30s] <= X`: no push, no migration.
- `pool_dead`: pool < 10, age >= 10 s, `m_flow.sell_count @!volume [5s] <= 1`.
- `operator_cash_out` (T3): pool 45-65 and `m_holdings.profit_sol @volume >= 5.8`.

T3 in the engine: 09-29..10-11 +9.51 SOL; 10-12..10-18 +3.16; tree-study splits PICK 10-07..10-17
+3.05, HOLD 10-18..10-20 +1.96, OLD 09-29..10-06 +9.46 (93 of 114 migrated kept).

---

## 5. Studies so far, and why each fell short

| Study | What it did | Result | Why it fell short |
| --- | --- | --- | --- |
| 400k rounds 2-5 | snipe; sell lines per pool band; read his policy; smooth thresholds; blocked folds | best eval +2.57 vs v3 +3.06 | ~20 free thresholds overfit; peaks, not plateaus |
| 400k round 10 | meaning audit of the DB rules | two lines read something else than their name | a tag that excludes nothing; a "gross" read that is a buy; lifetime reads after a late entry |
| threat | outcome classes; AMM exit first; threat lines under a migrated cap | T1/T2/T3 | it stands; later redos lose to it |
| threat redo | 0 migrated sold; stable-subset lines | eval -1.7..-2.0 | a cap of 0 cut the losers late; no comparison with the rule in hand |
| threat rerun on T3 | added `operator_opening_pump` | eval below T3 | the line separated coins only across the 10-07 habit change |
| curve scalp | TP / trail / ladder on T3 | +0.06..+0.12 on eval | curve scalping is not a main profit source here |
| tree (S0-S13) | 6ix-style tree, greedy per-cell search | A +3.13 / B +2.65 PICK vs T3 +3.05; first PICK half negative | search-based, row-weighted labels; engine gaps unexplained |
| fresh (current) | the method from W0 | W0-W1 done, W2-W4 to redo | section 6 |

---

## 6. The current run (`study-5ix-threat/fresh/`)

| Step | State |
| --- | --- |
| W0 | done. Kernel `kern.py` on the engine's rows with part sells and the AMM; parity on a trivial rule (buy 2 s, part sells 40 % at 10 s and 30 % at 20 s, all at 60 s, dump print): **98.1 % of 878 coins within 0.003**, kernel -4.18 vs engine -4.13. The engine buys on prints only. One coin's print and trade counts differ (1927 vs 1991). |
| W1 | done (section 3). |
| W2 | done (onset, per-coin lift vs the phase x age baseline, 20-coin window rule, best-of-shuffle). Facts: off the top in P1 (fall 38-58 %) the price keeps falling (-21 % in 10 s, death +47 points); near migration (P3) a quiet owner or leaving outsiders precede a rise (+14..+23 % in 10 s); outsiders under water precede a fall (-30..-47 %). Reaction to an outsider buy of our size (0.05 / 0.1 / 0.3 SOL): none that hurts, so the reaction cost is 0 at all three sizes. |
| W0 refresh | data to 10-21 (1269 coins, 120 migrated; newest week 10-15..10-21: 359 coins, 33 migrated). 10-22 on is the forward test. |
| W3 | peak pool cuts 18 / 40 / 60, rechecked on the new window: migration P0 9 %, P1 9 %, P2 24 %, P3 48 %; 18 sits on his step (sell chance 0.02 -> 0.19 at 22), 40 and 60 are a steady rise (C2 approved). |
| W4 | DEAD, FALL (fall from the life peak >= 20 %), IN (outsiders net >= 0.5 in 5 s), PUSH (his net >= 0.5 in 2 s), OUT (outsiders net <= -0.2 in 5 s), REST. IN and PUSH lose in every phase; P3 OUT / REST are calm (C2 approved). |
| W5 | restarts as the rule loop (method W5). A first pass (exits chosen on all coins, buys searched before the exits, the gate frozen early, the AMM exit carried from the older window) gave an entry "first print at peak pool >= 40" with three exits: +0.84 newest week, chosen on the same weeks; the gate lost 0.15-0.94 SOL a week under that entry. It is a reference, not a result. |
| W6-W9 | to do. |

---

## 7. 5ix mistakes (each now a rule in the method, "Mistakes")

| Mistake | Rule it gives |
| --- | --- |
| Lines true at the fill on most coins in a cell sold every coin at once | not true at the fill on most coins |
| A tag that excludes nothing (`@!dump` before the dump) | read what a tag holds at the decision |
| "gross" outsider read that is 94-96 % a buy | name a read for what it measures |
| A lifetime read after a late entry | lifetime = since launch, not since we bought |
| The creator's bag counted as outsider flow | the creator is in the owner tag |
| A 0-migrated cap | H is the user's number, not 0 |
| A line that only separated the habit change | regime first |
| Hindsight tops and bottoms as states (fresh W2, first pass) | live only |
| W2 tables per row, all rows, no baseline | onset, per coin, lift |
| Kernel bought on ticks; the engine buys on prints | read it from the parity check |
| The fingerprint's spend (1 SOL) taken as our buy size | our size is the rule's (0.05) |
| A greedy exit set scored on the data it was chosen on (launch buy +3.05 in-sample, +0.51 and -0.23 chosen on one week and tested on the other) | choose on one half, test on the other, and report that number |
| A `lake-export` by another run pruned the OLD days mid-study | replay before the lake moves; `.env` only with the user's yes |
| Backfilled AMM prints carry whole-second times and sort before the last curve print | check AMM print order at migration in parity |
| Gate found first on plain policies and frozen; buys searched before the exits; a whole cell taken as the entry; the AMM exit carried from an older window | the rule loop (method W5, L18) |

---

## 8. Open

- The streak gate: T, X, N, M from W5 step 3, judged with the exits under the reference entry, re-checked with the final entry (start point T 30 s, N 2, M 30 min, peak limit 30 SOL). Engine side: [streak-gate-plan.md](../../roadmap/streak-gate-plan.md), for the user's approval.
- Rule B's held share H: found in W6 on the frontier; the user may overrule.
- The lake moves daily: the W8 replay covers the days still in it (the user agreed).
- The W2 redo: onset, per-coin lift, cost, four checks, reaction cost at 0.05, 0.1 and 0.3 SOL.
