# Snipe latency — where the time actually goes, and what to change

Scope: a rule with **no entry metric params** (fingerprint-only / always-true entry),
i.e. pure sniper-on-`TokenCreated`. Goal is minimising **create-lands → our-buy-lands**.
Constraint from the operator: **no fee/tip raising, no extra sender regions, no second
event feed** — code and architecture only.

Status: analysis + ranked proposal. Nothing implemented.

**Ranking principle:** only three things in this file can cost a *slot*. Everything else
costs microseconds. The table in §2 is ordered by worth ÷ effort on that basis, not by
where the code sits in the pipeline.

---

## 1. The actual path today

| # | Stage | Code | Cost class |
| --- | --- | --- | --- |
| 1 | create tx executes on a leader | — | — |
| 2 | Helius LaserStream (`fra`, **PROCESSED**) → EC2 | `ingest/core/config.rs:82` | network, ~unknown (unmeasured) |
| 3 | transport task: `classify` → mpsc(4096) | `ingest/core/transport/mod.rs:495` | **log-string scan per tx** + queue |
| 4 | **single** decode task: `venue.decode` for *every* pump.fun tx | `ingest/core/session.rs:114` | **serial head-of-line** |
| 5 | mpsc(4096) → **single** consumer task | `live/ingest/consumer.rs:109` | **serial** |
| 6 | `token_cache.insert` + `ping_strategy` (try_send, cap **512**) | `consumer.rs:223-224` | µs |
| 7 | **single** decision loop: `producer.on_ping` → `reduce` | `decision_loop.rs:239,255` | **serial**, ping lane shared with trades |
| 8 | Pass-1 sink (registry+SSE, PG spawned) → Pass-2 `spawn(run_entry)` | `decision_loop.rs:296-315` | µs (PG already off-path) |
| 9 | `run_entry` guards → `buy_token_snipe_write_ahead` | `exec_real.rs:184` | µs |
| 10 | **`acquire_nonce()`** | `executor/core/nonce.rs:46-106` | **0 … 4000 ms** ⚠ |
| 11 | derive 5 PDAs, pop template, build ixs, sign, base64 | `buy.rs:187-341` | **< 1 ms** |
| 12 | POST to 2 Helius Sender endpoints (`ams`, `fra`), first wins | `send.rs:420` | 1 RTT |
| 13 | rebroadcast every 500 ms for 5 s (**first re-post at 500 ms**) | `send.rs:382-400` | mistimed — see C |

**The signing work (stage 11) is already ~free.** Everything pre-computable is
pre-computed: nonce hash cached, tip floor cached, CU ixs prebuilt, token-account
template pool (16), global accounts cached, reserves from RAM, PG writes spawned.

### Why "pre-signing the tx" cannot work here

A curve buy names `mint`, `bonding_curve`, `associated_bonding_curve`,
`creator_vault`, `bonding_curve_v2` — all derived from a mint that **does not exist**
until the create lands. There is no tx to pre-sign. The residual per-mint work
(5 PDA derivations + one ed25519 signature + bincode + base64) is sub-millisecond
and is *not* where the time goes.

### The physical floor (be honest about this)

Reacting to a PROCESSED create notification means the earliest we can possibly land
is **create_slot + 1**, realistically **+2…+4**. Anyone landing *in* the create slot
is either in the creator's own bundle or predicting the mint keypair. No amount of
local optimisation crosses that line — the whole game is compressing +4 down to +1.

---

## 2. Ranked levers

| # | Lever | Worth | Effort | Verdict |
| --- | --- | --- | --- | --- |
| **M** | Measure with the data you already have | gates everything | **none** | Do first |
| **A** | Nonce off the buy path | **0–4000 ms**, or exactly 0 | S | Do it *if* M says contention |
| **B** | Fail-fast `acquire_nonce` on the entry path | caps the tail at ~40 ms | XS | Do it regardless |
| **C** | Front-load the rebroadcast schedule | ~1–2 slots on a missed send | XS | Do it |
| **D** | `classify` off account keys + create detection | modest median, real tail; **enables E** | M | Best structural item |
| **E** | Create fast lane (priority channel end-to-end) | tail only, under burst | L | Only after D + evidence |
| **F** | HTTP/2 keep-alive on the sender client | 60–200 ms, sparse rules only | XS | 2 lines |
| **G** | Right-size `curve_buy_cu` | landing probability, not latency | S | Cheap, measure first |
| **H** | `worker_threads = 4` on a 2-vCPU box | scheduling tail | XS | Free experiment |
| **L0** | Full stamped timing breakdown | diagnostic only | M | Only if M shows a tail to explain |

---

### M — measure before writing code (zero effort, do first)

The original version of this plan made a full `received_at` → `sender_ack` stamping pass
(L0 below) the prerequisite for everything. It is not. Two free checks decide most of the
list:

1. **`grep` the server logs for `Nonce contention` / `Nonce pool exhausted`**
   (`nonce.rs:76`, `nonce.rs:112`). Present ⇒ **A** is worth seconds. Absent ⇒ **A** is
   worth ~0.3 ms (one mutex + a hash copy) and must **not** be done.
