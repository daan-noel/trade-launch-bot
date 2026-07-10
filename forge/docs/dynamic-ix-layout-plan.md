# Dynamic Ix Layout — P0 + P1 (types + interpreter, no behavior change)

> Execute the phases step by step, checking off todos as you go. This supersedes the
> original same-named plan (which predated the ALT/v0 work and the create-only
> clarification).

## Context

forge's executor was redesigned so the **shape** of a launch transaction — which
decoration instructions (compute-unit limit, compute-unit price, buyer-ATA create,
Jito tip) wrap the core create/buy/sell ix, and in what order — can vary from data. A
token create should be expressible as a lean 3-ix `[create, ata, buy]` on one launch
and a full 6-ix `[cu_limit, cu_price, create, ata, buy, tip]` on another, selectable
per launch. This serves two goals: operational control over tx shape, and
anti-fingerprint variation across bundle legs (a decoder fingerprints byte-identical
legs; bots then won't follow).

The redesign delivered the **value** half — `Disguise` draws CU/tip/variant from a
sticky per-wallet `Persona` ([../orchestrator/src/disguise.rs](../orchestrator/src/disguise.rs)).
The **shape** half is still hardcoded: every bundle leg emits the same `ixs.push`
sequence ([../../shared/executor/pumpfun/src/trader/bundle_buy.rs:200](../../shared/executor/pumpfun/src/trader/bundle_buy.rs#L200)),
and create builds its dev-buy by a build-then-strip hack
([../../shared/executor/pumpfun/src/trader/create.rs:221](../../shared/executor/pumpfun/src/trader/create.rs#L221)).

This doc is the **first slice only**: P0 (data types) + P1 (an interpreter that
assembles instructions from a layout, with the current shape as the default). It is a
**provable no-op** — a golden byte-identity test pins the refactored output equal to
today's — and also removes the build-then-strip smell. Persona-driven layout draws,
create-template layouts, the `UniformLayout` audit rule, and persistence (P2–P5) are
follow-ups built on top of this and are **out of scope for this slice** (documented at
the end for continuity).

## Two orthogonal axes: variant vs layout

Keeping these separate is what makes the design clean. **This slice touches only the
layout axis**; the variant axis already works.

| | **Variant** (already dynamic) | **Layout** (this plan) |
|---|---|---|
| Answers | *What* is the core instruction? | *Where* do Core + its wrappers sit? |
| Controls | discriminator + account list + arg encoding (denom) | presence + order of `CuLimit`/`CuPrice`/`CreateAta`/`Tip` |
| SSOT | `CATALOG` rows ([catalog.rs:83](../../shared/executor/pumpfun/src/catalog.rs#L83)) | `IxLayout.steps` (new, P0) |
| Chosen by | `disguise.choose_variant` (buys/sells) or authored (create/transfer) | persona pool / launch template (P2/P3) |
| Fills | `IxParts.core: Vec<Instruction>` (+ `needs_wsol` → extra ATA) | orders `core` + decorations via `assemble()` |
| Touches core ix bytes? | **yes — that is the point** | **never — places it opaque** |

```
  VARIANT ──► picks a CATALOG row ──► builder arm emits the CORE ix (disc+accts+args)
                                          │
                                          ▼   fills the Core slot
  LAYOUT  ──► IxLayout.steps ──► assemble() orders [cu_limit, cu_price, ata, CORE, tip]
```

Both always stay **valid transactions the program accepts**: a variant only swaps the
core ix for another catalog-legal encoding of the *same op* (same kind, a denom the
amount can take, same stage — guarded by `denom_accepts` + the catalog), and a layout
only reorders wrappers around it (guarded by `IxLayout::validate`). Neither can produce
a reverting tx. **P1 needs no variant changes** — the variant already fills the `Core`
slot today; P1 only changes how that slot and its wrappers are ordered.

## The hard boundary (do not cross)

An on-chain ix's account list + discriminator are fixed by the program; reshaping them
just reverts. This layer varies **only** decoration ixs and their order. Specifically:

- The Jito **tip must stay an inline `system_instruction::transfer`** — Jito's auction
  check scans static account keys and does not resolve ALTs
  ([../../shared/executor/pumpfun/src/trader/alt.rs:17](../../shared/executor/pumpfun/src/trader/alt.rs#L17)).
  The `Tip` step already emits an inline transfer; keep it that way (comment it).
- Instruction **reordering is ALT-safe**: v0 compilation matches accounts by pubkey
  regardless of ix order, so `assemble()` operating on the `Vec<Instruction>` *before*
  compilation ([bundle_buy.rs:157](../../shared/executor/pumpfun/src/trader/bundle_buy.rs#L157),
  [create.rs:262](../../shared/executor/pumpfun/src/trader/create.rs#L262)) is unaffected.

## Decisions locked

- **Create core = opaque block; contents vary by whether a dev buy is present.** The
  create layout is always `[CuLimit, CuPrice, Core, Tip]` (or a lean subset); only what
  is *inside* `Core` changes:

  | Launch | Core contents | Full canonical tx | Lean `[Core]` |
  |---|---|---|---|
  | Create **without** dev buy | `[create]` (1 ix) | `[cu_limit, cu_price, create, tip]` | 1-ix create |
  | Create **with** dev buy | `[create, ata, buy]` fused | `[cu_limit, cu_price, create, ata, buy, tip]` | 3-ix |

  Create-only is first-class and the simple case; the fusion only matters when a dev
  buy exists (the program forces the buyer ATA between create and buy). Create's layout
  freedom is only CU-limit / CU-price / tip presence + position around `Core`.
- **Cross-leg diversity is the goal** (a P2+ concern) — so P0/P1 only establish the
  canonical defaults and the machinery; no per-leg variation is drawn yet.
- **Scope = P0 + P1 first**, shipped green before any behavior change.

---

## Phase 0 — data types (`executor_core`, solana-free)

New file `shared/executor/core/src/ix_layout.rs`, exported from
[../../shared/executor/core/src/lib.rs](../../shared/executor/core/src/lib.rs). Plain
data, no solana deps — so the forge orchestrator (already depends on `executor_core`
for `ComputeBudgetCfg`) can name these in P2 without pulling solana.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecoStep { CuLimit, CuPrice, CreateAta, Core, Tip }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IxLayout { pub steps: Vec<DecoStep> }
```

**Todos:**

- [ ] Create `ix_layout.rs` with `DecoStep`, `IxLayout`.
- [ ] `IxLayout::canonical_buy()` → `[CuLimit, CuPrice, CreateAta, Core, Tip]` (matches
      [bundle_buy.rs:200-256](../../shared/executor/pumpfun/src/trader/bundle_buy.rs#L200);
      for a v2 buy, `CreateAta` expands to base ATA **and** WSOL ATA — the interpreter
      emits every ix the `ata` part carries).
- [ ] `IxLayout::canonical_create()` → `[CuLimit, CuPrice, Core, Tip]` (Core = `[create]`
      for create-only, or the fused `[create, ata, buy]` when a dev buy is present —
      same layout, different Core contents).
- [ ] `IxLayout::validate(kind, is_snipe) -> Result<(), &'static str>` enforcing the
      landing-safety rails: (1) exactly one `Core`; (2) `CreateAta` (if present)
      precedes `Core`; (3) a snipe buy must include `CuPrice` and `Tip`; (4) CU steps
      independent (absent ⇒ runtime default budget — allowed for non-snipe legs,
      forbidden for snipes by rule 3).
- [ ] `pub mod ix_layout;` + `pub use ix_layout::{DecoStep, IxLayout};` in `lib.rs`.
- [ ] Unit tests: canonical constants pass validate; each rule 1–4 rejects a crafted
      bad layout; serde round-trips.
- [ ] `cargo check -p executor-core` (or crate name) green.

## Phase 1 — interpreter + builder refactor (no behavior change)

New file `shared/executor/pumpfun/src/trader/assemble.rs` — the interpreter lives in
`pumpfun` because it constructs real ixs (`ComputeBudgetInstruction`,
`system_instruction::transfer`).

```rust
pub struct IxParts {
    pub core: Vec<Instruction>,   // opaque fixed block: [buy] or [create, ata, buy]
    pub ata:  Vec<Instruction>,   // buyer ATA(s); empty when folded into core (create)
    pub cu_limit: u32,
    pub cu_price: u64,
    pub tip_lamports: u64,
    pub tip_account: Pubkey,
    pub payer: Pubkey,
}
pub fn assemble(layout: &IxLayout, parts: IxParts) -> Vec<Instruction> { /* walk steps */ }
```

`assemble` walks `layout.steps`: `CuLimit`/`CuPrice` → push compute-budget ix;
`CreateAta` → extend `parts.ata` (0..2 ixs); `Core` → extend `parts.core` (a block,
extend not push); `Tip` → push inline `system_instruction::transfer(payer, tip_account,
tip_lamports)`.

**Todos:**

- [ ] Create `assemble.rs` with `IxParts` + `assemble`; register `mod assemble;` in
      [trader/mod.rs](../../shared/executor/pumpfun/src/trader/mod.rs).
- [ ] Refactor `build_bundle_v1_curve_buy_ixs` / `build_bundle_v2_buy_ixs`
      ([bundle_buy.rs](../../shared/executor/pumpfun/src/trader/bundle_buy.rs)): keep the
      `vec![AccountMeta…]` + `buy_data` blocks **verbatim** (the fixed core); stop
      hand-pushing cu/ata/tip. Build `IxParts { core: vec![buy_ix], ata:
      account_creation_ixs (+ wsol ata for v2), cu_limit/cu_price/tip from `leg`,
      tip_account: self.engine.jito_tip_account, payer: signer.pubkey() }` and return
      `assemble(&IxLayout::canonical_buy(), parts)`.
- [ ] Refactor `create_token_inner` / `create_token_v2_inner`
      ([create.rs](../../shared/executor/pumpfun/src/trader/create.rs)): build the core
      block, then `assemble(&IxLayout::canonical_create(), parts)` with `ata: vec![]`
      (folded) and tip from `jito_tip_ix(0)`'s amount.
      - **No dev buy:** `Core = [create_ix]` — `append_dev_buy_ixs` already no-ops
        ([create.rs:182-184](../../shared/executor/pumpfun/src/trader/create.rs#L182)).
      - **With dev buy:** `Core = [create_ix, ata_ix, buy_ix]`. **Delete the
        build-then-strip filter** ([create.rs:221-225](../../shared/executor/pumpfun/src/trader/create.rs#L221))
        and its `is_jito_tip_ix`/`compute_budget::id()` dependency — reuse
        `build_curve_buy_ixs`'s account/data block but return just the bare buy ix.
- [ ] **Golden byte-identity test** (load-bearing): for create-only, create+dev-buy, v1
      bundle buy, v2 bundle buy, assert `assemble(canonical_*, parts)` yields a
      `Vec<Instruction>` with identical program ids, accounts, and data to the
      pre-refactor builder. Create-only must reproduce `[cu_limit, cu_price, create,
      tip]` exactly.
- [ ] Confirm existing size tests still pass:
      `bundle_v2_leg_v0_alt_fits_and_beats_legacy`,
      `create_v2_dev_buy_v0_with_alt_fits_1232`, `create_v2_dev_buy_legacy_overflows_1232`.
- [ ] `cargo check -p forge-live -p forge-lab` green; clippy on touched files.
- [ ] Dep partition holds: `cargo tree -p forge-live` (no new deps from `ix_layout`),
      `cargo tree -p forge-lab`.

> **Note for P2:** adding a `layout` field to `BundleLegParams`
> ([bundle_buy.rs:61](../../shared/executor/pumpfun/src/trader/bundle_buy.rs#L61)) will
> drop its `#[derive(Copy)]` (a `Vec` isn't `Copy`). Not in this slice; flagged so it
> isn't a surprise. `leg_params` ([../src/plan_exec.rs:50](../launcher/src/plan_exec.rs#L50))
> returns by value, so the ripple is contained.

## Verification (zero real SOL)

1. Golden byte-identity test (above) is the proof P1 is a no-op.
2. Layout drives shape (forward-looking, using P0 types): a hand-built `IxLayout {
   steps: [CreateAta, Core] }` → 2-ix result, no cu/tip; the 5-step canonical buy →
   full shape. Assert the `Core` block's accounts are **identical** across both.
3. `IxLayout::validate` rejects: snipe missing `Tip`/`CuPrice`; 0-or-2 `Core`;
   `CreateAta` after `Core`.
4. Commands: `cargo test -p executor-pumpfun` (golden + shape), `cargo test
   -p executor-core` (validate), `cargo check -p forge-live -p forge-lab`, `cargo tree`
   partition checks.

---

## Out of scope — follow-up slices (documented for continuity)

- **P2 — persona draw.** `Persona.layouts: Vec<IxLayout>`, `Disguise.layout` drawn in
  `disguise.rs` (seeded rng, then re-validate; fall back to `canonical_buy` on failure),
  thread `layout` into `BundleLegParams` (drops `Copy`), `leg_params` copies it. Now
  bundler legs can vary shape.
- **P3 — create layout.** `create_layout` in the launch template
  (`PumpfunTemplateParams`), parsed + validated, honored by the create builder → 3-ix
  vs 6-ix from config.
- **P4 — audit + safety.** `UniformLayout` warn rule (≥3 legs sharing a `steps`
  sequence) in [../orchestrator/src/audit.rs](../orchestrator/src/audit.rs); wire
  `IxLayout::validate` fail-closed into `plan_pipeline::gate` before send.
- **P5 — persistence/preview.** `LegStructure.layout` (resolved step names) in
  `display_legs_json` ([../launcher/src/plan_pipeline.rs:161](../launcher/src/plan_pipeline.rs#L161))
  so the dry-run preview shows the exact ix shape each wallet will broadcast.
