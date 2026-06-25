# Plan: extract a dedicated `pump-constants` crate

**Goal:** one audited home for the *completely static* on-chain facts (program IDs,
discriminators, mints/authorities/fee recipients, unit/math), shared by both
`backend` and `pump-trader`. Kill the duplicated buy/sell discriminators.

**Core design lever — zero call-site churn.** Constants only *move*; the old module
paths (`pump_trader::constants::*`, `backend::config::constants::*`) become
**re-exports** of the new crate. Every existing `constants::NAME` reference keeps
resolving. The only behavioural edit is de-duplicating the buy/sell discriminators
in `pump-trader` (Task 6).

**Crate graph after:** `backend → pump-trader → pump-constants`, and `backend →
pump-constants`. `pump-constants` has **no dependencies** (all values are
`&str` / `[u8; 8]` / `u64` / `f64` / `&[&str]` / `usize`).

## Scope

**MOVES into `pump-constants` (Tier-1 static facts):**
- All program IDs (pump-trader's + backend `protocol.rs`'s) + system/ALT/aggregator/bot IDs
- Mints & authorities: `WSOL_MINT`, `EVENT_AUTHORITY`, `FEE_PROGRAM_ID`, `ASSOCIATED_TOKEN_PROGRAM_ID`
- Fee recipients + Jito tips: `PUMP_CURVE_FEE_RECIPIENT`, `PUMP_AMM_BUYBACK_FEE_RECIPIENT`, `PUMP_AMM_CASHBACK_GLOBAL`, `JITO_TIP_ACCOUNTS`
- All discriminators: instruction + event (backend `discriminators.rs`) + cashback (`SYNC_UVA_DISC`, `CLAIM_CASHBACK_DISC`, `CLAIM_CASHBACK_V2_DISC`)
- Math/units: `LAMPORTS_PER_SOL`, `INITIAL_VIRTUAL_*`, `INITIAL_REAL_TOKEN_RESERVES`, `TOKEN_TOTAL_SUPPLY`, `PUMPFUN_GENESIS_PRICE_PER_RAW_TOKEN`

**STAYS PUT (tunables / policy / impl-detail — NOT facts):**
- pump-trader tuning: `BUY_SEED_POOL_SIZE`, `MAX_BUY_SOL`, all `JITO_TIP_*` tuning, `COMPUTE_UNIT_*`, `MAX_SELL_ATTEMPTS`, `CONFIRM_*`, `NONCE_*`, `RESERVE_CACHE_MAX_AGE_MS`, `BLOCKHASH_*`, `AMM_DEFAULT_SLIPPAGE_BPS`, `CURVE_FEE_BUFFER_BPS`, `TOKEN_ACCOUNT_RENT_PLACEHOLDER`
- backend `tuning.rs` (everything) + all module-local sizing caps
- Helper fns `program_friendly_name` (backend) and `total_supply_for` (backend) — stay, importing from the crate
- **Phase 2 / optional:** account-layout offsets & sizes (`AMM_POOL_*`, `AMM_CONFIG_*`, `TOKEN_ACCOUNT_SPACE`, `TOKEN_2022_ACCOUNT_SPACE`, and fn-local offsets in `query.rs`/`claim.rs`). Static but trader-private and not duplicated — defer to Task 9 only if desired.

> **Build note:** if `backend.exe` is running it locks `target/`; append `--target-dir target-check` to every `cargo check`. Each task ends with its own check — do them in order.

---

## Task 1 — Scaffold the crate

1. Create dir `pump-constants/src/`.
2. Create `pump-constants/Cargo.toml`:
   ```toml
   [package]
   name = "pump-constants"
   version = "0.1.0"
   edition = "2021"
   description = "Static Pump.fun/Solana on-chain facts (program IDs, discriminators, mints, math) shared by backend + pump-trader."

   [lib]
   name = "pump_constants"
   path = "src/lib.rs"

   [dependencies]
   ```
