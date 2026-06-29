# Bug-fix Plans — Index & Execution Order

These plans were split out of three consolidated docs so each fix can be executed
**one at a time, step by step**. Each file has exactly one purpose. Run them in the
order below; dependencies are called out per file.

## Two independent workstreams

| Folder | Domain | Source it came from |
| --- | --- | --- |
| [`tpsl-realtime/`](tpsl-realtime/) | Ingest + strategy real-trading bugs (caps, adoption, freshness, replay timestamps, gap-replay) | `from-now-i-ll-tell-rippling-thompson.md` |
| [`buy-sell-failures/`](buy-sell-failures/) | Buy/sell transaction failure modes (cashback 6024, routing, revert recovery, retries, monitoring) | `buy-sell-failure-cases-audit.md` + `fix-6024-cashback-sell-bug.md` |

The two workstreams are **complementary, not overlapping**. `buy-sell-failures`
explicitly defers "missed create/migration events" to a *separate ingest workstream*
(memory `missed-tokens-restart-replay-gap`) — that workstream is `tpsl-realtime`.

## Duplication that was removed

The standalone `fix-6024-cashback-sell-bug.md` was a **strict subset** of the audit:

| Old location | Old location | Canonical home now |
| --- | --- | --- |
| 6024 plan **Fix 1** (manual-sell reads `routing.cashback_enabled`) | audit **Fix 1** | [buy-sell-failures/01-manual-sell-6024-cashback.md](buy-sell-failures/01-manual-sell-6024-cashback.md) |
| 6024 plan **Fix 2** (`derive_token_pdas` hardening) | audit **Fix 4** | [buy-sell-failures/02-buy-path-cashback-hardening.md](buy-sell-failures/02-buy-path-cashback-hardening.md) |

The audit's versions are kept (they're the more complete ones — they also thread the
flag through the snipe buy path). The standalone 6024 file is gone.

---

## Execution order

### Workstream A — `tpsl-realtime/` (do these first; they protect funds)

| # | File | Fixes | Depends on |
| --- | --- | --- | --- |
| A1 | [01-concurrency-caps-inflight.md](tpsl-realtime/01-concurrency-caps-inflight.md) | Error 1 — caps ignored in real mode | — |
| A2 | [02-buy-adoption-orphan-cleanup.md](tpsl-realtime/02-buy-adoption-orphan-cleanup.md) | Error 2 — successful buys never become `Holding` | A1 reduces the trigger |
| A3 | [03-replay-blocktime-anchor.md](tpsl-realtime/03-replay-blocktime-anchor.md) | Error 4 — replay stamps `now()` instead of chain time | — (**prerequisite for A4**) |
| A4 | [04-snipe-freshness-gate.md](tpsl-realtime/04-snipe-freshness-gate.md) | Error 3 — sniped stale/dead tokens | **A3** (a replayed create looks fresh until A3 lands) |
| A5 | [05-gap-replay-settings-controls.md](tpsl-realtime/05-gap-replay-settings-controls.md) | Feature A — gap-replay toggle + window | safety layer; pairs with A4 |

Reference (read-only, not a fix):
[00-gap-replay-mechanisms.md](tpsl-realtime/00-gap-replay-mechanisms.md) — Error 5 design
analysis (the two replay mechanisms, only one drives trading).

### Workstream B — `buy-sell-failures/`

| # | File | Fixes | Priority | Status | Depends on |
| --- | --- | --- | --- | --- | --- |
| B1 | [01-manual-sell-6024-cashback.md](buy-sell-failures/01-manual-sell-6024-cashback.md) | Fix 1 — manual-sell 6024 | P0 | ✅ done | — |
| B2 | [02-buy-path-cashback-hardening.md](buy-sell-failures/02-buy-path-cashback-hardening.md) | Fix 4 — cache the true cashback flag at buy | P0 | ✅ done | (defense for B1) |
| B3 | [03-manual-sell-reresolve-routing.md](buy-sell-failures/03-manual-sell-reresolve-routing.md) | Fix 2 — re-resolve routing inside clear loop | P0 | ✅ done | B1 (moves the same line) |
| B4 | [04-bot-curve-sell-revert-recovery.md](buy-sell-failures/04-bot-curve-sell-revert-recovery.md) | Fix 3 — bot 6024+6005 recovery | P0 | ✅ done | — |
| B5 | [05-manual-buy-slippage-and-confirm.md](buy-sell-failures/05-manual-buy-slippage-and-confirm.md) | Fix 5 + 5b — buy retry + confirm-timeout | P1 | — | 5b plumbs the signature 5 needs |
| B6 | [06-resolve-routing-retry.md](buy-sell-failures/06-resolve-routing-retry.md) | Fix 6 — `resolve_buy_routing` retry | P1 | ✅ done | — |
| B7 | [07-constant-rot-nonce-monitoring.md](buy-sell-failures/07-constant-rot-nonce-monitoring.md) | Fix 7 — constant-rot + nonce metrics | P2 | — | — |
| B8 | [08-slippage-doc-dead-const.md](buy-sell-failures/08-slippage-doc-dead-const.md) | Fix 8 — slippage doc + dead const | P2 | — | — |

Reference (read-only, not a fix):
[00-failure-case-catalog.md](buy-sell-failures/00-failure-case-catalog.md) — the full
A–E failure-case catalog with current status of every case.

---

## ⚠️ Path caveat — line refs predate TWO renames

Every file ref of the form `backend/src/...` in these plans was written **before** the
single `backend` crate was split, and the split crates were then **renamed again** to the
`live`/`lab`/`trading_core` topology (see [CLAUDE.md](../CLAUDE.md)). The plans' bodies
still say `backend-deploy`/`backend-core`; map to the CURRENT crates before you edit:

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
`-p pump-trader` (the plans say `cargo check -p backend`/`-p backend-deploy`, which no
longer exist).

The `tpsl_sniper_1` / `tpsl_sniper_2` **decision** modules (entry/exit/cohort, in
`trading_core`) remain intentional clones — **every edit in one belongs in both** (memory
`tpsl-clones-intentional`). But the live **sell orchestration** is no longer cloned, so the
plans' "mirror in TPSL1 + TPSL2" notes for the sell-revert path (B4) apply to a **single**
`real.rs` now.
