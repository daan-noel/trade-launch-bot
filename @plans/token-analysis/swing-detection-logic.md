# Swing Detection Logic — Implementation Reference

> Concise, code-accurate spec of how this project detects price **swings** in a token's
> trade history. Source of truth = the Rust implementation, not the older TA-terms spec.
> Reuse this file as a prompt to re-implement or extend the feature.
>
> This file documents **two layers** that share the same leg detector:
>
> 1. **§1–§11 — the leg detector** (original TA-style swing analyzer): the
>    4-phase reversal scan + pair-based quality filter, surfaced by the lab swing
>    chart/table endpoints.
> 2. **§12 — swing1** (the last-updated logic — base everything on this): the
>    causal kill→volume phase classifier + entry + exit ladder that consumes the
>    detector's **raw** ledger and drives the backtest sweep and live trading.
>    swing1 is the reason the detector moved into `trading_core` and grew the
>    `dust_frac` axis.
>
> **Key files — leg detector (§1–§11)**
>
> - Backend algorithm: `trading_core/src/strategies/swing_1/swing.rs`
>   (moved here from `lab/src/analyzers/swing_analyzer.rs`, which is now only a
>   re-export shim; generic over `TradeRow` so lab/sweep/live share one scan)
> - Lab API handlers: `lab/src/api/handlers/tokens/swing.rs` (single + batch)
> - Params UI / form coercion: [`frontend-react/src/lab/components/analysis/swingParams.tsx`](../../frontend-react/src/lab/components/analysis/swingParams.tsx)
> - Post-detection visibility filter (client only): [`frontend-react/src/lab/components/analysis/swingFilter.ts`](../../frontend-react/src/lab/components/analysis/swingFilter.ts)
> - Chain-of-swings grouping (client only): [`frontend-react/src/lab/components/analysis/swingChains.ts`](../../frontend-react/src/lab/components/analysis/swingChains.ts)
> - Chart overlay rendering: [`frontend-react/src/shared/components/token-price-chart/swingOverlay.ts`](../../frontend-react/src/shared/components/token-price-chart/swingOverlay.ts)
> - Page wiring: [`frontend-react/src/lab/pages/analysis/SwingDetectionPage.tsx`](../../frontend-react/src/lab/pages/analysis/SwingDetectionPage.tsx)
>
> **Key files — swing1 (§12)**
>
> - Domain root + rule→params builders: `trading_core/src/strategies/swing_1/mod.rs`
> - Raw-ledger scan entry: `swing::detect_swing_legs_raw` (same file as the detector)
> - Phase classifier (kill/volume lows, kill→volume latch): `trading_core/src/strategies/swing_1/classifier.rs`
> - Entry (higher-low confirm, spot-priced worst-case fill): `trading_core/src/strategies/swing_1/entry/mod.rs`
> - Exit (NextKill ladder): `trading_core/src/strategies/swing_1/exit/mod.rs`
> - Backtest sweep strategy: `lab/src/sweep/strategies/swing1.rs`
> - Per-token detect funnel handler: `lab/src/api/handlers/tokens/swing1_detect.rs`
> - CLI diagnostic twin: `lab/src/swing_probe.rs` (`lab swing-probe` / `swing-census`)
> - Per-token detect page + client: [`frontend-react/src/lab/pages/analysis/Swing1DetectPage.tsx`](../../frontend-react/src/lab/pages/analysis/Swing1DetectPage.tsx) · [`frontend-react/src/lab/services/swing1Detect.ts`](../../frontend-react/src/lab/services/swing1Detect.ts)

---

## 1. What it does

Scans a token's trades in time order and segments them into **strictly alternating legs**:

- **Swing High** — a buy-dominant leg (price/MC pushing up).
- **Swing Low** — a sell-dominant leg (price/MC pushing down).

A leg ends (a **reversal** is confirmed) only when accumulated opposite-side SOL flow
crosses a configurable **reversal threshold**. Small opposite-side pokes that don't reach
the threshold are absorbed back into the current leg (a **merge-back**), so noise doesn't
fragment legs. All volume math is in **SOL**, never token amount.

Output is a flat, time-ordered ledger: `swing_high, swing_low, swing_high, swing_low, …`
(possibly with a leading lone low or trailing lone high).

---

## 2. End-to-end data flow

```
Frontend (SwingDetectionPage)
  ├─ Single token:  POST /api/tokens/:mint/swings        body = { params, window_start_ms?, window_end_ms?, curve_only? } ({} = defaults)
  └─ All filtered:  POST /api/tokens/swings/batch         body = { mints[], params, window_start_ms?, window_end_ms?, curve_only? }
        │
        ▼
Backend handler  →  load trades (in-memory token_cache first, else TradeRepo from DB)
        │   (batch: cache-miss DB loads fan out via buffer_unordered(16); the
        │    detect_swings CPU pass runs on web::block, off the HTTP worker)
        ▼
detect_swings(trades, params)
   1. sanitize_and_order(trades)   → Vec<Tx>      (filter sol<=0, canonical sort, derive prices)
   2. scan(txs, params)            → Vec<LegAcc>  (phase machine → strictly-alternating ledger)
   3. apply_quality_filter(...)    → Vec<SwingLeg> (pair-based discard of weak legs)
        │
        ▼
JSON response → Redux (swingResult / swingAllResults) → chart overlay + results table.
```

