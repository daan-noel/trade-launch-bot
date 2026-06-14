# Backend Performance Audit

Multi-agent review of the Rust `backend` + `pump-trader` crates focused on **performance, low-latency buy/sell, data management, caching, and API/SSE handlers**. Six subsystem reviewers fanned out (ingest, strategies, execution, api/db, sse/state, services); every finding was then adversarially re-checked against the actual source before inclusion.

- Severity scale: **high** = per-event waste on ingest/strategy hot path or unbounded growth · **medium** = per-request API/DB waste · **low** = cold-path inefficiency.
- The buy/sell send path and the sell-confirm loop are clean (no critical findings). The remaining weight is in **ingest per-event allocation** and assorted **bounded-query / cold-path** cleanups.

---

## Confirmed findings (open)

### HIGH — hot-path / unbounded

#### H1. Protobuf is transcoded to `serde_json::Value`, then re-parsed by the decoder (double work per event)
- **File:** [backend/src/ingest_laserstream/adapter.rs:24-137](backend/src/ingest_laserstream/adapter.rs#L24-L137) (decode side: `decoder/parse.rs`, `decoder/instructions.rs:199`)
- **Category:** throughput · **Hot path:** yes
- Every passing pump.fun/PumpSwap tx is converted from the already-typed protobuf `SubscribeUpdateTransaction` into a `serde_json::Value`: account keys base58-encoded into `Value::String`, instructions turned into `json!` maps, instruction data base58-**encoded**, balances boxed into `Value`. The decoder then immediately re-parses it.
- **Note:** the upstream pre-filter (only build the `Value` for pump/PumpSwap txs), the decode-once-per-tx of instruction data, and the account-key `String` re-allocation (decoder now threads `&[&str]` borrowed from the `Value` instead of cloning every key per tx — `tier-b-plan.md` Tier A recap) are now in place — so the *ignored-tx* build, the repeated re-decode, and the per-tx account-key alloc are gone. What remains is the adapter still base58-**encoding** data that the decoder then base58-**decodes** once — removable only by a protobuf-native decode (Tier B), which forks the shared decoder from token_sync's RPC `Value` path and is gated on a profile.
- **Caveat:** the `Value` is genuinely needed as the persisted `raw_tx` blob, so it can't be fully removed.
- **Profiled (2026-06-14, live, ~45 tx/s):** Value build **~2.7 ms** avg vs decode **~0.83 ms** (`INGEST_PROFILE` span). `built_ratio = 1.0` — the gRPC subscription already scopes server-side, so the Value is built for *every* received tx (the pre-filter saves nothing). Build dominates (~76% of hot-path CPU) and scales on a single task → bottleneck at high volume. **Tier B is justified.**
- **Fix:** **Tier B** — decode directly from typed protobuf into the event structs; build the `Value` only for the raw blob, off-thread in the DbWriter. (A mimalloc global allocator was tested first on the allocator-bound hypothesis — **no measurable change**, so the cost is CPU-bound in base58 + `Value` construction; mimalloc reverted.) See `tier-b-plan.md`.

### MEDIUM — per-request API/DB waste

#### M5. Whole 50K-trade buffer cloned while holding the DashMap shard lock (swing endpoints)
- **File:** [backend/src/api/handlers/tokens/swing.rs:125-126, 196-198](backend/src/api/handlers/tokens/swing.rs#L125-L126)
- The guard is now dropped before `detect_swings` runs (heavy work no longer under the lock), but `entry.trades.clone()` — the multi-MB deep copy of up to 50K `Trade`s — still happens **while the shard read-guard is held**. The ingest pipeline's `get_mut` on any mint in the same shard blocks for that copy; the batch endpoint amplifies this across 16 concurrent mints.
- **Fix:** store trades as `Arc<[Trade]>` in `TokenState` so readers clone a refcount instead of deep-copying under the guard.

