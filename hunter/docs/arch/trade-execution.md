# Trade execution — `pump-trader` crate

File-level map of `pump-trader/` (crate `pump_trader`; has `lib.rs` + unit tests). `live` re-exports via `live/src/trader/mod.rs`.
Deep-dive detail: `@plans/trade-execution/module-details.md`, `@plans/trade-execution/slippage-logic-buy-sell.md`, `@plans/trade-execution/buy-in-flight-recovery.md`.
**Standalone & reusable:** the crate is a drop-in library — a consumer supplies a `TraderConfig` (sane `Default`s, `Arc<dyn Signer>` for HSM/remote-signer support), gets a typed `TradeError`, and never forks source to tune. No workspace deps.

## Modules — `src/trader/`

| File | Responsibility |
| --- | --- |
| `mod.rs` | `PumpFunTrader` struct + construction (program IDs sourced from `protocol` const Pubkeys — **no init-time parse/unwrap**); SOL-exposure tracking (`commit_sol_for_position`, `release_sol_for_position`, `can_commit_buy`); `buy_lamports_checked` validation (reads `config.limits.max_buy_sol`) |
| `buy.rs` | Bonding-curve buys; `buy_token_snipe_write_ahead` (Phase 2 write-ahead: hook fires on sign, before submit; `user_token_account_override` skips the template pool to re-buy into an existing account); `build_curve_buy_ixs` + `compute_curve_buy_min_out` (pure, reused by sim) |
| `sell.rs` | Durable-nonce curve sells; per-attempt Jito tip escalation; `close_token_account` (off-path rent reclaim) |
| `consolidate.rs` | **Off-path** pre-buy consolidation: `consolidate_token_accounts` sweeps non-canonical (orphan) accounts for a mint into the canonical ATA + closes them, so a buy/sell deals with one account. Happy path = one enumeration RPC, zero writes |
| `amm.rs` | PumpSwap AMM swaps (post-migration); WSOL wrap/unwrap; cached `GlobalConfig` (freshness-bounded); `refresh_amm_pool_info` (stale `coin_creator` self-heal, changed/unchanged) |
| `swap_retry.rs` | **SSOT** `classify_swap_revert(custom, route, direction) -> SwapRetryDecision` — maps a landed revert's `(code, route, direction)` to a retry decision: slippage retry (sell 6003 curve / 6004 AMM **and** buy 6002+6042 curve / 6004+6040 AMM), 2006 creator/coin_creator refresh (both directions), 6024 cashback refresh, 6005 reroute (sell-only); everything else → StopFeeBurn. Shared by this crate's own `confirm=true` in-call heal (`sell.rs`/`buy.rs`/`amm.rs`), `live`'s feed-confirmed bot sell + snipe-buy loops, and the `manual_buy` handler, all importing it instead of keeping their own code lists |
| `tx.rs` | `build_nonce_tx`/`build_recent_tx`; `send_transaction` (fan-out to all Sender endpoints, first-win); `signature_state_detailed` (returns landed-revert program error code) |
| `nonce.rs` | Zero-copy durable-nonce pool; push re-arm from the host's nonce-account feed (`on_nonce_account_update`, slot-gated + `use_epoch`-guarded) with the post-send poll demoted to a fallback (first read delayed by `nonce.refresh_first_delay_ms` on push-fed hosts); re-arms only after on-chain blockhash advances |
| `jito_tip.rs` | `JitoTipCache` — bg-refreshed tip-floor; Level 0→p95→p99→×escalation mult, clamped `[MIN,MAX]` |
| `pool.rs` | Pre-built buy-template seed pool; async replenish |
| `blockhash.rs` | `BlockhashCache` — recent-blockhash cache for AMM buys; push-fed via `Engine::set_cached_blockhash` (slot-gated `blocks_meta` bridge), refresher loop is a stall watchdog (fetches only when the feed didn't cover the tick) |
| `init.rs` | One-time setup: warm tip/blockhash caches, fill buy pools, launch bg refresh tasks |
| `query.rs` | Read-only RPC: `get_sol_balance`, `get_all_token_accounts`, `get_all_token_accounts_for_mint` (ALL accounts for one mint — drives sweep-sell/consolidation), `get_account_balance_raw`, `cached_token_account` (in-mem, no RPC), `resolve_buy_routing`, `resolve_curve_facts_batch` |
| `reserves.rs` | `ReserveCache` — WS-fed reserve snapshots, freshness-bounded, venue-tagged |
| `sim.rs` | **Simulation engine** — `simulate_ixs` (zero SOL, unsigned); `simulate_curve_{buy,sell}`, `simulate_amm_{buy,sell}`; reuses same ix builders as live trades |
| `probe.rs` | Diagnostics backing `probe` subcommands; curve + AMM simulations. **Behind `feature = "probe"`** (off by default) |
| `claim.rs` | Off-path cashback sweep (curve WSOL pot + AMM buyback pot + curve stable pot). **Behind `feature = "claim"`** (off by default) |

## Other modules

| File | Responsibility |
| --- | --- |
| `lib.rs` | Public facade re-exports (`config::*`, `error::{Result, TradeError}`, `protocol`; `claim`/`probe` re-exports are feature-gated) |
| `types.rs` | `TokenProgram`, `WalletHolding`, `BuyRouting`, `TokenBalance`, `CurveFacts` |
| `protocol.rs` | **Tier 1 — compile-time invariants.** Program IDs / WSOL mint / fee recipients as `const Pubkey` (via `pubkey!`); discriminators, AMM byte offsets, account spaces, `LAMPORTS_PER_SOL`. **Two distinct fee recipients** (`PUMP_CURVE_FEE_RECIPIENT` slot-17 exact match; `PUMP_AMM_BUYBACK_FEE_RECIPIENT` whitelist — do NOT swap them) |
| `config.rs` | **Tier 2 — operational tuning.** `TraderConfig` (4 required fields incl. `signer: Arc<dyn Signer + Send + Sync>` + `nonce_accounts: Vec<Pubkey>`) + 7 `Default` sub-structs (`ComputeBudgetCfg`/`JitoTipCfg`/`RetryCfg`/`NonceCfg`/`CacheCfg`/`SlippageCfg`/`LimitsCfg`). `TraderConfig::new(..)` builds with all tuning at defaults |
| `error.rs` | Crate-owned `TradeError` (`thiserror`, **no `anyhow`**) + `Result<T>` alias; local `Context` trait + `bail!` macro preserve the migrated call-site ergonomics. Large source errors (`ClientError`, nonce-utils) are boxed so the hot-path `Result` stays small |
| `constants.rs` | Thin back-compat shim re-exporting `LAMPORTS_PER_SOL`/`TOKEN_PROGRAM_ID` + a `&str` `WSOL_MINT` for external string consumers (`live`) |

## Key behaviors

- Every buy entry runs `buy_lamports_checked` before building a tx (rejects NaN/∞, ≤0, >MAX_BUY_SOL, dust).
- Curve sells use durable nonce; AMM buys use recent blockhash (exceeds nonce-tx size limit).
- `send_transaction` serializes the JSON-RPC body **once** (`Arc<Vec<u8>>`), fans out to all Sender endpoints concurrently — first success wins, tip paid once.
- Rent (~0.002 SOL) reclaimed via `close_token_account` after balance confirmed cleared — off the exit hot path.
- **Multi-account safety (off-path):** Solana can leave several token accounts per mint (the snipe template pool mints non-canonical ones; bot re-buys reuse whatever account the first fill picked). Manual *buy* is not latency-sensitive, so `buy_token_inner` always targets the real ATA and prefixes an idempotent `create_associated_token_account_idempotent` — no template pool, no create-with-seed account, so GMGN/explorers that resolve holdings via the canonical ATA see it immediately. Manual *sell* still enumerates EVERY account (`get_all_token_accounts_for_mint`) and sweeps+closes each, to mop up any non-canonical accounts left over from prior bot activity on that mint; manual *buy* also first runs `consolidate_token_accounts` to fold such orphans into the canonical ATA before buying. Bot buys/sells persist the account on the position row (`strategy_positions.token_account`) so a re-buy reuses one (non-canonical) account and the sell targets it across restarts.
- `initialize()` warms HTTP keep-alive pool so the first trade skips TLS handshake.
- Simulation engine is **off the hot path** (RPC round-trips); never inline before a real send.
- **SOL exposure lifecycle:** `commit_sol_for_position` debits before buy; `release_sol_for_position` (idempotent) credits on close or buy failure. `can_commit_buy` (balance-floor guard) + `max_committed_sol` setting gate every real buy before any position is created.
- **Buy flow (live `exec_real::run_entry`):** SOL guards in `dispatch_buy` → commit → adopt prior sigs → reserve re-quote → write-ahead hook (sign → persist sig → submit) → event-driven feed poll (~12 s) → a proven revert is classified by `classify_swap_revert`: buy slippage → `FillFailed::Reverted` (engine resubmits), 2006 → refresh-then-resend (unchanged → `Fatal`), structural/unknown → `Fatal`.
- **Sell flow (live `exec_real::run_exit`):** SOL released first (idempotent) → up to 6 tip-escalating attempts → event-driven per-sig balance poll (rate-limited ≥ 250 ms) → revert classified by error code → route re-read per attempt (migration auto-heals) → rent reclaim on clear (M1).
- **Strategy integration:** the engine adapters call `buy_token_snipe_write_ahead` / `sell_token_once`/`amm_sell` with `confirm=false`; fill confirmation comes from the `trades` LaserStream feed, never a new RPC call.
- **Zero-RPC AMM pool warmup:** `observe_amm_swap_accounts` harvests a migrated token's pool facts (`AmmPoolInfo` incl. the fee-share marker and the creator-vault pair) from one observed on-chain swap's account list — pure CPU, fed inline by hunter's ingest consumer. The old RPC `prewarm_amm_pool` (`getSignaturesForAddress` + `getTransaction` bursts) is deleted; `fetch_fee_share_marker` remains only as the cold fallback inside `amm_pool_info` (limit 5, sequential, early-exit). A round-trip guard test pins the harvest parser to `amm_swap_accounts`' builder layout; any layout drift fails safe to the cold path. `amm_config` (fee bps) is stale-while-revalidate — trades never block on its refresh once primed.
- **Stale-creator (2006) self-heal:** a `bonding_curve.creator` (curve) or pool `coin_creator` (AMM) can rotate on-chain after a cache read, reverting the next buy/sell with Anchor `ConstraintSeeds` (2006). Two triggers share one `swap_retry::classify_swap_revert` decision: (1) **sync** — every `confirm=true` swap (`sell_token`/`buy_token`/`amm_sell`/`amm_buy`) catches a confirmed 2006, refreshes the creator/pool cache, and resends ONCE with a fresh nonce (a confirmed revert bought/sold nothing, so this can't double-spend); refresh returning "unchanged" or erroring stops instead of re-paying fees. (2) **async** — `live`'s feed-confirmed bot sell loop and curve-buy snipe retry classify a landed revert the same way after their own poll window, refreshing the creator and continuing their existing attempt loop. See `stale-creator-2006-unify-plan.md` (repo root) for the design rationale.

Deep-dive on the full end-to-end workflow: [@plans/trade-execution/execution-workflow.md](@plans/trade-execution/execution-workflow.md)

## Unit tests

`cargo test -p pump-trader` (jito_tip, fan_out, reserves, min-out math).
