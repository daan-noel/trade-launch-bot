# Event log — format, rotation, recovery, replay

Deep-dive reference for the strategy engine's **event log** (redesign decision 12).
Companion to the engine design plan
([fingerprint-metrics-engine-plan.md](fingerprint-metrics-engine-plan.md)); this file
documents the log itself — its wire format, the recorder, boot recovery, and the
Phase-6 time-travel inspector.

## 1. What it is (and is not)

The engine is a pure fold — `reduce(&mut EngineState, Event) -> Vec<Effect>` — over one
ordered event stream. Because the fold is deterministic and reads no clock/DB/entropy,
**the event stream fully determines every decision.** The event log is that stream,
persisted: append every loggable engine event to disk and any live run can be
reproduced offline by replaying the log through the same `reduce`.

- It is **not a DB table.** Positions in Postgres stay the source of truth; the log is
  for determinism, crash recovery, and debugging only. Losing it loses reproducibility,
  never money or position state.
- It is a **rotated, append-only local file** — cheap JSONL, daily-rotated,
  retention-capped. Trades dominate its volume.
- Ticks are **not** logged (they are regenerable — replay derives them from event
  timestamps). `RulesReloaded` is **not** logged (rules are reloaded from PG).

## 2. Wire format (SSOT: `hunter/engine/src/event_log.rs`)

The on-disk format is the single type
[`hunter_engine::event_log::LoggedEvent`](../../../engine/src/event_log.rs) — defined
**once in the pure engine crate** so the live recorder (writer) and the lab inspector
(reader) can never drift. It is the serializable projection of `Event`:

| `LoggedEvent` variant | Carries | Notes |
| --- | --- | --- |
| `TokenCreated` | `mint`, `fp` (instant axes), `at` | first-slot axes still unknown |
| `FirstSlotSettled` | `mint`, `buy_lamports`, `sell_lamports`, `at` | resolves first-slot fingerprint axes |
| `Trade` | `mint`, `trade` (`TradeLite`: side/sol/price/reserve_sol/at) | dominant volume |
| `FillConfirmed` | `intent`, `fill` (price/sol/token_amount/at) | entry **or** exit fill |
| `FillFailed` | `intent`, `reason` (`Reverted`/`Timeout`/`Unconfirmed`) | drives retry policy |
| `Migrated` | `mint`, `at` | token left the curve |
| `ManualClose` | `position` | manual sell / stop-all |

**Excluded on purpose:** `Event::Tick` (regenerable) and `Event::RulesReloaded`
(reloaded from PG). `LoggedEvent::from_event` returns `None` for both, so the recorder
skips them; `into_event` maps every logged variant back to its `Event`.

Encoding: one `serde_json` object per line (JSONL). Serde tags each line by variant
name (`{"Trade": {…}}`), and every inner type is an engine type with matching serde
derives, so a line written by live deserializes byte-for-byte on the lab side.

## 3. Recorder (live: `hunter/live/src/strategies/engine/event_log.rs`)

`EventLogRecorder` is created once in the decision loop and `record(&event)` is called
for every event **before** it is folded, so the log reflects exactly the inputs the
engine saw, in order.

- **Files:** `events-YYYY-MM-DD.jsonl` in `$EVENT_LOG_DIR` (created if missing).
- **Rotation:** daily, by UTC date — the first `record` of a new UTC day reopens a new
  day-file and prunes expired ones.
- **Retention:** files older than `$EVENT_LOG_RETENTION_DAYS` are deleted on rotation.
- **Best-effort:** a write/serialize error is logged and swallowed — a failed log line
  must never stop trading. If the directory can't be created, logging is disabled
  (`from_env() -> None`) and the engine runs without a log.
