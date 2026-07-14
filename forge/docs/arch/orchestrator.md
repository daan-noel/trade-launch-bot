# forge orchestrator — architecture map

The `forge-orchestrator` crate (lib name `orchestrator`, dir `forge/orchestrator/`)
is forge's **brain over the executor write-stack**: one uniform `Operation`/`Plan`
model of a trade, catalog-gated **providers** that make an illegal tx
unrepresentable, sticky-persona **disguise**, a fingerprint **auditor**, and a
pure zero-SOL **dry-run** (validation + preview). It never builds a real
`solana_sdk::Instruction` — that happens at the launcher's Phase-F cutover through
an initialized `PumpFunTrader`.

- **LIVE-only, forge-only.** `hunter/live` calls the executor stack directly (lean
  snipe, no plan/disguise); neither family's `lab` links this crate.
- **Deps:** `executor-core` (neutral `VenueId`/`DecoStep`/`IxLayout`/`ComputeBudgetCfg`
  seam) and `pump-trader` (dependency key; package `executor-pumpfun`, lib
  `pump_trader` — the catalog SSOT + ix builders). Plus `serde`, `serde_json`,
  `uuid`, `solana-sdk` (address parse at the provider boundary only).

## Architecture

A forge write flow (launch / manage) never hand-rolls instructions. It:

1. **assembles a `Plan`** from the high-level `macros` (`bundle_launch`,
   `volume_make`, `exit`, `consolidate`, `fund`), which own the `deps` wiring and
   draw op ids from a shared `IdSeq`;
2. **`prepare`s** it — the providers validate every op against the venue catalog
   (variant legality, kind≡variant mechanism, amount⊥denom encodability, address
   parse, dep resolution). Pure: zero SOL, zero network. This IS the dry-run: a
   `PreparedPlan` of `PreparedOp`s carrying the resolved `&'static VariantSpec`,
   parsed pubkeys, and the late-bound `MinOut` **policy** (never a frozen number);
3. **`disguise`s** every op — each wallet gets a *sticky* `Persona` (stable
   `hash(pubkey) % n`), and a per-op deterministic RNG draws a landing-guaranteed
   send shape (variant / cu_limit / cu_price / tip). The chosen encoding is applied
   back onto the plan (except `lock_variant` legs) so audited == sent;
4. **`audit`s** the disguised plan + per-op `SendProfile`s for on-chain fingerprint
   tells; a plan that doesn't pass is refused.

The orchestrator produces the validated, disguised, audited `Plan`; the **launcher**
(`forge/launcher`, the only consumer) turns it into txs. The whole model is keyed on
the same venue/variant axes the catalog owns, so the two can never drift.

The domain model is built on **five orthogonal axes** — a launch, a snipe, a volume
leg, a consolidate, and a fund are the *same shape* with different field values:

| Axis | Type | Meaning |
|---|---|---|
| mechanism | `OpKind` (= `pump_trader::VariantKind`, reused verbatim) | *what* on-chain action: Create / Buy / Sell / TransferSol / TransferToken |
| role | `Role` | *which* wallet persona: Dev / Bundler / Volume / Treasury / External |
| intent | `Intent` | *why*: Snipe / Accumulate / MakeVolume / Exit / Fund / Consolidate / Create |
| venue | `VenueId` (= `pump_trader::VenueId`) | *which* launchpad (only `PumpFun` wired) |
| amount | `Amount` | *how much*, denominated by which side is exact |

So `DevBuy ≡ { kind: Buy, role: Dev, intent: Snipe }` — not a bespoke type.

## Module map

