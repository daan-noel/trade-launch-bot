# TPSL Scalp Strategy — Entry/Exit Logic

> Single source of truth for entry + exit. **Supersedes** the old launch-sniper `N1–N10` plan
> (deleted). Market numbers / param rationale live in `pumpfun-market-status-2026-and-tpsl-params.md`.

## Implementation status (2026-06-11) — SHIPPED in `tpsl_sniper_2`

The scalp model is **built and live in `tpsl_sniper_2`** (it is **tpsl2-only**;
`tpsl_sniper_1` keeps the legacy token-fingerprint entry + virtual-reserve exits).

- **Params:** all entry/exit columns exist on the rule, **renamed** to a
  `p_entry_*` / `p_exit_*` convention (migration 0008 added them, 0010 renamed).
  Map of plan-name → shipped column:
  `p_min_age_secs`→`p_entry_min_age_secs`, `p_min_alive_sol`→`p_entry_min_alive_sol`,
  `p_min_organic_sol`→`p_entry_min_organic_sol`, `p_pullback_pct`→`p_entry_pullback_pct`,
  `p_higher_low_secs`→`p_entry_higher_low_secs`, `p_max_cohort_held`→`p_entry_max_cohort_held`,
  `p_min_liquidity_sol`→`p_entry_min_liquidity_sol`, `p_min_organic_liq`→`p_entry_min_organic_liq`,
  `p_cohort_exit_ratio`→`p_exit_cohort_ratio`, `take_profit`→`p_exit_take_profit`,
  `stop_loss`→`p_exit_stop_loss`, `p_trailing_stop_pct`→`p_exit_trailing_stop_pct`,
  `p_stall_secs`→`p_exit_stall_secs`, `p_time_stop_secs`→`p_exit_time_stop_secs`,
  `p_liquidity_drop_pct`→`p_exit_liquidity_drop_pct`.
- **Entry gate** — DONE: `tpsl_sniper_2/entry/scalp.rs` (`find_scalp_entry`,
  `scalp_features`, `higher_low_confirmed`). All gates real + unit-tested
  (age / alive / organic / higher-low / cohort-held / real-liquidity / organic-liq);
  each inert at `None/0`. The backtest **requires** ≥1 scalp gate be set for a
  tpsl2 rule.
- **Cohort** — DONE: `tpsl_sniper_2/cohort.rs` (cohort window
  `EARLY_COHORT_SLOT_WINDOW = 150`, `held_ratio`, outside-net-SOL).
