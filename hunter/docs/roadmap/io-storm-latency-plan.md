# IO-storm latency fix - plan

**Status: Phase 1 committed on `strategy-redesign` (the branch the server runs); Phase 2
(deploy) and Phase 3 (gates) open.** Live entry, confirm and exit latency are degraded,
and the cause is one failing background write. Measurements:
[latency-measurement.md](latency-measurement.md).

## Evidence (EC2, measured Sep 15)

| Stage | Healthy | Now |
| --- | --- | --- |
| decision -> own fill, p50 / p90 (PG, real) | 59-91 / 180-240 ms | 249 / 669 ms |
| trigger -> fill slot delta, p50 | 0 | 1 |
| `decide_to_ack_ms`, first entry after idle | ~10 ms | 388-654 ms |
| `entry_landed.ack_to_fill_ms` (own fill on feed: 19-185 ms) | 37-82 ms | 58-4141 ms |
| `ack_to_clear_ms` | 296-717 ms | 0-3192 ms |
| time exit when two real rules share a mint | 90 s | 102-152 s |

The regression starts Sep 11 15:00 UTC, the hour the arm rate rises from ~20/h to
200-2000/h.

## Root cause chain

1. `ArmRepo::end_arms` is `UPDATE strategy_arms ... FROM (VALUES ...)`. With two or more
   rows it is a hash join, so neither the planner nor the executor can exclude chunks: it
   seq-scans all 31 chunks, and the only qual pushed into the 23 compressed ones is
   `ended_at IS NULL`. TimescaleDB decompresses those rows, crosses
   `max_tuples_decompressed_per_dml_transaction` (100 000) and aborts. A one-row batch keeps
   exclusion and succeeds - which is why the bug stays silent at ~20 arms/h.
2. Each abort leaves ~100 000 dead tuples plus WAL. 830 aborts in 11 h (one per ~48 s):
   compressed chunks carry 350-550 k dead tuples and are autovacuumed endlessly;
   `strategy_arms` is 4.25 GB. The writer drops the failed batch, so ~99 % of arms armed since
   Sep 11 15:00 UTC read as open.
3. The t3.medium's EBS path saturates: 42.5 MB/s during a storm (its baseline is 347 Mbps ~
   43 MB/s), 12-14 ms per read or write, iowait 78 %, `/proc/pressure/io` full avg60 72 %.
4. Consequences on the trade path:
   - **Slow first entry.** hunter-live has 57 MB of 90 MB swapped out. The first buy after a
     long idle faults its cold pages back from the saturated disk: `prep_ms` (pure CPU) reads
     73-124 ms, all three stages stretch together, a re-entry seconds later is ~10 ms.
   - **Slow entry confirm.** `poll_feed_buy` runs `find_fill_by_signature` BEFORE its first
     wait. That SQL has no time bound, probes every `trades` chunk and measures 475 ms (49 page
     reads at ~10 ms). The own-leg wake that lands during it is lost, because the `Notified`
     future is created after the check (`notify_waiters` keeps no permit).
   - **Slow sell confirm.** Same SQL and same lost wake in `confirm_sell`, plus the 250 ms rate
     limit discarding a wake.
   - `mark_buy_submitted` and statement timeouts: commits wait on `WALSync`.
5. Independent bug: `run_exit` returns without emitting anything when the mint exit lock is
   held. The engine never re-decides an `ExitPending` arm, so the sibling sell waits for the
   60 s reaper tick.

Controlled confirmation: every rule is deactivated at 14:27:27 UTC on Sep 15. Arm ends stop,
and within 10 minutes iowait falls from 72 % to 0.6 %, CPU goes from ~2 % idle to 94 % idle,
kswapd scanning drops from ~4 500 pages/s to ~0, and memory pressure reads 0. **Re-activating
any rule restarts the storm until C1 ships or Phase 0 is set.**

## Server budget (t3.medium: 2 vCPU, 20 % CPU baseline on credits, 3.8 GB RAM, 347 Mbps EBS)

