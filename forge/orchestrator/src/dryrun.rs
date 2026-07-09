//! Dry-run — the zero-SOL preview + test hook. Turn a validated [`PreparedPlan`]
//! into a serializable [`DryRunReport`]: one line per op (mechanism / variant /
//! role / intent / wallet / amount / `min_out` policy / deps) plus roll-ups. This
//! is the surface the operator previews before executing and the fixture the tests
//! assert on — it never touches the network or a signer.
//!
//! `min_out` is shown as its **policy** ("computed-at-send @ Nbps"), never a
//! concrete number: the floor is late-bound from live reserves and does not exist
//! until the tx is signed (Phase F). Concrete signed txs + a `simulate_ixs`
//! "would-land" check need an initialized `PumpFunTrader` and are exercised at the
//! Phase F cutover.

use crate::plan::{Amount, Operation, Plan, VenueId};
use crate::provider::{prepare, PlanError, PreparedOp, PreparedPlan};
use serde::Serialize;

/// One op, flattened for preview.
#[derive(Debug, Clone, Serialize)]
pub struct DryRunOp {
    pub id: u32,
    pub kind: String,
    pub venue: String,
    pub variant: String,
    pub role: String,
    pub intent: String,
    pub wallet: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Human amount description (e.g. `"exact_quote 1500000 lamports"`).
    pub amount: String,
    /// The `min_out` policy label — never a frozen number.
    pub min_out: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub deps: Vec<u32>,
}

/// A previewable plan summary + roll-ups. `total_quote_spend` sums only the
/// statically-known quote legs (an `ExactBase` buy's spend is a live ceiling, so
/// it's annotated computed-at-send, not summed).
#[derive(Debug, Clone, Serialize)]
pub struct DryRunReport {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mint_address: Option<String>,
    pub ops: Vec<DryRunOp>,
    pub n_ops: usize,
    pub n_create: usize,
    pub n_buy: usize,
    pub n_sell: usize,
    pub n_transfer: usize,
    pub total_quote_spend: u64,
    pub total_base: u64,
}

/// Validate `plan` and render its dry-run report. Returns the first [`PlanError`]
/// if any op is unbuildable — a plan that dry-runs clean is one every provider
/// accepted. Pure: zero SOL, zero network.
pub fn dry_run(plan: &Plan) -> Result<DryRunReport, PlanError> {
    let prepared = prepare(plan)?;
    Ok(report(&prepared))
}

/// Render an already-prepared plan (skips re-validation).
pub fn report(prepared: &PreparedPlan) -> DryRunReport {
    let ops: Vec<DryRunOp> = prepared.ops.iter().map(render_op).collect();

    let n_create = count_kind(prepared, crate::plan::OpKind::Create);
    let n_buy = count_kind(prepared, crate::plan::OpKind::Buy);
    let n_sell = count_kind(prepared, crate::plan::OpKind::Sell);
    let n_transfer = prepared
        .ops
        .iter()
        .filter(|p| {
            matches!(
                p.op.kind,
                crate::plan::OpKind::TransferSol | crate::plan::OpKind::TransferToken
            )
        })
        .count();

    let total_quote_spend = prepared.ops.iter().map(|p| p.op.amount.quote_spend()).sum();
    let total_base = prepared.ops.iter().map(|p| p.op.amount.base_amount()).sum();

    DryRunReport {
        mint_address: prepared.mint_address.map(str::to_string),
        n_ops: ops.len(),
        ops,
        n_create,
        n_buy,
        n_sell,
        n_transfer,
        total_quote_spend,
        total_base,
    }
}

fn count_kind(p: &PreparedPlan, kind: crate::plan::OpKind) -> usize {
    p.ops.iter().filter(|o| o.op.kind == kind).count()
}

fn render_op(p: &PreparedOp) -> DryRunOp {
    let op: &Operation = p.op;
    DryRunOp {
        id: op.id,
        kind: format!("{:?}", op.kind),
        venue: venue_str(op.venue).to_string(),
        variant: op.variant.clone(),
        role: op.role.as_str().to_string(),
        intent: op.intent.as_str().to_string(),
        wallet: op.wallet.pubkey.clone(),
        target: op.target.clone(),
        amount: describe_amount(op.amount),
        min_out: p.min_out.label(),
        deps: op.deps.clone(),
    }
}