- **Flush:** each line is flushed on write (durability over throughput — the log is not
  on the hot decision path's latency budget).

### Environment

| Var | Meaning | Default |
| --- | --- | --- |
| `EVENT_LOG_DIR` | directory the log is written to | `event_log` |
| `EVENT_LOG_RETENTION_DAYS` | days of rotated day-files to keep | `7` |

Both are in `hunter/.env` + `.env.example`.

## 4. Boot recovery (armed-state rebuild)

On startup, after the initial rule load, the loop calls `recover_armed` to rebuild
in-memory **armed** state that a crash would otherwise lose. It is deliberately
conservative:

- Replays only the **recent tail** — events newer than `MAX_SNIPE_AGE_SECS` (older
  tokens are past the snipe window and can never arm).
- Replays only **pre-entry** events (`TokenCreated`/`FirstSlotSettled`/`Trade`/
  `Migrated`) — never fills or manual-closes, so no position is resurrected from the log.
- **Excludes any mint that already has an open PG position** (`find_open_positions`)
  **and any mint that reached a fill/close in the log** — a held token can never be
  re-armed or re-entered.
- Effects produced during replay are **discarded** — recovery rebuilds arm state only;
  PG rows (`Holding`/`BuySubmitted`/`ExitPending`) are reconciled by the reapers, which
  own in-flight rows.

Net effect: after a restart the engine re-arms the tokens it was watching (but hadn't
entered) and lets PG + the reapers settle anything in flight. Full re-adoption of open
positions into engine state is deferred (PG + reaper are authoritative for them).

## 5. Replay / inspection — the time-travel debugger (Phase 6.1)

`POST /api/replay/inspect` (lab bin) loads a recorded log, re-runs `reduce` over it, and
returns every `event → effects` decision as JSON. Backend:
`hunter/lab/src/api/handlers/replay.rs` (handler) +
`hunter/lab/src/strategies/replay_inspect.rs` (fold driver). Frontend viewer = FE plan
FE6.

### Two inputs supplied outside the log

1. **Rules** — reloaded from PG (the log omits `RulesReloaded`). An inspection therefore
   replays the recorded events against the **current** rule set. This is the intended
   "what would this token do under today's rules" lens; a rule changed since the run
   decides differently. `active_only` (default `false` ⇒ all rules, so a since-paused
   rule that fired in the log still arms) and `rule_ids` narrow the set.
2. **Ticks** — regenerable, so (exactly like the replay driver and boot recovery) the
   inspector interleaves synthetic 500 ms ticks on the `hunter_engine::TICK_MS` grid
   between logged event timestamps, letting tick-driven decisions (stall/dead/
   TP-on-tick) reproduce. `synthetic_ticks: false` shows only the logged events' direct
   effects. When no token is tracked the tick loop skips the whole gap in O(1), so a
   multi-hour quiet stretch in a day-file never emits millions of ticks.

Unlike [`replay.rs`](../../../lab/src/strategies/replay.rs) (which synthesizes fills
because the lake has none), the log already contains the real `FillConfirmed`/
`FillFailed` events, so the inspector replays them verbatim — no sim fill model.

### Request

```jsonc
POST /api/replay/inspect
{
  "dir": null,               // log dir; default EVENT_LOG_DIR, else "event_log"
  "date": "2026-07-16",      // one YYYY-MM-DD day-file; omit ⇒ every day-file in dir
  "mint": null,              // dump only steps touching this token (whole log still folded)
  "since": null,             // RFC3339; dump only steps at/after
  "until": null,             // RFC3339; dump only steps at/before
  "synthetic_ticks": true,   // interleave 500 ms ticks
  "active_only": false,      // false ⇒ load all rules (incl. paused)
  "rule_ids": null,          // restrict loaded rules to these ids
  "max_steps": 10000         // cap dumped steps (truncated flag set if hit)
}
```

### Response

```jsonc
{
  "dir": "event_log",
  "files": ["events-2026-07-16.jsonl"],
  "rules_loaded": 3,
  "fingerprints_loaded": 5,
  "logged_events": 12048,    // logged events folded
  "synthetic_ticks": 640,    // ticks folded
  "events_replayed": 12688,  // logged + ticks
  "steps_returned": 214,
  "truncated": false,
  "steps": [
    { "seq": 0, "at": "…", "event": { "TokenCreated": { … } }, "effects": [] },
    { "seq": 7, "at": "…", "event": { "Trade": { … } },
      "effects": [ { "effect": "SubmitBuy", "intent": {…}, "rule": "…", "mint": "…", "lamports": 1000000000 } ] },
    …
  ]
}
```

`Effect` is not itself `Serialize`; the dump uses an `effect`-tagged projection
(`SubmitBuy` / `SubmitSell` / `PositionUpdate` / `ArmedChanged`) in `replay_inspect.rs`.

### Slicing caveats (important for correct reading)

- The engine's concurrency/lifetime caps are **cross-token** (per-rule counters shared
  across tokens), so a faithful replay must fold the **whole** log against one
  `EngineState`. The `mint` / `since` / `until` filters therefore narrow only the
  **output** — every event is still folded, so a filtered token's cap pressure from
  other tokens is still honored.
- `date` is the exception: it selects which day-files are **loaded at all**. A token
  created on an earlier day won't be armed if that day's file is excluded — load the
  full range (omit `date`) for a token whose lifecycle spans a rotation boundary.
- A `mint`-filtered dump includes a synthetic-tick step only when one of that tick's
  effects references the mint (a tick's input references no token).

## 6. Parity — why this reproduces live decisions

Live, boot-recovery, the analysis replay driver, and this inspector all call the **one**
`hunter_engine::reduce` and derive their tick cadence from the **one**
`hunter_engine::TICK_MS`. They differ only in *who produces events* and *who consumes
effects*. So an inspection over a recorded log reproduces the live decisions exactly,
modulo the two documented substitutions (current rules for the run's rules; regenerated
ticks for the run's ticks). Divergence would require a `reduce` or `TICK_MS` change,
which the engine's golden-log tests and the replay tick-guard (`tick_matches_engine_ssot`)
catch.

## 7. File map

| File | Role |
| --- | --- |
| `hunter/engine/src/event_log.rs` | `LoggedEvent` — the on-disk format SSOT (writer+reader share it) |
| `hunter/live/src/strategies/engine/event_log.rs` | `EventLogRecorder` (write/rotate/retain) + `recover_armed` (boot) |
| `hunter/lab/src/strategies/replay_inspect.rs` | inspector fold driver (log read + `reduce` + JSON projection) |
| `hunter/lab/src/api/handlers/replay.rs` | `POST /api/replay/inspect` handler (loads rules from PG) |
