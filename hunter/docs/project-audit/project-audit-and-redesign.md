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
landed. The *quote/multi-venue data model* landed **in forge**. But the plan's **highest-priority items —
the Phase 0 real-money bugs marked "ship before anything else" — were never shipped.** They are still
live in the hunter trading path.

---

## Status of every original finding

Legend: ✅ done · 🟡 partial · ❌ open · ⚪ obsolete/moved. Paths are **current** (post-restructure).

| ID | Finding | Status | Current evidence / note |
|---|---|---|---|
| **C1** | Sell re-sends on non-reverted (Succeeded/Pending) tx — double-sell | ❌ | `hunter/live/src/strategies/execution/real.rs:860` still `_ => SwapRetryDecision::Retry`. Buy path *did* get the symmetric guard (`classify_silent_send`, `real.rs:59-73`); sell path did not. No `ExitUnconfirmed` state exists. **Still a real-money hazard.** |
| **C2** | Lab sweep threads capped at cores−6 | ❌ | `hunter/lab/src/sweep/registry.rs:222-240` still `cores − (4 tokio + 2 http)`; docstring still cites the live hot path lab never runs. |
| **H1** | Market-cap formula wrong + inconsistent | 🟡 | SSOT now exists (`hunter/core/src/storage/token_enrichment.rs:42` `MARKET_CAP_SQL`, mirrored by `config/constants/token_math.rs:56` `market_cap_sol`) so SQL and in-RAM **agree** — but on the **still-wrong** value: formula is `price × initial_supply_token`, and `initial_supply_token` is still the dev's first-buy (`models/token.rs:21`), not supply. No real `total_supply` column. |
| **H2** | Ingest consumer couples strategy pings to DB backpressure | ❌ | `hunter/live/src/ingest/consumer.rs:233-234` awaits `enqueue_db` before `ping_strategy` (`:282`). A lossy `enqueue_db_lossy` exists (`:392-402`) but is used only for Metrics/Raw, not the Trade/Wallet/Migration writes that gate the pings. |
| **H3** | Lake day-files seal with no completeness check | ❌ | `hunter/lab/src/lake/export.rs:129-139` still skip-if-exists, no manifest/row-count, no re-seal. Sync (`hunter/scripts/db-incremental-sync.ps1`) reconciles `wallet_dict` but has no per-day `COUNT(*)` check for `trades`. |
| **H4** | sim↔sweep/lake parity test `#[ignore]`d | ✅ | `hunter/lab/src/lake/duck.rs:452-524` `parity_tests` — un-ignored, self-skips when no lake present. (Proves sweep-load ↔ sim-load parity, not a lake-vs-PG baseline.) |
| **H5** | Corpus load row-by-row, re-done every sweep | 🟡 | Redesigned to lake/DuckDB (PG `DbSource` retired). Still uses the **row API by design** (arrow-version isolation, `duck.rs:14-16`); sweep still reloads unconditionally (`grouped_sweep.rs:426`); `sweep_corpus_cache` is read only by the drill-in, not the next sweep; `stage_mints` still runs twice (`duck.rs:109,170`). |
| **H6** | Single-rule simulate single-threaded, no `spawn_blocking` | ❌ | `hunter/lab/src/strategies/tpsl_sniper_2/backtest.rs:209` still a sequential `for` loop on the async worker; `SIM_PER_MINT_CAP` still `i64::MAX` (`sim_fetch.rs:34`, now intentional for full-history parity). |
| **H7** | Quote currency (SOL) fused into units/names — T1 blocker | ❌ hunter · ✅ forge | Hunter: pool PDA still hardcodes WSOL (`shared/ingest/pumpfun/src/pool.rs:47-56`); amounts still `f64` SOL via `/LAMPORTS_PER_SOL` (`shared/executor/core/src/engine.rs:438`); neutral `IngestEvent` still `sol: f64` (`shared/ingest/core/src/event.rs:26`); schema still `_lamports`/`_sol`. Forge implemented the full integer quote axis in its DB. |
| **H8** | Venue recognition = pump-only string match — T2/T4 blocker | ❌ hunter · ✅ forge | Hunter/ingest still substring `contains(pump_id)` (`shared/ingest/pumpfun/src/venue.rs:73-79`), `enum TxRelevance { Curve, Amm }`, `venue CHECK IN ('curve','amm')`. Forge has the open `launchpads` registry + `launchpad_id`/`market_kind`. |
| **H9** | `Protocol` builder is a false venue seam | ✅ | Replaced by real `Venue` / `IngestVenue` traits. `protocol.rs` demoted to a static constants/descriptor module in both stacks. (Still single-venue — `VenueId::PumpFun` is the only arm.) |
| **H10** | Strategy-code triplication across lab + core | 🟡 | A real `Strategy`/`ParamSpace` trait + generic sweep engine now exist (`hunter/lab/src/sweep/strategy.rs`), removing engine-level dup. But every concrete item is still duplicated: 3 sweep adapters (`tpsl1/tpsl2/swing1.rs`), `sweep_*`/`simulate_*_one_combo` clones in `registry.rs`, the `tpsl_sniper_1/2` decision clones (`util.rs` byte-identical), the frontend pages/columns, and **12 per-strategy sweep tables**. No `StrategyDescriptor`/`register_strategy!`/`sweep_dispatch<S>`. |
| **M1** | Shared token account, no refcount / same-mint serialization | ❌ | `real.rs:136-144,787-794` still fire-and-forget `close_token_account` on any single exit; no `(wallet,mint)` refcount, no per-mint mutex. Compounds C1. |
| **M2** | `mark_buy_submitted` DB persist held inside nonce slot | ❌ | `shared/executor/pumpfun/src/trader/buy.rs:314-321` still awaits the hook inline before send; no timeout, not spawned. |
| **M3** | `wallet_dict` intern uses no-op `DO UPDATE` (dead tuples) | ❌ | `hunter/core/src/storage/repositories/wallet_dict_repo.rs:27,68` still `ON CONFLICT DO UPDATE SET address=EXCLUDED.address RETURNING id`; `id_for` SELECT (`:82`) exists but isn't tried first. |
| **M4** | Confirm-loop wallet round-trip per call (2 RTs) | 🟡 | Still a separate `id_for` per query (`trade_repo.rs:402,448,505,692,746`), no cache/CTE — but downgraded from an intern write to a read-only SELECT, so the WAL/dead-tuple cost is gone. |
| **M8** | `/api/tokens` re-filters/re-sorts ~1M rows per poll | ✅ | `hunter/core/src/state/token_list_cache.rs` — staleness-bounded shared snapshot, filter-by-reference, page-only clone. (Different mechanism than the proposed per-query memo, but the per-poll full clone+sort is gone.) |
| **M9** | Current-day analysis needs two uncoupled `--include-today` runs | ❌ | `sim_fetch.rs:144-161` still warn-only; `lake-export --include-today` exists but no chained `sync` command couples hop-1 (DB sync) to hop-2 (export). |
| **M10** | Legacy `Position` model kept only for SSE wire shape | ❌ | `hunter/core/src/models/position.rs` + `system/stream.rs:181-184` `PositionResponse::from(...)` adapter still present. |
| **M14** | FE boundary cross-imports; no lint enforcement | ❌ (worse) | **No ESLint config exists at all** (only `knip.json`). Offenders remain and multiplied: shared→`@lab` in 4 files (`GroupedCreationSection.tsx:8-13`, `BackgroundJobsIndicator.tsx:3-8`, …), lab→`@live` in 3 pages. |
| **M15** | Shared SSE has no `onerror`/`onopen` — silent death | ❌ | `hunter/frontend/src/shared/services/sse.ts:80-93` still event-listeners only; no status signal, no resync on reconnect. |
| **M16** | `SwingDetectionPage` pulls up to 20k full records | ❌ | `sharedEndpoints.ts:61` `TOKENS_LIST_LIMIT=20_000`; `SwingDetectionPage.tsx:588` still one-shot (now discards to mints client-side, server still ships the rows). |
| **M17** | `PriceUnit` hard SOL/USD binary, one global rate | ❌ | `shared/types/index.ts:3` unchanged; `usePriceDisplay.ts` still hardcodes `◎`; `TokenRecord` has no quote field. Pairs with H7. |
| **M18** | Oversized components | ❌ (worse) | `TokenPriceChart.tsx` grew 1862→**1931** lines; lab strategy pages ~1187–1296. |
| **L9** | FE dead code (`getTokens`/`TransactionsPage` …) | ✅ | Removed; only doc references remain. |

