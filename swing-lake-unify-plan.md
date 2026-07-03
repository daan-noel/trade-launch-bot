# Swing1 lake-unification + full-history analysis plan

**Status: IMPLEMENTED 2026-07-03** (scope = "Lake for legs/entry/exit only"). `cargo check
-p trading_core -p lab` clean; `cargo test -p lab` green; frontend tsc clean on touched files
(one pre-existing unrelated `live/App.tsx` unused-import error remains). Chart-candle
unification (options B/C) deferred. Generic `swing.rs` endpoints left on PG + cap.

**Follow-up 2 (2026-07-03) — single-source hardening.** Confirmed the *decision* SoT was
already met: sweep + backtest + simulate + detect all resolve entry/exit through the same
`entry::find_phase_entry` / `exit::find_trade_driven_exit` primitives. Closed the remaining
*diagnostic* duplication: the per-low verdict walk is now extracted to
`funnel::classify_swing_lows(legs, profile)` and shared by `build_swing1_funnel`,
`lab swing-probe`, and `lab swing-census` (the two probe fns previously re-walked the gate
loop inline). Added the `funnel_matches_leg_primitive` parity test pinning the funnel's legs
to the exact `detect_swing_legs_raw` call the backtest carries. **Deliberately did NOT** make
the backtest carry the whole `Swing1Funnel` (plan change §4's `Swing1BacktestTokenResult {
funnel, entry_index }`): the inspect chart draws only legs, so carrying per-token lows/latch
would bloat the multi-token sim payload for data the table never shows — the backtest keeps
carrying just `swing_legs` from the same shared leg primitive. §4 as originally specced is
therefore **intentionally superseded**, not pending.

**Follow-up (same day):** grouped sweep per-mint trade cap removed too —
`SWEEP_DEFAULT_PER_MINT_CAP` 5000→`i64::MAX`, so ALL analysis (sweep + simulate +
backtest) runs over each token's entire history, no cap. Also closes a real parity gap
(the sweep used to truncate high-volume tokens to the launch-window 5k while simulate was
uncapped). `SWEEP_PER_MINT_CAP` env (≥1) kept as an opt-in perf bound; cost is corpus
weight (`tokens × trades/token`, was cut ~10–25× by the cap) on high-volume tokens.

Continuation of `simulate-lake-migration-plan.md`. Closes the "same token bounded three
different ways" seam on the chart page and makes swing1 legs, entry, and exit come from
**one uncapped lake read** — the same corpus the backtest/sweep price on.

## Problem (confirmed by trace)

The "same token" on the chart page is loaded three inconsistent ways:

| Data | Source | Bound | Site |
| --- | --- | --- | --- |
| Chart candles (`GET /trades`) | PG (shared core) | 5,000 | `tokens.rs:526` |
| Swing legs (`swing1-detect`) | PG | **2,500 = `MAX_TRADES_RETAINED`** | `swing1_detect.rs:35` |
| Backtest entry/exit (`fetch_sim_histories`) | Lake | uncapped (`i64::MAX`) | `sim_fetch.rs:31` |

`MAX_TRADES_RETAINED = 2_500` is the **live box's in-RAM cache trim** (4 GB EC2 guardrail),
not an analysis bound — using it to cap an analysis read is a category error. Visible bug
today: a high-volume token draws up to 5,000 candles but the swing overlay is computed off
only the first 2,500 rows (`find_by_mint_paged(.., 2500, 0)` returns oldest-first), so the
legs stop partway across the chart, and they can disagree with the entry/exit the sim (lake,
uncapped) actually resolved.

## Decision (user, 2026-07-03)

Scope = **"Lake for legs/entry/exit only."** Chart candles stay on PG `GET /trades`. For a
**sealed** (past-day) token PG and the lake are identical mirrors, so legs align with candles;
for a **today** token the lake is stale (sealed-days-only) — same pre-existing `warn_if_stale`
caveat, now surfaced on the detect path too. Full unification of the candle source (lake+PG
tail) was explicitly deferred.

## Changes

### 1. `fetch_sim_histories` — parameterize `curve_only`, add single-mint helper
`lab/src/strategies/sim_fetch.rs`
- `CorpusTrade` has no `venue`, so `curve_only` must be a **load-time** filter via
  `Selection.curve_only`, not post-projection.
- Signature: `fetch_sim_histories(mints, curve_only)` (backtests pass `false`, preserving
  today's `find_by_mints_all`-parity behavior). Add `fetch_sim_history_one(mint, curve_only)`
  thin wrapper for the detect endpoint.
- Keep `SIM_PER_MINT_CAP = i64::MAX` (full history) and `warn_if_stale`.

### 2. Shared swing1 funnel builder (used by backtest + detect + probe)
New `lab/src/strategies/swing_1/funnel.rs` (or a fn on the existing decision module):
- Move `Swing1LowVerdict` / `Swing1LatchInfo` / `Swing1EntryInfo` / `Swing1ExitInfo` /
  `Swing1DetectResponse`'s **funnel core** out of the handler into a reusable
  `build_swing1_funnel(trades: &[impl TradeRow], rule) -> Swing1Funnel`.
- `Swing1Funnel { gate_configured, legs, lows, latch }` — the leg ledger + per-low verdicts +
  latch. Entry/exit stay where the backtest already computes them (avoid double-resolve).
- Generic over `TradeRow` so it runs over `CorpusTrade` (backtest) and `Trade` (probe) alike;
  `detect_swing_legs_raw` + `classify_phase` already take `TradeRow`.
- `swing_probe.rs` and `swing1_detect.rs` both call this one fn (kill the duplicated walk;
  the module doc already says "keep the two in lockstep").

### 3. (a) `swing1-detect` reads the lake
`lab/src/api/handlers/tokens/swing1_detect.rs`
- Replace `repo.find_by_mint_paged(&mint, SWING_DB_TRADE_CAP, 0)` with
  `fetch_sim_history_one(&mint, curve_only)`.
- Delete `SWING_DB_TRADE_CAP` / `MAX_TRADES_RETAINED` import — cap gone.
- `curve_only` → lake `Selection.curve_only` (handled inside `fetch_sim_history_one`).
- `window_start_ms`/`window_end_ms` → apply over the loaded `CorpusTrade` slice by
  `block_time` (port `filter_trades_to_window` to a `TradeRow`-generic form, or filter inline).
- Response identical shape → no frontend change to `swing1Detect.ts`.
- Add `warn_if_stale` (reuse from `sim_fetch`) so a today-token detect logs the same caveat.

### 4. (b) Carry the funnel in the swing1 backtest result
`lab/src/strategies/swing_1/backtest.rs`
- swing1 has its **own** `select_simulated_tokens` copy, so it can use a swing1-owned result
  without touching tpsl1. Stop `pub use`-ing tpsl1's `BacktestTokenResult`; define
  `Swing1BacktestTokenResult { #[serde(flatten)] base: BacktestTokenResult, funnel:
  Swing1Funnel, entry_index: Option<usize> }`. `#[serde(flatten)]` keeps every existing wire
  field so the shared result table/card is unchanged; the swing1 inspect modal reads the new
  `funnel`.
