# Executor Redesign Plan — `executor-core` / `executor-pumpfun` / `orchestrator`

> **Supersedes `monorepo-trader-plan.md` Part 2 entirely.** This is THE trader/executor
> redesign plan. Run it **after** the `monorepo-structure-plan.md` layout migration (hunter/
> forge rename) lands. Part 1 (the monorepo consolidation) is already done and green.

## Context — why this change

Today the on-chain execution stack is one 8,600-line god-object crate (`shared/pump-trader`,
struct `PumpFunTrader`) that fuses two unrelated concerns — a **venue-agnostic send engine**
(sign/send/confirm/nonce/tip/retry, ~2,000 lines) and **pump.fun venue specifics**
(buy/sell/amm/create/pricing/discriminators, ~5,000 lines). On top of it, SLP's launcher grew
three inconsistent, ad-hoc execution paths no one can hold in their head.

Four concrete defects (all verified in code):

1. **Variant chosen three inconsistent ways** — free-text string match
   (`launcher/src/service.rs:203`, `"pumpfun.create_v2"`/`"pumpfun.create_v1"`), an
   enum→string→enum round-trip (`launcher/src/bundle.rs:16` `BuyVariant` → persisted string →
   `pump_trader::BundleBuyVariant::parse` at `bundle_execute.rs:145`), and hardcoded dispatch
   (`launcher/src/manage/execute.rs:82`, `leg.side` string match, no variant knob at all).
2. **Dev-buy `min_out` computed against hardcoded initial reserves** —
   `shared/pump-trader/src/trader/create.rs:199-208` uses
   `protocol::INITIAL_VIRTUAL_{TOKEN,SOL}_RESERVES` instead of a live curve read. This is the
   structural cause of the on-chain launch `MathOverflow (6025)` / `ShareCalculationOverflow
   (6015)` error.
3. **Raw `system_instruction::transfer` bypasses the trader in three places** —
   `launcher/src/wallet_funding.rs:824`, `launcher/src/dust_sweep.rs:100/115`,
   `launcher/src/manage/execute.rs:341` (consolidate). No typed op, no plan, no audit.
4. **Equal-amount bundle legs** — `launcher/src/bundle.rs:104` gives every leg the same
   `quote_per_leg`, a self-inflicted bot fingerprint.

**Intended outcome:** collapse this into ONE uniform model of "a trade" and "a batch of
trades," structurally cure the overflow bug, make every on-chain variant first-class, give SLP
a real anti-fingerprint layer, and make "add a second launchpad" a localized addition — all
without a cathedral of crates.

**Prerequisite:** the `monorepo-structure-plan.md` folder/rename migration is done and green
(`meme-trading` → `hunter/`, `solana-launch-platform` → `forge/`). Behavior for hunter
(meme-trading) must stay **identical** throughout; forge (SLP) cuts over last.

---

## Decision: three crates, two layers (settled)

Neither existing plan cleanly separates the reusable engine from pump specifics: the old
trader-plan welds all 8,600 lines into one `trader` crate; the structure-plan would duplicate
the send engine across `trader-pump`/`trader-bonk`. This plan extracts the engine **now**,
because multi-launchpad (Bonk/Raydium/Meteora) is the stated direction.

This is the **write side** (`shared/executor/`). It is **totally separate** from the read side
(`shared/ingest/`, owned by `ingest-redesign-plan.md`): no crate here depends on anything under
`shared/ingest/` or vice-versa. Each stack is an independent, self-contained drop-in.

| Crate | Home | Responsibility | Depends on |
|---|---|---|---|
| **`executor-core`** | `shared/executor/core/` | Venue-**agnostic** engine: sign · fan-out send · feed-confirm · nonce · Jito tip · retry-classify · sim harness · `Venue` trait. Knows nothing about pump. | (third-party only) |
| **`executor-pumpfun`** | `shared/executor/pumpfun/` | The pump.fun **venue** impl: protocol consts, discriminators, variant catalog (SSOT), ix builders, curve+AMM pricing, one live-reserve slippage calc. Self-contained (its own protocol consts). | `executor-core` |
| **`orchestrator`** | `forge/` | The **brain** (SLP-only): `Operation`/`Plan` model, macros, personas, fingerprint auditor. Everything runs on the `Plan` with zero SOL. | `executor-core`, `executor-pumpfun` |

