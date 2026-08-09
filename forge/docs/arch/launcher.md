# Launcher — `forge-launcher` crate

File-level map of `forge/launcher/src/` (crate name `forge-launcher`, lib name `launcher`, dir `forge/launcher/`). LIVE-only — never a lab dep.

The launcher is forge's write orchestrator: it turns a stored `launch_templates` row + a dev wallet into a pump.fun token (create `v1`/`v2` + fused dev-buy), composes N bundler co-buy legs into ONE atomic Jito bundle, submits it (multi-region race), confirms landing off the ingested `trades` feed, and runs the surrounding wallet-pool machinery (generate → fund → claim → sweep/retire), operator wallet↔wallet transfers, keystore, backups, and post-launch token management (sell/buy/consolidate/ladder/volume).

**Execution layer.** Every on-chain build/sign/send goes through the shared executor stack, imported as `pump_trader` (the lib name kept for back-compat) + `executor_core`:
- `pump-trader` dep key → package `executor-pumpfun` = `shared/executor/pumpfun` (the pump.fun venue: catalog, discriminators, ix builders, curve/AMM pricing, `PumpFunTrader`).
- `executor-core` = `shared/executor/core` (venue-agnostic engine: sign / fan-out send / feed-confirm / Jito tip / retry / sim + the `Venue` trait).
- The launcher assembles a `forge-orchestrator` (`orchestrator`) `Plan` and passes the **mandatory audit gate** before any ix is built.

Signing is always an `Arc<dyn Signer>` resolved from the envelope-encrypted keystore (`keystore.rs`) — private keys never touch Postgres.

Deep-dives belong at `@plans/launcher/<topic>.md`.

## Launch flow modules — `src/`

