# TPSL2 Entry & Exit Params — How They Work

A plain-language guide to every param, what it measures, and when it fires.
Code references: `entry/mod.rs`, `entry/scalp.rs`, `exit/mod.rs`.

---

## Entry — Two Stages in Order

Entry is a **two-stage funnel**. A token must clear both stages before a position opens.

```
Token created on-chain
        │
   Stage 1: Token criteria  ◄── checked once at creation time
   (does this token match the rule's fingerprint?)
        │  pass
        ▼
   Position created (Holding, no fill yet)
        │
   Stage 2: Scalp gates     ◄── checked on every new trade
   (wait for the right moment in the trade stream)
        │  first trade where all gates hold
        ▼
   Entry fill recorded, buy sent
```

---

### Stage 1 — Token Criteria (`entry/mod.rs`)

These fire **once**, at token creation, before any trade prints.
All configured params must pass. A rule with zero configured params never matches (safety guard).

| Param | What it checks | Match rule |
| --- | --- | --- |
| `p_token_initial_buy_sol` | SOL size of the token's very first buy | within `tolerance_pct`% band |
| `p_token_cu_limit` | Compute unit limit on the creation tx | exact |
| `p_token_cu_price` | Compute unit price on the creation tx | exact |
| `p_token_max_sol_cost` | `max_sol_cost` field from the creation instruction args (lamports → SOL) | within `tolerance_pct`% band |
| `p_token_spendable_sol_in` | `spendable_sol_in` field from the creation instruction args (lamports → SOL) | within `tolerance_pct`% band |
| `p_token_ix_labels` | Ordered list of instruction labels on the creation tx | exact ordered match, same length |

**`tolerance_pct`** applies to the three SOL-value params above. A 10% tolerance on a 1 SOL target accepts 0.9–1.1 SOL. It does NOT apply to CU or label params.

**Purpose:** filter tokens whose creation transaction matches a known profitable bot/deployer signature (CU config, instruction shape, dev buy size).

---

### Stage 2 — Scalp Entry Gates (`entry/scalp.rs`)

After a matching token is found, the system watches its trade stream.
Gates are checked **on every incoming buy trade, in this order**:

```
Each new buy trade
    ↓
1. age gate          — too early?
    ↓ pass
2. alive gate        — token still trading?
    ↓ pass
3. organic gate      — real net buying pressure?
    ↓ pass
4. liquidity gate    — pool has enough real SOL?
    ↓ pass
5. organic-liq gate  — second, independently tunable real-reserves floor?
    ↓ pass
6. higher-low gate   — price chart shows continuation shape?
    ↓ all pass
→  ENTER at this trade's price
```

