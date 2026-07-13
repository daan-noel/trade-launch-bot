//! Phase 2.F — the **mandatory plan gate** every forge write flow passes through.
//!
//! The launcher/manage flows no longer hand-roll instructions from free-text
//! variant strings. Instead each flow assembles an [`orchestrator::Plan`] (via the
//! macros — `bundle_launch` / `volume_make` / `exit` / `consolidate` / `fund`),
//! then calls [`gate`], which:
//!
//!   1. **validates** the plan against the venue catalog ([`orchestrator::prepare`])
//!      — an off-catalog variant, a kind≠variant, or an amount a variant's denom
//!      can't encode is rejected here, before any ix is built or any SOL moves;
//!   2. **disguises** every op (sticky per-wallet persona → landing CU/tip/variant),
//!      applying the persona-chosen encoding to the plan so what's audited is what
//!      sends. Disguises are a *pure, reproducible* function of the wallet + op, so
//!      a persisted plan re-derives the identical disguises on replay — only the
//!      [`Plan`] itself is persisted;
//!   3. runs the **fingerprint auditor** ([`orchestrator::audit_with`]) over the
//!      disguised plan and its send profiles, and **refuses to return** a plan that
//!      doesn't pass (a hard-reject — malformed account shape — is never
//!      overridable; other tells require `allow_fingerprint`).
//!
//! The executor ([`crate::plan_exec`]) then builds the real txs from a [`GatedPlan`]
//! through an initialized `PumpFunTrader` — mapping each op to the *same* proven ix
//! builders the pre-cutover code used, so the on-chain bytes are unchanged; only the
//! selection/validation layer moved onto the catalog + audit.

use anyhow::{bail, Result};
use orchestrator::{
    audit_with, disguise_ops, prepare, AuditConfig, AuditReport, Disguise, OpId, Operation, Plan,
    PersonaSet, SendProfile,
};
use pump_trader::BundleBuyVariant;

/// The buy encoding every launch dev-buy + bundler co-buy uses: SOL-in, min_out
/// floored. Fixing this to the `ExactQuote` encoding is the structural half of the
/// overflow cure (the tokens-out `ExactBase` encoding — whose max-cost calc could
/// overflow — is never chosen for our own launch buys).
pub const LAUNCH_BUY_VARIANT: &str = "buy_exact_sol_in";

/// Fallback bundler-leg slippage when a template carries none (kept equal to the
/// historical composer default so a leg's min_out floor is unchanged).
pub const DEFAULT_BUNDLE_SLIPPAGE_BPS: u64 = 500;

/// A plan that has passed the mandatory gate: the (disguise-applied) plan, the
/// per-op disguises aligned to `plan.ops`, and the audit report captured at the
/// gate. This is the ONLY thing the executor will build txs from.
#[derive(Debug, Clone)]
pub struct GatedPlan {
    pub plan: Plan,
    pub disguises: Vec<Disguise>,
    pub audit: AuditReport,
}

impl GatedPlan {
    /// The disguise for a given op id (executor looks it up per op).
    pub fn disguise_of(&self, op_id: OpId) -> Option<&Disguise> {
        self.plan
            .ops
            .iter()
            .position(|o| o.id == op_id)
            .and_then(|i| self.disguises.get(i))
    }

    /// The plan, serialized for persistence (`bundles.plan` / `manage_actions.
    /// plan_orchestrator`). Disguises are NOT persisted — they're recomputed
    /// deterministically on replay from the plan's wallets + op ids.
    pub fn plan_json(&self) -> serde_json::Value {
        serde_json::to_value(&self.plan).unwrap_or(serde_json::Value::Null)
    }

    /// The audit report, serialized for persistence (`*.audit`) — display only.
    pub fn audit_json(&self) -> serde_json::Value {
        serde_json::to_value(&self.audit).unwrap_or(serde_json::Value::Null)
    }
}