Client-side, **two more passes** run on the returned ledger (they never re-hit the backend):

- **Visibility filter** (`swingFilter.ts`) — narrows which legs are *displayed*.
- **Chain stats** (`swingChains.ts`) — groups high→low pairs into "chains" for the table columns
  and the longest-chain chart highlight.

---

## 3. Input: transaction sanitization & ordering

Each DB `Trade` is mapped to an internal `Tx`. Before scanning:

- **Skip** any trade with `sol_amount <= 0`.
- **Dynamic dust floor** (`SwingParams.dust_frac`, swing1's `p_dust_frac` rule axis;
  `0` = off): applied *inside* `scan`, not here — "dust" is defined **relative to the
  active leg's largest same-side trade** (`sol_amount < dust_frac * active_leg.max_sol`),
  and that running max only exists during the scan. A trade judged dust is skipped
  entirely: it doesn't apply to the leg, doesn't trigger a reversal, and doesn't merge
  back. Because the floor scales with the leg's own activity, the **same `dust_frac`
  works on a 200-SOL token and a 2-SOL token** — no per-token SOL amount to guess
  (that was the whole point: a fixed absolute floor can't be right across tokens).
  Each leg's first trade seeds `max_sol`, so it's never itself dust. This keeps a leg's
  endpoints anchored to real trades — a low ends at the last real SELL before a
  dust-only breakage, the next high starts at the first real BUY after it — and stops
  a stray dust trade from opening a spurious bridge leg across the gap. (Presentation
  pairs with the chart's `gapBreakMs` overlay break, which stops a connected line from
  drawing a diagonal across the silent time gap between those two legs.)
- **Sort by the one canonical trade order** — identical to the DB `ORDER BY`, the
  Parquet lake, the export, and the frontend chart, so a leg detected here is built
  on the exact same trade sequence the candles are:

  ```
  slot       ASC      ← primary
  tx_index   ASC      ← real intra-block position of the tx (authoritative)
  leg_index  ASC      ← leg order within a single tx
  block_time ASC      ← last-resort tie-break only (second-precision)
  ```

  `tx_index` is the real on-chain intra-block index ([`TradeRow::tx_index`]).
  Measured against the curve reserve chain across the whole DB, `tx_index ASC`
  reproduces true same-slot execution order for **~99% of pairs** (and is fully
  correct for ~95% of tokens), so it's the authoritative intra-slot key — **no
  reserve-chain reconstruction at read time**. `duration_ms` is still
  `end_at - start_at` in `block_time` ms.

  The small residue of mis-ordered rows traces to the **RPC-backfill proxy**:
  `stamp_proxy_tx_index` (live `token_sync.rs`) used to stamp the per-slot counter in
  gTFA page order, which returns a slot's txs *newest-first* → `tx_index` came out
  reversed for proxy-stamped tokens. That stamp is now **reversed within each slot**
  so `tx_index ASC` = chain order; tokens backfilled before the fix need a re-sync to
  pick it up. The fix is at the **source**, not a read-time workaround.

  Slim rows that don't carry `tx_index` (`SweepTrade`/`CachedTrade`) default
  [`TradeRow::tx_index`] to `0`; they're built from an already-ordered DB slice and
  never re-sorted, so they pay no per-row RAM for a field they don't need.

### Price derivation (important — two distinct prices)

For each `Tx` two prices are computed and one carried-over value:

| Field             | Meaning                                                                                  |
|-------------------|------------------------------------------------------------------------------------------|
| `execution_price` | `sol_amount / token_amount` for that trade.                                              |
| `price` (post-spot) | Curve spot **after** the trade = `virtual_sol / virtual_token`; falls back to `execution_price` if reserves are absent. |
| `pre_spot`        | Curve spot **just before** this trade = the **previous** trade's post-spot. First trade has no prior state → falls back to its own post-spot. |

A leg's **`start_price` = the opening trade's `pre_spot`**; its **`end_price` = the last
same-side trade's post-`price`**. (So `start_price` is the market right *before* the leg
began, not the opener's own execution price.)

---

## 4. Parameters (`SwingParams`) — 21 fields

Sent as JSON. Defaults applied per-field via serde; `{}` ⇒ all defaults. The fields
split into four groups: **reversal thresholds**, **leg quality filters**, **per-leg-type
magnitude bounds**, and the **big-tx threshold**.

### 4a. Reversal thresholds (govern when a leg ends)

| Param                        | Default | Type | Meaning |
|------------------------------|--------:|------|---------|
| `high_to_low_threshold_sol`  | `5.0`   | f64  | Absolute SOL needed to confirm a High→Low reversal. `0` = no bound. |
| `high_to_low_threshold_pct`  | `50.0`  | f64  | Percent (0–100) of the current high's `|net_flow|` to confirm High→Low. `0` = no bound. |
| `low_to_high_threshold_sol`  | `5.0`   | f64  | Absolute SOL to confirm a Low→High reversal. `0` = no bound. |
| `low_to_high_threshold_pct`  | `50.0`  | f64  | Percent of the current low's `|net_flow|` to confirm Low→High. `0` = no bound. |

### 4b. Leg quality filters (post-detection, magnitude-based — §6)

| Param                        | Default | Type | Meaning |
|------------------------------|--------:|------|---------|
| `min_leg_trades`             | `2`     | u32  | Leg fails if `trade_count <` this. |
| `min_leg_duration_ms`        | `0`     | i64  | Leg fails if `duration_ms <` this. |
| `min_leg_volume`             | `0.0`   | f64  | Leg fails if `inflow+outflow <` this. |
| `min_leg_net_flow`           | `0.0`   | f64  | Leg fails if `|net_flow| <` this. |
| `max_leg_trades`             | `0`     | u32  | If `>0`: leg fails if `trade_count >` this. |
| `max_leg_duration_ms`        | `0`     | i64  | If `>0`: leg fails if `duration_ms >` this. |
| `max_leg_volume`             | `0.0`   | f64  | If `>0`: leg fails if `inflow+outflow >` this. |
| `max_leg_net_flow`           | `0.0`   | f64  | If `>0`: leg fails if `|net_flow| >` this. |

### 4c. Per-leg-type magnitude bounds (compared by absolute value — §6)

Swing-low legs have **negative** delta % and net flow, so all bounds are compared by
**magnitude** (`abs()`) — a swing low uses the same positive threshold as a swing high.
`0` = no bound on every field.

- **Delta % magnitude** = `|(end_price − start_price) / start_price × 100|`.
- **Net flow per second** = `|net_flow / (duration_ms/1000)|`; **skipped** for 0-duration legs (rate undefined).

| Param                                | Default | Type | Meaning |
|--------------------------------------|--------:|------|---------|
| `swing_high_min_delta_pct`           | `0.0`   | f64  | High fails if delta-% magnitude `<` this. |
| `swing_high_max_delta_pct`           | `0.0`   | f64  | High fails if delta-% magnitude `>` this (when `>0`). |
| `swing_high_min_net_flow_per_sec`    | `0.0`   | f64  | High fails if net-flow/s magnitude `<` this. |
| `swing_high_max_net_flow_per_sec`    | `0.0`   | f64  | High fails if net-flow/s magnitude `>` this (when `>0`). |
| `swing_low_min_delta_pct`            | `0.0`   | f64  | Low fails if delta-% magnitude `<` this. |
| `swing_low_max_delta_pct`            | `0.0`   | f64  | Low fails if delta-% magnitude `>` this (when `>0`). |
| `swing_low_min_net_flow_per_sec`     | `0.0`   | f64  | Low fails if net-flow/s magnitude `<` this. |
| `swing_low_max_net_flow_per_sec`     | `0.0`   | f64  | Low fails if net-flow/s magnitude `>` this (when `>0`). |

### 4d. Big-transaction threshold

| Param        | Default | Type | Meaning |
|--------------|--------:|------|---------|
| `big_tx_sol` | `0.0`   | f64  | `0` = disabled. When `>0`, a single tx with `sol_amount >= big_tx_sol` does two things: **(a)** it confirms a reversal *immediately* on that tx, bypassing the net-flow reversal threshold; and **(b)** it anchors the leg's **terminal pivot** (`pivot_end_*`) to the LAST such same-side big tx — the real pump/dump point — instead of the chronologically last (possibly dust) trade. |

(Frontend default mirror lives in `DEFAULT_SWING_PARAMS`. Empty form fields coerce to `0`.)

### Reversal threshold rule (the crux)

When a temp opposite leg opens, a single `frozen_threshold` is computed once and frozen:

```rust
fn reversal_threshold(sol, pct, prev_leg_net_flow_abs) -> f64 {
    sol_bound = sol > 0 ? sol : +INF        // 0 ⇒ no bound ⇒ drops out of min
    pct_bound = pct > 0 ? (pct/100)*prev_leg_net_flow_abs : +INF
    return min(sol_bound, pct_bound)        // ← MIN of the two active bounds
}
```

- Basis = the **current (active) leg's `net_flow`**, snapshotted (abs) at the instant the
  temp leg opens. A swing low's `net_flow` is negative → `abs()` makes the basis positive.
- **`min`** is used for **both** directions. The smaller of the two active bounds wins, so a
  reversal triggers as soon as *either* bound is met (more sensitive).
- `0` on a term ⇒ `+∞` ⇒ that term is ignored. **Both `0` ⇒ `+∞` ⇒ no reversal can ever
  fire** (the whole history becomes one leg).
- **Re-snapshot every time a temp leg opens.** After a sub-threshold merge-back shrinks the
  active leg's `net_flow`, the next temp leg snapshots the *reduced* value (smaller basis on
  the pct term). This is intentional.

