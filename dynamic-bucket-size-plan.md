# Dynamic bucket size for continuous grouping fields — plan

## Status (2026-07-02)

**NOT STARTED — design/recommendation only.** The static version is live: the
continuous SOL-amount group fields bucket at a **hardcoded 0.1 SOL** width
(`trading_core` `grouping::SOL_BIN_WIDTH`). This plan makes that width
**runtime-configurable** per grouped sweep / dashboard query.

Current binned fields (5): `InitialBuySol`, `MaxCostLamports`,
`SpendableLamportsIn`, `FirstSlotBuySol`, `FirstSlotSellSol`. Discrete fields
(`CuLimit`, `CuPrice`, `IsCashbackEnabled`, `TokenProgramId`, `IxLabels`) group
on exact value.

## Goal

Let the user pick the bucket width at grouping time instead of recompiling.
Motivating problem: **the SOL fields span very different magnitudes** — an
`initial_buy_sol` is usually 0–5 SOL (0.1 is a good width), but a
`first_slot_buy_sol` can be 0–50+ SOL (0.1 shatters it into hundreds of groups).
One hardcoded width can't serve all of them, and can't be tuned per analysis.

## Guiding principle — the binning seam is already isolated

All bucketing lives in exactly **two mirror functions** that turn a value into a
group-key string; everything downstream (partition, aggregate, tables, metrics)
only ever sees the resulting string:

- **Rust (sweep):** `grouping::bin_sol_label(v, width, decimals)` →
  `render_field` → `group_key`.
- **SQL (dashboard):** `creation_stats_repo::sol_bin_sql(sol_expr)` →
  `group_field_sql` → `grouped()`.

So "dynamic width" = **thread a width value into those two functions.** No engine,
metric, or storage-schema change beyond persisting the chosen width for
reproducibility. This is why it's cheap.

## The options

| Opt | UX | Backend data model | Effort | Handles per-field magnitude? | Generalizes to CU fields? |
| --- | --- | --- | --- | --- | --- |
| **A** | One width per run | single `f64` | S | ❌ (one width for all) | ❌ |
| **B** | Width per field | `Map<GroupField, f64>` | M | ✅ | ✅ |
| **C** | Preset dropdown (coarse/med/fine) | single `f64` | XS | ❌ | ❌ |
| **D** | Auto/quantile (equal-population) | derived | L | ✅ (data-driven) | n/a |

## Recommendation — **B's data model, phased UI (A first)**

**Build the backend for per-field widths (a `Map<GroupField, f64>`), but ship the
UI as a single global width input first, then add per-field overrides as a fast
follow.** Concretely:

1. **Backend = per-field width map from day one.** The plumbing cost of a
   `HashMap<GroupField, f64>` vs a bare `f64` is negligible, and it is the
   *correct* model — it's the only one that solves the magnitude problem and
   later lets `CuLimit`/`CuPrice` bucket too (in their own units). Choosing A's
   `f64` now means a schema + signature rewrite later; choosing B's map now costs
   almost nothing extra and never needs redoing.