3. Edit root [Cargo.toml](../Cargo.toml) line 2:
   ```toml
   members = ["backend", "pump-constants", "pump-trader"]
   ```
4. Create `pump-constants/src/lib.rs`:
   ```rust
   //! Completely-static on-chain facts shared across the workspace.
   //! Single source of truth — no tunables here (those live in their owning crate).
   pub mod discriminators;
   pub mod fees;
   pub mod ids;
   pub mod math;

   pub use discriminators::*;
   pub use fees::*;
   pub use ids::*;
   pub use math::*;
   ```

**Verify:** task ends compilable only after Tasks 2–5 add the modules; skip standalone check here.

---

## Task 2 — `pump-constants/src/ids.rs` (program IDs, mints, authorities)

Create the file. **Copy values verbatim** from the sources below (do not retype byte/string literals — copy to avoid transcription errors).

- From [pump-trader/src/constants.rs:9-24](../pump-trader/src/constants.rs#L9): `ASSOCIATED_TOKEN_PROGRAM_ID`, `EVENT_AUTHORITY`, `FEE_PROGRAM_ID`, `PUMP_FUN_PROGRAM_ID`, `TOKEN_PROGRAM_ID`, `TOKEN_2022_PROGRAM_ID`, `PUMP_SWAP_PROGRAM_ID`, `WSOL_MINT` (with their doc comments).
- From [backend/src/config/constants/protocol.rs:13-30](../backend/src/config/constants/protocol.rs#L13): `SYSTEM_PROGRAM_ID`, `COMPUTE_BUDGET_PROGRAM_ID`, `ADDRESS_LOOKUP_TABLE_PROGRAM_ID`, and all aggregator/bot IDs (`ARBITRAGE_BOT_*`, `AXIOM_TRADE_PROGRAM_ID`, `PHOTON_PROGRAM_ID`, `GMGN_BOT_PROGRAM_ID`, `DFLOW_AGGREGATOR_V4_PROGRAM_ID`, `TERMINAL_FORMERLY_PADRE_PROGRAM_ID`, `TROJAN_TRADE_PROGRAM_ID`, `JUPITER_AGGREGATOR_V6_PROGRAM_ID`, `BLOOM_ROUTER_PROGRAM_ID`, `METEORA_DAMM_V2_PROGRAM_ID`).

> Do **not** move the `program_friendly_name` fn — it stays in backend (Task 7).

---

## Task 3 — `pump-constants/src/fees.rs` (fee recipients + Jito tips)

Create the file, copying verbatim **including the full audit doc-comments** (they encode the [[fee-recipient-rotation-bug]] history — preserve them exactly):
- From [pump-trader/src/constants.rs:42-80](../pump-trader/src/constants.rs#L42): `PUMP_AMM_CASHBACK_GLOBAL`, `PUMP_CURVE_FEE_RECIPIENT`, `PUMP_AMM_BUYBACK_FEE_RECIPIENT`, `JITO_TIP_ACCOUNTS`.

---

## Task 4 — `pump-constants/src/discriminators.rs`

Create the file. Copy verbatim:
- **All** of [backend/src/config/constants/discriminators.rs:1-104](../backend/src/config/constants/discriminators.rs#L1) (every instruction + event discriminator, with the header comments).
- The cashback discriminators from [pump-trader/src/constants.rs:32-40](../pump-trader/src/constants.rs#L32): `SYNC_UVA_DISC`, `CLAIM_CASHBACK_DISC`, `CLAIM_CASHBACK_V2_DISC` (with doc-comments).

> This makes `BUY_DISCRIMINATOR` / `SELL_DISCRIMINATOR` visible to `pump-trader` for the first time — enabling the de-dup in Task 6.

---

## Task 5 — `pump-constants/src/math.rs`

Create the file. Copy the **consts only** (not the `total_supply_for` fn) from [backend/src/config/constants/token_math.rs:10-21](../backend/src/config/constants/token_math.rs#L10): `INITIAL_VIRTUAL_TOKEN_RESERVES`, `INITIAL_VIRTUAL_SOL_RESERVES`, `INITIAL_REAL_TOKEN_RESERVES`, `TOKEN_TOTAL_SUPPLY`, `PUMPFUN_GENESIS_PRICE_PER_RAW_TOKEN`.
Also move `LAMPORTS_PER_SOL` here from [pump-trader/src/constants.rs:12](../pump-trader/src/constants.rs#L12).

Fix the genesis-price expression to use the local `LAMPORTS_PER_SOL` (drop the `pump_trader::constants::` path):
```rust
pub const LAMPORTS_PER_SOL: u64 = 1_000_000_000;
// ...
pub const PUMPFUN_GENESIS_PRICE_PER_RAW_TOKEN: f64 =
    INITIAL_VIRTUAL_SOL_RESERVES / LAMPORTS_PER_SOL as f64 / INITIAL_VIRTUAL_TOKEN_RESERVES;
```

**Verify (crate now self-contained):** `cargo check -p pump-constants`

---

## Task 6 — Rewire `pump-trader`, de-dup discriminators

1. `pump-trader/Cargo.toml` — add under `[dependencies]`:
   ```toml
   pump-constants = { path = "../pump-constants" }
   ```
2. [pump-trader/src/constants.rs](../pump-trader/src/constants.rs) — **delete** the moved blocks (lines 9-24 IDs, 32-40 cashback discs, 42-80 fees, and `LAMPORTS_PER_SOL` at line 12). At the top of the file add a re-export so every existing `crate::constants::NAME` still resolves:
   ```rust
   //! Trader behaviour/tuning constants. Static on-chain facts are re-exported
   //! from `pump_constants` (the workspace-wide single source of truth).
   pub use pump_constants::{
       ASSOCIATED_TOKEN_PROGRAM_ID, EVENT_AUTHORITY, FEE_PROGRAM_ID, LAMPORTS_PER_SOL,
       PUMP_FUN_PROGRAM_ID, PUMP_SWAP_PROGRAM_ID, TOKEN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID,
       WSOL_MINT, SYNC_UVA_DISC, CLAIM_CASHBACK_DISC, CLAIM_CASHBACK_V2_DISC,
       PUMP_AMM_CASHBACK_GLOBAL, PUMP_CURVE_FEE_RECIPIENT, PUMP_AMM_BUYBACK_FEE_RECIPIENT,
       JITO_TIP_ACCOUNTS, BUY_DISCRIMINATOR, SELL_DISCRIMINATOR,
   };
   ```
   Keep ALL tuning consts below (`BUY_SEED_POOL_SIZE`, `MAX_BUY_SOL`, `JITO_TIP_*`, `COMPUTE_UNIT_*`, `MAX_SELL_ATTEMPTS`, `CONFIRM_*`, `NONCE_*`, `RESERVE_CACHE_MAX_AGE_MS`, `BLOCKHASH_*`, `AMM_DEFAULT_SLIPPAGE_BPS`, `CURVE_FEE_BUFFER_BPS`, `TOKEN_ACCOUNT_*`, the `AMM_POOL_*`/`AMM_CONFIG_*` offsets) untouched.
3. **De-dup amm.rs** — [pump-trader/src/trader/amm.rs:51-52](../pump-trader/src/trader/amm.rs#L51): delete the two `const BUY_DISC`/`const SELL_DISC` lines; replace with a `use` alias at module scope (near the other `use`s):
   ```rust
   use pump_constants::{BUY_DISCRIMINATOR as BUY_DISC, SELL_DISCRIMINATOR as SELL_DISC};
   ```
   The 6 usage sites (lines ~297, 369, 836, 1000, 1023) keep working unchanged.
   - **If** the test-module import at [amm.rs:896](../pump-trader/src/trader/amm.rs#L896) (`use super::{… BUY_DISC, SELL_DISC}`) fails to resolve, remove `BUY_DISC, SELL_DISC` from that `super::` list and add inside the test module: `use pump_constants::{BUY_DISCRIMINATOR as BUY_DISC, SELL_DISCRIMINATOR as SELL_DISC};`
4. **De-dup sell.rs** — [pump-trader/src/trader/sell.rs:425](../pump-trader/src/trader/sell.rs#L425): replace the inline array with the shared const:
   ```rust
   sell_data.extend_from_slice(&crate::constants::SELL_DISCRIMINATOR);
   ```

**Verify:** `cargo check -p pump-trader` and `cargo test -p pump-trader` (the amm.rs disc test must pass — it asserts the values).

---

## Task 7 — Rewire `backend`

1. `backend/Cargo.toml` — add under `[dependencies]` (keep alphabetical-ish near `pump-trader`):
   ```toml
   pump-constants = { path = "../pump-constants" }
   ```
2. [backend/src/config/constants/protocol.rs](../backend/src/config/constants/protocol.rs) — delete all the `pub const` ID definitions (lines 13-30) and the `pub use pump_trader::constants::{…}` block (lines 7-10). Replace with a single re-export, then keep `program_friendly_name` as-is below it:
   ```rust
   pub use pump_constants::{
       ADDRESS_LOOKUP_TABLE_PROGRAM_ID, ARBITRAGE_BOT_9ZZF9_ID, ARBITRAGE_BOT_FADO9_ID,
       ASSOCIATED_TOKEN_PROGRAM_ID, AXIOM_TRADE_PROGRAM_ID, BLOOM_ROUTER_PROGRAM_ID,
       COMPUTE_BUDGET_PROGRAM_ID, DFLOW_AGGREGATOR_V4_PROGRAM_ID, EVENT_AUTHORITY,
       FEE_PROGRAM_ID, GMGN_BOT_PROGRAM_ID, JUPITER_AGGREGATOR_V6_PROGRAM_ID, LAMPORTS_PER_SOL,
       METEORA_DAMM_V2_PROGRAM_ID, PHOTON_PROGRAM_ID, PUMP_FUN_PROGRAM_ID, PUMP_SWAP_PROGRAM_ID,
       SYSTEM_PROGRAM_ID, TERMINAL_FORMERLY_PADRE_PROGRAM_ID, TOKEN_2022_PROGRAM_ID,
       TOKEN_PROGRAM_ID, TROJAN_TRADE_PROGRAM_ID, WSOL_MINT,
   };
   ```
   (`program_friendly_name` still compiles — it references the now re-exported IDs.)
3. [backend/src/config/constants/discriminators.rs](../backend/src/config/constants/discriminators.rs) — replace the entire file body with:
   ```rust
   //! Re-exported from `pump_constants` (workspace single source of truth).
   pub use pump_constants::{
       ANCHOR_EVENT_CPI_DISCRIMINATOR, BUY_DISCRIMINATOR, BUY_EXACT_QUOTE_IN_DISCRIMINATOR,
       BUY_EXACT_QUOTE_IN_V2_DISCRIMINATOR, BUY_EXACT_SOL_IN_DISCRIMINATOR, BUY_V2_DISCRIMINATOR,
       CREATE_EVENT_DISCRIMINATOR, CREATE_INSTRUCTION_DISCRIMINATOR, CREATE_V2_INSTRUCTION_DISCRIMINATOR,
       MIGRATE_INSTRUCTION_DISCRIMINATOR, MIGRATE_V2_INSTRUCTION_DISCRIMINATOR,
       PUMP_COLLECT_CREATOR_FEE_DISCRIMINATOR, PUMP_EXTEND_ACCOUNT_DISCRIMINATOR,
       PUMP_INITIALIZE_DISCRIMINATOR, PUMP_SET_PARAMS_DISCRIMINATOR, PUMP_WITHDRAW_DISCRIMINATOR,
       PUMP_SWAP_BUY_EVENT_DISCRIMINATOR, PUMP_SWAP_CREATE_POOL_DISCRIMINATOR,
       PUMP_SWAP_DEPOSIT_DISCRIMINATOR, PUMP_SWAP_DISABLE_DISCRIMINATOR,
       PUMP_SWAP_SELL_EVENT_DISCRIMINATOR, PUMP_SWAP_UPDATE_ADMIN_DISCRIMINATOR,
       PUMP_SWAP_UPDATE_FEE_CONFIG_DISCRIMINATOR, SELL_DISCRIMINATOR, TRADE_EVENT_DISCRIMINATOR,
   };
   ```
4. [backend/src/config/constants/token_math.rs](../backend/src/config/constants/token_math.rs) — delete the moved consts (lines 10-21) and the `use pump_trader::constants::LAMPORTS_PER_SOL;` line. Replace top with a re-export; keep `total_supply_for`:
   ```rust
   pub use pump_constants::{
       INITIAL_REAL_TOKEN_RESERVES, INITIAL_VIRTUAL_SOL_RESERVES, INITIAL_VIRTUAL_TOKEN_RESERVES,
       PUMPFUN_GENESIS_PRICE_PER_RAW_TOKEN, TOKEN_TOTAL_SUPPLY,
   };

   pub fn total_supply_for(is_mayhem_mode: bool) -> f64 { /* unchanged */ }
   ```
5. `mod.rs` unchanged — its `pub use protocol::*` / `discriminators::*` / `token_math::*` now re-export the crate symbols transparently.

**Verify:** `cargo check --bin backend` and `cargo test --bin backend`.

---

## Task 8 — Sweep for stragglers + final verification

1. Grep for any remaining direct definitions that should now be re-exports only:
   - `Grep "pub const PUMP_FUN_PROGRAM_ID|pub const SELL_DISCRIMINATOR|pub const LAMPORTS_PER_SOL"` — should match **only** files in `pump-constants/`.
   - `Grep "0x33, 0xe6, 0x85, 0xa4"` and `"const BUY_DISC|const SELL_DISC"` — **zero** hits outside `pump-constants/`.
2. Full workspace build + clippy on touched crates:
   ```powershell
   cargo check --workspace            # or per-crate with --target-dir target-check if exe is running
   cargo clippy -p pump-constants -p pump-trader --bin backend
   cargo test --bin backend; cargo test -p pump-trader
   ```

**Acceptance:** all three checks clean; no new warnings; the only changed *values* are zero (pure move); buy/sell discriminators now defined exactly once.

---

## Task 9 — (OPTIONAL, Phase 2) move account-layout offsets

Only if you want the layout facts centralized too. Move `AMM_POOL_*`, `AMM_CONFIG_*`,
`TOKEN_ACCOUNT_SPACE`, `TOKEN_2022_ACCOUNT_SPACE` from `pump-trader/src/constants.rs`
into a new `pump-constants/src/layout.rs` (+ `pub mod layout; pub use layout::*;` in
lib.rs), and re-export them from `pump-trader/src/constants.rs` alongside the Task-6
block. Leave the fn-local offsets in `query.rs`/`claim.rs` unless you want those too.
Verify with `cargo check -p pump-trader`.

---

## Docs to update on completion (per CLAUDE.md "Definition of done")

- **[@arch/architecture.md](../arch/architecture.md)** + **[@arch/trade-execution.md](../arch/trade-execution.md)**: note the new `pump-constants` crate and the `backend → pump-trader → pump-constants` graph; constants are now defined there and re-exported.
- **CLAUDE.md** "Architecture" table/intro: mention `pump-constants` as the source of truth for static on-chain facts.
- No `.env` impact.
