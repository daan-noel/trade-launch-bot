# Bot latency — how to measure it, and what the numbers are

**Verdict: the code path is 10 ms; the strategy wait is 6.35 s.** Entry timing is a
rule-condition question, not an engineering one. Re-check with the runbook below
before concluding anything has regressed.

Field reference (what each log field means):
[arch/trade-execution.md](../arch/trade-execution.md) · [arch/ingest.md](../arch/ingest.md).

---

## Re-check runbook

### 0. Prerequisites

`LATENCY_TRACE=1` in `hunter/.env` **on the box** is what makes the trade lane
report `recv_to_ping_ms` / `ping_to_decide_ms`. Without it a flow-triggered rule
logs `decide_to_ack_ms` alone. It costs a mint clone + a map insert per trade ping,
so turn it on for a measuring window and back off afterwards.

```bash
ssh -i ~/.ssh/aws-ec2-key.pem ubuntu@35.158.128.131
cd ~/trade-launch-bot
grep LATENCY_TRACE hunter/.env                       # expect LATENCY_TRACE=1
docker exec hunter-live-api printenv LATENCY_TRACE   # 1 = the RUNNING process has it
```

The container must have been **recreated** (not just restarted) after the flag
changed, or the process is still running without it.

> **`docker logs` does not survive a container recreate.** A redeploy starts the log
> history empty. Pull the lines to a file before redeploying, or accept starting
> over.

### 1. Pull everything to a file

```bash
ssh -i ~/.ssh/aws-ec2-key.pem ubuntu@35.158.128.131 \
  'docker logs hunter-live-api 2>&1 | sed -e "s/\x1b\[[0-9;]*m//g" \
   | grep -E "snipe_latency|entry_landed|exit_latency|exit_landed|feed_lag"' > lat.log
wc -l lat.log
```

### 2. Percentiles for any field

```bash
f=decide_to_ack_ms   # or recv_to_ping_ms, ping_to_decide_ms, send_ms, ack_to_fill_ms, ...
grep -o "$f=[0-9-]*" lat.log | cut -d= -f2 | sort -n | \
  awk '{v[NR]=$1} END{printf "n=%d p50=%d p90=%d p99=%d max=%d\n",NR,v[int(NR*.5)],v[int(NR*.9)],v[int(NR*.99)],v[NR]}'
```

**Split by lane first** — a create snipe and a flow reaction have different floors,
and pooling them describes neither:

```bash
grep 'lane="trade"'  lat.log > lat.trade.log
grep 'lane="create"' lat.log > lat.create.log
```

### 3. Strategy wait — the term that dominates

Not in the logs; it comes from the arm ledger. This is the number worth watching.

```bash
ssh -i ~/.ssh/aws-ec2-key.pem ubuntu@35.158.128.131 \
  'docker exec -i hunter-postgres psql -U postgres -d hunter_bot' <<'SQL'
SELECT count(*) AS n,
  percentile_disc(0.5) WITHIN GROUP (ORDER BY ms) AS p50_ms,
  percentile_disc(0.9) WITHIN GROUP (ORDER BY ms) AS p90_ms,
  max(ms) AS max_ms
FROM (
  SELECT (extract(epoch FROM (p.created_at - a.armed_at))*1000)::bigint AS ms
  FROM strategy_arms a JOIN strategy_positions p ON p.id = a.position_id
  WHERE p.mode='real' AND a.armed_at > now() - interval '14 days'
    AND p.created_at >= a.armed_at
) s;
SQL
```

### 4. Sender RTT (optional — costs base fee + one tip)

Run it **on the box**; the number is the EC2↔Helius path, not the workstation's.
Lamports return to the same wallet.

```bash
docker exec hunter-live-api hunter-live probe fanout 1 --tip --confirm
```

---

## What healthy looks like (2026-08-14 baseline)

Compare a re-check against these. A stage more than ~3x its baseline is the one to
dig into.