| File | Responsibility |
|---|---|
| `lib.rs` | Crate root + flat re-export surface. Doc-comments the phase map (C: Operation/Plan+providers · D: macros+personas/disguise · E: audit · F: launcher cutover). |
| `plan.rs` | The **domain model**: `Operation` (the atom) and `Plan` (a batch + `Schedule`). The five axes, `Amount`, `WalletRef`, `IdSeq` op-id allocator, `Schedule`/`ScheduleSlot`. Constructors: `create`/`buy`/`sell`/`sell_with`/`transfer_sol`/`transfer_sol_as`, `with_authored`. Serializable (base58 `String` addresses, not `Pubkey`). |
| `provider.rs` | The **catalog gate**: `prepare(&Plan) -> Result<PreparedPlan, PlanError>`. Static dispatch over `VenueId`/`OpKind` (a match, no `dyn`). Validates variant∈catalog, `spec.kind==op.kind`, `denom_accepts(denom, amount)`, addresses parse, deps resolve, ids unique. `MinOut` late-binding policy. `denom_accepts` is public (disguise reuses it). |
| `macros.rs` | The **plan builders**: `bundle_launch`, `volume_make`, `exit`, `consolidate`, `fund` — each expands one operator intent into `Vec<Operation>` with correct `deps`, drawing ids from a shared `IdSeq`. Fund/consolidate emit typed `TransferSol` ops (never raw `system_instruction::transfer`). `DEFAULT_SOL_TRANSFER_VARIANT = "system_transfer"`. |
| `personas.rs` | **Sticky personas**: `Persona` (buy/sell variant pools, `cu_headroom`/`cu_price`/`tip_lamports` `JitterRange`s, `non_snipe_tip_pct`), `PersonaSet` with `assign` (hash→persona, sticky per wallet), `from_config` (lab-derived JSON, catalog-validated), `builtin` (3 hand-authored archetypes). |
| `disguise.rs` | **Disguise sampler**: `draw`/`for_op`/`disguise_ops` turn (op + persona) → a `Disguise` (variant, cu_limit, cu_price, tip). Landing guarantees: `cu_limit = real_consumption + headroom`, `cu_price ≥ min > 0`, chosen variant stays denom+stage compatible. Tips: snipe always, non-snipe buy by prob, sell never, create/transfer never. |
| `audit.rs` | **Fingerprint auditor** (mandatory last gate): `audit`/`audit_with` → `AuditReport`. 10 standalone rules over `Plan` + `SendProfile`s; `Severity::{Warn, Reject}`; `passed(allow_fingerprint)` — a `Reject` (malformed account shape) is never overridable. |
| `rng.rs` | Deterministic PRNG: `SplitMix64` + `fnv1a`. Seeded from `(wallet_pubkey, op_id)` so disguises are reproducible/replayable, never wall-clock. Not security-sensitive (no key material). |

### Key types

| Type | Where | Note |
|---|---|---|
| `Operation` | plan | id, kind, venue, `variant` (catalog name), role, intent, `amount`, `slippage_bps` (NOT min_out), `wallet`, `target`, `deps`, optional `layout` (`Vec<DecoStep>`), `lock_variant`. |
| `Plan` | plan | `mint_address` + `ops` + `schedule`. Persisted (generalizes `bundles.legs` JSONB + `manage::ActionPlan`). |
| `Amount` | plan | `ExactQuote`(SOL-in) / `ExactBase`(tokens-out) / `ExactBaseIn`(sell) / `Sol` / `Token` / `None`. Exact base-unit integers. |
| `PreparedPlan`/`PreparedOp` | provider | Validated, buildable/summarizable; borrows the `Operation`, pins `&'static VariantSpec` + parsed `Pubkey`s + `MinOut`. |
| `MinOut` | provider | `Late{slippage_bps}` (computed at send from live reserves) / `Unprotected` (`min_out=1`) / `NotApplicable`. `label()` for preview. |
| `PlanError` | provider | Crate-owned (no `anyhow` leak): UnknownVariant / KindMismatch / DenomMismatch / BadAddress / MissingTarget / DanglingDep / DuplicateId. |
| `Disguise` | disguise | variant, cu_limit, cu_price_micro_lamports, `tip_lamports: Option`. Serializable for preview; only recomputed on replay (not persisted). |
| `AuditReport` / `Finding` / `Rule` / `Severity` / `SendProfile` | audit | Report = findings + fingerprint `score` + `hard_reject`. |

### Providers

"Providers" = the `provider.rs` validate-and-resolve layer that turns each
`Operation` into a `PreparedOp` by drawing its variant from the **venue catalog
(SSOT)**. Dispatch is *static* over `VenueId`/`OpKind` — a second venue adds a match
arm, never a trait object. This is the gate that makes an invalid tx
**unrepresentable**. It does NOT emit instructions (the pump ix builders are methods
on an initialized `PumpFunTrader` that reads the on-chain `Global` account) — that is
the launcher's Phase-F job. `denom_accepts` encodes the variant⊥amount rule, notably
that an `ExactQuote` SOL budget is valid for *both* buy denominations (a tokens-out
`buy`/`buy_v2` takes it as the `max_sol_cost` ceiling), which is what lets a bundler
leg (and the disguise) rotate across all four buy encodings.

### Dry-run

