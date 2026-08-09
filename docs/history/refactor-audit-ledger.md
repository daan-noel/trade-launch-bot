# Full-repo refactor audit - completed / out-of-scope ledger

> **History.** Origin: seven parallel audits (2026-07-10) on `feat/restructure-hunter-forge`,
> re-verified against the working tree 2026-07-19 on `strategy-redesign`. This is the
> record of what was finished and what was deliberately dropped, kept so none of it is
> redone. Open items live in [../refactor-plan.md](../refactor-plan.md).

## What changed since the last re-verify (2026-07-13 → 2026-07-19)

The **strategy redesign fully landed** (commits `40965acd`…`07592d19`, FE `c114693c`…`e077a361`,
Phase 7 `b274512e`…`07592d19`). This reshaped the ground the old plan stood on:

- **Legacy strategy triplication is gone.** Phase 7 *deleted* the named tpsl1/tpsl2/swing1
  decision stack (`live`, `lab`, and the FE swing feature). There is now **one generic
  fold** — `hunter-engine::reduce`, driven live by `hunter/live/src/strategies/engine/` and in
  analysis by `lab`'s replay/sweep. The audit's old "C1 — keep the intentional clones (OUT OF
  SCOPE)" ledger entry is **reversed and moot**; do not look for the clones.
- **A generic metric/fingerprint engine is the new extensibility surface** — metrics are pluggable
  modules, fingerprints are shared DB rows, rule params are JSONB `{entry, exit, tp/sl}` with
  per-operator grammar. See the new **Extensibility** section — this is where new-feature work now goes.
- **Structure drift:** `forge` orchestrator is its own crate (`forge/orchestrator/`, was
  `forge/launcher/src/orchestrator/`). The hunter ingest event model moved out of `hunter/core`
  into a **neutral** crate `shared/ingest/core` and grew a real `IngestVenue` trait — the venue seam
  is now half-real, not purely aspirational.
- **Perf work landed** on the sweep/sim path (AVX-512 exit scan behind a per-run toggle `a8766d54`;
  phase-split sim timers `6fdd5746`; fold hot-path fixes) and on billed RPC (`3911d070`, `01f5f782`,
  `63a455df`). Several Phase-5 perf items were explicitly *deferred* by those commits, not done —
  they remain below.

---

## Status ledger — DONE / OUT (do not redo)

- ✅ **Phase 0 — all correctness/safety/security bugs B1–B17** (bundle CAS, funding serialization,
  AMM poison idiom, loud rail parsing, forge-lab auth, postgres required-password + loopback, e500
  sanitization, constant-time token compare, etc.). Verified green when fixed.
- ✅ **C3 — dead code (C3-1…C3-6)** deleted (ingest `websocket`, orchestrator `dryrun.rs` + funding
  graph, legacy `Position` mutators, forge/lab `run_export` stub, `db-incremental-sync.ps1` stub;
  `transfer_with_seed`→`system_transfer`).
- ✅ **Strategy triplication removed** (was "C1 — OUT OF SCOPE / keep clones"). Phase 7 deleted the
  tpsl/swing stack; one generic engine now. Ledger reversal noted above.
- ✅ **`crates/forge-live` / `crates/forge-lab` doc refs** — gone (paths are `forge/live` / `forge/lab`).
- ✅ **`deploy/DOCKER.md` six→two compose files** — already documents the two merged compose files.
- ✅ **forge `/health` route** — present on forge-live (`forge/live/src/http.rs:62`) and forge-lab
  (`forge/lab/src/http.rs:19`). (hunter still lacks it — see Deploy below.)
- ⛔ **OUT OF SCOPE (agreed with user):**
  - **C2 — forge↔hunter infra dedup** (`shared/db`, `shared/units`, `shared/sol-price`,
    ingest-consumer extraction, http-auth bootstrap, `task_fault`). forge was copied from hunter and
    is still WIP; do not extract to shared crates yet.
  - **hunter deps stay direct** (not `.workspace = true`) — intentional split (`Cargo.toml:55-56`).
  - **solana `resolver = "1"` pin** — explained in `Cargo.toml:2-8`.