/// Run the mandatory gate over `plan`. `allow_fingerprint` waves through the
/// fingerprint *tells* (equal amounts, star funding, …) but never a hard reject
/// (a reshaped account list). Returns the gated plan or an error naming the defect.
pub fn gate(plan: Plan, allow_fingerprint: bool) -> Result<GatedPlan> {
    // 1) Validate against the catalog. Fail-fast on the first illegal op.
    prepare(&plan).map_err(|e| anyhow::anyhow!("plan is not buildable: {e}"))?;

    // 2) Disguise: draw a sticky-persona landing shape for every op, then APPLY the
    //    chosen encoding to the plan so the audit + build see the real variant.
    //    (Disguise only swaps a buy/sell to a denom-compatible sibling; create /
    //    transfer keep their authored variant.)
    let personas = PersonaSet::builtin();
    let cfg = executor_core::config::ComputeBudgetCfg::default();
    let mut plan = plan;
    let disguises = disguise_ops(&personas, &plan.ops, &cfg);
    for (op, d) in plan.ops.iter_mut().zip(&disguises) {
        // A hand-picked (authored) leg keeps its operator-chosen variant — the
        // disguise must not swap it. CU/price/tip still come from the disguise, so
        // even a locked leg keeps anti-fingerprint fee jitter.
        if !op.lock_variant {
            d.apply_variant(op);
        }
    }
    // Re-validate: a disguise is denom-safe by construction, but assert it so a
    // future persona bug can't slip an unbuildable variant past the gate.
    prepare(&plan).map_err(|e| anyhow::anyhow!("disguised plan is not buildable: {e}"))?;

    // Fail-closed layout validation: any authored ix layout must satisfy the
    // landing-safety rails (exactly one Core; CreateAta precedes Core; a snipe buy
    // includes CuPrice + Tip). A bundler co-buy is a snipe.
    for op in &plan.ops {
        if let Some(steps) = &op.layout {
            use orchestrator::{Intent, OpKind};
            let layout = executor_core::IxLayout { steps: steps.clone() };
            let kind = match op.kind {
                OpKind::Create => executor_core::LayoutKind::Create,
                _ => executor_core::LayoutKind::Buy,
            };
            let is_snipe = op.intent == Intent::Snipe;
            layout
                .validate(kind, is_snipe)
                .map_err(|e| anyhow::anyhow!("op {} has an invalid ix layout: {e}", op.id))?;
        }
    }

    // 3) Fingerprint audit over the disguised plan + its send profiles.
    let profiles = send_profiles(&plan, &disguises);
    let audit = audit_with(&plan, &profiles, AuditConfig::default());
    if !audit.passed(allow_fingerprint) {
        let tells: Vec<String> = audit
            .findings
            .iter()
            .map(|f| format!("{:?}({:?}): {}", f.rule, f.severity, f.message))
            .collect();
        bail!(
            "plan rejected by fingerprint auditor (hard_reject={}, score={}): {}",
            audit.hard_reject,
            audit.score,
            tells.join("; ")
        );
    }

    Ok(GatedPlan { plan, disguises, audit })
}

/// Build the per-op [`SendProfile`]s the auditor inspects from the drawn disguises.
/// The build-shape fields (account list / fee recipient / nonce) are left default:
/// the executor uses the canonical pump ix builders (never a hand-assembled account
/// list), so the account-shape-integrity hard-reject can't structurally arise — the
/// disguise-driven rules (constant CU/tip, etc.) are what a Plan-time gate can see.
fn send_profiles(plan: &Plan, disguises: &[Disguise]) -> Vec<SendProfile> {
    plan.ops
        .iter()
        .zip(disguises)
        .map(|(op, d)| SendProfile { disguise: Some(d.clone()), ..SendProfile::new(op.id) })
        .collect()
}

/// Map a catalog buy variant name → the executor's [`BundleBuyVariant`] the Jito
/// leg builder consumes. Only the SOL-in / tokens-out curve encodings the disguise
/// can draw for a bundler buy are valid here; anything else is a bug (the gate
/// already rejected off-catalog names, so this only guards the leg-builder mapping).
pub fn bundle_buy_variant(name: &str) -> Result<BundleBuyVariant> {
    Ok(match name {
        "buy" => BundleBuyVariant::Buy,
        "buy_exact_sol_in" => BundleBuyVariant::BuyExactSolIn,
        "buy_v2" => BundleBuyVariant::BuyV2,
        // The v2 exact-quote encoding — BundleBuyVariant::BuyExactQuoteIn resolves
        // to the v2 account layout when cashback is on, else the v1 SOL-in path
        // (both SOL-in, min_out-floored — overflow-safe).
        "buy_exact_quote_in_v2" => BundleBuyVariant::BuyExactQuoteIn,
        other => bail!("variant {other:?} is not a bundle-buy encoding"),
    })
}

/// Whether an op is a launch-bundle co-buyer leg (a `Bundler`-role snipe buy that
/// `deps` the create) — the set the Jito bundle submits together.
pub fn is_bundler_leg(op: &Operation) -> bool {
    use orchestrator::{OpKind, Role};
    op.kind == OpKind::Buy && op.role == Role::Bundler
}

