# NATS relay transport — the second bonding-curve source

A third party subscribes to Helius `transactionSubscribe` once and rebroadcasts the raw
`transactionNotification` frames on a NATS subject. Consuming that subject costs no
Helius credits, so the bonding curve moves off the metered plan while AMM pool traffic
stays on LaserStream.

Module map: [@arch/ingest.md](../../arch/ingest.md). Reconnect rules:
[reconnect-restart-flow.md](reconnect-restart-flow.md).

## The two roles

Ingest covers two disjoint slices of traffic, and only one of them can move.

| Role | Filter it needs | Sources that can serve it |
| --- | --- | --- |
| **Bonding curve** | the pump.fun program id — the same set for everyone | `nats`, `grpc` |
| **AMM pools** | *this bot's* tracked pool PDAs, changing as tokens migrate | `grpc` only |

A broadcast relay publishes one stream to every subscriber, so it cannot carry a filter
that depends on which tokens this bot holds. AMM therefore stays on gRPC in every
configuration, which is also what keeps an open position's feed alive across a switch.

## Workflow

```text
                    ┌────────────────────────────────────────┐
                    │  hunter/live  ·  ingest.curve_source    │
                    │  boot: CURVE_SOURCE env                 │
                    │  live: PUT /api/system/curve-source     │
                    └────────────────────┬───────────────────┘
                                         │ watch<AppSettings>
                                         │   -> IngestHandle::set_curve_feed
                                         ▼
                  assembly.rs   watch<FeedKind>       no restart, no rebuild
                    scope_for(feed, curve, caps.server_filter)
                    ┌────────────────────┴───────────────────┐
        StreamScope │ CURVE                       StreamScope │ POOLS
                    ▼                                        ▼
  ┌──────────────────────────────────┐   ┌──────────────────────────────────┐
  │  ingest-nats                     │   │  ingest-laserstream (gRPC)       │
  │  SUB helius.raw.bondingcurve     │   │  account_include =               │
  │                                  │   │    scope.program                 │
  │  reader task: socket -> queue    │   │      -> pump program id          │
  │    shed on full (never blocks)   │   │    scope.pools                   │
  │  frame.rs: JSON -> protobuf      │   │      -> tracked pool PDAs        │
  │                                  │   │                                  │
  │  CAPS replay:false               │   │  CAPS replay:true                │
  │       server_filter:false        │   │       server_filter:true         │
  │       in_place_resubscribe:false │   │       in_place_resubscribe:true  │
  │  credits FREE                    │   │  credits PAID                    │
  └────────────────┬─────────────────┘   └────────────────┬─────────────────┘
                   │ ingest-core::convert                 │ already protobuf
                   │ json_tx_to_protobuf()                │
                   └───────────────┬──────────────────────┘
                                   ▼
                    FeedUpdate::Transaction           one currency, one shape
                                   ▼
                 ingest-core::supervisor::run<V, F>   ONE loop, per feed
                                   ▼
                     dedupe::SignatureDedupe          absorbs the hand-over
                                   ▼                  and migration double-match
                        venue.classify()
                          ┌────────┴────────┐
                   Create │                 │ Curve / Amm
                          ▼                 ▼
                   create lane          normal lane
                          └────────┬────────┘
                                   ▼
                         venue.decode() -> IngestEvent
                                   ▼
                consumer -> { db_writer · token cache · SSE · strategy }
                                                          UNCHANGED
```

## Switching

`IngestHandle::set_curve_feed` re-points the curve with no restart. It is a
**hand-over, not a cut-over** — widen first, narrow last:

1. The relay feed is spawned whenever `NATS_URL` is set, *regardless of the selection*.
   When its scope is empty it idles fully disconnected, so the subject costs no
   bandwidth and reselecting it is a connect, not a process restart.
2. `assembly.rs` gives the feed **gaining** the curve its `program` scope immediately,
   and starts a `HANDOVER` timer.
3. Both wires carry the curve for that window. The dedupe ring drops the copies, so
   nothing is double-booked.
4. Only then does the feed **losing** the curve narrow. Narrowing first cost 7-10 slots
   of trades in each direction, measured on mainnet: a relay connect + subscribe runs
   ~2.2 s, and the old owner had already dropped the program id.
5. gRPC applies its new scope by **resubscribing on the open stream** (`CAPS
   .in_place_resubscribe`). The connection is never dropped, so the AMM side sees no gap
   at any point.
6. Selecting `nats` with no `NATS_URL` configured is persisted but not applied: the curve
   stays on gRPC and logs a warning. A curve pointed at a wire that cannot run is
   silently dead ingest, which is the worst failure this path has.

`HANDOVER` (5 s) must stay inside `IngestConfig::dedupe_window` (30 s) — the ring is what
makes the overlap free. A unit test pins the two together.

