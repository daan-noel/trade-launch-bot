# Trade execution — `pump-trader` crate

File-level map of `pump-trader/` (crate `pump_trader`; has `lib.rs` + unit tests). Backend re-exports via `backend/src/trader/mod.rs`.
Deep-dive detail: `@plans/trade-execution/module-details.md`, `@plans/trade-execution/slippage-logic-buy-sell.md`, `@plans/trade-execution/buy-in-flight-recovery.md`.

## Modules — `src/trader/`

| File | Responsibility |
| --- | --- |
| `mod.rs` | `PumpFunTrader` struct + construction; SOL-exposure tracking (`commit_sol_for_position`, `release_sol_for_position`, `can_commit_buy`); `buy_lamports_checked` validation |
| `buy.rs` | Bonding-curve buys; `buy_token_snipe_write_ahead` (Phase 2 write-ahead: hook fires on sign, before submit); `build_curve_buy_ixs` + `compute_curve_buy_min_out` (pure, reused by sim) |
| `sell.rs` | Durable-nonce curve sells; per-attempt Jito tip escalation; `close_token_account` (off-path rent reclaim) |
| `amm.rs` | PumpSwap AMM swaps (post-migration); WSOL wrap/unwrap; cached `GlobalConfig` (freshness-bounded) |
| `tx.rs` | `build_nonce_tx`/`build_recent_tx`; `send_transaction` (fan-out to all Sender endpoints, first-win); `signature_state_detailed` (returns landed-revert program error code) |
| `nonce.rs` | Zero-copy durable-nonce pool; background hash refresh; re-arms only after on-chain blockhash advances |
| `jito_tip.rs` | `JitoTipCache` — bg-refreshed tip-floor; Level 0→p95→p99→×escalation mult, clamped `[MIN,MAX]` |
| `pool.rs` | Pre-built buy-template seed pool; async replenish |
| `blockhash.rs` | `BlockhashCache` — recent-blockhash cache for AMM buys |
| `init.rs` | One-time setup: warm tip/blockhash caches, fill buy pools, launch bg refresh tasks |
| `query.rs` | Read-only RPC: `get_sol_balance`, `get_all_token_accounts`, `resolve_buy_routing`, `resolve_curve_facts_batch` |
| `reserves.rs` | `ReserveCache` — WS-fed reserve snapshots, freshness-bounded, venue-tagged |
| `sim.rs` | **Simulation engine** — `simulate_ixs` (zero SOL, unsigned); `simulate_curve_{buy,sell}`, `simulate_amm_{buy,sell}`; reuses same ix builders as live trades |
| `probe.rs` | Diagnostics backing `probe` subcommands; curve + AMM simulations |
| `claim.rs` | Off-path cashback sweep (curve WSOL pot + AMM buyback pot + curve stable pot) |

## Other modules

| File | Responsibility |
| --- | --- |
| `lib.rs` | Public facade re-exports |
| `types.rs` | `TokenProgram`, `WalletHolding`, `BuyRouting`, `TokenBalance`, `CurveFacts` |
| `constants.rs` | Program IDs, CU limits, Jito tip bounds, `MAX_BUY_SOL`, **two distinct fee recipients** (`PUMP_CURVE_FEE_RECIPIENT` slot-17 exact match; `PUMP_AMM_BUYBACK_FEE_RECIPIENT` whitelist — do NOT swap them) |

## Key behaviors

- Every buy entry runs `buy_lamports_checked` before building a tx (rejects NaN/∞, ≤0, >MAX_BUY_SOL, dust).
- Curve sells use durable nonce; AMM buys use recent blockhash (exceeds nonce-tx size limit).
- `send_transaction` serializes the JSON-RPC body **once** (`Arc<Vec<u8>>`), fans out to all Sender endpoints concurrently — first success wins, tip paid once.
- Rent (~0.002 SOL) reclaimed via `close_token_account` after balance confirmed cleared — off the exit hot path.
- `initialize()` warms HTTP keep-alive pool so the first trade skips TLS handshake.
- Simulation engine is **off the hot path** (RPC round-trips); never inline before a real send.

## Unit tests

`cargo test -p pump-trader` (jito_tip, fan_out, reserves, min-out math).
