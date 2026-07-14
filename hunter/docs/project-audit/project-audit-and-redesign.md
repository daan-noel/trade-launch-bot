# Hunter Audit & Redesign — Status Reconciliation

**Original audit:** 2026-07-03 (code-first review of the six pre-monorepo crates + SPA).
**This rewrite:** 2026-07-14 — every original finding was re-verified against current code by a
six-agent sweep. This document now reflects **current reality**, not the 2026-07-03 snapshot.

> The full original blueprint is preserved verbatim in git history (this file's `f541b85`-era
> version). It was **not executed as written** — see "What actually happened" below. Finding IDs
> (C1, H7, M14 …) are kept so cross-references in other docs still resolve.

---

## What actually happened since 2026-07-03

The repo did **not** follow this plan. Three larger currents overtook it:

1. **Monorepo restructure** (`feat/restructure-hunter-forge`, now on `master`). `meme-trading → hunter/`,
   `SLP → forge/`, shared drop-in crates → `shared/`. Crucially the two god-crates were split along
   the exact seam this audit proposed — just under **different names**:
   - `pump-trader` → `shared/executor/core` (venue-agnostic engine, = the proposed `sol-executor`)
     + `shared/executor/pumpfun` (venue impl).
   - `ingest-laserstream` → `shared/ingest/core` (transport-only, = the proposed `ingest-grpc`)
     + `shared/ingest/pumpfun`.
   - The `Protocol` "false seam" (H9) was replaced by **real trait seams** — `Venue`
     (`shared/executor/core/src/venue.rs`) and `IngestVenue` (`shared/ingest/core/src/venue.rs`).

2. **Forge became a full product** and, being greenfield, **implemented this audit's venue/quote/
   schema-v2 vision natively**: integer `amount_quote`/`amount_base` columns, a `quote_assets`
   dimension, an open `launchpads` registry (`market_kind IN ('bonding_curve','amm','clmm','orderbook')`),
   quote-decimals in views. Everything H7/H8/B.2/B.3 asked for **exists in forge, not hunter.**

3. **A newer, forge-focused efficiency audit** (2026-07-13, `docs/forge-efficiency-audit-2026-07-13.md`)
   became the *active* audit and drove ~15 `perf(forge/*)` commits (SSE push, ingest decouple,
   watchdog, reconcile off request path). That is the live perf track today — this hunter doc is not.

**Net effect on this plan:** the *structural* recommendations (topology split, real venue seam) largely
landed. The *quote/multi-venue data model* landed **in forge**. The plan's **highest-priority items —
the Phase 0 real-money bugs marked "ship before anything else" — were unshipped until 2026-07-14, when
Part 1 (C1, M1, H2, M2, M3/M4, H1) was implemented** (see "Part 1 — SHIPPED" below; real-SOL smoke of
the sell/close paths still pending).

---

## Status of every original finding

Legend: ✅ done · 🟡 partial · ❌ open · ⚪ obsolete/moved. Paths are **current** (post-restructure).