| File | Responsibility |
| --- | --- |
| `lib.rs` | Crate facade — module decls + the public `pub use` surface (the live/CLI bins call these). |
| `service.rs` | **Launch orchestrator** `execute_launch(pool, settings, req)`. Loads template + metadata + dev wallet; resolves the dev signer; builds the launch `PumpFunTrader`; validates the template (`PumpfunTemplateParams::validate`); pre-launch dev-balance gate (`min_dev_launch_lamports` = variant-aware rent floor + tip ceiling, via `funding_plan` SSOT, typed `InsufficientDevBalance` → HTTP 400); inserts the `pending` launch; reserves the dev wallet (`claim_specific`). **Bundled path**: create+dev-buy is *not* sent standalone — it becomes tx0 of the atomic bundle (`bundle_phase`). **Bundler-less legacy path**: sends the standalone `create_token[_v2][_and_dev_buy]`, marks the dev wallet `used`, seeds the dev position. Owns two error handlers that release reservations + mark the launch `failed`. Holds `PumpfunTemplateParams` (template `params` brain) + `create_rent_and_fee_lamports`. |
| `bundle.rs` | Per-leg descriptor types + the `bundles.legs` JSON shape (`BundledLegPlan`/`LegStructure`/`LegStructureRecipe`/`BuyVariant`). `resolve_leg_count` (request override → template `bundle_leg_count`), `resolve_bundle_quote` (per-leg SOL + tip). `legs_to_json`/`legs_from_json`. The composed legs are a *display projection* of the gated `Plan`. |
| `plan_pipeline.rs` | **The mandatory gate** `gate(plan, allow_fingerprint) -> GatedPlan`: (1) catalog-validate (`orchestrator::prepare`), (2) draw sticky-persona disguises + apply the chosen encoding (locked/authored legs keep their variant, still get CU/tip jitter), (3) fail-closed ix-layout validation, (4) fingerprint audit (hard-reject never waved; tells gated by `allow_fingerprint`). `bundle_buy_variant` (catalog name → executor `BundleBuyVariant`), `is_bundler_leg`, `display_legs_json`. Consts `LAUNCH_BUY_VARIANT="buy_exact_sol_in"`, `DEFAULT_BUNDLE_SLIPPAGE_BPS=500`. |
| `plan_exec.rs` | **Executor bridge** — turns a gated `Plan`'s ops into real txs through `PumpFunTrader`. `build_leg_tx` (bundler buy → `trader.build_bundle_leg_tx`, v0/ALT), `leg_params` (op slippage + disguise CU/price/tip, floored to `min_tip`), `buy_lamports` (asserts `Amount::ExactQuote`), `execute_transfer` (the ONE plain-`system_instruction::transfer` SSOT for funding/consolidate/dust — `Exact` or `SweepAll{min}`). |
| `bundle_execute.rs` | **Atomic bundle build + Jito submit.** `execute_bundle` / `execute_bundle_with_trader` (reuse the launch's dev trader). Atomic CAS `planned → submitting` (`claim_for_submitting`); `build_atomic_bundle_txs` (SSOT: tx0 create+dev-buy via `build_create[_v2]_leg_tx`, then each co-buy leg on ONE shared blockhash; forward-simulates curve reserves for per-leg `min_out`); leader gate; `submit_jito_bundle` (races all `jito_block_engine_urls` in a `JoinSet`, first-accept wins, 1232 B wire check). Persists `CreateLegArgs` so a re-bid rebuilds tx0 without the launch request. |
| `confirm.rs` | **Feed-based landing confirmation** (`spawn_bundle_confirm_watcher`). Never calls the chain — checks whether every co-buy leg signature is present in the ingested `trades` table (`TradeRepo::find_signatures_present`). Woken by `trades_notify` (push) or a 10s fallback tick. Outcomes: `landed`/`dropped`/`partial` (anomaly). On timeout with nothing landed → auto **re-bid** (`rebid_dropped` → `reset_for_rebid` → `execute_bundle`) up to `bundle_max_retries`; retries exhausted / re-bid-can't-resubmit → `finalize_dropped`. Terminal handlers seed positions, transition wallets, delete the mint key, emit an `EventSink` push. |
| `jito_leader.rs` | **Leader-schedule gate** `wait_for_jito_leader(cfg, rpc, level)`. Jito bundles only land in a Jito-validator leader slot (dominant drop cause, *not* tip size). Derived SOL-free from RPC `getSlotLeaders` ∩ Jito StakeNet validator identity set (`getNextScheduledLeader` isn't on the public API). **Fail-open**: any error / disabled / empty set / spent budget → submit immediately. `level` (= `submit_attempts`) scales the wait budget (cap 4×). |
| `trader_config.rs` | `build_launch_trader_config` (Sender+RPC fan-out, long confirm window, `launch_alt`, `durable_nonce=false`, recent blockhash, launch Jito tip bounds) and `build_manage_trader_config` (plain RPC only, zeroed Jito tip, low CU price — manage is operator-timed, not a slot race). |
| `keystore.rs` | **Envelope-encrypted keystore** (AES-256-GCM, DEK wrapped by a pluggable `Kek`; `EnvKek` = SHA-256(passphrase)). `resolve_signer` → `Arc<dyn Signer>` (runtime SSOT); `resolve_secret_bytes` (export-only, raw material); `write_envelope[_to_keystore]`; mint-key persistence for re-bids (`write_mint_key`/`mint_key_ref`/`delete_mint_key`, deterministic `mints/{launch_id}.enc`); `token_program_for_variant`. |
| `config.rs` | `LauncherSettings::from_env` — RPC/sender URLs, nonce accounts, keystore dir, KEK passphrase, `jito_block_engine_urls` (parallel submit), `bundle_max_retries`, tip floor/ceiling/percentile, `LeaderGateConfig`, `launch_alt`, `backup_dir`, `pinata_jwt`, `FundingConfig` (kill switch `FUND_ENABLED`), `export_secret`, `ManageConfig` (kill switch `MANAGE_ENABLED`), `allow_fingerprint`. Set-but-malformed money-rail vars are fatal. `launch_tip_ceiling_lamports()`. |
| `funding_plan.rs` | **JIT-funding SSOT** — `dev_launch_required_lamports` (= `min_dev_launch_lamports` + dev-buy spend) is the SAME figure the pre-launch gate enforces, so funder target and gate can't drift. `leg_required_lamports`, `FUNDING_HEADROOM_LAMPORTS`. |
| `events.rs` | `EventSink` push seam + `LaunchStatusEvent` — background workers emit terminal transitions; `None` sink = no-op (CLI/tests). |
| `alt.rs` | CLI `create-alt` — provision the persistent launch Address Lookup Table (`PUMP_LAUNCH_ALT`) so create_v2+dev-buy compresses under 1232 B. |
| `bundle_simulate.rs` | CLI `bundle-simulate <id>` — read-only pre-flight; rebuilds the EXACT txs `execute_bundle` would submit (shared `build_atomic_bundle_txs`, `require_reservation=false`) and runs Jito `simulateBundle`. |
| `launch_sim_matrix.rs` | CLI `launch-sim-matrix` — zero-SOL Jito `simulateBundle` across every create × cashback × buy-variant combo to find working pairs. |
| `probe.rs` | CLI one-shot launch probe — balance check + create-only (no bundle), reuses the service floor. |
| `metadata_upload.rs` | Token-metadata authoring — pin image + off-chain JSON to Pinata/IPFS, persist a `metadata_templates` row (SSOT for token identity). |

## Wallet-pool modules — `src/`

| File | Responsibility |
| --- | --- |
| `wallet_pool.rs` | Pool lifecycle primitives. `generate_wallets` (fresh ed25519 → envelope-encrypt → insert `generated`, server-side, caller never sees keys); `sweep_reservations_once` (reservation + funding TTL sweep, 15m, DB-only); `refresh_all_balances` (operator on-demand "Refresh balances" — `getMultipleAccounts` batched 100/call → `record_balance` → promote `generated`/`funding` → `funded` at `MIN_FUNDED_LAMPORTS=0.001 SOL`); `fresh_cached_balance` (15s cache reuse). The claim/mark-used transitions themselves are SQL on `ManagedWalletRepo`. (The automatic `poll_balances_once` balance poll was **removed** — balance refresh + promotion are now operator-triggered, so the box makes no idle Helius RPC calls.) |
| `wallet_lifecycle.rs` | `spawn_wallet_lifecycle` — a single **DB-only safety timer** (60s): the reservation/funding TTL sweep (`sweep_reservations_once`), which releases wallets stranded `reserved`/`funding` by a crashed/aborted launch. There is deliberately no automatic balance poll, warm-pool funder, or hourly dust sweep — idle Helius RPC must stay at zero. No RPC, no SOL movement. |
| `wallet_funding.rs` | **Treasury → pool funding** (inverse of dust sweep), **operator-triggered only** ("Fund pool" button → `fund_once`). `FundingStrategy` seam (Tier-1 `DirectJittered`: one jittered transfer per wallet; Tier-2 multi-hop stubbed). Each send is **confirmed** and the wallet **promoted `funding` → `funded` in-place** (a post-fund `getMultipleAccounts` read-back), so one click leaves the pool claimable — no background poll. Plain `solana-client` transfer (no Jito). Guarded by `FundingConfig` reserve floor + per-interval cap + `FUNDING_LOCK` + `FUND_DRY_RUN`. (There is no autonomous background funder and no JIT `fund_for_launch` — funding is a click, never a daemon.) |
| `wallet_sweep.rs` | Operator-triggered reclaim passes (Wallet Pool buttons): `sweep_used_and_retired` + `consolidate_all`, sharing the plain-transfer SSOT + a token-account closer. Per-wallet `drain_to_treasury` is **failure-isolated**: close-accounts and SOL-sweep are independent steps (a close hiccup never strands the SOL sweep), each send has a bounded fresh-blockhash retry (`SEND_ATTEMPTS`), the cached balance is re-read for **every** wallet regardless of outcome (no stale/inflated snapshot after a partial pass), and a `WalletSweepOutcome.failed` flag distinguishes a genuine drain failure from a benign mid-launch skip so the UI can flag it + prompt a re-run. Both passes are idempotent — re-clicking retries only what's left. |
| `dust_sweep.rs` | `used`-wallet dust sweep home to treasury, then retire — **operator-triggered** via the "Sweep & retire" button (`sweep_used_wallets`) — there is no automatic hourly pass. Open-position guard (`TokenPositionRepo`) protects a wallet still holding tokens. Plain transfer; `SWEEP_MIN_LAMPORTS=0.0001 SOL`. |
| `wallet_transfer.rs` | Operator wallet↔wallet SOL transfer (dashboard, no Phantom/raw keys) — e.g. re-gas a wallet at 0 SOL. `TransferAmount` exact/max. |
| `wallet_encrypt.rs` | CLI — encrypt an external Solana keypair into the managed keystore. |
| `wallet_export.rs` | CLI — decrypt one wallet → base58 private key. The ONLY raw-material path (gated by `WALLET_EXPORT_SECRET`). |
| `wallet_verify.rs` | CLI — decrypt a blob + confirm the derived pubkey (restore-runbook check). |
| `backup.rs` | `run_backup` — copy the encrypted keystore + export `managed_wallets` after each generation batch (`WALLET_BACKUP_DIR`, opt-in). |

## Post-launch management — `src/manage/`

| File | Responsibility |
| --- | --- |
| `mod.rs` | Facade. Three primitives — Sell / Buy / Consolidate — over a wallet selection. |
| `model.rs` | Request + plan DTOs (`ManageRequest` → `ActionPlan` of `PlanLeg`, `WalletSelection`). |
| `plan.rs` | `build_plan` — turn a request into a previewable plan (pure DB reads, no chain). |
| `execute.rs` | `execute_action` — recompute the plan fresh, gate it, run legs via `PumpFunTrader` (manage trader config), insert the audit row. Gated by `MANAGE_ENABLED` (kill switch) + `MANAGE_DRY_RUN`. |
| `positions.rs` | Holdings read model — seed positions from launch/bundle fills, reconcile RPC balance + realized proceeds. |
| `ladder.rs` | Simple-threshold sell ladders (`arm_ladder`, `spawn_ladder_evaluator`, `LadderRung`). |
| `volume.rs` | Volume-making bots (`start_volume_bot`, `spawn_volume_scheduler`, `VolumeConfig`). |

## Architecture

```
execute_launch(template, dev_wallet)                            [service.rs]
  ├─ load template + metadata + dev wallet; resolve dev signer  [keystore]
  ├─ PumpfunTemplateParams::validate  (fail-closed, pre-spend)
  ├─ build launch PumpFunTrader (dev signer)                    [trader_config]
  ├─ pre-launch gate: balance ≥ dev_launch_required_lamports    [funding_plan SSOT]
  │      (= variant rent floor + tip ceiling + dev-buy)  → 400 InsufficientDevBalance
  ├─ INSERT launches(status=pending); claim_specific(dev)       [wallet reserved]
  │
  ├─ NO bundle ── send standalone create[_v2][_and_dev_buy] ──► set_created
  │                 mark dev used; seed dev position                (legacy path)
  │
  └─ WANTS bundle ─────────────────────────── bundle_phase ───────────────────────
        ├─ claim_funded(Bundler, N, launch)   FOR UPDATE SKIP LOCKED  (may be < N)
        ├─ orchestrator::bundle_launch → Plan (create tx0 + dev-buy + N co-buys)
        ├─ gate(plan, allow_fingerprint)      [plan_pipeline: catalog+disguise+audit]
        ├─ write_mint_key (envelope-enc, for re-bid)             [keystore]
        ├─ BundleRepo::insert(plan, audit, create_args); set_bundle_id
        └─ execute_bundle_with_trader ──────────────────────────────────────────────
              ├─ CAS planned→submitting (claim_for_submitting)
              ├─ build_atomic_bundle_txs  (tx0 create+dev-buy, then co-buys,        [bundle_execute]
              │     ONE blockhash; forward-sim reserves → per-leg min_out)
              ├─ wait_for_jito_leader(level = submit_attempts)   [jito_leader, fail-open]
              ├─ submit_jito_bundle → race all regions, first-accept wins
              └─ set_submitted(jito_id, leg_sigs); stamp create sig on launch
                      (launch stays PENDING — submit ≠ landing)

spawn_bundle_confirm_watcher (background)                       [confirm.rs]
  woken by trades_notify | 10s tick → for each submitted bundle:
     all co-buy sigs in `trades`? ─ yes → LANDED  → launch=created; seed dev+bundler
     │                                              positions; wallets → used; del mint key
     └ no, past 90s timeout & nothing landed:
           submit_attempts ≤ max_retries → RE-BID (reset_for_rebid → execute_bundle,
                                                    higher tip level, wallets stay reserved)
           else                          → DROPPED → launch=failed; release dev+bundler
                                                     wallets → funded; del mint key
```

Wallet-pool state machine (`managed_wallets.status`):

```
generate_wallets ─► generated ─(balance ≥ 0.001 SOL, poll)─► funded
       (funding pass sends: generated ─► funding ─► funded)      │
                                                                 │ claim_funded / claim_specific
                                                                 ▼  (FOR UPDATE SKIP LOCKED)
                                                              reserved ──► used ──(dust sweep)──► retired
                                                                 │  (bundle landed/partial: spent)
                                                                 └──(dropped / launch fail / TTL 15m)──► funded
```

## Key rules

- **Atomic launch**: for a bundled launch the create (+ fused dev-buy) is tx0 of the Jito bundle, never a separate submission — create + every co-buy land in one slot or none do (no front-run gap, no separate-submission drop). A bundler-less launch keeps the legacy create-then-confirm path.
- **`build_atomic_bundle_txs` is the on-wire SSOT** — `execute_bundle` submits exactly it and `bundle-simulate` simulates exactly it; a dry-run can't diverge from the real spend. Co-buy legs share tx0's blockhash and forward-simulate the freshly-created curve for each leg's `min_out`.
- **Every write passes the mandatory gate** (`plan_pipeline::gate`): catalog-validate → deterministic sticky-persona disguise (applied to the plan so audited == sent; authored/locked legs keep their variant but still get fee jitter) → fail-closed ix-layout check → fingerprint audit. Only the `Plan` is persisted; disguises re-derive identically on replay.
- **Launch buys are always `buy_exact_sol_in` (`ExactQuote`, SOL-in, min_out-floored)** — the overflow-prone tokens-out `ExactBase` encoding is never chosen for our own buys. Authored tokens-out `buy`/`buy_v2` legs are allowed (SOL budget as `max_sol_cost`) but require slippage for a real floor.
- **Drops are leader-slot timing, not tip.** The leader gate (`jito_leader`) delays submit until a Jito validator leads the upcoming slot; it is fail-open — never blocks a submit, only bounded-delays it. The whole-bundle Jito tip rides tx0 (create leg); co-buy legs carry only persona tip jitter (`min_tip=0`).
- **Tip sizing + re-bid ladder**: escalation `level = bundle.submit_attempts`. First attempt targets `JITO_TIP_PERCENTILE`; each confirm-watcher re-bid re-enters at a higher level (higher live-floor tip) and hunts a Jito leader harder, clamped to the `JITO_MAX_TIP_SOL` ceiling. The pre-launch dev-balance gate budgets that ceiling so a fully-escalated launch can't strand the dev wallet.
- **Submit is a CAS** (`claim_for_submitting`, `planned → submitting`) so two concurrent executes can't double-submit real-SOL buys; a re-bid `reset_for_rebid`s back to `planned` first.
- **Multi-region submit**: the same signed bundle is raced across all `JITO_BLOCK_ENGINE_URLS` in a `JoinSet`; first region to accept wins, the rest are aborted. Shared signatures ⇒ deduped on-chain, so racing can only land it once.
- **Confirmation trusts the feed, never the chain** — a leg is "landed" iff its signature is in the ingested `trades` table. Woken by `trades_notify` (push), 10s fallback tick is the timeout backstop. A true bundle is atomic, so `partial` is logged as an anomaly, not designed around.
- **Wallet claims are `FOR UPDATE SKIP LOCKED`** (`claim_funded` for the N bundler legs; `claim_specific` for the dev wallet) — concurrent launches never pick the same funded wallet. Bundler wallets are always a server-side atomic claim from the `funded` pool, never a client pick; a short pool plans fewer legs rather than erroring.
- **Reserve → used / released correctly**: a landed/partial bundle *spent*, so its wallets go `used` (dust-sweep eligible) and positions are seeded; a dropped/failed launch *spent nothing* (a dropped Jito bundle executes no txs), so its wallets are released back to `funded` and are immediately reusable. A stranded reservation is reclaimed by the 15m TTL sweep.
- **Mint key persistence**: the ephemeral mint keypair is envelope-encrypted at plan time (`mints/{launch_id}.enc`) so a re-bid can re-sign tx0's create leg with the same mint; deleted on any terminal outcome.
- **Signing = `Arc<dyn Signer>` from the keystore only.** Private keys are envelope-encrypted at rest (AES-256-GCM, wrapped DEK, pluggable `Kek`), decrypted in-process, never stored in Postgres. `resolve_secret_bytes` (raw export) is the one exception, gated behind `WALLET_EXPORT_SECRET`.
- **Plain SOL moves go through one primitive** (`plan_exec::execute_transfer`) shared by funding, consolidate, and dust sweep — no Jito (no landing urgency), but every move is a typed, auditable op.
- **Manage path is decoupled from launch**: operator-timed, plain RPC, zeroed Jito tip, low CU price; kill-switched by `MANAGE_ENABLED`. Funding is kill-switched by `FUND_ENABLED`; both revert-safe (`*_DRY_RUN`).