> ⚠️ **Delta vs. the old TA spec:** that spec claims Low→High uses `max(...)`. The shipped
> code uses `min` for both directions. Trust the code.

---

## 5. The phase machine (`scan`)

Four phases; the temp phases are the "is this a real reversal?" probation states:

```
enum Phase { SwingHigh, TempSwingLow, SwingLow, TempSwingHigh }
```

**Single-counting rule (foundational):** every sanitized tx is applied to exactly one leg,
exactly once. "Seed a leg from this tx" fully consumes it (sets start/end/price, seeds the
side amount, `trade_count = 1`); later same-side txs only extend it.

**Initialization** — from the first tx: BUY ⇒ seed `current_high`, phase `SwingHigh`.
SELL ⇒ seed `current_low`, phase `SwingLow`.

**Big-tx override (`is_big`):** a single tx with `sol_amount >= big_tx_sol` (and
`big_tx_sol > 0`) forces a confirm on its own, OR-ed into every confirm check below
(`temp.outflow >= threshold || is_big(tx)`). When `big_tx_sol = 0` the override is dead
and only the accumulated net-flow threshold governs.

### SwingHigh

- **BUY** → `apply_buy`: `inflow += sol`, `end_at/end_price = tx`, `trade_count++`,
  `consider_pivot` (advance the leg's max-spot extreme; record this tx if it's a big tx).
