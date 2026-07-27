# Snipe latency — remaining work

Scope: a rule with **no entry metric params** (fingerprint-only / always-true entry),
i.e. pure sniper-on-`TokenCreated`. Goal is minimising **create-lands → our-buy-lands**.
Constraint from the operator: **no fee/tip raising, no extra sender regions, no second
event feed** — code and architecture only.

Shipped on `strategy-redesign` and folded into the permanent docs — nothing below
re-covers them:

| Item | Commit | Reference |
| --- | --- | --- |
| **B** entry-path nonce fail-fast (`TxAnchor`) · **C** front-loaded rebroadcast · **F** warm sender socket | `23b0a97a` | [arch/trade-execution.md](../arch/trade-execution.md) |
| **H** live runtime sized to the box (`WORKER_THREADS`) | `dbb65356` | — |
| **D** classify off account keys + `TxRelevance::Create` | `19ea7d5c` | [arch/ingest.md](../arch/ingest.md) |

---

## 1. Blocked — needs data only the server has

### A — nonce off the snipe buy path

**Gate:** does the live box actually contend for nonce slots?

```powershell
ssh -i $HOME/.ssh/aws-ec2-key.pem ubuntu@35.158.128.131 `
  "docker logs hunter-live 2>&1 | grep -cE 'Nonce contention|Nonce pool exhausted'"