## Format conversion

`ingest_core::convert` is the single adapter for every JSON transaction source — the RPC
backfill, this relay, and any future WebSocket transport. It auto-detects the encoding
from the JSON itself.

| Publisher encoding | `transaction.transaction` | Handling |
| --- | --- | --- |
| `base64` | `["<b64>", "base64"]` | bincode `VersionedTransaction`, the pre-existing path |
| `jsonParsed` | `{ signatures, message }` | rebuilt field by field (below) |

**Why `jsonParsed` still produces a gRPC-shaped update.** It pre-resolves address-lookup
table keys inline into `message.accountKeys`, tagging each with `source` and `writable`,
and emits them in the order `static ++ loaded_writable ++ loaded_readonly`. That is the
same flat index space `decode::grpc::key_at` walks, so the converter splits the array
back into `message.account_keys` + `meta.loaded_{writable,readonly}_addresses` instead of
flattening it. A NATS-sourced update is then shaped exactly like a gRPC one, and
`raw_txs.payload` stays one format. `parsed_keys_preserve_the_flat_index_space` guards
the ordering assumption; an interleaved array falls back to one flat `account_keys`,
which keeps indices correct and loses only the static/loaded split.

Instruction `accounts` arrive as pubkey strings rather than `u8` indices, so the
converter maps them back through the key index. **An unresolvable pubkey rejects the
whole transaction** — a shifted index decodes into a different wallet, so guessing is
worse than dropping.

### Parity with the gRPC subscription

The gRPC subscription filters server-side with `vote: false, failed: false`. A relay
applies no such filter, so the NATS task screens failures locally via
`convert::json_tx_failed`. Without that, failed transactions decode into phantom trades
on one source and not the other.

### Instruction data the node consumes

Instructions belonging to a program the RPC node parses (`system`, `spl-token`,
`spl-token-2022`, ATA) arrive as `{program, parsed}` with raw `data` dropped.
`convert::data_from_parsed` re-encodes those bytes from `parsed`, because `ix_labels` is
built from the instruction discriminator: an empty `data` labels every one of them
`"System Program: Unknown"`, which blinds the ix-pattern fingerprints on the whole NATS
feed. The rebuild is **byte-exact or nothing** — an uncovered instruction type keeps an
empty `data` and its `Unknown` label, so a payload is never short or invented.
`system_rebuild_matches_the_sdk_encoder` pins the system layouts against
`solana_sdk::system_instruction` itself, so the tags cannot drift from the SDK's.

pump.fun and ComputeBudget are not in the node's parsed set, so they arrive raw with
base58 `data` on either encoding — trade decode and the `cu_limit`/`cu_price` extraction
are unaffected by the publisher's choice.

### Instruction `accounts` on a parsed instruction

jsonParsed carries **no `accounts` array** on a `{program, parsed}` instruction: the node
folds the accounts into `parsed.info` under role names, and `info` serialises as a sorted
object, so the order is gone from the JSON. It is not gone from the parser, though —
solana-transaction-status writes every role as `"<role>": account_keys[accounts[N]]`, so
`convert::accounts_from_parsed` reads that mapping back in index order. The role tables
are transcribed from solana-transaction-status's `parse_system`, `parse_token` and
`parse_associated_token` modules, including the `parse_signers` tail (one authority, or a
multisig authority followed by its `signers`). `system_accounts_match_the_sdk_builders`
pins the system order against the `solana_program::system_instruction` builders
themselves.

### What jsonParsed cannot round-trip, and what to emit instead

A few instructions have **more than one valid encoding**, and the parsed view records the
call rather than the encoding. Measured against chain (`getTransaction encoding=json`,
public RPC, free), all four shapes below occur in both forms on this feed, so no rule
recovers the original — the choice is which valid encoding to emit.

| Shape | Forms on chain | Emitted |
| --- | --- | --- |
| ATA `create` data | `[0]` discriminant, or zero bytes (pre-1.0.5) | `[0]` — 20 of 25 sampled, and what the rest of the family encodes |
| ATA `create`/`createIdempotent` accounts | 6, or 7 with a trailing rent sysvar (pre-1.0.4) | the required 6 |
| `system:createAccountWithSeed` accounts, `base == source` | 3 (Rust SDK), or 2 (`@solana/web3.js`) | the required 2 |
| `spl-memo` accounts | the signer accounts, or none | none — the parser discards their identity |

The rule is **the minimal list the program accepts for that instruction**: it stays a
valid encoding of the same call under either builder, and never names an account the
instruction does not touch. Dropping the list entirely would not be valid — an empty
`accounts` claims the instruction touches nothing. A role whose *identity* the parsed view
does not carry is never invented.