| Resource | Storm | Idle | This plan's cost |
| --- | --- | --- | --- |
| CPU user+sys (baseline 20 %) | 23 % today, 32 % Sep 14: draining credits | ~4-6 % | C1 +5-7 ms PG planning per flush, max 2 flushes/s (<= 0.7 % of one vCPU, warm backend, measured). C2 removes most confirm queries. |
| RAM | Postgres 1.7 GB, hunter 131 MB + 57 MB swapped, kswapd ~4 500 scans/s | pressure 0 | C3 map is pruned on release. C4 protects 256 MB and allocates nothing. |
| Disk | at the 43 MB/s instance baseline | ~1 MB/s | C2's bounded SELECT prunes 15 of 16 `trades` chunks under a generic plan: 0.045 ms vs 475 ms (measured). |

What the plan deliberately does NOT do on this box:
- No `VACUUM FULL`, manual decompression or recompression of `strategy_arms`. Its bloat ages
  out through the 30-day retention; autovacuum already clears the dead tuples once aborts stop.
- No change to `shared_buffers`, pool sizes or cache caps.
- No Rust build next to live trading (see Phase 2).

## Phase 0 - stop the bleeding (server, no restart, reversible)

```sql
ALTER SYSTEM SET timescaledb.max_tuples_decompressed_per_dml_transaction = 1000;
SELECT pg_reload_conf();   -- context `user`: applies to the live pool's sessions
```

Every failing batch then aborts after 1 000 decompressed rows instead of 100 000. Nothing on
the live path runs DML into a compressed chunk (ingest writes today's `block_time`;
compression and retention are jobs, not DML). No trade repair runs while it is set. Revert
after Phase 2: `ALTER SYSTEM RESET ...; SELECT pg_reload_conf();`.

The user checks in the AWS console: the t3 credit specification plus `CPUCreditBalance`, and
the EBS volume type plus `BurstBalance`. That is the headroom Phase 3 is judged against.

## Phase 1 - code (on `strategy-redesign`, one commit each) - done

**C1 `026cd794` - arm-end write never plans across chunks** (`arm_repo.rs::end_arms`, `arm_ledger.rs::flush`)
- The UPDATE carries the batch's own `armed_at` span and runs in a transaction opened by
  `SET LOCAL plan_cache_mode = force_custom_plan`, so Postgres plans it with the values and
  prunes to the chunks in the span. Plan-only EXPLAIN on the server: a custom plan is a
  one-chunk index scan; a generic plan (what a cached statement can switch to) scans all 31
  chunks. sqlx 0.6's `.persistent(false)` would also force a fresh plan but leaks a named
  server-side statement per call, so it is not used.
- Ends armed past the compression horizon are dropped with one aggregated `warn`.
  `ARM_COMPRESS_AFTER_DAYS` is pinned to `0002_arm_ledger.sql` by a no-DB test.
- The `#[ignore]` DB test compresses a throwaway chunk, caps decompression at one row, and
  runs a multi-row end seven times. It fails on the old statement and without `SET LOCAL`.

**C2 `d225e165` - confirm loops resolve from the feed, not PG** (`exec_real.rs::await_own_legs`)
- One helper serves `poll_feed_buy` and `confirm_sell`. It creates the wake future before
  the checks (`notify_waiters` reaches only futures that exist when it fires) and reads the
  own-leg preview on every wake. Postgres runs every 500 ms (buy) / 250 ms (sell), only when
  `WaitGuard::seq` moved, never at t=0, and once at the deadline.
- `sum_legs_by_signatures` / `find_fill_by_signature` take a `since` anchor that precedes
  the signatures, less `OWN_TX_LOOKUP_SLACK` (1 h): the buy's decision, the journal's first
  signing (`SubmittedBuyJournal::signed`), the position row, the exit's start, the sell's
  own block time.
- Tests: a leg landing during the query still wakes the waiter (fails on the old
  wake-in-`select!` shape), no query when the preview resolves, no re-query without a new
  trade, one query at a zero window. `d15fd970` repairs the two DB tests of the lookups.

**C3 `26a0501b` - a sibling sell queues on the mint, never skips** (`engine/mod.rs` `InFlightGuards`, `exec_real.rs::run_exit`)
- `exit_mints` is a FIFO `tokio::sync::Mutex` per mint, pruned by `remove_if` once no guard
  or waiter holds it. The `Arc` is cloned out of the map before any await.
