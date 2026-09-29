# Dead-slot sweep - removing legs of blocks the chain abandoned

Code: `hunter/core/src/services/dead_slot_sweep.rs` (`DeadSlotSweep`). Runs as a loop in
`hunter-live` and as `cargo run -p hunter-lab -- sweep-dead-slots` on the workstation.

## The fault it removes

Ingest reads at `processed` commitment, on both wires (the NATS relay and LaserStream's
`Commitment::Processed` default), so a block is stored the moment its leader broadcasts it.
A leader can build on a fork the cluster then abandons. That block never finalizes: its
slot is skipped on the finalized chain, and every transaction in it never happened there -
an explorer reports the signature as not found. Its legs are true decodes of a real block,
so nothing at ingest tells them apart, and the writer keeps them.

The signature of such a leg in `trades`:

- its slot is missing from `getBlocks` at `finalized`;
- its reserves continue from an older slot than its neighbours' (the fork's parent), so the
  pool state steps backwards;
- a bot that sends one order several ways (tipped and untipped copies of a nonce-bound
  buy) shows the same order twice: the copy that won on the real chain and the copy that
  won on the fork, with different signatures and tips.

Worked case: `HgXQvUU6sdL68XYmT8eP524keYetZ4ALwZgq18Hspump`, slot 450566826 (32 legs over
15 mints, built on slot 820's state while the chain went 821..825 -> 827).

Size: a busy day (2026-09-26, 3,209,723 legs over 318,600 slots) carries 18 dead slots
holding 238 legs, mostly in runs of four - one leader's turn.

## What the sweep does

It never touches ingest or a decision: live keeps acting on processed data the instant it
lands, and the sweep corrects the stored history once the chain has finalized past it.

One step judges a window of received time `[cursor - OVERLAP, min(cursor + WINDOW,
newest leg))`:

1. `TradeRepo::slots_between` - each slot with legs in the window, and its earliest
   `block_time`.
2. `getSlot(finalized)` = the tip. Slots above it are not judged.
3. `getBlocks(finalized)` over the judged slots' range, 20,000 slots a call.
4. A judged slot with no block is dead. `TradeRepo::delete_slots_between` removes its legs
   inside the window.
5. The cursor moves to the window end, or to the earliest unjudged slot's time, and is
   stored in `app_settings` under `ingest.dead_slot_sweep_through` (per database).

| Knob | Value | Why |
| --- | --- | --- |
| `WINDOW` | 1 h | ~9,000 slots, one `getBlocks` call |
| `OVERLAP` | 10 min | a slot straddling a window edge, a writer batch landing after its stamp, and a gap replay (<= 300 s) stamped behind the live edge |
| `RPC_SPAN_SLOTS` | 20,000 | the public endpoint serves old ranges from long-term storage and times out on ~320,000; 20,000 answers in 2-10 s |
| `CATCH_UP_PAUSE` | 250 ms | a backfill neither floods the public endpoint nor holds a core on the 2-vCPU box |
| `IDLE_INTERVAL` | 60 s | one pass a minute once caught up: 1 `getSlot` + 1 `getBlocks` |

**Plausibility guard.** A window may hold at most `max(16, 1% of judged slots)` dead
slots. A long-term-storage answer missing blocks would read as dead slots and delete real
legs, so a step past the guard deletes nothing, keeps the cursor, and logs an error; the
next pass retries it.

**Cost.** Zero Helius: `CANONICAL_RPC_URL` defaults to `https://api.mainnet-beta.solana.com`,
and a Helius URL turns the sweep off. Postgres: one indexed `GROUP BY slot` over the
window and a `DELETE` bounded by `block_time`, on the batch pool. A fresh database sweeps
its whole history once from the oldest chunk (`tape_floor`); after that, the minute pass
reads about 11 minutes of legs.

## Where the cleaned rows go

- **Server:** the live loop cleans its own `trades`.
- **Workstation:** `db-incremental-sync.ps1` appends and never deletes, so the server's
  deletes never reach rows already copied. The script runs `sweep-dead-slots` after every
  pull (skip with `-SkipDeadSlotSweep`). The cursor never passes the newest local leg, so
  the next pull's rows land ahead of it.
- **Lake:** `lake-export` re-seals a day whose `_meta.json` row count no longer matches
  Postgres, so the next export rewrites exactly the days the sweep changed. A lake day
  older than local Postgres retention cannot be re-sealed and keeps its dead legs.

## Not covered

- **Live in-memory state.** `TokenCache` and the engine fold keep a dead leg until the
  token leaves the cache or the process restarts. A live check that detects a fork within
  a slot or two from reserve continuity (each leg's pre-trade reserve must equal the
  previous leg's post-trade reserve) is not built.
- **Derived tables.** `tokens.creation_slot` (a create that landed only on a fork),
  `tokens_info` aggregates and `build_breadth_day_stats` are computed from the legs as
  they arrived and are not recomputed.