- **Exit ladder** — DONE: all 7 in `tpsl_sniper_2/exit/mod.rs` in priority order
  Cohort(E5) → Liquidity(E4) → StopLoss → TakeProfit → Trailing(E1) → Stall(E3) →
  TimeStop(E2). **E4 reads REAL reserves** in tpsl2 (the "virtual→real switch" is
  shipped); **E5 cohort-dump** is live. (tpsl1's E4 still uses virtual; no E5.)
- **Mayhem exclusion** — DONE: `!t.is_mayhem_mode` filter in
  `tpsl_sniper_2/backtest.rs` (and tpsl1's). *(The old `simulation_tpsl.rs:326`
  reference is dead — that file no longer exists; the filter is now in `backtest.rs`.)*

The remaining sections below are the original design rationale; the inline
"build/built (EN)" markers are superseded by the status above.

## Goal

- Skip rugs, catch the token that keeps climbing, take a **small** profit, get out fast.
- It's a **scalp**, not a survivor-pick — in early, out in ~40s–3min. We don't care it rugs at minute 20; we're already gone.
- Buy **once**, then the exit ladder runs.

## The 3 shapes — buy only the 3rd

| shape | looks like | action |
|---|---|---|
| **Spike-and-die** | pumps once, dead in 10s–2m | **skip** |
| **Bot-eater** | flat plateau at the top, then ONE giant dump | **skip** |
| **Real continuation** | pumps, dips, climbs again on real buying | **BUY** |

---

## ENTRY — buy on the first trade where ALL hold

- **Wait a few seconds** — skip the launch spike; instant-rugs are already dead by then. `p_min_age_secs` (~8–15s)
- **Still trading** — buys/sells still printing in the window. `p_min_alive_sol`
- **New people buying** — wallets absent at launch are net-buying (real demand = who you sell to; dev wallets buying = nothing). `p_min_organic_sol`
- **Higher-low continuation** *(the shape gate — see below)*. `p_pullback_pct`, `p_higher_low_secs`
- **Launch cohort already sold most of its bag** — no overhang loaded to dump on you. `p_max_cohort_held`
- **Real, big-enough liquidity** — from buyers, not one dev deposit; use **real** reserves. `p_min_liquidity_sol` + `p_min_organic_liq`

### Higher-low continuation (replaces the old "new-high" gate)

- Watch the **bottoms** of each dip, not the tops.
- A dip only counts if it falls **≥ `p_pullback_pct`** off the local high — tiny wiggles are ignored.
- **BUY** when a new dip bottoms **above the previous dip** (a *higher low*) and price turns back up — and new buyers confirm it.
- Why: real charts swing several times, so "make a new high" is rigid and easy to fake. **Higher-lows = a genuine uptrend** and are harder to fake than a single bounce.
- You only need the **first** higher-low; after that you're in and the exit ladder owns the trade.

```
example (p_pullback_pct = 15%):
1.00  up
0.85  dip 1  (−15% → counts)
1.05  bounce
0.92  dip 2  ← higher than 0.85 = HIGHER LOW → BUY (new buyers confirm)

ignored: 1.00 → 0.98 → 1.01   (wiggle never dips 15%)
skipped: 1.00 → 0.80 → 0.50   (freefall never makes a higher low)
```

---

## EXIT — sell on the FIRST that fires (top → bottom)

- **Cohort/dev wallets start dumping** → out now (biggest danger). `p_exit_cohort_ratio` (~0.05) — **DONE (E5)**
- **Liquidity draining** → out. `p_exit_liquidity_drop_pct` (~25–30%) — **DONE (E4), on REAL reserves**
- **Down 25–30%** → cut loss. `stop_loss` — built (retune)
- **Up 15–25%** → take the small profit *(the goal)*. `take_profit` — built (retune, was 60)
- **Dropped 12–18% off peak** → lock it in. `p_trailing_stop_pct` — built (retune, was 25)
- **Flat ~30–45s, or held ~2–3min** → done, move on. `p_stall_secs`, `p_time_stop_secs` — built (retune)

Priority when several fire on one trade: **Cohort → Liquidity → StopLoss → TakeProfit → Trailing → Stall → TimeStop.**

The **bot-eater is a one-candle dump** — you can't sell into it. The higher-low entry gate is what keeps you *out* of it; the exit ladder only saves you from slow deaths. → **entry avoidance beats exit** for that shape.

---

## Shared primitives (Tier-1, from current data only)

```
t0 / first_slot = first trade time / slot
COHORT C = wallets buying within 150 slots of first_slot  ∪  creator   (≠ my own wallet)
OUTSIDE O = everyone else                                  ← continuation demand & exit liquidity
cohort_held_ratio = Σ_C max(net_tokens,0) / Σ_C bought_tokens   (1 = holding, →0 = sold out)
liquidity         = REAL sol reserves   (NEVER virtual — wash can't fake real)
```

Reuses the rug-detection cohort window (`EARLY_COHORT_SLOT_WINDOW = 150`) so there's one cluster definition.

---

## Starting param values (scalp — supersede the launch-sniper snapshot)

| param | start | note |
|---|---|---|
| `p_min_age_secs` | 8–15s | skip the launch spike |
| `take_profit` | ~20% | bank the small leg (was 60) |
| `stop_loss` | 25–30% | survive normal chop |
| `p_trailing_stop_pct` | 12–18% | tighter than the old 25 |
| `p_liquidity_drop_pct` | 25–30% | on **real** reserves; primary rug-catcher |
| `p_stall_secs` | 30–45s | faster than the old 60 |
| `p_time_stop_secs` | 120–180s | ~2–3min (was 480) |
| `buy_amount` (SOL) | 0.05 *(locked)* | tiny vs early-curve reserves |
| Mayhem tokens | excluded *(shipped)* | manufactured noise, no edge |

---

## Build order — all DONE (2026-06-11, in `tpsl_sniper_2`)

1. ~~**Higher-low gate + cohort-already-sold**~~ — DONE (`entry/scalp.rs`, `cohort.rs`).
2. ~~**New-buyers + real-liquidity + age/alive.**~~ — DONE (all entry gates in `scalp_features`).
3. ~~**E5 cohort-dump + E4 real-reserves switch**~~ — DONE (`exit/mod.rs`).
   Remaining is **tuning**, not building: retune TP/SL/trail/stall/time on recent data.

## Plumbing per new param (all inert at `0/NULL/false`)

- **Migration** — `ADD COLUMN` to `tpslN_strategy_rules` (own migration file; auto-applies on restart).
- **Model** — field on the rule struct + `new()`.
- **Repo** — row struct + `From` + INSERT / SELECT / UPDATE lists.
- **API** — `RuleResponse` / `CreateRuleRequest` / `UpdateRuleRequest`.
- **Frontend** — rule type + form.
- **Verify** — `cargo test tpsl` → `cargo build`; `npm run build`; restart, set **only** the new param, run sim, confirm the effect *and* that `0/NULL/false` reproduces the prior run.
- Canonical module: `backend/src/strategies/tpsl_sniper_1/`; mirror to `tpsl_sniper_2`.

## Dropped (and why)

- **Old N1–N10 launch-sniper list** — superseded; we gate by **shape + flow**, not block-0 fingerprints.
- **"New-high" entry gate** — too rigid / fakeable on multi-swing charts; replaced by higher-low.
- **N1 exclude-rugged/dead** — paper must equal real; the `is_dead` verdict is a *post-mortem* (liquidity already gone) and we trade rug-prone tokens anyway, so it's no entry gate.
- **N7 migration-bait reject** — bait with a real continuation IS the trade; filter by *shape*, not labels.
- **Distinct-buyer-count / raw volume** — bundlers fake both (40-wallet splits, 24/7 wash); use organic outside-buying + real reserves instead.

## Caveat — Mayhem supply

Mayhem tokens have **2B** supply but `TOKEN_TOTAL_SUPPLY` is hardcoded 1B → any supply-% gate (cohort/overhang) reads **2× high**. Mayhem is already excluded from backtests; if a supply-% gate ever runs on Mayhem, derive supply per-token.