fn venue_str(v: VenueId) -> &'static str {
    v.as_str()
}

fn describe_amount(a: Amount) -> String {
    match a {
        Amount::ExactQuote(q) => format!("exact_quote {q} lamports"),
        Amount::ExactBase(b) => format!("exact_base {b} tokens"),
        Amount::ExactBaseIn(b) => format!("exact_base_in {b} tokens"),
        Amount::Sol(l) => format!("sol {l} lamports"),
        Amount::Token(t) => format!("token {t} base"),
        Amount::None => "—".to_string(),
    }
}

// Re-export so callers can `use orchestrator::dryrun::MinOut`-adjacent helpers if
// they build a report from parts.
pub use crate::provider::MinOut as MinOutPolicy;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{Amount, Intent, Operation, Plan, Role, WalletRef};
    use solana_sdk::signature::{Keypair, Signer};

    fn addr() -> String {
        Keypair::new().pubkey().to_string()
    }

    /// Gate C fixture: a hand-authored launch Plan — 1 create + dev-buy + 2
    /// bundler buys — validates through every provider and renders a clean
    /// dry-run, zero SOL. (Concrete signed txs + a `simulate_ixs` would-land
    /// check need a live `PumpFunTrader`, wired at Phase F.)
    fn launch_plan() -> (Plan, String) {
        let mint = addr();
        let dev = WalletRef::new(addr());
        let b1 = WalletRef::new(addr());
        let b2 = WalletRef::new(addr());

        let create = Operation::create(0, "create_v2", dev.clone(), mint.clone());
        // Dev-buy: exact SOL in, in the SAME tx as create (deps the create).
        let dev_buy = Operation::buy(
            1,
            "buy_exact_sol_in",
            Role::Dev,
            Intent::Snipe,
            dev,
            mint.clone(),
            Amount::ExactQuote(1_500_000),
            None, // launch leg keeps the historical unprotected min_out=1
            vec![0],
        );
        // Two bundler co-buys, same-slot bundle, each depends on the create.
        let bundler1 = Operation::buy(
            2,
            "buy_exact_sol_in",
            Role::Bundler,
            Intent::Snipe,
            b1,
            mint.clone(),
            Amount::ExactQuote(800_000),
            Some(500),
            vec![0],
        );
        let bundler2 = Operation::buy(
            3,
            "buy_exact_sol_in",
            Role::Bundler,
            Intent::Snipe,
            b2,
            mint.clone(),
            Amount::ExactQuote(900_000),
            Some(500),
            vec![0],
        );

        let plan = Plan::for_mint(mint.clone(), vec![create, dev_buy, bundler1, bundler2]);
        (plan, mint)
    }

    #[test]
    fn gate_c_launch_plan_dry_runs_clean() {
        let (plan, mint) = launch_plan();
        let report = dry_run(&plan).expect("hand-authored launch plan must be buildable");

        assert_eq!(report.n_ops, 4);
        assert_eq!(report.n_create, 1);
        assert_eq!(report.n_buy, 3);
        assert_eq!(report.n_sell, 0);
        assert_eq!(report.mint_address.as_deref(), Some(mint.as_str()));

        // Static quote roll-up = the three SOL-in legs (create/dev-buy amounts).
        assert_eq!(report.total_quote_spend, 1_500_000 + 800_000 + 900_000);

        // Dev-buy is unprotected (None slippage); bundler buys are late-bound.
        let dev_buy = report.ops.iter().find(|o| o.id == 1).unwrap();
        assert_eq!(dev_buy.min_out, "1 (unprotected)");
        assert_eq!(dev_buy.role, "dev");
        assert_eq!(dev_buy.intent, "snipe");
        let bundler = report.ops.iter().find(|o| o.id == 2).unwrap();
        assert_eq!(bundler.min_out, "computed-at-send @ 500bps");
        assert_eq!(bundler.role, "bundler");

        // Every buy deps the create (bundle ordering preserved).
        for id in [1, 2, 3] {
            let op = report.ops.iter().find(|o| o.id == id).unwrap();
            assert_eq!(op.deps, vec![0], "buy {id} must land after the create");
        }

        // The report serializes (the preview surface / persisted replay row).
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("create_v2"));
        assert!(json.contains("buy_exact_sol_in"));
    }

    /// An off-catalog variant is rejected before any build — the whole plan fails.
    #[test]
    fn off_catalog_variant_is_rejected() {
        let mint = addr();
        let dev = WalletRef::new(addr());
        let bad = Operation::buy(
            0,
            "flash_loan", // not in the catalog
            Role::Dev,
            Intent::Snipe,
            dev,
            mint.clone(),
            Amount::ExactQuote(1_000),
            None,
            vec![],
        );
        let plan = Plan::for_mint(mint, vec![bad]);
        let err = dry_run(&plan).unwrap_err();
        assert!(matches!(err, PlanError::UnknownVariant { .. }), "got {err}");
    }

    /// A variant of the wrong mechanism (a `sell` name on a `Buy` op) is rejected.
    #[test]
    fn kind_mismatch_is_rejected() {
        let mint = addr();
        let mut op = Operation::buy(
            0,
            "sell", // a Sell variant on a Buy op
            Role::Volume,
            Intent::MakeVolume,
            WalletRef::new(addr()),
            mint.clone(),
            Amount::ExactQuote(1_000),
            None,
            vec![],
        );
        // Force the amount to something the sell denom would accept so the KIND
        // check is what fires, not the denom check.
        op.amount = Amount::ExactBaseIn(1_000);
        let plan = Plan::for_mint(mint, vec![op]);
        let err = dry_run(&plan).unwrap_err();
        assert!(matches!(err, PlanError::KindMismatch { .. }), "got {err}");
    }

    /// An amount whose denomination the variant can't encode is rejected
    /// (variant ⊥ amount): `buy` is tokens-out (`ExactBase`), so an `ExactQuote`
    /// amount doesn't fit.
    #[test]
    fn denom_mismatch_is_rejected() {
        let mint = addr();
        let op = Operation::buy(
            0,
            "buy", // ExactBaseOut
            Role::Dev,
            Intent::Snipe,
            WalletRef::new(addr()),
            mint.clone(),
            Amount::ExactQuote(1_000), // but a SOL-in amount
            None,
            vec![],
        );
        let plan = Plan::for_mint(mint, vec![op]);
        let err = dry_run(&plan).unwrap_err();
        assert!(matches!(err, PlanError::DenomMismatch { .. }), "got {err}");
    }

    /// A typed consolidate (`TransferSol` / `Consolidate`) is buildable — the shape
    /// that replaces the raw `system_instruction::transfer` bypasses.
    #[test]
    fn typed_consolidate_transfer_is_buildable() {
        let treasury = addr();
        let op = Operation::transfer_sol(
            0,
            "transfer_with_seed",
            Intent::Consolidate,
            WalletRef::new(addr()),
            treasury,
            2_000_000,
            vec![],
        );
        let plan = Plan { mint_address: None, ops: vec![op], ..Default::default() };
        let report = dry_run(&plan).expect("typed transfer must build");
        assert_eq!(report.n_transfer, 1);
        assert_eq!(report.total_quote_spend, 2_000_000);
        assert_eq!(report.ops[0].min_out, "—");
    }

    /// A dependency on a non-existent op is rejected.
    #[test]
    fn dangling_dep_is_rejected() {
        let mint = addr();
        let op = Operation::buy(
            0,
            "buy_exact_sol_in",
            Role::Dev,
            Intent::Snipe,
            WalletRef::new(addr()),
            mint.clone(),
            Amount::ExactQuote(1_000),
            None,
            vec![99], // no such op
        );
        let plan = Plan::for_mint(mint, vec![op]);
        let err = dry_run(&plan).unwrap_err();
        assert!(matches!(err, PlanError::DanglingDep { .. }), "got {err}");
    }
}