None of the four changes what the instruction does, and none changes `ix_labels`:
`label_instruction` reads `Create` from an ATA `data` of `[0]` or of zero bytes alike. A
memo's `data` **is** recoverable — the parsed string's UTF-8 bytes are the instruction
data — so only its accounts are lost.

Verified end to end against chain over 120 captured transactions: **584 of 593
instructions byte-identical**, the 9 differences being exactly the shapes above.

### Arrays that must not be silently short

An index-aligned array that quietly drops one element shifts every account above it, and
the trade prices against a different wallet. `pre/postBalances` (indexed by account
position by `decode::grpc::compute_sol_change`), the loaded-address vectors (which extend
the flat key space) and the token balances (which carry the trade's token amount) are
therefore **all-or-nothing**: one malformed element rejects the whole transaction, which
surfaces as `unparseable` in the NATS stats instead of as a wrong trade. `json_tx_to_protobuf`
additionally rejects a frame whose balance arrays disagree in length with the resolved key
space. Log lines and address-table indexes carry no positional meaning to any decoder and
stay lenient.

## Slow-consumer defence

Core NATS is at-most-once with no history. A consumer that falls behind is **disconnected
by the server**, not buffered — so the read loop's only job is to stay fast.

- The reader task moves frames into a bounded queue and does nothing else. JSON parsing
  and decode dispatch run on a separate task.
- A full queue **sheds** the frame and counts it (`nats::shed_frames`). Shedding is what
  keeps the reader fast enough to stay connected; blocking would end the connection.
- A blocked decode lane also sheds rather than reconnecting. The gRPC transport
  deliberately reconnects under backpressure so the provider can replay the gap; here a
  reconnect loses the same frames *and* costs a resubscribe, so it buys nothing.

A nonzero shed count means the decode pipeline, not the network, is the bottleneck.

## Dedupe

`dedupe::SignatureDedupe` is a fixed ring of `AtomicU64` slots holding the first 8 bytes
of each signature: lock-free, no allocation per transaction, no background sweep. Two
transports deliver the same signature in two situations — the overlap during a switch,
and a migration transaction that touches both the venue program and a tracked pool PDA.
It is built only when `NATS_URL` is set, so a single-transport deployment skips the check
entirely.

The window is enforced by **capacity, not a clock**: an entry survives until roughly
`capacity` further signatures pass through. `for_window` sizes the ring from
`dedupe_window` at a deliberately generous throughput ceiling, so the real window is at
least the one asked for.

## The NATS client

`ingest-core::nats::client` implements the wire protocol directly on tokio TCP. Every
NATS crate on crates.io hard-depends on `nkeys`, whose curve25519-dalek 4.x pulls a
`zeroize` requirement that cannot co-exist with the curve25519-dalek 3.2.1 that solana
1.17.27 pins across this workspace — the resolver rejects the combination outright.

Supported: plaintext `nats://`, user/password or token auth from the URL, comma-separated
seeds, `SUB` with an optional queue group, inline `PING`/`PONG`. Not supported: TLS,
JetStream, request/reply, nkey auth. A server demanding TLS or nkeys reports a clear
configuration error rather than failing to connect for an unexplained reason.

## What the publisher controls

Two settings on the relay side change what this bot pays for. Neither is required — the
transport handles the current shape — but both are strictly better.

| Ask | Why |
| --- | --- |
| `encoding: "base64"` instead of `jsonParsed` | About half the bytes on the wire, and the converter takes the bincode path: no pubkey→index remapping, no instruction-data rebuild |
| One subject per stream, no mirrored duplicate | A relay that publishes the same frames to both a specific and a catch-all subject doubles bandwidth for no extra data |

Alongside `base64`, `transactionDetails: "full"` and `maxSupportedTransactionVersion: 0`
keep the frame complete and versioned transactions decodable.

Subscribe to an **exact** subject regardless. A wildcard picks up any mirror the relay
publishes; the dedupe ring drops the copies, but only after they cross the network.

## Configuration

| Key | Where | Meaning |
| --- | --- | --- |
| `CURVE_SOURCE` | env | Boot default: `grpc` (default) or `nats` |
| `NATS_URL` | env | Relay address. Empty disables the transport entirely |
| `NATS_SUBJECT` | env | Exact subject, default `helius.raw.bondingcurve` |
| `ingest.curve_source` | `app_settings` | The live selection; overrides the env default once written |

`PUT /api/system/curve-source {"curve_source":"nats"}` persists the key and publishes the
settings snapshot, which the ingest adapter turns into `set_curve_feed`.

## Verifying a relay

`cargo run -p ingest-pumpfun --features nats --example nats_probe -- 20` runs the exact
live path — relay, convert, classify, decode — with no database and no session, and
reports conversion rate, relevance rate, and decoded event counts. A healthy relay
reports 100% conversion and 100% relevance; anything less names which stage rejects
frames.
