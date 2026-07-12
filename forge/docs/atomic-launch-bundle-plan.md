# Atomic launch bundle — fix "create lands, bundler buys drop"

**Status:** IMPLEMENTED 2026-07-12 (code-complete, cargo check + unit tests + clippy
green; real-SOL mainnet launch is the remaining operator gate). Mint-key fork =
**Option A** (encrypt & persist). Owner: launcher + shared executor.
**Root-cause evidence:** local `forge_bot` DB, launch `5b039b47…` / bundle `520aadfb…`
(2026-07-12). See "Evidence" below.

## Implemented (deviations from the plan below)

- **Shared executor** (`shared/executor`): `Engine::build_v0_tx_with_blockhash_multi`
  (send.rs); `PumpFunTrader::build_create_leg_tx` / `build_create_v2_leg_tx` — unsent
  signed v0 create(+dev-buy) leg on a shared blockhash, tip level threaded through
  `assemble_create_ixs`; `price::apply_curve_buy` + `simulate_launch_leg_reserves`;
  `build_bundle_leg_tx` gained a `reserves_override`. Additive — hunter unaffected.
- **Keystore**: `write_mint_key` / `delete_mint_key` / `mint_key_ref(launch_id)`
  (deterministic ref, no DB column).
- **DB**: migration `0005` adds `bundles.create_args` JSONB (the create leg's rebuild
  inputs); `BundleRepo::insert(+create_args)` / `set_tip_quote`;
  `LaunchRepo::set_create_signature`; `ManagedWalletRepo::release_reservation`.
- **`bundle_execute`**: one `execute_bundle_inner` builds create leg (tx0) + co-buy
  legs (simulated reserves) on one blockhash → `sendBundle`; trader stands up from the
  DEV wallet; whole-bundle tip rides the create leg, co-buys keep persona jitter;
  `tip_quote` persisted; create sig stamped on the launch (status stays `pending`).
- **`service.rs`**: bundler launches DEFER create into the bundle (launch stays
  `pending`); bundler-**less** launches keep the legacy create-then-confirm path
  (no drop risk). Empty bundler claim on an intended-atomic launch FAILS the launch
  (create is inside the bundle) + releases the dev wallet. Auto-submit failure cleans
  up the mint key + fails the launch.
- **`confirm.rs`**: landed → launch `created` + seed dev/bundler positions + mark
  wallets `used` + delete mint key; terminal drop → launch `failed` + **release** dev +
  bundler wallets (a dropped atomic bundle spent nothing) + delete mint key; re-bid
  rebuilds the WHOLE bundle at the escalated tip.
- **Config**: env-tunable `JITO_MIN_TIP_SOL` / `JITO_MAX_TIP_SOL` (default 0.01, up
  from 0.005) / `JITO_TIP_PERCENTILE`, applied in `build_launch_trader_config`.

**Known minor gaps (acceptable):** a standalone re-bid rebuilds the create leg with the
canonical ix layout (the authored `create_layout` isn't persisted — only the auto-submit
in the same request has it); the token row + market_state are inserted optimistically at
submit (a fully-dropped launch leaves a `failed`-launch token row, filterable). Both
noted for a follow-up if they matter in practice.

## Problem

A launch reliably creates the token but the **bundler buys drop** — the exact
recurring symptom. The create+dev-buy and the bundler buys are **two separate
submissions**:

1. [`service.rs`](../launcher/src/service.rs) sends create+dev-buy via Helius `/fast`
   and **waits for confirmation** (`create_token_v2(&mint, &args, /*confirm*/ true)`).
2. Only *after* it confirms does it plan + `sendBundle` the bundler buys to Jito
   ([`bundle_execute.rs`](../launcher/src/bundle_execute.rs)).

That split is fatal on a contested launch:

- **Front-run gap.** Confirmation + planning is seconds long. A sniper lands in it.
- **The bundler bundle races Jito's auction alone**, with no create anchoring it.
- **Slippage reverts.** By the time the bundle is auctioned (and especially on the
  90 s-later re-bids), the dev-buy + snipers have moved the curve past the legs' 5 %
  slippage → the buys **revert** → the bundle is un-landable *at any tip*. The re-bid
  loop only escalates the tip, which can't rescue a reverting tx.

### Evidence (local DB)

Launch `5b039b47…`, mint `Hxix…GGjL`, status `created`. Bundle `520aadfb…`,
`status=dropped`, `submit_attempts=3` (exhausted `BUNDLE_MAX_RETRIES=2`), 2 legs of
0.01 SOL, per-leg tips 0.00128 + 0.00100 ≈ **0.0023 SOL total**. On-chain `trades`:

| slot | type | amount | wallet |
|---|---|---|---|
| 432329065 | buy | 0.0198 SOL | dev `Cb1j…` (our create+dev-buy) — **landed** |
| 432329067 (+2 slots ≈ 1 s) | buy | 0.005 SOL | **external sniper** `Grj5…` |
| 432329294 | sell | 0.005 SOL | sniper dumps |

Our two bundler wallets never appear on-chain. Tip was adequate; the buys never
landed — front-run + revert, not an auction-price problem.

## Target architecture

Submit the **whole launch as ONE atomic Jito bundle**:

```
sendBundle([ tx0 = create(+dev-buy), tx1 = bundler-buy, tx2 = bundler-buy, … ])
```

- All txs share one blockhash; Jito bundles are atomic → all land in one slot or none.
- No front-run window; bundler buys land at the curve bottom, right behind the dev buy.
- Slippage is deterministic (only our own preceding legs move the curve — simulated,
  not read from chain).

This inverts the launch lifecycle: **create is no longer independently "created"** —
the launch becomes `created` only when the atomic bundle lands. A dropped bundle means
nothing was created; a re-bid re-submits the *entire* bundle (create included) at a
higher tip; terminal drop ⇒ launch `failed`, wallets released, mint discarded.