- Populate `funnel` from `build_swing1_funnel(trades, &rule)` (the `_trigger_idx` currently
  discarded becomes `entry_index`).
- Return type of `run_backtest` + the swing1 handler updates to the new struct.

### 5. Frontend — swing1 inspect draws the carried legs
- `TokenInspectModal` (tpsl1) is reused by swing1 but only builds entry/exit markers. Add a
  swing1 variant (or an optional `swingLegs` prop) that feeds the carried `funnel.legs` into
  `TokenTradeChart`'s swing overlay (`ChartSwingLeg[]`), instead of re-calling `swing1-detect`.
- Map backend `SwingLeg` → `ChartSwingLeg` (fields already align: `type`/`start_at`/`end_at`/
  `start_price`/`end_price` + pivots). Verify the pivot fields are present on `SwingLeg`.
- The standalone Swing1DetectPage keeps calling `swing1-detect` (now lake-backed) — unchanged.

## Out of scope (flagged, not done)

- **Generic `swing.rs` endpoints** (`detect_token_swings`, `detect_tokens_swings_batch`) —
  same `MAX_TRADES_RETAINED` cap, but a different analyzer (`detect_swings`) and a
  thousands-of-token PG fan-out batch that populates the tokens-list chain columns. Moving that
  to the lake is a separate decision. **Left on PG + 2,500 cap for now.** Note the inconsistency.
- **Chart candle source** — stays PG `GET /trades` (5,000). Today-token staleness accepted.

## Risks / parity

- **Today tokens**: lake stale → detect/legs truncated vs fresh PG candles. Covered by
  `warn_if_stale` log; acceptable per scope decision.
- **Parity**: the detect endpoint must produce the SAME legs the backtest does now that both
  read the lake — guaranteed by the shared `build_swing1_funnel` over the shared corpus. Keep
  the `duck::parity_tests` (`--ignored`) green; add a unit test that
  `build_swing1_funnel(corpus_trades)` == the backtest's carried funnel for a fixture token.
- **`venue` drop**: confirmed `curve_only` must go through `Selection.curve_only` (load-time),
  not `CorpusTrade` (no `venue`). Don't reintroduce `venue` on the slim row.

## DoD

- `cargo check -p lab` + `cargo check -p trading_core` clean; clippy on touched code.
- `npm run build` clean; swing1 inspect draws legs with no extra detect round-trip.
- Docs: update `@arch/strategies.md` (funnel now shared + lake-backed), `@arch/sweep.md` if the
  corpus loader signature changes, and this plan's status. CLAUDE.md data-scale note if the
  detect-endpoint source line changes.