2. **Query the slot delta already in Postgres.** Every position stores its fill signature
   and `trades` carries `slot`, so `landed_slot − creation_slot` per snipe is a plain SQL
   query — no instrumentation at all. That distribution *is* the number that matters:
   - p50 of 1–2 ⇒ we are at the physical floor; everything below is noise, stop here.
   - fat / bimodal tail ⇒ queueing or nonce; **A**, **B**, **D**, **E** are live and
     **L0** is now justified to say which.

---

### A — nonce off the buy path (highest suspected impact, small change)

`build_trade_tx` (`executor/core/send.rs:118-131`) branches on a **global**
`config.durable_nonce`, so buys and sells share one pool and the buy blocks inside
`acquire_nonce` for up to `max_wait_iters × wait_sleep_ms` = **200 × 20 ms = 4 s**
(`executor/core/config.rs:132-133`).

Slot accounting:
- pool size = **5** (`NONCE_ACCOUNTS`), shared by buys **and** sells;
- a slot is held `in_use` from send until re-armed. The push feed re-arms at feed speed
  **only if the tx lands** (`nonce.rs:244`). A snipe that *doesn't* land — the common
  case — falls back to the poll: `refresh_first_delay_ms = 2000` + up to
  4 × 150 ms ≈ **2.6 s hold**.

⇒ sustained ceiling ≈ **~2 tx/s**. An unfiltered snipe rule arms on *every* pump.fun
create; bursts are far above that.

**Drop the durable nonce for the snipe buy; use the pushed recent blockhash.** The
original reason for the nonce was avoiding a `getLatestBlockhash` RPC on the hot path.
That reason is gone — `blocks_meta` push already keeps `BlockhashCache` ~400 ms fresh at
zero RPC (`main.rs:1237`), and `build_recent_tx` (`send.rs:141`) already reads it. Gains:

- zero acquisition wait, unbounded concurrency, no slot bookkeeping;
- a smaller tx (drops `advance_nonce_account` + 2 accounts ≈ 70 B) — real headroom on a
  create-with-seed + initialize + buy + tip message;
- the write-ahead signature guarantee is **unchanged** (the signature is fixed at signing
  in both modes — `send.rs:117`).

Two things the earlier draft of this plan got wrong or left out:

- **Durable nonce buys this path nothing.** Its purpose is surviving long validity gaps,
  but `REBROADCAST_WINDOW` is 5 s and `maxRetries: 0`. A ~60 s blockhash covers that with
  12× headroom. The comment at `send.rs:370-373` ("a recent-blockhash tx would expire
  mid-window") is false for a 5 s window.
- **The flag is global, so the change is a per-call mode, not a flip.** Sells keep the
  durable nonce (long holds must not expire). That means threading a tx-mode argument
  through `build_trade_tx`, and loosening the `skip_confirm && self.config.durable_nonce`
  gate at `buy.rs:363` so the blockhash snipe still rebroadcasts.

**Rejected alternative:** growing the pool to 24–32 accounts. It keeps the coupling and
the 4 s tail risk, and costs rent. A is strictly better.

### B — fail-fast on the entry path (do this whatever A decides)

Make `acquire_nonce` bail after ~2 iterations when called from the entry path and fall
back to a recent blockhash, rather than spinning 200. Five lines, and it converts a 4 s
wait into a bail. **A snipe that is 4 s late is worth less than no snipe.**

### C — front-load the rebroadcast schedule (free, and mistimed today)

`send.rs:385` sleeps *before* the deadline check, so the first re-post lands at **500 ms**
— roughly 1¼ slots after the snipe's value has decayed. Replace the flat
`REBROADCAST_INTERVAL` with a schedule like `[60, 120, 250, 400, 700, 1000…]` ms so three
retries fall inside the window that still matters, then taper to the 5 s deadline.

Same bytes, same tip, ~15 lines. The bank dedups on signature, so re-posts remain free
(`send.rs:356-372`).

### D — classify off account keys, and detect `create` in the same pass

**Not in the original plan, and it is the best structural item.**

`classify` (`ingest/pumpfun/src/venue.rs:69-80`) substring-scans **every log line of every
transaction** for a 44-char base58 program id. But `subscription_accounts`
(`venue.rs:62-67`) already sets the gRPC `account_include` to the pump program + tracked
pool PDAs — the filter has *already guaranteed* what the scan re-derives. It runs on the
single transport task that gates every create's arrival, ahead of everything else.

The fix and the create fast lane are **the same edit**:

- classify off the tx message instead of the logs — 32-byte pubkey compares against
  `account_keys` to separate `Curve` from `Amm` (a pool-PDA match can deliver an AMM tx,
  so the distinction still has to be made);
- in the same pass, check the 8-byte discriminator on the top-level instruction data for
  `create` / `create_v2` — the discriminators already exist in `classify_pump_ix`
  (`ingest/pumpfun/src/decode/instructions.rs:41`);
- widen `TxRelevance` to `Create | Curve | Amm`. That tag is the prerequisite for E.

**Honest magnitude:** in steady state this is single-digit microseconds per tx — a few
percent of a core, not a median win. Its value is that (a) it is the only place a create
queues behind unrelated volume on a 2-vCPU box, and (b) it is the cheap half of E.

**Guard:** a test asserting old-vs-new `classify` agree over a corpus of recorded
`SubscribeUpdateTransaction`s, so the semantics cannot drift.

### E — the rest of the create fast lane

Given D's `Create` tag, route it on a separate high-priority channel all the way through
without violating the one-decision-kernel rule:

- separate channel + task per relevance out of the transport, so AMM swap volume can never
  delay a create decode (`session.rs:114` is one task for everything today);
- a dedicated `create_rx` arm in the decision loop, `biased` above the general ping arm
  (`decision_loop.rs:233-249`). `reduce` stays the sole decider — only *arrival order*
  changes, so SSOT and determinism are untouched;
- raise `STRATEGY_QUEUE_CAP` (currently **512**, `consumer.rs:40`) or at least alarm on
  `shed.strategy_pings` — a shed create ping is a snipe that never happened, and today it
  is silently counted.

Expected win: bounded tail latency under burst, nothing on a quiet median. Do it only
after D lands and M shows a tail.

### F — HTTP/2 keep-alive (2 lines, not a custom pinger)

`pool_idle_timeout` is 90 s (`executor/core/engine.rs:156-158`) with a one-shot warmup at
init, so between sparse snipes the connection goes cold and the hot-path POST pays a fresh
TCP+TLS handshake (+1–2 RTT to Europe).

Do **not** write a keep-alive task — `reqwest` does this at protocol level. Add
`.http2_keep_alive_interval(~20s)` + `.http2_keep_alive_while_idle(true)` to the builder.

Worth ≈ 0 for an unfiltered sniper (connections never idle); worth 60–200 ms for a
selective fingerprint rule that fires a few times an hour. Know which regime the live rule
is in before crediting this.

### G — right-size the transaction

- `curve_buy_cu` is **150 000** (`executor/core/config.rs:36`). Real consumption is in
  `meta.compute_units_consumed`, a field our own ingest proto already carries
  (`ingest/core/generated/solana.storage.confirmed_block.rs:113`) — so it can be read off
  our own landed buys with **no probe run and no RPC**. Trimming to actual + margin is a
  landing-probability and cost win at the same tip, not a fee change: a smaller requested
  limit is easier for a leader to pack into a nearly-full block.
- With A the nonce ix disappears; re-check the wire size against the 1232 B ceiling
  (`send.rs:57`) and record the resulting headroom.

### H — runtime sizing (free experiment)

`main.rs:1000` runs `worker_threads = 4` on a 2-vCPU box. Oversubscription on IO-bound
work mostly adds scheduling latency for the runnable task. One line, trivially reversible,
measurable against the M slot-delta query.

### L0 — full stamped breakdown (diagnostic, only if M shows a tail)

Carry one timestamp end to end: stamp `received_at` at the transport (already exists,
`transport/mod.rs:496`), thread it through `IngestEvent::TokenCreated` → `StrategyPing` →
`Event::TokenCreated`, and log one structured line per snipe: `block_time → received_at →
decoded → pinged → reduced → nonce_acquired → signed → sender_ack`, alongside the landed
slot from the fill feed.

This is what tells you *which* stage owns the tail. It is not needed to know *whether*
there is one — M answers that for free.

---

## 3. Suggested order

1. **M** — grep the nonce warnings; run the slot-delta query. Stop here if p50 is 1–2.
2. **B + C + F** in one small commit — three XS, independent, no-regret changes.
3. **A** — decided by what the grep said in step 1.
4. **G** (measure off `compute_units_consumed`), **H** (one line).
5. **D** as its own change, with the classify parity guard test.
6. **L0**, then **E**, only if the tail still justifies them.

A, B, C, F, G, H are individually small and contained. D touches the ingest crate's venue
seam; E is the only one that touches the ingest/engine *structure*, and it is deliberately
arranged so `reduce` stays the single decision kernel.

## 4. Explicitly not doing

- **Pre-signing the buy tx** — impossible (§1); the mint does not exist yet.
- **Growing the nonce pool** — keeps the coupling and the tail; A supersedes it.
- **Parallelising the decode task across a thread pool** — decode is microseconds; it buys
  reordering risk for nothing.
- **Second independent create feed / more sender regions / higher tip** — ruled out by the
  operator constraint. A second feed is a real tail win but the most expensive item, and
  it only makes sense after D/E prove the tail is *not* ours.
- **Box placement / `probe pin-senders` re-run** — infrastructure, not code. Still likely
  worth more than everything above combined if the EC2 box is not in `eu-central-1`
  (a US↔EU round trip is ~90–180 ms, i.e. several slots), but it is out of this plan's
  scope by the same constraint.

## 5. What this cannot fix

Reactive sniping lands at `create_slot + 1` at the very best. If the requirement is
landing *in* the create slot, that is a different product (mint-key prediction or bundle
co-ordination), not a latency fix.