| Stage | Field | Baseline | Source |
| --- | --- | --- | --- |
| rule armed → entry condition true | `armed_at` → `created_at` | **6 354 ms** p50 | n=28, 14 d |
| frame at socket → ping | `recv_to_ping_ms` | 0–1 ms | n=3 |
| ping → decision (`reduce`) | `ping_to_decide_ms` | **0 ms** | n=3 |
| decision → sender ACK | `decide_to_ack_ms` | 8–10 ms | n=3 |
| ⤷ prep / anchor / send | `prep_ms`/`anchor_ms`/`send_ms` | 0 / 0 / 8–10 ms | n=3 |
| ACK → own fill on feed | `ack_to_fill_ms` | 37–82 ms | n=3 |
| exit decision → sell ACK | `exit_latency.decide_to_ack_ms` | 7–10 ms | n=3 |
| exit ACK → bag cleared | `ack_to_clear_ms` | 296–717 ms | n=3 |
| sender ACK (Frankfurt) | `probe fanout` | 1 ms | live |
| sender ACK (Amsterdam) | `probe fanout` | 19 ms | live |
| send → confirmed | `probe fanout --confirm` | 689 ms | live |

**The pipeline is 10 ms against a 6.35 s strategy wait — a factor of ~635.** No code
path is a lever on entry timing.

Strategy wait is **bimodal**, never in between:

| armed → decided | n |
| --- | --- |
| <0.1 s | 11 |
| 5–30 s | 12 |
| >30 s | 5 |

Half of entries fire the instant the rule arms; the other half wait 5–48 s for a
metric gate (`m_snapshot.time > 10` and similar). That split is a strategy choice.

Historical cross-check (Postgres, 395 real positions): decision → own fill observed
p50 97 ms, against the instrumented 47–92 ms (`decide_to_ack` + `ack_to_fill`). The
two agree. Slot-level: our buy lands **+1 slot** after the prior print for 55 % of
buys (170 buys; 48 % on quiet mints alone), which is the physical floor.

## Reading a result

- **`ping_to_decide_ms` > 0 while the entry condition was already satisfiable** ⇒ the
  first real evidence of a code-side problem: the engine is queueing. Check the shed
  counters. Baseline is 0.
- **`recv_to_ping_ms` large** ⇒ decode/pipeline cost.
- **`anchor_ms` large** ⇒ nonce contention. `TxAnchor::Entry` caps it near 40 ms by
  falling back to a blockhash, so a larger reading means the cap is being hit.
- **`send_ms` large but `probe fanout` fast** ⇒ the fan-out is degraded, not the wire.
- **Strategy wait large** ⇒ working as configured. Change entry conditions, not code.
- **Everything small but slot delta wide** ⇒ tip / leader-schedule territory.

## Three traps

1. **`feed_lag`'s absolute value is NOT transport lag.** It reads a stable ~0.82 s
   (~4 500 slots), but `ack_to_fill_ms` of 37–82 ms proves this bot sees its **own**
   transaction on the feed in under a tenth of a second — impossible if the stream
   ran 0.8 s behind. The gauge is dominated by Solana's `block_time` (a
   stake-weighted cluster clock trailing wall time) plus whole-second truncation.
   Use it as a **relative** health signal: a jump to 5 s is a real backlog.
   `ack_to_fill_ms` is the honest freshness measure.
2. **`trades.block_time` is the ingest clock, not chain time.** A transaction frame
   carries no block time, so the decoder stamps `received_at`. Any feed-lag query
   against that column returns zero by construction.
3. **"Slots behind the previous trade" is confounded by trade density.** On a busy
   mint some wallet trades every slot, so the gap is ~1 regardless of our speed.
   Only an ambient-gap-filtered subset measures us.

## Still open

### The exit detects ~10x slower than the entry

Both legs ACK under 10 ms, but the entry sees its fill in 37–82 ms while the exit
takes 296–717 ms to see the bag clear. The entry gets `observe_own_leg`'s early
wake; `confirm_sell` polls on a ≥250 ms rate limit. This delays neither buy nor
sell — only the *detection*, which holds the position's concurrency slot open
longer than needed.

### Sample size

The per-stage figures rest on 3 buys in a 30-minute window. They are tightly
clustered and agree with 395 historical positions, but a wide reading on a re-check
with a larger sample supersedes them.

### Pure-snipe corpus

No empty-entry real rule trades, so there is still no fingerprint-only slot-delta
distribution — see [snipe-latency-plan.md](snipe-latency-plan.md).
