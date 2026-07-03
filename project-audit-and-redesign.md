# Project Audit & Redesign Blueprint

**Date:** 2026-07-03 · **Method:** code-first review of all 6 workspace crates + `frontend-react` (source read directly; `@arch`/`@plans` docs used only to locate files — contradictions recorded in the Docs-Drift appendix). Five parallel audit passes (core, live hot path, lab pipeline, frontend, cross-cutting extensibility) followed by manual source verification of every Critical/High finding.

**Hardware/goal frame used for judging:**
- `live` — EC2 2 vCPU / 4 GB. Latency, stability, correct concurrency on real-money paths.
- `lab` — workstation 8 CPU / 32 GB. Raw throughput and parallelism over big token/trade history.
- `core` — pure shared logic; leaks in either direction are boundary bugs.
- Extension targets: **T1** Pump.fun USDC pairs · **T2** other launchpads (Bonk) · **T3** dynamic bonding curves · **T4** AMM/DEX venues (Raydium/Meteora/PumpSwap).

---

## Executive summary

1. **A real-money double-sell hazard exists.** The sell retry loop re-sends whenever the landed tx is *not* a confirmed revert — including when RPC reports `Succeeded`/`Pending` but the trades feed/DB hasn't indexed it yet. The buy path guards against exactly this; the sell path doesn't. With two positions sharing one token account, a re-sent sell can consume the other position's tokens. Fix immediately, independent of any redesign. (`live/src/strategies/execution/real.rs:791-817`)
2. **Lab runs its main workload on 2 of 8 cores.** The sweep's rayon pool reserves 6 threads (4 tokio + 2 HTTP) to protect a "live trading hot path" that does not exist in the lab binary. The box's primary purpose is throttled to 25%. One-line-class fix. (`lab/src/sweep/registry.rs:215-240`)
3. **The extensibility story is transport-shaped, not venue-shaped.** `pump-trader` and `ingest-laserstream` are genuinely isolated crates (verified: zero workspace deps), but the isolation seam swaps *transports* (gRPC↔websocket), while T1–T4 all need a *venue* seam: decode + curve math + instruction build + quote currency. Every one of the four targets today is a fork/rewrite, not a plug-in. The `Protocol` builder looks like a venue seam but only parameterizes ID/discriminator bytes — the Borsh layouts, lamports scaling, and WSOL pool math behind it are hardcoded.
4. **"SOL" is a fused unit, not a config value.** Quote currency is baked into column *names* (`*_lamports`), a global `/1e9`, the WSOL pool-PDA seed, strategy thresholds, the lake schema, and the frontend `◎` formatter. USDC pairs (T1) are the least-supported target: a USDC pool today would be invisible to ingest (wrong derived PDA) and mis-scaled 1000× (9 vs 6 decimals).
5. **Market cap is computed wrong and inconsistently.** SQL paths use `current_price × initial_supply_token` where `initial_supply_token` is the dev's *first-buy* amount, not supply; the in-RAM cache path uses `total_supply × price`. The same list mixes both. Display, sort, and filter on market cap are unreliable. (`trading_core/src/api/handlers/tokens/tokens.rs:697,736,783`)
6. **Triplication is the dominant code-quality tax.** Three ~600–900-line sweep adapters (~70% identical), three copy-pasted backtests, near-verbatim registry dispatch/simulate clones, tpsl1/tpsl2 decision modules ~75–90% identical (one file byte-identical), and the same duplication mirrored in the frontend (column defs ×2, strategy pages differing by ~10%). Adding a 4th strategy or 2nd venue multiplies all of it.
7. **The lake pipeline has silent-corruption edges.** Day files are sealed immutably with no completeness check against the source; the sync script has no missing-row reconciliation for `trades`; the sim↔sweep parity test is `#[ignore]`d and the lake-vs-PG parity baseline no longer exists.
8. **Much of the foundation is genuinely good** and should be kept: bounded channels with counted drops, RAII entry/exit interlocks (panic-safe, verified atomic), OS-thread watchdog, durable-nonce tx plumbing, buy-side confirmed-revert-only re-send, entry-reuse + DDSketch fold in the sweep engine, sealed-day lake concept, memoized frontend hot paths, server-side token paging.

---

# Part A — Findings (ranked)

Severity reflects impact on this system's stated goals (real-money safety > lab throughput > maintainability). Every Critical/High item was verified against source during this audit.

## A.1 Critical

**C1. Sell re-sends on non-reverted (possibly succeeded) transactions — double-sell / cross-position oversell risk.**
`live/src/strategies/execution/real.rs:791-817` (`classify_sell_revert`), consumed at `:984-987`.
After the feed-confirm poll window elapses with an unchanged balance, the code classifies the sent tx via RPC. Only a confirmed revert on the same route stops or re-routes; **every other state — `Succeeded`, `Pending`, RPC error — falls into `_ => SellRetryDecision::Retry`** and re-sends with a fresh durable nonce and escalated tip. The confirm loop's data source (`sum_legs_by_signatures`) reads the `trades` table, which is fed by the same DB writer that is the system's backpressured component — so DB lag beyond the ~5 s window is exactly the trigger. The buy path is symmetric-safe (re-sends only on `Ok(Some(false))` confirmed revert); the sell path is not.
- Single position: the second sell reverts on the emptied account → fee burn (bounded harm).
- **Two concurrent positions on the same mint share one token account** (`find_reusable_token_account`, `real.rs:330-347`): the re-sent sell for position A's `entry_token_amount` can consume position B's tokens → real oversell, B marked `ExitFailed`.
**Direction:** treat `Succeeded`/`Pending`/error as "do not re-send; keep polling the feed" — mirror the buy guard. Ship before anything else.

**C2. Lab sweep parallelism capped at `cores − 6` — 2 threads on the 8-core box.**
`lab/src/sweep/registry.rs:215-240` (`bounded_threads`), with `TOKIO_WORKER_THREADS=4` and `HTTP_WORKERS=2` defaults; `lab/src/main.rs` runs no ingest and its `TokenCache` is empty.
The reservation comment says it protects "the live trading hot path (ingest / sell-confirm)" — a workload the lab binary does not run. The machine bought for sweep throughput does 25% of its potential work; `SWEEP_RAYON_THREADS` can override but the default is wrong for the only box that runs it.
**Direction:** in lab, default the rayon pool to `cores − 1` (leave one core for the HTTP/UI path); delete the ingest-reservation rationale.

## A.2 High — correctness & data

**H1. Market cap formula is wrong in SQL and inconsistent with the cache.**
`trading_core/src/api/handlers/tokens/tokens.rs:697,736,783`, `tokens/sql.rs:150,365`, `token_repo.rs:404,461,525,556`, `strategy_repo.rs:389,441,619,647`, `models/token.rs:124-126`.
SQL computes `market_cap = i.current_price * t.initial_supply_token`, but `initial_supply_token` is documented and populated as the creator's **first-buy token count** (`models/token.rs:21-22`), not total supply. The in-RAM path (`TokenState::update_market_cap`) uses `total_supply_for(is_mayhem) × price` (1e15/2e15 raw). The merged `/api/tokens` list mixes both; the live bin's SQL-paged list is uniformly the wrong-magnitude formula, and `mcap_min`/`mcap_max` filters and sorts silently act on it.
**Direction:** compute FDV as `total_supply_for(is_mayhem) × current_price` in one shared site (SQL expression + Rust fn); store a real supply column.

