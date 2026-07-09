# Monorepo + Trader Redesign Plan

> **Status:** ✅ **Part 1 (Monorepo migration) is COMPLETE** — `meme-trading` and
> `solana-launch-platform` now live in ONE `Bot/` git repo + ONE Cargo workspace
> (`resolver = "1"`), the shared drop-in crates moved to `shared/`, SLP's bins renamed
> `slp-live`/`slp-lab`, and both stacks' Docker images build green. The Part 1 phase
> plan has been removed now that it's landed; only **Part 2** remains below. (History of
> the migration is in the commits + the git-bundle backups under `Bot/_monorepo-backup/`.)

What remains — **Part 2 — Trader redesign.** Reshape the execution stack into **two crates
a person can hold in their head**: a shared **`trader`** ("build & send one trade") and an
SLP-only **`campaign`** ("plan many trades and disguise them"). Handles every instruction
variant, anti-fingerprint personas, and future launchpads — without a cathedral of crates.

**Two sentences to remember the whole design:** `trader` builds and sends one trade.
`campaign` plans a batch and disguises them.

Each phase ends with a **verify gate** — do not proceed until it passes.

---

## The design in one picture

```
Bot/                                 ← monorepo, one workspace, resolver "1"
├── shared/
│   └── trader/          "build & send a trade"   (BOTH projects use this)
│         send/          sign · send · confirm  (feed-confirmed, no extra RPC poll)
│         build/         buy/sell/create ix + variant catalog + quote/base units
│         venue/         Venue trait — pumpfun.rs now; bonk.rs later drops in here
│
├── meme-trading/        → calls trader directly (fast snipe path, no disguise)
│
└── solana-launch-platform/
      └── campaign/       "plan many trades + disguise them"
            plan.rs       fund → launch → buy → sell → consolidate  (Operation/Plan)
            disguise.rs   personas (per-wallet sticky)
            audit.rs      reject bot-looking plans
```

