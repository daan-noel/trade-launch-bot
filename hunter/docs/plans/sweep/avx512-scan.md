# AVX-512 exit-scan + frontend toggle — implementation plan

**Status:** IMPLEMENTED 2026-07-19 (P0–P4 landed + parity-verified on the AVX-512
workstation). **Remaining: the P0 wall-clock share + P5 A/B speedup numbers**, which
need one representative grouped-sweep run on the workstation lake — see *Measured
numbers* at the bottom for the exact commands. Design settled 2026-07-19.

## What shipped (read this first on resume)

| Phase | Delivered |
| --- | --- |
| **P0** | `sweep_pass` `Stage` timer wraps the grid/random fold (`grouped_engine.rs`) so every run — not just refine runs — logs the corpus-load-vs-fold split the Amdahl gate reads. **Deliberately did NOT** add a per-`resolve_exit` clock (millions of calls × N threads = a hot-path violation + it perturbs the very measurement); the corpus-vs-fold stage split + the `sweep-fold-hot-path-waste` audit already establish that `resolve_exit` dominates the fold, and a sampling profiler gives the exact within-fold share with zero hot-path cost. |
| **P1** | `resolve_exit_simd` + the `first_exit_row` / `first_exit_row_avx512` / `first_exit_row_scalar` kernel (`generic/strategy.rs`). Stable-Rust `#[target_feature(enable="avx512f")] unsafe fn`, runtime-gated. **Correction to the design prose:** the price column is `f64` (`MetricSeries.price: Vec<f64>`), so the width is **8 lanes** (`__m512d`), not the 16×`f32` the plan assumed — everything else holds. Finite lanes are `|p| ≤ f64::MAX` (exactly `is_finite`); dead lanes are built scalar so the kernel needs only `avx512f` (no BW/VL). |
| **P2** | `registry::{USE_SIMD, set_use_simd, use_simd, avx512_available}` (mirrors `set_ram_reserve_mb`); dispatch branch in `GenericSweepStrategy::resolve_exit`; handler CPU gate + forced-scalar `sweep_notice` toast in `start_grouped_sweep`. |
| **P3** | Parity proven **on the real kernel** (the i9-11900F has `avx512f`, so the tests exercise the vector path, not the fallback): `guard::simd_exit_scan_matches_scalar_across_paths` (end-to-end `TokenOutcome` equality over TP+SL / TP-only / SL-only / dead-open / deferred-entry / metrics-fallback rules × the corpus + gappy fixtures) + `strategy::tests::simd_*` (kernel vs scalar at every block boundary, remainder tail, and NaN/±inf lanes). `cargo test -p hunter-lab sweep::generic` → **31 passed**. |
| **P4** | `use_avx512?: boolean` on the request type; `useAvx512` config + On/Off toggle beside the RAM-reserve radio in `GenericSweepConfigForm.tsx` (omit-when-default, default **Off**); `SWEEP_FIELD_HELP.avx512` help text. `npm run build:lab` + `npm run lint` clean. |

Toolchain confirmed **stable** (`rustc 1.96.1`, no `rust-toolchain.toml`) — AVX-512
intrinsics + `#[target_feature]` are stable since 1.89, so the `std::simd` nightly
question is moot; the `#[target_feature]` form is what shipped.

## Original plan (below) — kept for the design rationale

Goal: make the grouped sweep's per-`(combo × token)` exit scan run on the CPU's
AVX-512 vector unit, behind a **frontend toggle** (use / don't-use), with a parity
guard proving the vectorized path is byte-identical to the scalar one.

## Why (context a fresh session needs)

- The sweep's hot loop is [`resolve_exit`](../../../lab/src/sweep/generic/strategy.rs)
  (`hunter/lab/src/sweep/generic/strategy.rs:753`) — for each `(combo × token)` it
  walks the token's trade series looking for the first exit (stop-loss / take-profit
  crossing, dead, or metric condition). Per the sweep-fold audit, `resolve_exit` is
  the dominant cost center (NOT `prepare_token`).
- The SL/TP crossing test is `price <= entry·(1−sl)` / `price >= entry·(1+tp)` over a
  contiguous `f32` price column — the canonical "scan an array for the first element
  crossing a threshold" pattern, which AVX-512 does 16-wide with native mask
  registers + first-lane (`tzcnt`).