/// Project a gated launch plan's **bundler legs** into the legacy `bundles.legs`
/// display shape (`BundledLegPlan[]`), so the confirm watcher + operator UI keep
/// reading the per-leg descriptor unchanged. The executable SSOT is the persisted
/// `Plan` (`bundles.plan`); this is a read-only projection derived from it (variant
/// + amount from the op, CU/tip from the disguise) — the two can't drift because
/// both come from the same gated plan.
pub fn display_legs_json(gated: &GatedPlan) -> serde_json::Value {
    use crate::bundle::{BundledLegPlan, LegStructure};
    let legs: Vec<BundledLegPlan> = gated
        .plan
        .ops
        .iter()
        .filter(|op| is_bundler_leg(op))
        .enumerate()
        .filter_map(|(ix, op)| {
            let d = gated.disguise_of(op.id)?;
            let quote = crate::plan_exec::buy_lamports(op).ok()?;
            // The exact ix shape this leg will broadcast: the authored layout, or
            // the canonical buy shape — resolved to step names for the preview.
            let layout = op
                .layout
                .clone()
                .map(|steps| executor_core::IxLayout { steps })
                .unwrap_or_else(executor_core::IxLayout::canonical_buy);
            let layout_names = layout.steps.iter().map(|s| s.as_str().to_string()).collect();
            Some(BundledLegPlan {
                managed_wallet_id: op.wallet.managed_wallet_id?,
                quote_amount: quote as i64,
                structure: LegStructure {
                    variant: op.variant.clone(),
                    slippage_bps: op.slippage_bps.unwrap_or(DEFAULT_BUNDLE_SLIPPAGE_BPS),
                    cu_limit: d.cu_limit,
                    cu_price: d.cu_price_micro_lamports,
                    tip_quote: d.tip_lamports.unwrap_or(0) as i64,
                    ix_order: ix as u8,
                    layout: layout_names,
                },
            })
        })
        .collect();
    serde_json::json!(legs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use orchestrator::{
        bundle_launch, consolidate, BundleLaunch, BundlerLeg, IdSeq, Intent, Operation, Plan, Role,
        Sweep, WalletRef,
    };
    use solana_sdk::signature::{Keypair, Signer};

    fn addr() -> String {
        Keypair::new().pubkey().to_string()
    }
    fn w() -> WalletRef {
        WalletRef::managed(uuid::Uuid::new_v4(), addr())
    }

    fn launch_plan(dev_buy: u64, bundler_sols: &[u64]) -> Plan {
        let mut seq = IdSeq::new(0);
        let ops = bundle_launch(
            &mut seq,
            &BundleLaunch {
                mint: addr(),
                dev: w(),
                create_variant: "create_v2".to_string(),
                buy_variant: LAUNCH_BUY_VARIANT.to_string(),
                dev_buy_variant: LAUNCH_BUY_VARIANT.to_string(),
                dev_buy_sol: dev_buy,
                dev_slippage_bps: Some(500),
                bundlers: bundler_sols
                    .iter()
                    .map(|&sol| BundlerLeg { wallet: w(), sol, slippage_bps: Some(500), variant: None, layout: None })
                    .collect(),
            },
        );
        let mint = ops[0].target.clone().unwrap();
        Plan::for_mint(mint, ops)
    }

    /// A launch bundle (create + dev-buy + equal-amount bundlers) passes the gate
    /// with fingerprint tells allowed (the default), and the gated plan JSON
    /// round-trips back into a plan that re-gates identically (persist/replay).
    #[test]
    fn launch_plan_gates_and_round_trips() {
        let plan = launch_plan(1_500_000, &[800_000, 800_000, 800_000]);
        let gated = gate(plan, true).expect("equal-amount launch must gate with allow_fingerprint");
        // Disguise applied every buy op a concrete (denom-safe) variant.
        for op in gated.plan.ops.iter().filter(|o| is_bundler_leg(o) || is_dev_buy_local(o)) {
            assert!(spec_is_buy(&op.variant), "buy op variant stays a buy encoding");
        }
        // Persist → replay.
        let json = gated.plan_json();
        let back: Plan = serde_json::from_value(json).expect("plan JSON round-trips");
        let regated = gate(back, true).expect("replayed plan re-gates");
        assert_eq!(regated.plan.ops.len(), gated.plan.ops.len());
        // Deterministic disguises: the same plan re-gates to the same variants.
        for (a, b) in gated.plan.ops.iter().zip(&regated.plan.ops) {
            assert_eq!(a.variant, b.variant, "disguise is reproducible across replay");
        }
    }

    /// A launch whose `leg_structures` mixes an authored tokens-out `"buy"` with a
    /// SOL-in `"buy_exact_sol_in"` on SOL-denominated (`Amount::ExactQuote`) legs
    /// GATES cleanly: a SOL budget is a valid `max_sol_cost` for a tokens-out buy, so
    /// `denom_accepts` allows it and the executor derives the token amount from
    /// reserves at build time. (Regression guard: this used to be rejected with a
    /// `DenomMismatch`, which surfaced as an "internal server error" mid-launch.)
    #[test]
    fn authored_tokens_out_buy_variant_now_gates() {
        let mut seq = IdSeq::new(0);
        let ops = bundle_launch(
            &mut seq,
            &BundleLaunch {
                mint: addr(),
                dev: w(),
                create_variant: "create_v2".to_string(),
                buy_variant: LAUNCH_BUY_VARIANT.to_string(),
                dev_buy_variant: LAUNCH_BUY_VARIANT.to_string(),
                dev_buy_sol: 20_000_000,
                dev_slippage_bps: None,
                bundlers: vec![
                    BundlerLeg {
                        wallet: w(),
                        sol: 10_000_000,
                        slippage_bps: None,
                        variant: Some("buy_exact_sol_in".to_string()),
                        layout: None,
                    },
                    // authored tokens-out "buy" on a SOL budget — now valid
                    BundlerLeg {
                        wallet: w(),
                        sol: 10_000_000,
                        slippage_bps: None,
                        variant: Some("buy".to_string()),
                        layout: None,
                    },
                    // and "buy_v2" (also tokens-out)
                    BundlerLeg {
                        wallet: w(),
                        sol: 10_000_000,
                        slippage_bps: None,
                        variant: Some("buy_v2".to_string()),
                        layout: None,
                    },
                ],
            },
        );
        let mint = ops[0].target.clone().unwrap();
        let plan = Plan::for_mint(mint, ops);
        let gated = gate(plan, true).expect("authored tokens-out buy legs must gate");
        // The two authored tokens-out legs kept their variant (locked, disguise
        // can't swap an authored leg); the plan is buildable.
        let buy_legs: Vec<&str> = gated
            .plan
            .ops
            .iter()
            .filter(|o| is_bundler_leg(o))
            .map(|o| o.variant.as_str())
            .collect();
        assert!(buy_legs.contains(&"buy"), "the authored `buy` leg survives gating: {buy_legs:?}");
        assert!(buy_legs.contains(&"buy_v2"), "the authored `buy_v2` leg survives gating: {buy_legs:?}");
    }

    /// A varied-amount launch (distinct buys) passes even the STRICT gate — no
    /// equal-amounts / constant-fee / star tells to trip.
    #[test]
    fn varied_launch_passes_strict_gate() {
        let plan = launch_plan(1_500_000, &[700_000, 850_000, 1_050_000]);
        let gated = gate(plan, false).expect("varied-amount launch passes strict gate");
        assert!(!gated.audit.hard_reject);
    }

    /// The catalog→BundleBuyVariant map accepts every SOL-in/tokens-out curve buy
    /// encoding and rejects a non-buy name.
    #[test]
    fn bundle_buy_variant_maps_and_rejects() {
        assert!(bundle_buy_variant("buy_exact_sol_in").is_ok());
        assert!(bundle_buy_variant("buy").is_ok());
        assert!(bundle_buy_variant("buy_v2").is_ok());
        assert!(bundle_buy_variant("buy_exact_quote_in_v2").is_ok());
        assert!(bundle_buy_variant("sell").is_err());
        assert!(bundle_buy_variant("create_v2").is_err());
    }

    /// `display_legs_json` projects exactly the bundler legs (one per bundler) with
    /// a managed wallet id + a buy variant + the disguise's CU/tip.
    #[test]
    fn display_legs_projects_bundlers() {
        let plan = launch_plan(1_000_000, &[500_000, 600_000]);
        let gated = gate(plan, true).unwrap();
        let legs: Vec<crate::bundle::BundledLegPlan> =
            serde_json::from_value(display_legs_json(&gated)).unwrap();
        assert_eq!(legs.len(), 2, "one display leg per bundler");
        for leg in &legs {
            assert!(spec_is_buy(&leg.structure.variant));
            assert!(leg.quote_amount > 0);
        }
    }

    /// A consolidate plan (typed `TransferSol` sweeps to treasury) gates — every
    /// internal SOL move is now a validated, auditable op, not a raw transfer.
    #[test]
    fn consolidate_plan_gates() {
        let treasury = addr();
        let mut seq = IdSeq::new(0);
        let ops = consolidate(
            &mut seq,
            &treasury,
            &[
                Sweep { wallet: w(), role: Role::Volume, lamports: 2_000_000 },
                Sweep { wallet: w(), role: Role::Bundler, lamports: 3_100_000 },
            ],
            &[],
        );
        let plan = Plan::for_mint(addr(), ops);
        let gated = gate(plan, true).expect("consolidate plan gates");
        assert!(gated
            .plan
            .ops
            .iter()
            .all(|o| o.intent == Intent::Consolidate));
    }

    // ---- test helpers ----
    fn is_dev_buy_local(op: &Operation) -> bool {
        use orchestrator::OpKind;
        op.kind == OpKind::Buy && op.role == Role::Dev
    }
    fn spec_is_buy(variant: &str) -> bool {
        matches!(
            variant,
            "buy" | "buy_exact_sol_in" | "buy_v2" | "buy_exact_quote_in_v2"
        )
    }
}