- **SELL** → open `temp_low` seeded from this SELL (`outflow = sol`, counts immediately),
  freeze `high→low` threshold off `current_high.net_flow`, phase → `TempSwingLow`.
  *Then immediately check:* if `temp.outflow >= threshold` **or** `is_big(tx)`, the reversal
  confirms on this very SELL (push `current_high` to ledger, `temp` becomes `current_low`,
  phase → `SwingLow`).

### TempSwingLow

- **SELL** → `temp.outflow += sol`. If `>= threshold` **or** `is_big(tx)` ⇒ **confirm**:
  push `current_high`, `temp` → `current_low`, phase → `SwingLow`.
- **BUY** (threshold not reached) → **merge-back**: fold temp's SELLs into the high
  (`high.outflow += temp.outflow`, `high.trade_count += temp.trade_count`; `end_at/end_price`
  untouched — they track the last BUY), phase → `SwingHigh`, then `apply_buy(tx)` once.

### SwingLow (mirror of SwingHigh)

- **SELL** → `apply_sell`: `outflow += sol`, `end_at/end_price = tx`, `trade_count++`.
- **BUY** → open `temp_high` (`inflow = sol`, counts immediately), freeze `low→high`
  threshold off `current_low.net_flow` (abs), phase → `TempSwingHigh`; immediate-confirm check.

### TempSwingHigh (mirror of TempSwingLow)

- **BUY** → `temp.inflow += sol`. If `>= threshold` ⇒ **confirm**: push `current_low`,
  `temp` → `current_high`, phase → `SwingHigh`.
- **SELL** → **merge-back** into the low (`low.inflow += temp.inflow`, `trade_count +=`),
  phase → `SwingLow`, then `apply_sell(tx)` once.

### End of history (flush)

Unlike the old spec (which discarded trailing legs), the implementation **keeps** them:

- `SwingHigh` / `SwingLow`: push the active leg.
- `TempSwingLow` / `TempSwingHigh`: push the active leg **and** the open temp leg.

So the raw ledger can end with an unconfirmed/temp leg. Net flow is recomputed at finalize:
`net_flow = inflow - outflow` (positive for highs, negative for lows). `duration_ms =
end_at - start_at`.

### Terminal pivot (`pivot_end_*`) — charting anchor, never affects stats

Each `LegAcc` tracks two extra cursors that **do not** touch any stat or filter:

- `extreme_*` — the leg's price extreme: **max** post-spot for a high, **min** for a low,
  updated on every same-side tx via `consider_pivot`.
- `last_big_*` — timestamp/price of the **last** same-side tx with `sol_amount >= big_tx_sol`
  (only tracked when `big_tx_sol > 0`).

At `finalize`, the **terminal pivot** = `last_big_*` if the leg contained any big tx, else
`extreme_*`. This is the "real" pump/dump point the chart draws a leg's end at, distinct from
`end_*` (the full-leg chronological span used by stats and filters). When `big_tx_sol = 0`
the pivot is always the price extreme.

---

## 6. Quality filter (`apply_quality_filter`) — pair-based

Walk the ledger; form **fixed, non-overlapping `(swing_high, swing_low)` pairs** (a high
immediately followed by a low).

A single leg **fails** if **any** bound is violated. The per-leg-type bounds (§4c) pick the
`swing_high_*` set for highs and the `swing_low_*` set for lows, and compare by **magnitude**:

```
let delta_pct_abs   = start_price != 0 ? |(end_price-start_price)/start_price*100| : 0
let nf_per_sec_abs  = duration_ms > 0  ? |net_flow / (duration_ms/1000)|  : None   // skipped if 0-duration

trade_count < min_leg_trades
|| duration_ms < min_leg_duration_ms
|| |net_flow| < min_leg_net_flow
|| (inflow+outflow) < min_leg_volume
|| (max_leg_trades   > 0 && trade_count   > max_leg_trades)
|| (max_leg_duration > 0 && duration_ms   > max_leg_duration_ms)
|| (max_leg_net_flow > 0 && |net_flow|    > max_leg_net_flow)
|| (max_leg_volume   > 0 && inflow+outflow > max_leg_volume)
|| (min_delta_pct    > 0 && delta_pct_abs  < min_delta_pct)
|| (max_delta_pct    > 0 && delta_pct_abs  > max_delta_pct)
|| (min_nf_per_sec   > 0 && nf_per_sec_abs.is_some_and(|r| r < min_nf_per_sec))
|| (max_nf_per_sec   > 0 && nf_per_sec_abs.is_some_and(|r| r > max_nf_per_sec))
```

(`min/max_delta_pct` and `min/max_nf_per_sec` resolve to the leg-type-specific params from §4c.
A 0-duration leg has `nf_per_sec_abs = None`, so the two net-flow-per-second bounds are simply
skipped for it rather than dividing by zero.)

**Discard rule:** a pair is kept **only if NEITHER leg fails**; if **either** leg fails, the
**whole pair is dropped**. An **unpaired** leg (a leading lone `swing_low` or a trailing lone
`swing_high`) is **ignored by the filter and kept as-is**.

> ⚠️ **Delta vs. the old TA spec:** that spec says "discard only if BOTH legs fail." The
> shipped code discards if **either** fails. Trust the code (`!(fails(a) || fails(b))`).

The surviving ledger still strictly alternates.

---

## 7. Output schema

Backend `SwingLeg` == frontend `SwingLegRecord`:

```ts
interface SwingLegRecord {
  type: 'swing_high' | 'swing_low';
  start_at: number;        // ms epoch — first tx of the leg
  end_at: number;          // ms epoch — last same-side tx (full-leg span; stats/filters use this)
  duration_ms: number;     // end_at - start_at
  start_price: number;     // pre-trade curve spot before the leg opened
  end_price: number;       // post-trade curve spot of the last same-side tx
  pivot_end_at: number;    // ms epoch — terminal pivot: last big same-side tx, else price extreme (§5)
  pivot_end_price: number; // price at the terminal pivot — the chart's leg-end anchor
  inflow: number;          // total buy SOL
  outflow: number;         // total sell SOL
  net_flow: number;        // inflow - outflow  (+ for high, − for low)
  trade_count: number;
}
```

`pivot_end_*` is for **charting only** — it marks the real pump/dump point so a leg's drawn
endpoint lands on the decisive big tx (or the price extreme) rather than a trailing dust trade.
All stats, filters, and `duration_ms` use `end_*`, never the pivot.

Single-token response: `{ mint, params, count, swings[] }`.
Batch response: `{ params, results: [{ mint, count, swings[] }] }` — one entry per requested
mint, **in request order**; a mint whose trades fail to load returns an empty `swings[]`.

---

## 8. Worked examples

### Example A — basic High→Low reversal

Params: `high_to_low_threshold_sol = 5`, `high_to_low_threshold_pct = 50`, `min_leg_trades = 1`.

```
t=0   BUY  4      → seed swing_high: inflow=4,  net_flow=4,  tc=1   (phase SwingHigh)
t=1s  BUY  6      → apply_buy:       inflow=10, net_flow=10, tc=2
t=2s  SELL 2      → open temp_low. snapshot net_flow=10.
                    threshold = min(5, 0.50*10=5) = 5.  temp.outflow=2  (2 < 5, probation)
t=3s  SELL 4      → temp.outflow=6  →  6 >= 5  ✓ CONFIRM
```

Ledger → `swing_high{inflow 10, net_flow +10, end_price = post-spot of BUY@t1}`,
then `swing_low` begins accumulating from `outflow 6`.

### Example B — sub-threshold merge-back (single-counting)

Params: `high_to_low_threshold_sol = 40`, `high_to_low_threshold_pct = 100`.

```
BUY  100   → swing_high: inflow=100, net_flow=100, tc=1
SELL 10    → temp_low opens. snapshot=100. threshold = min(40, 1.00*100=100) = 40.
             temp.outflow=10, tc=1                          (10 < 40)
SELL 15    → temp.outflow=25, tc=2                          (25 < 40)
BUY  50    → MERGE-BACK (threshold never reached):
                swing_high.outflow     += 25  → 25
                swing_high.trade_count += 2   → 3
             then apply_buy(BUY 50):
                swing_high.inflow      += 50  → 150
                swing_high.trade_count += 1   → 4
                end_at/end_price = BUY 50
```

