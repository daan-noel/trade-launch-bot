# A4 — Bot sniped stale/dead tokens — add a freshness gate (Error 3)

> Workstream A (tpsl-realtime). **Requires [A3](03-replay-blocktime-anchor.md) first** — a
> replayed create gets `created_at = now()` and would defeat this gate until A3 lands.
> This is the **money-protecting fix** (see [00-gap-replay-mechanisms.md](00-gap-replay-mechanisms.md)).
> Apply to **both** TPSL1 + TPSL2 (clones).
> Paths are pre-crate-split — see [../README.md](../README.md#-path-caveat--line-refs-predate-the-crate-split).

## Report

The bot bought tokens — e.g. `4syqCLagMxUKYWTjFuz2aUigzV7gefqhLgsGSFjJpump` and
`9eAKH9JrQxepX5Q73zwrhimkvGwRhWS6NvwS5jGkpump` — **created 10+ hours ago with no trade history
since creation**. A sniper should only buy fresh launches.

## Root cause — no freshness/age gate on the entry path

The buy decision matches **only the token's creation-transaction properties**, with **no
token-age criterion**:

- `on_token_created` ([service.rs:177-201](../../backend/src/strategies/tpsl_sniper_1/service.rs#L177-L201))
  delegates to `find_all_matching_buy_rules`
  ([entry/mod.rs:63-81](../../backend/src/strategies/tpsl_sniper_1/entry/mod.rs#L63-L81)).
- The criteria list ([entry/mod.rs:36-43](../../backend/src/strategies/tpsl_sniper_1/entry/mod.rs#L36-L43))
  is `initial_buy_sol`, `cu_limit`, `cu_price`, `max_sol_cost`, `spendable_sol_in`,
  `instruction_labels` — all immutable properties of the **create tx**. None depend on token age
  or post-create activity.

So **any** `TokenCreated` event that structurally matches is bought, whether the token is 2 s or
10 h old. `Time` / `Stall` / `Liq` params are *exit* controls, not entry filters.

`token.created_at` is the genuine on-chain create `block_time`
([create.rs:164,178-184](../../backend/src/ingest_laserstream/decoder/create.rs#L164)) — the
strategy *has* the data, it just never checks it.

## The trigger — stale creates delivered late via reconnect replay (Mechanism A)

Activation and boot-seeding were ruled out (neither pings the strategy). The only source of a
`TokenCreated` ping is live ingest ([pipeline.rs:446](../../backend/src/ingest_laserstream/pipeline.rs#L446)).
After the overload from Errors 1+2, the stream dropped and reconnected, and `client.rs` replayed
`from_slot = last_slot+1` (up to Helius's ~24 h window) straight through the normal pipeline →
`ping_strategy(TokenCreated)` → buy path
([client.rs:180-184,300-310](../../backend/src/ingest_laserstream/client.rs#L180-L184)). With no
freshness gate each replayed old create was treated as fresh, and with the cap broken
([A1](01-concurrency-caps-inflight.md)) it bought every match.

> The **token_sync backfill** (Mechanism B, "Fetch All/Fetch New") is **NOT** the trigger — it
> never pings the strategy. See [00-gap-replay-mechanisms.md](00-gap-replay-mechanisms.md).

## Fix (recommended)

1. **Add a freshness criterion** to the entry path: reject the buy when
   `Utc::now() - token.created_at > MAX_SNIPE_AGE`. Implement as the first guard in
   `on_token_created` (cheapest — bail before building the handler) or as a `check_*` in the
   `CRITERIA` list in [entry/mod.rs](../../backend/src/strategies/tpsl_sniper_1/entry/mod.rs).
   Make the threshold small and configurable (e.g. 5–30 s).
2. **(Defense in depth)** Ensure the replay/backfill path never *pings the strategy* with
   historical creates — persist them, but don't route old-slot `TokenCreated` into the live buy
   path. Optionally drop/flag any create whose `block_time` lags current slot time by more than the
   threshold before pinging.

## Scope & done

- Mirror in **TPSL1 + TPSL2** (shared entry-matcher shape).
- Depends on A3 (accurate `created_at` on replayed creates).
- `cargo check -p backend-deploy` clean; unit-test the age guard rejects a stale `created_at` and
  passes a fresh one.