**Rules for all gates:** `None` or `0` = gate is off (inert). A gate only blocks if it is configured. If NO scalp gate is configured, the rule never enters (safety guard — it can't accidentally buy every trade).

**Fill convention (sim/backtest/sweep):** the trigger trade T fires in slot S. The fill window is **slot S (trades after T only) + the next observed slot after S**, provided the next slot is within `MAX_FILL_WAIT_SLOTS` (≈ 1 s). The recorded entry price is the **highest-priced qualifying buy in the window** — worst case for the buyer. Returns no-fill if the window is empty.

---

#### Gate Details

**1. `p_entry_min_age_secs` — Minimum Age**

Skips the initial launch spike. Waits until the token is at least N seconds old (measured from the first ever trade on that mint).

> Example: `30` → ignore all trades in the first 30 seconds; only consider trades from T+30s onward.

---

**2. `p_entry_min_alive_sol` — Alive / Active Volume**

Total SOL traded (buys + sells, all wallets) in a trailing **10-second window** ending at the candidate trade must be ≥ N.

> Example: `1.5` → if less than 1.5 SOL changed hands in the last 10 seconds, skip. Filters dead/rug tokens that stopped trading.

---

**3. `p_entry_min_organic_sol` — Organic Demand**

Net SOL flow (buys − sells, any wallet) since the token's first trade must be ≥ N.

> Example: `2.0` → at least 2 SOL net bought so far — genuine buying pressure, not just the initial launch trade.

---

**4. `p_entry_min_liquidity_sol` — Real Reserves Floor**

The pool's **real** SOL reserves (from the on-chain `real_sol_reserves` field, NOT virtual) must be ≥ N at the candidate trade.

> Example: `5.0` → pool must have at least 5 SOL of real depth. Real reserves can't be faked by wash trading (virtual reserves can be).

Uses the most recent trade in the prefix that carries a `real_sol_reserves` snapshot.

> **Data source per path.** Live/paper use the program-emitted `real_sol_reserves`
> (exact: curve = the curve `TradeEvent` field, AMM = pool quote reserve). The
> `trades` table **dropped** that column, so BOTH offline paths that read it back
> **approximate** it from the priced reserve pair + `venue` via the shared
> `approx_real_sol_reserves` — AMM → `reserve_sol`, curve → `reserve_sol − 30` (the
> initial virtual SOL), clamped ≥0:
>
> - **Single-rule simulate** (`.../simulate`, the per-rule backtest) now reads the
>   **same Parquet lake, same loader, same `SweepTrade`** the grouped sweep does
>   (`fetch_sim_histories` → `LakeSource::load` with `with_signatures=true`), so
>   `lab::lake::duck` reconstructs the value there — one source, identical pricing
>   whether a rule is swept or drilled into. (It used to read Postgres via
>   `TradeRepo::find_by_mints_all`; that split is gone.)
> - **Grouped sweep** reads the Parquet lake; `lab::lake::duck` reconstructs it there.
>
> Same "true liquidity" the chart shows. Consequence: on the curve this gate is
> effectively "virtual reserve ≥ 30 + N SOL", and sim is a close-but-not-lamport-
> identical proxy for the live gate. **Before this, both offline paths hardcoded
> `real_sol_reserves = None → 0`, so any `min_liq_sol > 0` silently rejected 100% of
> candidates (empty sim despite matched tokens).**

---

**5. `p_entry_min_organic_liq` — Organic Liquidity Floor**

A second, independently tunable real-reserves floor — reads the same `real_sol_reserves` snapshot as `p_entry_min_liquidity_sol`, so the two can be set to different thresholds.

> Example: `3.0` → at least 3 SOL of real pool depth, checked independently from `p_entry_min_liquidity_sol`.

---

**6. `p_entry_pullback_pct` + `p_entry_higher_low_secs` — Higher-Low Shape**

Waits for a specific price structure: the token must have formed a **higher low** — a swing bottom that is above the previous swing bottom — confirming a continuation pattern.

- `p_entry_pullback_pct`: the first dip must be at least N% from its local high to count as a "real" swing (filters micro-wiggles).
- `p_entry_higher_low_secs`: the two consecutive lows must be at least N seconds apart (filters sub-second fakes).

> Example: `pullback=15, secs=10` → wait for a dip of ≥15% off a local high, then a second dip higher than the first, with the two dips at least 10 seconds apart.

This gate is computed in one forward pass (`higher_low_confirmed_index`) and reused across all candidate trades — it is monotonic (once true, stays true).

---

## Exit Ladder (`exit/mod.rs`)

Once in a position, a **priority ladder** is evaluated on every new trade.
**First rule that fires wins** — lower-priority rules are never checked once a higher one fires.

```
Priority  Reason          Param                        Trigger condition
────────  ──────────────  ───────────────────────────  ────────────────────────────────────────────
  1 (top) LiquidityExit   p_exit_liquidity_drop_pct    real reserves < peak_reserves × (1 − drop%)
  2       StopLoss        p_exit_stop_loss             price ≤ entry × (1 − loss%)  [always on]
  3       TakeProfit      p_exit_take_profit           price ≥ entry × (1 + profit%) [always on]
  4       TrailingStop    p_exit_trailing_stop_pct     price ≤ peak_since_entry × (1 − trail%)
  5       Stall           p_exit_stall_secs            no new higher-high for N seconds
  6 (bot) TimeStop        p_exit_time_stop_secs        held for longer than N seconds total
```

**Always on:** StopLoss and TakeProfit. All others are off when their param is `0` or `None`.

**Fill convention (sim/backtest/sweep):** when a ladder rule fires on trade T in slot S, the fill window is **slot S (trades after T only) + the next observed slot after S**, provided the next slot is within `MAX_FILL_WAIT_SLOTS` (≈ 1 s). The recorded exit price is the **lowest-priced trade in the window** — worst case for the seller. If the window is empty the exit is not taken on this firing and the walk continues.

---

### Exit Rule Details

**E4 — `p_exit_liquidity_drop_pct` — Liquidity Crash (top priority)**

Real SOL reserves fall more than N% below their peak since entry.

> Example: `50` → if reserves were 10 SOL at peak and drop to 4.9 SOL (−51%), exit.

Uses `real_sol_reserves` (not virtual). A drop in virtual reserves can be a normal bonding curve artifact; real reserves dropping means actual SOL is leaving the pool.

---

**StopLoss — `p_exit_stop_loss`**

Fixed floor: price falls N% below the entry price.

> Example: `20` → exit if price drops to 80% of entry.

---

**TakeProfit — `p_exit_take_profit`**

Fixed ceiling: price rises N% above the entry price.

> Example: `50` → exit once price hits 150% of entry.

---

**E1 — `p_exit_trailing_stop_pct` — Trailing Stop**

Locks in gains. Exit once price falls N% below the **highest price since entry** (the running peak).

> Example: `30` → if the peak was 3× entry and price pulls back to 2.1× entry (−30% from peak), exit.

Unlike a fixed stop, this rises with the price — it protects gains without capping the upside.

---

**E3 — `p_exit_stall_secs` — Stall / Flatline**

Exit if no new higher-high prints for N seconds. The stall clock starts at entry (not first trade).

> Example: `60` → if the price hasn't made a new high in the last 60 seconds, sell the flatline.

Can fire **during silence** (no new trades) via the 1-second clock sweep.

---

**E2 — `p_exit_time_stop_secs` — Time Stop (lowest priority)**

Cut the position once it has been held for N seconds regardless of price.

> Example: `300` → always exit after 5 minutes, even if no other rule fired.

Can also fire during silence via the clock sweep.

---

## Two Exit Triggers: Trade vs Clock

| Trigger | What fires it | Which rules |
| --- | --- | --- |
| **Trade-driven** | new trade prints on the mint | ALL 6 rules |
| **Clock-driven (1s sweep)** | wall clock, even in silence | Stall (E3) + TimeStop (E2) only |

Price-based rules (LiquidityExit, StopLoss, TakeProfit, TrailingStop) only make sense when a new price observation arrives, so they are trade-only. Time-based rules can fire while the token is dead-quiet.

---

## Percent Convention

All `p_*_pct` and ratio params are stored as **whole percent** (0–100), not fractions (0–1).

The comparison sites divide by 100 before comparing:

- `p_exit_stop_loss = 20` means `−20%` → checked as `price ≤ entry_price * (1 − 20/100)`

The two exceptions that are always on and unclamped: `p_exit_take_profit` (unbounded above 0) and `p_exit_stop_loss`.