**H2. Ingest consumer couples strategy-ping dispatch to DB backpressure.**
`live/src/ingest/consumer.rs:196-197,233-234,312`.
`enqueue_db(...).await` (non-lossy, cap 16384) runs on the single consumer `select!` task that also dispatches strategy pings. When Postgres hiccups and the queue fills, the awaited send blocks the consumer → strategy pings stop → **real exits stall exactly when the DB is slow**, then the watchdog force-exits and the live gRPC cursor is dropped (reconnect gap). This also feeds C1: the same lag starves the sell-confirm's data source.
**Direction:** decouple — dispatch pings before/independently of the DB enqueue, or bound the hot-path enqueue with a timeout that sheds to lossy behavior for non-critical ops.

**H3. Lake day-files seal immutably with no completeness check.**
`lab/src/lake/export.rs:119-147`.
Export seals `dt=D/data.parquet` from whatever local PG holds at export time (skip-if-exists, no row-count check vs source). If `db-incremental-sync` hadn't fully landed day D, the immutable file is **permanently partial** and every sweep/backtest silently under-counts forever. Compounding: `scripts/db-incremental-sync.ps1:407-412` pulls `trades`/`raw_txs` by `block_time >= watermark` with no missing-row reconciliation (`tokens` has an `OR NOT EXISTS` path; trades does not), so a late backfill into a passed day never arrives.
**Direction:** record source row-count per sealed day; refuse/re-seal on mismatch; add per-day count reconciliation (or a rolling re-pull window) to the sync.

**H4. The sim↔sweep/lake parity guarantee is untested in practice.**
`lab/src/lake/duck.rs:426-497` — the only parity test is `#[ignore]` (needs a populated `$SWEEP_LAKE_DIR`), and with `DbSource` deleted there is **no lake-vs-PG byte-parity baseline at all**. The migration's central claim ("lake produces identical metrics") is unverifiable in CI.
**Direction:** a fixture-lake test (tiny PG fixture → export → load → compare) that runs un-ignored.

## A.3 High — performance

