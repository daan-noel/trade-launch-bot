# Wallet unlinkability / anti-clustering plan

> **Status: PLANNED (not started).** Extends the (now-implemented, retired-to-git)
> `wallet-funding-plan.md` + `jit-funding-plan.md` design + decision **D5**. Goal: make our launch/trade wallets
> resist on-chain clustering by *other traders* (snipers, copy-bots, rug-checkers,
> rival launchers). **Scope of protection:** other on-chain observers — NOT the
> exchange (it KYCs us) and NOT law enforcement. Bulk CEX automation carries real
> AML/ToS risk; the CEX-seeding step here is deliberately **semi-manual +
> low-frequency**, not an automated withdrawal API.

## Context primer (read first in a fresh session)

We run a Solana launch+trade platform (`forge`). Wallets flow
`generated → funding → funded → used → retired` (`ManagedWallet`,
`platform-core::models`). Treasury wallets are the SOL *source*; dev + bundler
wallets are the launch buyers. Funding lives in
`crates/launcher/src/wallet_funding.rs` (seam: `trait FundingStrategy`,
`DirectJittered` impl, `TreasuryPool` multi-source spill, `fund_once` /
`fund_for_launch`, `FundMode::{Background,Manual}`, safety rails
`FUND_ENABLED`/`FUND_DRY_RUN`/reserve floor/per-interval cap/amount+timing
jitter). The inverse is `crates/launcher/src/dust_sweep.rs`. Config =
`FundingConfig` in `crates/launcher/src/config.rs`. Latest migration = `0011`;
**new work starts at `0012`**.

### The two detectors we must beat (they are orthogonal)

1. **Entity clustering** — Bubblemaps / Nansen / Chainalysis / Arkham. Build a
   funding graph: *A funded B, B bought → linked*, tracing shared funding
   ancestry within **2–5 hops**. Also cluster by **identical amounts** and
   behavior. → beaten by **funding-origin unlinkability** (Phases 1–2).
2. **Bundle / insider detection** — Trench (`TrenchScannerBot`), pump.fun bundle
   checkers, GMGN "bundle/insider ratio". Ignore funding; key on **same-slot
   (~0.4s) co-buys**, report **total bundled %** + **current held %**, flag **3+
   wallets** holding supply. → *not* helped by clean funding; needs **timing +
   low held-%** (Phases 3, 6).

