# Token Swing Analyzer Specification (using TA Terminology)

## Goal

Analyze a token's transaction history and detect alternating:

- **SWING HIGH** (buy-dominant leg)
- **SWING LOW** (sell-dominant leg)

A leg reversal is confirmed when opposite-side flow reaches a configurable reversal threshold.

---

## Transaction Structure

```typescript
Transaction {
    slot: number          // Solana slot
    position: number      // index of this tx within backend data (cache/DB) — tie-breaker only
    timestamp_ms: number
    side: "buy" | "sell"
    sol_amount: number
    price: number
}
```

All volume calculations use **SOL amount**, not token amount.

### Transaction Ordering (canonical)

```text
timestamp_ms ASC
slot         ASC      (tie-break)
position     ASC      (final tie-break, = order in backend storage)
```

`timestamp_ms` is the primary ordering key and is assumed to be reliable. When two
transactions share an identical `timestamp_ms`, fall back to `slot`, then to the
stored `position`. `timestamp_ms` is also the sole source for `duration_ms`.

### Input Sanitization

Before processing, **skip** (do not count, do not affect any leg) any transaction where:

```text
sol_amount <= 0
OR side is not exactly "buy" or "sell"
```

---

## Global Parameters

```typescript
// Reversal thresholds: Swing High → Swing Low
high_to_low_threshold_sol: number
high_to_low_threshold_pct: number      // range 0–100 (real percent, e.g. 50 = 50%)

// Reversal thresholds: Swing Low → Swing High
low_to_high_threshold_sol: number
low_to_high_threshold_pct: number      // range 0–100 (real percent)

// Minimum leg quality filters
min_leg_trades: number
min_leg_duration_ms: number
min_leg_volume: number
min_leg_net_flow: number
```

### Threshold Rule