The "zero-SOL dry-run over the executor stack" is not a separate module — it is the
purity of `prepare` + `audit`: both are pure (no SOL, no network, no signer) and
operate entirely on catalog metadata. `prepare` yields a `PreparedPlan` whose
`MinOut::label` renders the slippage floor as a *policy* string ("computed-at-send @
Nbps"), never a fabricated number — so a dry-run can't "pass" a fill the live tx
would revert. A frozen `min_out` is deliberately absent from the model for exactly
this reason.

### The 10 audit rules

`EqualAmounts` · `SameSlotCluster` · `DirectOwnGraphTokenEdge` · `ConstantCuTip` ·
`SynchronizedBundlerExit` · `NonceReuse` · `DurableNonceMisuse` · `ClusteredAtaClose`
· `UniformLayout` (all `Warn`, waivable) · `AccountShapeIntegrity` (`Reject`,
un-waivable — fee recipient must sit at `PUMP_FEE_RECIPIENT_INDEX = 17`). Build-shape
rules (CU/tip, nonce, ATA, account list) need `SendProfile`s populated at build time;
a pre-build `audit(plan)` still runs the amount/schedule/token-edge rules.

## Consumers

Only **`forge/launcher`** links the crate (`Cargo.toml` dep key `orchestrator`).

| Launcher file | Uses |
|---|---|
| `plan_pipeline.rs` | The **mandatory gate**: `gate(plan, allow_fingerprint) -> GatedPlan`. Runs `prepare` → `disguise_ops` (applies non-locked variants) → re-`prepare` → per-op `IxLayout::validate` → `audit_with` → `passed`. `GatedPlan` is the only thing the executor builds from. Constants `LAUNCH_BUY_VARIANT = "buy_exact_sol_in"`, `DEFAULT_BUNDLE_SLIPPAGE_BPS = 500`. |
| `plan_exec.rs` | Phase-F **executor bridge**: maps a `GatedPlan`'s `Operation`s + `Disguise`s onto the `PumpFunTrader` ix builders. |
| `service.rs` | Assembles launch `Plan`s via `bundle_launch` (`BundlerLeg`/`BundleLaunch`/`WalletRef::managed`/`IdSeq`/`Plan::for_mint`). |
| `manage/execute.rs` | Builds management-action `Plan`s from `Operation`/`macros::DEFAULT_SOL_TRANSFER_VARIANT`. |
| `bundle_execute.rs`, `bundle_simulate.rs`, `bundle.rs` | Deserialize persisted `orchestrator::Plan` JSON and walk `Operation`s (e.g. `bundler_ops`). |
| `core/models/own_launch.rs`, `core/storage/.../own_launch.rs` | Persist/read the plan JSON alongside a launch row. |

## Key rules

- **Orthogonal axes, one shape.** Every trade is an `Operation` on the five axes; no
  bespoke `DevBuy`/`VolumeLeg` op types. `OpKind`/`VenueId` are *re-exported from the
  catalog*, so the op vocabulary can never drift from the on-chain ix table.
- **Illegal tx unrepresentable.** `prepare` is the choke point: off-catalog variant,
  kind≠variant mechanism, amount a variant's denom can't encode, unparseable address,
  dangling dep, or duplicate id → `PlanError`, before any ix/SOL. Fail-fast (first
  defect rejects the whole plan).
- **`min_out` is never stored.** Only `slippage_bps` lives in the plan; the floor is
  late-bound from live reserves at send (`MinOut::Late`). This is what keeps the
  dry-run honest.
- **Plans are serializable.** Addresses are base58 `String`s (not `Pubkey`); the plan
  persists and replays; disguises are *not* persisted — they're recomputed
  deterministically from `(wallet, op_id)`.
- **Personas, not per-field jitter.** A wallet is *assigned* one sticky persona
  (`hash(pubkey) % n`) and every disguise draws from that persona's ranges, so a
  wallet reads as one coherent actor. Per-op RNG seed makes ops of the same wallet
  differ yet stay reproducible.
- **Disguise can raise cost, never starve.** `cu_limit ≥ real_consumption`,
  `cu_price ≥ min > 0`; the chosen variant stays kind+denom+stage compatible, so a
  disguise never produces something `prepare` would reject. Authored (`lock_variant`)
  legs keep their operator-chosen variant but still get fee jitter.
- **Every SOL move is a typed op.** Fund/consolidate emit `OpKind::TransferSol` (not
  raw `system_instruction::transfer`), so the auditor sees every transfer.
- **Audit is mandatory and pure.** Fingerprint `Warn`s are waivable with
  `allow_fingerprint`; a `Reject` (malformed account shape) is not. Zero SOL, zero
  network.
- **Static venue dispatch.** Only `PumpFun` is wired; a new venue is a match arm, not
  a `Box<dyn>`.

## Status

Fully implemented and unit-tested (macros, provider, personas, disguise, audit, rng
all carry `#[cfg(test)]` suites incl. the composed fund→launch→volume→exit→consolidate
"Gate D/E" scenarios). Wired into the launcher's mandatory `gate`. Not a thin
scaffold. Per the crate docs the remaining seam is Phase F real-tx assembly through an
initialized `PumpFunTrader` in `plan_exec.rs`; real-SOL smoke is the outstanding
validation (consistent with the repo's launch-stability memory notes). This crate is
absent from `forge/CLAUDE.md`'s crate table — this doc is its first documentation.