## Design decisions (need sign-off — see DECISION FORK)

### Mint keypair persistence (**FORK — pick one**)

Create must be re-signable on re-bid, but the mint secret is currently ephemeral
(`Keypair::new()`, dropped when the request returns).

- **Option A — persist encrypted mint key (recommended).** Envelope-encrypt the mint
  keypair into the keystore at plan time (same `EnvKek` path wallets use), keyed by
  launch id; `execute_bundle` loads it to sign the create leg; delete on terminal
  outcome. Keeps the existing **async** confirm-watcher re-bid. Cost: one new keystore
  blob per in-flight launch + a delete on completion.
- **Option B — new mint per attempt.** On drop, generate a fresh mint for the retry.
  No secret persistence, but the launch's `mint_address` changes per attempt (churns
  the token row, metadata, any pre-shared address) — rejected unless A is unacceptable.
- **Option C — synchronous re-bid.** Keep the mint keypair in memory and run the whole
  submit→confirm→re-bid loop inside the launch HTTP request. No secret at rest, but
  blocks the request up to ~90 s × retries and duplicates the watcher's confirm logic.

Recommendation: **A**.

## Work plan

### Phase 1 — shared executor (`shared/executor/pumpfun`), additive only
Do not change any signature hunter consumes; add new methods.

1. **`build_create_leg_tx`** (new, in `trader/create.rs`): same instruction assembly as
   `create_token_*_inner` but returns an **unsent signed `VersionedTransaction`**
   against a **caller-supplied blockhash** (mirrors `build_bundle_leg_tx`), signed by
   dev + mint. Factor the shared ix-assembly out of `create_token_*_inner` so the
   send-path and the bundle-leg-path share one builder (SSOT).
2. **Simulated-reserve bundler legs.** `build_bundle_leg_tx` currently reads live
   `curve_reserves` — impossible when the curve is created in the same bundle. Add a
   variant (or an `Option<(u128,u128)> reserves_override`) so the caller passes the
   **simulated pre-leg reserves**. Add a pure `apply_curve_buy(reserves, net_lamports)
   -> (new_reserves, tokens_out)` helper next to `curve_buy_min_out` (constant-product,
   saturating) and a `simulate_launch_reserves` sequence: fresh → after dev-buy → after
   each bundler leg. min_out per leg is `curve_buy_min_out` over that leg's pre-state.
3. Unit tests: golden create-leg bytes == existing send-path ixs; reserve simulation
   monotonic (each leg faces higher vq, gets fewer tokens); atomic bundle wire sizes
   (create + N legs) each ≤ 1232 B with the ALT.

### Phase 2 — launcher (`forge/launcher`)
4. **Atomic submit path** (`bundle_execute.rs`): build create leg + all bundler legs on
   one blockhash, `submit_jito_bundle([create, legs…])`. Persist `create_signature`
   from the create leg into the bundle's `leg_signatures` (or a dedicated column) so the
   watcher can confirm it.
5. **Launch service** (`service.rs`): stop `create_token_*(confirm=true)`. Instead:
   generate mint, persist encrypted mint key (Option A), plan the bundle (create + dev
   + bundlers), insert launch as `pending`, submit the atomic bundle. `launch.status`
   stays `pending` until the watcher confirms landing.
6. **Confirm watcher** (`confirm.rs`): on all-legs-landed, set `launch.status=created`,
   `set_created(create_signature)`, seed dev + bundler positions. On drop within
   retries, re-bid the **whole** bundle (fresh blockhash, `submit_attempts`-driven tip,
   reload mint key). On terminal drop: `launch.status=failed`, release wallets, delete
   the encrypted mint key. Confirmation set = create sig (its dev-buy trade carries it)
   + bundler leg sigs; for a no-dev-buy launch, confirm create via the ingested
   create/`raw_txs` row rather than `trades`.
7. **Persist the tip** actually used into `bundles.tip_quote` (today it's always NULL —
   the value only lives in `legs[].structure.tip_quote`). Minor, but it's the column the
   UI/audit reads.

### Phase 3 — config + safety
8. Raise the tip ceiling: `JitoTipCfg.max_sol` default 0.005 → make it env-tunable
   (`JITO_MAX_TIP_SOL`) and set a launch-appropriate default; the escalation base is
   otherwise capped too low for a hot launch. Keep the `[min,max]` cost guardrail.
9. Wire-size guard already exists (`MAX_TX_WIRE_BYTES`); extend it to the create leg.

### Phase 4 — tests / verify
10. `cargo check -p forge-live` + touched-crate clippy; unit tests from 3 & the reserve
    sim. Dep partition unchanged (additive).
11. Zero-SOL verification per [[zero-sol-trade-verification]]: dry-run assembles the
    atomic bundle and asserts wire sizes + leg count + that create is tx0, without
    submitting. Real-SOL launch is the final gate (operator-run).

## Risks / rollout

- **Lifecycle inversion is the risky part** — the frontend + `GET /api/launches` treat a
  launch as `created` immediately today. `pending`-until-landed changes what the UI
  shows for ~1 slot–90 s. Confirm the launches page tolerates a `pending→created`
  transition (it already renders `pending`).
- Mint-key-at-rest (Option A) is a new secret category in the keystore; it must be
  deleted on every terminal path (landed cleanup + failed + retries-exhausted) or it
  leaks blobs. Guard with the reservation-TTL sweep as a backstop.
- No dev-buy edge case (create-only + bundlers): confirm create via `raw_txs`, not
  `trades`.

## Out of scope

Multi-block / multi-bundle launches, changing the ALT contents, and the dynamic ix
layout work (shipped separately).
