# Swing Detection Logic — Implementation Reference

> Concise, code-accurate spec of how this project detects price **swings** in a token's
> trade history. Source of truth = the Rust implementation, not the older TA-terms spec.
> Reuse this file as a prompt to re-implement or extend the feature.
>
> **Key files**
> - Backend algorithm: [`backend/src/analyzers/swing_analyzer.rs`](../backend/src/analyzers/swing_analyzer.rs)
> - API handlers: [`backend/src/api/handlers/tokens/swing.rs`](../backend/src/api/handlers/tokens/swing.rs)
> - Older TA spec (partially stale — see deltas below): [`token_swing_analyzer_spec_using_TA_terms.md`](token_swing_analyzer_spec_using_TA_terms.md)
> - Params UI / form coercion: [`frontend-react/src/components/analysis/swingParams.tsx`](../frontend-react/src/components/analysis/swingParams.tsx)
> - Post-detection visibility filter (client only): [`frontend-react/src/components/analysis/swingFilter.ts`](../frontend-react/src/components/analysis/swingFilter.ts)
> - Chain-of-swings grouping (client only): [`frontend-react/src/components/analysis/swingChains.ts`](../frontend-react/src/components/analysis/swingChains.ts)
> - Chart overlay rendering: [`frontend-react/src/components/token-price-chart/swingOverlay.ts`](../frontend-react/src/components/token-price-chart/swingOverlay.ts)
> - Page wiring: [`frontend-react/src/pages/analysis/SwingDetectionPage.tsx`](../frontend-react/src/pages/analysis/SwingDetectionPage.tsx)

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
        │
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
- **Sort canonically** (stable tie-breaks):
  ```
  block_time (timestamp_ms)  ASC      ← primary, also the sole source of duration_ms
  slot                       ASC      ← tie-break
  leg_index (position)       ASC      ← final tie-break (= storage order)
  ```

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

## 4. Parameters (`SwingParams`) — 12 fields

Sent as JSON. Defaults applied per-field via serde; `{}` ⇒ all defaults.

| Param                        | Default | Type | Meaning |
|------------------------------|--------:|------|---------|
| `high_to_low_threshold_sol`  | `5.0`   | f64  | Absolute SOL needed to confirm a High→Low reversal. `0` = no bound. |
| `high_to_low_threshold_pct`  | `50.0`  | f64  | Percent (0–100) of the current high's `|net_flow|` to confirm High→Low. `0` = no bound. |
| `low_to_high_threshold_sol`  | `5.0`   | f64  | Absolute SOL to confirm a Low→High reversal. `0` = no bound. |
| `low_to_high_threshold_pct`  | `50.0`  | f64  | Percent of the current low's `|net_flow|` to confirm Low→High. `0` = no bound. |
| `min_leg_trades`             | `2`     | u32  | Quality filter: leg fails if `trade_count <` this. |
| `min_leg_duration_ms`        | `0`     | i64  | Leg fails if `duration_ms <` this. |
| `min_leg_volume`             | `0.0`   | f64  | Leg fails if `inflow+outflow <` this. |
| `min_leg_net_flow`           | `0.0`   | f64  | Leg fails if `|net_flow| <` this. |
| `max_leg_trades`             | `0`     | u32  | If `>0`: leg fails if `trade_count >` this. |
| `max_leg_duration_ms`        | `0`     | i64  | If `>0`: leg fails if `duration_ms >` this. |
| `max_leg_volume`             | `0.0`   | f64  | If `>0`: leg fails if `inflow+outflow >` this. |
| `max_leg_net_flow`           | `0.0`   | f64  | If `>0`: leg fails if `|net_flow| >` this. |

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

### SwingHigh
- **BUY** → `apply_buy`: `inflow += sol`, `end_at/end_price = tx`, `trade_count++`.
- **SELL** → open `temp_low` seeded from this SELL (`outflow = sol`, counts immediately),
  freeze `high→low` threshold off `current_high.net_flow`, phase → `TempSwingLow`.
  *Then immediately check:* if `temp.outflow >= threshold`, the reversal confirms on this
  very SELL (push `current_high` to ledger, `temp` becomes `current_low`, phase → `SwingLow`).

### TempSwingLow
- **SELL** → `temp.outflow += sol`. If `>= threshold` ⇒ **confirm**: push `current_high`,
  `temp` → `current_low`, phase → `SwingLow`.
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

---

## 6. Quality filter (`apply_quality_filter`) — pair-based

Walk the ledger; form **fixed, non-overlapping `(swing_high, swing_low)` pairs** (a high
immediately followed by a low).

A single leg **fails** if **any** bound is violated:

```
trade_count < min_leg_trades
|| duration_ms < min_leg_duration_ms
|| |net_flow| < min_leg_net_flow
|| (inflow+outflow) < min_leg_volume
|| (max_leg_trades   > 0 && trade_count   > max_leg_trades)
|| (max_leg_duration > 0 && duration_ms   > max_leg_duration_ms)
|| (max_leg_net_flow > 0 && |net_flow|    > max_leg_net_flow)
|| (max_leg_volume   > 0 && inflow+outflow > max_leg_volume)
```

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
  start_at: number;     // ms epoch — first tx of the leg
  end_at: number;       // ms epoch — last same-side tx
  duration_ms: number;  // end_at - start_at
  start_price: number;  // pre-trade curve spot before the leg opened
  end_price: number;    // post-trade curve spot of the last same-side tx
  inflow: number;       // total buy SOL
  outflow: number;      // total sell SOL
  net_flow: number;     // inflow - outflow  (+ for high, − for low)
  trade_count: number;
}
```

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

A leg's stable key is `swingLegKey(leg) = `${type}-${start_at}-${end_at}``. Times resolve to
seconds in time mode, or to the nearest trade's slot in slot mode. The longest chain (9b) is
passed as `highlightChain` and drawn as a background band.

---

## 11. Reuse checklist (if re-implementing)

1. Sanitize: drop `sol_amount <= 0`; sort by `(timestamp_ms, slot, position)`.
2. Carry `pre_spot` = previous trade's post-spot for `start_price`; `end_price` = last
   same-side post-spot.
3. Run the 4-phase machine; freeze threshold = `min(sol_bound, pct_bound)` with `0 ⇒ +∞`,
   basis = active leg's `|net_flow|` snapshotted at temp open; **re-snapshot** each temp.
4. Merge-back sub-threshold pokes (fold opposite flow + trade_count; keep end_* on same side).
5. Flush keeps the trailing active **and** temp leg.
6. Quality filter on `(high, low)` pairs: drop the pair if **either** leg fails; keep unpaired
   legs untouched.
7. Everything in **SOL**; `net_flow > 0` ⇒ high, `< 0` ⇒ low.
```
