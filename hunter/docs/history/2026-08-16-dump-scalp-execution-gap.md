# 2026-08-16 — Dump-scalp family: the loss is execution, not thresholds

Research journal for a session that started from "64hP earns consistently, none of my
rules do" and ended with the gap measured. Read this before touching the `dems` rules or
re-litigating whether a scalp threshold can be tuned into profit.

Prior context: a Cursor session (`c:\Users\User\Music\cursor_trader_logic_analysis.md`)
produced the 64hP wallet portrait and the DEMS thesis. This session tested it.

## The thesis under test

64hP's observed habit, as read off his fills: he buys **into** a violent dump on a live
pump.fun token and exits within seconds. Cursor's reading was that the edge is the
knife-catch itself, and it removed a bounce gate as "anti-64hP".

The question this session answered: does that thesis survive when *we* trade it, given we
see the dump on a confirmed feed rather than from inside its slot?

## What was built

Four rules in the local DB (`hunter_bot` on localhost:5555), all `trade_mode=paper`,
`is_active=false`, 0.05 SOL, concurrency 3, fingerprint
`793c5b87-b33a-4c28-9147-7bef8a45e9f7` (`init=0 · bkt=1000`), tag `dems`:

| id | name | entry idea |
| --- | --- | --- |
| `dc2e2c49` | DEMS-B fresh knife | baseline + `trail(2) >= 10` — buy mid-fall |
| `824916d9` | DEMS-C exhaustion | `trail(2) <= 3` + `net_flow(2) >= 0` — buy after the fall stops |
| `b5128688` | DEMS-D exhaustion no-heat | C minus the `gross_flow(60) >= 55` gate |
| `d240753d` | DEMS-E deep-pool | D with `liquidity` 50–85 instead of 30–50 |

Baseline A is the pre-existing `FlowScalper` (`3abf7578`), left untouched.

DEMS-D's entry, the shape everything else varies from:

```json
"entry": {
  "m_snapshot":     { "time": [{">=", 30}], "liquidity": [{">", 30}, {"<", 50}] },
  "m_flow_window":  { "net_flow": [{">=", 0}], "window_size_sec": 2.0 },
  "m_price_window": [
    { "trail": [{">=", 25}], "window_size_sec": 30.0 },
    { "trail": [{"<=",  3}], "window_size_sec":  2.0 }
  ]
}
```

Exit, shared by every variant: `m_position` `pnl <= -8`, `held >= 45`, `retrace > 2` with
`arm_above_pct: 5`; plus `m_flow_window(30).gross_flow <= 3` as a death-close.

## Results — 7 simulate runs, Aug 1–13 lake, 298,429 matched tokens

Authoritative pricing is `worst_case` fill + `pumpfun_impact` cost. That is what live
paper books.

| run | pricing | entered | win | total SOL | mean/trade | median | PF |
| --- | --- | --- | --- | --- | --- | --- | --- |
| A baseline `trail(30)>=25` | worst | 5,978 | 32.2 % | −35.93 | −12.02 % | — | 0.248 |
| B fresh knife | worst | 5,326 | 30.6 % | −35.05 | −13.16 % | — | 0.219 |
| C exhaustion | worst | 4,747 | 35.4 % | −18.85 | −7.94 % | — | 0.355 |
| **D exhaustion no-heat** | worst | 5,872 | 38.1 % | −21.44 | **−7.30 %** | — | 0.393 |
| F = D repriced | first + fee-only | 5,872 | 52.6 % | −1.10 | **−0.37 %** | +0.97 % | 0.947 |
| **E deep pool** | worst | 1,902 | 38.7 % | −6.90 | **−7.25 %** | −11.24 % | 0.347 |
| E repriced | first + fee-only | 1,902 | 52.1 % | −1.33 | **−1.40 %** | +0.88 % | 0.796 |

Every variant is negative on **13 of 13 days**. Median hold on E is 5 s.

## The finding

**The same taken set, repriced two ways, is the whole story.**

| rule | worst fill + impact | first fill + fee only | gap |
| --- | --- | --- | --- |
| D (n=5,872) | −7.30 %/trade | −0.37 %/trade | **6.93 pp** |
| E (n=1,902) | −7.25 %/trade | −1.40 %/trade | **5.85 pp** |

The signal is near-breakeven. Execution costs ~6 pp per round trip and that is the entire
loss. It holds across the liquidity range, so it is not a pool-depth artifact.