The threshold basis is the **net flow** of the current (active) leg, snapshotted at the
instant the temp reversal leg begins. `abs()` is used so the basis is always a positive
volume (a swing low's `net_flow` is negative).

```text
reversal_threshold (high → low) =
    min(
        high_to_low_threshold_sol,
        (high_to_low_threshold_pct / 100) * abs(net_flow_snapshot)
    )

reversal_threshold (low → high) =
    max(
        low_to_high_threshold_sol,
        (low_to_high_threshold_pct / 100) * abs(net_flow_snapshot)
    )
```

- `net_flow_snapshot` = the current leg's `net_flow` at the moment the temp leg begins.
  - high → low: `net_flow_snapshot = current_swing_high.net_flow` (positive).
  - low → high: `net_flow_snapshot = current_swing_low.net_flow` (negative → use `abs`).
- Note the asymmetry: high → low uses **min**, low → high uses **max**.
- Threshold is **frozen** at the moment the temp reversal leg begins.
- **Re-snapshot on every temp begin.** After a temp leg merges back (sub-threshold), the
  current leg's `net_flow` is reduced by the merged opposite-side flow. The *next* temp
  leg re-snapshots this reduced `net_flow`, so successive reversal attempts use a smaller
  (high → low) basis. This is intended.
- Reversal is confirmed when:

```text
temp_leg_volume >= reversal_threshold
```

where `temp_leg_volume` is `temp_swing_low.outflow` (high → low) or
`temp_swing_high.inflow` (low → high).

---

## States

```typescript
enum Phase {
    SWING_HIGH,
    TEMP_SWING_LOW,
    SWING_LOW,
    TEMP_SWING_HIGH
}
```

---

## Runtime Variables

```typescript
swing_ledger = []       // finalized alternating legs (pre-filter)

current_swing_high = null
current_swing_low  = null

temp_swing_high = null
temp_swing_low  = null

phase: Phase
```

---

## Swing Leg Structure

```typescript
SwingLeg {
    // Identity
    type: "swing_high" | "swing_low"

    // Timing
    start_at: number        // timestamp_ms of first transaction of the leg
    end_at: number          // timestamp_ms of the last same-side transaction (see below)
    duration_ms: number     // end_at - start_at

    // Price
    start_price: number     // spot before the first transaction of the leg (pre-trade)
    end_price: number       // post-trade spot of the last same-side transaction (see below)

    // Flow (SOL)
    inflow: number          // total buy-side SOL
    outflow: number         // total sell-side SOL
    net_flow: number        // inflow - outflow (positive = buy pressure)

    // Activity
    trade_count: number
}
```

### `end_at` / `end_price` definition

- **swing_high**: the time/price of the **last BUY** counted into the leg.
- **swing_low**: the time/price of the **last SELL** counted into the leg.

They are updated on every same-side transaction. The BUY/SELL that triggers a sub-threshold
**merge-back** is processed normally and therefore updates `end_at`/`end_price`; the
merged-back opposite-side transactions do **not** change them. Net effect: `end_*` always
reflects the most recent same-side trade.

---

## Single-Counting Rule (foundational)

Every sanitized transaction is applied to exactly **one** leg, exactly **once**.

**"Create a leg from this transaction" fully consumes that transaction**: it sets
`start_at/start_price/end_at/end_price` from the tx, seeds the side amount, and sets
`trade_count = 1`. A subsequently-running "while receiving …" block applies only to
*later* transactions — never re-applies the creating transaction.

---

## Initialization

Using the first sanitized transaction:

```text
If first transaction is BUY:
    phase = SWING_HIGH
    create current_swing_high:
        inflow      = buy_amount
        outflow     = 0
        net_flow    = buy_amount
        trade_count = 1
        start_at  = end_at  = tx.timestamp_ms
        start_price = end_price = tx.price

If first transaction is SELL:
    phase = SWING_LOW
    create current_swing_low:
        outflow     = sell_amount
        inflow      = 0
        net_flow    = -sell_amount
        trade_count = 1
        start_at  = end_at  = tx.timestamp_ms
        start_price = end_price = tx.price
```

---

## SWING HIGH Phase Logic

### While receiving BUY:

```text
swing_high.inflow      += buy_amount
swing_high.net_flow    += buy_amount
swing_high.end_price    = tx.price
swing_high.end_at       = tx.timestamp_ms
swing_high.trade_count += 1
```

### When first SELL arrives:

```text
phase = TEMP_SWING_LOW

create temp_swing_low from this transaction (fully consumes it):
    outflow     = sell_amount
    trade_count = 1
    start_at    = tx.timestamp_ms
    start_price = tx.price

net_flow_snapshot = current_swing_high.net_flow      ← frozen
freeze reversal_threshold (high → low)
```

The opening SELL's `sol_amount` **counts toward** the reversal threshold immediately.

---

## TEMP_SWING_LOW Phase Logic

`net_flow` is **not** maintained on temp legs.

### While receiving SELL:

```text
temp_swing_low.outflow     += sell_amount
temp_swing_low.trade_count += 1
```

### If threshold is crossed:

```text
temp_swing_low.outflow >= reversal_threshold (high → low)
```

Then:

```text
finalize current_swing_high   (end_* already = last BUY)
push current_swing_high to swing_ledger

current_swing_low = temp_swing_low, completing its fields:
    inflow      = 0
    net_flow    = -outflow
    start_at / start_price = (first SELL of the temp leg, already set on creation)
    end_at / end_price     = last SELL counted into the temp leg
clear temp_swing_low

phase = SWING_LOW
```

### If BUY arrives before threshold (merge-back):

```text
merge temp_swing_low back into current_swing_high:
    swing_high.outflow     += temp_swing_low.outflow
    swing_high.net_flow    -= temp_swing_low.outflow
    swing_high.trade_count += temp_swing_low.trade_count
    (end_at / end_price unchanged here — they track the last BUY)

clear temp_swing_low
phase = SWING_HIGH

then process this BUY normally (SWING HIGH "while receiving BUY"),
which updates inflow / net_flow / end_at / end_price / trade_count.
```

The merge moves only the temp SELLs; the triggering BUY is counted exactly once, by the
normal handler.

---

## SWING LOW Phase Logic

Mirror image of SWING HIGH.

### While receiving SELL:

```text
swing_low.outflow      += sell_amount
swing_low.net_flow     -= sell_amount
swing_low.end_price     = tx.price
swing_low.end_at        = tx.timestamp_ms
swing_low.trade_count  += 1
```

### When first BUY arrives:

```text
phase = TEMP_SWING_HIGH

create temp_swing_high from this transaction (fully consumes it):
    inflow      = buy_amount
    trade_count = 1
    start_at    = tx.timestamp_ms
    start_price = tx.price

net_flow_snapshot = current_swing_low.net_flow       ← frozen (negative)
freeze reversal_threshold (low → high)
```

The opening BUY's `sol_amount` **counts toward** the reversal threshold immediately.

---

## TEMP_SWING_HIGH Phase Logic

### While receiving BUY:

```text
temp_swing_high.inflow      += buy_amount
temp_swing_high.trade_count += 1
```

### If threshold is crossed:

```text
temp_swing_high.inflow >= reversal_threshold (low → high)
```

Then:

```text
finalize current_swing_low   (end_* already = last SELL)
push current_swing_low to swing_ledger

current_swing_high = temp_swing_high, completing its fields:
    outflow     = 0
    net_flow    = inflow
    start_at / start_price = (first BUY of the temp leg, already set on creation)
    end_at / end_price     = last BUY counted into the temp leg
clear temp_swing_high

phase = SWING_HIGH
```

### If SELL arrives before threshold (merge-back):

```text
merge temp_swing_high back into current_swing_low:
    swing_low.inflow       += temp_swing_high.inflow
    swing_low.net_flow     += temp_swing_high.inflow
    swing_low.trade_count  += temp_swing_high.trade_count
    (end_at / end_price unchanged here — they track the last SELL)

clear temp_swing_high
phase = SWING_LOW

then process this SELL normally (SWING LOW "while receiving SELL").
```

---

## Ownership Rule

```text
If temp leg does NOT reach threshold:
    → merge all temp transactions back into the current leg
    → no double counting

If temp leg DOES reach threshold:
    → the temp leg becomes the new current leg
    → finalize the old current leg, push it, begin the new leg
```

---

## Finalization

### Swing High:

```text
swing_high.end_at      = timestamp_ms of last BUY counted into the leg
swing_high.end_price   = price of last BUY counted into the leg
swing_high.duration_ms = swing_high.end_at - swing_high.start_at
swing_high.net_flow    = swing_high.inflow - swing_high.outflow      ← positive
```

### Swing Low:

```text
swing_low.end_at       = timestamp_ms of last SELL counted into the leg
swing_low.end_price    = price of last SELL counted into the leg
swing_low.duration_ms  = swing_low.end_at - swing_low.start_at
swing_low.net_flow     = swing_low.inflow - swing_low.outflow        ← negative
```

---

## End of History

Discard the currently active leg and any temp leg (they are never finalized). Only fully
confirmed, finalized legs reach `swing_ledger`.

---

## Quality Filter (post-processing, pair-based)

After the full `swing_ledger` is built, apply quality filtering as a final pass.

### Per-leg pass/fail

A leg **fails** if **any** of these are true:

```text
leg.trade_count        <  min_leg_trades
leg.duration_ms        <  min_leg_duration_ms
abs(leg.net_flow)      <  min_leg_net_flow
(leg.inflow + leg.outflow) < min_leg_volume
```

### Pairing & discard rule

- Form **fixed, non-overlapping `(swing_high, swing_low)` pairs** by scanning the ledger:
  each `swing_high` immediately followed by a `swing_low` is one pair.
- The discard rule applies **only to `high → low` pairs**.
- **Discard a pair (both legs) only if BOTH legs fail.** Otherwise keep **both** legs.
- Any leg that does not belong to a complete `(swing_high, swing_low)` pair — e.g. a
  leading `swing_low` (history started with a SELL) or a trailing `swing_high` — is
  **ignored by the filter and kept as-is**.

The resulting ledger still alternates strictly:

```text
swing_high → swing_low → swing_high → swing_low → ...
```

---

## Examples

### Example 1 — basic reversal

#### Input Transactions:

```text
BUY  100
BUY   50
SELL  10
SELL  15
SELL  20
```

#### Parameters:

```text
high_to_low_threshold_sol = 40
high_to_low_threshold_pct = 50      // 50%
```

#### Processing:

```text
After BUY 100 + BUY 50:
    swing_high.inflow   = 150
    swing_high.net_flow = 150

First SELL (10) arrives → open temp_swing_low:
    net_flow_snapshot = 150
    reversal_threshold = min(40, 0.50 * 150) = min(40, 75) = 40
    temp_swing_low.outflow = 10

After SELL 15:
    temp_swing_low.outflow = 25   →  25 < 40, not yet

After SELL 20:
    temp_swing_low.outflow = 45   →  45 >= 40  ✓ reversal confirmed
```

#### Output:

```text
swing_ledger[0] — Swing High:
    inflow    = 150
    outflow   =   0
    net_flow  = +150
    end_price = price of BUY 50 (last BUY)

swing_ledger[1] — Swing Low:
    outflow   =  45
    inflow    =   0
    net_flow  =  -45
```

### Example 2 — sub-threshold merge-back (single counting)

#### Input Transactions:

```text
BUY  100
SELL  10
SELL  15
BUY   50
```

#### Parameters:

```text
high_to_low_threshold_sol = 40
high_to_low_threshold_pct = 100
```

#### Processing:

```text
BUY 100 → init swing_high: inflow=100, net_flow=100, trade_count=1

First SELL (10) → open temp_swing_low:
    net_flow_snapshot = 100
    threshold = min(40, 1.00 * 100) = 40
    temp_swing_low.outflow = 10, trade_count = 1

SELL 15 → temp_swing_low.outflow = 25, trade_count = 2    (25 < 40)

BUY 50 arrives before threshold → MERGE-BACK:
    swing_high.outflow     += 25   → 25
    swing_high.net_flow    -= 25   → 75
    swing_high.trade_count += 2    → 3
    then process BUY 50 normally:
        swing_high.inflow   += 50  → 150
        swing_high.net_flow += 50  → 125
        swing_high.trade_count += 1 → 4
        end_at/end_price = BUY 50
```

#### Resulting active swing_high (not yet finalized):

```text
inflow      = 150
outflow     =  25
net_flow    = 125
trade_count =   4     (BUY100, SELL10, SELL15, BUY50 — each counted once)
```

If a later temp leg opens, its `net_flow_snapshot` would be the **reduced** 125
(re-snapshot), not the original 100.

---

## Term Reference (v1 → v2)

| v1 Term | v2 Term |
|---|---|
| peak / hill | swing high |
| valley | swing low |
| direction | phase |
| stack | swing_ledger |
| turn2valley_sol / turn2peak_sol | high_to_low_threshold_sol / low_to_high_threshold_sol |
| turn2valley_pct / turn2peak_pct | high_to_low_threshold_pct / low_to_high_threshold_pct |
| peak_sol_amount_plus | swing_high.inflow |
| peak_sol_amount_minus | swing_high.outflow |
| peak_sol_amount | swing_high.net_flow |
| valley_sol_amount_minus | swing_low.outflow |
| valley_sol_amount_plus | swing_low.inflow |
| valley_sol_amount | swing_low.net_flow |
| peak_trades / valley_trades | leg.trade_count |
| peak_duration / valley_duration | leg.duration_ms |