Plus: "fresh wallet funded then instantly buys" is its own flag (GMGN "fresh
wallets"; 80%+ of trades within 3s of launch ⇒ scripted); and **output
convergence** (all proceeds → one wallet) re-clusters everything at the exit.

### Invariants (must hold after this plan)

- **No fan-out hub.** CEX pays each source wallet **directly**; the platform
  never creates an intermediate wallet that funds many (that wallet becomes the
  hub node). Source→pool fan-out is bounded + rotated instead.
- **No uniform amounts.** Never two wallets funded to the identical lamports in a
  launch (the "exactly 1,000 USDC to every wallet" tell). Jitter + non-round.
- **Bounded wallets per funding source**, then retire the source.
- **No single exit sink.** Retired-wallet SOL + proceeds must not all land on one
  address.
- Every rule above has a **guard test** (CLAUDE-style SSOT/rail discipline).

---

## Phase 0 — Threat model, config, data model

**Goal:** scaffolding every later phase depends on.

- [ ] Add a "Threat model" section to `wallet-management.md` (the two detectors +
      invariants above) so it's the SSOT explanation.
- [ ] Migration **`0012_wallet_unlinkability.sql`** on `managed_wallets`:
  - [ ] `funding_origin TEXT NULL` — e.g. `cex:kraken#2`, `cex:binance#1`
        (opaque operator label; identifies which CEX *account* seeded a source).
  - [ ] `source_wallet_id UUID NULL` — which source treasury funded this pool
        wallet (bounded-fan-out accounting + retirement).
  - [ ] `funded_at TIMESTAMPTZ NULL` — when the wallet first received SOL
        (drives the min-age gate, Phase 3).
  - [ ] `launches_served INT NOT NULL DEFAULT 0` on treasury rows (retirement).
  - [ ] index `(role, status, funding_origin)`.
- [ ] Extend `FundingConfig` + `.env`/`.env.example` (all off/no-op by default):

| env | default | meaning |
| --- | --- | --- |
| `UNLINK_ENABLED` | `false` | master switch for this subsystem |
| `UNLINK_MAX_WALLETS_PER_SOURCE` | `4` | retire a source after it funds this many |
| `UNLINK_MAX_LAUNCHES_PER_SOURCE` | `1` | retire a source after N launches |
| `UNLINK_MIN_WALLET_AGE_SECS` | `0` | min gap funded→first buy (0 = off) |
| `UNLINK_AMOUNT_ROUND_LAMPORTS` | `0` | quantize amounts OFF round numbers |
| `UNLINK_NO_SHARED_SOURCE_PER_LAUNCH` | `true` | forbid 2 wallets in one launch sharing a source |

- [ ] DoD: `cargo check -p launcher -p live` clean; migration applies; no
      behavior change while `UNLINK_ENABLED=false`.

## Phase 1 — CEX-seeded rotating source wallets

**Goal:** replace the persistent treasury with a pool of fresh, CEX-seeded,
retire-after-use sources. `TreasuryPool` already spills across *all*
`role=treasury` wallets — reuse it; add lifecycle + rotation.

- [ ] **Seed flow (semi-manual)** — new op path so the operator withdraws from a
      CEX to fresh sources:
  - [ ] `POST /api/wallet_pool/seed_sources { count, funding_origin, amount_lamports, jitter_pct }`
        → generates `count` fresh `role=treasury status=generated` wallets, each
        tagged `funding_origin`, and returns a **withdrawal worksheet**: per-wallet
        address + a **jittered non-round** target amount to withdraw from that CEX
        account. Platform sends **nothing** (the CEX does).
  - [ ] Balance poller promotes a seeded source `generated→funded` when SOL
        arrives (reuse existing promotion; set `funded_at`).
- [ ] **Rotation / retirement:** after `UNLINK_MAX_WALLETS_PER_SOURCE` funded or
      `UNLINK_MAX_LAUNCHES_PER_SOURCE` launches, mark the source `retired`
      (residual swept per Phase 5). Increment `launches_served`.
- [ ] **Source selection rail** in `TreasuryPool::pick_source`: when
      `UNLINK_NO_SHARED_SOURCE_PER_LAUNCH=true`, exclude sources already used by
      *this* launch and prefer spreading across `funding_origin`s.
- [ ] **No-hub invariant test:** assert the funder never creates a
      treasury→treasury (or dev→dev) fan-out transfer — CEX is the only many-payer.
- [ ] Frontend `WalletPool.tsx`: "Seed sources from CEX" panel rendering the
      worksheet + arrival status; show per-source `funding_origin`,
      `launches_served`, age.
- [ ] Files: `wallet_funding.rs`, `wallet_pool.rs`, `http.rs`, `config.rs`,
      `managed_wallet` repo, `WalletPool.tsx`.
- [ ] DoD: seed → arrival → source usable by `fund_for_launch`; rotation retires
      on threshold; rail test green.

## Phase 2 — Amount de-correlation

**Goal:** kill the amount fingerprint on both funding hops.

- [ ] Seed worksheet (Phase 1) amounts already jittered — also quantize **off**
      round numbers via `UNLINK_AMOUNT_ROUND_LAMPORTS` (e.g. never `1.000`,
      `0.500`).
- [ ] `DirectJittered` / JIT `fund_for_launch`: JIT currently funds the **exact**
      template need (jitter 0 — see `jit-funding-plan.md`), so every leg lands on
      an identical balance = a fingerprint. Add **per-wallet headroom jitter**
      (still ≥ launch gate `dev_launch_required_lamports`) so no two legs match.
  - [ ] Keep SSOT: floor stays `dev_launch_required_lamports` /
        `leg_required_lamports`; jitter only adds headroom, never underfunds.
- [ ] Guard test: in any single launch, all funded amounts are **distinct** and
      non-round; every amount ≥ its gate.
- [ ] DoD: test green; launch still passes its pre-launch balance gate.

## Phase 3 — Timing & session-level de-correlation

**Goal:** beat same-window + fresh-wallet + session clustering. Per-send jitter
(`max_delay_ms`) alone is insufficient.

- [ ] **Min-age gate:** a wallet may not *buy* until
      `now - funded_at ≥ UNLINK_MIN_WALLET_AGE_SECS`. Encourages pre-funding
      ahead of launch; defeats fund-then-instant-buy. Enforce in the launch/buy
      claim path.
- [ ] **Session de-correlation** for buy/sell activity (not just funding): jitter
      *entry* per wallet so non-bundle buys avoid the 2–3s co-activity window, and
      stagger *exits* so wallets don't wake/dorm together across the session.
- [ ] **Launch-slot policy knob** (per template): `bundle_in_slot` (max snipe
      protection, maximally bundle-visible) vs `staggered_entry` (spread out of
      the ~0.4s slot, lower bundle-%). Document the trade-off; default explicit.
- [ ] Files: `wallet_funding.rs` (age gate), launch/`service.rs` claim path,
      trade scheduling, template params.
- [ ] DoD: age gate blocks an underage claim; staggered mode produces buys in
      distinct slots in a dry run.

## Phase 4 — Behavior / route fingerprint diversity

**Goal:** identical tx configs fingerprint the tooling and survive amount/time
jitter.

- [ ] Randomize **per wallet, within bounds**: priority fee (`cu_price`),
      slippage, and (where >1 venue exists) route. Feed from a per-wallet seed so
      it's stable per wallet but varied across the set.
- [ ] Verify the exact knobs in the `pump-trader` buy/sell args before wiring
      (path-dep crate from hunter) — do **not** invent field names.
- [ ] Guard test: across a launch's wallets, `cu_price`/slippage are not all equal.
- [ ] DoD: test green; no regression to landing reliability on the snipe path.

## Phase 5 — Exit-side de-clustering (`dust_sweep` + proceeds)

**Goal:** stop output convergence. Today `dust_sweep.rs::sweep_once` sends **every**
retired wallet's balance to **one** treasury (`by_role(...).next()`) — that single
address re-clusters the whole operation.

- [ ] Route retired-wallet dust to a **rotating** set of collection targets, not
      one address: fresh unrelated collection wallets and/or **CEX deposit
      addresses** (rotating), chosen per-wallet.
- [ ] Same for trade **proceeds** — never funnel to a single home treasury.
- [ ] **CEX deposit caveat (doc):** exchanges run pre-deposit **provenance
      scanning**; freshly-bundled meme proceeds can flag/freeze. Use fresh deposit
      addresses; treat the deposit path as monitored. Note in `wallet-management.md`.
- [ ] Config: `UNLINK_SWEEP_TARGETS` (list) or a `collection` wallet role;
      rail = no target receives > X% of a batch.
- [ ] Files: `dust_sweep.rs`, `config.rs`, docs.
- [ ] DoD: a multi-wallet sweep lands on ≥N distinct targets; guard test asserts
      non-convergence.

## Phase 6 — Cluster self-audit (verify we're not flagged)

**Goal:** measure ourselves with the adversary's lens before launching.

- [ ] Pre-launch **cluster-risk score** computed from our own data: max shared
      funding-origin within N hops, amount-uniformity, projected same-slot
      bundle-% and current-held-%, wallet ages.
- [ ] Surface in Launch Console; **block/warn** on threshold (e.g. held-% high or
      shared-source detected).
- [ ] Optional later: reconcile against an external checker (Trench/Bubblemaps-style)
      out-of-band; do not hard-depend on a 3rd-party API in the hot path.
- [ ] DoD: score renders for a real template; a deliberately-bad config trips the
      warning.

## Phase 7 — Docs, rails, tests, honest scope

- [ ] Update: `wallet-funding-plan.md` (add unlinkability layer), `jit-funding-plan.md`
      (amount headroom jitter), `decisions.md` **D5** (mark hop-graph *rejected* in
      favor of CEX-seed + rotation; record rationale), `wallet-management.md`
      (threat model + exit caveat), `roadmap-plan.md`.
- [ ] Consolidated safety rails: `UNLINK_ENABLED` master switch, dry-run parity,
      wallets-per-source cap, min-age, exit non-convergence — all fail safe (off).
- [ ] Guard-test suite (no-DB where possible): no-hub, amount-distinct,
      no-shared-source-per-launch, exit non-convergence, age-gate.
- [ ] **Honest-scope note** in docs: resists *other traders'* clustering only; not
      privacy from the exchange (KYC) or authorities; keep CEX seeding manual +
      low-frequency to bound AML/ToS exposure.
- [ ] DoD: `cargo check -p launcher -p live` + `cargo test -p launcher` + `npm run
      build` clean.

---

## Sequencing

`0 → 1 → 2` are the load-bearing funding-unlinkability core (do first). `3` and
`5` are independent and high-value (timing + exit). `4` depends on confirming
`pump-trader` args. `6` needs `0`. `7` closes out. Ship behind `UNLINK_ENABLED`
so each phase is a dark-launch until the guard tests pass.

## What we explicitly are NOT doing (and why)

- **Multi-hop peel chains** (old D5 Tier-2 `MultiHop`): tracked within hop
  distance, adds a branching fingerprint, doesn't break clustering. Rejected.
- **Mixers / privacy pools:** sanctions + tainted-funds + freeze risk; provenance
  scanners flag them. Wrong tool for funds we want to bank.
- **Fully automated CEX withdrawal API:** whitelist/velocity/AML-flag risk to the
  account; brittle at bundle scale. Seeding stays semi-manual.

## Key real-world facts behind these choices (sources)

- Clustering traces funding ancestry **2–5 hops**; a 340-wallet cluster shared one
  Binance withdrawal source within 3 hops (zkSync airdrop). → spread across CEX
  accounts, pay wallets directly, bound fan-out.
- CEX-funded wallets still clustered when every wallet got an **identical amount**
  (1,000 USDC case). → Phase 2.
- Bundle checkers key on **same ~0.4s slot**, report total-bundled-% vs
  current-held-%, flag 3+ holders; unrelated same-slot buys are false positives. →
  Phases 3, 6 + keep held-% low.
- Fresh-wallet + 80%-of-trades-within-3s ⇒ flagged scripted. → Phase 3 age gate.
- Exchanges run pre-deposit provenance scanning. → Phase 5 caveat.
