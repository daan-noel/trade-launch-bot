# TPSL Strategy Invariants

All 9 invariants that must be preserved when editing `live/src/strategies/` and `trading_core/src/strategies/`. See [@arch/strategies.md](@arch/strategies.md) for the module map. Both `tpsl_sniper_1` and `tpsl_sniper_2` share these invariants.

## 1. No double-buy

**Rule:** A position is written to DB (`status = Arming`) **before** the buy tx is submitted. The DB row is the authoritative lock — if the row exists, no new buy is allowed for that `(rule_id, mint)` pair.

**Mechanism:**
- `execution/real.rs::execute_buy()` calls `position_repo.insert(position{status: Arming})` first
- If insert fails (UNIQUE conflict), bail out — another instance already owns this entry
- Buy tx is then submitted. If it fails: `position_repo.delete(position_id)` (allowing retry)
- If tx succeeds but confirmation is lost (crash): boot reaper in `service.rs::recover_buy_submitted()` finds `status = BuySubmitted` rows, waits for the tx to land (via `trades` feed), then transitions to `Holding` — **never re-sends**

**Why:** Without write-ahead, a restart between "build tx" and "submit tx" can send a second buy for the same token at the same price, doubling exposure silently.

## 2. No double-sell

**Rule:** Only one sell path can be active per position at a time.

**Mechanism:** `ExitGuard` RAII object in `runtime_cache.rs`:

```rust
pub struct ExitGuard<'a> {
    cache: &'a RuntimeCache,
    position_id: Uuid,
}
impl Drop for ExitGuard<'_> {
    fn drop(&mut self) { self.cache.exiting.remove(&self.position_id); }
}
```

`service.rs::try_exit()`:
1. `cache.exiting.insert(position_id)` — atomic; returns false if already present
2. If false: bail (another exit path is in flight)
3. Spawn sell with `ExitGuard` moved into the task — automatically released on task completion/drop

All exit paths (trade-driven, clock-driven, manual sell) go through `try_exit()`.

## 3. Sell-confirm via `trades` feed — no new RPC

**Rule:** The exit loop confirms a sell fill by watching the `trades` table (populated by the gRPC feed), not by calling `getSignatureStatuses` or any other RPC.

**Why:** An extra RPC call reintroduces latency (round-trip vs. stream push) and creates a double-sell window: if the first confirm RPC times out and the sell is retried, but the original tx actually landed, both sells succeed.

**Mechanism:** After a sell tx is submitted, `execution/real.rs` registers the signature in the position's `submitted_sell_signatures` (TEXT[]). The exit watcher (`service.rs` wakeup loop) waits for `TradeSignals.notify(wallet, mint)` then checks `trade_repo.find_fill_by_signature()` — a fast indexed lookup by `tx_signature`.

**Attribution:** fills are matched per signature (`sum_legs_by_signatures`), not by net balance change. This handles the case where multiple positions in the same wallet exit the same token simultaneously.

## 4. Time exits fire on silence

**Rule:** `TimeStop` and `Stall` exits must trigger even when no new trades arrive for the token.

**Mechanism:** `runner.rs` fires a 1s clock tick regardless of trade activity:

```rust
tokio::select! {
    Some(event) = strategy_rx.recv() => { ... }
    _ = clock_tick.tick() => { sweep_time_exits(&mut services).await; }
}
```

`sweep_time_exits()` iterates all `Holding` positions across both strategies, calls `find_clock_driven_exit(now)`, and exits any that have exceeded their `time_stop_secs` or `stall_secs` threshold.

**Why without the tick:** if a token goes dead (no new trades), no `on_trade_executed` fires, so the position would sit open indefinitely even if `time_stop_secs` has elapsed.

## 5. Strategy eval reads runtime_cache only — never DB-per-event

**Rule:** Every call inside the `select!` hot path (entry gating, exit evaluation) must read from `StrategyRuntimeCache` only. No `sqlx` queries.

**What's cached:**
- All active rules (`Arc<Vec<Rule>>`)
- All `Holding` / `ExitPending` positions (`DashMap<position_id, Position>`)
- Entry guard set (`DashSet<(rule_id, mint)>`)
- Exit guard set (`DashSet<position_id>`)
- `LadderParams` per rule id (pre-extracted, no full Rule clone on hot path)
- Paper run state, paper position counters

**Boot:** `mod.rs::load_runtime_cache()` seeds from DB at startup. Subsequent DB writes are async spawns; cache transitions are inline in the `select!` body.

**Why:** At peak volume (1000+ trades/min), a DB round-trip per event would block the serialized `select!` loop and make exit latency unbounded.

## 6. Live-rule edit guard (409)

**Rule:** Fields that determine entry matching (mint filters, volume thresholds, buy amount) must not change while a rule is `is_active = true`.

**Mechanism:** `lifecycle.rs::update_rule()` checks `rule.is_active` before applying the patch. If active, frozen fields in the request body return `409 Conflict`.

Frozen fields (tpsl1): `buy_amount_sol`, `min_volume_usd`, `min_trade_count`, `allowed_mints`, `excluded_mints`, `trade_mode`.

Why: changing `buy_amount_sol` mid-run would make the position's token balance inconsistent with what the exit ladder expects. Changing `trade_mode` from `Paper` to `Real` mid-run would cause paper positions to be sent to the chain.

## 7. Clear-results guard

**Rule:** `POST /api/strategies/tpsl1/rules/{id}/paper-clear` is only allowed when `trade_mode = Paper` AND `is_active = false`.

**Why:** Clearing results while a paper run is active would delete in-progress positions. Clearing a real rule's results is a destructive operation that should never be available (real positions have financial consequences).

**Mechanism:** `handler.rs` enforces both conditions before delegating to `paper_trading_repo.clear_runs()`.

## 8. SOL balance-floor guard — never overdraft the wallet

**Rule:** Every real buy must pass `can_commit_buy(buy_lamports)` before any position is created. The guard always leaves 0.02 SOL free for sell fees and Jito tips on the next exit.

**Mechanism:** `trader.can_commit_buy` = `cached_balance − RESERVE_FLOOR(0.02 SOL) − committed_lamports ≥ buy_lamports`. Checked inline in `service.rs::on_token_created`, before `sync_position`. Fails open when the balance cache is empty (avoids blocking all buys on startup; the on-chain transaction is the real safety net).

**Why:** Without the guard, a wallet with exactly N SOL committed could accept another buy, leaving insufficient SOL to pay the sell's Jito tip — the sell would then fail to land regardless of price action.

## 9. `max_committed_sol` ceiling — cap concurrent exposure

**Rule:** If `trade.max_committed_sol` is set, a new real buy is blocked when it would push the running committed total past the ceiling, regardless of wallet balance.

**Mechanism:** Checked inline after the balance-floor guard: `committed_lamports + buy_lamports > max_lamports → continue`. The setting is read from the live-updated `AppSettings` borrow (DB-backed); no restart needed when changed.

**Why separate from guard 8:** the balance-floor prevents overdraft; `max_committed_sol` caps risk exposure — e.g. a 10 SOL wallet can be capped at 2 SOL in-play at any moment. `committed_lamports` is shared across both TPSL1 and TPSL2, so the ceiling applies to the combined open exposure of all strategies.