Adding a venue you **trade** later = a new `shared/executor/<venue>/` leaf crate depending on
`executor-core` **only** — zero churn to `executor-core` or `executor-pumpfun` (a venue you only
*watch* adds `shared/ingest/<venue>/` instead; the two sides are independent). `hunter/lab` +
`forge/lab` link **none** of these (they price from their existing lake/DB source; resolver-1
crate-boundary partition preserved).

### Target tree

```
shared/
├── executor/                    ← WRITE stack (this plan). Never pulls gRPC/proto.
│   ├── core/                    ← venue-AGNOSTIC engine  (crate: executor-core)
│   │    ├── send/               Arc<dyn Signer> · fan-out send · feed-confirm · TxLayout
│   │    │                        (CU ixs, tip+position, ALT, legacy/v0/bundle, 1232-byte pack)
│   │    ├── nonce/              durable-nonce + recent-blockhash sources (BlockhashSource)
│   │    ├── tip/                Jito tip sizing + sendBundle
│   │    ├── retry/              classify_swap_revert SSOT + typed TradeError + error-code consts
│   │    ├── sim/                simulateTransaction harness (zero-SOL)
│   │    └── venue.rs            open `Venue` trait + VenueId (static dispatch, no Box<dyn>)
│   │
│   └── pumpfun/                 ← pump.fun VENUE impl  (crate: executor-pumpfun, deps executor-core)
│        ├── protocol.rs         program IDs, WSOL, fee recipients, discriminators, acct offsets
│        ├── catalog.rs          const VariantSpec table (SSOT) + valid_*(venue) subset
│        ├── build/              buy/sell/amm/create/dev-buy/transfer ix builders (+ Amount/QuoteCtx)
│        └── price.rs            curve + AMM pricing, ONE slippage calc, min_out from LIVE reserves
│
└── ingest/                      ← READ stack (owned by ingest-redesign-plan.md). Separate; never
     core/  pumpfun/  websocket/   pulled by anything in shared/executor/. Never pulls signing.

forge/                          (was solana-launch-platform)
├── orchestrator/               ← the brain (deps executor + pumpfun)
│    ├── plan.rs                OpKind/Operation/Plan (mechanism ⊥ role ⊥ intent ⊥ venue ⊥ amount)
│    ├── macros.rs              fund · bundle_launch · volume_make · exit · consolidate
│    ├── disguise.rs            per-wallet sticky personas
│    ├── personas.rs            persona config (produced by an offline forge/lab clustering job)
│    └── audit.rs               reject bot-looking plans (index-17 integrity, star-fund, …)
├── live/                       (slp-live) → drives orchestrator::Plan → executor-core
└── lab/                        (slp-lab) → links NONE of the three; hosts the persona job

hunter/                         (was meme-trading)
└── live/                       → calls executor-core + executor-pumpfun directly (lean snipe, no disguise)
```

---

## Invariants that must survive (bug if violated)

Carried from the current stack + `monorepo-trader-plan.md`'s locked decisions:

- **Feed-confirmed sends, no RPC poll on the hot path.** Sell-confirm reads the `trades` gRPC
  feed; tip baked once, RPC body serialized once, fan-out first-win.
- **`min_out` is late-bound** — recomputed per attempt from live reserves, never frozen into a
  `Plan`. **`slippage = None` ⇒ `min_out = 1` ⇒ skip the reserve read** (latency-critical;
  `None` stays distinct from `Some(0)`). Slippage clamp `[10, 5000]` bps stays in the API
  handler, above `executor-core`.
- **Buy recovery contract:** durable-nonce buys + sign→persist-signature→submit (signature
  known before network); **never re-send a buy to recover it** (adopt/wait/drop; re-send only
  on a proven on-chain revert).
- **One shared retry table** — `classify_swap_revert(custom, route, direction)` lives in
  `executor/retry/` and is the sole copy; hunter/live imports it. Do **not** fork it.
