# WebSocket ingest transport (Helius `transactionSubscribe`)

A second live transport for the ingest read-stack that speaks Helius WebSocket
JSON-RPC instead of Yellowstone gRPC, and is otherwise indistinguishable to
every consumer: same `IngestEvent` stream, same `IngestHandle`, same lanes, same
reconnect policy, same host adapter.

## Both feeds ship. Neither replaces the other.

The gRPC path stays exactly as it is. The WebSocket path is a peer, selectable
per run and switchable without a restart. Nothing in `ingest-pumpfun`,
`hunter/live`, or `forge/live` changes shape.

## What the current crate does right, and what the WS crate must copy

The laserstream stack is modular along **one axis: engine vs venue.**
`ingest-core` owns mechanism (transport, reconnect, session, event contract);
`ingest-pumpfun` owns program knowledge (classify, decode, pool PDAs) and a
back-compat façade. Eight properties make it work, and the WS crate inherits
every one of them:

1. **Neutrality is enforced by the crate boundary** — `venue.rs` states it:
   "Nothing in this module knows about pump.fun." `ingest-ws` gets the same rule
   one axis over: *nothing in it knows about a venue*, and nothing outside it
   knows about JSON-RPC.
2. **Static dispatch at every seam** — `Ingest<V>`, `transport::run<V>`, no
   `Box<dyn>` on the hot path. The feed seam is generic too.
3. **The owner of shared state is the one who mutates it** — the venue owns
   `PoolIndex` + `Notify` so transport and decoder share one instance.
4. **A façade absorbs churn** — consumers import `ingest_laserstream::…` and
   have never had to move. Adding a transport must not move a single import.
5. **Config is data; the crate never reads env.** The host builds `IngestConfig`.
6. **Provider-as-config** — `Auth` is an enum, swapping providers is data.
   Feed choice joins it as data.
7. **Optional payload paths are feature-gated** (`raw-tx`, `rpc-backfill`).
8. **Policy lives in one file and is unit-tested pure** — `resolve_from_slot`
   has five tests and no network. That policy is transport-independent and must
   not be copied into the WS crate; it must be *shared*.

Point 8 is the reason the WS path is not a fork. The reconnect/backoff loop, the
replay anchor, the create/normal lane split, the backpressure-forces-reconnect
shed, the idle watchdog, the live gate, the resubscribe debounce, the push-hook
contract, `IngestVenue`, `Ingest<V>` and the whole decoder are already
transport-agnostic — ~2500 lines. Forking them is a direct SSOT violation and
guarantees the two feeds drift.

## Recommended structure — four crates on three axes

LaserStream leaves `ingest-core`. The engine, the two transports, and the venue
become four peers, and `ingest-core` ends up owning **no** transport at all —
which is the only arrangement in which "venue-agnostic ingest engine" is
literally true.

```
shared/ingest/
  core/         ingest-core         the engine.    prost, NO tonic
  laserstream/  ingest-laserstream  gRPC feed.     deps: ingest-core + tonic
  ws/           ingest-ws           WS feed.       deps: ingest-core + tokio-tungstenite
  pumpfun/      ingest-pumpfun      venue + façade + feed selection
                                    deps: ingest-core + ingest-laserstream + ingest-ws
```

### The big picture