| ID | Finding | Status | Current evidence / note |
|---|---|---|---|
| **C1** | Sell re-sends on non-reverted (Succeeded/Pending) tx — double-sell | ✅ (2026-07-14) | Fixed: `classify_sell_confirm` (`real.rs`) re-sends only on a confirmed same-route revert (or reverted route-change); Succeeded/Pending/RPC-error → extended feed-poll then terminal `ExitUnconfirmed` (new state, alarmed) — never a second sell. New tests pin it. |
| **C2** | Lab sweep threads capped at cores−6 | ✅ (2026-07-14) | `bounded_threads` (`sweep/registry.rs`) now defaults to `cores − 1` on `lab` (analysis box runs no ingest/gRPC/trader, so a sweep never co-runs the live hot path) — one core left for the OS + idle actix workers. `SWEEP_RAYON_THREADS` override + `≥1` floor kept; the live-hot-path rationale (`TOKIO_WORKER_THREADS`/`HTTP_WORKERS` reservation) dropped. |
| **H1** | Market-cap formula wrong + inconsistent | ✅ (2026-07-14) | Real `tokens.total_supply_token` column added (migration 0003, populated at ingest from the `total_supply_for` Rust SSOT; backfill + `token_overview` view + sync script updated). `MARKET_CAP_SQL` + `market_cap_sol` repointed at `total_supply × price`. `initial_supply_token` doc-comment clarified as NOT-supply. |
| **H2** | Ingest consumer couples strategy pings to DB backpressure | ✅ (2026-07-14) | `consumer.rs` now dispatches the cache update + reserve update + `ping_strategy` **before** the durable enqueue, and the Trade/Wallet/Token/Migration writes go through `enqueue_db_timeout` (500 ms bound + drop-metric). `notify_mint` deliberately stays after the Trade enqueue (sell-confirm ordering). |
| **H3** | Lake day-files seal with no completeness check | ❌ | `hunter/lab/src/lake/export.rs:129-139` still skip-if-exists, no manifest/row-count, no re-seal. Sync (`hunter/scripts/db-incremental-sync.ps1`) reconciles `wallet_dict` but has no per-day `COUNT(*)` check for `trades`. |
| **H4** | sim↔sweep/lake parity test `#[ignore]`d | ✅ | `hunter/lab/src/lake/duck.rs:452-524` `parity_tests` — un-ignored, self-skips when no lake present. (Proves sweep-load ↔ sim-load parity, not a lake-vs-PG baseline.) |
| **H5** | Corpus load row-by-row, re-done every sweep | ✅ (2026-07-14) | `sweep_corpus_cache` is now reused **at sweep start** (`grouped_sweep.rs`), not only the drill-in: `lake_hash` folds a **lake-version fingerprint** (partition set + token-dim mtime/len) so the cache key is `(selection, lake version)` — a same-selection re-run over an unchanged lake skips the DuckDB load; a fresh `lake-export` invalidates it. `stage_mints` now runs **once** (`resolve_candidates` stages `sel_mints` in both paths; `load_sync` no longer re-stages). Still the **row API by design** (arrow isolation) — the deferred arrow-batch path is the remaining optional "M". |
| **H6** | Single-rule simulate single-threaded, no `spawn_blocking` | ✅ (2026-07-14) | The per-token entry→exit resolve is extracted to a pure `resolve_token` and run via `rayon` `par_iter` inside `tokio::task::spawn_blocking` across all three backtests (`tpsl_sniper_1/2`, `swing_1`) — off the actix worker, across cores. The DuckDB `LakeSource::load` now runs its whole synchronous read inside `spawn_blocking` too (benefits sweep + simulate). `SIM_PER_MINT_CAP` stays `i64::MAX` (intentional full-history parity). |
| **H7** | Quote currency (SOL) fused into units/names — T1 blocker | ❌ hunter · ✅ forge | Hunter: pool PDA still hardcodes WSOL (`shared/ingest/pumpfun/src/pool.rs:47-56`); amounts still `f64` SOL via `/LAMPORTS_PER_SOL` (`shared/executor/core/src/engine.rs:438`); neutral `IngestEvent` still `sol: f64` (`shared/ingest/core/src/event.rs:26`); schema still `_lamports`/`_sol`. Forge implemented the full integer quote axis in its DB. |
| **H8** | Venue recognition = pump-only string match — T2/T4 blocker | ❌ hunter · ✅ forge | Hunter/ingest still substring `contains(pump_id)` (`shared/ingest/pumpfun/src/venue.rs:73-79`), `enum TxRelevance { Curve, Amm }`, `venue CHECK IN ('curve','amm')`. Forge has the open `launchpads` registry + `launchpad_id`/`market_kind`. |
| **H9** | `Protocol` builder is a false venue seam | ✅ | Replaced by real `Venue` / `IngestVenue` traits. `protocol.rs` demoted to a static constants/descriptor module in both stacks. (Still single-venue — `VenueId::PumpFun` is the only arm.) |
| **H10** | Strategy-code triplication across lab + core | 🟡 | A real `Strategy`/`ParamSpace` trait + generic sweep engine now exist (`hunter/lab/src/sweep/strategy.rs`), removing engine-level dup. But every concrete item is still duplicated: 3 sweep adapters (`tpsl1/tpsl2/swing1.rs`), `sweep_*`/`simulate_*_one_combo` clones in `registry.rs`, the `tpsl_sniper_1/2` decision clones (`util.rs` byte-identical), the frontend pages/columns, and **12 per-strategy sweep tables**. No `StrategyDescriptor`/`register_strategy!`/`sweep_dispatch<S>`. |
| **M1** | Shared token account, no refcount / same-mint serialization | ✅ (2026-07-14) | Rent-reclaim `close_token_account` now gated by `has_other_open_position_on_mint` (DB check — restart-safe, replaces the proposed in-memory refcount) at both close sites (`reclaim_token_account_if_last`); same-mint real exits serialized by `runtime.mint_exit_lock` (per-`(wallet,mint)` async Mutex). |
| **M2** | `mark_buy_submitted` DB persist held inside nonce slot | ✅ (2026-07-14) | Bounded in the `on_signed` hook (`real.rs`) with a 250 ms `tokio::time::timeout`; the shared executor stays DB-agnostic. In-memory `signed_slot` journal set synchronously first, so write-ahead recovery for this process is preserved even on timeout. |
| **M3** | `wallet_dict` intern uses no-op `DO UPDATE` (dead tuples) | ✅ (2026-07-14) | `wallet_dict_repo.rs` rewritten: cache-first, SELECT fast path → `INSERT … ON CONFLICT DO NOTHING RETURNING id` → SELECT fallback. No more dead-tuple churn on the hot write path. |
| **M4** | Confirm-loop wallet round-trip per call (2 RTs) | ✅ (2026-07-14) | Shared process-wide bounded `address→id` cache (`LazyLock<DashMap>`, cap 200k; ids are immutable so no invalidation) fronts `id_for`/`intern` — a resident id is 0 round-trips. |
| **M8** | `/api/tokens` re-filters/re-sorts ~1M rows per poll | ✅ | `hunter/core/src/state/token_list_cache.rs` — staleness-bounded shared snapshot, filter-by-reference, page-only clone. (Different mechanism than the proposed per-query memo, but the per-poll full clone+sort is gone.) |
| **M9** | Current-day analysis needs two uncoupled `--include-today` runs | ❌ | `sim_fetch.rs:144-161` still warn-only; `lake-export --include-today` exists but no chained `sync` command couples hop-1 (DB sync) to hop-2 (export). |
| **M10** | Legacy `Position` model kept only for SSE wire shape | ❌ | `hunter/core/src/models/position.rs` + `system/stream.rs:181-184` `PositionResponse::from(...)` adapter still present. |
| **M14** | FE boundary cross-imports; no lint enforcement | ✅ (2026-07-14) | ESLint flat config (`frontend/eslint.config.js`, `npm run lint`) with `no-restricted-imports` boundary zones (shared ⊬ `@live`/`@lab`; live ⊬ `@lab`; lab ⊬ `@live`) — boundary-only (the `@typescript-eslint`/`react-hooks` plugins are registered just so existing disable comments resolve; no rules enabled). Offenders relocated: the grouped-creation trio (`GroupedCreationSection`/`GroupedCreationTrendChart`/`groupedCreationStats`) + `BackgroundJobsIndicator` + `sweepParamColors` moved shared→`lab/` (the shared `creationStats.ts` stayed — `sharedEndpoints` uses it); the 3 lab strategy pages dropped the live-only `sellToken` (lab can't sell — no keys/trader). Lint is green (0 violations) and the rule fires on a probe. |
| **M15** | Shared SSE has no `onerror`/`onopen` — silent death | ✅ (2026-07-14) | `sse.ts` now tracks connection status (`connecting`/`open`/`error`) via `onopen`/`onerror`, exposes `subscribeSseStatus`/`getSseStatus` + a `useSseStatus` hook (header health dot), and an `onSseReopen` resync hook: the idempotent "refetch" subscribers (`connectTokenCreatedStream`, `connectTpslRulesChanged`) re-fire on a *reconnect* (error→open) so a page that missed frames during an outage catches up. |
| **M16** | `SwingDetectionPage` pulls up to 20k full records | ✅ (2026-07-14) | New lab-only `POST /api/tokens/mints` (`core::collect_filtered_mints` runs the SAME `q.matches` filter as `build_tokens_list`, projects `mint_address` only) + `labApi.getTokenMints` (reuses the shared `tokensTableRequestBody` builder — SSOT with `getTokensPage`). "Swing Detection All" reads the mint set from it instead of pulling ~20k full rows to `.map` down to mints. |
| **M17** | `PriceUnit` hard SOL/USD binary, one global rate | ❌ | `shared/types/index.ts:3` unchanged; `usePriceDisplay.ts` still hardcodes `◎`; `TokenRecord` has no quote field. Pairs with H7. |
| **M18** | Oversized components | ❌ (worse) | `TokenPriceChart.tsx` grew 1862→**1931** lines; lab strategy pages ~1187–1296. |
| **L9** | FE dead code (`getTokens`/`TransactionsPage` …) | ✅ | Removed; only doc references remain. |

Original items not individually re-verified this pass (assume **open** unless noted): M5–M7, M11–M13, L1–L8, L10–L11. M5 (pump-specific ingest contract) tracks with H8; M12 (`runtime_cache` in shared core) — still relevant since the crate move didn't relocate it to `live`.

**Docs↔code drift appendix:**
- `initial_supply_token` documented as "first creator buy" while `MARKET_CAP_SQL` treats it as supply — **FIXED 2026-07-14** (H1: `MARKET_CAP_SQL` now uses `total_supply_token`; `models/token.rs` comment clarified).
- `0001_init.sql:272` comment says `strategy_id` is `'tpsl1'|'tpsl2'` but the registry writes `tpsl_sniper_1/2` (`match_keys.rs:211`) — **FIXED 2026-07-14** (comment now `'tpsl_sniper_1' | 'tpsl_sniper_2' | 'swing1' | …`).
- `sharedEndpoints.ts` "Swing uses `getTokens`" — **FIXED/obsolete** (`getTokens` deleted; Swing uses `getTokensPage`).

---

## Revised priorities

Priority order is unchanged in spirit — **real-money safety > lab throughput > maintainability** — but
the redesign is now explicitly deprioritized below bugs/perf (forge is the multi-venue product; hunter
stays pump/SOL for now, adopting forge's model later — see Part 5).

> **Parts 1 & 2 shipped 2026-07-14 and their plan sections were removed as finished** — the
> outcome is recorded in the status table above (C1, M1, H2, M2, M3/M4, H1, C2, H6, H5 all ✅) and
> in commits `fb30590` (Part 1) / `4a95c7a` (Part 2). Part 1's only remaining task is the real-SOL
> smoke test of the sell/close paths.

### Part 3 — Modularity (H10 remainder) ⛔ DECLINED 2026-07-14 (intentional separation)

> **Decision (2026-07-14):** Part 3 is **deliberately not pursued.** A four-agent re-map of the
> actual code confirmed the "duplication" is mostly *intentional separation*, and the one item that
> touches real-money code (the decision-module merge) is actively unsafe. Part 3 is the
> explicitly-deprioritized maintainability tier; Parts 1–2 (the items that mattered) shipped. Revisit
> a specific item only if/when adding strategy #4 makes its copy-paste cost real. Per-item rationale:
>
> 1. **Merge `tpsl_sniper_1/2` decision modules — REJECTED (unsafe + fights an intentional design).**
>    `hunter/CLAUDE.md` already codifies these as *intentional clones* ("a fix in one usually belongs
>    in both"). The map found the divergence is **larger than this audit originally claimed** — not
>    just `ReserveSource {Virtual,Real}` (2 read sites in the E4 rung) + tpsl2's scalp entry, but also
>    a genuinely reimplemented exit fill-window (`find_trade_driven_exit_live` vs
>    `find_trade_driven_exit_with_slot`, which returns the fire slot) and a `CachedExitState`
>    `Copy`-derive difference; tpsl2's entry is a separate scalp path with 7 extra `Tpsl2Rule` fields.
>    A `ReserveSource` param alone will **not** unify them. Merging would rewrite **live real-money
>    sell-decision code (consumed via `exit_state.rs`) that has not yet had its Part 1 real-SOL smoke
>    test** — maintainability churn *against* real-money safety, inverting this repo's priority order,
>    to save an 18-line byte-identical `util.rs` clone. Not worth it.
> 2. **`sweep_dispatch<S>` / `StrategyDescriptor` + `register_strategy!` — DEFERRED.** This is the one
>    spot with genuine copy-paste (the `sweep_*`/`simulate_*_one_combo`/`sweep_base_rule_*` clones in
>    `hunter/lab/src/sweep/registry.rs`; the generic engine `strategy.rs`/`grouped_engine.rs` already
>    landed). But it's isolated and working; a macro trades the copy-paste for macro complexity. Only
>    worth it while actively adding strategy #4.
> 3. **Collapse the 12 sweep tables — REJECTED (no win).** The tables are byte-identical *but the Rust
>    repo (`GroupedSweepRepo`) is already fully strategy-blind* — the SQL is written once and
>    `format!`s the table name in, so there is **zero code duplication today**. The only "cost" is one
>    migration block per new strategy (rare); per-strategy tables also give clean per-strategy
>    retention/pruning. Collapsing would **drop existing sweep results** and add JSONB indirection for
>    no runtime or maintenance gain.
> 4. **Parametrized `StrategyLabPage` — DEFERRED (the only item with a real tax).** The 3×
>    ~1187–1296-line near-identical lab pages (`Tpsl1Page`/`Tpsl2Page`/`Swing1Page`) mean a bug gets
>    fixed in triplicate; the shared column base (`strategyColumns.tsx`) and the sweep layer's
>    `GroupedSweepView` thin-wrapper pattern already show the target shape. If any Part 3 item is later
>    picked up, this is the one with an ongoing cost — but it's a nice-to-have, not a need, and pairs
>    with M18.

### Part 4 — Frontend cleanup 🟡 (M14/M15/M16 shipped; M18/M17 deferred)

> **M14/M15/M16 shipped 2026-07-14 and their plan bullets were removed as finished** — the
> outcome is recorded in the status table above (M14, M15, M16 all ✅): ESLint import-boundary
> gate + offender relocation, SSE `onopen`/`onerror` status + reopen-resync, and the mints-only
> `POST /api/tokens/mints` behind "Swing Detection All". Remaining, both deferred:

- **M18 — extract `TokenPriceChart` (1931 lines)** and the strategy pages ❌ *deferred* — pairs with
  Part 3.4 and New-plan #1's fork-vs-share (hunter/forge) decision, which must be made first.
- **M17 — quote-aware price display** ❌ *deferred* with Part 5 (needs the quote axis).

### Part 5 — Venue/quote/schema-v2 redesign (future hunter goal) 🔵 *deferred*

Kept as a goal, **below** Parts 1–4. Forge already implements this end-to-end, so when hunter pursues it
the work is **"port forge's model back to hunter," not "design anew"**: adopt forge's `quote_assets` /
`launchpads` dimension tables, integer `amount_quote`/`amount_base` columns, quote-decimals-in-views, and
open venue registry. Structural prerequisites already partly met in hunter (executor/ingest split, real
`Venue`/`IngestVenue` traits). Still required if pursued: a `venue-core`-style shared unit-type layer
(`QuoteUnit`/`QuoteAmount`/`BaseAmount`/`CurveModel`), a program-id→decoder registry / `AnyVenue`
dispatch (today `VenueId` is a single `PumpFun` arm), un-hardcoding the WSOL pool seed (`pool.rs:47-56`),
and flipping workspace `resolver = "1" → "2"` for per-crate feature unification. Acceptance test unchanged:
a USDC-paired pump token (T1) must be addable without touching anything outside venue config + data.

---

## New plans to add (not in the original audit)

1. **Cross-product SSOT drift (hunter ↔ forge).** The restructure created deliberate duplication that can
   now silently diverge. Highest-risk: forge's token chart is a **separate fork** of hunter's
   (`forge/frontend/src/shared/components/tokenChart/` vs `hunter/frontend/src/shared/components/token-price-chart/`,
   already diverging — hunter's grew to 1931 lines with swing overlays, forge's is swing-stripped). Also
   the curve-math constants and pump program IDs now live in both products. **Plan:** inventory the
   hunter/forge duplicated facts; for each, either extract to a `shared/` module or add a guard test that
   asserts the copies stay equal (the CLAUDE.md SSOT rule). A chart refactor (M18) must decide fork-vs-share
   up front.

2. **Adopt forge's model as the reference for Part 5.** Rather than re-deriving the venue/quote design,
   write the hunter port as a diff against forge's shipped schema (`forge/migrations/0001_init.sql`
   `quote_assets`/`launchpads`) and forge's quote-decimals-in-views pattern. This retires the original
   B.2/B.3 design sections wholesale.

3. **Doc hygiene after the restructure.** `hunter/docs/arch/*` and `docs/plans/*` still use pre-restructure
   crate names in prose (`pump-trader`, `trading_core`, `ingest-laserstream`, `live/src/...`). Evidence
   citations across docs point at moved paths. **Plan:** a mechanical rename pass (`trading_core→hunter/core`,
   `pump-trader→shared/executor/*`, `ingest-laserstream→shared/ingest/*`, `live/→hunter/live/`).

4. **Track the newer forge audit separately.** `docs/forge-efficiency-audit-2026-07-13.md` is the active
   perf track for forge and is out of scope here; this document covers **hunter** only. Don't double-plan
   the forge SSE/ingest/reconcile work — it's already in flight there.

---

## Dependency / sequencing

```
Part 1 (real-money, no deps)  ──►  ✅ SHIPPED; only real-SOL smoke of sell/close paths remains
Part 2 (lab throughput)       ──►  ✅ SHIPPED
Part 3 (modularity)           ──►  ⛔ DECLINED — intentional separation; revisit per-item at strategy #4
Part 4 (frontend)             ──►  M14/M15/M16 ✅ SHIPPED; M18 waits on New-plan #1 fork decision, M17 on Part 5
Part 5 (venue/quote port)     ──►  deferred; port from forge when prioritized
New-plan #1 (SSOT drift)      ──►  do before any hunter/forge shared refactor (esp. chart M18)
```

Nothing in Parts 4–5 should merge ahead of Part 1's real-SOL smoke test. **Next real work: the
Part 1 sell/close real-SOL smoke test — the only genuinely open risk in this audit.**