- **`MetricSeries` is already Structure-of-Arrays** (`series.price[j]`, `series.dead[j]`,
  `series.at[j]`, `series.slot[j]` are separate columns) — the layout SIMD needs is
  already there. That's why this is high-leverage, not a rewrite.
- **Hardware confirmed** on the workstation (2026-07-19, ran `is_x86_feature_detected!`):
  i9-11900F (Rocket Lake, 8c/16t) has `avx512f`, `avx512vl`, `avx512dq`, `avx512bw`,
  `avx512vnni` — all **detected active at runtime** (BIOS-enabled). 16×`f32` per instr.
- This is **lab-only**. lab never ships to EC2 (2vCPU/4GB), so targeting AVX-512 is free
  of the deploy constraint. A CPU-feature gate still forces scalar on any box lacking it.

**Why the CPU (AVX-512), not the GPU:** the only GPU on the box is a GTX 1660 SUPER
(6 GB VRAM, ~5 TFLOPS) — too weak/small to beat the 8-core AVX-512 CPU by enough to
justify a **second copy of the exit/cost math** in CUDA (an SSOT violation +
permanent parity-test burden). AVX-512 vectorizes only the *search for the exit
index*; the money math (`round_trip_with_costs`) and the aggregator (`RunAgg`) stay
the one shared copy in `trading_core::strategies::kernel`, untouched. See the
`sweep-fold-hot-path-waste` and `sweep-sim-ssot-divergence` memories.

## Diagram A — runtime data flow (where the toggle value travels)

```
┌───────────────────────────────────────────────────────────────┐
│ FRONTEND  ·  lab sweep form (GenericSweepConfigForm.tsx)       │
│                                                                │
│   RAM reserve  ( 4G  2G  ●1G  512M )   ← existing radio        │
│   AVX-512      ( ●On   ○Off )          ← NEW toggle            │
│                     │                                          │
│                     ▼   use_avx512: true                       │
│   POST /api/strategies/sweeps  { ...params, use_avx512 }       │
└──────────────────────────┬─────────────────────────────────────┘
                           │ HTTP
                           ▼
┌───────────────────────────────────────────────────────────────┐
│ BACKEND  ·  hunter-lab                                          │
│                                                                │
│  sweep handler (api/handlers/strategies/grouped_sweep.rs)      │
│    1. read use_avx512 from request body                        │
│    2. gate: is_x86_feature_detected!("avx512f") ?              │
│         ├─ no  → force OFF + SweepObserver::notice (toast)     │
│         └─ yes → honor the toggle                              │
│    3. registry::set_use_simd(effective)                        │
│         └─ process-global, mirrors set_ram_reserve_mb          │
│                     │                                          │
│                     ▼                                          │
│  run_grouped_sweep → engine, per (combo × token):              │
│       resolve_entry                                            │
│       GenericSweepStrategy::resolve_exit ──dispatch──┐         │
│              ├─ use_simd()==false ─► resolve_exit       (scalar)│
│              └─ use_simd()==true  ─► resolve_exit_simd  (AVX512)│
│                              │                                 │
│                              ▼                                 │
│              round_trip_with_costs · RunAgg                    │
│              ↑ SHARED kernel — runs ONCE per token, UNCHANGED  │
└───────────────────────────────────────────────────────────────┘
```

## Diagram B — build/verify phases

```
 P0          P1               P2            P3            P4            P5
 MEASURE     BUILD SIMD       DISPATCH      PARITY        FRONTEND      SHIP
 ────────    ────────────     ─────────     ──────────    ─────────     ───────
 phase       resolve_exit_    set_use_simd  scalar ==     toggle in     A/B
 timers  ──► simd (find-  ──► + CPU     ──► simd byte ──► sweep form ──► speedup
 → scan      first-cross)     feature       identical     + notice      number
   share     annotated fn     gate          guard test    wiring        + docs
```

## Locked design decisions (do not re-litigate)

1. **Vectorize the search, not the arithmetic.** `resolve_exit_simd` only finds the
   first exit *row*; `round_trip_with_costs` / `RunAgg` stay the shared kernel copy.
   SSOT surface = "did the vector scan find the same first-crossing row as scalar?" —
   provable with one property test.
2. **The toggle IS the verification harness.** Default it **Off** until P3 proves
   parity; then flip the default to **On**. First job: same sweep, on vs off → assert
   identical results + compare wall-clock.