```
                       HOST   hunter/live · forge/live
                       live/src/ingest/mod.rs :: spawn_ingest()
                                    │
                    IngestConfig { feed: Grpc | Ws, .. } + PushHooks
                                    ▼
 ┌──────────────────────────────────────────────────────────────────────────────┐
 │ ingest-pumpfun            shared/ingest/pumpfun          lib ingest_pumpfun   │
 │ ─ the venue, and the assembly root ─────────────────────────────────────────  │
 │   IngestBuilder::feed(Feed::Grpc | Feed::Ws)   picks the impl, spawns run<V,F>│
 │   supervisor  watch<Feed>                      hot switch, overlap + dedupe   │
 │   PumpFunVenue   classify · decode · derive_pool · PoolIndex                  │
 └──────────────┬───────────────────────────────────┬───────────────────────────┘
                │ impl Feed                         │ impl Feed
                ▼                                   ▼
 ┌────────────────────────────────┐   ┌────────────────────────────────────────┐
 │ ingest-laserstream       MOVED │   │ ingest-ws                          NEW │
 │ shared/ingest/laserstream      │   │ shared/ingest/ws                       │
 │   generated/geyser_client.rs   │   │   conn · rpc · subscribe               │
 │   feed.rs   GrpcFeed           │   │   notification · convert               │
 │   subscribe.rs                 │   │   dedupe · error                       │
 │   tonic · tokio-stream         │   │   tokio-tungstenite · serde            │
 └──────────────┬─────────────────┘   └──────────────┬─────────────────────────┘
                │                                    │
                └──── FeedUpdate::{Transaction, BlockMeta, Account} ────┐
                                                                        ▼
 ┌──────────────────────────────────────────────────────────────────────────────┐
 │ ingest-core               shared/ingest/core             lib ingest_core      │
 │ ─ the engine. no transport · no venue · no provider · no env ───────────────  │
 │   transport/feed.rs   Feed · FeedConn · Subscription · FeedUpdate · FeedError │
 │   transport/mod.rs    run<V,F>: reconnect · ReplayAnchor · lanes · shed· idle │
 │   session.rs          Ingest<V> · IngestHandle<V> · the two decode lanes      │
 │   venue.rs            IngestVenue        event.rs   IngestEvent contract      │
 │   proto/  MESSAGES only      convert.rs  json -> pb, the ONE converter        │
 │   config · error · slot_anchor · backfill · raw_tx        prost, NO tonic     │
 └──────────────────────────────────────────────────────────────────────────────┘
```

At runtime the two feeds are interchangeable upstream of one shared pipeline:

```
  gRPC stream ─┐                                       ┌── create lane ─► decode ─┐
               ├─► FeedConn::next ─► venue.classify ───┤                           ├─► IngestEvent ─► host
  WS socket ───┘         ▲                             └── normal lane ─► decode ─┘
        └─ parse task ───┘  (WS only: JSON -> protobuf, off the reader task)
```

### Why the move is clean

The objection to it would be that `SubscribeUpdateTransaction` is the engine's
own currency — every decoder signature names it — so the proto cannot leave
core. It does not have to. The generated file splits at a seam that is already
there:

- `generated/geyser.rs` lines 1-476 are **prost message types**. They stay.
- Line 477 onward is `pub mod geyser_client`, the **tonic client**. It moves,
  with a `use ingest_core::proto::geyser::*;` header.

There is no `build.rs` and no `.proto` in the tree — the generated code is
checked in and hand-maintained, so this is a one-time mechanical cut, not a
codegen constraint. The Yellowstone service definition is stable, so it does not
re-diverge.

The payoff: `ingest-core` drops `tonic` and `tokio-stream` outright, the two
transports become structural peers instead of one privileged and one bolted on,
and `forge/scripts/dep-partition-check.*` gets sharper — it can assert that
`forge-lab` pulls no transport crate at all.

### The name is the point, and it has to be sequenced

`ingest-pumpfun`'s lib target is currently named `ingest_laserstream` — a
back-compat alias from the pre-split layout, which is why 21 files say
`use ingest_laserstream::` to reach **pump.fun decode code**. That name is
actively misleading today and it blocks the new crate from claiming it.

[refactor-plan.md](../refactor-plan.md) already schedules the fix as an isolated
no-logic commit: `ingest_laserstream` → `ingest_pumpfun`, 21 files. **That
rename must land before the new crate exists**, or two packages contend for one
lib name. Afterwards the vocabulary finally means what it says:

| Name | Before | After |
| --- | --- | --- |
| `ingest_laserstream` | pump.fun decode façade | the LaserStream gRPC transport |
| `ingest_pumpfun` | — | the pump.fun venue + façade |
| dep key `ingest-laserstream` | → package `ingest-pumpfun` | → package `ingest-laserstream` |

Also repointed by that commit: the dep-key notes in `hunter/CLAUDE.md` and
`forge/CLAUDE.md`, `forge/README.md`, and both `dep-partition-check` scripts.

### Config ownership after the split

`IngestConfig` stays the single host-facing knob surface — the host builds one
struct, the crate still reads no env. `TransportConfig` keeps only the
transport-neutral policy fields (timeouts, backoff, idle, `pipeline_send_timeout`,
`resubscribe_debounce`, commitment). Each feed crate owns its own small config
extracted from `IngestConfig`: `GrpcConfig { http2_keepalive, tcp_keepalive,
max_decoding_message_size }`, `WsConfig { ping_interval, frame_channel_cap }`.
Feed-specific knobs never leak into the shared policy struct.

### What absorbs the churn

