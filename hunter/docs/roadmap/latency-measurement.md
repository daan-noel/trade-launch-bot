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

## Measured (2026-08-14, EC2, live instrumentation)

Per-stage, real buys on the **trade lane** (n=3, 30 min post-deploy):

| Stage | Field | Measured |
| --- | --- | --- |
| rule armed → entry condition true | `strategy_arms.armed_at` → `created_at` | **6 354 ms** (p50) |
| frame at socket → ping | `recv_to_ping_ms` | 0–1 ms |
| ping → decision (`reduce`) | `ping_to_decide_ms` | **0 ms** |
| decision → sender ACK | `decide_to_ack_ms` | 8–10 ms (all of it `send_ms`; `prep_ms`/`anchor_ms` = 0) |
| ACK → own fill on the feed | `ack_to_fill_ms` | 37–82 ms |
| exit decision → sell ACK | `exit_latency` | 7–10 ms |
| exit ACK → bag cleared | `ack_to_clear_ms` | 296–717 ms |

**The pipeline is 10 ms and the strategy wait is 6.35 s — a factor of ~635.** Nothing
in the code path is a lever on entry timing; the rules' entry conditions are.

Strategy wait is **bimodal**, never in between (n=28, 14 d, real):

| armed → decided | n |
| --- | --- |
| <0.1 s | 11 |
| 5–30 s | 12 |
| >30 s | 5 |

Half of entries fire the instant the rule arms; the other half wait 5–48 s for a
metric gate. That split is a strategy choice, not lag.

Cross-check: `decide_to_ack` (10 ms) + `ack_to_fill` (37–82 ms) = 47–92 ms, against
the historical PG proxy's p50 of 97 ms over 395 positions. The two agree, so the
older number was measuring what it claimed to.

### `feed_lag`'s absolute value is NOT transport lag

30 windows (~4 500 slots) read mean **0.82 s**, range 0.77–0.85 — suspiciously
stable. It is not lag: `ack_to_fill_ms` of 37–82 ms proves this bot sees its **own**
transaction on the feed in well under a tenth of a second, which is impossible if
the stream ran 0.8 s behind. The reading is dominated by the offset between
Solana's `block_time` (a stake-weighted cluster-clock estimate that trails wall
time) and the host clock, plus whole-second truncation.

Read the gauge as a **relative** health signal — a jump to 5 s is a real backlog —
and never as an absolute segment. `ack_to_fill_ms` is the honest freshness measure.

## Historical baseline (Postgres only, before the instrumentation existed)

| Measure | n | p50 | p90 | p99 |
| --- | --- | --- | --- | --- |
| decision (`created_at`) → own fill observed (`entry_time`) | 395 | 97 ms | 228 ms | 513 ms |
| our buy slot − previous trade slot on the mint | 170 | **1** | 11 | 13 |
| same, quiet mints only (ambient inter-trade gap ≥ 3 slots) | 44 | **1** | 8 | — |

55 % of buys land in the very next slot after the print they reacted to.

Two traps in re-deriving these:

- **`trades.block_time` is the ingest clock, not chain time** — a transaction frame
  carries no block time, so the decoder stamps `received_at`. Any "feed lag" query
  against that column returns zero by construction.
- **"Slots behind the previous trade" is confounded by trade density.** On a busy
  mint some wallet trades every slot, so the gap is ~1 regardless of our speed.

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

### The exit detects 10x slower than the entry

Both legs ACK in under 10 ms, but the entry sees its fill in 37–82 ms while the exit
takes 296–717 ms to see the bag clear. The entry gets `observe_own_leg`'s early wake;
`confirm_sell` polls on a ≥250 ms rate limit. This delays neither sell nor buy — only
the *detection*, which holds the position's concurrency slot open longer than needed.

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