**H5. Corpus load is row-by-row, single-threaded, and re-done every sweep.**
`lab/src/lake/duck.rs:253-312` — every trade row goes through the DuckDB row API (~11 `row.get` calls + a fresh `String` mint per row), tens of millions of rows, no Arrow (avoided deliberately for arrow-version isolation, `duck.rs:12-16`). This is the dominant single-threaded cost of every sweep and backtest. Compounding: `lab/src/api/handlers/strategies/grouped_sweep.rs:425` always reloads — `sweep_corpus_cache` is written after load but only read by the drill-in, never by the next sweep, so the normal tune-and-rerun loop pays the full load each iteration. Also `duck.rs:108,169`: `stage_mints` runs twice on the explicit-mint path.
**Direction:** Arrow record-batch extraction (isolate DuckDB's arrow version behind a small boundary) or at minimum hoist per-row allocations; check the hash-keyed corpus cache before `LakeSource::load` in the sweep path.

**H6. Single-rule simulate is a single-threaded for-loop over uncapped histories.**
`lab/src/strategies/tpsl_sniper_2/backtest.rs:185-281` (same in tpsl1/swing1); `sim_fetch.rs:31` (`SIM_PER_MINT_CAP = i64::MAX`).
The grouped sweep parallelizes identical per-token work; the drill-down backtest runs on 1 of 8 cores. Additionally the DuckDB load and the resolve loop run on an actix worker without `spawn_blocking` (`grouped_sweep.rs:425`, tpsl2 handler `:438`), occupying 1 of 2 HTTP workers for multi-second stretches.
**Direction:** `par_iter` the per-token resolve (results independent; sort after); wrap load + resolve in `spawn_blocking`.

## A.4 High — extensibility (what blocks T1–T4)

**H7. Quote currency (SOL) is fused into units, names, and derivations — T1 blocker.**
- WSOL is *the* pool-PDA seed: `ingest-laserstream/src/pool.rs:28-56` derives `[b"pool", 0u16, authority, mint, WSOL]` — a USDC-quoted pool derives to the wrong address and is **never discovered, never subscribed, invisible to ingest**.
- The AMM builder wraps/unwraps WSOL, uses `ATA(user, WSOL)` and WSOL fee ATAs throughout: `pump-trader/src/trader/amm.rs:280-531`.
- One global scale: `/LAMPORTS_PER_SOL` on every amount (`decode/trade.rs:46-57,192-214,282`; `trade_repo.rs:835-846`; frontend `/1e9` in `sharedTokenColumns.tsx:257/271` etc.). A 6-decimal USDC quote is mis-scaled 1000×.
- The schema names the unit into columns (`*_lamports`, migration `0009`) and PnL views hardcode `/1e9` (`0001_init.sql:370`).
- Strategy thresholds are absolute SOL (`swing1_strategy_rule.rs:35,50,52,78` + tpsl rules + frontend param specs) — meaningless across quotes.
**Direction:** a quote axis (`quote_mint`, `quote_decimals`) as *data* at decode/schema/model/display layers; integer base-unit amounts with decimals-aware conversion at one boundary.

**H8. Venue recognition and subscription are pump-only string matches — T2/T4 blocker.**
`ingest-laserstream/src/decode/mod.rs:85-98` + `transport/mod.rs:143-180` — classification is substring-`contains(pump_id)` over log lines (duplicated in two places), and the gRPC subscribe filter includes only the pump program + tracked pools. Any Bonk/Raydium/Meteora tx is `None` → silently dropped at the transport edge. `enum Venue { Curve, Amm }` (`event.rs:52`), `TxRelevance` (`decode/mod.rs:26-32`), `venue TEXT CHECK IN ('curve','amm')` (`0001_init.sql:106,165-166`), `is_amm: bool` in the ingest contract (`trading_core/src/ingest.rs:59`), and `is_migrated: bool` routing with hardcoded revert codes 6003/6004/6005/6024 (`real.rs:786-903`) all encode a closed two-venue world.
**Direction:** venue registry (program id → decoder) driving both the subscribe set and classification; venue as open data, not a CHECK; venue-tagged reserve updates and per-venue revert classification.

**H9. The `Protocol` builder is a false venue seam — T2/T3/T4 blocker.**
`ingest-laserstream/src/protocol.rs:112-158` parameterizes program IDs and discriminator *bytes*, but the `RawTradeEvent` Borsh layout (`decode/trade.rs:17-30`), PumpSwap event structs (`:90-222`), lamports scaling, and pool math are hardcoded — swapping `Protocol` cannot produce a Bonk or Raydium decoder. Likewise `pump-trader`'s Tier-1 "invariants" (`protocol.rs:28-99,143-157`) place the entire venue (IDs, WSOL, fee accounts, curve formula in `buy.rs:430-455`, account layouts) on the unchangeable side by design; its own header says different values = "a different chain/program". The genuinely reusable part is the tx/nonce/send/confirm plumbing. `ingest-websocket` (verified: `spawn()` is `unimplemented!()`) mirrors only the *transport* contract — confirming the isolation investment went to transport swapping, which is not what T1–T4 need.
- Curve math is additionally hardcoded in **three languages**: Rust consts (`token_math.rs:10-45`, incl. the `−30` reconstruction in `approx_real_sol_reserves`), the trader min-out (`buy.rs:430-455`), and the frontend chart (`chartBars.ts:93-132`, `constants.ts:134-144`). Mitigant for T3: live pricing reads program-emitted reserves per trade, so dynamic curves mainly break sim/backtest reconstruction, the chart, and FDV/dead-token baselines.
- Program IDs/WSOL/`LAMPORTS_PER_SOL` are deliberately duplicated across `trading_core/src/config/constants/protocol.rs:12-21` and `pump-trader/src/protocol.rs` with a "keep in sync" comment — two edit sites for any venue change.
**Direction:** see Part B — a `Venue` trait family (decoder, curve model, ix-builder, quote unit) with pump.fun as the first implementation; keep the tx plumbing as the venue-agnostic core of the trader.

## A.5 High — modularity

**H10. Strategy-code triplication across lab and core.**
- `lab/src/sweep/strategies/{tpsl1,tpsl2,swing1}.rs`: ~70% identical across three 600–900-line adapters (`from_spec`/`axis`/`combo_count`/`combo_at`/`sample`/`refine`/`params_json`).
- `lab/src/sweep/registry.rs`: `sweep_tpsl1/tpsl2/swing1` (`:247-337,371-450,484-566`) and `simulate_*_one_combo` (`:608-756`) are near-verbatim clones; `trading_core/src/strategies/registry.rs:125-469` hand-rolls three `*Params` conversions sharing ~12 identical fields, rebuilt per call on every `matches_entry`/`resolve_*`.
- Backtests: `select_simulated_tokens` copy-pasted verbatim ×3 (tpsl1:45-78, tpsl2:56-89, swing1:26-59); the whole `run_backtest` skeleton triplicated.
- Decision modules: `tpsl_sniper_1/exit` vs `tpsl_sniper_2/exit` are ~75% byte-identical (927 vs 1000 lines, 233 changed); `entry` modules near-identical; `util.rs` **byte-identical** (`diff -q` clean). Real divergence is tiny: E4's reserve source (`reserve_sol` vs `real_reserve_sol`), tpsl2's `find_trade_driven_exit_with_slot` + scalp entry, the Rule type. The "intentional clone" tradeoff no longer holds for the shared 90%.
- Mirrored in the frontend: `tpsl1/utils.ts` ≡ `tpsl2/utils.ts` (empty diff), column defs duplicated (`tokenColumns.tsx` 513 lines typed vs `sharedTokenColumns.tsx:76-392` untyped mirror), lab `Tpsl1Page` vs `Tpsl2Page` differ ~238 of ~2418 lines.
Adding strategy #4 today = ~1000 lines of copying across 7+ surfaces (adapter, backtest, TABLES, 2 match arms, sweep fn, simulate fn, migration, handler, frontend page/columns).
**Direction:** one generic `run_backtest<S>` / `sweep_dispatch<S>` / `StrategyDescriptor` registration; Rule trait + E4-reserve-source param to merge the decision clones; single typed column base + parametrized strategy page in the frontend.

## A.6 Medium

**Live / real-money**
- **M1.** Shared token account across concurrent same-mint positions: rent-reclaim `close_token_account` fires when the *first* position clears, targeting an account still holding the second's tokens (preflight revert; wasteful; compounds C1). `real.rs:330-347,701-710`. → Refcount the (wallet,mint) account; close on last exit; serialize same-mint exits.
- **M2.** Write-ahead `mark_buy_submitted` DB persist runs while the durable-nonce slot is held `in_use` (`buy.rs:313-355`, `nonce.rs:37-116`); DB backpressure pins nonce slots → concurrent buys spin (`200×20ms` then bail). → Timeout on the hook.
- **M3.** `wallet_dict` intern uses `ON CONFLICT DO UPDATE SET address = EXCLUDED.address` solely to force `RETURNING id` — every re-intern of a popular wallet writes a dead tuple + WAL on the hot path (`wallet_dict_repo.rs:22-79`). → `SELECT` fast path, `INSERT ... ON CONFLICT DO NOTHING` fallback.
- **M4.** Confirm-loop queries do a separate wallet-intern round-trip per call (2 RTs) — `trade_repo.rs:392-421,558-609,617-635`. → Resolve wallet id in-SQL (JOIN/CTE) or cache hot ids.
- **M5.** The ingest contract is pump-specific (`TraderHook::update_live_reserves(..., is_amm: bool)`, `prewarm_amm_pool`, `IngestKind::Migrated`, discriminator table) — `trading_core/src/ingest.rs:55-69`. → Venue-tagged events (part of H8 fix).

**Lab / data**
- **M6.** Grid sampling eagerly materializes up to 1M full rule clones + params JSON (`tpsl2.rs:278-306`, `grouped_engine.rs:221`) — hundreds of MB at the ceiling. → Lazy combo iterator / intern invariant fields.
- **M7.** `sub_corpus` clones every `CorpusToken` (strings + fingerprint) per large group including the ALL group (`grouped_engine.rs:414-420`). → Index-based sub-views (the serial path already does this).
- **M8.** Every `GET /api/tokens` re-filters/re-sorts up to 1M rows single-threaded per 5s UI poll; ETag saves bytes, not compute (`tokens.rs:398-424`). → Memoize per (query, snapshot generation).
- **M9.** Current-day analysis needs two uncoupled `--include-today` runs (sync, then export); `warn_if_stale` only logs → silently truncated sims (`sim_fetch.rs:74-91`). → One command or a hard freshness check.

**Core / models**
- **M10.** Legacy `Position` model + adapters kept only for the SSE wire shape; unified `StrategyPosition` already the source of truth (`models/position.rs`, `stream.rs:182`). → Render `PositionResponse` from `StrategyPosition`; delete.
- **M11.** `find_tx_by_fill` matches on exact f64 equality of a price ratio — silent misses (`trade_repo.rs:761-780`). → Integer ratio or tolerance band.
- **M12.** Live-only runtime orchestration (`runtime_cache.rs`, 1568 lines + `exit_state.rs`) lives in shared core; lab links none of it. → Move to `live`.
- **M13.** `find_by_mint_all`/`find_by_mints_all` unbounded, batch-only by convention, unenforced (`trade_repo.rs:425-498`). → Hard cap or batch-pool assertion.

**Frontend**
- **M14.** Shared-tree files do value imports from `@lab` (`GroupedCreationSection.tsx:8-13`, `BackgroundJobsIndicator.tsx:8`) — only tree-shaking keeps the live bundle lab-free; the seam is incidental, not enforced. Lab pages likewise import `@live` endpoints, registering all live routes in the lab store (`Tpsl1Page.tsx:88` etc.). → ESLint `no-restricted-imports` + relocate offenders.
- **M15.** The shared `EventSource` has no `onerror`/`onopen` — a permanently failed stream is silent, no resync on reconnect (`sse.ts:72-115`). → Connection-status signal + resync refetch.
- **M16.** `SwingDetectionPage` pulls up to 20,000 full `TokenRecord`s in one request for client-side analysis (`SwingDetectionPage.tsx:589`). → Server-side or windowed.
- **M17.** `PriceUnit` is a hard `'SOL'|'USD'` binary with one global rate; `TokenRecord` has no venue/quote field (`types/index.ts:3,10-50`; `usePriceDisplay.ts:19-36` hardcodes `◎`). → Quote-aware value type (pairs with H7).
- **M18.** Oversized components: `TokenPriceChart.tsx` 1862 lines; lab strategy pages ~1200–1290 each. → Extract; pairs with H10's page parametrization.

## A.7 Low (abbreviated)

- **L1.** Hot-path maps key on `String` mint (44-char base58) everywhere; wallets are interned to `u32` but mints are not; `WalletInterner` stores each address twice (`token_cache.rs`, `trade_signals.rs`, `wallet_interner.rs:26-27`). → `[u8;32]`/interned mint keys.
- **L2.** `push_trade_capped` `Arc::make_mut` can deep-copy up to 3500 cached trades on the ingest path while an API snapshot holds the Arc (`token_cache.rs:404-416`).
- **L3.** Every buy *and* sell spawns a detached nonce-refresh task polling `get_account` (≤8×150ms) — task/RPC churn on the small box (`nonce.rs:119-195`).
- **L4.** `on_trade` builds label JSON + double-clones per trade even when not persisted (`consumer.rs:236-280`).
- **L5.** `token_list_cache` clones the whole live cache under a Mutex per staleness window (`token_list_cache.rs:168-187`). → arc-swap of a background-built snapshot.
- **L6.** `sol_to_lamports`/`lamports_to_sol` redefined in 4 repos — unit-drift risk of exactly the class migration 0008 fixed. → One shared helper (subsumed by H7's unit boundary).
- **L7.** Analysis/dashboard concerns in core (`analyzers.rs`, `grouping.rs`, `creation_stats_repo.rs`) — if lab-only, move out.
- **L8.** SSE `render_sse_frame` does two cache-shard lookups per `TokenCreated` (`stream.rs:63-86`).
- **L9.** Frontend dead code: `getTokens`/`useGetTokensQuery`/`fetchTokens` (zero consumers), unrouted `TransactionsPage` + `tradeColumns` + `useTradeStream`. → Delete.
- **L10.** Unchunked mint lists in `getTokensByMints`/`getWalletPrices`; DataTable hover CSS caps at 48 columns; type-only `@lab` imports from shared (hygiene).
- **L11.** Lake `Selection::default().per_mint_cap` (2500) diverges from the real default (5000) — unused but misleading (`corpus.rs:133,145`).

## A.8 Verified positives (keep these)

- Double-sell/double-buy **interlock** is atomic and panic-safe (`runtime_cache.rs:485-502`, DashSet + RAII guards) — verified correct.
- **Buy** path re-sends only on confirmed revert — correct and unit-tested (the model C1 should copy).
- No `.await` held across `std::sync::Mutex` in trader hot paths; bounded channels everywhere with counted drops; watchdog on a real OS thread.
- Sweep engine: entry resolved once per entry-key and reused across the exit sub-grid; `TokenOutcome` is `Copy`; DDSketch fold; memory-budgeted combo batches.
- Lake reads do get DuckDB column pruning + predicate pushdown (the cost is row extraction, H5).
- Frontend: SSE trade path coalesced + visibility-gated; price cells memo'd so USD ticks don't rebuild columns; DataTable rows memo'd; 100K-token page is server-side paged; chart coalesces crosshair via rAF. Verified: the live bundle is currently lab-free (by import graph).

---

# Part B — Redesign blueprint

Effort scale: **S** ≈ ≤1 day, **M** ≈ 2–5 days, **L** ≈ 1–2+ weeks.

## B.1 Target topology

**Verdict on the two contested crates:**

- **`pump-trader` is dissolved.** Its Tier-1/Tier-2 split is real, but the entire venue (IDs, WSOL, fee recipients, curve math, AMM layout, ix builders) sits on the "invariant" Tier-1 side by design (`protocol.rs:1-19` explicitly declares different values = different chain). Tier-2 (durable-nonce pool, tx build/sign/fan-out, confirm poll — all audit-verified good) extracts to **`sol-executor`**. Tier-1 becomes the first venue impl, **`venue-pumpfun`**. The `Protocol` builder struct is deleted — a false seam (H9).
- **`ingest-laserstream` shrinks to transport-only** (**`ingest-grpc`**). It keeps what it's good at: gRPC subscribe/reconnect/backoff/gap-replay/idle-stall (verified good). Everything venue-shaped — `classify_tx` substring matching, Borsh decode leaves, discriminator tables, PDA pool derivation, lamports scaling — moves into venue crates. The subscribe filter and hot-path classifier are *generated from* the venue registry's program-ID set instead of hardcoding pump IDs.
- **`ingest-websocket` is deleted.** Empty scaffold; the transport seam (`IngestHandles`) survives in code and can be revived in an afternoon when actually needed.

**Proposed workspace:**

```
venue-core/          Traits + unit types: VenueId, QuoteUnit, QuoteAmount/BaseAmount,
                     VenueDecoder, CurveModel, VenueIxBuilder, PoolResolver, RevertClass.
                     Deps: solana-sdk types + borsh only. NO RPC, NO executor.
venues/pumpfun/      pump.fun curve + PumpSwap AMM impl: discriminators, Borsh decode,
                     constant-product CurveModel, WSOL/USDC pool PDA derivation,
                     buy/sell ix builders, 6003/6004 revert classification.
                     Cargo features: decode, curve (default), exec (ix building).
venues/registry/     Umbrella: `enum AnyVenue` + VenueRegistry (program-id → VenueId map,
                     static enum dispatch). The ONLY crate that knows all venues.
sol-executor/        Ex pump-trader Tier-2: durable-nonce pool, tx assemble/sign,
                     Helius fan-out send, confirm poll, tip/CU tuning, token-account mgmt.
                     Consumes Instructions from VenueIxBuilder; knows no venue.
ingest-grpc/         Ex ingest-laserstream: transport only. Subscribe filter built from
                     VenueRegistry; raw tx → registry classify → decode → IngestEvent v2.
trading_core/        Slimmed shared lib: schema-v2 models, repos/migrations, config,
                     TokenCache/TokenListCache, shared API routes, ingest contract
                     (IngestHandles, TraderHook v2). runtime_cache/exit_state MOVE OUT
                     to live; analyzers/grouping MOVE OUT to lab.
strategy-core/       Rule trait, CommonParams, decision kernel (merged tpsl_sniper +
                     swing_1), StrategyDescriptor + registration macro. Pure compute —
                     no DB, no executor. Linked by both live and lab.
live/       (bin)    EC2. Runtime cache, strategy runner, execution loops, watchdog,
                     db_writer, live API. Links: venues/registry (full features),
                     sol-executor, strategy-core, trading_core, ingest-grpc.
lab/        (bin)    Workstation. Lake, sweep/backtest engines, lab API. Links:
                     venues/registry (decode+curve only — NO exec feature),
                     strategy-core, trading_core. NEVER links sol-executor.
```

**Why the venue abstraction lives in `venue-core` + `venues/registry`:** live stays lean because dispatch is a monomorphized `match` over `AnyVenue` (no vtables, no dyn-per-event allocation); lab never links trader code because ix-building is feature-gated in the venue crates and `sol-executor` is a crate lab simply doesn't depend on. `venue-core` pulls only solana-sdk types, so `trading_core`, `lab`, and `ingest-grpc` can all speak `VenueId`/`QuoteUnit` without dragging the RPC/executor stack.

Also: workspace `resolver = "1"` (`Cargo.toml:3`, verified) must become `"2"` — required for the per-crate feature unification this design relies on (lab must not have `exec` features unified in).

## B.2 Venue abstraction design

**Unit types — integers are the truth; f64 only at decision/display edges:**

```rust
/// Integer amount in the quote asset's native base units (lamports, µUSDC…).
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct QuoteAmount(pub u64);
/// Integer amount in the token's native base units (pump tokens: 6 decimals).
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct BaseAmount(pub u64);

#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub struct QuoteUnit { pub mint: [u8; 32], pub decimals: u8 }
impl QuoteUnit {
    pub const SOL:  QuoteUnit = /* WSOL mint, 9 */;
    pub const USDC: QuoteUnit = /* USDC mint, 6 */;
    #[inline] pub fn display(&self, a: QuoteAmount) -> f64 { a.0 as f64 / 10f64.powi(self.decimals as i32) }
}
```

This kills the 1000×-mislabel class of bug (T1): a USDC amount can never be silently divided by 1e9 because there is no bare `/LAMPORTS_PER_SOL` anywhere — conversion goes through `QuoteUnit`.

**Venue identity — replaces `enum Venue { Curve, Amm }`, `is_amm`, `is_migrated`:**

```rust
#[non_exhaustive]
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub enum VenueId { PumpCurve, PumpAmm /* , BonkCurve, RaydiumAmm, MeteoraDlmm */ }
impl VenueId {
    pub fn as_str(self) -> &'static str;          // "pump_curve" — the persisted form
    pub fn stage(self) -> VenueStage;             // Launchpad | Amm  (derived, not a stored bool)
}
```

`is_amm` becomes `venue.stage() == Amm`; `is_migrated` becomes `token.active_venue != token.genesis_venue` — venue is data, booleans are derived.

**Per-stage traits:**

```rust
/// classify + decode — implemented per venue, called by ingest-grpc.
pub trait VenueDecoder {
    fn program_ids(&self) -> &'static [Pubkey];   // drives gRPC subscribe filter
    /// Hot-path relevance check (replaces classify_tx substring duplication).
    #[inline] fn matches(&self, account_keys: &[Pubkey]) -> bool;
    /// Zero-alloc-happy decode: SmallVec out-param, no per-event boxing.
    fn decode(&self, tx: &RawTxView<'_>, out: &mut SmallVec<[VenueEvent; 4]>) -> Result<(), DecodeError>;
}

/// Pricing math — per venue+curve; also usable by lab (no exec deps).
pub trait CurveModel {
    fn spot(&self, r: &ReserveSnapshot) -> f64;                       // quote-per-base, display/decision
    fn fill_buy(&self, r: &ReserveSnapshot, quote_in: QuoteAmount) -> BaseAmount;   // exact integer
    fn fill_sell(&self, r: &ReserveSnapshot, base_in: BaseAmount) -> QuoteAmount;
    fn real_quote(&self, r: &ReserveSnapshot) -> QuoteAmount;         // replaces approx_real_sol_reserves(−30)
    fn genesis(&self) -> CurveGenesis;                                // per-token DATA, not consts
}
pub struct CurveGenesis {
    pub init_virtual_quote: QuoteAmount,
    pub init_virtual_base: BaseAmount,
    pub total_supply: BaseAmount,          // fixes the market_cap-from-first-buy bug at the source
}

/// Ix building + revert taxonomy — `exec` feature only; live-only surface.
pub trait VenueIxBuilder {
    fn build_buy(&self, ctx: &TradeCtx, quote_in: QuoteAmount, min_base_out: BaseAmount) -> Result<Vec<Instruction>>;
    fn build_sell(&self, ctx: &TradeCtx, base_in: BaseAmount, min_quote_out: QuoteAmount) -> Result<Vec<Instruction>>;
    fn classify_revert(&self, err: &TransactionError) -> RevertClass; // per-venue 6003/6004/…
}
pub enum RevertClass { Slippage, StaleState, Fatal, Unknown }

/// Pool/route discovery — PDA derive for pump (per quote mint!), index lookup for Raydium/Meteora.
pub trait PoolResolver {
    fn resolve(&self, mint: &Mint, quote: QuoteUnit) -> BoxFuture<'_, Result<RouteAccounts>>;
}
```

`TradeCtx` carries wallet, mint, `QuoteUnit`, and the resolved `RouteAccounts` — so the pump `derive_pool` seed `[b"pool", 0u16, authority, mint, WSOL]` (`pool.rs:28-56`) generalizes to any quote mint (T1) and any index, and Raydium's non-derivable pools come from a lookup table instead (T4).

**Dispatch — enum, not dyn:**

```rust
// venues/registry
pub enum AnyVenue { PumpCurve(pumpfun::Curve), PumpAmm(pumpfun::Amm) }
impl AnyVenue { #[inline] pub fn decode(&self, tx: &RawTxView, out: &mut SmallVec<..>) -> ... { match self { ... } } }

pub struct VenueRegistry {
    by_program: /* const-built sorted array (Pubkey, VenueId), binary search */,
    venues: &'static [AnyVenue],
}
```

Hot path: raw tx → `by_program` lookup on account keys (replaces the two duplicated substring scans, H8) → matched venue's `decode` via `match`. No allocation per event, fully monomorphized in live. Adding Bonk (T2) = new `venues/bonk` crate + one enum arm + one registry entry; the subscribe filter widens automatically because it's generated from `program_ids()`.

**Per-venue pipeline stages:** classify (registry program map) → decode (`VenueDecoder`, integer units) → price (`CurveModel` per token's active venue, genesis from token row) → ix-build (`VenueIxBuilder` via token's active venue) → revert-classify (`classify_revert`, replacing the hardcoded 6003/6004/6005/6024 in `real.rs:786-903`).

**IngestEvent v2 / TraderHook v2** (widens `trading_core/src/ingest.rs:55-69`, fixes M5):

```rust
pub struct TradeEvent {
    pub venue: VenueId,
    pub mint: Mint,                    // [u8;32], interned to mint_id at DB edge
    pub side: Side,
    pub quote: QuoteUnit,
    pub quote_amount: QuoteAmount,     // integer — replaces `sol: f64`
    pub base_amount: BaseAmount,
    pub reserves: ReserveSnapshot,     // integer quote/base + real variants, venue-tagged
    /* signature, slot, tx_index, leg_index, block_time, received_at, labels… */
}

pub trait TraderHook: Send + Sync + 'static {
    fn update_live_reserves(&self, mint: &Mint, venue: VenueId, r: &ReserveSnapshot);
    /// Replaces prewarm_amm_pool(is_amm) + IngestKind::Migrated: generic venue transition.
    fn on_venue_transition(&self, mint: &Mint, from: VenueId, to: VenueId, route_hint: Option<Pubkey>)
        -> BoxFuture<'_, anyhow::Result<()>>;
}
```

Positions record `entry_venue`; exits route through the token's *current* `active_venue` and fall back to `entry_venue` on route failure — replacing the `is_migrated` bool branch in execution.

## B.3 Data layer design — schema v2

Nothing is frozen, so this is a **new baseline migration**, not an in-place ALTER chain.

**Dimension tables (tiny, cached in-process):**

- `venues(id SMALLINT PK, name TEXT, stage TEXT, program_id TEXT)` — replaces `CHECK IN ('curve','amm')`; code enum ⇄ id.
- `quote_dict(id SMALLINT PK, mint TEXT, decimals SMALLINT, symbol TEXT, glyph TEXT)` — SOL=1, USDC=2, …
- `mint_dict(id INT PK, mint TEXT UNIQUE)` — intern mints like wallets (fixes L1 at the DB layer too; hypertable rows shrink materially).

**Fact columns — naming rule.** Replace `*_lamports` / `*_sol` with: *integer amounts in native base units carry the suffix `_raw`; the prefix names the asset role (`quote_` / `base_`)*. Concretely:

| v1 | v2 |
|---|---|
| `amount_lamports` / `sol_amount` | `quote_amount_raw BIGINT` |
| `token_amount` | `base_amount_raw BIGINT` |
| `reserve_sol` / `reserve_token` | `quote_reserve_raw` / `base_reserve_raw` |
| `real_reserve_sol` | `real_quote_reserve_raw` |
| `entry_sol` / `exit_sol` / `pnl_sol` | `entry_quote_raw` / `exit_quote_raw` / `pnl_quote_raw` |
| `initial_buy_sol` | `initial_buy_quote_raw` |
| `buy_amount` (DOUBLE, SOL) | `buy_amount_quote DOUBLE` + rule-level `quote_id` |

`trades` gains `venue_id SMALLINT` + `quote_id SMALLINT` (denormalized — hypertable stays join-free on write); `tokens` gains `quote_id`, `base_decimals SMALLINT`, `active_venue_id`, `genesis_venue_id`, and curve genesis: `total_supply_raw BIGINT`, `init_virtual_quote_raw`, `init_virtual_base_raw` (captured at create-decode; legacy backfill = today's pump constants).

**Market-cap bug (H1) killed structurally:** `initial_supply_token` (dev first-buy) is renamed `dev_first_buy_base_raw` so it can never be mistaken for supply again; FDV is defined in exactly one place — the `token_overview` view: `fdv_quote = current_price * total_supply_raw / 10^base_decimals` — and `TokenState::update_market_cap` computes the identical formula from the same stored `total_supply_raw`. The live-rows/db-rows formula mix in `/api/tokens` disappears because both read the one definition.

**Decimals-aware views:** `trades_priced` and `strategy_position_pnl` join `quote_dict` and compute `price = (quote_amount_raw / 10^q.decimals) / (base_amount_raw / 10^t.base_decimals)`; PnL exposed as `pnl_quote` (display units) + `pnl_quote_raw`, with USD via a `quote_usd_rates` feed table (SOL rate today; USDC rate ≡ 1). All `/1e9` literals in SQL die.

**Lake v2:**

- Path versioning: `lake/v2/trades/dt=YYYY-MM-DD/data.parquet`; `schema_version` also in Parquet key-value metadata. DuckDB readers pin to `v2/` — no more `union_by_name` papering.
- Columns mirror schema v2: `venue_id`, `quote_id`, `quote_decimals`, integer `quote_amount_raw`/`base_amount_raw`/reserves. `approx_real_sol_reserves` (−30 clamp in `duck.rs:301`) is deleted; real reserves are either stored (program-emitted) or derived on load via the venue's `CurveModel::real_quote` using per-token genesis.
- **Seal protocol fixes H3:** export writes `data.parquet.tmp` + `manifest.json {row_count, min/max block_time, source_day_count, schema_version, sha256}`; the file is renamed (sealed) only when `row_count == source_day_count`, where `source_day_count` is recorded by the sync step. Re-seal is permitted whenever the manifest mismatches a fresh count — immutability begins *after* verification, not before.
- **Sync reconciliation:** in addition to the watermark, reconcile per-day `count(*)` server-vs-local for a rolling window (last N days + all unsealed days); mismatch → re-pull that day. One `lake sync --include-today` command chains sync → verify → export, so the two-invocation trap (M9) disappears.
- Re-instate the lake-vs-PG parity test against a fixture lake, un-ignored in CI (H4).

**Historical data:** legacy rows map mechanically (`curve`→PumpCurve, `amm`→PumpAmm, quote=SOL/9, genesis=pump constants); given the drop/re-ingest tolerance, re-export the whole lake as v2 from local PG rather than supporting dual-format reads.

## B.4 Strategy framework

**Merge tpsl1/tpsl2 (75% byte-identical, `util.rs` 100%):** one `tpsl_sniper` decision module in `strategy-core`:

- `Rule` trait + `CommonParams` (the ~12 shared leading fields, `#[serde(flatten)]`) — deletes the hand-rolled `from_rule/to_rule` triplets and the per-call `to_rule()` rebuild (parse once at rule load, store typed).
- The three real deltas become parameters: `e4_reserve: ReserveSource { Virtual, Real }` (the `reserve_sol` vs `real_reserve_sol` split); tpsl2's slot-aware `find_trade_driven_exit_with_slot` becomes the single implementation (strictly more capable); Copy-vs-Clone resolved by the merged type.
- `tpsl_sniper_1` / `tpsl_sniper_2` survive as **strategy-id presets** over the merged module, so persisted rules/runs stay meaningful with zero decision drift (parity test: run both presets over a fixture corpus vs recorded v1 outputs).

**StrategyDescriptor — one registration, everything else generic:**

```rust
pub struct StrategyDescriptor {
    pub id: &'static str,
    pub aliases: &'static [&'static str],
    pub quote_scope: QuoteScope,          // which quote units the params are denominated in
    // vtable-free: a macro-generated enum arm gives static dispatch
}
pub trait Strategy: Send + Sync + Sized {
    type Params: DeserializeOwned + Serialize + Clone + SweepAxes;  // derive macro
    fn resolve_entry(p: &Self::Params, tok: &CorpusToken, ...) -> Option<EntryFill>;
    fn resolve_exit(p: &Self::Params, entry: &EntryFill, ...) -> ExitOutcome;
}
register_strategy!(TpslSniper, "tpsl_sniper", aliases = ["tpsl1", "tpsl2"]);
```

- **Lab:** `run_backtest<S: Strategy>`, `sweep_dispatch<S>`, `simulate_one_combo<S>`, and one shared `select_simulated_tokens` replace the triplicated 600–900-line adapters and the near-verbatim registry clones (H10). Axis machinery (`combo_at`/`sample`/`refine`/`params_json`) is written once over `Vec<Axis>` produced by `#[derive(SweepAxes)]` field attributes.
- **Tables:** collapse the per-strategy quadruples (`tpsl1_grouped_sweep_*` × 3) into ONE shared quadruple `sweep_runs/groups/results/combos` with `strategy_id` + `params JSONB` + `extra_metrics JSONB` (swing's `n_exit_next_kill` goes there). New strategy = zero migrations.
- **Live:** the same descriptor drives `StrategyImpl` resolution and params parsing in `trading_core/src/strategies/registry.rs` — one enum generated by the same macro.
- **Net:** a new strategy = one file implementing `Strategy` + one `register_strategy!` line. No table triple, no handler module, no match-arm shotgun.

**Per-quote-unit params (H7/M17):** every quote-denominated param renames `*_sol` → `*_quote` (`buy_amount_quote`, `p_entry_min_liquidity_quote`, `p_swing_*_quote`) and the **rule itself carries `quote_id` + venue scope** — a rule targets (venue set, quote unit), so thresholds are always interpreted in a declared unit rather than silently changing meaning per pair. USD-normalized thresholds are a later option via the rates table; not v2 scope.

**Grouped sweep engine (entry-reuse, DDSketch fold, fingerprint partition, GroupSink writer): unchanged.** Already strategy-blind and audit-verified good; it just sits behind `Strategy` instead of three adapters.

## B.5 Live hot-path fixes (ship independent of, and before, the redesign)

Ordered by real-money severity:

1. **Sell re-send on Succeeded/Pending (C1)** — `real.rs:791-817, 984-987`. Make `classify_sell_revert` symmetric with the buy guard: re-send **only** on confirmed on-chain revert. `Succeeded`/`Pending`/RPC-error/unknown → keep polling (feed + direct RPC `getSignatureStatuses`) with an extended deadline; on deadline, raise an operator alarm and mark the position `ExitUnconfirmed` — never fire a second sell. Closes both the fee-burn case and the cross-position oversell via shared token account.
2. **Shared-token-account refcount + same-mint exit serialization (M1)** — `real.rs:330-347, 701-710`. `DashMap<(wallet, mint), AtomicU32>`: increment on entry fill, decrement on exit-cleared; `close_token_account` only at zero. Add a per-mint async `Mutex` around real exits so two positions on one mint can never race the same account balance.
3. **Decouple strategy pings from DB backpressure (H2)** — `consumer.rs:196-234, 312`. In `on_trade`, dispatch the strategy ping and `TraderHook` reserve update *before* `enqueue_db`, and replace the unbounded `.await` send with `send_timeout` + small in-task overflow buffer + drop-metric for Trade rows. A PG hiccup can no longer stall real exits, and the watchdog stops being the de-facto backpressure handler (avoiding forced exit → gRPC cursor loss).
4. **Nonce-hold during DB persist (M2)** — `buy.rs:313-355` + `nonce.rs:37-116`. The `on_signed` `mark_buy_submitted` persist becomes fire-and-forget (spawned) with the write-ahead guarantee preserved by an in-memory journal; the durable-nonce slot is held only for sign+send. Minimal alternative: 250 ms timeout on the hook. Concurrent buys stop spin-waiting 4 s under DB lag.
5. **Wallet-intern dead-tuple UPDATE (M3)** — `wallet_dict_repo.rs:22-79`. SELECT-first fast path → `INSERT … ON CONFLICT DO NOTHING RETURNING id` → re-SELECT on conflict; plus an in-process LRU of hot wallet ids, which also removes the 2-round-trip intern in the confirm-loop queries (M4).
6. *(rides along, display-correctness)* **Market-cap unification (H1)** — can ship pre-v2 by adding `total_supply_raw` and pointing both SQL and `TokenState::update_market_cap` at one formula.

## B.6 Lab throughput fixes (ordered by leverage)

1. **Rayon pool sizing (C2)** — `registry.rs:215-240` + `main.rs`. Default `SWEEP_RAYON_THREADS = cores − 1`, `worker_threads = 2` on lab. One-line-class change, ~3.5× sweep throughput on the 8-core box.
2. **Arrow batch corpus load (H5)** — `duck.rs:253-312`. Replace the row-by-row DuckDB row API with `query_arrow` record batches + columnar extraction, mint hoisted per token. The arrow-version isolation worry that motivated the row API is manageable with a pinned dep behind a small boundary. This is the dominant single-threaded cost of every sweep.
3. **Parallel simulate (H6)** — falls out of `run_backtest<S>` (§B.4): `par_iter` over tokens on the sweep rayon pool; give `SIM_PER_MINT_CAP` a real default instead of `i64::MAX`.
4. **Corpus cache reuse for sweeps (H5)** — key `sweep_corpus_cache` by (Selection hash, lake manifest version) and check it *before* `LakeSource::load` on sweep start, not just drill-in. The tuning loop stops re-reading the lake every run; manifest versioning (§B.3) gives a correct invalidation signal.
5. **spawn_blocking gaps (H6)** — wrap the synchronous DuckDB load and the detached sim loop in `spawn_blocking` so multi-second loads stop occupying 1 of 2 HTTP workers.
6. **Minor (M6–M8, L11):** stage_mints once; index-based `sub_corpus` views; lazy combo iterator instead of eagerly materializing up to 1M rule clones; memoize `build_tokens_list` per (query, snapshot generation).

## B.7 Frontend

- **Enforceable split (M14):** ESLint `no-restricted-imports` (or `eslint-plugin-boundaries`) zones — `shared` may import neither `@live` nor `@lab` (types included; shared types move to `@shared/types`); `live` ⊬ `@lab`; `lab` ⊬ `@live`. CI-gated. Forced fixes: `GroupedCreationSection`/`BackgroundJobsIndicator` relocate to `@lab` (or accept injected hooks as props); lab strategy pages drop `useSellTokenMutation` from `@live` — live-only sell controls move into a live-only component (the lab backend never serves those routes anyway).
- **Quote-aware price display (M17, pairs with H7):** API delivers `venue` + `quote: { mint, symbol, glyph, decimals }` on `TokenRecord` and all money fields as `*_quote_raw` integers. One module `shared/lib/units.ts` (`fromRaw(raw, decimals)`, `formatQuote(raw, quote)`) replaces every hardcoded `/1e9`. `usePriceDisplay` takes the row's quote info; `PriceUnitContext` becomes `{ unit: 'QUOTE' | 'USD', usdRateByQuoteMint }` — the ◎ glyph comes from `quote.glyph`. Chart: the client-side k-inversion and `reserve − 30` liquidity (`chartBars.ts:93-132`) are deleted; the backend serves pre-trade price and curve liquidity via the venue's `CurveModel` (removing the third copy of the curve constants).
- **Single typed column base (M14/H10):** one typed `ColumnDef<TokenRecord>[]` module; live/lab consume through a pick/override map (widths, visibility). Deletes the `any`-typed `ALL_TOKEN_COLS` mirror.
- **Strategy-page parametrization (H10/M18):** one `StrategyLabPage` + `StrategyUiDescriptor` (id, param specs, axis specs, columns, extra sections) mirroring the backend `StrategyDescriptor`; the tpsl/swing pages, byte-identical `utils.ts`, one-prop-delta `TokenInspectModal`, `SimSummaryCard`, and table/rule columns all become descriptor instances. Also ship the SSE hardening (M15: onerror/onopen, connection status, resync refetch on reopen) — small and real.

## B.8 Keep vs rebuild

| Subsystem | Decision | Why |
|---|---|---|
| ingest transport (reconnect/gap-replay/backpressure) | **Keep** (refactor: classify → registry) | Audit-verified good; only the venue knowledge leaves |
| ingest decode leaves (Borsh, discriminators) | **Rebuild** into `venue-pumpfun` | Logic ports verbatim; shape and units (f64 SOL → integer quote) must change |
| pump-trader Tier-2 (nonce/tx/send/confirm) | **Keep** → extract `sol-executor` | Verified good; venue-agnostic already |
| pump-trader Tier-1 (protocol/ix/quotes) | **Rebuild** as `venue-pumpfun` IxBuilder | Hard-won constants (fee recipients etc.) keep; `Protocol` builder is a false seam |
| live execution loops (buy/sell until cleared) | **Refactor in place** | Interlocks correct; fix §B.5 bugs, then venue-parametrize |
| runtime_cache / exit_state | **Keep**, move to `live` bin | Live-only orchestration in shared core (M12) |
| tpsl1/tpsl2 decision modules | **Rebuild** (merge; logic ports verbatim) | 75% identical; intentional-clone rationale expired |
| grouped sweep engine (entry-reuse, DDSketch fold) | **Keep** | Already strategy-blind and verified good |
| lab sweep adapters + registry + backtests | **Rebuild** generic (`Strategy` + descriptor) | 70%+ triplication, shotgun registration |
| DB schema | **Rebuild** (v2 baseline) | Venue/quote as data, integer `_raw` units, real supply — not cleanly ALTER-able |
| repos | **Refactor** | Mechanical column mapping + intern fix; structure fine |
| Parquet lake | **Rebuild** schema v2 + manifest sealing | Sealed-day concept keeps; format and seal protocol change |
| sync script | **Refactor** | Add per-day reconciliation + one-command chain |
| db_writer / watchdog | **Keep** | Verified good (watchdog stops firing spuriously once B.5.3 lands) |
| frontend store/SSE/DataTable/chart internals | **Keep** (+ SSE error handling) | Genuinely well-optimized |
| frontend columns / strategy pages / price display | **Refactor** to typed base + descriptor + quote-aware units | Duplication + SOL hardcoding |
| ingest-websocket | **Delete** | Empty scaffold; the seam survives in `IngestHandles` |

## B.9 Migration path

**Phase 0 — ship immediately (real-money bugs, no redesign dependency):**
1. Sell re-send guard (B.5.1) — **S**
2. Token-account refcount + same-mint exit serialization (B.5.2) — **S/M**
3. Consumer ping decoupling (B.5.3) — **S**

These three close the only paths to unintended on-chain spends and stalled real exits. Nothing else should merge ahead of them.

**Phase 1 — lab quick wins (independent of everything):** rayon sizing, spawn_blocking, corpus-cache reuse, stage_mints (**S**); Arrow corpus load (**M**). Frontend lint rule + SSE hardening can also land here (**S**).

**Phase 2 — schema v2 + lake v2 (the keystone):** new PG baseline, dimension tables, `_raw` columns, real supply, decimals-aware views; lake v2 export + manifest sealing + sync reconciliation; legacy backfill mapping. **L.** Unblocks Phases 3, 5, and T1–T4.

**Phase 3 — crate topology + venue abstraction:** `venue-core` traits, `venues/registry`, dissolve pump-trader → `sol-executor` + `venue-pumpfun`, `ingest-grpc` transport-only, integer-unit decode, TraderHook v2, per-venue revert classification. **L.** Depends on Phase 2 event/column shapes; trait design can start in parallel.

**Phase 4 — strategy framework unification:** tpsl merge (+ parity test vs v1 outputs), `Strategy`/`StrategyDescriptor`, generic backtest/sweep/registry, shared sweep tables, per-quote params. **M/L.** Runs largely **in parallel with Phase 3** (touches strategy-core + lab, not venues).

**Phase 5 — frontend v2:** typed column base + strategy-page descriptor (**M**, independent); quote-aware display (**S/M**, gated on Phase 2 API).

**Phase 6 — first new venue as proof:** T1 USDC-paired pump tokens (exercises the quote axis end-to-end with the least new decode work), then T2 Bonk (exercises the venue axis). **M each.** This is the acceptance test of the whole design: **if T1 requires touching anything outside `venues/pumpfun` config + data, the abstraction leaked.**

Dependency picture: `0 → (1 ∥ 2) → (3 ∥ 4) → 5 → 6`, with 1 and most of 5 free-floating.

**Critical files for implementation:**
- `live/src/strategies/execution/real.rs` — Phase 0 sell re-send + refcount fixes; later venue-parametrized revert routing
- `trading_core/src/ingest.rs` — TraderHook v2 / IngestHandles, the contract every crate move pivots on
- `pump-trader/src/protocol.rs` — dissolution point: Tier-1 → venue-pumpfun, Tier-2 → sol-executor
- `ingest-laserstream/src/event.rs` — IngestEvent v2 (integer quote/base units, VenueId) shape
- `lab/src/sweep/registry.rs` — StrategyDescriptor replaces the per-strategy wiring and table triples

---

# Appendix — Docs↔code drift

- `api/handlers/mod.rs:4-5` vs `api/handlers/strategies/mod.rs:6-7` — contradictory comments about where the rule domain lives; it is `strategies/rules.rs` now (the `tpsl_rules_core` name in CLAUDE.md no longer exists).
- `0001_init.sql:214` says `strategy_id` is `'tpsl1'|'tpsl2'`; registry persists canonical `tpsl_sniper_1/2` (`registry.rs:49-54`).
- `models/token.rs:21` documents `initial_supply_token` as "first creator buy" while every market-cap SQL treats it as total supply (root of H1).
- `state/mod.rs:2-4` claims core_state/token_list_cache "live with the api layer"; they live in `state/`.
- `lab/src/sweep/corpus.rs:6-8` and `duck.rs:5,30,370` reference the deleted `DbSource`/`attach_fingerprints`; `registry.rs:5-6` says "adding swing later" though swing_1 is fully wired.
- `sharedEndpoints.ts:89,136` says Swing uses `getTokens`; it uses `getTokensPage` — `getTokens` has zero consumers.
- pump-trader sell doc says "up to 5 attempts"; the live loop is a separate 6-attempt loop (`execution/mod.rs:27`) — two loops, easily misread.