`ingest-pumpfun`'s `src/transport/mod.rs` façade re-exports
`ingest_core::transport::{LaserStreamClient, TransportConfig, XTokenInterceptor}`
for the one-shot replay service. It repoints to `ingest_laserstream::` — three
lines — and [hunter/live/src/services/laserstream_replay.rs](hunter/live/src/services/laserstream_replay.rs)
compiles untouched. That is discipline point 4 doing exactly its job.

### `ingest-ws` — one responsibility per module, mirroring the core discipline

```
Cargo.toml            package ingest-ws, lib ingest_ws
src/lib.rs            WsFeed + WsConfig — the ONLY public surface (façade discipline)
src/conn.rs           socket lifecycle: connect, split, reader task, ping timer,
                      close frame -> FeedError
src/rpc.rs            JSON-RPC envelope: request ids, subscribe/unsubscribe,
                      ack correlation, live subscription-id registry
src/subscribe.rs      Subscription -> transactionSubscribe / blockSubscribe /
                      accountSubscribe params. The analog of build_subscribe_request,
                      and unit-tested the same way (pure, no network)
src/notification.rs   typed borrowed `Deserialize` structs for the three notification kinds
src/convert.rs        notification -> SubscribeUpdateTransaction, delegating to core's
                      ONE converter. No second converter is written here.
src/dedupe.rs         bounded recent-signature ring, armed only during an overlap
src/error.rs          WsError -> FeedError, incl. the billing-shaped closes that must
                      map onto the existing no-replay branch
```

It knows no venue, no pump.fun, and reads no env — the same three prohibitions
`ingest-core` already lives under.

### `ingest-pumpfun` — the assembly root picks the feed

Feed selection lives here because it is the only crate that already depends on
both sides, which keeps the dependency graph acyclic (`ingest-ws` → `ingest-core`,
never the reverse). The builder grows one method:

```rust
Ingest::builder()
    .endpoint(url)          // gRPC endpoint; WS derives its URL from the same host
    .api_key(key)
    .feed(Feed::Ws)         // NEW. Default Feed::Grpc — existing callers unchanged.
    .protocol(Protocol::pump_fun())
    .config(IngestConfig::default())
    .build()?
    .start(live)
```

Everything downstream — `IngestHandle`, the two lanes, the decoder, the host
adapter in `hunter/live/src/ingest/mod.rs`, the consumer, the DB writer, the SSE
bridge — is untouched.

### Why neither transport is a feature flag on `ingest-core`

A Helius JSON dialect inside the neutral engine breaks the same boundary a
pump.fun symbol would, and it drags `tokio-tungstenite` + a TLS stack into every
consumer of `ingest-core` including builds that never stream. The same argument
retired gRPC from core: a feature flag would have left `tonic` in the engine's
manifest and kept one transport privileged over the other.

Crate boundaries also keep `forge/scripts/dep-partition-check.*` meaningful —
`forge-lab` must pull neither `ingest-laserstream` nor `ingest-ws`, and after
the split that assertion is exact rather than a `tonic` proxy.

## Switching between feeds

Two levels, in order.

**Startup selection** — `IngestConfig.feed: Feed::{Grpc, Ws}`, sourced by the
host from `INGEST_FEED=grpc|ws`. The crate still reads no env; `hunter/live`
does, like every other setting. Zero risk, and it is what makes the WS path
testable at all.

**Hot switch, no restart** — mirrors the mechanism already in the transport for
`live_rx` and `gap_replay_rx`: a `watch` channel into the transport supervisor,
plus `IngestHandle::set_feed(Feed)`. On change the active transport ends through
the existing graceful path (the same one `live_rx == false` uses — reason
`Graceful`, no reconnect), the supervisor awaits its `JoinHandle`, then spawns
the other. Both feeds write to the **same** `create_tx` / `normal_tx` lanes, so
nothing below the transport observes the switch.

The supervisor lives in `ingest-pumpfun` alongside the selection, for the same
acyclic-graph reason.

For a zero-gap switch, overlap the two feeds for a few seconds and let the
`dedupe.rs` ring drop the duplicates. That is the same component the WS
resubscribe overlap needs — written once, used twice. The same ring makes a
permanent dual-feed standby possible later; note it bills both streams.

## Wire parity — Helius WS vs LaserStream gRPC

`transactionSubscribe` is Yellowstone's transaction subscription with a JSON
skin. It is the only WS method that can feed the existing decoder.

