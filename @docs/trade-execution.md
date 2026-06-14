# Trade execution — `pump-trader` crate

File-level map of `pump-trader/` (crate `pump_trader`; has `lib.rs` + real unit tests). Backend re-exports via `backend/src/trader/mod.rs`.
Logic explainer: `@project_plans/trade-execution/slippage-logic-buy-sell.md`.

## Public surface (`src/lib.rs`)
`PumpFunTrader`, `TraderConfig`, `WalletHolding`, `BuyRouting`, `TokenBalance`, `TokenProgram{Legacy,Token2022}`; probe types `EndpointResult`, `FanoutReport`, `SimReport`.

`TraderConfig` fields: `rpc_url: String`, `helius_sender_urls: Vec<String>` (fan-out targets), `keypair: Keypair`, `nonce_accounts: Vec<String>` (round-robin).

## Modules — `src/`
| File | Key items | Responsibility |
|---|---|---|
| `lib.rs` | re-exports | public facade |
| `types.rs` | `TokenProgram`, `WalletHolding`, `BuyRouting`, `TokenBalance`, `CurveFacts` | type defs |
| `constants.rs` | program IDs, CU limits (curve buy 150k / sell 100k / AMM 180k), `MIN/MAX_JITO_TIP_SOL`, `JITO_TIP_PERCENTILE`, cache-refresh intervals, slippage/fee-buffer bps | protocol + tuning consts |

### `src/trader/`
| File | Key items | Responsibility |
|---|---|---|
| `mod.rs` | `PumpFunTrader`, `new()`, `initialize()`, `wallet_pubkey()`, `update_live_reserves()`, `rpc_url()` | struct + construction; holds RPC/HTTP clients + all caches |
| `buy.rs` | `buy_token` (confirmed, optional ATA check), `buy_token_snipe` (skips ATA-check RPC + RPC confirm, returns sig; caller confirms via feed) | bonding-curve buys, level-0 Jito tip, recent blockhash |
| `sell.rs` | `sell_token` (retry wrapper, ≤ `MAX_SELL_ATTEMPTS`, fresh nonce each), `sell_token_once(... tip_level, confirm)`, `sell_token_once_clearing(...)`, `close_token_account(token_mint, token_account_override)` | durable-nonce curve sells, per-attempt Jito tip escalation, `OnChainRevert` budget guard. `_clearing` appends an SPL `close_account` (program-routed legacy/2022) to recover token-account rent (~0.002 SOL) — **terminal full-balance sell only** (close reverts the tx if any balance remains). curve sell + nonce + close = 872 B (< 1232). `close_token_account` is the **off-hot-path** standalone rent reclaim: resolves the token account (override→cache→one scan), routes a lone `close_account` by cached `token_program`, builds it on a **recent blockhash** (`build_recent_tx`, no nonce slot), **no Jito tip**, sends via the RPC `send_transaction` (preflight **on**, fire-and-forget, no confirm) — preflight rejects a still-funded account in sim so no landed-revert fee |
| `amm.rs` | `amm_buy` (recent blockhash, no nonce), `amm_sell` (`tip_level`,`confirm`), `amm_sell_clearing(...)`, `prewarm_amm_pool` | PumpSwap AMM swaps (post-migration); WSOL wrap/unwrap, sim for min_out. `_clearing` also closes the base token account (rent recovery), terminal-sell only; AMM sell + nonce + close = 1186 B (< 1232) |
| `tx.rs` | `OnChainRevert`, `build_nonce_tx`, `build_recent_tx`, `send_transaction`, `confirm_transaction`, `encode_send_body`, `post_tx`/`post_tx_bytes` | `send_transaction` serializes the JSON-RPC body to bytes **once** (`Arc<Vec<u8>>`) then **fans out** to all Sender endpoints concurrently via `post_tx_bytes` (first success wins; sig dedup ⇒ lands once, tip paid once). `confirm_transaction` polls RPC w/ ramped backoff |
| `nonce.rs` | `acquire_nonce`, `schedule_nonce_refresh`, `fetch_nonce_hash_async` | zero-copy durable-nonce pool (`nonce_slots: std::sync::Mutex` — sync critical sections, no `.await` held); background hash refresh, `Notify` wakeup |
| `jito_tip.rs` | `TipFloor`, `JitoTipCache` (`store`, `tip_lamports_for_level`), `refresh_tip_floor` | bg-refreshed tip-floor cache. Level 0 = configured pct → 1 = p95 → 2 = p99 → ×`JITO_TIP_ESCALATION_TAIL_MULT`, clamped `[MIN,MAX]`. **Unit tests** here |
| `pool.rs` | `fill_buy_pool`, `acquire_buy_template`, `replenish_pool_async`, `prebuild_one_template_async` | pre-built buy-template seed pool (per token program), async replenish |
| `blockhash.rs` | `BlockhashCache` (`store`, `get_fresh`) | recent-blockhash cache for AMM buys (can't use durable nonce) |
| `init.rs` | `initialize()` | one-time setup: global account, rent, compute-budget ix, warm tip/blockhash caches, fill buy pools, launch bg refresh tasks |
| `query.rs` | `get_all_token_accounts`, `get_token_account_for_mint`, `resolve_cached_token_account`, `get_token_balance`, `resolve_buy_routing`, `resolve_curve_facts_batch`, `get_creator_from_mint_pda` | read-only RPC (not on trade hot path) |
| `reserves.rs` | `ReserveCache` (`update`, `get_fresh`) | WS-fed reserve snapshots, freshness-bounded, venue-tagged (curve vs AMM). **Unit tests** |
| `probe.rs` | `probe_tip_ladder`, `probe_fanout_self_transfer`, `probe_simulate_curve_sell` | zero/low-SOL diagnostics backing `probe` subcommands |

## Key behaviors
- **Helius Sender** already dual-routes (Jito + SWQOS) internally, 0 credits. Client-side multi-endpoint fan-out adds *geographic* redundancy, not extra Jito exposure. Endpoints from `HELIUS_FAST_SENDER_URLS` (CSV) or `HELIUS_FAST_SENDER_URL`.
- A tx that never lands costs nothing → tip escalation only ever costs more once it wins.
- Curve sells use durable nonce; AMM buys exceed the nonce-tx size limit so use a recent blockhash.
- **Rent recovery:** the preferred path is now `close_token_account` — an **async post-clear close** fired *after* the caller has confirmed the balance is cleared: a standalone single-`close_account` tx on a **recent blockhash** (no nonce slot), **no Jito tip**, sent through the RPC `send_transaction` with **preflight on**, **fire-and-forget** (no confirm). It is off the exit hot path, so reclaiming rent never adds latency to or contends with the sell. Preflight makes a still-funded (dust) account fail cheaply in simulation rather than paying a landed-revert fee; errors are returned for the caller to log and ignore. This supersedes the earlier "needs a one-line backend wiring" caveat for the inline-close variants. The `sell_token_once_clearing` / `amm_sell_clearing` variants (which append `close_account` to the terminal sell tx itself) remain available as *capability* methods for callers that want to ride the close on the final balance-→0 sell, but the standalone `close_token_account` is the recommended decoupled approach.
- `initialize()` warms the HTTP keep-alive pool (`getHealth` to each sender + RPC) so the first trade skips the TLS handshake; the reqwest client sets `tcp_nodelay(true)` + a 90 s `pool_idle_timeout`.

## Unit tests
`cargo test -p pump-trader` (e.g. `jito_tip`, `fan_out_returns_success_despite_a_failing_endpoint`, reserves).