Original items not individually re-verified this pass (assume **open** unless noted): M5–M7, M11–M13, L1–L8, L10–L11. M5 (pump-specific ingest contract) tracks with H8; M12 (`runtime_cache` in shared core) — still relevant since the crate move didn't relocate it to `live`.

**Docs↔code drift appendix:**
- `initial_supply_token` documented as "first creator buy" while `MARKET_CAP_SQL` treats it as supply — **STILL TRUE** (root of H1; `models/token.rs:21` vs `token_enrichment.rs:42`).
- `0001_init.sql:272` comment says `strategy_id` is `'tpsl1'|'tpsl2'` but the registry writes `tpsl_sniper_1/2` (`match_keys.rs:211`) — **STILL TRUE** (comment only).
- `sharedEndpoints.ts` "Swing uses `getTokens`" — **FIXED/obsolete** (`getTokens` deleted; Swing uses `getTokensPage`).

---

## Revised priorities

Priority order is unchanged in spirit — **real-money safety > lab throughput > maintainability** — but
the redesign is now explicitly deprioritized below bugs/perf (forge is the multi-venue product; hunter
stays pump/SOL for now, adopting forge's model later — see Part 5).

### Part 1 — Real-money & correctness (ship first; still open) 🔴

These are the original Phase 0 items. They never shipped and remain the top of the list.

1. **C1 — sell re-send guard.** Make `classify_sell_revert` (`real.rs:850-862`) symmetric with the
   already-shipped buy guard `classify_silent_send` (`real.rs:59-73`): re-send **only** on a confirmed
   on-chain revert; `Succeeded`/`Pending`/RPC-error → keep polling (feed + `getSignatureStatuses`) on an
   extended deadline, then mark the position `ExitUnconfirmed` and alarm — never fire a second sell. **S.**
2. **M1 — token-account refcount + same-mint exit serialization.** `DashMap<(wallet,mint), AtomicU32>`;
   `close_token_account` only at zero; per-mint async `Mutex` around real exits. Closes the cross-position
   oversell path C1 rides on. **S/M.**
3. **H2 — decouple strategy pings from DB backpressure.** Dispatch the ping + reserve update *before*
   `enqueue_db`; give the hot-path Trade enqueue a `send_timeout` + drop-metric. Stops a PG hiccup from
   stalling real exits (and the watchdog from being the de-facto backpressure handler). **S.**
4. **M2 — nonce-hold during persist.** Make `mark_buy_submitted` fire-and-forget (spawn, in-memory
   journal preserves the write-ahead guarantee) or add a 250 ms timeout; hold the nonce slot only for
   sign+send. **S.**
5. **M3/M4 — wallet_dict hot path.** `intern`: SELECT fast path → `INSERT … ON CONFLICT DO NOTHING`;
   add an in-process LRU of hot ids, which also collapses the confirm-loop's 2 RTs (M4). **S.**
6. **H1 — market-cap formula (display correctness).** Add a real `total_supply_raw` column; repoint
   the single `MARKET_CAP_SQL` + `market_cap_sol` SSOT (already unified) at `total_supply × price`.
   The SSOT plumbing is done — only the underlying quantity is wrong. **S.**

### Part 2 — Lab throughput (open) 🟡

1. **C2 — rayon sizing.** Default `SWEEP_RAYON_THREADS = cores − 1` on lab; drop the live-hot-path
   rationale. One-line-class, ~3.5× on the 8-core box. **S.**
2. **H6 — parallel simulate + `spawn_blocking`.** `par_iter` the per-token resolve; wrap DuckDB load +
   resolve in `spawn_blocking` so they stop occupying an actix worker. **S/M.**
3. **H5 — corpus cache reuse.** Key `sweep_corpus_cache` by (Selection hash, lake version) and check it
   *before* `LakeSource::load` on sweep start (not just drill-in); fix `stage_mints`-twice. Arrow batch
   load remains deferred (deliberate row-API isolation). **S** (cache) / **M** (arrow, optional).

### Part 3 — Modularity (H10 remainder) 🟡

The generic engine landed; the concrete collapse did not. Remaining, in leverage order:
1. Merge `tpsl_sniper_1/2` decision modules into one `tpsl_sniper` with a `ReserveSource {Virtual,Real}`
   param (the only real delta besides tpsl2's scalp entry); keep `tpsl_sniper_1/2` as strategy-id presets
   + a parity test vs recorded v1 outputs. Deletes the byte-identical `util.rs` clone.
2. Collapse the 3 sweep adapters + `sweep_*`/`simulate_*_one_combo` clones behind `sweep_dispatch<S>` /
   a `StrategyDescriptor` + `register_strategy!` macro.
3. Collapse the **12 per-strategy sweep tables** into one shared quadruple keyed by `strategy_id` +
   `params JSONB` + `extra_metrics JSONB`. New strategy = zero migrations.
4. Frontend: one parametrized `StrategyLabPage` + typed column base; delete the tpsl1/tpsl2 page/column
   duplication.

### Part 4 — Frontend cleanup 🟡

- **M14 — introduce ESLint** (none exists) with `no-restricted-imports` boundary zones (shared ⊬ @live/@lab;
  live ⊬ @lab; lab ⊬ @live), CI-gated; relocate the current offenders.
- **M15 — SSE hardening:** `onerror`/`onopen`, connection-status signal, resync refetch on reopen. Small, real.
- **M16 — window/server-side** the SwingDetectionPage 20k pull.
- **M18 — extract `TokenPriceChart` (1931 lines)** and the strategy pages (pairs with Part 3.4).
- **M17 — quote-aware price display** — deferred with Part 5 (needs the quote axis).

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

4. **Fix the two live doc-drift comments** (cheap, ship with Part 1): `strategy_id` CHECK comment
   (`0001_init.sql:272`) and the `initial_supply_token` doc-comment (`models/token.rs:21`, resolves with H1).

5. **Track the newer forge audit separately.** `docs/forge-efficiency-audit-2026-07-13.md` is the active
   perf track for forge and is out of scope here; this document covers **hunter** only. Don't double-plan
   the forge SSE/ingest/reconcile work — it's already in flight there.

---

## Dependency / sequencing

```
Part 1 (real-money, no deps)  ──►  ship immediately, before anything else
Part 2 (lab throughput)       ──►  independent, any time
Part 3 (modularity)           ──►  independent; enables cheaper strategy #4
Part 4 (frontend)             ──►  independent (M17 waits on Part 5)
Part 5 (venue/quote port)     ──►  deferred; port from forge when prioritized
New-plan #1 (SSOT drift)      ──►  do before any hunter/forge shared refactor (esp. chart M18)
```

Nothing in Parts 2–5 should merge ahead of Part 1.
