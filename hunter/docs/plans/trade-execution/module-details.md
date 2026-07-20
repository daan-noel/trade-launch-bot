# Trade Execution — Module Detail

Deep-dive on `pump-trader/src/trader/` internals. See [@arch/trade-execution.md](@arch/trade-execution.md) for the file-level map and key behaviors.

## `buy.rs` — `buy_token_snipe_write_ahead`

The primary entry point for strategy buys. "Write-ahead" = Phase 2: the write-ahead hook fires **after signing but before submission**. This gives the caller a signed tx + position ID to persist before the tx hits the network.

```
validate (buy_lamports_checked)
  → build_curve_buy_ixs (or amm path)
  → sign tx
  → [HOOK: on_signed(position_id, signature)] ← persisted here (write-ahead moment)
  → send_transaction (fan-out)
  → return (signature, fill_result)
```

`build_curve_buy_ixs` is pure and reused by `sim.rs`. It takes `CurveFacts` (virtual reserves + bonding curve address) and the desired SOL input, computes `compute_curve_buy_min_out(sol_in, reserves, slippage_bps)`, then assembles:
1. `ComputeBudgetInstruction::set_compute_unit_limit`
2. `ComputeBudgetInstruction::set_compute_unit_price` (Jito tip — via `JitoTipCache`)
3. `pump_fun_buy(accounts, sol_amount, min_token_out)` ix
4. Fee-recipient transfer (slot-17: `PUMP_CURVE_FEE_RECIPIENT` — exact address match, not whitelist)

## `sell.rs` — durable-nonce curve sells

Curve sells use a durable nonce so the tx can be pre-built and re-signed on retry without a new blockhash RPC call.

**Per-attempt escalation pattern:**

```rust
for attempt in 0..MAX_SELL_ATTEMPTS {
    let tip = jito_tip_cache.level(attempt);   // 0: p95, 1: p99, 2: p99×mult, 3+: clamped max
    let nonce = nonce_pool.acquire().await;
    let tx = build_nonce_tx(ixs_with_tip(tip), nonce);
    match send_transaction(&tx).await {
        Ok(sig) => { nonce_pool.release_armed(nonce); return Ok(sig); }
        Err(e) if e.is_retryable() => { nonce_pool.release_spent(nonce); continue; }
        Err(e) => return Err(e),
    }
}
```

`close_token_account` is called **off the exit hot path** after the balance is confirmed cleared (not inline with the sell tx). It reclaims ~0.002 SOL rent and is best-effort — a failed close is logged, not retried.

## `amm.rs` — PumpSwap AMM swaps

AMM swaps use a **recent blockhash** (not durable nonce) because AMM txs are larger and exceed the nonce-tx size limit after adding WSOL wrap/unwrap instructions.

AMM buy flow:
1. `ensure_wsol_account()` — create associated token account if missing
2. `wrap_sol(amount)` — transfer SOL → WSOL ATA
3. `pump_swap_buy(pool, wsol_in, min_token_out)` ix
4. `unwrap_sol()` — close WSOL ATA to reclaim any residual

`GlobalConfig` is cached with a `freshness_secs` bound — fetched once at startup, re-fetched only when stale. It holds the protocol fee rate and fee recipient for the AMM program (`PUMP_AMM_BUYBACK_FEE_RECIPIENT` — whitelist of known addresses, **not** the curve's slot-17 exact match).

## `tx.rs` — send_transaction fan-out

`send_transaction` is the sole submission path for all trade types:

1. Serialize the signed tx to `Vec<u8>` → wrap in `Arc`
2. Clone `Arc` to each `Sender` endpoint (no copy of bytes)
3. Fan out concurrently via `tokio::join_all`
4. Return first `Ok(signature)`, discard remaining in-flight requests

All Jito tips go through the same path. The tip is baked into the tx instructions before `send_transaction` is called — the sender is tip-agnostic.

`signature_state_detailed` is a read-only RPC call (not part of the submit path) used by the write-ahead recovery reaper to check if a submitted-but-unconfirmed signature landed. It returns the program error code on revert (e.g., `NotAuthorized(6000)` = wrong fee recipient for that program).

## `nonce.rs` — durable-nonce pool

Pool holds N pre-fetched nonces (configurable, default 4). Each nonce has state:

```rust
enum NonceState {
    Armed { hash: Hash, blockhash_at_fetch: Hash },
    Spent,
}
```

**Re-arm logic:** a nonce advances its on-chain hash only after the Solana runtime processes a tx that uses it. The pool's background refresh thread polls `getAccountInfo` for each `Spent` nonce; it re-arms the nonce only when the on-chain `blockhash` has advanced past the hash that was current when the nonce was used. This prevents using a nonce that hasn't yet been advanced.

## `jito_tip.rs` — tip escalation

`JitoTipCache` fetches the percentile tip floors from the Jito tip API on a background interval (default 3s). `tip_lamports_for_level(level)` returns the **max** of:

1. **Live percentile ladder** — level 0 = configured percentile (default p75), 1 = p95, 2 = p99, then `p99 × escalation_tail_mult^(n-2)` (cold feed: floor-scaled).
2. **Floor escalation** — `min_sol × escalation_tail_mult^level` so retries still climb when live percentiles sit below the Sender floor.

Result is clamped to `[min_sol, max_sol]` (hunter defaults `0.001` / `0.005` — Sender Max priority-tip-buffer). Stale/cold feed falls back to the floor ladder.

## `sim.rs` — simulation engine

`simulate_ixs` sends a `simulateTransaction` RPC call with `sigVerify: false` and an empty signer list (zero SOL required). Returns `SimulateTransactionResponse` with CU consumed, logs, and inner instruction data.

`simulate_curve_buy` and `simulate_curve_sell` are wrappers that build the same ix structs as live trades (reusing `build_curve_buy_ixs` etc.) then call `simulate_ixs`. This means the simulation result is authoritative for fee/slippage computation.

**Critical:** simulation is RPC round-trip heavy. It is **never called on the hot path** — only used for the `probe` subcommand, strategy `simulate` endpoint, and the frontend's preview flow. Calling it inline before a live buy would add 200–800ms of latency.

## `constants.rs` — two distinct fee recipients

This is the most common source of `NotAuthorized(6000)` errors. **Do not swap these:**

| Constant | Program | Match type | Address |
|---|---|---|---|
| `PUMP_CURVE_FEE_RECIPIENT` | PumpFun bonding curve | Exact (slot-17 position) | `A7hAgCz...` |
| `PUMP_AMM_BUYBACK_FEE_RECIPIENT` | PumpSwap AMM | Whitelist (any of ~3 known) | various |

The bonding curve program requires the fee recipient at **exactly instruction account index 17**. The AMM program accepts any address from a whitelist. Swapping them causes a program error on submission, not a simulation failure (simulation doesn't verify account ownership).

## `reserves.rs` — ReserveCache

WS-fed (not polled). The cache is populated from two sources:
1. `on_trade_executed` in `pipeline.rs` — every decoded trade includes current virtual reserves; these are the most up-to-date
2. One-shot RPC fetch on first AMM trade for a mint — prewarms the AMM pool entry

Entries are tagged by venue (`Curve` / `Amm`) and have a `freshness_secs` threshold. Stale reads log a warning but are still returned (better than blocking on stale data during a sell decision).