**The bar it sets:** to clear zero at worst-fill pricing, a rule in this family needs an
optimistic-pricing mean above **~+6 %/trade**. Nothing tested is within 6 pp. A strategy
targeting 3–12 % per round trip cannot clear a 6 pp execution cost — that is a ratio, and
no threshold in the rule editor changes a ratio.

### Supporting results

- **Knife-catching is refuted for a late participant.** Adding `trail(2) >= 10` made every
  metric worse (win 30.6 %, PF 0.219, median hold 2 s). Being *more* 64hP-like is strictly
  worse when you are not in his slot.
- **Exhaustion roughly halves the loss.** The bounce gate Cursor deleted as "anti-64hP" is
  directionally right *for us*, precisely because we are late.
- **A gate the dump itself creates is a lagging gate.** Dropping `gross_flow(60) >= 55`
  improved quality *and* raised volume (5,872 vs 4,747) — it was selecting post-move
  moments, and it was the AND-binding clause deciding entry timing.
- **Deep pools are a wash, not a fix.** liq 30–50 → 50–85 bought ~1 pp of execution and
  gave back ~1 pp of signal. Net mean identical, entries 3× fewer.
- **The stop does not stop.** `pnl <= -8` realized an average loss of **−19.4 %** (worst
  −102 %), derived from PF and win rate: prints are sparse and price gaps straight past
  the level. Avg win +12.4 % against avg loss −19.4 % never closes. Downside on this token
  class is not controllable by a price stop.

### Level vs edge, resolved

`trail(30s) >= 25` is a **level**, true across a long stretch — it fires equally at the
dump's first print, 9 s later, and 20 s later. It is not "fire on the dump tx".

But **no engine change is needed** to express the edge. A short-window `trail` already is
an edge detector (`trail(2)` reads 26–34 during a fall, 1–4 after it — see
[../../engine/src/metrics/price_window.rs](../../engine/src/metrics/price_window.rs),
whose monotonic deques also already carry peak timestamps), and multi-window arrays parse
for `m_price_window` / `m_flow_window`, validated on rule create. The level-vs-edge
problem was expressible all along; it just was not the binding constraint.

## Open fork — needs a user decision

Tuning this family further is not worth compute. Three directions:

1. **Change the target size** — a thesis holding for 30–100 %+ moves, where 6 pp is
   affordable overhead. Fewer trades, longer holds. This is what the rule-search / habit
   machinery in [../roadmap/rule-search-habit.md](../roadmap/rule-search-habit.md) already
   points at, and the shape the promoted g4/g8/g12 rules live in.
2. **Attack the 6 pp directly** — sub-slot visibility, tips, feed latency. Currently ruled
   out by the latency plan; reopening it is a real decision, not a rule change.
3. **Accept a lower bound** — reality sits between `first` and `worst`. At the midpoint
   (~3.5 pp) a very selective version might be marginal. Not recommended: live paper books
   `worst`.

Direction (1) trades "small consistent daily profit" for larger, lumpier wins. That
trade-off is the thing to confirm before spending on it.

## Reproduction

Lab API on `:8140`, bearer token from `hunter/.env` `API_AUTH_TOKEN`. Driver scripts and
raw result JSON live in the session scratchpad (`run-stage1.ps1`, `run-F.ps1`,
`run-E.ps1`, `result-*.json`, `timesummary-*.json`).

```
POST /api/strategies/simulate   {rule_id, since, until, fill_model, cost_model}
POST /api/strategies/simulate/{id}/result/summary
POST /api/strategies/simulate/{id}/result/time-summary
```

Bounds used: `since=2026-08-01T00:00:00Z`, `until=2026-08-14T00:00:00Z`.

### Operational gotchas hit along the way

- **`until` must not exceed the lake's newest sealed day** (2026-08-14 at the time). Past
  it the log warns and tokens' tails are truncated.
- **Run sims strictly sequentially.** Concurrent runs starve DuckDB —
  `duckdb_memory_limit_mb=512 usable_mb=0` in the log, then failures.
- **A wedged sim poisons the shared DuckDB spill dir** and the next run dies with
  `Could not read file ... duckdb_temp_storage_*.tmp: Reached the end of the file`.
  Restarting hunter-lab clears it.
- **A sim inside the corpus-load phase cannot be cancelled.** `POST .../cancel` returns
  `{"cancelling":true}` and has no effect; only killing the process works. Load has no
  cancel check — a real weakness, out of scope here.
- DB column names: `is_enabled` (not `enabled`); `fingerprints` uses typed axis columns
  (`init_buy_lamports`, `bucket_size_amount`, `ix_labels`), not a `pattern` column.
- `/result/summary` returns 408 while a run is still settling; retry with a long timeout.
