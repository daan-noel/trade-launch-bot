# WebSocket ingest transport (Helius `transactionSubscribe`)

A third live wire for the ingest read-stack that speaks Helius WebSocket JSON-RPC
instead of Yellowstone gRPC, and is otherwise indistinguishable to every
consumer: same `IngestEvent` stream, same `IngestHandle`, same lanes, same
reconnect policy, same host adapter.

## The seam it plugs into already exists

The four-crate split this doc used to specify has shipped. `ingest-core` is the
engine and carries no wire; `ingest-laserstream` and `ingest-nats` are one wire
each; `ingest-pumpfun` is the venue plus `assembly.rs`, the only module that
names a transport. Structure and rules:
[hunter/docs/arch/ingest.md](../../hunter/docs/arch/ingest.md).

What that leaves for this crate is genuinely only the wire:

```
ingest-ws/
  Cargo.toml            package ingest-ws, lib ingest_ws
  src/lib.rs            WsFeed + WsConfig + CAPS - the ONLY public surface
  src/conn.rs           socket lifecycle: connect, split, reader task, ping timer,
                        close frame -> FeedError
  src/rpc.rs            JSON-RPC envelope: request ids, subscribe/unsubscribe,
                        ack correlation, live subscription-id registry
  src/subscribe.rs      Subscription -> transactionSubscribe / blockSubscribe /
                        accountSubscribe params. Pure, unit-tested like
                        laserstream/subscribe.rs
  src/notification.rs   typed borrowed `Deserialize` structs for the three kinds
  src/convert.rs        notification -> SubscribeUpdateTransaction, delegating to
                        `ingest_core::convert`. No second converter is written here.
```

It knows no venue and reads no env — the same prohibitions every feed crate lives
under. **Nothing in `ingest-core` or `ingest-pumpfun` changes except one arm in
`assembly.rs`.**

## What the caps already say for it

```rust
pub const CAPS: FeedCaps = FeedCaps {
    replay: false,              // no `from_slot`; also selects shed-not-reconnect
    server_filter: true,        // `accountInclude`, cap 50 000
    in_place_resubscribe: false // subscribe-new then unsubscribe-old
};
```

Those three flags are the whole integration. `replay: false` makes the supervisor
skip the resume point, warn once if the operator turns gap replay on, and shed
under back-pressure instead of reconnecting. `in_place_resubscribe: false` makes
a pool-set change reconnect rather than call `resubscribe`. `server_filter: true`
makes an empty account set mean "idle", and makes the idle guard judge a
program-carrying scope by transactions.

The one thing gap 2 below still asks for is an overlap *inside* the resubscribe,
which `in_place_resubscribe: false` currently converts into a reconnect. Decide
whether that is good enough before building the overlap.

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

**Decision:** already handled by the seam. The WS feed declares
`FeedCaps::replay = false`; the supervisor then never resolves a resume point and
warns once at `warn` if the operator turns gap replay on. The default is already
off, so this changes no current behaviour.
An RPC gap-backfill (reuse `backfill::pager`) is a separate, opt-in, credit-
spending follow-up, never a silent default.

### 2. Filter changes are not atomic

Pool tracking mutates `account_include` on the live stream today. WS has no
in-place update, so a pool add/remove is subscribe-new → unsubscribe-old.

**Decision:** overlap, never gap — the same shape `assembly.rs` already uses to
hand the curve between feeds. Subscribe the new filter set first, keep both
subscription ids briefly, then unsubscribe the old one. Duplicates fall to
`ingest_core::dedupe::SignatureDedupe`, which is required because nothing between
the feed and the strategy fold dedups (only the PG insert does), so a duplicate
would double-count into live volume/flow metrics. The existing
`resubscribe_debounce` still coalesces bursts.

Until that overlap exists, `in_place_resubscribe: false` makes the supervisor
reconnect on a pool-set change instead — correct, but it drops the socket.

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

## Implementation notes for the WS wire

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
`slot → tx_index → leg_index` order that the gRPC feed produces.

The converter already lives behind its own `json-tx` feature, reached by both the
RPC backfill and `ingest-nats`; only the `tx_index` parameter is still owed.
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
  through the bounded frame channel, exactly as `ingest-nats`'s reader does.
  Because `CAPS.replay` is false the supervisor sheds rather than reconnecting —
  a reconnect would lose the same frames and cost a resubscribe.
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

Phases 1-5 are done: the wire is verified, the name is freed, the feed seam
exists, LaserStream lives in its own crate, and the converter is shared. What
remains is the WS crate itself.

1. ~~**Verify the wire.**~~ Done — see *Measured wire shape*.
2. ~~**Free the name.**~~ Done — lib `ingest_pumpfun`, dep key `ingest-pumpfun`.
3. ~~**Extract the feed seam.**~~ Done — `core/src/feed.rs` + `supervisor::run<V, F>`.
4. ~~**Move LaserStream out.**~~ Done — `shared/ingest/laserstream`; `ingest-core`
   carries no `tonic` and no `tokio-stream`.
5. ~~**Converter SSOT.**~~ Done — `ingest_core::convert` behind `json-tx`, shared
   by the RPC backfill and `ingest-nats`; a `tx_index` parameter is still owed
   (the backfill passes `0` today, and WS should pass `result.transactionIndex`).
6. **`ingest-ws`.** The new crate: reader/ping/parse tasks, typed borrowed
   deserialization, `FeedError` mapping (HTTP 429 and a billing-shaped close
   frame → `FeedError::Exhausted`, the variant that already forbids a replay).
7. **Push hooks.** `blockSubscribe` + `accountSubscribe` on the same socket.
8. **Selection.** `FeedKind::Ws` + one arm in `assembly.rs`, `INGEST_FEED` read by
   the host, WS URL derived from the existing Helius host. `scope_for` needs no
   change — it is already O(feeds).
9. ~~**Hot switch.**~~ Already built: `IngestHandle::set_curve_feed` plus the
   widen-before-narrow hand-over in `assembly.rs`, verified on mainnet with zero
   slot holes in either direction.
10. **Parity run.** Both feeds live against the same PG, compared on
    `(signature, leg_index)`: coverage, per-signature latency delta, and
    duplicate/miss counts across a forced reconnect. This is what decides whether
    WS is trustworthy for snipes. `shared/ingest/pumpfun/examples/feed_parity.rs`
    is the harness — it already measures decode, slot continuity and switch holes
    for two feeds and takes a third without changes.

## Open

- A clean byte-rate measurement with the real pool set, before the credit draw
  is treated as known.
- Whether `blockSubscribe` is plan-gated on the same key (needed only for the
  block-meta push hook).
- RPC gap-backfill on reconnect: opt-in, costed, deferred.
- Whether a permanent dual-feed standby is wanted later. The dedupe ring makes
  it nearly free to build and roughly doubles the credit draw to run.