3. **Process-global flag, not threaded param.** Mirror `registry::set_ram_reserve_mb`
   exactly (`hunter/lab/src/sweep/registry.rs:156-178`): single-flight means one run's
   choice is live, rayon workers read it lock-free from any depth. Add
   `USE_SIMD: AtomicBool`, `set_use_simd(Option<bool>) -> bool`, `use_simd() -> bool`.
4. **Runtime-gated, in one native binary.** Use an annotated
   `#[target_feature(enable="avx512f,avx512dq,avx512vl,avx512bw")] unsafe fn`
   selected at runtime — NOT a compile-time `target-cpu=native` switch (that gives no
   toggle). The `is_x86_feature_detected!` gate makes calling it safe.
5. **Don't persist `use_avx512` on the run row.** Like `ram_reserve_mb`, it's a
   property of *how the box computed*, not of the analysis. Log it in the start log +
   notice only. Keeps sweep results comparable regardless of path.
6. **P0 is the gate.** If the scan is a small share of wall-clock (e.g. DuckDB corpus
   load dominates), the toggle still works but the headline speedup is Amdahl-capped.
   Measure before investing in P1.

## Exact touch points (mirror the RAM-reserve feature — it's the template)

### Backend

| File | Change | Mirror |
| --- | --- | --- |
| `hunter/lab/src/sweep/registry.rs` | Add `USE_SIMD: AtomicBool` + `set_use_simd(Option<bool>)->bool` + `use_simd()->bool` + a `avx512_available()` helper wrapping `is_x86_feature_detected!("avx512f")`. Default flag = **false**. | `RAM_RESERVE_MB` block, lines ~152-178 |
| `hunter/lab/src/api/handlers/strategies/grouped_sweep.rs` | Add `#[serde(default)] pub use_avx512: Option<bool>` to the request body struct (the one carrying `token_cap`/`ram_reserve_mb`, ~lines 95-124). In the handler, right after the `set_ram_reserve_mb` call (~line 360): gate on `registry::avx512_available()`, `set_use_simd`, `tracing::info!`, and emit a `SweepObserver::notice` if the toggle was requested-on but forced-off. | `ram_reserve_mb` field + `set_ram_reserve_mb` call site |
| `hunter/lab/src/sweep/generic/strategy.rs` | Add `resolve_exit_simd(...)` (same signature as `resolve_exit` at line 753). Dispatch in `GenericSweepStrategy::resolve_exit` (line 272-280): branch on `registry::use_simd()` to call scalar or simd. | free fn `resolve_exit` (753), impl call site (280) |

Dispatch stays inside `GenericSweepStrategy::resolve_exit` (strategy.rs:280) — the
engine (`engine.rs:57` calls `strategy.resolve_exit`) and the `Strategy` trait are
untouched.

### Frontend (`hunter/frontend/src/lab/components/sweep/`)

| File | Change | Mirror (`ramReserve`) |
| --- | --- | --- |
| `groupedTypes.ts` | Add `use_avx512?: boolean;` to the request type | `ram_reserve_mb?: number;` (line 212) |
| `GenericSweepConfigForm.tsx` | (a) `useAvx512: boolean` on `GenericSweepConfig` (near line 115); (b) default `useAvx512: false` (line 141); (c) destructure `useAvx512` (line 273); (d) payload `use_avx512: useAvx512 ? true : undefined` (near line 346); (e) a toggle `<Field label="AVX-512">` beside the RAM-reserve `<Field>` (lines 432-439); (f) `SWEEP_FIELD_HELP.avx512` help text | `ramReserveMb` / `RAM_RESERVE_CHOICES` radiogroup |

Note the RAM-reserve pattern only sends the field when it differs from the default
(`ram_reserve_mb: ramReserveMb !== DEFAULT_RAM_RESERVE_MB ? ... : undefined`) — the
`groupedTypes.ts` type keeps it optional. Follow the same omit-when-default shape.

## Phase steps

### P0 — Measure (the gate; cheap)
- Extend `SweepClock` (`hunter/lab/src/sweep/obs.rs:127`) / `SweepProgress`
  (`progress.rs`) with wall-clock timers on the three phases (`corpus → coarse →
  sweep`) plus a sub-timer accumulating time in `resolve_exit`.
- Run one representative grouped sweep; log the split. **Decision:** if the exit-scan
  is a meaningful share (say >30% of wall-clock), proceed to P1; else stop and
  reconsider (DuckDB load or fold is the real target).