Active `swing_high`: inflow 150, outflow 25, **net_flow 125**, trade_count 4
(BUY100, SELL10, SELL15, BUY50 — each counted exactly once). A *next* temp leg would
snapshot the reduced **125**, not 100.

### Example C — "no bound" trap

If `high_to_low_threshold_sol = 0` **and** `high_to_low_threshold_pct = 0`, the High→Low
threshold is `+∞`: no sell flow can ever confirm a reversal, so the entire history collapses
into one swing high (plus, at most, a trailing temp low at flush).

---

## 9. Client-side post-passes (display only — no re-fetch)

### 9a. Visibility filter (`swingFilter.ts`)

Narrows which already-detected legs are *shown* in the table/chart. Independent of the
backend quality filter. `0` on any numeric bound = ignore it. Criteria:
`leg_type` (`all|swing_high|swing_low`), min/max `duration_ms`, `trades`, `volume_sol`
(= inflow+outflow), `net_flow_sol`, and `change_pct` where
`change_pct = (end_price - start_price) / start_price * 100`.

### 9b. Chain of swings (`swingChains.ts`)

A **swing pair** = a `swing_high` immediately followed by a `swing_low` (one up-then-down
cycle); unpaired legs are skipped. Two consecutive pairs are **linked** when the idle gap
`next.startAt − current.endAt <= chainLatencyMs` (default **60 000 ms**). A **chain** is a
maximal run of **≥ 2 linked pairs** (an isolated pair is not a chain). Produces per-token:
`swingCount`, `totalPairCount`, `maxSequentialPairCount`, `chainCount`, and `longestChain`
(`{startAt, endAt, pairCount}`, used for the chart band highlight). Re-tuning the latency
re-groups instantly without re-running detection.

**Chain example** (`chainLatencyMs = 60000`):

```
pairs (startAt→endAt, ms):  P1 0→10k   P2 50k→70k   P3 130k→150k   P4 155k→160k
gaps:  P1→P2 = 50k-10k = 40k  ≤60k  link
       P2→P3 = 130k-70k = 60k ≤60k  link   → chain {P1,P2,P3}, run=3
       P3→P4 = 155k-150k = 5k ≤60k  link   → extend → {P1..P4}, run=4
result: totalPairCount=4, chainCount=1, maxSequentialPairCount=4,
        longestChain = {startAt:0, endAt:160k, pairCount:4}
```

---

## 10. Chart overlay (rendering)

`swingOverlay.ts` turns the ledger into chart geometry. Three `segmentMode`s drive the
`TokenPriceChart` `swingOverlay` prop:

- `connected` — one continuous reversal path (first leg's start, then each leg's end),
  colored per leg (`swingHigh`/`swingLow`). Default when "connect swings" is on.
