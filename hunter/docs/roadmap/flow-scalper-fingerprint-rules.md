# Flow-scalper fingerprint rules - testable rule set (2026-07-23)

Concrete, runnable rules for the dip-reversion scalper, partitioned by omego's
`ix_labels` fingerprint groups. Seed script:
[`hunter/scripts/seed-flow-scalper-rules.sql`](../../scripts/seed-flow-scalper-rules.sql).
Analysis these numbers come from:
[`flow-reversion-scalper.md`](flow-reversion-scalper.md) ("Token filtering" +
"Fingerprint-axis grouping"). Engine capabilities assumed are all **shipped** on
`strategy-redesign`: `m_price_window {trail, rise}`, `m_position {retrace, pnl, held}`,
multi-window-per-group, `reentry`.

**Safety.** The seed inserts every rule as `trade_mode='paper'`, `is_active=false`. The
engine loads `WHERE is_active AND is_enabled` (`rule_repo.rs`), so nothing fires until
you flip `is_active`. Params were validated against `RuleParams::parse` before commit.

## What this experiment answers

The fingerprint analysis found creation shape carries **no signal** for omego's token
pick once hotness is controlled for (chi2/df ~ 1.0 on every axis). That is a measurement
on *his* behaviour, not on *profitability* - it is still possible that some creation
shape produces tokens that this strategy trades better. This rule set tests exactly
that, and nothing else: **6 rules, one shared metric core, only the fingerprint differs.**

If all six converge to the same per-episode PnL, the fingerprint is confirmed irrelevant
and you run the broad control alone. If one group separates, that is a real edge the
static analysis could not see.

## The six rules

| rule | fingerprint | hot tokens / 42h | his mints | his entries |
| --- | --- | --- | --- | --- |
| `fs-ALL base` | broad control (any dev-buy < 1000 SOL) | 950 | 136 | 1,013 |
| `fs-IX1 base` | `CU:Limit, CU:Price, Create_v2, CreateIdempotent, BuyV2` | 167 | 47 | 273 |
| `fs-IX2 base` | `CU:Limit, CU:Price, Create_v2, CreateIdempotent, Buy, Transfer` | 191 | 24 | 187 |
| `fs-IX3 base` | `Transfer, Transfer, Create_v2, ExtendAccount, CreateIdempotent, BuyExactSolIn` | 101 | 15 | 168 |
| `fs-IX4 base` | `CU:Limit, CU:Price, Create_v2, CreateIdempotent, Buy, Transfer, Transfer` | 91 | 12 | 126 |
| `fs-IX5 base` | `Transfer, Transfer, Create_v2, CreateIdempotent, BuyExactQuoteInV2` | 43 | 7 | 88 |

A fingerprint with zero configured axes never matches
(`Fingerprint::has_any_criterion`), so the control is expressed as one bucket axis with
a 1000 SOL bucket width (`init_buy_lamports = 0`, `bucket_size_amount = 1000`) - it
admits every token that has a dev-buy at all (135 of his 136).

## Per-group calibration

Liquidity band and gross-flow floor are the **p25 / p90 of omego's own entries inside
that group**; everything else is identical across the six rules.

| rule | liquidity band | dip (`m_price_window(30).trail`) | `gross_flow(60)` floor | source: his median liq / gross60 |
| --- | --- | --- | --- | --- |
| ALL | 55 - 105 | >= 14 | >= 14 | 71.7 / 35.2 |
| IX1 | 50 - 90 | >= 12 | >= 8 | 62.1 / 15.9 |
| IX2 | 55 - 105 | >= 15 | >= 15 | 71.7 / 39.5 |
| IX3 | 55 - 105 | >= 15 | >= 16 | 71.5 / 38.3 |
| IX4 | 48 - 100 | >= 18 | >= 15 | 65.0 / 32.0 |
| IX5 | 58 - 95 | >= 13 | >= 12 | 68.8 / 24.2 |

IX1's tokens are genuinely thinner (liq med 62 vs 72, gross60 med 16 vs ~39) - that is
the only per-group difference the data supports, and it is why IX1 gets a looser gate.

## The shared metric core

```json
{
  "stop_loss": 25,
  "entry": {
    "m_snapshot": {
      "time":      [{"operator": ">=", "value": 150}],
      "liquidity": [{"operator": ">=", "value": 55}, {"operator": "<=", "value": 105}]
    },
    "m_price_window": { "window_size_sec": 30, "trail": [{"operator": ">=", "value": 14}] },
    "m_flow_window": [
      { "window_size_sec": 60, "gross_flow": [{"operator": ">=", "value": 14}] },
      { "window_size_sec": 2,  "net_flow":   [{"operator": ">=", "value": 0}]  }
    ]
  },
  "exit": {
    "m_position":       { "retrace": [{"operator": ">=", "value": 5}] },
    "m_price_lifetime": { "stall":   [{"operator": ">=", "value": 15}] }
  },
  "reentry": { "cooldown_sec": 5, "max_episodes_per_token": 12 }
}
```

Why each line, with its evidence:

| clause | value | why |
| --- | --- | --- |
| `time >= 150` | 150 s | his entry age p10 = 179 s; keeps launch-snipe noise out |
| `liquidity` band | per group | his entry liq p25 58 / p90 101 |
| `m_price_window(30).trail >= 14` | 14% | his entry dip vs 30 s high: p25 6.1 / **med 14.6** / p75 24.5 |
| `gross_flow(60) >= 14` | 14 SOL | his gross60 p25 = 14.5. The old blueprint's `>= 10` was measured **non-binding** |
| `net_flow(2) >= 0` | 0 | the single best lever in the Phase-3 probe (+0.61 / +0.51 SOL honest fee-only) |
| `m_position.retrace >= 5` | 5% | winners' trail retrace med 4.7%; r5 was the robust winner under `first` and `signal` fills |
| `m_price_lifetime.stall >= 15` | 15 s | measured to cut **losers**, not winners (dropping it: win% 44 -> 32) |
| `stop_loss: 25` | -25% | desugars to `m_position.pnl <= -25`; his loss p10 is -25% |
| `reentry` 5 s / 12 | | his re-entry gap p25 4 s / med 24 s; median 5 episodes/mint, max 31 |
| `max_concurrent_tokens: 4` | | his p90 concurrency is 4 (max 6) |
| `buy_amount_lamports` 1.0 SOL | | his entries are 0.43-1.34 SOL, med 0.87. Pct-of-vsol sizing is Phase 5, not built - use a fixed size and keep the liquidity band narrow so impact stays roughly constant |

## Variants to A/B (run these on the broad control only)

Change one thing at a time against `fs-ALL base`.

**V1 - near-ATH adoption gate.** Add to `entry`:
```json
"m_price_lifetime": { "trail": [{"operator": "<=", "value": 25}] }
```
His *first* buy on a token lands at only -15% off the lifetime ATH (p75 -8%), but across
all entries the median is -35.6% because re-entries ride the token down. So this gate
captures roughly the top third of his entries. Expect **fewer, better** episodes -
it is the cheapest available proxy for "the token is still in its run". Highest-value
variant to test.

**V2 - no exhaustion gate.** Drop the `window_size_sec: 2` clause, leaving a single
`m_flow_window` object. Tests whether the 2 s net-flow floor still earns its recall cost
once re-entry is on (it was measured on the one-shot variant only).

**V3 - wider trail + time stop.** `retrace >= 8`, add `"held": [{"operator": ">=", "value": 90}]`
to `m_position`, `reentry: {cooldown_sec: 30, max_episodes_per_token: 6}`. Tests the
"our holds are too short vs omego's 17 s median" hypothesis with an explicit time bound
rather than relying on the stall net.

## How to run

**Simulation - fingerprint groups.** The lab's simulate/sweep reads rules from the DB and
the real per-token fingerprint from the lake (`K_FP_*` columns, `duck.rs::load_fingerprints`),
so this is the only path that can actually distinguish the six rules.

```powershell
psql $env:DATABASE_URL -f hunter/scripts/seed-flow-scalper-rules.sql
cargo run -p hunter-lab            # :8140, then use the lab UI simulate / grouped sweep
```

**Simulation - metric variants.** The existing Rust harness is faster for V1-V3 because
it pins every token to one synthetic fingerprint (`uniform_tf()`), which is exactly what
you want when the fingerprint is *not* the variable:

```powershell
cargo test -p hunter-lab --test flow_scalper_validation -- --ignored --nocapture
```
(add the variant as a new `#[test]` next to `gated_combined`; set `FLOW_SCALPER_LAKE`
if your lake is not at `hunter/lake-data`.)

**Live paper.**
```sql
UPDATE strategy_rules SET is_active = true WHERE rule_name LIKE 'fs-%';
-- to stop:
UPDATE strategy_rules SET is_active = false WHERE rule_name LIKE 'fs-%';
```
All six run concurrently at 4 concurrent tokens each. Watch the run navigator; the
per-rule PnL trend is the comparison.

**Before real money:** re-validate under the `first`-print fill model, not `worst` - the
whole Phase-3 edge is conditional on fill quality (+0.61 SOL under `first`, -0.50 under
`worst`). Then start at 0.1 SOL (`buy_amount_lamports = 100000000`).

## What to measure, and the decision gate

Per rule, from the run summary: episodes, win% before costs, median episode PnL%,
median hold, realized fee-only PnL (**not** `realA` - it double-counts slippage).

- **Fingerprint verdict:** if the six per-episode PnL medians sit inside each other's
  noise, the fingerprint is dead - keep `fs-ALL` and delete the rest. Given the static
  analysis this is the expected outcome; the point is to confirm it cheaply.
- **Power:** IX5 sees ~43 hot tokens per 42 h, IX4 ~91. Those two will not have a usable
  sample for days. Judge ALL / IX1 / IX2 / IX3 first.
- **Kill gate:** any rule whose fee-only realized PnL is negative over >= 100 episodes
  under a `first` fill is dead, not undertuned.

## Known limits of this rule set

1. **No `unique_wallets` metric.** The strongest discriminator found (66 vs 3 unique
   wallets per 60 s; 60 s price range 59.5% vs 2.6%) cannot be expressed today.
   `gross_flow` is the only available proxy and it is weak: raising it from 20 to 60 SOL
   moves precision 8.23% -> 8.56% while recall falls 49.7% -> 18.2%. Expect roughly a
   5-6x lift over the base rate from these gates, where a wallet-count gate showed far
   more. This is the top engine gap.
2. **`rise` is not a volatility proxy at entry.** Measured: his `rise(60)` median is only
   10.5 (p25 1.2) because at entry the price sits *at* the window low. `trail` alone
   carries the range. Do not add a `rise >=` gate.
3. **No pct-of-vsol sizing** (Phase 5). Fixed 1.0 SOL inside a 50-point liquidity band
   means impact varies about 2x across the band.
4. **One 42 h window, one wallet** behind the calibration. Re-derive the percentiles if
   you extend the history via the EC2 sync.
