# Bug-fix Plans — Index & Execution Order

These plans were split out of consolidated docs so each fix can be executed
**one at a time, step by step**. Each file has exactly one purpose. Run them in the
order below; dependencies are called out per file. Completed plans are deleted once
verified in the code.

**Workstream B (buy-sell-failures) is complete — all B1–B8 done and deleted.**

**Workstream A (`tpsl-realtime/`) is complete — A1–A5 done and deleted** (verified against
`live`/`trading_core`/`ingest-laserstream` on 2026-06-29). The reference doc
`00-gap-replay-mechanisms.md` was deleted with it (its purpose was to ground A3–A5).

---

## Open plans

| File | Fixes | Status |
| --- | --- | --- |
| [when-i-do-real-proud-mountain.md](when-i-do-real-proud-mountain.md) | Multi-account token sell + pre-buy consolidation + persisted `token_account` | ⬜ Not started (0/3 parts) |

---

## ⚠️ Path caveat — line refs in older plans predate TWO renames

Any file ref of the form `backend/src/...` in a plan was written **before** the single
`backend` crate was split, and the split crates were then **renamed again** to the
`live`/`lab`/`trading_core` topology (see [CLAUDE.md](../CLAUDE.md)). Map old paths to the
CURRENT crates before you edit:

| Old path prefix (in the plans) | Now lives in |
| --- | --- |
| `backend/src/strategies/tpsl_sniper_*/execution/real.rs` | **`live/src/strategies/execution/real.rs`** — the two tpsl1/tpsl2 sell clones are now **one unified file** (registry-dispatched); there is a single `classify_sell_revert` to edit |
| `backend/src/api/handlers/trading/*` | `live/src/api/handlers/trading/*` |
| `backend/src/api/handlers/system\|tokens/*` | `live/src/api/handlers/{system,tokens}/*` or `trading_core/src/api/handlers/*` (core handlers) |
| `backend/src/storage/repositories/*` | `trading_core/src/storage/repositories/*` |
| `backend/src/services/token_sync.rs` | `live/src/services/token_sync.rs` |
| `backend/src/ingest_laserstream/*` | `ingest-laserstream` crate (or `live/src/ingest/*`) |
| `backend/src/config/constants/*` | `trading_core/src/config/constants/*` |
| `backend/src/main.rs` | `live/src/main.rs` |
| `pump-trader/src/constants.rs` | constants moved to `pump-trader/src/protocol.rs` (Tier-1) + `config.rs` (Tier-2); `constants.rs` is a **back-compat shim** |
| `pump-trader/src/*` (other) | `pump-trader` crate (unchanged) |

Line numbers will have drifted — **re-grep the named symbol** rather than trusting the
exact line. Build checks are `cargo check -p live` / `-p lab` / `-p trading_core` /
`-p pump-trader` (older plans say `cargo check -p backend`/`-p backend-deploy`, which no
longer exist).

The `tpsl_sniper_1` / `tpsl_sniper_2` **decision** modules (entry/exit/cohort, in
`trading_core`) remain intentional clones — **every edit in one belongs in both** (memory
`tpsl-clones-intentional`). But the live **sell orchestration** is no longer cloned, so a
plan's "mirror in TPSL1 + TPSL2" note for the sell-revert path applies to a **single**
`real.rs` now.