2. **UI = phased.** Phase 1 exposes **one** "bucket size (SOL)" number that the
   frontend fans out to every bucketed field (identical width for all) — this
   covers the 80% need ("try 0.1 vs 0.5") with a single input. Phase 2 adds an
   optional per-field width box (next to each field's existing filter input) that
   overrides the global default. Same request shape both phases; only the form
   grows.

3. **A field is bucketed iff it has a width.** Drop the hardcoded "which fields
   bucket" set on the backend: any field present in the width map with `w > 0`
   buckets (in its natural unit); absent ⇒ exact. This makes the binned set
   data-driven and lets the user *opt* `cu_price` into bucketing without a code
   change. The frontend keeps a default-width map (SOL fields → 0.1, others
   unset) so behavior is unchanged out of the box.

Why not the others:
- **A / C** are cheaper UI but bake in the one-width-for-all limitation that is
  the entire reason to do this — the SOL fields' magnitudes differ too much. C
  also can't express an arbitrary value.
- **D (auto/quantile)** is powerful but changes grouping *semantics* (unequal or
  data-dependent widths), breaks trivially-reproducible group keys unless the
  resolved edges are persisted, and complicates the Rust/SQL parity guarantee.
  Defer; revisit only if fixed-width tuning proves insufficient.

## Correctness — the one real risk: Rust ⇄ SQL label parity for arbitrary widths

The static version keeps `bin_sol_label` and `sol_bin_sql` byte-identical by
hand (shared `+1e-9` epsilon, 1-decimal `to_char '0.0'` ⇔ `{:.1}`, en-dash). For
a *dynamic* width both sides must **derive the same decimal precision** and build
the same label:

- **Decimals from width** must match: `0.5 → 1`, `1.0 → 0`, `0.25 → 2`,
  `5.0 → 0`. Add one helper each side (`decimals_for(width)`), unit-tested against
  a shared table of `(width, expected_decimals)`.
- **SQL `to_char` format** becomes dynamic: build `'FM99999990'` + (`.` + `'0'*d`)
  from `decimals`. Interpolated width/decimals are validated `f64`/`usize` (never
  user text) → injection-safe.
- **Epsilon** stays `1e-9` on the ratio (safe for any width ≥ ~1e-6 SOL; reject
  widths below that).
- **Parity test (blocking):** an `--ignored` test that, for a grid of widths
  (`0.05, 0.1, 0.25, 0.5, 1, 5`) × edge values, asserts `bin_sol_label(...)`
  equals what the SQL bin returns for the same value (run the SQL expression via
  a scratch query or replicate its integer math in Rust and cross-check). Until
  green, "identical labels" is unproven — this is the analogue of the existing
  `continuous_fields_bucket_into_ranges` test, widened to dynamic widths.

## Validation

- `width` must be finite and `> 0`; reject `≤ 0`, `NaN`, `Inf`. Clamp to a sane
  floor (e.g. `≥ 1e-6 SOL`) so the epsilon stays valid and labels don't explode
  in decimals.
- Missing/empty width map ⇒ current defaults (SOL fields at 0.1) so old
  clients + old persisted runs render unchanged.

## Implementation sketch

### Backend (`trading_core`)
- `grouping.rs`:
  - `render_field(fp, field, widths: &BinWidths)` + `group_key(fp, fields, widths)`
    — thread the map. `BinWidths` = thin wrapper over `HashMap<GroupField, f64>`
    with a `width_of(field) -> Option<f64>` and the default map.
  - `bin_sol_label(v, width, decimals)` already takes width/decimals — just stop
    passing the constant. Add `decimals_for(width)`.
  - Keep `SOL_BIN_WIDTH`/`SOL_BIN_DECIMALS` as the **defaults** the frontend and
    any width-less caller use.
- `creation_stats_repo.rs`: `sol_bin_sql(sol_expr, width, decimals)` +
  `group_field_sql(field, widths)`; `grouped(...)` takes the width map and passes
  it down. Both the gkey expr and the filter predicate use the same binned expr
  (already true today).
- `grouping.rs` grouped-sweep + dashboard request DTOs: parse an optional
  `bin_widths: {field_tag: number}` map alongside `group_by`.

### Sweep engine + handler (`lab`)
- `sweep/grouped_engine.rs::partition` calls `group_key(&tt.fp, fields, widths)` —
  thread `widths` from the run config.
- `api/handlers/strategies/grouped_sweep.rs`: accept `bin_widths` on the start
  args; store on the run; pass to `partition`. (Optional) make
  `matches_field_filter` bucket-aware for consistency, or leave exact + document.

### Persistence (`lab/migrations`)
- Add `bin_widths jsonb` to each `<strategy>_grouped_sweep_runs` table (or fold
  into the existing `grouping_spec`), so a saved run reproduces its exact
  grouping. Dashboard queries are live (request-carried), no persistence needed.

### Frontend (`frontend-react`)
- `groupedTypes.ts`: `SOL_BUCKET_WIDTH` (default) already exists; add
  `binWidths?: Record<GroupField, number>` to `GroupedSweepStartArgs` +
  `GroupedSweepRunRecord` (echo back). Dashboard `GroupedCreationArgs` too.
- `FingerprintGroupPicker.tsx`: Phase 1 — one "bucket size (SOL)" input in the
  header; Phase 2 — per-field width box (reuse the row's right slot). Update the
  legend/marker (already reads `SOL_BUCKET_WIDTH`) to reflect the active width(s).
- `GroupedCreationSection.tsx` + `groupedCreationStats.ts`: thread the width(s)
  into the dashboard request.

## Stages

1. **Backend width-map plumbing** — `BinWidths` + `decimals_for`; `render_field`/
   `group_key`/`sol_bin_sql`/`group_field_sql`/`grouped` take widths; defaults
   preserve current behavior. `cargo check -p trading_core` + existing grouping
   tests green (defaults unchanged).
2. **Parity test (blocking)** — dynamic-width Rust⇄SQL label parity, `--ignored`.
3. **Sweep wiring + persistence** — thread widths through `partition` + the start
   handler; `bin_widths jsonb` migration; run record echoes it. `cargo check -p lab`.
4. **Frontend Phase 1 (global width)** — single input, request field, legend
   reads the active width. `npm run build`.
5. **Frontend Phase 2 (per-field)** — optional per-field override boxes.
6. **Docs + memory** — `@arch/sweep.md` (dynamic width), this file's decisions,
   memory note if the width becomes a standing part of the sweep contract.

## Open decisions (for the user)

- [ ] **Confirm B-backend / phased-UI** (vs plain A, or C presets).
- [ ] **Default width** when the user sets none — keep `0.1`?
- [ ] **CU fields:** allow opting them into bucketing (their own width/unit), or
  keep them exact-only? (The width-map model supports either.)
- [ ] **Sweep value-filter** on binned fields: make it bucket-aware, or leave the
  known exact/bucket split documented?

## Cross-plan note

Independent of `simulate-lake-migration-plan.md` and
`token-first-slot-activity-plan.md` — touches only the grouping seam
(`grouping.rs`, `creation_stats_repo.rs`, sweep handler, picker UI), none of the
lake/trades files those plans own. No re-export needed (binning is render-time,
not stored in the lake).
