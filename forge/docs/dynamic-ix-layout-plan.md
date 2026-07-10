# Dynamic Ix Layout — finishing §3e (per-op decoration presence + ordering)

**Goal.** Make the *decoration* instructions around a venue instruction — compute-unit
limit, compute-unit price, ATA-create, Jito tip — **presence-toggleable and
re-orderable per op, from data**, so a token create can be a lean 3-ix
`[create, ata, buy]` on one launch and a 6-ix `[cu_limit, cu_price, create, ata, buy,
tip]` on another, chosen by config/persona rather than hardcoded in the builder.

This closes the **un-built half of §3e** ([and-about-the-instructions-shimmying-shore.md
§3e](and-about-the-instructions-shimmying-shore.md#L198)). Today the redesign delivers
per-op *value* randomization (`Disguise.{cu_limit,cu_price,tip_lamports}`) but the ix
*shape* — which decoration ixs exist and in what order — is fixed in each
`build_*_ixs` function. `LegStructure.ix_order` exists but only records a leg's index in
the bundle ([plan_pipeline.rs:181](../launcher/src/plan_pipeline.rs#L181)); it drives
nothing.

## Scope — the hard boundary (do not cross)

§3e line 204 is law: **an instruction's account list is fixed by the on-chain program;
reshaping it just reverts.** So:

| In scope (decoration — safe to vary) | Out of scope (fixed by program) |
| --- | --- |
| Presence of CU-limit / CU-price / tip / ATA-create ixs | The account vector of the pump `create`/`buy`/`sell` ix |
| Order of those decoration ixs relative to the core ix | The discriminator + arg encoding of the core ix (that's the *variant* knob, already dynamic) |
| Tip placement (which position, still one tip transfer) | A runtime interpreter for arbitrary accounts (explicitly rejected in §3e) |

The core venue ix stays a **black box the layout places but never reshapes**.

## The model

One new type pair, shared low (define in `shared/executor/core` so both the pumpfun
builders and the orchestrator can name it; orchestrator re-exports).

```rust
// shared/executor/core/src/ix_layout.rs  (new)

/// One slot in a tx's decoration recipe. `Core` is the venue instruction
/// (create/buy/sell) — opaque, program-fixed, MUST appear exactly once. The rest are
/// wrapper ixs whose presence + position are free to vary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DecoStep {
    CuLimit,    // ComputeBudgetInstruction::set_compute_unit_limit
    CuPrice,    // ComputeBudgetInstruction::set_compute_unit_price
    CreateAta,  // idempotent buyer ATA create (curve v1 / v2 / dev-buy)
    Core,       // the venue ix — placed here, never reshaped
    Tip,        // system transfer to the Jito tip account
}

/// An ordered decoration recipe. `steps` is the literal emit order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IxLayout { pub steps: Vec<DecoStep> }
```

`IxLayout` gives presence (absent ⇒ omitted) and order (position) in one `Vec`. The
current hardcoded shape becomes the default constant:

```rust
impl IxLayout {
    /// The shape the builders emit today — the safe default when no layout is chosen.
    pub fn canonical_buy()    -> Self { steps![CuLimit, CuPrice, CreateAta, Core, Tip] }
    pub fn canonical_create() -> Self { steps![CuLimit, CuPrice, Core, CreateAta, Tip] } // dev-buy ATA+buy fold into Core-adjacent
}
```

### Landing-safety rails (real SOL — non-negotiable)

A layout must never make a launch lose. `IxLayout::validate(op_kind, intent)` rejects a
layout unless:

1. **Exactly one `Core`.** Zero or many is a malformed tx.
2. **`CreateAta` (if present) precedes `Core`.** The buy needs its ATA to exist first —
   this is a real runtime dependency, not cosmetics.
3. **A `Snipe` buy must include `CuPrice` and `Tip`.** Dropping either forfeits landing
   / bundle inclusion. (Mirrors the disguise's existing "a snipe always tips, never
   zero-fees" guarantee — [disguise.rs:18](../orchestrator/src/disguise.rs#L18).)
4. **CU value only emitted with its ix.** If `CuLimit`/`CuPrice` is absent the tx runs
   at the runtime default budget — allowed for non-snipe/volume decoy legs, forbidden
   for snipes by rule 3.

Validation runs at **plan-gate time** (fail-closed, before any send), alongside the
existing audit.

## The interpreter — one place assembles, builders only make parts

Refactor each builder so the account-vector+data construction returns **one
`Instruction`**, and a single interpreter walks the layout. This is the change that
turns "hardcoded push sequence" into "data-driven emit."

```rust
// shared/executor/pumpfun/src/trader/assemble.rs  (new — the interpreter)

/// The concrete, already-built ingredients for one tx. The core ix is prebuilt by the
/// per-variant builder (accounts fixed); the interpreter only *orders* them.
pub struct IxParts {
    pub core: Instruction,               // build_core_v1_buy_ix / _v2 / _create_ix
    pub ata: Option<Vec<Instruction>>,   // account-creation ixs (may be >1: WSOL etc.)
    pub cu_limit: u32,
    pub cu_price: u64,
    pub tip_lamports: u64,
    pub tip_account: Pubkey,
    pub payer: Pubkey,
}

pub fn assemble(layout: &IxLayout, parts: IxParts) -> Vec<Instruction> {
    let mut ixs = Vec::with_capacity(layout.steps.len() + 1);
    for step in &layout.steps {
        match step {
            DecoStep::CuLimit  => ixs.push(ComputeBudgetInstruction::set_compute_unit_limit(parts.cu_limit)),
            DecoStep::CuPrice  => ixs.push(ComputeBudgetInstruction::set_compute_unit_price(parts.cu_price)),
            DecoStep::CreateAta => ixs.extend(parts.ata.clone().unwrap_or_default()),
            DecoStep::Core     => ixs.push(parts.core.clone()),
            DecoStep::Tip      => ixs.push(system_instruction::transfer(&parts.payer, &parts.tip_account, parts.tip_lamports)),
        }
    }
    ixs
}
```

Then the existing builders shrink to "make the core ix + parts, call `assemble`":

- [`build_bundle_v1_curve_buy_ixs`](../../shared/executor/pumpfun/src/trader/bundle_buy.rs#L182)
  and `_v2_buy_ixs`: keep the `vec![AccountMeta…]` + `buy_data` block **verbatim**
  (that's the fixed core), but wrap it as `IxParts.core` and return
  `assemble(&params.layout, parts)` instead of the hand-rolled `ixs.push` sequence.
- [`create.rs`](../../shared/executor/pumpfun/src/trader/create.rs) `create_token_inner`
  / `create_token_v2_inner`: same — `build_create_ix` becomes the `core`, the dev-buy
  ATA + buy become `ata`/a fused core, and `assemble` orders them. (Note: create+dev-buy
  is one atomic tx; the dev-buy's own buy ix rides alongside the create as core-adjacent
  — keep the current fusion, just make CU/tip placement layout-driven.)

**Guarantee to preserve:** the default layouts (`canonical_*`) must produce a
**byte-identical** tx to today's builders. A golden test pins this (see Verification) so
the refactor is provably a no-op until a non-default layout is chosen.

## Where a layout comes from

Following the §3e "persona for legs, template for create" split:

**Bundler / volume legs → the persona (disguise).** Add a layout pool to `Persona`,
draw one per op into `Disguise`, thread it through:

```rust
// personas.rs
pub struct Persona { …, pub layouts: Vec<IxLayout> }   // pool of allowed shapes

// disguise.rs  Disguise struct gains:
pub layout: IxLayout,        // drawn in draw(): rng.index(persona.layouts) or canonical fallback
```

`draw()` ([disguise.rs:86](../orchestrator/src/disguise.rs#L86)) picks a layout the same
seeded way it already picks a variant, then **re-validates it against the op** (rule 3
forces snipe CuPrice+Tip; if the drawn layout fails, fall back to `canonical_buy()` —
never emit an unsafe shape). `leg_params` ([plan_exec.rs:50](../launcher/src/plan_exec.rs#L50))
copies `disguise.layout` into `BundleLegParams.layout`.

**Create tx → the launch template.** Create isn't disguised (`choose_variant` returns
early for non-buy/sell — [disguise.rs:132](../orchestrator/src/disguise.rs#L132)), so its
layout comes from `launch_templates.params`:

```jsonc
// launch_templates.params
{ "create_layout": ["cu_limit","cu_price","core","create_ata","tip"],  // or ["core","create_ata"] for lean 3-ix
  "leg_layouts": [ … optional per-template pool that seeds the persona draw … ] }
```

`PumpfunTemplateParams` ([service.rs](../launcher/src/service.rs)) parses `create_layout`
(default `canonical_create()`), validates it, and passes it to the create builder.

## Audit — layout is a fingerprint surface

Add one rule to the gate ([audit.rs](../orchestrator/src/audit.rs)), same spirit as
`ConstantCuTip`:

- **`UniformLayout` (Warn, w30):** ≥3 disguised legs sharing an identical `steps`
  sequence — a decoder fingerprints "all bundle legs have the same ix shape." Overridable
  with `allow_fingerprint` like the other Warn rules.

The existing `AccountShapeIntegrity` (Reject, fee-recipient at index 17) is unaffected —
it inspects the **core** ix's accounts, which the layout never touches. Good: the two
concerns stay orthogonal (layout = wrapper order, integrity = core accounts).

## Persistence & preview

`bundles.legs` is JSONB — **no migration.** Extend the persisted descriptor:

```rust
// launcher/src/bundle.rs  LegStructure gains:
pub layout: Vec<String>,   // the resolved step names, e.g. ["cu_limit","cu_price","core","tip"]
```

`display_legs_json` ([plan_pipeline.rs:161](../launcher/src/plan_pipeline.rs#L161)) fills
it from `disguise.layout`, so the dry-run preview and the stored plan show the exact ix
shape each wallet will broadcast — the operator sees "3-ix" vs "6-ix" before anything
sends. Drop the now-vestigial `ix_order` field (or keep it as the leg index; it's
independent).

## Task list (phased, each independently shippable & green)

- [ ] **P0 — types.** `shared/executor/core/src/ix_layout.rs`: `DecoStep`, `IxLayout`,
  `canonical_*`, `validate`. Orchestrator re-exports. Unit tests for validate rules 1–4.
- [ ] **P1 — interpreter, no behavior change.** `trader/assemble.rs` + refactor
  `bundle_buy.rs` and `create.rs` to build `IxParts` + call `assemble` with the
  `canonical_*` default. **Golden byte-identity test** vs pre-refactor tx. Add
  `layout: IxLayout` to `BundleLegParams` (default `canonical_buy`).
- [ ] **P2 — persona draw.** `Persona.layouts`, `Disguise.layout`, draw+revalidate in
  `disguise.rs`, thread via `leg_params`. Now bundler legs can vary shape.
- [ ] **P3 — create layout.** `create_layout` in `PumpfunTemplateParams`, honored by the
  create builder. Now a create can be 3-ix or 6-ix from the template.
- [ ] **P4 — audit + safety.** `UniformLayout` rule; wire `IxLayout::validate` into
  `plan_pipeline::gate` (fail-closed before send).
- [ ] **P5 — persistence/preview.** `LegStructure.layout`; `display_legs_json` fills it;
  dry-run shows resolved shapes.
- [ ] **P6 — frontend (optional).** Surface `create_layout` / `leg_layouts` in the launch
  & template editors; show the resolved shape per leg on the launch console.

## Verification (zero real SOL)

1. **Golden byte-identity (P1, the load-bearing test):** for create / v1 buy / v2 buy,
   `assemble(canonical_*, parts)` produces the exact same `Vec<Instruction>` (program
   ids, accounts, data) as the current builder. This proves the refactor is a no-op.
2. **Layout drives shape:** feed `["core","create_ata"]` → assert a 2-decoration tx with
   no CU/tip ixs; feed the 6-step create → assert full shape. Assert core ix accounts
   are **identical** across both (the boundary holds).
3. **Safety rejects:** a snipe leg with a `Tip`-less layout is rejected by `validate`;
   `draw` falls back to `canonical_buy`. A layout with 0 or 2 `Core` is rejected.
4. **Dep partition unchanged:** `ix_layout` is pure (no solana in `core`? — it needs
   `ComputeBudgetInstruction`/`system_instruction`, so keep the *interpreter* in
   `pumpfun`, only the `DecoStep`/`IxLayout` **data types** in `core`). Re-check
   `cargo tree -p forge-live` / `-p forge-lab`.
5. **Simulate:** run the built tx through the existing `simulate_ixs` path (dry-run
   subcommand) to confirm a non-default layout still lands on-chain before spending SOL.

## Open decisions

1. **Where do `DecoStep`/`IxLayout` live?** `core` for the data types keeps the
   orchestrator solana-free; the **interpreter** (`assemble`) must live in `pumpfun`
   (it constructs real ixs). Confirm this split vs. putting both in `pumpfun` and having
   the orchestrator carry only step *names*.
2. **Layout pool authoring:** hand-authored per persona/template now, or a generator
   that enumerates safe permutations? Start hand-authored (a handful of shapes),
   generator later.
3. **Should sells/transfers get layouts too?** Sells already default no-tip; a transfer
   is one ix. Low value — defer; the design covers them for free if wanted.
4. **`create_ata` for the dev-buy inside create:** the fused create+dev-buy has its own
   ATA + buy. Decide whether those are one `Core` blob or separate `CreateAta`+`Core`
   steps (affects how granular create-layout control is). Recommend: keep dev-buy fused
   into `Core`, expose only CU/tip placement for create in P3; split later if needed.
</content>
</invoke>
