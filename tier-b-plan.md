# Tier B — Protobuf-native ingest decode (build the `Value` blob off-thread)

Supersedes `h1-typed-decode-plan.md` (Tier A landed; this is the scoped Tier B PR) and
consolidates the former `BACKEND-PERF-AUDIT.md`: that backend perf review (six subsystem
reviewers, adversarially re-checked) found the buy/sell + sell-confirm paths clean; its only
remaining open item is **H1 = this plan**, and its other finding (**M5**) is now done — both
recorded under "Already landed" below.

## Why (settled by profiling — do not re-litigate)

Live `fra`, ~45 tx/s quiet window, ~1500 txs (`INGEST_PROFILE=1`):

| Stage | avg | max |
| --- | --- | --- |
| Value build (`update_tx_to_value`) | **~2.7 ms** | ~18 ms |
| decode (`decode_result`) | **~0.83 ms** | ~4.2 ms |

- `built_ratio = 1.0`, `prefilter_reject_avg_us = 0.0` → the LaserStream **subscription**
  already scopes server-side, so the client-side pre-filter rejects ~nothing; the `Value`
  is built for **every received tx**. The "build only happens for a cheap relevant subset"
  counter-argument is dead.
- Value build is ~76% of combined hot-path CPU, ~3× decode, on a **single serial task** →
  ~370 tx/s saturates it → ingest backpressure. pump.fun peaks exceed that.
- mimalloc tested as a cheap alternative → **no measurable change** (CPU-bound in base58 +
  `Value` construction, not heap). Tier B is the only lever.

**Goal:** get the `Value` build (base58 of sig + every account key + every ix `data`, plus
the `json!` tree) off the ingest hot path. A Value-driven decode needs the Value first, so
the only way is: decode straight from typed protobuf; build the `Value` **only** for the
persisted blob, off-thread in the DbWriter.

## Already landed (Tier A + adjacent audit items)

- **Tier A (H1 partial):** upstream pre-filter, decode-once-per-tx of instruction data, and
  `&[&str]` account-key borrowing (the decoder threads keys from the `Value` instead of
  cloning every key per tx) — so the *ignored-tx* build, the repeated re-decode, and the
  per-tx account-key alloc are gone. What remains is the adapter base58-**encoding** data
  that the decoder then base58-**decodes** — removable only by this plan's protobuf-native
  decode.
- **Caveat (still true):** the `Value` is genuinely needed as the persisted `raw_tx` blob,
  so it can't be removed outright — only relocated off-thread (work item 3).
- **M5 (done 2026-06-14):** `TokenState.trades` is now `Arc<Vec<Trade>>`, so the swing
  (single + batch) and paper fill-poll readers refcount-clone under the DashMap shard guard,
  drop it, then `try_unwrap`-or-clone *off* the lock — the multi-MB deep copy no longer
  blocks the ingest writer's `get_mut`. The append hot path mutates in place via
  `Arc::make_mut` (copies only on the rare tick a reader still holds a snapshot, and that
  copy lands on the writer, not under a read guard). `Arc<Vec<Trade>>`, not `Arc<[Trade]>`:
  the buffer is appended every ingest tick, and an immutable slice would reallocate all 50K
  elements per trade.

## The accepted trade-off: the decoder forks