- **Account lists are fixed by the on-chain program and MUST NOT be reshaped** — pump-curve
  fee recipient is exact at **account index 17**; a mis-order reverts `NotAuthorized(6000)`
  and simulation won't catch it. Legs are composed from audited builders, never a runtime
  account interpreter.
- **Durable nonce OFF by default for forge** (`Recent` blockhash); only latency-critical
  launch-bundle buys opt in, with a **fresh nonce account per wallet, never reused** (auditor
  flags reuse). Hunter's bot keeps its durable-nonce curve-sell path.
- **Money vocab:** `*_quote` / `*_base` exact BIGINT base units; native SOL is a `QuoteAsset`
  with lamport base. No `amount_lamports`.
- **CU cost-model constants single-sourced** and re-exported (back-compat path) so the backtest
  CU model can't drift from the live engine.

---

## Workflow — phases (each ends with a zero-SOL verify gate; do not proceed until it passes)

### Phase A — Split `pump-trader` → `executor-core` + `executor-pumpfun`
Mechanical + structural foundation. **No behavior change.** This is the biggest lift:
decompose the `PumpFunTrader` god-object across the engine/venue seam.

- `git mv shared/pump-trader shared/executor/pumpfun` (pkg `executor-pumpfun`); create
  `shared/executor/core` (pkg `executor-core`). Add both to the root `[workspace]` members.
- **Move to `executor-core`** (venue-agnostic): `tx.rs`, `nonce.rs`, `blockhash.rs`, `jito_tip.rs`,
  `swap_retry.rs` (→ `retry/`, keep `classify_swap_revert` + all error-code consts as SSOT),
  `sim.rs`, `pool.rs`, `error.rs` (`TradeError`), the `Arc<dyn Signer>` seam, and the new
  `venue.rs` (`Venue` trait + `VenueId`).
- **Keep in `executor-pumpfun`** (venue): `protocol.rs`, `buy.rs`, `sell.rs`, `amm.rs`, `create.rs`,
  `bundle_buy.rs`, `query.rs`, `reserves.rs`, `consolidate.rs`, `claim.rs`, `types.rs`,
  `config.rs`. Implement `Venue` for pump; curve/AMM are **stages inside** pump, not the venue
  axis. The pump protocol consts (program IDs, discriminators) stay **self-contained here** — the
  ingest stack keeps its own copy (the two stacks are separate; a few immutable on-chain consts
  are duplicated deliberately for portability, not shared via a kernel crate).
- **Decompose `PumpFunTrader`**: the engine (signer + rpc + tx/nonce/tip/blockhash/send/confirm)
  moves behind `executor-core`; pump caches (curve facts, pools, reserves, AMM global config) + ix
  builders + pricing stay in `executor-pumpfun` and consume the engine via the `Venue` seam.
- **Back-compat re-export:** keep a `pump_trader::*` / `pump_trader::constants` /
  `pump_trader::protocol` façade (re-exporting from `executor-core` + `executor-pumpfun`) so
  `hunter/live` and `launcher` compile **unchanged** this phase. Repoint the two direct consumers'
  Cargo deps.
- Preserve every SSOT guard test (`protocol_constants_ssot`, `classify_swap_revert`, `jito_tip`,
  `fan_out`, `reserves`, min-out math), no-DB.

**Gate:** `cargo build -p hunter-live` + `-p forge-live` green via the façade; `cargo test -p
executor-core -p executor-pumpfun` green; `cargo tree -p hunter-lab`/`-p forge-lab` link none of
`executor-core`/`executor-pumpfun`/`ingest-core`/`ingest-pumpfun`.

### Phase B — Variant catalog = SSOT + structural overflow fix
- Add the **variant catalog** to `executor-pumpfun/catalog.rs`: one const `VariantSpec` row per on-chain
  ix (`buy`, `buy_v2`, `buy_exact_sol_in`, `buy_exact_quote_in`, `buy_exact_quote_in_v2`,
  `sell`, `sell_v2`, AMM `sell`, `create`, `create_v2`, `transfer_with_seed`, SPL
  `transfer`/`transfer_checked`) carrying `disc / venue / denom / needs_wsol`. `valid_*(venue)`
  returns the legal subset so an invalid tx is unrepresentable. **Adding a variant = one row +
  one builder arm, nowhere else.** This kills the three inconsistent variant paths.
