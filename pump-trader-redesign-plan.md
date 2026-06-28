# pump-trader standalone redesign

## Context

`pump-trader` is the real buy/sell executor (no simulation of SOL — `sim.rs` only does dry-run `simulateTransaction`). The goal is to make it a **totally isolated, drop-in library** reusable in other projects. Today it is ~95% standalone but has three couplings and one mis-categorised-constants problem:

1. **Constants are split on the wrong axis** (static vs dynamic). The right axis is *who owns the value*: protocol invariants (never change) vs operational tuning (re-tuned per deployment) vs per-call params.
2. **`anyhow::Result` leaks through 28 public functions** — the host's error model, not a library boundary.
3. **`config.keypair: Keypair`** forces callers to hand over a raw private key (no HSM / remote-signer option).
4. **`pump-constants` is a separate crate** re-exported as `pump_trader::constants`, mixing protocol IDs with tuning knobs.

Outcome: one self-contained crate where another project supplies a `TraderConfig` (sane `Default`s, override any knob), gets a typed error, and never forks source to tune.

## Design principle — three tiers, not "static vs dynamic"

| Tier | Examples | Lives in | Rationale |
| --- | --- | --- | --- |
| **1. Protocol invariants** | program IDs, WSOL mint, discriminators, byte offsets, account spaces, `LAMPORTS_PER_SOL` | `const` in a `protocol` module | Changing one = a *different protocol*. Never config. |
| **2. Operational tuning** | CU limits/price, jito tip bounds, retry/confirm counts, slippage bps, cache TTLs, `max_buy_sol` | `TraderConfig` sub-structs w/ `Default` | A/B-tuned per deploy; a consumer overrides without forking. |
| **3. Per-call params** | buy amount, `slippage_bps`, `tip_level` | function args (already present) | Vary per trade. |

CU price is **Tier 2 (a tuned scalar)**, not "dynamic" — the value that genuinely adapts at runtime (the Jito tip) is already dynamic via the tip-floor feed. Do not over-engineer CU price into a percentile strategy.

---

## Tier 1 — `protocol` module (compile-time invariants)

Move the protocol half of `pump-constants` into `pump-trader/src/protocol.rs`. Convert base58 **`&str` → `const Pubkey`** via the `solana_program::pubkey!` macro (a.k.a. `solana_sdk::pubkey!`; if unavailable on 1.17.27, fall back to a const `Pubkey::new_from_array`).

```rust
pub const PUMP_FUN: Pubkey   = pubkey!("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P");
pub const PUMP_SWAP: Pubkey  = pubkey!("pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA");
pub const WSOL_MINT: Pubkey  = pubkey!("So111...112");
// ...EVENT_AUTHORITY, FEE_PROGRAM, TOKEN, TOKEN_2022, fee recipients, JITO_TIP_ACCOUNTS (as [Pubkey; 10])
```

**Stays plain const** (non-pubkey): discriminators `[u8;8]`, `AMM_POOL_*_OFFSET`, `AMM_CONFIG_*_OFFSET`, `*_ACCOUNT_SPACE`, `TOKEN_ACCOUNT_RENT_PLACEHOLDER`, `LAMPORTS_PER_SOL`.

**Effect on `trader/mod.rs::new()`:** delete every `Pubkey::from_str(...).unwrap()` (≈12 fields: `pump_program`, `token_program`, `system_program`, `event_authority`, `fee_program`, `curve_fee_recipient`, `amm_buyback_fee_recipient`, `pump_swap_program`, `wsol_mint`, + jito tip account selection). Reference `protocol::PUMP_FUN` etc. directly at use sites. **Removes all init-time parsing and all `.unwrap()` panics.** PDAs (`amm_global_config_pda`, etc.) are still `find_program_address`-derived once in `new()` — now from `const` inputs.

---

## Tier 2 — `TraderConfig` (grouped sub-structs, each `Default`)

