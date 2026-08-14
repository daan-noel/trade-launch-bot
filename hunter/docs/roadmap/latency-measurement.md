# Latency measurement — per-stage breakdown, both legs

Scope: **where the wall-clock goes** on a flow-triggered rule (scalper), entry *and*
exit. Distinct from [snipe-latency-plan.md](snipe-latency-plan.md), which covers
create-lane *optimisation* for a fingerprint-only sniper.

Instrumentation reference (what each field means, how to read it):
[arch/trade-execution.md](../arch/trade-execution.md) ·
[arch/ingest.md](../arch/ingest.md).

## The chain

`t_block` = the slot the trigger tx lands in · `t_recv` = frame at our socket ·
`t_ping` = strategy ping enqueued · `t_decide` = post-`reduce` dispatch ·
`t_ack` = sender ACK · `t_land` = our own tx's slot.

| # | Segment | Covered by |
| --- | --- | --- |
| S1 | leader packs the trigger tx | physical floor, not ours |
| S2 | chain → frame at our socket | `feed_lag` (whole-second resolution) |
| S3 | decode + classify + pipeline + consumer → ping | `recv_to_ping_ms` |
| S4 | ping queue wait + `reduce` + dispatch | `ping_to_decide_ms` |
| S5 | reserve read + nonce + build + sign + fan-out RTT | `decide_to_ack_ms`, split into `prep_ms` / `anchor_ms` / `send_ms` |
| S6 | ACK → own fill observed | `entry_landed.ack_to_fill_ms` |
| S7 | exit decision → sell ACK | `exit_latency.decide_to_ack_ms` |
| S8 | sell ACK → bag cleared | `exit_landed.ack_to_clear_ms` |

S3/S4 need a `PingStamp`. The create lane always records one; the trade lane records
one only under `LATENCY_TRACE`, so **a flow rule logs `decide_to_ack_ms` alone until
that flag is set**.

## Baseline (2026-08-14, EC2, 7 d of `trades` + 14 d of `strategy_positions`)

Measured before the instrumentation above existed, from Postgres alone. Both
numbers are ceilings on our own cost, not stage timings — keep them as the bar the
log lines have to beat.

| Measure | n | p50 | p90 | p99 |
| --- | --- | --- | --- | --- |
| decision (`created_at`) → own fill observed (`entry_time`) | 395 | 97 ms | 228 ms | 513 ms |
| our buy slot − previous trade slot on the mint | 170 | **1** | 11 | 13 |
| same, quiet mints only (ambient inter-trade gap ≥ 3 slots) | 44 | **1** | 8 | — |

55 % of buys (94/170) land in the very next slot after the print they reacted to,
and on quiet mints — where trade density cannot manufacture a small gap — it is
still 48 % (21/44). **The path does reach the physical floor.** The tail is the
open question: 20/44 quiet-mint fills land ≥3 slots (1.2 s+) behind, and Postgres
cannot say whether that is strategy wait or pipeline, because `created_at` is
stamped after `reduce` has already decided.

Two traps in re-deriving these:

- **`trades.block_time` is the ingest clock, not chain time** — a transaction frame
  carries no block time, so the decoder stamps `received_at`. Any "feed lag" query
  against that column returns zero by construction. Use `feed_lag`.
- **"Slots behind the previous trade" is confounded by trade density.** On a busy
  mint some wallet trades every slot, so the gap is ~1 regardless of our speed.
  Only the ambient-gap-filtered subset measures us.

## Harvesting

`docker logs` does not survive a container **recreate** — a redeploy starts the
history empty, which is why the baseline above is PG-only. Pull the lines to a file
early in a measuring window.

```bash
ssh -i ~/.ssh/aws-ec2-key.pem ubuntu@35.158.128.131 \
  'docker logs hunter-live-api --since 168h 2>&1 | grep -E "snipe_latency|exit_latency|feed_lag"' > lat.log

# percentiles for any field
grep -o 'decide_to_ack_ms=[0-9-]*' lat.log | cut -d= -f2 | sort -n | \
  awk '{v[NR]=$1} END{printf "n=%d p50=%d p90=%d p99=%d max=%d\n",NR,v[int(NR*.5)],v[int(NR*.9)],v[int(NR*.99)],v[NR]}'
```

Split `snipe_latency` by `lane=create` / `lane=trade` before taking percentiles —
a snipe and a flow reaction have different floors and pooling them describes
neither.

Sender RTT, separately (`probe fanout` costs base fee + one tip and returns the
lamports; **run it on the box**, since the number is the EC2↔Helius path):

```bash
docker exec hunter-live-api hunter-live probe fanout 1 --tip --confirm
```

## Still open

### S6 is two segments, not one

`ack_to_fill_ms` spans leader inclusion **and** the feed's return trip. Splitting
them needs a chain clock finer than `feed_lag`'s whole seconds; pair the two before
concluding which half owns a wide reading. The `sig` field on `snipe_latency` joins
to `trades.tx_signature` for the exact landed slot when that matters.

### Pure-snipe corpus

Unchanged from [snipe-latency-plan.md](snipe-latency-plan.md): no empty-entry real
rule trades, so there is still no fingerprint-only slot-delta distribution.

## Interpreting a reading

- **`feed_lag` mean ≥2 s** ⇒ transport problem; no code path is the lever.
- **`recv_to_ping_ms` large** ⇒ decode/queue cost; check the shed counters.
- **`ping_to_decide_ms` large on a metric-gated rule** ⇒ the strategy waiting on
  purpose, not lag. Only meaningful on a rule whose entry can fire immediately.
- **`decide_to_ack_ms` large with `probe fanout` fast** ⇒ nonce contention or build
  cost, not the wire.
- **Everything small but the slot delta wide** ⇒ tip / leader-schedule territory.
