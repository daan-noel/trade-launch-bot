# Ingest follow-ups (post Tier B)

Tier B (protobuf-native ingest decode) **landed and is validated** — the live
parity soak ran clean (0 mismatches / 4200 live curve txs) and the protobuf hot
path decodes ~6× cheaper than the old `Value` build+decode. See the decoder fork
in [@docs/ingest.md](@docs/ingest.md) and the opt-in guard
`live_parity.rs::live_parity_soak`.

These are the **remaining, deliberately-deferred** items. Each is blocked on
something outside the Tier B scope; none is needed for correctness today.

## Blocked on token_sync moving off the `Value` path

The `Value` decode path (`decoder/json/`) is still the only decoder token_sync
(RPC + gRPC replay) can use, so it stays. Retire these only **after** token_sync
is moved off `Value`:

- `adapter::update_tx_to_value` — keep until
  [laserstream_replay.rs](backend/src/ingest_laserstream/laserstream_replay.rs)
  (cold replay) is pointed at `build_raw_blob`; then drop it.
- `HeliusDecoder::decode_result(Value)` / `decode_amm_live` — keep for token_sync.
  `decode_amm_live` is live-only and becomes dead once token_sync stops using the
  `Value` entry point.
- The whole `decoder/json/` subtree
  ([parse.rs](backend/src/ingest_laserstream/decoder/json/parse.rs),
  [instructions.rs](backend/src/ingest_laserstream/decoder/json/instructions.rs),
  [trade.rs](backend/src/ingest_laserstream/decoder/json/trade.rs)) — retire only
  if token_sync moves off `Value`.

## Optional / independent

- **Profiler** ([profile.rs](backend/src/ingest_laserstream/profile.rs) + its 2
  pipeline/client hooks + the `@docs/ingest.md` row): zero-cost when off; keep for
  regression checks, or drop the module + `pub mod profile;` + hooks together if
  no longer wanted now the win is measured.
- **DB write-cost reduction** via blob size/retention (`raw_transactions`
  retention, dropping `source='rpc'` rows) — an open question from the perf
  review, independent of Tier B.

## Larger piece (own project)

- **Move token_sync off `Value`** — this is what unblocks the whole `json/`
  retirement above. It's a real project (token_sync only has an RPC `jsonParsed`
  `Value`, no protobuf), not a quick cleanup.