| Ingest needs | gRPC `Subscribe` | WS `transactionSubscribe` |
| --- | --- | --- |
| Program + pool filter | `account_include` | `accountInclude` (cap 50 000) |
| Drop votes / failures | `vote`/`failed` = false | `vote`/`failed` = false |
| Commitment `processed` | yes | yes |
| Slot | `SubscribeUpdateTransaction.slot` | `result.slot` |
| Block position | `info.index` | `result.transactionIndex` |
| Signature | `info.signature` | `result.signature` |
| Message + instructions | `scb::Message` | `transaction.transaction[0]`, base64 bincode |
| Account keys + ALT | `loaded_{writable,readonly}_addresses` | `meta.loadedAddresses.{writable,readonly}` |
| Inner instructions | `meta.inner_instructions` | `meta.innerInstructions` |
| Logs | `meta.log_messages` | `meta.logMessages` |
| Pre/post balances | `meta.{pre,post}_balances` | `meta.{pre,post}Balances` |
| Token balances | `meta.{pre,post}_token_balances` | `meta.{pre,post}TokenBalances` |
| Fee | `meta.fee` | `meta.fee` |
| Resume from a slot | `from_slot` | **absent** |
| In-place filter change | resend `SubscribeRequest` on the stream | **unsubscribe + subscribe** |
| Block metas (push hook) | `blocks_meta` filter | separate `blockSubscribe` |
| Watched accounts (push hook) | `accounts` filter | separate `accountSubscribe` per key |

The decoder consumes the protobuf `SubscribeUpdateTransaction`, and that JSON
shape is byte-for-byte the shape `backfill::rpc_to_protobuf` already parses.
The WS transport therefore reuses that converter rather than growing a second
one — see *Converter SSOT* below.

## The three real gaps

### 1. No `from_slot`

`ReplayAnchor`, `resolve_from_slot` and `MAX_REPLAY_ATTEMPTS` have no WS
equivalent: a reconnect always resumes live and the gap is lost.

**Decision:** the WS feed reports `supports_replay() = false`; the session
force-disables gap replay and logs once at `warn` if the operator turns the
setting on. The default is already off, so this changes no current behaviour.
An RPC gap-backfill (reuse `backfill::pager`) is a separate, opt-in, credit-
spending follow-up, never a silent default.

### 2. Filter changes are not atomic

Pool tracking mutates `account_include` on the live stream today. WS has no
in-place update, so a pool add/remove is subscribe-new → unsubscribe-old.

**Decision:** overlap, never gap. Subscribe the new filter set first, keep both
subscription ids briefly, then unsubscribe the old one. During the overlap the
transport drops duplicates through a small ring of recent signatures — required
because nothing between the transport and the strategy fold dedups (only the PG
insert does), so a duplicate would double-count into live volume/flow metrics.
The ring is armed only during an overlap, so the steady-state hot path pays
nothing. The existing `resubscribe_debounce` still coalesces bursts.

### 3. Push hooks are separate subscriptions

`PushHooks` rides one gRPC subscription today. On WS:

- `on_block_meta` → `blockSubscribe` with `transactionDetails: "none"`,
  `showRewards: false` (blockhash + slot only, near-zero payload). Confirmed
  commitment, which is the right level for a blockhash to sign against.
- `on_account` → one `accountSubscribe` per watched key, `encoding: "base64"`,
  `commitment: "processed"`; carries `lamports` + `data`, matching the hook
  signature exactly.

Both ride the same socket, so the callback contract (`Fn`, cheap, non-blocking,
runs on the transport task) is unchanged.

## The feed seam

```rust
pub struct Subscription {
    pub filter_key: &'static str,
    pub account_include: Vec<String>,
    pub from_slot: Option<u64>,
    pub commitment: Commitment,
    pub blocks_meta: bool,
    pub watch_accounts: Vec<String>,
}

pub enum FeedUpdate {
    Transaction(SubscribeUpdateTransaction),
    BlockMeta { slot: u64, blockhash: String },
    Account { slot: u64, pubkey: String, lamports: u64, data: Vec<u8> },
}

pub trait Feed: Send + Sync + 'static {
    type Conn: FeedConn;
    fn supports_replay(&self) -> bool;
    async fn connect(&self, ep: &str, auth: &Auth, cfg: &TransportConfig,
                     sub: &Subscription) -> Result<Self::Conn>;
}

pub trait FeedConn: Send {
    /// MUST be cancel-safe: it is awaited inside the transport `select!`.
    async fn next(&mut self) -> Result<Option<FeedUpdate>, FeedError>;
    async fn resubscribe(&mut self, sub: &Subscription) -> Result<()>;
}
```