### P1 — Build `resolve_exit_simd`
- Same signature/semantics as `resolve_exit` (strategy.rs:753). Fast path = pure
  TP/SL (the common tpsl case): load 16 `f32` prices, broadcast-compare against the
  SL threshold **and** the TP threshold, OR the masks with the `dead` lane, `tzcnt`
  the combined mask to find the first crossing row in the 16-wide block; handle the
  `< 16` remainder scalar.
- At the found row, classify the exit code with the **same priority order** as scalar
  (`Dead > StopLoss > TakeProfit > Metrics`) and call the shared `closed(...)` /
  `round_trip_with_costs` — do not reimplement pricing.
- **Metrics fallback:** when `has_exit_metrics` is true (arbitrary
  `reqs_any_satisfied` conditions), fall back to the scalar loop for that combo. Most
  tpsl combos are pure TP/SL, so the vectorized path covers the bulk.
- **Implementation form (stable Rust):** annotate
  `#[target_feature(enable="avx512f,avx512dq,avx512vl,avx512bw")] unsafe fn` and write
  the 16-wide block loop so LLVM emits AVX-512 (no nightly needed). `std::simd`
  (`Simd<f32,16>` + masks) is cleaner but **nightly-only** — only use it if the repo is
  already on nightly. Wrap the `unsafe fn` in a safe caller guarded by
  `registry::avx512_available()`.

### P2 — Dispatch + CPU gate
- Wire `set_use_simd` / `use_simd` (registry.rs) and the handler gate (grouped_sweep.rs).
- The gate: if `use_avx512` requested but `!avx512_available()`, force scalar and
  `notice` "AVX-512 unavailable on this host — running scalar" (rides the existing
  `sweep_notice` SSE → info toast).

### P3 — Parity guard (SSOT safety net; non-negotiable)
- Test near `guard.rs:351` (which already drives `resolve_exit` per token): run BOTH
  `resolve_exit` and `resolve_exit_simd` over the same corpus/combos and
  `assert_eq!` every `TokenOutcome` field (exit code, prices, times, slots, pnl).
- Cover the edge cases: early exit on row 1, exit inside a 16-block, exit in the
  remainder tail, never-exits (`Open`), `dead` mid-block, and a metrics-fallback combo.
- Prefer a no-DB synthetic-series test so it runs on every `cargo test -p hunter-lab`.

### P4 — Frontend toggle
- The `groupedTypes.ts` + `GenericSweepConfigForm.tsx` changes above.
- Verify `npm run build:lab` clean + `npm run lint` clean (import-boundary gate).

### P5 — Ship
- A/B: same sweep, toggle On vs Off → assert identical group/best-combo results,
  record wall-clock delta. Update `docs/arch/sweep.md` (add a row to the resource-fence
  / driver table noting the optional AVX-512 exit-scan) and finalize this file with the
  measured numbers.

## Definition of done
- [x] `cargo check -p hunter-lab` clean; clippy on touched code clean; parity tests green (31 passed).
- [x] `npm run build:lab` + `npm run lint` clean.
- [x] Toggle On vs Off produces **byte-identical** results — P3 proves it on the real
  AVX-512 kernel (`guard::simd_exit_scan_matches_scalar_across_paths`). **A/B wall-clock
  delta: PENDING a workstation run** (see *Measured numbers*).
- [x] CPU-feature gate: `registry::avx512_available()` forces scalar + emits the
  `sweep_notice` toast when `avx512f` is absent (handler `start_grouped_sweep`).
- [x] Docs updated (arch resource-fence row + this file).

## Measured numbers (PENDING — needs one workstation run)

Neither headline number can come from a code review; both need one representative
grouped sweep over the local Parquet lake. To capture them:

1. **P0 Amdahl gate** — run any real grouped sweep and read the log:
   `milestone=corpus_loaded elapsed_s=…` (DuckDB load) vs the `stage=sweep_pass secs=…`
   line vs `milestone=done elapsed_s=…` (total). If `sweep_pass / total > ~0.3`, the
   exit-scan speedup is worth banking; if DuckDB load dominates, the toggle still works
   but the win is Amdahl-capped. **Record the split here.**
