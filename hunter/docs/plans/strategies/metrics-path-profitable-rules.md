# Metrics path — finding profitable rules (authority backtest)

Operational plan for **generic metric + partial-exit rules** on **validated
playbook fingerprints**. Session handoff context lives in
[fingerprint-rule-handoff.md](fingerprint-rule-handoff.md); proxy-driven knobs are
in [mx-metric-rules.md](mx-metric-rules.md). Wallet-copy ladders (`fs*`) are a
separate calibration track ([wallet-analysis.md](wallet-analysis.md)).

---

## Definition of "profitable" (promotion bar)

| Gate | Setting |
| --- | --- |
| **Authority** | Lab **`simulate_one_combo`** / full rule simulate — not Python reserve-walk |
| **Cost** | **`CostModelKind::PumpfunImpact`**, **`FillModel::Worst`**, `buy_amount_sol` = intended live notional |
| **OOS** | Train/validate split on token **`created_at`** ([`validate.rs`](../../../lab/src/discovery/validate.rs) Layer 3; discovery pipeline splits **before** fit) |
| **Reject** | Large train→validate score drop, `ThinValidate` / `NoFireValidate`, profit factor ≤ 1 on **worst** fill |
| **Sample** | Enough closed episodes; inspect worst mints (breakage-trap playbooks) |

Read [execution-costs.md](execution-costs.md) before comparing runs across dates — fee/impact
constants changed 2026-07-28.

---

## Phase 0 — Playbook list (PG)

**Goal:** clusters of mints that share a dev **create recipe**, with enough trades to sim.

1. Run `_local/rule-research/scripts/mine-playbook-clusters.sql`
   (tune `since` / `until`, `min_mints`, `min_median_trades`).
2. Pick one **first-swing** cluster (high median trades, stable mint count) — start from
   **exact** `ix_labels` on an exemplar mint; widen only if fire rate is too low.
3. Tag **breakage-trap** families manually (chart review or adversarial exemplars like
   `Fb6shLknTdApxiTmT4muVubHSxMM1HsWke1mQwVypump`) — rules for those need tighter liq
   bands, **`scale_out`**, **no re-entry** ([handoff §4](fingerprint-rule-handoff.md)).
4. Optional quick proxy rank inside a cluster: `_local/rule-research/scripts/analyze_dip_hot_neutral.py` — **relative only**.

**Outputs:** shortlist of `(ix_labels, init_buy bucket)` rows + 1–3 exemplar `mint_address` each.

---

## Phase 1 — Corpus per fingerprint

```powershell
cargo run -p hunter-lab -- lake-export
# optional same-day: cargo run -p hunter-lab -- lake-export --include-today
# or: scripts/db-incremental-sync.ps1 -ExportLake
```

Set **`SWEEP_LAKE_DIR`** in `hunter/.env` (see `.env.example`). Re-export after PG sync.

**Save a fingerprint row** for the chosen playbook:

- Hand in UI, or
- `_local/rule-research/scripts/seed-fp-playbook-from-mint.sql`
  (set exemplar mint + name), then verify with lab token list / engine match.

All simulate, grouped sweep, and metric discovery scope by **`fingerprint_id`** (engine
`matches` SSOT — [sweep arch § corpus](../../arch/sweep.md)).

---

## Phase 2 — Search entry/exit

Two sanctioned paths (re-rank with sweep; **promote with simulate**):

### A. Metric discovery (automated screen → family grid → OOS)

- **API:** `POST /api/strategies/metric-discovery` ([`metric_discovery.rs`](../../../lab/src/api/handlers/strategies/metric_discovery.rs)).
- **UI:** lab → Metric Discovery page → **Open as sweep** on survivors.
- **Scope:** `fingerprint_id` + date window + `buy_amount_sol` + baseline TP/SL for Layer 1.
- **Deliverable:** `SweepSeed` (narrowed axes + TP/SL menus) — not a final rule.

Discovery **does not** grid `scale_out` today; use mx/fs5 ladder shapes as fixed exit
templates, then sweep entry axes only, or hand-author `scale_out` after entry grid settles.

### B. Grouped sweep (full grid)

- Scope run with same `fingerprint_id`, `pumpfun_impact`, worst fill, live buy size.
- Grid entry metrics (`m_snapshot`, `m_price_window.trail`, `m_flow_window.gross_flow`, …)
  with **`off`** per axis ([axis-value-candidates.md](../sweep/axis-value-candidates.md)).