- `EXIT_MINT_WAIT` (140 s) is the holder's worst case, derived from `SELL_ATTEMPTS` and the
  windows. Timeout emits `FillFailed::Reverted` (nothing was sent). After a wait the sell
  drops out if the registry's `inflight_intent` is no longer its intent.
- `orphan_exit` and the wallet Sell All keep try-lock semantics.

**C4 `4b2154f6` - protect the trader's memory** (`deploy/hunter.compose.yml`, `hunter-live-api`)
- `mem_reservation: 256m`: `memory.low` on this cgroup-v2 host.

Checked: `cargo check -p hunter-live -p hunter-lab`, clippy on touched code (no new
warnings), `hunter-core` 355 + `hunter-live` 105 tests, and the three `--ignored` DB tests.

## Phase 2 - deploy

`up -d --build live-api` compiles Rust on this box (`CARGO_BUILD_JOBS=2`). That is tens of
minutes at 100 % of both vCPUs, well past the credit baseline, and about 1-2 GB of rustc
memory next to Postgres's 1.7 GB: swap or OOM risk while the old bot still trades. So:
- Build only while every real rule is inactive and zero real positions are open. The
  current all-rules-off state is such a window.
- Save `docker logs hunter-live-api` to a file first (a recreate empties it), then recreate the
  api container (C4 needs a recreate, not a restart).
- Revert Phase 0 once the ledger logs its first clean multi-row end.

## Phase 3 - acceptance gates (24 h after deploy)

| Gate | Target |
| --- | --- |
| `arm ledger: end failed` | 0 |
| open share of arms armed after deploy | pre-regression level (0-8 %) |
| `/proc/pressure/io` full avg300; nvme `r_await` | < 10 %; < 3 ms |
| `decide_to_ack_ms` p90, first entries included | <= 50 ms |
| hunter-live `VmSwap` | ~0 |
| `entry_landed.ack_to_fill_ms` p50 | <= 100 ms |
| `mint exit lock held - skipping` | 0; sibling exits within one sell cycle |
| `ack_to_clear_ms` p50 | <= 700 ms |
| decision -> fill p50 / p90; slot p50 | <= 90 / <= 250 ms; 0 |
| `mark_buy_submitted timed out`, statement timeouts | 0 |
| CPU user+sys, 24 h avg (`sar -u`) | < 20 % (the t3 baseline) |
| memory pressure avg300; kswapd `pgscank/s` (`sar -B`) | ~0; ~0 |

For the gate window, turn `LATENCY_TRACE=1` on to read `recv_to_ping_ms` and
`ping_to_decide_ms`, i.e. whether the serialized loop queues on 2 vCPUs. It costs a mint clone
plus a map insert per trade ping; turn it off after.

If a disk gate fails with the storm gone, the next levers are in this order: the t3 CPU and
EBS headroom from Phase 0, then the 1-minute `trades_ohlcv_1m` refresh (5.5 s every minute).

## Phase 4 - docs

Done with Phase 1: `db-patterns.md` (DML on a compressed hypertable), `arm-ledger.md`,
`execution-workflow.md`, `trade-execution.md`, `strategies.md`, and the exit-detection
line in `latency-measurement.md`.

Open:
- `hunter/CLAUDE.md` gains one landmine row pointing to the `db-patterns.md` rule, once
  that file's in-progress edits are committed.
- `latency-measurement.md`: a new baseline from the Phase 3 numbers.
- A `docs/history/` entry records the ledger gap from Sep 11 15:00 UTC to the deploy. Those
  ends cannot be recovered and the rows age out through retention. No engine path reads
  open arms from PG.

## Out of scope, tracked separately

- `reverted 2006 (stale creator)`: 14 of 61 Flip-Catch entry attempts, each charged a fee.
  Root cause and fix: [stale-creator-2006-plan.md](stale-creator-2006-plan.md).
- Paper rules on the live box share the 2 vCPUs, the serialized decision loop and the arm
  ledger with the real rules (Sep 15: ~3 500 arms/h from 4 rules). Keeping paper studies in
  lab simulate is the budget lever if Phase 3's `ping_to_decide_ms` shows queueing.
