# Dynamic Ix Layout — Frontend control (P2/P3 + P4/P5, hand-pick surface)

> Follow-up to [dynamic-ix-layout-plan.md](dynamic-ix-layout-plan.md). That doc ships
> P0+P1 (the `DecoStep`/`IxLayout` types + the `assemble()` interpreter) as a **provable
> no-op**. This doc is the operator-facing continuation: it makes variant + layout
> **settable from the frontend**, folding in that doc's deferred P2/P3/P4/P5.

## Context

The redesign lets a launch tx's **shape** — which decoration ixs (`CuLimit`, `CuPrice`,
`CreateAta`, `Tip`) wrap the `Core` create/buy ix, and in what order — vary from data.
P0/P1 built the machinery with the current shape as the canonical default and no operator
surface. This doc answers *"how do I set variant and ix layout dynamically on the
frontend?"* under two decisions taken with the operator:

- **Bundle buy legs → hand-pick per leg.** The operator authors each bundler co-buy's
  **variant** (`buy` / `buy_exact_sol_in` / `buy_v2` / `buy_exact_quote_in`) and its exact
  **step order**. This **revives the currently-dead `leg_structures`** template field as
  the live execution path.
- **Create transaction → step-list editor.** The operator authors the create tx layout by
  toggling/reordering steps, stored as `create_layout` on the template.

### Two "variant" meanings (don't conflate them)

1. **Create variant** (`launch_templates.variant`, e.g. `pumpfun.create_v2`) — already
   frontend-set today; unchanged. This is the *core create ix*, orthogonal to
   `create_layout` (the *wrappers around it*).
