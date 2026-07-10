# Decision record (ADR)

Resolutions for the foundation plan's §9 *Open decisions*
([`../../and-about-the-instructions-shimmying-shore.md`](../../and-about-the-instructions-shimmying-shore.md)).
Each entry: **status**, the **choice**, the **rationale**, and **where it lives** in
the code so the decision can't silently drift.

Status legend: **LOCKED** (implemented, guarded) · **CHOSEN** (decided, builds in a
later phase) · **DEFERRED** (intentionally not now; recorded so it isn't forgotten).

---

## D1 — Amount naming — **LOCKED**

**Choice:** asset-referenced `*_quote` / `*_base` base-unit columns everywhere.
The dual-vocab alternative (`*_lamports` for native-SOL rows + `*_quote` only for
non-native) is **rejected** — it re-introduces the exact hard-coded-lamport
assumption this redesign exists to remove.

**Rationale:** one unit vocabulary end-to-end. Native SOL is *just* the
`quote_assets` row with `is_native = true, decimals = 9`; a lamport is that row's
base unit, not a special case in the schema. "Store integer base units, display
÷10^decimals" is preserved — the unit is now the *referenced* quote/base asset.

**Where it lives:** `trades.amount_quote` / `amount_base` / `reserve_quote` /
`reserve_base`; `tokens.initial_supply_base` / `initial_buy_quote`;
`launches.dev_buy_quote`, `bundles.tip_quote`; models mirror these
(`models/trade.rs`, `models/own_launch.rs`). Prices are raw ratios
(`amount_quote / amount_base`), decimals + USD applied only in the `trades_priced`
/ `token_overview` views. Naming rule documented in `migrations/0001_init.sql`
header (lines 15–24).

---

## D2 — `markets` dimension vs. denormalized-only — **LOCKED (hybrid)**

**Choice:** keep the `markets` dimension table for per-market metadata **and**
denormalize `launchpad_id` / `market_kind` / `quote_asset_id` onto `trades` for the
hot read. (Not "denormalized-only", not "dimension-only".)

**Rationale:** the dimension carries what a hot row shouldn't (`program_id`,
`pool_address`, `created_slot`) and anchors the per-`(mint, market)` ingest
watermark (`token_sync_state.market_id` FKs it), so a token graduating curve→amm is
a new **row**, never a column flip. The three denormalized ids keep the hot trade
read join-free (performance budget — no dimension join on the ingest/query path).

**Where it lives:** `markets` + `token_sync_state` in `0001_init.sql`; the three
denormalized ids on `trades` (with `CHECK` on `market_kind`).

---

## D3 — Managed-wallet keystore backend — **CHOSEN** (builds in the launcher, phase 2)

**Choice:** **envelope-encrypted file + a pluggable KEK trait.** `key_ref` is a blob
id/path to an AES-GCM-encrypted ed25519 secret whose data-encryption key is wrapped
by a KEK. The KEK source is a trait — **env/passphrase now** (no infra, works
headless on EC2, honours hunter's "no new infra spend"), swappable to **AWS
KMS** (unwrap via the instance IAM role) for production with no schema or call-site
change.

**Rationale:** AWS KMS cannot *sign* Solana's ed25519 (KMS supports only RSA/ECC key
types), so in every candidate backend the ed25519 secret is decrypted into memory
and signs in-process — the real question is only *where the wrapping key / secret
lives at rest*. An envelope file with a pluggable KEK gives the best at-rest story
that still runs headless on EC2 today, and defers the AWS dependency until prod
actually wants it. OS keyring was rejected as the primary: no interactive unlock on
headless EC2 would force a second backend anyway.

**Seam to build (phase 2, `launcher`, live-only):**

```text
key_ref: "wallets/dev-01.enc"          // stored in managed_wallets.key_ref

trait Kek { fn unwrap(&self, wrapped_dek: &[u8]) -> Zeroizing<Vec<u8>>; }
  EnvKek     // KEK from env/passphrase  (default now)
  KmsKek     // AWS KMS Decrypt via IAM  (prod later)

resolve(key_ref) -> Arc<dyn Signer>:
    blob   = read(key_ref)                       // {wrapped_dek, nonce, ciphertext}
    dek    = kek.unwrap(blob.wrapped_dek)
    secret = aes_gcm_decrypt(dek, blob.nonce, blob.ciphertext)   // zeroized on drop
    Keypair::from(secret)                         // pump-trader Arc<dyn Signer>
```

**Already enforced:** no secret bytes in Postgres — `managed_wallets.key_ref` is a
reference only (`0002_own_launch.sql`), and `ManagedWallet.key_ref` is
`#[serde(skip_serializing)]` so it never leaves the process over the API.

---

## D4 — USD rate source for non-SOL quotes — **CHOSEN**

**Choice:** **USDC pinned to `1.0`** (seeded in `quote_assets`); **SOL** from a
price poller carried over from hunter, writing `quote_assets.usd_rate` /
`usd_rate_at`. A live oracle is **deferred** until a quote that is neither SOL nor a
USD-stable actually needs one.

