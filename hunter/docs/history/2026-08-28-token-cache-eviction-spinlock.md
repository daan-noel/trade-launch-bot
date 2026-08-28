# 2026-08-28 — the TokenCache eviction sweep froze the whole process, ~20x a day

A `DashMap` guard held across an `.await` in `run_token_cache_eviction` wedged the entire
tokio runtime every time the sweep found a dead token while a trade landed on the same
shard. The ingest watchdog force-exited the process 90 s later. Each occurrence cost
~105 s of feed, permanently.

Fixed in `hunter/core/src/state/token_cache.rs` (snapshot into `DeadFlush`, drop the
guard, then write). Gated by `scripts/check-async-guards.sh`.

## The live consequence — read this before trusting a number

**`trades` has ~15-30 holes per day of ~100-107 s each, from at least 2026-08-18 through
2026-08-28**, roughly **25-50 minutes of missing feed per day**. Nothing backfilled them:
`process::exit` discards the gRPC replay anchor, and the NATS relay has no replay at all.

Inside one hole (13:17:09 → 13:18:55 on 08-26) the chain advanced 292 slots and carried
25-30 pump.fun transactions **per slot**, while `trades` holds **zero rows** for those
slots. The holes are invisible to any query that does not look for them: the rows are
simply absent, and the surrounding data is healthy.

So any measurement over that window is short a random 2-4% of the feed, concentrated into
~20 contiguous gaps a day rather than spread evenly. Re-baseline anything whose result
turns on trade counts, gap/stall metrics, or per-token continuity over 08-18..08-28. A
stall-based exit (`m_price_lifetime.stall`) reads a hole as genuine silence, so any rule
tuned on it in that window is fitted partly to the outage.

## The mechanism

dashmap 4.0.2's shard lock is an unbounded spinlock — `dashmap::lock::RwLock::write` is
`loop { try_write() else cpu_relax() }`. It never parks, never yields, has no timeout.

1. The sweep runs every 2 minutes. For each dead token it took a shard read guard
   (`token_cache.get(mint)`) and held it across `info_repo.upsert_metrics(..).await`.
2. The `.await` released the *task* but not the *guard*. The worker thread moved to the
   next task — an ingest trade whose mint hashed to the same shard → `get_mut()` →
   `RwLock::write()` → spin at 100% CPU inside a non-async loop.
3. That worker could then never poll the eviction's pending future to completion, so the
   guard was never dropped and the spin never ended. Livelock.
4. `worker_threads = 2` on the deploy box, so the second worker blocked too. Ingest, the
   DB writer, SSE, the price poller, LaserStream and NATS all stopped at once.
5. The watchdog — deliberately on its own OS thread, which is the only reason anything
   survived — force-exited at 90 s. Restart and boot took a further 10-16 s.

## What the evidence looked like, so the next one is recognisable

Four consecutive incidents on 08-28 each began at the exact second an eviction pass was
due, and in every one that pass never logged its completion line:

| Last eviction logged | Next due | Freeze start (watchdog timestamp − its own stall figure) |
| --- | --- | --- |
| 15:49:08 | 15:51:08 | 15:51:08.11 |
| 16:00:49 | 16:02:49 | 16:02:49.15 |
| 16:34:20 | 16:36:20 | 16:36:20.42 |
| 16:44:02 | 16:46:02 | 16:46:02.44 |

Per-thread sampling through one freeze: **one `tokio-rt-worker` in state `R` burning
~110 ticks/s (100% of a core) for 100 straight seconds, every other thread at exactly
zero.** The machine itself was idle — no swap, no memory pressure, no IO stall.

The two signatures that identify this class, and separate it from the failures it
imitates:

- **A spin, not a stall.** One thread pegged at 100% with the rest at zero. A lock that
  *parks* shows all threads idle; a starved box shows pressure in `/proc/pressure`. Here
  both are absent, which is what a spinlock livelock looks like and nothing else does.
- **No decay.** Writes run at full rate to the last second and then stop dead. A wedged
  DB writer or an exhausted pool decays over tens of seconds first.

The watchdog message names the two causes it can distinguish ("feed stalled or DB pool
exhausted"). It was neither, both times it looked like either.

## The dependency, still on 4.0

The workspace pins dashmap 4.0 because solana 1.17.27 already pulls 4.0.2. dashmap 5 parks
instead of spinning, which would downgrade this failure from a hard livelock to a blocked
worker — but the code would still be wrong, and the pin is deliberate, so the fix is the
guard-lifetime rule and the script that enforces it, not the upgrade.
