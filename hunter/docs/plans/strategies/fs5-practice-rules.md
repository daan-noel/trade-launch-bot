# fs5 practice rules — local DB mining (2026-07-29)

Starter ladder for **paper** trading: one validated fingerprint axis per family,
metric gates, and **`scale_out`** partial exits. Seeded by
`_local/rule-research/scripts/seed-fs5-practice-rules.sql`.

## What the local DB actually said

Corpus: PG `trades` **2026-07-22 18:47 -> 2026-07-28 14:09 UTC** (~7.9M rows),
120k+ tokens. Wallet episode tables `s63c_*` (63ot) were reused and match
[wallet-analysis.md](wallet-analysis.md) headline counts (1,090 closed eps, 65.1% win,
+14.58 SOL net of 125 bps/leg).

### Fingerprint axes (token creation shape)

| Axis | Verdict for 63ot-style edge |
| --- | --- |
| **`init_buy_lamports` bucket [0, 6.4) SOL** | **Keep.** 863 eps / +16.97 SOL vs 216 eps / **-3.17 SOL** in [6.4, 25.6). Reproduced twice from `s63c_full`. |
| `cu_limit`, `cu_price` (exact) | No stable screen; high `cu_limit` tail weak but n≈46. |
| `first_slot_buy` / `first_slot_sell` | Not on local `tokens` table (ingest dim only); not used. |
| `ix_labels` / creation ix type | `BuyExactQuoteInV2` net **-0.59 SOL** on 60 eps inside [0,6.4) — real, but **not expressible** as a fingerprint without enumerating full label sequences (wallet-analysis re-narrowing section). |

**Only fingerprint wired into fs5:** `fs4-dev small [0-6.4)` (`init_buy_lamports=0`,
`bucket_size_amount=6.4`). Second family: `fs3-dev big [12.8-25.6)` (64hP/dev13
simulate-validated in wallet-analysis).

### Metric gates (when to enter)

On **63ot's own** entries (`s63c_full` + last trade before `entry_time`):

| vsol band at entry | n | win% | avg %/ep | total SOL |
| --- | --- | --- | --- | --- |
| **< 55** | 163 | 63.2% | **+7.37%** | +6.20 |
| 55-85 (fs4-00 band) | 516 | 68.6% | +3.28% | +9.24 |
| > 85 | 182 | 63.7% | +0.49% | +0.47 |

So **`fs4-00`'s liquidity 55-85 was too tight on the low side** — it drops the
highest *average*%-per-episode bucket 63ot actually uses. **fs5-00 uses 40-85** instead.

Dip depth on his book (already in wallet-analysis): money concentrates in **deeper**
30s dips (<= -35% best avg%/ep). **fs5-01** raises `trail` to **22%** as a one-knob
variant.

Do **not** add `m_flow_window(2).net_flow >= 0` on 63ot-shaped rules — he buys into
negative 2s flow ~56% of the time.

### Partial exits (`scale_out`)

Engine support is shipped ([`partial-exits-plan.md`](partial-exits.md)).
Motivation from simulating **`fs4-00`** (same window, `pumpfun_impact`, `worst` fill):
~**11%** of closed episodes contributed ~**77%** of loss via cascade fills past -50%
(wallet-analysis "`63ot` - simulating the RULE itself"). Banking most of the bag at
**+17%** (his measured winner median) leaves a **15% remainder** with armed trail +
`stop_loss` on the stub — same economic idea as omego's scalp/runner split, but
authorable in JSON.

## Rule ladder (all `paper`, `is_active=false`)

| Rule | Fingerprint | Buy | Partial exit | Notes |
| --- | --- | --- | --- | --- |
| **fs5-00 63ot scale bank** | fs4-dev small | 0.5 SOL | 85% @ TP17; remainder trail 6% / arm 2% / held 90; cold-flow exit | **Start here** — base geometry + scale_out |
| fs5-01 63ot deep dip 22 | same | 0.5 | same ladder | Dip gate 22% vs 15% |
| fs5-02 63ot small tail-safe | same | **0.3** | 90% @ TP15; remainder time 45s; SL **22** | Caps cascade notional |
| fs5-03 63ot scale one-shot | same | 0.5 | same as fs5-00 | No `reentry` |
| fs5-04 dev13 scale trail | fs3-dev big | 0.1 | 70% @ TP25; remainder 64hP trail | Different token universe |

Global **`stop_loss`** on fs5-00/01/03 remains **28** for the unbanked tranche until
scale stages fire.

## How to validate before real money

1. `psql "$DATABASE_URL" -f hunter/scripts/seed-fs5-practice-rules.sql`
2. Export lake: `cargo run -p hunter-lab -- lake-export` (needs `SWEEP_LAKE_DIR`).
3. Lab simulate each rule (`POST /api/strategies/simulate`) with **`pumpfun_impact`**
   and both **`worst`** and **`first`** fill — the tail split in wallet-analysis is
   sensitive to fill model.
4. Promote knobs only if profit factor > 1 on **`worst`**; treat **`first`** as an
   upper bound.

No simulate numbers are recorded here yet — local **`SWEEP_LAKE_DIR`** had no parquet
in-repo at seed time; re-run after export.

## Open questions for the operator

- **Bankroll / max exposure:** fs5-00 assumes ~1 SOL peak (0.5 x conc 2). Tight
  2-3 SOL bankroll → prefer **fs5-02** or lower `max_concurrent_tokens`.
- **Target wallet:** fs5-00..03 track **63ot**; fs5-04 tracks **64hP/dev13**. Say if
  you want a ladder anchored on a different tracked wallet.
- **`BuyExactQuoteInV2` screen:** needs a new fingerprint axis (e.g. creation ix type),
  not more SQL narrowing — only pursue if you want an engine change.