- Exit: compare **`scale_out`** ladder vs full TP/SL using rule JSON from
  [mx-metric-rules.md](mx-metric-rules.md) / [fs5-practice-rules.md](fs5-practice-rules.md).

**Default search policy:** **no `reentry`** unless the playbook clearly supports multiple
swings; re-entry compounded losses in neutral proxy ([mx-metric-rules.md](mx-metric-rules.md)).

---

## Phase 3 — Validate

1. Re-run **simulate** on top sweep/discovery combos (`POST /api/strategies/simulate` or
   probe) — **worst** and **first** fill (tail sensitivity).
2. Run discovery Layer 3 semantics explicitly if you fit on full corpus: split
   **`split_fraction`** ~0.7 on `created_at` ([`pipeline.rs`](../../../lab/src/discovery/pipeline.rs)).
3. Per-token drill-in on worst losses; check floor/breakage entries ([handoff §4](fingerprint-rule-handoff.md)).

**Hypothesis seeds (until sim proves otherwise):** apply
`_local/rule-research/scripts/seed-mx-metric-rules.sql` for **`mx-*`**
paper rules — primary **`mx-00`**, controls **`mx-01`/`mx-02`/`mx-04`**.

---

## Phase 4 — Guards

Prefer existing metric vetoes (liq band, shallower trail, shorter `m_position.held`,
`scale_out` bank tranche). If sim still buys in **near-creation-floor** regime, spec
**liquidity drawdown** entry veto (new metric) only after failure mode is visible on
simulate rows — do not add engine fields preemptively.

---

## Phase 5 — Live

1. Seed **`paper`**, **`is_active=false`**, prefix e.g. **`fp-*`** (playbook-scoped) or promote **`mx-*`** after sim.
2. Small size; **`max_concurrent_tokens`** matched to bankroll (2–3 SOL → 0.10 SOL × conc 4 per mx draft).
3. Compare live slippage to **worst-case** sim before arming real.

---

## Command cheat sheet

```powershell
# Phase 0
psql "$env:DATABASE_URL" -f hunter/scripts/mine-playbook-clusters.sql

# Phase 1
cargo run -p hunter-lab -- lake-export

# Phase 0 → saved fingerprint (edit mint + name inside file first)
psql "$env:DATABASE_URL" -f hunter/scripts/seed-fp-playbook-from-mint.sql

# Hypothesis paper rules (mx track)
psql "$env:DATABASE_URL" -f hunter/scripts/seed-mx-metric-rules.sql

# Lab bin (simulate / discovery / sweep UI)
cargo run -p hunter-lab
cd hunter/frontend; npm run dev:lab
```

---

## Artifact map

| Phase | Artifact |
| --- | --- |
| 0 | `mine-playbook-clusters.sql` |
| 1 | Parquet under `SWEEP_LAKE_DIR`, `fingerprints` row |
| 2 | Discovery run and/or grouped sweep run id + `SweepSeed` |
| 3 | Simulate results + validate verdicts |
| 4–5 | `strategy_rules` paper rows, operator sign-off |

---

## Open operator inputs

See [fingerprint-rule-handoff.md §8](fingerprint-rule-handoff.md): date window, target
playbook exemplar, live buy size, scale_out preferences, mx vs fp-only seeds.

---

## Phase 0 snapshot (local PG, 2026-07-29)

Window `2026-07-22` .. `2026-07-28`, filters `min_mints=8`, `min_median_trades=200`:

- **Largest clusters by total trades** skew **`init_buy [12.8, 25.6)`** + distinct
  `ix_labels` (high-volume dev recipes) — not the mx **`[0, 6.4)`** band. Treat those as
  separate playbooks; do not assume one dip-hot rule fits all.
- **`[0, 6.4)` collapsed** query (second result set in the mining script) surfaces
  smaller but mx-aligned families — pick an **`exemplar_mint_high_trades`** there for
  `seed-fp-playbook-from-mint.sql` when moving off init_buy-only **`mx-dev small`**.
- **Adversarial mint** `Fb6shLkn…` is in PG: ~3.95 SOL init buy, 961 trades, dead —
  use for breakage-trap drill-in after sim (handoff §4).

Lake export on this box: **`SWEEP_LAKE_DIR=./lake-data`**, 7 sealed days present,
130803 token dimension rows. **`mx-*` rules seeded** — ready for lab simulate.