- **Fix R4 (overflow):** every buy `min_out`, **including the dev-buy launch leg**, computes
  from **live reserves** via the one shared slippage calc in `executor-pumpfun/price.rs`. Delete the
  hardcoded `INITIAL_VIRTUAL_*_RESERVES` path at `create.rs:199-208`.
- **Variant ⊥ amount:** builders take a canonical `Amount` + live `QuoteCtx`; choosing a
  variant changes **encoding only**, never the economic spend.

**Gate:** a catalog test asserts `valid_*(venue)` lists only legal variants; two buy variants
over the same canonical `Amount` produce the same on-chain economic effect (variant ⊥ amount);
the dev-buy leg's `min_out` derives from live reserves (unit test over `price.rs`), zero SOL.

### Phase C — `orchestrator` crate: `Operation` / `Plan` + providers + dry-run
- New `forge/orchestrator/`, depends on `executor-core` + `executor-pumpfun`.
- `plan.rs`: `OpKind` (Tier-1 mechanism: `Create`, `Buy`, `Sell`, `TransferSol`,
  `TransferToken`) with `Role` / `Intent` / `VenueId` / canonical `Amount` as **orthogonal
  fields** (`DevBuy` ≡ `{Buy, role: Dev, intent: Snipe}`). `Plan { ops, funding, schedule }` —
  `funding` (who funds whom) + `schedule` (timing/bundling) are **typed inputs** from SLP's
  existing unlinkability/jit-funding workstream, not built here.
- **Providers** turn each `Operation` → ixs by calling `executor-pumpfun`'s builders (enum-dispatched
  over the kind, no `Box<dyn>`); reject any variant not in `valid_*(venue)`.
- **Dry-run:** build every tx from a `Plan`, stop before submit → serialized txs + summary
  (the preview surface + test hook); `min_out` shown as computed-at-send.
- **Generalize, don't replace** SLP's existing `manage::ActionPlan`/`PlanLeg`
  (`launcher/src/manage/model.rs`) + `bundles.legs` JSONB + `launch_templates.params
  .leg_structures` — unify the two divergent leg types under `Operation`.

**Gate:** a hand-authored `Plan` (1 create + dev-buy + 2 bundler buys) → valid signed txs in
dry-run with zero SOL; `simulate_ixs` confirms they'd land.

### Phase D — Macros + personas (disguise, forge-only)
- `orchestrator/macros.rs`: `fund`, `bundle_launch`, `volume_make`, `exit`, `consolidate` — each
  expands to `Vec<Operation>` with correct `deps`. Dev-buy is its own op; **consolidate emits a
  typed `TransferSol` op, not raw `system_instruction`**.