**Everything is a folder, not a crate**, unless two things must be deployed/linked
separately. Only `trader` (shared) and `campaign` (SLP) are new crate boundaries. Split a
folder into its own crate **later, only when forced** (see [When to split further](#when-to-split-further-not-now)).

---

## Locked decisions (settled; don't re-litigate)

**Altitude & names**
- **Two crates:** shared **`trader`** (build + send) + SLP-only **`campaign`** (plan +
  disguise + audit). Everything else is folders inside them.
- `trader` = today's `pump-trader`, **renamed** and extended (pumpfun becomes a folder
  under `venue/`). Keeps a working public API so meme-trading compiles unchanged.

**Structure**
- **`resolver = "1"` stays** (safest for the pinned solana `1.17.27`). `lab`/`slp-lab` stay
  executor-free by **crate boundary**: they never link `trader` (they get pricing where it
  already lives). No `exec` feature exists for resolver-1 to additively unify.
- **Open venue seam as a `Venue` trait (a folder, not a crate).** `VenueId` is open data;
  `venue/pumpfun.rs` is the first impl; curve/amm are **stages inside pump**, not the venue
  axis. Adding Bonk = a new `venue/bonk.rs`, no other code moves.
- **Variant catalog is the SSOT** for every on-chain ix. One const row per
  `buy`/`buy_v2`/`buy_exact_sol_in`/`buy_exact_quote_in`/`buy_exact_quote_in_v2`,
  `sell`/`sell_v2`/AMM `sell`, `create`/`create_v2`, system/`transfer_with_seed`, SPL
  `transfer`/`transfer_checked` (disc / venue / denom / needs_wsol). Adding a variant = ONE
  row + one builder arm. The disguiser draws only from a **const-valid subset**
  (`valid_*(venue)`), so an invalid tx is unrepresentable.

**Anti-fingerprint**
- **Personas, not per-field jitter.** `campaign/disguise.rs` samples whole **persona
  templates** (each mimics one real client's self-consistent combo of variant + CU + tip),
  derived offline by a `lab` job over real pump traffic, **per-wallet sticky**. Independent
  jitter is rejected — self-fingerprinting and no cheaper.
- **Disguise is SLP-only.** meme-trading stays on its lean snipe path (no per-tx sampling).
- **Fingerprint surface is hard-bounded** to `{variant, amount/slippage, CU limit, CU
  price, tip value+placement, order among *optional* ixs}`. **Account lists are fixed by
  the on-chain program and MUST NOT be reshaped** — the pump-curve fee recipient is exact at
  **account index 17**; a mis-order reverts `NotAuthorized(6000)` and simulation won't catch
  it. A leg is composed from audited builders, correct-by-construction — never a runtime
  account interpreter.
- **Funding-graph (who funds whom) + timing** are NOT built here — they come from SLP's
  existing `wallet-unlinkability` / `jit-funding` workstream; `campaign` consumes their
  output.
- **Auditor is mandatory** — a `Plan` that fails audit doesn't execute without an explicit
  `--allow-fingerprint` override.

**Hot-path & correctness invariants (inherited — bug if violated)**
- **Sell-confirm reads the `trades` gRPC feed, never a fresh RPC poll.** No blocking I/O on
  the hot path; tip baked once, RPC body serialized once, fan-out first-win.
- **Buy recovery contract:** durable-nonce buys + sign→persist-signature→submit (signature
  known before network); **never re-send a buy to recover it** (adopt/wait/drop; re-send
  only on a proven on-chain revert). The `Plan` keeps signatures deterministic at sign time.
- **`min_out` is late-bound** — recomputed per attempt from live reserves, never frozen into
  the `Plan`. **`slippage = None` ⇒ `min_out = 1` ⇒ the snipe skips the reserve read**
  (latency-critical; `None` stays distinct from `Some(0)`). Slippage clamp ([10, 5000] bps)
  stays in the API handler, above `trader`.
- **One shared retry table** — `classify_swap_revert(custom, route, direction)` lives in
  `trader/send/` and is imported by meme-trading's live loops + manual handler; the redesign
  may **not** fork it (guard-tested, no-DB).
- **Durable nonce is OFF by default for SLP** (`Recent` blockhash); only latency-critical
  launch-bundle buys opt in, with a **fresh nonce account per wallet, never reused** (the
  auditor flags reuse). meme-trading's bot keeps its durable-nonce curve-sell path.
- **Money vocab:** `*_quote` / `*_base` exact **BIGINT base units**; native SOL is a
  `QuoteAsset` with lamport base. No `amount_lamports`.
- **CU cost-model constants stay single-sourced** and re-exported at
  `pump_trader::constants` (via the crate's back-compat path) so the backtest CU model can't
  drift from the live trader.

---

## The two crates in detail

### `shared/trader/` — build & send one trade

The renamed, extended `pump-trader`. Standalone drop-in, no workspace deps.

- **`send/`** — the engine: `Arc<dyn Signer>` (HSM/remote-ready), sign→persist-sig→submit
  hook, RPC send | Jito `sendBundle`, feed-confirm, `TxLayout` (CU ixs, tip + position,
  ALT, legacy/v0/bundle, blockhash-vs-durable-nonce), 1232-byte pack decision,
  `classify_swap_revert` SSOT, typed `TradeError`.
- **`build/`** — pure ix builders + the **variant catalog** (const `VariantSpec` table +
  `valid_*(venue)`), quote/base unit types + one shared slippage calc. Each builder takes a
  canonical `Amount` + live `QuoteCtx` → that variant's args (variant changes **encoding
  only**, never the economic amount). `create.rs` + dev-buy live here.
- **`venue/`** — the open `Venue` trait + registry (`AnyVenue`, static dispatch, no
  `Box<dyn>`); `pumpfun.rs` = program IDs, discriminators, AMM offsets, pump curve/AMM
  pricing. Curve/AMM are stages inside it.

`trader` is **live-oriented** (it signs/sends). Its `build/`+`venue/` folders are pure, but
`lab` doesn't need them (lab prices from its existing source), so `lab` simply never links
`trader`.

### `solana-launch-platform/crates/campaign/` — plan many trades + disguise them

The brain. Depends on `trader`. Everything valuable runs on the `Plan` (preview, dry-run,
tests, audit) with **zero SOL**.

- **`plan.rs`** — the uniform model:
  - `OpKind` (Tier-1 mechanism: `Create`, `Buy`, `Sell`, `TransferSol`, `TransferToken`),
    with `Role` / `Intent` / `VenueId` / canonical `Amount` as **orthogonal fields**
    (`DevBuy` ≡ `{Buy, role: Dev, intent: Snipe}`).
  - Tier-2 **macros** (`fund`, `bundle_launch`, `volume_make`, `exit`, `consolidate`) —
    functions that expand to primitive `Operation`s with correct `deps`.
  - `Plan { ops, funding, schedule }` — `funding` (who funds whom) + `schedule` (timing,
    bundling) are **inputs from the SLP unlinkability/jit-funding workstream**, not built
    here.
  - **Providers** turn each `Operation` → ixs by asking `trader`'s builders (enum-dispatched
    over the kind, no `Box<dyn>`); reject a variant not in `valid_*(venue)`.
- **`disguise.rs`** — Plane-1 policy: pick the wallet's **sticky persona**, then jitter
  within its ranges (variant from `valid_*`, CU/tip). Every disguise guaranteed to land
  (CU ≥ real consumption; `cu_price` never dropped on a snipe). Personas come from…
- **`personas.rs`** (config, produced by an **offline `lab` clustering job** over real pump
  traffic — the only new anti-fingerprint compute, never on a hot path; re-run on a cadence
  so disguises track the meta).
- **`audit.rs`** — scores a `Plan` before execution; each rule a unit-testable fn over the
  `Plan` (star-funding, equal-amounts, same-slot cluster, direct own-graph token edge,
  constant CU/tip, synchronized exit, nonce-account reuse, `close_token_ata` exit-tell,
  **account-shape integrity** = fee recipient must be at index 17). Fail → reject unless
  overridden.

---

# Part 2 — Trader redesign

Prerequisite: Part 1 done and green. Behavior stays identical for meme-trading throughout
(it keeps using `trader`'s public API); the new model is added alongside and SLP cuts over
last.

### Phase A — Rename & extend `pump-trader` → `trader`
Mechanical foundation. No behavior change.
- [ ] `git mv shared/pump-trader shared/trader`; set `name = "trader"`; keep a back-compat
      re-export path (`pump_trader::*` / `pump_trader::constants`) so meme-trading (`live`,
      `trading_core`) compiles unchanged. Repoint SLP's dep.
- [ ] Reorganize into folders: **`send/`** (sign/send/confirm/layout/pack/retry/error, incl.
      `classify_swap_revert` SSOT + `BlockhashSource`), **`build/`** (ix builders + variant
      catalog + quote/base units + slippage), **`venue/`** (`Venue` trait + registry +
      `pumpfun.rs` protocol & pricing). `create.rs` + dev-buy land in `build/`.
- [ ] Add the **variant catalog** (const `VariantSpec` + `valid_*(venue)`) and the open
      `VenueId`/`Venue` trait — pump as the sole impl for now. Preserve fixed account order
      (fee recipient index 17), AMM-buy exact-base-out, `min_out` late-bound + `slippage
      None → skip read`, durable-nonce buy determinism + sign→persist-sig→submit.
- [ ] Keep all SSOT guard tests (`protocol_constants_ssot`, `classify_swap_revert`,
      `jito_tip`, `fan_out`, `reserves`, `min-out math`) green, no-DB.

**Gate:** `cargo build --bin live` (meme) + `--bin slp-live` green via the re-export;
`cargo test -p trader` green (incl. a catalog test that `valid_*(venue)` lists only legal
variants). `cargo tree -p lab`/`-p slp-lab` still link neither `trader` nor
`ingest-laserstream`.

### Phase B — `campaign` crate: `Operation` / `Plan` + providers + dry-run
- [ ] New `solana-launch-platform/crates/campaign/`, depends on `trader`.
- [ ] `plan.rs`: `OpKind` + `Operation` (role/intent/venue/amount orthogonal) + `Plan`
      (`ops`, `funding`, `schedule` — the latter two typed as inputs). Providers turn each
      `Operation` → ixs via `trader` builders; reject invalid variants.
- [ ] **Dry-run**: build every tx from a `Plan`, stop before submit → serialized txs +
      summary (the preview surface + test hook). `min_out` shown as computed-at-send.
- [ ] Reconcile the model with SLP's existing `manage::ActionPlan`/`PlanLeg` + `bundles.legs`
      JSONB + `launch_templates.params.leg_structures` — **generalize, don't replace**; unify
      the two divergent leg types under `Operation`.

**Gate:** a hand-authored `Plan` (1 create + dev-buy + 2 bundler buys) → valid signed txs in
dry-run with zero SOL; `simulate_ixs`/probe confirms they'd land; two buy variants over the
same canonical amount produce the same economic spend (variant ⊥ amount).

### Phase C — Macros + personas (disguise)
- [ ] `plan.rs` macros: `fund`, `bundle_launch`, `volume_make`, `exit`, `consolidate` → each
      expands to `Vec<Operation>` with `deps`. Dev-buy is its own op; consolidate emits
      `TransferSol` (no raw `system_instruction`).
- [ ] `disguise.rs` + `personas.rs` (SLP-only): per-wallet sticky persona → draw a landing
      `Disguise` (variant from `valid_*`, CU/tip jitter). `Recent` blockhash default;
      `DurableNonce` only for launch-bundle buys with a fresh per-wallet nonce account.
- [ ] **Offline persona derivation (`lab` job):** cluster real pump txs by envelope features
      → persona templates → `personas.config` shipped to `slp-live`. Consume `funding`
      (non-star, jittered amounts) + `schedule` (bundle-at-launch; spread/ladder buys+sells;
      jittered intervals; **sells default un-bundled/direct-send, tip None**) from the
      existing SLP unlinkability/jit-funding workstream — do not reimplement.

**Note (where the value is):** `buy` and `buy_exact_sol_in` emit the same on-chain
`TradeEvent`, and for sells venue is forced + variant swaps are few — so per-tx disguise
beats naive discriminator-trackers but not an analyst normalizing by economic effect. The
funding-graph + timing (the consumed workstream) carry ~90% of the anti-detection value.
Build the catalog + personas because they're cheap and clean.

**Gate:** `volume_make`/`exit` produce Plans where a wallet's ops share its persona but
differ in jittered CU/tip/variant; sells default `tip: None`.

### Phase D — Fingerprint auditor
- [ ] `audit.rs` rules (each a unit-testable fn over the `Plan`): star funding, equal
      amounts, same-slot cluster, direct own-graph token edge, constant CU/tip, synchronized
      `Bundler` exit, nonce-account reuse, `DurableNonce` on a non-latency op, clustered
      `close_token_ata: true` exit-tell, and **account-shape integrity** (fee recipient at
      index 17 — hard reject). Output score + findings; fail → reject unless
      `--allow-fingerprint`.
- [ ] Tests run entirely on `Plan` fixtures — zero SOL, zero network.

**Gate:** rejects a deliberately-naive Plan (star fund, equal 0.02 legs, same-slot sells, a
direct token transfer, a reused nonce, a mangled account list); passes a properly
planned/scheduled/persona'd one.

### Phase E — Wire SLP flows onto `Plan` + cut over
- [ ] Repoint SLP launcher (`execute_launch`, `bundle_execute`, `compose_bundle_legs`) to
      `bundle_launch` → `Plan` → `trader`, replacing the inline leg builder + free-text
      `template.variant` string match. Preserve the `funder-target == launch-gate` SSOT
      (`dev_launch_required_lamports`).
- [ ] Repoint `manage` (sell/buy/consolidate) + `ladder`/`volume` onto `volume_make` /
      `exit` / `consolidate`.
- [ ] Persist the `Plan` + audit result alongside `bundles`/`token_positions` for
      preview/replay (generalizing `bundles.legs` JSONB).
- [ ] Retire dead ad-hoc paths after parity.

**Gate:** a real (small) launch + volume + exit runs end-to-end through the `Plan` pipeline;
audit logged; landing feed-confirmed on `live` (no RPC poll); the on-chain "overflow" launch
error class is gone (dev-buy `min_out` normalized from live reserves, not hardcoded initial
reserves).

---

## When to split further (not now)

Turn a folder into its own crate ONLY when a real need lands:
- a **second launchpad** ships → extract `trader/venue/` (or split per-venue crates)
- **`lab` needs pump pricing** in a way its current source can't give → extract
  `trader/build/`+`venue/` pricing as a lab-safe crate (and keep the pricing math
  single-sourced — this is the SSOT trigger to watch: `trader` pricing vs any
  `trading_core` curve-spot copy)
- an **HSM / remote signer backend** → extract `trader/send/`
- **`campaign`** grows unwieldy → split providers or the auditor out of it

Until then, they stay folders. Do not pre-build the split.

---

## Non-goals / guardrails
- Don't put disguise/persona logic in `trader` — it stays a reusable drop-in.
- Don't make `campaign`/personas touch a hot path (persona derivation is offline `lab`).
- Don't reshape account lists or reorder the fee recipient (index 17); disguise varies only
  variant/CU/tip/amount/optional-ix order.
- Don't freeze `min_out` into the `Plan`; preserve `slippage None → skip read`,
  sign→persist-sig→submit, "never re-send a buy."
- Don't fork `classify_swap_revert`; one table in `trader/send/`.
- Don't default `DurableNonce` on for SLP; never reuse a nonce account.
- Don't reimplement funding-graph/timing here — consume the SLP unlinkability workstream.
- Don't ship independent per-field jitter — personas only.
- Keep `lab`/`slp-lab` free of `trader` (resolver-1 crate-boundary partition).

---

## What this supersedes / reconciles
- **Replaces** `monorepo-migration-plan.md` + `executor-redesign-plan.md` (folded in here at
  the simpler 2-crate altitude).
- **Adopts** the audit blueprint's open-venue idea (as a `Venue` trait folder, not a crate)
  and mechanism-vs-policy split; **rejects** its resolver-`"2"` demand (crate-boundary
  partition instead) and its `ingest-grpc` rename / `ingest-websocket` deletion (out of
  scope).
- **Supersedes** `live-lab-remake-plan.md`'s whole-`pump-trader` topology; preserves its
  SSOT / hot-path / dep-partition principles.
- **Preserves** meme-trading's binding execution invariants (`@arch/trade-execution.md`,
  `@plans/trade-execution/*`) and SLP's foundation-doc §3e/§5 (bounded fingerprint surface,
  unlinkability as a separate workstream).

