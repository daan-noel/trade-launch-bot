# Snipe latency — remaining work

Scope: a rule with **no entry metric params** (fingerprint-only / always-true entry),
i.e. pure sniper-on-`TokenCreated`. Goal is minimising **create-lands → our-buy-lands**.
Constraint from the operator: **no fee/tip raising, no extra sender regions, no second
event feed** — code and architecture only.

Shipped (folded into arch docs — do not re-cover here):

| Item | Reference |
| --- | --- |
| **B** entry-path nonce fail-fast (`TxAnchor`) · **C** front-loaded rebroadcast · **F** warm sender socket | [arch/trade-execution.md](../arch/trade-execution.md) |
| **H** live runtime sized to the box (`WORKER_THREADS`) | — |
| **D** classify off account keys + `TxRelevance::Create` | [arch/ingest.md](../arch/ingest.md) |
| **E** create fast lane (transport→decode split + dedicated create strategy ping + biased `create_rx`) | [arch/ingest.md](../arch/ingest.md), [arch/strategies.md](../arch/strategies.md) |
| **G** `curve_buy_cu` 150k → **110k** (measured off 40 live buys: p99=86_389) | `executor-core` `ComputeBudgetCfg` |
| **L0** `snipe_latency` structured log (`create_to_ping_ms` / `ping_to_decide_ms` / `decide_to_ack_ms`) | `exec_real::run_entry` |

---

## Server measurement (2026-08-06)

Re-checked on the live box (code + logs + Postgres — not the old plan alone).

| Check | Result |
| --- | --- |
| `Nonce contention` / `Nonce pool exhausted` (current container logs) | **0** |
| `Entry buy: no free nonce` fallback | **0** |
| `strategy ping queue full` sheds | **0** |
| `Buy pool miss` | **0** |
| `ingest.persist_raw` | **true** (`raw_txs` ≈ 12.8M rows) |
| Real rule positions (7d) slot_delta | n=192, **p50=42**, p90=126, min=1, max=640 |
| Empty-entry (pure snipe) real rules | **none** — every active real rule has metric entry gates |
| Active real rules | all require `m_snapshot.time > 10` (and usually liquidity / flow) |

**Read of the slot_delta numbers:** they are **not** pipeline latency for today's book.
Active rules refuse entry until token age ≥ 10 s (~25 slots), so a p50 of 22–42 is the
strategy waiting on purpose. The inactive `3 IX - 20-30` rule (entry `time < 45` +
`liquidity > 25`) still hit **+1 slot**  on 8 fills and ≤2 on 17/61 — proof the
create→buy path *can* land at the physical floor when conditions allow.

**Pipeline metric to watch after deploy:** `decide_to_ack_ms` on the `snipe_latency`
log line (post-`reduce` → sender ACK). `ping_to_decide_ms` includes intentional
metric waits and will look large on the current ladder.

### A — nonce off the snipe buy path — **do NOT**

Gate was contention logs → **0**. Worth ~0.3 ms median; B already caps the worst
case at ~40 ms. Revisit only if `Nonce contention` / `Nonce pool exhausted` appear.

CU decode helper (local): `cargo run -p ingest-core --example decode_cu -- <dir-of-*.b64>`.

---

## Still open

### Pure-snipe corpus (M.2 for fingerprint-only)

No empty-entry real rule is live, so there is still no fingerprint-only slot-delta
distribution. When one trades, re-run:

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

- **p50 of 1–2 on a fingerprint-only rule ⇒ physical floor. Stop.**
- **fat tail on `decide_to_ack_ms` ⇒ dig L0 stage that owns it.**

---

## The physical floor (be honest about this)

Reacting to a PROCESSED create notification means the earliest we can possibly land is
**create_slot + 1**, realistically **+2…+4**. Anyone landing *in* the create slot is
either in the creator's own bundle or predicting the mint keypair. No local optimisation
crosses that line — the whole game is compressing +4 down to +1. If the requirement is
landing *in* the create slot, that is a different product (mint-key prediction or bundle
co-ordination), not a latency fix.

## Explicitly not doing

- **Pre-signing the buy tx** — impossible until the mint exists.
- **Growing the nonce pool** — no contention; A supersedes it if that changes.
- **Second feed / more regions / higher tip** — operator constraint.
- **Treating metric-gated slot_delta as pipeline lag** — it isn't; use `decide_to_ack_ms`.
