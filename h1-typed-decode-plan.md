# H1 — Typed-protobuf decode / raw-tx blob plan

Scope decision (confirmed): **keep the persisted `raw_tx` blob, built lazily; treat the grpc live path and the token_sync rpc backfill path together.**

## The constraint that drives everything

The `serde_json::Value` plays **two roles at once**:

1. **Decode input** — every decoder fn borrows subtrees out of `raw_tx.raw_data`
   (`extract_account_keys`, `prepare_instructions`, `instruction_data_bytes`,
   the trade/create/migrate decoders). See
   [decoder/mod.rs:124-125](backend/src/ingest_laserstream/decoder/mod.rs#L124-L125).
2. **Persisted blob** — the same `Value` is moved into `RawTransaction` and written
   to `raw_transactions` (grpc) by the DbWriter.

Because we keep the blob, **the `Value` must be built for every *relevant* tx
regardless.** The upstream pre-filter
([adapter.rs:37-43](backend/src/ingest_laserstream/adapter.rs#L37-L43)) already
skips the build for *ignored* txs. ⇒ "build lazily" is effectively **already in
place**; there is no remaining "skip the Value build" win while the blob stays and
the decoder stays `Value`-driven.

### Consumers of `raw_transactions` (verified)
- `find_by_signature` / `exists` — both `#[allow(dead_code)]`, no live callers.
- Writers: grpc DbWriter (`source='grpc'`), token_sync backfill (`source='rpc'`).
- The table is **write-only today** (replay/analysis aspirational). This is *why*
  fully dropping it was on the table — but we're keeping it.

## What H1 actually leaves on the table (two tiers)

### Tier A — small, safe, shared-decoder-compatible  ✅ DONE

Status: A1 landed — `extract_account_keys` returns `Vec<&str>` and `&[&str]` is
threaded through parse/instructions/trade/create; per-tx account-key `String` allocs
removed (only the handful that reach event structs are owned). A2 verified: the hot
trade path already base58-decodes each instruction once (`prepare_instructions`
caches; logs use base64) — no double-decode to fix; residual re-decodes are
cold-path Create/Migrate only, left as-is. `cargo check`/`clippy` clean (no new
warnings), 107 unit tests pass.

Remove redundant work while the decoder stays `Value`-driven (so token_sync is
unaffected and there's one decoder):

- **A1. account-key `String` re-allocation.** `extract_account_keys` clones every
  base58 string out of the `Value` into a fresh `Vec<String>`. Decoder fns that
  only compare/look up could borrow `&str` from the `Value` instead. Net: drop one
  `Vec<String>` + N `String` allocs per relevant tx. (Internal decoder change; no
  protobuf, no token_sync impact.)
- **A2. confirm no double base58-decode of instruction data.** `prepare_instructions`
  already decodes outer-ix `data` once and caches bytes in `PreparedIx.data`
  ([instructions.rs build_instruction_labels reuses `p.data`]). Audit task: verify
  inner-ix paths (`instruction_data_bytes`, `find_pump_ixs_anywhere`,
  `decode_trade_events_from_inner_ixs`) don't re-decode the same ix more than once;
  cache where they do.
- Note: the adapter's `bs58::encode` of instruction `data` **cannot** be removed —
  the persisted blob must carry the base58 string to stay a faithful Helius-shaped
  blob. Encode-for-blob + decode-for-logic are both genuinely needed; only true
  *duplicate* decodes are removable.

Expected gain: trims per-event allocation on the hot path; modest but real, zero
correctness risk, no path fork.

### Tier B — large, risky, forks the decoder  ⚠️ not recommended under these choices
"Decode directly from typed protobuf, build the `Value` only for the blob":

- Requires rewriting the whole `Value`-driven decoder (parse/instructions/trade/
  create) to read protobuf structs (`CompiledInstruction`, `account_keys: Vec<bytes>`,
  `Vec<u64>` balances) directly.
- The `Value` would then be built **only at persist time** — ideally off the ingest
  thread, inside the DbWriter flush — so base58 encoding leaves the hot path.
- **Blocker for "change both together":** token_sync has only an RPC `jsonParsed`
  `Value`, no protobuf. A protobuf-native decoder can't serve it, so this **forks**
  into two decoders (or a protobuf path + a retained Value path). That contradicts
  the single-shared-decoder assumption and doubles the surface of the repo's most
  attribution-sensitive code.
- Only justified if profiling shows the base58 encode + `Value` build on the ingest
  thread is a measured bottleneck *and* we accept moving blob construction to the
  DbWriter. Revisit only if Tier A proves insufficient.

## Recommendation (goal: cut ingest-thread CPU)

Key fact: the dominant ingest-thread cost here is **building the `Value`** (base58 of
signature + every account key + every instruction `data`, plus the `json!` tree) in
`update_tx_to_value`, on the pipeline thread. Tier A only removes the duplicate
`String` clones — it does **not** touch the Value build.

Crucial coupling: you **cannot** move the Value build off the hot path while the
decoder stays `Value`-driven — decode must run on the hot path (strategies need the
trade promptly) and a Value-driven decode needs the Value first. ⇒ cutting the
Value-build CPU == Tier B (protobuf-native decode, blob built off-thread in DbWriter).
There is no middle option that removes the Value build cheaply.

Counterweight (argues against rushing Tier B): the pre-filter
([adapter.rs:37-43](backend/src/ingest_laserstream/adapter.rs#L37-L43)) builds the
`Value` **only for relevant pump/PumpSwap txs** — the irrelevant firehose is dropped
by a cheap `log_messages` scan with zero allocation. So Value-build CPU scales with
*relevant-tx* rate, not total stream rate, and may already be small.

Therefore — **profile first, then choose:**

1. **Measure** the Value-build + decode cost per relevant tx and the relevant-tx rate
   on the ingest thread (e.g. a scoped timing span around `update_tx_to_value` +
   `decode_result`, or a sampling profile of the pipeline task).
2. If the Value build is **not** a measured hot-path cost → do **Tier A** only and
   close H1; the rewrite isn't justified.
3. If it **is** dominant → commit to **Tier B**: protobuf-native decode on the grpc
   hot path, build the persisted base58 `Value` blob inside the DbWriter flush
   (off the ingest thread = the literal "build lazily"). Accept the decoder fork:
   token_sync stays `Value`-driven (cold backfill, unaffected). Budget this as a
   careful rewrite of attribution-sensitive code with the decoder tests as the guard.

Do **Tier A regardless** — it's a strict, safe improvement and is cheap.

## Validation
- `cargo check --bin backend`, `cargo clippy` on touched files.
- `cargo test --bin backend` — decoder unit tests must stay green (attribution is
  the failure mode to guard); add a case if A1 changes borrow shapes.
- Confirm token_sync decode path is byte-for-byte unchanged (Tier A is grpc-side /
  internal-decoder only).
- Update [@docs/ingest.md](@docs/ingest.md) decoder section if borrow shapes change.

## Open question for follow-up
If the real goal was to cut DB write cost (not ingest CPU), the lever is the blob's
**size/retention**, not the decode — e.g. shorter `raw_transactions` retention or
dropping `source='rpc'` backfill rows. Separate from H1; flag if that's the intent.