---

## Appendix — current-state reference

**Current bundle-launch structure** (SLP `execute_launch` + `bundle_execute`): NOT one tx —
**1 create+dev-buy tx** (`[CU limit, CU price, Create_v2, ATA CreateIdempotent, Buy,
Jito-tip]`, dev+mint signed, sent first) **+ a separate N-tx Jito bundle** (one leg per
bundler). `compose_bundle_legs` assigns the same `quote_per_leg` to every leg (the
equal-amount pattern the funding workstream must break). Variant is chosen three
inconsistent ways today (create free-text string; bundle enum; manage hardcoded); consolidate
bypasses the trader via raw `system_instruction`; dev-buy is fused into
`create_token_*_and_dev_buy`. The redesign unifies all of these under `Operation`.

**The "overflow" launch error is on-chain, not Rust** — Pump/PumpFee returning
`MathOverflow (6025)` / `ShareCalculationOverflow (6015)`, most plausibly the dev-buy leg's
`min_out` computed against **hardcoded assumed initial reserves** rather than a live curve
read. Phase B's `amount + QuoteCtx` normalization (live reserves, one slippage calc) is the
structural fix.

**Leaner launch variants exist in the wild** (e.g. `[Create_v2, ATA Create, BuyExactSolIn]`,
no CU ixs, no Jito tip) — proof CU ixs and the tip are optional and the buy variant is
swappable. That diversity is what the catalog + personas make first-class. But CU price = 0
(omit) risks non-inclusion under congestion and loses snipe races — jitter within a
competitive band, never drop it on a snipe.