2. **Buy variant** (dev-buy + bundler legs) — today **not** operator-set: a constant
   (`LAUNCH_BUY_VARIANT`, [../launcher/src/plan_pipeline.rs:37](../launcher/src/plan_pipeline.rs#L37))
   overwritten per-wallet by the persona **disguise** in `gate`
   ([../orchestrator/src/disguise.rs:126](../orchestrator/src/disguise.rs#L126)). The
   template's `leg_structures[].variant` is authored + stored but **ignored** by the
   current path (dead plumbing — only referenced by its struct def and set to `None` in
   `funding_plan.rs`). This doc makes it live.

### The gotcha the operator must see (load-bearing)

Hand-picking legs lets you make them **byte-identical** — exactly the tell the mandatory
fingerprint auditor rejects. [../orchestrator/src/audit.rs](../orchestrator/src/audit.rs)
already has a **constant CU / tip** rule (Rule 5) and `gate` is **fail-closed** unless
`allow_fingerprint` ([../launcher/src/service.rs:430](../launcher/src/service.rs#L430)). So
this doc also lands the **`UniformLayout`** audit rule (identical `steps` across ≥3 legs)
plus a client-side warning, so uniform hand-picked layouts are surfaced, never silently
shipped.

## Decisions locked

- **Recipes cycle across legs** (`recipe = leg_structures[i % len]`) — the leg count is
  dynamic (`claim_funded` can return fewer than requested), so you author a handful of
  distinct shapes and they distribute across however many wallets are claimed.
- **Empty `leg_structures` ⇒ unchanged persona/canonical behavior** — purely additive
  opt-in; existing templates keep working byte-for-byte.
- **Fingerprint gate stays fail-closed** — uniform hand-picked layouts rejected unless
  `allow_fingerprint`, surfaced by the new `UniformLayout` rule + client hint.
- **Create's dev-buy stays inside the fused `Core` block** — `create_layout` orders only
  the wrappers, never splits `create`/`ata`/`buy`.

---

## Phase A — prerequisite: land P0 + P1

Execute [dynamic-ix-layout-plan.md](dynamic-ix-layout-plan.md) verbatim first — it is the
golden byte-identity no-op and everything below builds on `IxLayout` + `assemble()`.

## Phase B — per-leg variant + layout as the live path (the doc's P2, template-sourced)

Goal: when a template supplies `leg_structures`, the authored variant + layout drive each
bundler leg; when absent, fall back to today's persona disguise + `canonical_buy()`.

**Todos:**

- [ ] **Recipe carries a layout.** Add `layout: Option<Vec<DecoStep>>` to
      `LegStructureRecipe` ([../launcher/src/bundle.rs:36](../launcher/src/bundle.rs#L36)).
      Keep the existing CU/tip/slippage range fields — they become the authored source
      when present (else the disguise supplies them).
- [ ] **`BundleLegParams` carries a layout.** Add `layout: IxLayout` to `BundleLegParams`
      ([../../shared/executor/pumpfun/src/trader/bundle_buy.rs:61](../../shared/executor/pumpfun/src/trader/bundle_buy.rs#L61)).
      This **drops `#[derive(Copy)]`** (a `Vec` isn't `Copy`) — the ripple is contained
      because `leg_params` ([../launcher/src/plan_exec.rs:53](../launcher/src/plan_exec.rs#L53))
      returns by value (the note already flagged in the P0/P1 doc).
- [ ] **Builder consumes it.** `build_bundle_leg_tx` → `build_bundle_v1/v2_buy_ixs` call
      `assemble(&leg.layout, parts)` (from Phase A) instead of the hardcoded `ixs.push`
      sequence.
- [ ] **Thread authored recipe → op.** In `execute_launch`'s bundle assembly
      ([../launcher/src/service.rs:398-428](../launcher/src/service.rs#L398)): set each
      op's `Operation::variant` from its cycled recipe (not the `LAUNCH_BUY_VARIANT`
      constant) and carry its `layout`. Recipes cycle across the claimed legs.
- [ ] **Respect the authored lock in the gate.** When an op has an authored recipe, the
      disguise must **not** overwrite its variant/layout (skip `d.apply_variant(op)` for
      locked ops, [../launcher/src/plan_pipeline.rs:91](../launcher/src/plan_pipeline.rs#L91));
      CU/price/tip come from the recipe ranges when set, else the disguise. Empty
      `leg_structures` ⇒ unchanged persona behavior.
- [ ] **Validate fail-closed.** Call `IxLayout::validate(Buy, is_snipe=true)` on every leg
      layout both (a) at author time in `validate_params`
      ([../live/src/http.rs:216](../live/src/http.rs#L216)) and (b) inside
      `plan_pipeline::gate` before send. A bundler co-buy is a snipe → forces `CuPrice` +
      `Tip`.
- [ ] **Audit `UniformLayout` (the doc's P4).** Add `Rule::UniformLayout` warn finding in
      [../orchestrator/src/audit.rs](../orchestrator/src/audit.rs), firing when ≥3 legs
      share an identical `steps` sequence; waved through only by `allow_fingerprint`.

## Phase C — `create_layout` on the template → create builder (the doc's P3)

**Todos:**

- [ ] Add `create_layout: Option<Vec<DecoStep>>` to `PumpfunTemplateParams`
      ([../launcher/src/service.rs:57](../launcher/src/service.rs#L57)); `validate_params`
      validates it via `IxLayout::validate(Create, is_snipe=false)`.
- [ ] Thread it into `create_token*_inner`
      ([../../shared/executor/pumpfun/src/trader/create.rs](../../shared/executor/pumpfun/src/trader/create.rs)):
      `assemble(&layout, parts)`, defaulting to `canonical_create()` when omitted. The
      dev-buy buy lives *inside* create's fused `Core`; the create step-list only orders
      `CuLimit`/`CuPrice`/`Core`/`Tip` around it.

## Phase D — frontend wire types

[../frontend/src/shared/types.ts](../frontend/src/shared/types.ts): add a `DecoStep` union
(`'cu_limit' | 'cu_price' | 'create_ata' | 'core' | 'tip'`), add `layout?: DecoStep[]` to
`LegStructureRecipe` (line 7), add `create_layout?: DecoStep[]` to `PumpfunTemplateParams`
(line 19). No RTK-Query change — these ride the existing template mutations
([../frontend/src/shared/store/endpoints.ts:66-85](../frontend/src/shared/store/endpoints.ts#L66)).

## Phase E — reusable `IxLayoutEditor` component

New `../frontend/src/shared/components/IxLayoutEditor.tsx`. There is **no drag-drop library**
in the repo (nothing in package.json, no reorder anywhere), so use **move-up / move-down
buttons**, matching the codebase's existing add/remove-row editors. Built from the ui-kit
primitives (`Field`, `Button`, `Badge` from
[../frontend/src/shared/components/ui](../frontend/src/shared/components/ui/index.ts)):

- Ordered steps as chips with ↑/↓/remove + a "+ add step" menu of the absent steps.
- `Core` pinned and required (exactly one, cannot be removed).
- Live client-side validation mirroring `IxLayout::validate`: exactly one `Core`;
  `CreateAta` before `Core`; a buy leg requires `CuPrice` + `Tip` (snipe rule). Inline
  validation like the existing forms.

## Phase F — wire the editor into the template form

[../frontend/src/features/templates/LaunchTemplatesPage.tsx](../frontend/src/features/templates/LaunchTemplatesPage.tsx):

- **Create layout:** one `IxLayoutEditor` (kind=create) in the template form; its steps
  flow into `params.create_layout` at submit (~line 136).
- **Per-leg:** extend the existing leg-structures row editor (~lines 271-302) — each row
  already has a `variant` Select; add an `IxLayoutEditor` (kind=buy) per row feeding
  `layout`. Update [../frontend/src/features/templates/legForm.ts](../frontend/src/features/templates/legForm.ts)
  `legRowToRecipe`/`recipeToLegRow` to carry `layout`.
- **Uniformity hint:** if ≥3 rows share the same `steps`, show a `Banner` tone="warn"
  mirroring the `UniformLayout` audit rule — client-side, informational.

## Phase G — preview (the doc's P5, recommended)

Add resolved step names to `LegStructure` + `display_legs_json`
([../launcher/src/plan_pipeline.rs:161](../launcher/src/plan_pipeline.rs#L161)) so the
Launch Console
([../frontend/src/features/launch/LaunchConsolePage.tsx](../frontend/src/features/launch/LaunchConsolePage.tsx))
shows the exact `variant + [step order]` each wallet will broadcast before submit. Cheap,
and the real "see it's dynamic" payoff.

## Verification (zero real SOL)

1. **Phase A:** the P0/P1 doc's golden byte-identity test + existing size tests
   (`create_v2_dev_buy_v0_with_alt_fits_1232`, etc.). Proves the refactor is a no-op.
2. **Phase B:** unit test — a template with 2 distinct recipes over 5 claimed legs
   produces the cycled variant+layout per leg; `assemble` output matches each recipe's
   steps; `Core` accounts identical across all legs. A recipe omitting `layout` falls back
   to `canonical_buy`; empty `leg_structures` reproduces today's disguise output.
3. **Validation:** `IxLayout::validate` rejects a buy leg missing `Tip`/`CuPrice`,
   0-or-2 `Core`, `CreateAta` after `Core`; `validate_params` rejects a bad template at
   author time; `gate` rejects at send time.
4. **Audit:** `UniformLayout` fires on 3 identical-`steps` legs, passes on diverse ones;
   `gate` fails without `allow_fingerprint`, passes with it.
5. **Frontend:** `npm run build` (tsc) green; author a template with per-leg + create
   layouts, reload the row (round-trips through `params`), see the uniformity banner when
   rows match, confirm the Launch Console preview shows the resolved shapes.
6. Commands: `cargo test -p executor-core -p executor-pumpfun`, `cargo test -p orchestrator`,
   `cargo check -p forge-live -p forge-lab`, `cargo tree -p forge-live` (no new deps),
   frontend `npm run build`.

## Critical files

**Backend:** `../../shared/executor/core/src/ix_layout.rs` (new, Phase A),
`../../shared/executor/pumpfun/src/trader/assemble.rs` (new, Phase A),
`../../shared/executor/pumpfun/src/trader/{bundle_buy.rs,create.rs}`,
[../launcher/src/bundle.rs](../launcher/src/bundle.rs),
[../launcher/src/service.rs](../launcher/src/service.rs),
[../launcher/src/plan_exec.rs](../launcher/src/plan_exec.rs),
[../launcher/src/plan_pipeline.rs](../launcher/src/plan_pipeline.rs),
[../orchestrator/src/audit.rs](../orchestrator/src/audit.rs),
[../orchestrator/src/disguise.rs](../orchestrator/src/disguise.rs),
[../live/src/http.rs](../live/src/http.rs).

**Frontend:** [../frontend/src/shared/types.ts](../frontend/src/shared/types.ts),
`../frontend/src/shared/components/IxLayoutEditor.tsx` (new),
[../frontend/src/features/templates/LaunchTemplatesPage.tsx](../frontend/src/features/templates/LaunchTemplatesPage.tsx),
[../frontend/src/features/templates/legForm.ts](../frontend/src/features/templates/legForm.ts),
[../frontend/src/features/launch/LaunchConsolePage.tsx](../frontend/src/features/launch/LaunchConsolePage.tsx).
