# A5 — Settings controls: gap-replay toggle + max replay window (Feature A)

> Workstream A (tpsl-realtime). A **safety layer, not the fix** — the real fix for stale snipes is
> [A4](04-snipe-freshness-gate.md). Default **OFF / 5 min** is conservative interim protection.
> Scope = **Mechanism A only** (see [00-gap-replay-mechanisms.md](00-gap-replay-mechanisms.md)).
> Paths are pre-crate-split — see [../README.md](../README.md#-path-caveat--line-refs-predate-the-crate-split).

## Goal — two operator controls on the Settings page for the LaserStream reconnect gap-replay

1. **Toggle** `gap_replay_on_reconnect` (bool, default **OFF**) — master on/off.
2. **Max replay window** `gap_replay_max_window_secs` (number, default **300 s**) — when ON,
   replay only if the disconnect gap ≤ this window; longer outages resume live (**gap-gate**).

Default OFF = a reconnect can never flood the buy path with stale creates, even before A4 lands.
The window bounds recovery once the operator turns replay on.

## Scope — Mechanism A only

Gate **only** the live reconnect replay in `client.rs` that feeds the buy path. **Do NOT** affect
Mechanism B (`token_sync` "Fetch All/Fetch New") — user-initiated, never buys, useful for charts.
Label the toggle precisely as **reconnect gap-replay**, not "replay" in general.

## Behavior

- **Toggle OFF (default)** → always reconnect live (`from_slot = None`); the gap is skipped (window
  ignored). Missed **trades** aren't recovered on reconnect (Mechanism B can still backfill on
  demand), but no stale **creates** reach the buy path.
- **Toggle ON, gap ≤ window** → resume from `last_slot+1`, re-fetch the missed window (keep the
  existing `PipelineBackpressure` / `ResourceExhausted` bailouts that already force live).
- **Toggle ON, gap > window** → resume live (`from_slot = None`). A long outage (e.g. the 10 h
  case) never replays — that's the gap-gate.

**Gap-gate measurement:** track an `Instant` of last stream progress in the client (set whenever
`last_slot` advances). At reconnect compute `disconnected_for = now − last_progress_at` and compare
to the window. No live-slot-tip estimation needed.

Guard at the `from_slot` computation
([client.rs:300-310](../../backend/src/ingest_laserstream/client.rs#L300-L310)):

```rust
let within_window = disconnected_for <= Duration::from_secs(gap_replay_max_window_secs);
from_slot = if seen > 0 && gap_replay_enabled && within_window
    && !matches!(reason, DisconnectReason::PipelineBackpressure
        | DisconnectReason::StreamError(tonic::Code::ResourceExhausted))
{
    Some(seen + 1)
} else {
    None
};
```

## Wiring (reuse the existing settings watch — no new endpoint)

Mirror the existing `live` / `persist_raw` / `track_mayhem` bool toggles end-to-end:

1. **[settings_repo.rs](../../backend/src/storage/repositories/settings_repo.rs)** — add
   `Setting::new("ingest.gap_replay_on_reconnect", || false)` (bool) and
   `Setting::new("ingest.gap_replay_max_window_secs", || 300)` (number). Add matching
   `pub gap_replay_on_reconnect: bool` + `pub gap_replay_max_window_secs: u64` on `AppSettings`,
   and the two `from_map()` lines. Bool mirrors `live`/`persist_raw`; number mirrors an existing
   numeric setting (e.g. `slippage_bps` / `max_committed_sol`).
2. **[system.rs](../../backend/src/api/handlers/system/system.rs)** (`PUT /api/system/settings`) —
   add both fields to `UpdateSettingsRequest` and the two existing patch spots (DB `set_many` +
   `state.modify_settings`). No new route.
3. **[client.rs](../../backend/src/ingest_laserstream/client.rs)** `run()` +
   **[main.rs](../../backend/src/main.rs)** spawn — thread the **existing** `settings_tx.subscribe()`
   (`watch::Receiver<AppSettings>`) into `run()` and read both
   `.borrow().gap_replay_on_reconnect` and `.borrow().gap_replay_max_window_secs` at the `from_slot`
   line. Add the `last_progress_at: Instant` tracking for the gap-gate. Reading the watch borrow once
   per reconnect is free (reconnects are rare). Changes take effect on the **next reconnect** — no
   restart.
4. **Frontend** — one `ToggleRow` + one numeric input row on
   [SettingsPage.tsx](../../frontend-react/src/pages/settings/SettingsPage.tsx) (greyed when the
   toggle is OFF), plus both fields on the `AppSettings` TS interface
   ([services/api.ts](../../frontend-react/src/services/api.ts)). `updateSettings` already forwards
   any `Partial<AppSettings>`, so no mutation changes. Descriptions: *toggle — off = skip missed
   data on reconnect, on = re-fetch the gap (can re-trigger snipes on old tokens unless the freshness
   gate is enabled); window — only replay when the disconnect was shorter than this many seconds.*

## Relationship to the fixes

A safety layer, not the fix. The real fix for stale snipes is [A4](04-snipe-freshness-gate.md),
which sits underneath both — even a within-window replay can't snipe stale creates. Once A4 lands,
turn the toggle **ON** with the window bounding recovery. Until then, default **OFF**.

## Done

- `cargo check -p backend-deploy` clean; `npm run build` clean; settings round-trip through the DB
  and take effect on next reconnect; no hot-path cost.