```rust
pub struct TraderConfig {
    // --- required, no Default ---
    pub rpc_url: String,
    pub helius_sender_urls: Vec<String>,
    pub signer: Arc<dyn Signer + Send + Sync>,   // was: keypair: Keypair
    pub nonce_accounts: Vec<Pubkey>,
    // --- tuning, all Default ---
    pub compute:  ComputeBudgetCfg,
    pub jito:     JitoTipCfg,
    pub retry:    RetryCfg,
    pub nonce:    NonceCfg,
    pub cache:    CacheCfg,
    pub slippage: SlippageCfg,
    pub limits:   LimitsCfg,
}
```

Full mapping of the 22 tuning constants → fields (each sub-struct `impl Default` = today's value):

| Sub-struct | Fields = (current const → value) |
| --- | --- |
| `ComputeBudgetCfg` | `curve_buy_cu` 150_000 · `curve_sell_cu` 100_000 · `amm_cu` 180_000 · `price_micro_lamports` 200_000 |
| `JitoTipCfg` | `min_sol` 0.0002 · `max_sol` 0.005 · `percentile` 75 · `escalation_tail_mult` 1.5 · `floor_url` · `floor_refresh_ms` 3_000 · `floor_max_age_ms` 30_000 |
| `RetryCfg` | `max_sell_attempts` 5 · `confirm_max_retries` 5 · `confirm_poll_ms` 1_000 · `confirm_poll_schedule_ms` [250,400,700,1000] |
| `NonceCfg` | `max_wait_iters` 200 · `wait_sleep_ms` 20 · `refresh_max_attempts` 8 · `refresh_retry_ms` 150 |
| `CacheCfg` | `reserve_max_age_ms` 3_000 · `blockhash_refresh_ms` 2_000 · `blockhash_max_age_ms` 10_000 |
| `SlippageCfg` | `amm_default_bps` 500 · `curve_fee_buffer_bps` 200 |
| `LimitsCfg` | `max_buy_sol` 5.0 · `buy_seed_pool_size` 16 |

**Performance:** hot-path reads (`buy_lamports_checked`, nonce loops, jito sizing, `curve_fee_buffer_bps`, confirm polling) become `self.config.<group>.<field>` — a load through the existing `Arc<TraderConfig>`, no lock/clone. CU instructions stay **pre-built once** in `initialize()` from `config.compute.*`; the per-AMM-buy inline reads at `amm.rs:991-992` switch to the pre-built `cu_ixs_amm` (matches the curve path; removes a per-trade ix rebuild). Net hot-path cost ≈ 0.

---

## Tier 3 — per-call params (already present)

`slippage_bps: Option<u64>` and `tip_level: u8` already exist on buy/sell. Keep them; `None` falls back to `config.slippage.*`. No change needed beyond sourcing the fallback from config.

---

## Isolation lever A — typed error, **full thiserror, no anyhow** (Q1 confirmed)

Remove `anyhow` from the crate entirely. A crate-owned `thiserror` enum with semantic variants + `#[from]` source wrappers, used on **both** public and internal signatures (~113 internal sites convert):

```rust
pub type Result<T> = std::result::Result<T, TradeError>;

#[derive(thiserror::Error, Debug)]
pub enum TradeError {
    // semantic — callers branch on these
    #[error("invalid buy amount: {0}")]        InvalidBuyAmount(String),
    #[error("slippage exceeded")]              SlippageExceeded,
    #[error("tx reverted: custom {custom:?}")] Reverted { custom: Option<u32> },
    #[error("confirmation timed out")]         ConfirmTimeout,
    #[error("not initialized")]                NotInitialized,
    #[error("account/pool not found: {0}")]    NotFound(String),
    // source wrappers via #[from] — replace anyhow's context chains
    #[error(transparent)] Rpc(#[from] solana_client::client_error::ClientError),
    #[error(transparent)] Http(#[from] reqwest::Error),
    #[error(transparent)] Json(#[from] serde_json::Error),
    #[error(transparent)] Pubkey(#[from] solana_sdk::pubkey::ParsePubkeyError),
    // decode family (base64/bincode/bs58) folded into one or kept separate
    #[error("decode: {0}")] Decode(String),
    // last-resort for ad-hoc bail! messages that have no semantic variant
    #[error("{0}")] Other(String),
}
```

**Migration of the ~113 internal sites:** `?` keeps working through the `#[from]` variants; `bail!("x")` → `return Err(TradeError::Other("x".into()))` (or a semantic variant where one fits); `ensure!(c,"x")` → `if !c { return Err(...) }`; `.context("x")?` → `.map_err(|e| TradeError::Other(format!("x: {e}")))?`. All 28 public signatures + internal helpers become `-> Result<T>` (the alias above). `anyhow` is dropped from `Cargo.toml`.

## Isolation lever B — `Arc<dyn Signer>`

`config.keypair: Keypair` → `config.signer: Arc<dyn Signer + Send + Sync>`.
- `.pubkey()` (≈20 sites): unchanged — `Signer::pubkey()`.
- Signing (`tx.sign(&[keypair], hash)` at `tx.rs:79,109`): `tx.try_sign(&[signer.as_ref()], hash)`.
- `pool.rs:125 insecure_clone()` for the background refill task → `Arc::clone(&signer)` (cleaner, no key copy).
- `create_with_seed` (`pool.rs`) needs only the base **pubkey** for derivation → still fine.

## Isolation lever C — feature gates

Gate the off-hot-path tools so a minimal consumer compiles only buy/sell:
- `feature = "probe"` → `trader/probe.rs` (+ `EndpointResult`, `FanoutReport`) — off by default.
- `feature = "claim"` → `trader/claim.rs` cashback (+ `PotStatus`, `ClaimOutcome`) — off by default.
- `sim` (`simulate_*`) reuses buy/sell ix builders; keep compiled in (or a default-on `sim` feature). The shared ix builders must stay in buy.rs/sell.rs regardless.

## Isolation lever D — absorb `pump-constants`

Fold the crate into `pump-trader`: protocol half → `protocol` module (Tier 1); tuning half → `Default`s (Tier 2). Drop the `pump-constants` path dep. (Other workspace crates that import `pump_constants`/`pump_trader::constants` repoint as part of the rebuild — Q4 below.)

---

## File-by-file change list

- `pump-trader/src/protocol.rs` **(new)** — Tier-1 consts (`pubkey!`).
- `pump-trader/src/error.rs` **(new)** — `TradeError`.
- `pump-trader/src/config.rs` **(new)** — `TraderConfig` + 7 sub-structs + `Default`s.
- `pump-trader/src/lib.rs` — re-export `protocol`, `error::TradeError`, `config::*`; drop `pub use pump_constants`.
- `pump-trader/src/trader/mod.rs` — new `TraderConfig`; delete parsed-once Pubkey fields + unwraps in `new()`; SOL-exposure/helpers return `TradeError`.
- `pump-trader/src/trader/init.rs` — pre-build CU ix from `config.compute.*`; refreshers from `config.jito`/`config.cache`; pool from `config.limits`.
- `pump-trader/src/trader/{buy,sell,amm,tx,nonce,jito_tip,query,reserves}.rs` — read tuning from `config.*`; signatures → `TradeError`; sign via `Arc<dyn Signer>`; `amm.rs` use pre-built `cu_ixs_amm`.
- `pump-trader/src/trader/{probe,claim}.rs` — `#[cfg(feature = ...)]`.
- `pump-trader/Cargo.toml` — add `thiserror`; **remove `anyhow`**; remove `pump-constants` path dep; define `[features] probe`, `claim`, `default = []` (sim stays in core).
- Callers (`backend-deploy/src/main.rs:439`) — build `TraderConfig` with `signer: Arc::new(keypair)`, map `TradeError`.

---

## How it works after — explicit walkthrough

**Construction — `PumpFunTrader::new(Arc<TraderConfig>)`**
1. No base58 parsing: program IDs are `const Pubkey` in `protocol`, referenced directly (or copied into fields with zero parse cost). No `.unwrap()` can panic here anymore.
2. Constant PumpSwap PDAs derived once from `protocol::*` inputs (unchanged logic).
3. Jito tip account chosen once from `protocol::JITO_TIP_ACCOUNTS` (now `[Pubkey; 10]`, no parse).
4. Tuning is **not** touched yet — it lives in `config`, read on demand.

**`initialize().await`**
1. Pre-builds `cu_ixs_curve_buy / curve_sell / amm` from `config.compute.{curve_buy_cu, curve_sell_cu, amm_cu, price_micro_lamports}` — built **once**, cloned per trade.
2. Spawns tip-floor refresher on `config.jito.{floor_url, floor_refresh_ms}`; blockhash refresher on `config.cache.blockhash_refresh_ms`.
3. Fills the buy-template pool to `config.limits.buy_seed_pool_size`; primes nonce hashes.
4. Returns `Result<(), TradeError>`.

**A buy — `buy_token(...).await -> Result<_, TradeError>`**
1. `buy_lamports_checked` validates against `config.limits.max_buy_sol` × `protocol::LAMPORTS_PER_SOL`; bad input → `TradeError::InvalidBuyAmount` (no `anyhow`).
2. Acquire nonce: spin bound `config.nonce.max_wait_iters`, sleep `config.nonce.wait_sleep_ms`.
3. `min_out`: `config.slippage.curve_fee_buffer_bps` + per-call `slippage_bps` (else `config.slippage.amm_default_bps`).
4. Prepend the **pre-built** `cu_ixs_curve_buy` clone (CU limit+price already baked from config).
5. Sign via `Arc<dyn Signer>` (`try_sign`), fan out to `helius_sender_urls`.
6. Confirm: `config.retry.{confirm_max_retries, confirm_poll_schedule_ms, confirm_poll_ms}`. Revert/timeout → typed `TradeError`.

**A sell — `sell_token(...)`** retries up to `config.retry.max_sell_attempts`; each retry climbs the tip ladder via `config.jito.{percentile, escalation_tail_mult, min_sol, max_sol}` against the live tip-floor cache; confirm via the same `config.retry` knobs. (Sell-confirm still polls the full gRPC-fed window before retry — unchanged, preserves the no-double-sell guard.)

**A second project reuses it:** `cargo add pump-trader` (no workspace deps), build `TraderConfig { rpc_url, helius_sender_urls, signer: Arc::new(my_signer), nonce_accounts, ..Default::default() }`, override only the knobs it cares about (e.g. `compute: ComputeBudgetCfg { amm_cu: 220_000, ..Default::default() }`), match on `TradeError`. Protocol invariants come along as `const`; nothing is read from env or globals.

---

## Verification

- `cargo check -p pump-trader` (default features) and `--no-default-features` (core buy/sell only) and `--all-features`.
- `cargo test -p pump-trader` — keep `buy_lamports_checked` tests (now asserting `TradeError` variants) and jito-tip ladder tests (now reading `JitoTipCfg`).
- `cargo check -p backend-deploy` after repointing the construction site + error mapping.
- Clippy clean on touched files; confirm no `Pubkey::from_str(...).unwrap()` remains in `new()`.
- Manual: `cargo run -p backend-deploy -- probe simulate-sell <mint>` (needs `--features probe`) to confirm a real path end-to-end against config defaults.

## Decisions (confirmed)

- **Q1 Error** — **full thiserror, drop `anyhow` entirely** (public + internal). Largest single piece of the work (~113 internal sites).
- **Q2 Signer** — **`Arc<dyn Signer + Send + Sync>`**.
- **Q3 Feature gates** — **gate `probe` + `claim`** (off by default); `sim` stays in core.
- **Q4 Constants** — **fold `pump-constants` into `pump-trader`**; drop the separate crate.

## Effort / sequencing note

Order to minimise breakage: (1) `protocol` module + `pubkey!` consts → delete parse/unwrap in `new()`; (2) `config.rs` sub-structs + thread `config.*` through call sites; (3) `Arc<dyn Signer>` swap; (4) `error.rs` full thiserror sweep (touches every module — do last, in one pass); (5) feature-gate probe/claim; (6) repoint `backend-deploy` construction + drop `pump-constants`. Steps 1–3 keep `cargo check` green incrementally; step 4 is the big atomic sweep.