token_sync has only an RPC `jsonParsed` `Value`, no protobuf — a protobuf-native decoder
can't serve it. So Tier B **forks** the decoder:
- **grpc hot path** → new protobuf-native decode.
- **token_sync backfill (cold)** → keeps the existing `Value`-driven decode, byte-for-byte
  unchanged (Tier A's `&[&str]` threading stays here).

This doubles the surface of the repo's most attribution-sensitive code. The decoder unit
tests (107) are the guard; mis-decoding a trade from raw protobuf vs the Helius-shaped Value
is the failure mode to watch.

## Work items (in order; nothing pre-deleted)

### 1. Protobuf-native decoder
Rewrite the grpc decode path to read protobuf structs directly instead of borrowing `Value`
subtrees:
- `CompiledInstruction`, `account_keys: Vec<bytes>`, `Vec<u64>` pre/post balances.
- New entry point alongside (not replacing) `HeliusDecoder::decode_result(Value)`:
  e.g. `decode_protobuf(&SubscribeUpdateTransaction) -> DecodeResult`.
- Reuse the existing event structs / trade-create-migrate logic; only the **source of the
  bytes** changes (protobuf instead of `Value`).
- Add decoder unit tests covering the protobuf borrow shapes (mirror the existing `Value`
  cases so attribution parity is provable).

### 2. Change the pipeline channel payload
[client.rs](backend/src/ingest_laserstream/client.rs) / [pipeline.rs](backend/src/ingest_laserstream/pipeline.rs):
- Channel payload `serde_json::Value` → typed protobuf (`Arc<SubscribeUpdateTransaction>`).
- Remove the `Value` send in `client.rs` and the `decode_result(value)` call in
  `pipeline.rs::run`; call `decode_protobuf` instead.
- Apply the pre-filter on the typed protobuf (cheap `log_messages` scan), not via Value
  construction.

### 3. Relocate blob construction to the DbWriter
- Extract `update_tx_to_value`'s protobuf→Helius-JSON synthesis (`compiled_ix`, `inner_ix`,
  `account_indexes`, `token_balances`, the base58 encodes) into a DbWriter-side
  `build_raw_blob(&protobuf)`.
- Run it **only** for `save_raw` txs, inside the DbWriter flush (off the ingest thread =
  the literal "build lazily"). The blob stays a faithful Helius-shaped base58 blob.

### 4. Cleanup checklist — each item only after its replacement is wired + tests green
- `value_tx`/`value_rx` channel + `channel_pair`: retype to the protobuf payload.
- `adapter::update_tx_to_value`: **keep** until [laserstream_replay.rs](backend/src/ingest_laserstream/laserstream_replay.rs)
  (cold replay caller) is pointed at `build_raw_blob`; then retire from the client task.
- `HeliusDecoder::decode_result(Value)` / `decode_amm_live`: **keep** for token_sync. Delete
  `decode_amm_live` (live-only) only if token_sync also stops using the Value entry point.
- Value-driven extraction layer ([parse.rs](backend/src/ingest_laserstream/decoder/parse.rs),
  Value arms of [instructions.rs](backend/src/ingest_laserstream/decoder/instructions.rs)):
  **retained for token_sync** — do not remove. Full retirement is a follow-up only if
  token_sync moves off `Value`.
- Profiler ([profile.rs](backend/src/ingest_laserstream/profile.rs) + 2 hooks + `@docs/ingest.md`
  row): keep (zero-cost off) for regression checks, or drop module+`pub mod profile;`+hooks
  together once the win is re-measured.
- Dead repo readers `TransactionRepo::{find_by_signature, exists}`: adjacent cleanup — delete
  if still unused after the rewrite.

## Validation / DoD
- `cargo check --bin backend` + `cargo clippy` on touched files clean, no new warnings.
- `cargo test --bin backend` — decoder unit tests green; add protobuf-shape cases.
- token_sync decode path confirmed byte-for-byte unchanged.
- **Re-profile** against the baseline above with `INGEST_PROFILE=1`: Value build must leave
  the ingest thread (value_build_avg on the client task → ~0).
- Update [@docs/ingest.md](@docs/ingest.md) decoder section (the fork: protobuf hot path +
  retained Value path for token_sync).

## Out of scope (follow-ups)
- Moving token_sync off `Value` (would retire the whole Value extraction layer).
- DB write-cost reduction via blob size/retention (`raw_transactions` retention, dropping
  `source='rpc'` rows) — an open question raised in the perf review, separate from Tier B.