2. **P5 A/B** — run the **same** sweep twice (identical axes/range/caps), AVX-512 **Off**
   then **On**. Assert the group/best-combo results are identical (P3 already guarantees
   this) and record the `stage=sweep_pass secs` ratio as the speedup. **Record it here.**

> Result (fill in): corpus_load = __ s · sweep_pass(scalar) = __ s · sweep_pass(avx512)
> = __ s · total = __ s → scan share = __% · speedup = __×.

## Open questions / gates
- **A/B speedup magnitude** is the one unknown left. `f64`/8-lane caps the theoretical
  per-instruction win at 8×, and the scan is partly memory-bound (streaming `price`/`dead`),
  so expect roughly 2–4× on the `sweep_pass` stage, Amdahl-capped by (1) above.
- Does the metrics-condition path (`has_exit_metrics`) deserve its own vectorization
  later, or is the pure-TP/SL fast path enough? Deferred until the A/B numbers show its
  share — the current build sends metric-exit combos down the scalar `resolve_exit`.
- **Other scan consumers** (raised 2026-07-19): the single-combo **drill-in**
  (`simulate_generic_one_combo` → `scan` → `resolve_exit`) shares the exact scan and could
  ride the same toggle cheaply, but it is one combo × tokens (already trivially cheap), so
  low value. The single-rule **simulate** (`replay::run_replay`) is the live-engine
  `hunter_engine::reduce` **fold**, a *different* stateful implementation — this kernel does
  NOT apply to it, and vectorizing the fold is a separate, SSOT-sensitive effort. See the
  note appended below.

## Follow-up: does this help *simulate*? (2026-07-19)

Two lab surfaces run exit logic, and they do NOT share one implementation:

- **Sweep + single-combo drill-in** use the **scan** (`generic::strategy::resolve_exit`
  over a precomputed `MetricSeries`). This kernel vectorizes exactly that. The sweep is
  the big win (many combos × tokens); the drill-in reuses the same `scan` but is one
  combo, so wiring the toggle to it is cheap-but-marginal.
- **Single-rule simulate / backtest** use `replay::run_replay` → the shared
  `hunter_engine::reduce` **fold** (the live-engine SSOT). It is stateful and sequential
  per token, not the "find the first array crossing" shape AVX-512 exploits, and forking
  it would be a *second copy of the exit math* — the exact SSOT trap the plan's GPU
  rejection called out. Vectorizing simulate is therefore a separate, larger project, not
  a toggle flip.

### Simulate is now instrumented (measure before optimizing) — 2026-07-19

Before touching the simulate fold, we need to know *where its seconds go* — the same
Amdahl discipline as the sweep's P0 gate. `run_engine_backtest` (`strategies/engine_sim.rs`)
now wraps each phase in the shared `obs::Stage` timer, log-only (analysis path, not the
hot path):

| Stage | Phase | Cache-backed? |
| --- | --- | --- |
| `sim_scan` | candidate token scan (`scan_matched_candidates`) | yes (TTL + single-flight) |
| `sim_load` | lake trade histories (`get_or_fetch_histories_state`) | yes (TTL + single-flight) |
| `sim_replay` | the single-threaded `reduce` fold + row build | no |
| `sim_enrich` | fired-token DB enrichment (`fetch_enrichment`) | no |
| `sim_backtest_total` | whole backtest after the concurrency permit | — |

Read one run's split: `grep 'stage=sim_' <lab-log>` → `stage=… secs=…`. Because scan/load
are cache + single-flight backed, a **cold** first run weights `sim_load` (DuckDB/lake read)
and a **warm** re-run weights `sim_replay` (the fold). That cold-vs-warm split is exactly
what decides the right tool:

- `sim_load` dominates ⇒ columnar/cache work (AVX-512 irrelevant to simulate).
- `sim_replay` dominates ⇒ the win is **precompute-per-token** (the sweep's own trick —
  build the `MetricSeries` once per token instead of recomputing inside the fold), *not*
  SIMD, because the fold is stateful/sequential and is the live-engine SSOT.

**PENDING:** one representative simulate run to fill in the split and pick the target.

## How to resume
1. This feature is code-complete through P4. The only work left is the **Measured
   numbers** section above (one workstation A/B run) and deciding the simulate follow-up.
2. Structural template if extending: the RAM-reserve feature (`registry::set_ram_reserve_mb`
   ↔ `set_use_simd`; the form's RAM-reserve radio ↔ the AVX-512 toggle).