`transport::run<V, F: Feed>` keeps every line of the current policy — the outer
loop, `resolve_from_slot`, `DisconnectReason`, `ReconnectCounts`, jittered
backoff, the four-branch `select!`, the lane split, `send_timeout` →
`PipelineBackpressure`. The gRPC impl is the current body, mechanically moved.
`FeedError` maps to `DisconnectReason` per transport (the WS impl maps HTTP 429
and a `ResourceExhausted`-shaped close frame onto the same billing-storm branch
that already refuses to replay).

**Cancel-safety is load-bearing.** `next()` is dropped mid-poll every time
another `select!` branch fires. The WS impl therefore does not read the socket
inside `next()`: it owns a dedicated reader task and `next()` is an
`mpsc::Receiver::recv()`, which is cancel-safe by construction. That reader task
is also where the ping writer lives (the sink half after `split()`).

### Task layout (WS)

```
[reader task]      socket -> frames        (tungstenite; owns ping timer)
      | mpsc<Frame>, bounded
[parse task]       frame -> SubscribeUpdateTransaction -> venue.classify
      | create_tx / normal_tx (existing lanes, unchanged)
[decode task x2]   unchanged
```

The parse task is a **new hop that does not exist on the gRPC path**, and it is
the one place this design can go wrong — see below.

### Converter SSOT

`rpc_to_protobuf` becomes a thin wrapper:

```rust
pub fn json_tx_to_protobuf(v: &Value, tx_index: Option<u32>) -> Option<SubscribeUpdateTransaction>
pub fn rpc_to_protobuf(v: &Value) -> Option<...> { json_tx_to_protobuf(v, None) }
```

The backfill path keeps `tx_index = 0` (documented today in `event::Trade`); the
WS path passes `result.transactionIndex`, so live WS rows keep the canonical
`slot → tx_index → leg_index` order that the gRPC feed produces. The converter
module moves out from behind `rpc-backfill` so both features can reach it.

## Performance

The gRPC path decodes prost into borrowed byte slices; the decoder's hot
pre-filter compares 32-byte keys with no allocation. The WS path adds, per
transaction: a JSON parse, a base64 decode, a bincode deserialize, one base58
decode per loaded ALT address, one per inner-instruction data blob, and a full
rebuild of the protobuf structs. Expect **3-10x the CPU per transaction** and
materially more allocation. Absolute cost is still small (order tens of µs on a
~5 KB notification), but on the 2 vCPU box, at pump-firehose rates, on a single
task, it is exactly the kind of hot-path work the latency rule forbids leaving
unbounded.

Rules for the implementation:

- **`encoding: "base64"`, `transactionDetails: "full"`, `maxSupportedTransactionVersion: 0`.**
  Never `jsonParsed`: it is several times larger on the wire (billed by the
  byte), slower to parse, and the decoder needs the raw message anyway.
- **No `serde_json::Value` on the hot path.** Deserialize into a typed
  `#[derive(Deserialize)]` notification struct with `#[serde(borrow)]` on every
  string field, and build the protobuf directly. `Value` doubles the allocation
  count for nothing. (`json_tx_to_protobuf` keeps its `Value` signature for
  backfill; the WS path gets a typed sibling feeding the same builder fns.)
- **Parsing never runs on the reader task.** A stalled parse must back-pressure
  through the bounded frame channel and surface as `PipelineBackpressure`, the
  same shed the gRPC path already has, rather than silently growing a queue.
- **The create fast lane must stay honest.** Classification happens after
  parsing, so a create can queue behind swap parses in the frame channel — the
  exact latency the lane split exists to prevent. Measure `frame → create_tx`
  under load before trusting the WS feed for snipes. If the single parse task
  saturates, shard by slot across two parsers with a per-slot reorder barrier;
  do not shard without the barrier (slot ordering is a live-fold input).
- **Budget to prove, not assume:** parse+convert p99 per notification, frame
  channel depth p99, and create-lane arrival delta vs the gRPC feed running
  side by side.

## Measured wire shape

A bounded probe against the live endpoint (`accountInclude` = the pump.fun
program, `processed`, `base64`, `transactionDetails: full`, 1328 notifications,
10.29 MB) settles every field the converter depends on:

| Checked | Result |
| --- | --- |
| Plan serves `transactionSubscribe` | yes, on the standard `wss://mainnet.helius-rpc.com` host |
| `result.transactionIndex` | present on 1328/1328 |
| `meta.loadedAddresses` | present on 1328/1328, non-empty on 980 (74% use ALTs) |
| `transaction.transaction[1]` | `"base64"` |
| `meta.innerInstructions[].data` encoding | **base58** — 1413 of 7937 samples decode to the anchor self-CPI discriminator `e445a52e51cb9a1d`, and zero samples across all 7937 contain a base64-only character (`+ / = 0 O I l`) |
| Average notification | 7.9 KB |

`meta` also carries `returnData` and `costUnits`, which the protobuf has no home
for; the converter ignores them, as it already ignores unmapped RPC fields.

The base58 result is the one that mattered: it means
`backfill::rpc_to_protobuf` — which already `bs58::decode`s inner-instruction
data — parses these notifications **as written**. No second converter, and no
silent inner-instruction loss (which would have broken truncated-log leg
recovery without failing loudly).

## Credits

WebSocket streaming is metered by volume — 2 credits per 0.1 MB uncompressed,
plus 1 credit per connection — on the same key the trading path spends.

The probe moved 10.29 MB. Its wall clock and its distinct-slot count disagree
about the true streaming window (91 s vs ~84 slots ≈ 34 s of chain time), so the
rate lands somewhere in **116-310 KB/s ⇒ roughly 200-540k credits/day**, program
filter only. Tracked AMM pool PDAs push it higher. Treat this as an order of
magnitude, not a budget input: re-measure over a clean fixed window, with the
real pool set, before committing. Do not leave a WS feed running alongside gRPC
unattended — both bill.

## Phases

Phases 1-4 change no behaviour at all. Each is separately verifiable, and the
existing tests are the proof — if `resolve_from_slot`'s five tests and the
subscribe-request tests stay green, the refactor moved code and nothing else.

1. ~~**Verify the wire.**~~ Done — see *Measured wire shape*.
2. **Free the name.** `ingest_laserstream` → `ingest_pumpfun`, 21 files, plus
   the dep key and the doc/script references. Pure rename, no logic, its own
   commit — the one already queued in `refactor-plan.md`. Everything after this
   depends on it.
3. **Extract the feed seam.** `transport/feed.rs` (`Feed`, `FeedConn`,
   `Subscription`, `FeedUpdate`, `FeedError`) and `run<V, F: Feed>`. The gRPC
   code stays put for this step; only the seam is new.
4. **Move LaserStream out.** New `shared/ingest/laserstream` crate: the
   `geyser_client` half of the generated file, `connect`,
   `build_subscribe_request`, the stream loop as `GrpcFeed`, `GrpcConfig`.
   `ingest-core` drops `tonic` + `tokio-stream`; the pumpfun façade repoints its
   three re-exports.
5. **Converter SSOT.** `json_tx_to_protobuf(v, tx_index)`, `rpc_to_protobuf`
   reduced to a wrapper passing `None`, module lifted out from behind
   `rpc-backfill` so both features reach it.
6. **`ingest-ws`.** The new crate: reader/ping/parse tasks, resubscribe overlap
   + dedupe ring, `FeedError` → `DisconnectReason` mapping, replay reported
   unsupported.
7. **Push hooks.** `blockSubscribe` + `accountSubscribe` on the same socket.
8. **Selection + wiring.** `Feed::{Grpc, Ws}` on the builder, `INGEST_FEED` read
   by the host, WS URL derived from the existing Helius host. Host adapter
   unchanged.
9. **Hot switch.** `IngestHandle::set_feed` + the supervisor, with the overlap
   window reusing the dedupe ring.
10. **Parity run.** Both feeds live against the same PG, compared on
    `(signature, leg_index)`: coverage, per-signature latency delta, and
    duplicate/miss counts across a forced reconnect. This is what decides
    whether WS is trustworthy for snipes.

## Open

- A clean byte-rate measurement with the real pool set, before the credit draw
  is treated as known.
- Whether `blockSubscribe` is plan-gated on the same key (needed only for the
  block-meta push hook).
- RPC gap-backfill on reconnect: opt-in, costed, deferred.
- Whether a permanent dual-feed standby is wanted later. The dedupe ring makes
  it nearly free to build and roughly doubles the credit draw to run.