**Rationale:** USD is only the cross-quote numeraire for comparison, and is
**derived in views, never stored** (`trades_priced.amount_usd`,
`token_overview.price_usd` / `market_cap_usd`). Pinning USDC and polling SOL covers
every quote the platform has today at zero new infra; the `usd_rate` column is the
single seam a future oracle writes to.

**Where it lives:** `quote_assets.usd_rate` / `usd_rate_at` (USDC seeded `1.0`, SOL
`NULL` until the poller runs); USD math only in the two views. **TODO (phase 2,
`live`):** port the SOL price poller to update the SOL row on a cadence.

---

## D6 — create_v2 + dev-buy tx size: v0 + Address Lookup Table — **BUILT**

**Problem:** a `create_v2` + dev-buy is one transaction naming ~27 accounts and
serializes to **1267 B**, over Solana's hard **1232 B** limit. The Helius `/fast`
sender rejects it as the opaque `base64 encoded too large`, and the launch dies at
create (`create_signature` NULL). `create_v2` overflows where `create_v1` fit
because it always carries the 5 Mayhem accounts (structurally required regardless
of `is_mayhem_mode`).

**Choice:** compile the create tx as a **v0 (versioned) message** referencing a
persistent on-chain **Address Lookup Table** of the ~15 *immutable* accounts
(program IDs, constant-seed PDAs, the 10 Jito tip accounts — SSOT
`pump_trader::launch_alt_addresses`). Those collapse from 32-byte keys to 1-byte
indexes, dropping the tx to **931 B** (measured; 301 B headroom). The ALT is
provisioned once (`forge-live -- create-alt <authority_key_ref>`, spends ~0.001
SOL) and referenced via `PUMP_LAUNCH_ALT`.

**Rejected:** (a) splitting the dev-buy into a second tx — loses create+buy
atomicity; (b) keeping the fee_recipient / fee_config in the ALT — their values are
governance-rotatable, so a stale table could misroute silently; only fixed-address
accounts go in the ALT, the 2 global-derived ones stay inline. **Non-breaking:**
`launch_alt_address` is `Option` on `TraderConfig`; unset → the legacy
single-message path (what hunter uses) is unchanged. A **fail-fast 1232 B guard**
now runs in the send path so any future overflow surfaces locally with an
actionable message instead of the sender's cryptic error.

**Also covers Jito bundle legs.** Each `build_bundle_leg_tx` leg is now a v0 tx over
the same ALT (empty-lookup fallback when unset). The v2 bundle-buy leg's ~27-account
buy sat at **1141 B** — 91 B under the ceiling, one longer disguise from failing — and
drops to **867 B** with the ALT. `submit_jito_bundle` guards each leg at 1232 B before
submit (a single over-limit leg fails the whole bundle). Measured by
`bundle_v2_leg_v0_alt_fits_and_beats_legacy`.

---

## D5 — Wallet-funding obfuscation — **PARTIALLY BUILT** (hop graph still deferred)

**Choice:** per-leg instruction variation (§3e) defeats naive "identical-tx"
fingerprinting but **not** funding-graph/timing analysis (N fresh wallets funded
from one source, same mint, same slot). The full automated hop-graph fan-out is
still not built — manual funding is the deliberate choice for now (see
`docs/wallet-pool-plan.md` Phase 5+).

**What `managed_wallets` records today:** `funding_source` (free-text audit note)
landed in the wallet-pool Phase 1 migration (`0004_wallet_pool.sql`), alongside the
full `status` lifecycle (`generated`/`funded`/`reserved`/`used`/`retired`) that
replaces the old `is_active` flag. The **hop graph** (which wallet funded which,
when) is still **deferred** — `docs/wallet-pool-plan.md` Phase 5+, "Automated
multi-hop funding fan-out."

---

## Verification status (plan §8)

| Check | Status |
| --- | --- |
| Dep partition — `cargo tree -p live` has no duckdb/arrow/parquet | **PASS** (rayon present, but as a Solana transitive via pump-trader, not the `lake` crate) |
| Dep partition — `cargo tree -p lab` has no pump-trader/ingest-laserstream/tonic | **PASS** |
| Workspace `cargo check --workspace` | **PASS** (exit 0, all 6 crates; only pre-existing future-incompat warnings from `solana-client`/`sqlx-core` deps, not this code) |
| Migrations apply clean on fresh PG+Timescale | **PASS in Phase 4** via the generality test's boot path (it runs migrations + cagg setup); standalone `sqlx migrate run` re-run pending Docker |
| Generality proof — mock USDC + SOL token, one `trades`/views handles both | **PASS in Phase 4** (`crates/platform-core/tests/generality.rs`); re-run this pass blocked only by Docker Desktop being stopped |
| Ingest round-trip | **PASS in Phase 6** with *synthetic* pump.fun events → schema; **live-feed** round-trip (real Helius gRPC + keys, `live` box) still **DEFERRED** |
| Lake parity (sealed-day Parquet vs PG) | **DEFERRED** — `lake` crate not yet implemented |

The core proofs already passed during the build (Phases 4/6). To **reconfirm** on a
fresh DB this pass: `docker compose up -d` → set `PLATFORM_TEST_DATABASE_URL` to a
dedicated throwaway DB → `cargo test -p platform-core` (the generality test runs its
own migrations + cagg setup, and self-skips if the env var is unset).
