# Full-Project Audit + Redesign Blueprint

## STATUS (2026-07-03)

The audit has been executed (user said "run it"): 5 parallel code-first audits + manual verification of all Critical/High findings + a designed redesign blueprint are DONE. **Part A (ranked findings) is already written to `project-audit-and-redesign.md` at the repo root.**

**One step remains:** replace the `# Part B — Redesign blueprint` placeholder in `project-audit-and-redesign.md` with the completed blueprint (target topology dissolving pump-trader into `sol-executor` + `venue-pumpfun`, Venue/CurveModel/QuoteUnit trait design, schema v2 with venue+quote as data and `_raw` integer units, generic StrategyDescriptor framework, ordered live/lab fix lists, keep-vs-rebuild table, 6-phase migration path). No other files will be touched.

## Context

The user wants a deep, code-first review of the entire meme-trading project (6 Rust crates + 3-tree React frontend) to find what is **not effectively implemented**, then a **target redesign blueprint** — big changes and from-scratch rebuilds are explicitly acceptable, including the DB schema and Parquet lake. This session first refined the user's rough prompt into an explicit spec (below); executing that spec is the plan.

## The Refined Prompt (agreed spec)

> **Task:** Deeply audit the entire project from source code (do NOT rely on `@arch/` / `@plans/` md docs — read the actual implementations; use docs only to locate code, and flag where docs and code disagree). Then produce ONE markdown report: **(A) ranked audit findings + (B) target redesign blueprint**.
>
> **The system is 3 backend parts + 3 frontend trees:**
> - `core` (`trading_core`) — pure shared logic. Audit for: logic that leaked into live/lab but belongs here (and vice-versa), impure dependencies, boundary violations.
> - `live` — real trading execution on EC2 **2 vCPU / 4 GB**. Audit for: latency, stability, and correct concurrency on every trading-execution path (ingest → strategy eval → buy/sell → sell-confirm); memory footprint; blocking/lock-contention/alloc-per-event hazards; failure/restart safety. Real-money paths must be stable, optimized, concurrent.
> - `lab` — big-data analysis on workstation **8 CPU / 32 GB**. Audit for: raw speed and parallelism on large token/trade history (sweep engine, backtests, lake/DuckDB, swing analyzer); wasted single-threaded work; inefficient data movement between PG ↔ lake ↔ memory.
> - `frontend-react` (`shared`/`live`/`lab`) — **same depth as backend**: state efficiency, re-render behavior on high-frequency streams, component/hook reuse, split hygiene.
>
> **Audit dimensions (every one, per subsystem):**
> 1. Performance (against each box's hardware profile above)
> 2. Data handling — Postgres schema/queries/pools, Timescale usage, Parquet lake, caches, channels, SSE
> 3. Workflow — ingest→trade and sync→export→sweep pipelines; dev/ops workflow (scripts, env, deploys)
> 4. Modularity & code quality — module boundaries, duplication (e.g. the intentional tpsl1/tpsl2 clones — judge whether that tradeoff still holds), dead code, error handling
> 5. **Extensibility** — validate the design against 4 concrete future targets:
>    - Pump.fun **USDC-paired** tokens (breaks hardcoded SOL/lamports assumptions in pricing/reserves/PnL)
>    - **Other launchpads** (Bonk etc. — pluggable decoders, instruction builders, curve math)
>    - **Dynamic bonding curves** (spot-price/reserve math must be pluggable, not const)
>    - **Post-graduation AMM/DEX venues** (Raydium/Meteora/PumpSwap — routing, pool discovery, account layouts)
>    Specifically judge whether `pump-trader` and `ingest-laserstream` as isolated crates actually deliver this extension story, or only look isolated.
>
> **Constraints:** Nothing is frozen — DB schema, lake format, crate topology, frontend split are all up for redesign; historical data may be dropped/re-ingested. The blueprint should propose the *right* architecture, then note migration cost, not the reverse.
>
> **Output:** one md file in the repo root (e.g. `project-audit-and-redesign.md`) with: (A) findings ranked by severity/impact, each with file:line evidence; (B) redesign blueprint — target crate/module topology, venue/transport/quote-currency abstraction design, data-layer design, what to keep vs rebuild. Keep chat output minimal until done; the file is the deliverable.

## Execution Plan

1. **Parallel code-first exploration** — fan out Explore agents (read-only), each with a bounded territory and instructed to read source, not md docs:
   - Agent 1: `trading_core` (models, storage/repos, state, api, strategy domain, ingest contract)
   - Agent 2: `live` + `pump-trader` + `ingest-laserstream` (hot trading path end-to-end)
   - Agent 3: `lab` (sweep engine, lake/duck, backtests, swing analyzer) + data pipeline scripts
   - Agent 4: `frontend-react` all three trees
   - Agent 5: cross-cutting extensibility probe — grep for hardcoded pump.fun/SOL assumptions (const pubkeys, lamports math, curve formulas, `WSOL`, quote-currency assumptions) across all crates
   Each returns structured findings: {area, file:line, problem, severity, dimension}.
2. **Synthesis + verification** — I read the highest-severity claimed findings in source myself to confirm before ranking (no unverified claims in the report).
3. **Redesign blueprint** — Plan agent designs target architecture from confirmed findings + the 4 extension targets; I review and adjust.
4. **Write `project-audit-and-redesign.md`** at repo root: Part A ranked findings with evidence, Part B blueprint (topology, abstraction seams, data layer, keep-vs-rebuild, migration-cost notes).

## Verification

- Every Part A finding cites `file:line` that I or an agent actually read this session.
- Extensibility section explicitly answers, per each of the 4 targets: "what breaks today, where."
- Report contradicting an `@arch/` doc → flagged in a "docs drift" appendix.
- Deliverable is the single md file; chat output stays minimal per the user's context-saving request.