```

- **`0` ⇒ do NOT do A.** It is worth ~0.3 ms (one mutex + a hash copy) and costs a
  tx-mode fork. **B already capped the downside** — the worst case is ~40 ms, not 4 s.
- **`> 0` ⇒ A is worth up to seconds.** The change: drop the durable nonce for the
  *snipe buy only* and ride the pushed recent blockhash (`BlockhashCache`, ~400 ms
  fresh at zero RPC, already read by `build_recent_tx`). B's `TxAnchor::Entry` is the
  seam — A becomes "Entry never takes a slot" rather than "Entry takes one briefly".
  Gains: no acquisition wait, unbounded concurrency, ~70 B smaller tx. The write-ahead
  signature guarantee is unchanged (fixed at signing in both modes).

  Why the nonce buys this path nothing: its purpose is surviving long validity gaps,
  but `REBROADCAST_WINDOW` is 5 s and a blockhash lasts ~60 s — 12× headroom. Sells
  keep the nonce (long holds must not expire).

  **Rejected alternative:** growing the pool to 24–32 accounts. Keeps the coupling and
  the tail risk, costs rent.

### G — right-size `curve_buy_cu` (150 000 today)

Not a latency item — a *landing-probability* one: a smaller requested limit is easier
for a leader to pack into a nearly-full block, at the same tip.

The data is structurally already ours: `raw_txs.payload` is the verbatim prost-encoded
`SubscribeUpdateTransaction`, so real consumption is at
`.transaction(1) → .meta(4) → .compute_units_consumed(16)` — **no probe run, no RPC**.
But in practice there is nothing to read:

- `settings.persist_raw` **defaults off** — nothing is being captured;
- `db-incremental-sync.ps1` skips `raw_txs` unless `-IncludeRawTxs`;
- local `raw_txs` is **empty**, and its retention is 7 days.

Trimming blind is worse than overshooting: a too-low limit is a hard tx failure.
Unblock = turn `persist_raw` on → let real buys land → sync with `-IncludeRawTxs` →
decode CU off the stored payloads → set actual + margin.

If **A** also lands, the nonce ix disappears — re-check the wire size against the
1232 B ceiling (`send.rs`) and record the headroom.

---

## 2. Deferred by design — gated on a measured tail

### The measurement that gates both (M.2, re-run when a snipe rule has run)

Ran 2026-07-27 and came back **inconclusive**: 39 real fills, but only **3** from a
rule (all `+37` slots, and not fingerprint-only entries) — everything else was a manual
buy. There is no snipe corpus yet, so the distribution says nothing about the pipeline.
Re-run once a real fingerprint-only rule has traded (bot wallet
`xxXgBgHE2S16gfe2CmcQ1cs2UwsFUqzMJaioovdZXxx`, `wallet_id` 78454):

```sql
SELECT p.rule_id, tr.slot - tk.creation_slot AS slot_delta
FROM strategy_positions p
JOIN tokens tk ON tk.mint_address = p.mint_address
JOIN LATERAL (
  SELECT slot FROM trades
   WHERE mint_address = p.mint_address AND trade_type = 'buy' AND wallet_id = 78454
     AND block_time BETWEEN p.entry_time - interval '5 min' AND p.entry_time + interval '5 min'
   ORDER BY abs(extract(epoch from (block_time - p.entry_time))) LIMIT 1
) tr ON TRUE
WHERE p.mode = 'real' AND p.rule_id IS NOT NULL AND p.entry_time IS NOT NULL;
```

- **p50 of 1–2 ⇒ we are at the physical floor. Stop — everything below is noise.**
- **fat / bimodal tail ⇒ E and L0 are live.**

### E — the rest of the create fast lane

D shipped the prerequisite (`TxRelevance::Create`). What remains, without violating the
one-decision-kernel rule:

- separate channel + task per relevance out of the transport, so AMM swap volume can
  never delay a create decode (`session.rs` is one decode task for everything today);
- a dedicated `create_rx` arm in the decision loop, `biased` above the general ping arm
  (`decision_loop.rs`). `reduce` stays the sole decider — only *arrival order* changes,
  so SSOT and determinism are untouched;
- raise `STRATEGY_QUEUE_CAP` (**512**, `consumer.rs`) or at least alarm on
  `shed.strategy_pings` — a shed create ping is a snipe that never happened, and today
  it is silently counted.

Bounded tail latency under burst; nothing on a quiet median.

### L0 — full stamped breakdown (diagnostic)

Carry one timestamp end to end: stamp `received_at` at the transport (already exists),
thread it through `IngestEvent::TokenCreated` → `StrategyPing` → `Event::TokenCreated`,
and log one structured line per snipe: `block_time → received_at → decoded → pinged →
reduced → nonce_acquired → signed → sender_ack`, alongside the landed slot from the fill
feed. This says *which* stage owns the tail; M.2 says *whether* there is one, for free.

---

## 3. The physical floor (be honest about this)

Reacting to a PROCESSED create notification means the earliest we can possibly land is
**create_slot + 1**, realistically **+2…+4**. Anyone landing *in* the create slot is
either in the creator's own bundle or predicting the mint keypair. No local optimisation
crosses that line — the whole game is compressing +4 down to +1. If the requirement is
landing *in* the create slot, that is a different product (mint-key prediction or bundle
co-ordination), not a latency fix.

## 4. Explicitly not doing

- **Pre-signing the buy tx** — impossible. A curve buy names `mint`, `bonding_curve`,
  `associated_bonding_curve`, `creator_vault`, `bonding_curve_v2`, all derived from a
  mint that does not exist until the create lands. The residual per-mint work (5 PDA
  derivations + one signature + bincode + base64) is sub-millisecond and is not where
  the time goes.
- **Growing the nonce pool** — keeps the coupling and the tail; A supersedes it.
- **Parallelising the decode task across a thread pool** — decode is microseconds; it
  buys reordering risk for nothing.
- **Second independent create feed / more sender regions / higher tip** — ruled out by
  the operator constraint. A second feed is a real tail win but the most expensive item,
  and it only makes sense after E proves the tail is *not* ours.
- **Box placement / `probe pin-senders` re-run** — infrastructure, not code. Still likely
  worth more than everything above combined if the EC2 box is not in `eu-central-1`
  (a US↔EU round trip is ~90–180 ms, i.e. several slots), but out of scope by the same
  constraint.