- `disguise.rs` + `personas.rs` (forge-only): per-wallet **sticky persona** → draw a landing
  `Disguise` (variant from `valid_*`, CU/tip jitter within the persona's ranges). Every disguise
  guaranteed to land (CU ≥ real consumption; `cu_price` never dropped on a snipe). **Personas,
  not independent per-field jitter** (self-fingerprinting, rejected).
- **Offline persona derivation** = a `forge/lab` clustering job over real pump traffic →
  persona templates → `personas.config` shipped to `forge/live`. **Never on a hot path.**
  Consume `funding` + `schedule` from the existing unlinkability workstream — do not
  reimplement. Sells default un-bundled / direct-send, `tip: None`.
- **Disguise is forge-only.** hunter/live keeps its lean snipe path, no per-tx sampling.

**Gate:** `volume_make`/`exit` produce Plans where a wallet's ops share its persona but differ
in jittered CU/tip/variant; sells default `tip: None`.

### Phase E — Fingerprint auditor (mandatory)
- `orchestrator/audit.rs` rules, each a unit-testable fn over the `Plan`: star funding, equal
  amounts, same-slot cluster, direct own-graph token edge, constant CU/tip, synchronized
  `Bundler` exit, nonce-account reuse, `DurableNonce` on a non-latency op, clustered
  `close_token_ata: true` exit-tell, and **account-shape integrity** (fee recipient at index 17
  — hard reject). Output score + findings; **fail ⇒ reject unless `--allow-fingerprint`.**
- Tests run entirely on `Plan` fixtures — zero SOL, zero network.

**Gate:** rejects a deliberately-naive Plan (star fund, equal 0.02 legs, same-slot sells, a
direct token transfer, a reused nonce, a mangled account list); passes a properly
scheduled/persona'd one.

### Phase F — Wire forge flows onto `Plan` + cut over
- Repoint the launcher (`execute_launch`, `bundle_execute`, `compose_bundle_legs` —
  `launcher/src/service.rs`, `bundle.rs`, `bundle_execute.rs`) to `bundle_launch` → `Plan` →
  `executor-core`, replacing the inline leg builder + free-text `template.variant` string match.
  Preserve the `funder-target == launch-gate` SSOT (`dev_launch_required_lamports`,
  `funding_plan.rs`).
- Repoint `manage` (sell/buy/consolidate — `launcher/src/manage/execute.rs`) + `ladder`/`volume`
  onto `volume_make` / `exit` / `consolidate`. **Retire the three raw-transfer paths**
  (`wallet_funding.rs:824`, `dust_sweep.rs`, `manage/execute.rs:341`) in favor of typed
  `TransferSol` ops through `executor-core` (funding may keep its plain-transfer strategy behind the
  `TransferSol` op if the unlinkability workstream requires it — but as a typed op, auditable).
- Persist the `Plan` + audit result alongside `bundles`/`token_positions` for preview/replay
  (generalizing `bundles.legs` JSONB).
- Retire dead ad-hoc paths after parity.

**Gate:** a real (small) launch + volume + exit runs end-to-end through the `Plan` pipeline;
audit logged; landing feed-confirmed on `forge/live` (no RPC poll); the on-chain "overflow"
launch error class is gone (dev-buy `min_out` normalized from live reserves).

---

## When to split further (not now)
- **Second launchpad** → new `shared/executor/<venue>/` leaf crate depending on `executor-core` only. Done.
- **HSM / remote signer backend** → the `Arc<dyn Signer>` seam already in `executor/send/`.
- **`orchestrator` grows unwieldy** → split providers or the auditor out.

## Non-goals / guardrails
- Don't put disguise/persona logic in `executor-core` or `executor-pumpfun` — they stay reusable drop-ins.
- Don't make personas touch a hot path (derivation is an offline `forge/lab` job).
- Don't reshape account lists or reorder the fee recipient (index 17).
- Don't freeze `min_out` into the `Plan`; preserve `slippage None → skip read`,
  sign→persist-sig→submit, "never re-send a buy."
- Don't fork `classify_swap_revert`; one table in `executor/retry/`.
- Don't default `DurableNonce` on for forge; never reuse a nonce account.
- Don't reimplement funding-graph/timing here — consume the SLP unlinkability workstream.
- Don't ship independent per-field jitter — personas only.
- Keep `hunter/lab` + `forge/lab` free of `executor-core`/`executor-pumpfun` (resolver-1 crate-boundary).

---

## Verification (end-to-end, mostly zero-SOL)
- **Per-phase gates** above are the primary check; run them in order.
- **Build/partition:** `cargo build -p hunter-live -p forge-live`; `cargo tree` confirms
  `hunter-lab`/`forge-lab` link none of `executor-core`/`executor-pumpfun`; run the existing
  `scripts/dep-partition-check.{sh,ps1}`.
- **SSOT tests:** `cargo test -p executor-core -p executor-pumpfun` (catalog, `classify_swap_revert`,
  `jito_tip`, `fan_out`, `reserves`, min-out math).
- **Plan/audit tests:** `cargo test -p orchestrator` — dry-run builds valid signed txs, variant ⊥
  amount, auditor rejects the naive fixture, all zero-SOL / zero-network.
- **Zero-SOL chain check:** `simulate_ixs` / the `*-dryrun` probes confirm a hand-authored Plan
  would land without submitting.
- **Final live check (Phase F):** one small real launch + volume + exit through the `Plan`
  pipeline on `forge/live`; confirm feed-based landing and no recurrence of the overflow class.