- `perLeg` — one isolated start→end segment per leg. Used when connect is off.
- `connectedSequential` — connected only within runs of legs that are adjacent in the full
  ledger (used when a visibility filter is active, so gaps aren't bridged).

A leg's stable key is `swingLegKey(leg) =`${type}-${start_at}-${end_at}``. Times resolve to
seconds in time mode, or to the nearest trade's slot in slot mode. The longest chain (9b) is
passed as `highlightChain` and drawn as a background band.

---

## 11. Reuse checklist (if re-implementing)

1. Sanitize: drop `sol_amount <= 0`; sort by the canonical order `(slot, tx_index,
   leg_index, block_time)`; apply the dynamic `dust_frac` floor **inside** the scan
   (relative to the active leg's running `max_sol`), never as a pre-pass.
2. Carry `pre_spot` = previous trade's post-spot for `start_price`; `end_price` = last
   same-side post-spot.
3. Run the 4-phase machine; freeze threshold = `min(sol_bound, pct_bound)` with `0 ⇒ +∞`,
   basis = active leg's `|net_flow|` snapshotted at temp open; **re-snapshot** each temp.
   OR every confirm check with `is_big(tx)` (`big_tx_sol > 0 && sol_amount >= big_tx_sol`).
4. Merge-back sub-threshold pokes (fold opposite flow + trade_count; keep end_* on same side).
5. Track the terminal pivot per leg (last big same-side tx, else price extreme) for charting;
   it never feeds stats/filters.
6. Flush keeps the trailing active **and** temp leg.
7. Quality filter on `(high, low)` pairs: drop the pair if **either** leg fails; keep unpaired
   legs untouched. Bounds compare by **magnitude** (so swing lows reuse positive thresholds);
   per-leg-type delta-% and net-flow/s bounds pick the matching `swing_high_*`/`swing_low_*` set,
   and net-flow/s is skipped for 0-duration legs.
8. Everything in **SOL**; `net_flow > 0` ⇒ high, `< 0` ⇒ low.

---

## 12. swing1 — Kill→Volume phase strategy (the last-updated logic)

> **Base everything on this section.** swing1 is the current strategy layer built
> *on top of* the leg detector above. It reads a token's swing chain, latches the
> dev's kill→volume transition, enters on the first volume-phase higher-low, and
> flees on the next kill. The thesis: meme-coin devs manufacture early **kill
> swings** (short, deep near-death lows that eat sniper bots), then a
> **volume-making phase** (longer, shallower higher-lows) to attract real traders
> before the rug — so buy the transition, sell the next kill.
>
> Everything here is **causal** (a leg's verdict never depends on a future leg),
> so the batch sweep, the CLI probe, the per-token detect page, and live trading
> resolve **byte-identical** decisions. This is pinned by a parity test and is the
> whole reason the detector moved into `trading_core` and became generic over
> `TradeRow`.

### 12a. Why the **raw** ledger (not the filtered one)

swing1 consumes `swing::detect_swing_legs_raw` — the §5 scan output finalized to
`SwingLeg`s **without** §6's quality filter. The pair-drop filter is **non-causal** (a leg's fate depends on its partner), so it
would break batch↔live parity. swing1 applies only **causal per-leg gates** instead.
The filtered `detect_swings` stays in use for the cold lab swing-chart/table endpoints.

`swing_params_from_rule` (in `mod.rs`) builds the `SwingParams` for the scan from a
`Swing1Rule`: only the four reversal thresholds + `min_leg_trades` + `dust_frac` are
taken from the rule (via `p_swing_*` / `p_dust_frac` axes); every quality-filter field
stays at its inert default. A `0`/`None` reversal bound falls back to the `SwingParams`
default so a rule that sets neither still splits legs.

### 12b. Per-swing-low features (`LowFeatures`, classifier.rs)

Each swing-LOW leg is reduced to causal features, all measured to the **terminal
pivot** (`pivot_end_*`), never the dust-extended `end_at`:

| Field             | Meaning |
|-------------------|---------|
| `depth_pct`       | `(start_price − pivot_end_price) / start_price`, clamped `[0,1]`. Larger = deeper. `0` if `start_price ≤ 0`. |
| `duration_ms`     | `pivot_end_at − start_at` (the fast collapse span, **not** the full leg span — a post-collapse dust tail must not inflate the kill duration). |
| `net_flow_per_sec`| `abs(net_flow) / (duration_ms/1000)`; `0` for 0-duration legs. |
| `trade_count`     | leg trade count. |
| `pivot_price`     | `pivot_end_price` — the leg's low, used for the higher-low comparison. |

### 12c. Kill / Volume gates (`PhaseProfile`, classifier.rs)

Built by `phase_profile_from_rule`. Each bound at `0`/`None` drops out of the AND.
Crucially, **at least one positive bound must be set** on each side, else "deep+short"
(or "shallow+long") is vacuously true for every leg and the latch is meaningless.

- **Kill low** = deep AND short (AND optionally a fast net-flow drain):
  `depth_pct ≥ kill_depth_min_pct` && `duration_ms ≤ kill_max_duration_ms` &&
  `net_flow_per_sec ≥ kill_min_net_flow_per_sec` — and ≥1 of those is positive.
- **Volume low** = shallow AND long, with real accumulation before it:
  `depth_pct ≤ vol_depth_max_pct` && `duration_ms ≥ vol_min_duration_ms` &&
  the immediately-preceding up-leg's duration `≥ vol_min_up_duration_ms` — and ≥1
  volume bound is positive.

### 12d. The kill→volume latch (`classify_phase`, classifier.rs)

Walk the raw ledger in time order, tracking `kills_seen` and the last kill low's
`pivot_price`. The volume phase **latches** at the *first* swing-low `L` where **all**:

1. `kills_seen ≥ min_kills_before_volume` (a swept, count-free floor; `0` = no kill gate),
2. `is_volume_low(L)` (shallow + long + accumulation), and
3. `L` is a **higher low** vs the last kill pivot (`L.pivot ≥ last_kill_pivot`;
   vacuously true with no prior kill).

The latch is **sticky** (never clears within the chain). A leg is tried as the
transition *before* being counted as a kill — one leg is either the transition or
another kill, never both. Output: `{ volume_phase_latched, latched_leg_index, kills_seen }`.

### 12e. Entry (`find_phase_entry`, entry/mod.rs)

1. `rule_configures_any_entry_gate` guard: requires a `entry_pullback_pct` **and** ≥1
   volume bound, else bail (never a silent buy-everything path).
2. Scan raw legs → `classify_phase`; if not latched, **no entry**.
3. Restrict the higher-low search to trades **after** the latch leg's `end_at`; find
   the first confirmation via `higher_low_confirmed_index` (reused from tpsl2, gated by
   `entry_pullback_pct` + `entry_higher_low_secs`).
4. Enforce `entry_max_age_secs` (confirmation must land within that window of the latch;
   `None`/0 = arm until the token dies).
5. Fill worst-case at the trigger via `find_worst_case_spot_entry_at` — priced off the
   **canonical GMGN spot** (`chart_spot_price`), *not* execution `price_per_token`.
   (Lake `SweepTrade` rows carry the reserve pair but a **zeroed** `price_per_token`;
   reading it yielded ~0 fills the sweep then discarded → the zero-fire bug. Pricing off
   spot keeps live/sweep parity — the Step 0 canonical-price contract. See
   [[canonical-price-gmgn]].) A fill `≤ 0` ⇒ no entry.

### 12f. Exit ladder (`find_trade_driven_exit`, exit/mod.rs)

Single forward scan over post-entry trades; first arm that fires on a trade wins, in
priority order:

```
NextKill → E4 LiquidityExit (real reserves) → StopLoss → TakeProfit →
E1 TrailingStop → E3 Stall → E2 TimeStop
```

- **NextKill** (top arm, swing1-specific): a causal pre-scan (`next_kill_fire_ms`) finds
  the first post-entry swing-LOW leg matching the **exit** next-kill profile
  (`exit_next_kill_profile` = `p_exit_next_kill_depth_min_pct` / `_max_duration_ms`,
  tuned *separately* from the entry kill thresholds). The position flees at the first
  trade at/after that leg's terminal pivot. `None` when neither next-kill bound is set.
- The remaining arms clone tpsl1/tpsl2's predicate shapes (E4 uses **real** reserves
  like tpsl2). Every feature is inert at `None`/0.
- All prices go through the same canonical `spot_price` (`chart_spot_price` → exec
  fallback) as the entry and the leg detector. Worst-case sell fill = lowest spot in the
  `[exit_slot, exit_slot+MAX_FILL_WAIT_SLOTS]` window. `_with_slot` variant also returns
  the firing slot for the live paper fill-poll window.

### 12g. The sweep (`lab/src/sweep/strategies/swing1.rs`)

~26 axes over the `Swing1Rule` knobs, in one canonical order shared by every sampler
(`combo_at` mixed-radix, LHS `axis_lens`, `refine`, `order_for_entry_cache`). Key design
points:

- **Entry/exit split:** exit knobs (TP/SL/trailing/stall/time/liq + next-kill) are
  deliberately **absent** from `Swing1EntryKey` — the entry never depends on them, so the
  expensive entry resolve (full swing scan + classify + higher-low confirm) is cached
  once per distinct entry key and reused across that key's whole exit sub-grid.
  `order_for_entry_cache` sorts same-entry combos contiguously to restore that hit rate
  under random/LHS/refine.
- **~26-axis space MUST use LHS/refine, never a full grid** (grid explosion). The handler
  rejects a spec whose grid product exceeds the combo cap.
- `resolve_entry`/`resolve_exit` call the **same** `entry::find_phase_entry` /
  `exit::find_trade_driven_exit` pure fns as live — decision parity is exact; PnL is the
  frictionless `round_trip_with_costs` of the decision prices (`CostModel::pumpfun_default`).
- Still-open-at-end-of-history ⇒ `ExitCode::Open` at last price (unrealized).

### 12h. Per-token detect funnel + CLI twin

- **`POST /api/tokens/{mint}/swing1-detect`** (`swing1_detect.rs`) runs the *identical*
  pure fns and returns the whole funnel for one token: `legs` (raw ledger), per-low
  `lows` verdicts (`is_kill`/`is_volume`/`higher_low_ok`), the `latch`, and the resolved
  `entry`/`exit`. It's the JSON twin of `lab swing-probe` — **keep the two in lockstep**.
- **`lab swing-probe [N] [after]`** / **`lab swing-census [N] [after]`** (`swing_probe.rs`):
  read-only diagnostics over the Parquet lake (the sweep's corpus source) that print the
  funnel stage-by-stage / a kill·volume·latch prevalence table — for bisecting a
  zero-fire rule. No DB, no HTTP, no sweep engine.
- The frontend [swing1Detect.ts](../../frontend-react/src/lab/services/swing1Detect.ts)
  mirrors the handler's request/response field-for-field; the page renders the funnel
  table + chart overlay.

### 12i. swing1 reuse checklist

1. Build `SwingParams` from the rule (reversal thresholds + `min_leg_trades` +
   `dust_frac` only); scan with `detect_swing_legs_raw` — **raw**, never the §6 filter.
2. Reduce each swing-low to `LowFeatures` measured to the **terminal pivot**.
3. Classify with causal kill/volume gates; require ≥1 positive bound per side.
4. Latch kill→volume at the first volume-phase higher-low with `kills_seen ≥
   min_kills_before_volume`; sticky.
5. Entry: guard the gate; search higher-low only *after* the latch leg; enforce
   `entry_max_age_secs`; fill worst-case off the **canonical spot**, never exec price.
6. Exit: NextKill (separate exit profile) on top, then the tpsl-style ladder; all prices
   off the canonical spot; worst-case sell fill in the slot window.
7. Keep batch and live on the **same pure fns** — parity is the invariant.
