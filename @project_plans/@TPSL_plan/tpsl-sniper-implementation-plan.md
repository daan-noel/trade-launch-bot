# TPSL Launch-Sniper Strategy — Per-Feature Implementation Plan

Each feature below is **one param = one self-contained, independently testable unit**. Implement and test them **one at a time** (do the shared plumbing + the feature's sim logic + its test, run the sim, read the effect, then start the next). Strategy rationale: see `pumpfun-sniper-strategy-research.md`.

---

## Shared plumbing checklist (repeat once per new column)

Every param is wired the same way; all are **inert by default** (`0 / NULL / false = disabled`, per the `ignore_zero_*` convention in `backend/src/strategies/tpsl/util.rs`).

- **Migration** — add the column (own migration file per feature for isolation, e.g. `backend/migrations/0009_*.sql`, `0010_*.sql`, …; or batch into one if preferred). `ALTER TABLE strategy_TPSL_rules ADD COLUMN IF NOT EXISTS …`. Auto-applies on restart via `sqlx::migrate!` (`backend/src/storage/postgres.rs:14`).
- **Model** — add field to `StrategyTPSLRule` + `new()` (`backend/src/models/strategy_tpsl_rule.rs`).
- **Repo** — add to `StrategyTPSLRuleDbRow`, `From`, and the INSERT / SELECT(×2) / UPDATE lists (`backend/src/storage/repositories/strategy_tpsl_rule_repo.rs`). bigint↔u64, f64 direct, bool direct.
- **API** — add to `RuleResponse`(+`From`), `CreateRuleRequest`, `UpdateRuleRequest` (`backend/src/api/handlers/strategies/tpsl.rs`); nullable numerics use `Option<Option<T>>`, bools `Option<bool>`.
- **Frontend** — add to the rule type (`frontend-react/src/types/index.ts`) and the form (`frontend-react/src/components/tpsl/RuleFormModal.tsx`: `RuleFormData`/`emptyForm`/`formFromRule`/one input/`buildCreatePayload`/`buildUpdatePayload`).

## One-time prerequisites (do with the first feature that needs them)

- **[P-exit] Exit walk skeleton** — replace `find_exit` with `simulate_exit(trades, entry, rule)` that walks post-entry trades chronologically (already sorted — `trade_repo.rs:203`) holding running `peak_price`, `peak_reserves`, `last_higher_high_time`. The first **Exit** feature builds this; later exit features just add one check into the loop. (`backend/src/strategies/tpsl/simulation_tpsl.rs`)
- **[P-entry-1] Entry struct** — extend `find_entry` to return `{price, tx, time, slot, entry_liquidity_sol}` instead of the 3-tuple. Needed by entry-liquidity / wash / velocity.
- **[P-entry-2] TokenInfo map** — load `TokenInfoRepo::list_all()` (`token_info_repo.rs:156`) into `HashMap<mint, TokenInfo>`. Needed by exclude-rugged / wash-volume / creator-rep.
- **[P-entry-3] Signals helper** — `compute_entry_signals(trades, creator_wallet)`; grows by one field per entry feature.
- **[P-display] Reasons + summary** — add new `exit_reason` strings, render them in `tableColumns.tsx`, and switch `SimSummaryCard.tsx` win/loss to **pnl-sign** classification. Do this alongside the first exit feature.

---

## EXIT LOGIC FEATURES

### E1 · Trailing stop — `p_trailing_stop_pct` (f64) ← start here (also builds **[P-exit]** + **[P-display]**)

- **Does:** banks a reversal — exit when price falls X% below the peak-since-entry.
- **Sim logic:** in the walk, track `peak_price`; trigger when `price ≤ peak_price·(1 − pct/100)` → `exit_reason = "TrailingStop"`.
- **Plumbing:** shared checklist + build `simulate_exit` skeleton + display.
- **Test:** unit test over a hand-built `Vec<Trade>` that runs +200% then reverses → exits ~peak·(1−pct), not "Open". Run sim → "Open" count drops, TRAIL appears.

### E2 · Time stop / max-hold — `p_time_stop_secs` (bigint)

- **Does:** cut positions that neither moon nor crash; memes die in minutes (set ~5–15 min, not hours).
- **Sim logic:** trigger at first trade with `block_time ≥ entry_time + p_time_stop_secs` → `"TimeStop"`.
- **Plumbing:** shared checklist (add one check to the existing walk).
- **Test:** flat price series longer than N → exits at the deadline trade. Run sim → long-tail "Open" tokens become TIME.

### E3 · Stall / momentum-death — `p_stall_secs` (bigint)

- **Does:** sell into the flatline — no new higher-high for N seconds.
- **Sim logic:** track `last_higher_high_time` (updated when `price > peak_price`); trigger when `block_time − last_higher_high_time ≥ p_stall_secs` → `"Stall"`.
- **Plumbing:** shared checklist.
- **Test:** series that peaks then trades flat past N → STALL fires at the right trade; a series making steady new highs does **not** stall.

### E4 · Liquidity-death exit — `p_liquidity_drop_pct` (f64)

- **Does:** the real killer — bail when liquidity is being pulled (reserve crash, 30–90s), which price stops miss.
- **Sim logic:** track `peak_reserves` (`virtual_sol_reserves`); trigger when `reserves < peak_reserves·(1 − pct/100)` → `"LiquidityExit"`. (Optional: if token `is_rugged` and still open at end, mark −100%.)
- **Plumbing:** shared checklist.
- **Test:** series with rising reserves then a sharp drop → LIQ fires on the drop trade. Run sim → rugged tokens resolve as LIQ instead of "Open".

> **Priority when several fire on the same trade:** LiquidityExit → StopLoss → TakeProfit → TrailingStop → Stall → TimeStop. (StopLoss/TakeProfit already exist; keep them.)

---

## ENTRY LOGIC FEATURES

### N1 · Exclude rugged — `p_exclude_rugged` (bool) ← start here (also builds **[P-entry-2]**)

- **Does:** never enter a token already flagged rugged.
- **Sim logic:** after `token_matches_rule`, skip if `p_exclude_rugged && info.is_rugged`.
- **Plumbing:** shared checklist + load TokenInfo map.
- **Test:** sim with flag on → rugged mints disappear from the matched set; off → unchanged.

### N2 · Liquidity floor at entry — `p_min_liquidity_sol` (f64) (also builds **[P-entry-1]**)

- **Does:** avoid tokens too thin to exit without destroying price.
- **Sim logic:** skip if `entry.entry_liquidity_sol < p_min_liquidity_sol`.
- **Plumbing:** shared checklist + extend `find_entry` to return entry liquidity.
- **Test:** two synthetic tokens (high vs low entry reserves) → only the thin one is filtered as the floor rises.

### N3 · Dev block-0 buy cap — `p_max_dev_block0_pct` (f64) (also builds **[P-entry-3]**)

- **Does:** reject when the creator front-loaded too much supply (you'd be exit liquidity).
- **Sim logic:** `dev_block0_pct = creator tokens bought in creation slot ÷ TOKEN_TOTAL_SUPPLY` (`config/constants.rs`); skip if `> p_max_dev_block0_pct`.
- **Plumbing:** shared checklist + add field to `compute_entry_signals`.
- **Test:** trades where creator buys 15% in slot 0 → filtered at cap 10; a 3% creator buy passes.

### N4 · First-slot bundle share cap — `p_max_first_slot_bundle_pct` (f64)

- **Does:** the core bundle filter — total supply bought by **all** wallets in the creation slot. (More wallets in slot 0 = worse, not "more diverse".)
- **Sim logic:** `first_slot_bundle_pct = Σ token_amount of buys in creation slot (and first 1–2 slots) ÷ TOKEN_TOTAL_SUPPLY`; skip if `> cap`.
- **Plumbing:** shared checklist + signals field.
- **Test:** one slot-0 cohort taking 40% → filtered at cap 25; trickle-in buyers across many slots pass.

### N5 · Bundle retained-supply cap — `p_max_bundle_held_pct` (f64)

- **Does:** overhang risk — how much the launch-slot cohort **still holds** at entry.
- **Sim logic:** for wallets present in the creation slot, net (buys − sells) up to entry time ÷ TOKEN_TOTAL_SUPPLY; skip if `> cap`.
- **Plumbing:** shared checklist + signals field.
- **Test:** bundle that still holds most of its slot-0 buy → filtered; a bundle that already sold out → passes.

### N6 · Wash-trade guard — `p_max_volume_per_liq` (f64)

- **Does:** skip manufactured volume (big `volume` on tiny liquidity).
- **Sim logic:** skip if `info.volume / entry.entry_liquidity_sol > p_max_volume_per_liq`.
- **Plumbing:** shared checklist (uses TokenInfo map + entry liquidity).
- **Test:** high-volume / low-liquidity synthetic → filtered; balanced one passes.

### N7 · vSOL velocity floor — `p_min_vsol_velocity` (f64)

- **Does:** the strongest academic predictor — fast liquidity accumulation in few trades.
- **Sim logic:** `vsol_velocity = Δvirtual_sol_reserves over the first window ÷ seconds (or ÷ trades)`; skip if `< floor`.
- **Plumbing:** shared checklist + signals field.
- **Test:** fast-accumulating series passes; slow grinder is filtered as the floor rises.

### N8 · Confirmation window — `p_confirm_window_secs` (bigint)

- **Does:** entry **timing** mode — enter the first qualifying trade at/after the window instead of block 0–1 (prerequisite for N9; lowers rug exposure at small upside cost).
- **Sim logic:** when `> 0`, `find_entry` picks the first trade with `block_time ≥ first_trade_time + window`; when `0`, keep block-0 entry.
- **Plumbing:** shared checklist + branch in `find_entry`.
- **Test:** verify entry trade/price shifts to the post-window trade; `0` reproduces current behavior.

### N9 · Organic continuation floor — `p_min_organic_sol` (f64) (depends on N8)

- **Does:** the real demand signal — net buying **after** the window from wallets **absent** at launch.
- **Sim logic:** `organic_sol = Σ net buy SOL in slots after the window from wallets not seen in the creation slot`; skip if `< floor`.
- **Plumbing:** shared checklist + signals field.
- **Test:** token with only launch-slot buyers → filtered; one with fresh post-window buyers → passes.

### N10 · Skip rugged creator — `p_skip_rugged_creator` (bool)

- **Does:** reputation prior — avoid creators who have rugged before.
- **Sim logic:** build creator rug-rate by grouping loaded tokens on `creator_wallet` + `is_rugged`; skip if flag on and the creator's prior rug-rate is high (or any prior rug).
- **Plumbing:** shared checklist + creator-rep map (from already-loaded tokens + TokenInfo).
- **Test:** synthetic creator with a prior rugged mint → its new mint filtered; clean creator passes.

---

## Cross-cutting / later (not per-param)

- **Recalibrate existing TP/SL** to the minutes reality (these columns already exist).
- **Realism pass:** slippage vs reserves, priority/Jito fees, fill latency, −100% rug floor (paper must ≈ real).
- **Scale-out partials + bankroll model:** reworks one-entry→multi-fill result schema; separate effort.

## Suggested order

Exit **E1 → E2 → E3 → E4**, then Entry **N1 → N2 → N3 → N4 → N5 → N6 → N7 → N8 → N9 → N10**. Exit first makes the metrics honest (resolves "Open" ghosts) so you can actually measure whether each entry filter improves results.

## Per-feature verification

- `cd backend && cargo test tpsl` (the feature's unit test) → `cargo build`; `cd frontend-react && npm run build`.
- Restart backend (migration applies) → set **only** the new param in the rule form → run the simulation → confirm the expected change (an exit reason appears / the matched set shrinks) and that leaving it at 0/NULL/false reproduces the prior run.
