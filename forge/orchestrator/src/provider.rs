//! Providers — turn each [`Operation`] into a validated, buildable [`PreparedOp`]
//! by drawing its variant from the venue catalog (SSOT). This is the gate that
//! makes an invalid tx **unrepresentable**: an off-catalog variant, a variant of
//! the wrong mechanism, an amount whose denomination the variant can't encode, an
//! unparseable address, or a dangling dependency is rejected here — before any ix
//! is built or any SOL moves.
//!
//! Dispatch is **static** over [`VenueId`] / [`OpKind`] (a plain match, no
//! `Box<dyn>`). Today only pump.fun is wired; a second venue adds an arm, never a
//! trait object.
//!
//! What this phase (C) does NOT do: emit `solana_sdk::Instruction`s. The pump ix
//! builders are methods on an *initialized* `PumpFunTrader` (they read the
//! on-chain `Global` account for fee recipients / volume accumulators), so real
//! tx assembly needs a live trader and is wired at the Phase F cutover. A
//! `PreparedOp` carries everything that build step consumes — the resolved
//! [`VariantSpec`], parsed accounts, the canonical [`Amount`], and the late-bound
//! `min_out` policy — plus it is exactly what the zero-SOL dry-run summarizes.

use crate::plan::{Amount, OpKind, Operation, Plan, VenueId};
use pump_trader::{is_valid, spec, Denom, VariantSpec};
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

/// How a leg's `min_out` floor is determined — never a frozen value. A buy/sell
/// with a slippage tolerance is `Late` (computed at send from live reserves); a
/// buy/sell with `None` slippage is `Unprotected` (`min_out = 1`, no reserve
/// read); create/transfer have no swap floor (`NotApplicable`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MinOut {
    /// Computed at send from live reserves at this tolerance (bps).
    Late { slippage_bps: u64 },
    /// `min_out = 1` — no protection, no reserve read (`slippage = None`).
    Unprotected,
    /// Not a swap (create / transfer).
    NotApplicable,
}

impl MinOut {
    /// Preview label for the dry-run summary — `min_out` is shown as its policy,
    /// never a concrete number (it doesn't exist until send).
    pub fn label(self) -> String {
        match self {
            MinOut::Late { slippage_bps } => {
                format!("computed-at-send @ {slippage_bps}bps")
            }
            MinOut::Unprotected => "1 (unprotected)".to_string(),
            MinOut::NotApplicable => "—".to_string(),
        }
    }
}

/// A validated operation, ready to build (Phase F) or summarize (dry-run). Borrows
/// its source [`Operation`] and pins the resolved catalog row + parsed accounts.
#[derive(Debug, Clone)]
pub struct PreparedOp<'a> {
    pub op: &'a Operation,
    /// The resolved catalog row (the `'static` SSOT entry, not a copy).
    pub spec: &'static VariantSpec,
    /// The acting wallet, parsed + validated.
    pub wallet: Pubkey,
    /// The target mint/recipient, parsed when present.
    pub target: Option<Pubkey>,
    pub min_out: MinOut,
}

impl PreparedOp<'_> {
    pub fn amount(&self) -> Amount {
        self.op.amount
    }
}

/// A whole plan validated into buildable ops, in the plan's order.
#[derive(Debug, Clone)]
pub struct PreparedPlan<'a> {
    pub mint_address: Option<&'a str>,
    pub ops: Vec<PreparedOp<'a>>,
}

/// Everything that can make an [`Operation`] unbuildable. A crate-owned error (no
/// `anyhow` leak across the boundary), each variant naming the exact defect so the
/// preview/audit surface can report it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    /// The variant name isn't in `catalog::valid_variants(venue)`.
    UnknownVariant { op: u32, venue: VenueId, variant: String },
    /// The variant exists but performs a different mechanism than the op's kind.
    KindMismatch { op: u32, variant: String, spec_kind: OpKind, op_kind: OpKind },
    /// The op's `Amount` can't be encoded by the variant's denomination.
    DenomMismatch { op: u32, variant: String, denom: Denom, amount: Amount },
    /// A wallet/target address failed to parse.
    BadAddress { op: u32, field: &'static str, value: String },
    /// A swap/create/transfer op is missing its required target (mint/recipient).
    MissingTarget { op: u32, kind: OpKind },
    /// A dependency references an op id not present in the plan.
    DanglingDep { op: u32, dep: u32 },
    /// Two ops share an id (ids must be unique for dep resolution).
    DuplicateId { id: u32 },
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlanError::UnknownVariant { op, venue, variant } => write!(
                f,
                "op {op}: variant {variant:?} is not a legal {} variant",
                venue.as_str()
            ),
            PlanError::KindMismatch { op, variant, spec_kind, op_kind } => write!(
                f,
                "op {op}: variant {variant:?} is a {spec_kind:?} but the op is a {op_kind:?}"
            ),
            PlanError::DenomMismatch { op, variant, denom, amount } => write!(
                f,
                "op {op}: variant {variant:?} denominates {denom:?} but the amount is {amount:?}"
            ),
            PlanError::BadAddress { op, field, value } => {
                write!(f, "op {op}: {field} address {value:?} is not a valid pubkey")
            }
            PlanError::MissingTarget { op, kind } => {
                write!(f, "op {op}: a {kind:?} op requires a target (mint/recipient)")
            }
            PlanError::DanglingDep { op, dep } => {
                write!(f, "op {op}: depends on op {dep}, which is not in the plan")
            }
            PlanError::DuplicateId { id } => write!(f, "duplicate op id {id}"),
        }
    }
}

impl std::error::Error for PlanError {}

/// Validate + resolve every op in `plan` against the venue catalog. Returns the
/// prepared plan or the FIRST defect found (fail fast — a plan with any illegal op
/// is rejected whole). Pure: zero SOL, zero network.
pub fn prepare<'a>(plan: &'a Plan) -> Result<PreparedPlan<'a>, PlanError> {
    // Ids must be unique (deps reference them).
    for (i, a) in plan.ops.iter().enumerate() {
        if plan.ops[i + 1..].iter().any(|b| b.id == a.id) {
            return Err(PlanError::DuplicateId { id: a.id });
        }
    }

    let mut prepared = Vec::with_capacity(plan.ops.len());
    for op in &plan.ops {
        prepared.push(prepare_op(plan, op)?);
    }
    Ok(PreparedPlan { mint_address: plan.mint_address.as_deref(), ops: prepared })
}

fn prepare_op<'a>(plan: &Plan, op: &'a Operation) -> Result<PreparedOp<'a>, PlanError> {
    // 1) The variant must be legal for the venue (catalog SSOT).
    if !is_valid(op.venue, &op.variant) {
        return Err(PlanError::UnknownVariant {
            op: op.id,
            venue: op.venue,
            variant: op.variant.clone(),
        });
    }
    let spec = spec(&op.variant).expect("is_valid ⇒ spec present");

    // 2) The variant's mechanism must equal the op's kind.
    if spec.kind != op.kind {
        return Err(PlanError::KindMismatch {
            op: op.id,
            variant: op.variant.clone(),
            spec_kind: spec.kind,
            op_kind: op.kind,
        });
    }

    // 3) The amount's denomination must be encodable by the variant.
    if !denom_accepts(spec.denom, op.amount) {
        return Err(PlanError::DenomMismatch {
            op: op.id,
            variant: op.variant.clone(),
            denom: spec.denom,
            amount: op.amount,
        });
    }

    // 4) Accounts parse.
    let wallet = Pubkey::from_str(&op.wallet.pubkey).map_err(|_| PlanError::BadAddress {
        op: op.id,
        field: "wallet",
        value: op.wallet.pubkey.clone(),
    })?;
    let target = match &op.target {
        Some(t) => Some(Pubkey::from_str(t).map_err(|_| PlanError::BadAddress {
            op: op.id,
            field: "target",
            value: t.clone(),
        })?),
        None => None,
    };
    // Everything except a bare-SOL-to-nowhere op needs a target; all our kinds do.
    if target.is_none() && requires_target(op.kind) {
        return Err(PlanError::MissingTarget { op: op.id, kind: op.kind });
    }

    // 5) Deps resolve to real ops.
    for &dep in &op.deps {
        if plan.op(dep).is_none() {
            return Err(PlanError::DanglingDep { op: op.id, dep });
        }
    }

    Ok(PreparedOp { op, spec, wallet, target, min_out: min_out_policy(op) })
}

/// Can a variant denominated `denom` encode this `amount`? The variant ⊥ amount
/// rule as a compatibility check (a SOL-in buy takes an `ExactQuote`, a tokens-out
/// buy takes an `ExactBase`, and neither takes the other).
fn denom_accepts(denom: Denom, amount: Amount) -> bool {
    matches!(
        (denom, amount),
        (Denom::ExactQuoteIn, Amount::ExactQuote(_))
            | (Denom::ExactBaseOut, Amount::ExactBase(_))
            | (Denom::ExactBaseIn, Amount::ExactBaseIn(_))
            | (Denom::None, Amount::None)
            | (Denom::Sol, Amount::Sol(_))
            | (Denom::Token, Amount::Token(_))
    )
}

/// Every mechanism we build targets a mint (swap/create) or a recipient
/// (transfer), so all require a target today.
fn requires_target(_kind: OpKind) -> bool {
    true
}

/// The late-binding `min_out` policy for an op — swaps with a tolerance are
/// computed at send; `None` slippage is unprotected; non-swaps have no floor.
fn min_out_policy(op: &Operation) -> MinOut {
    match op.kind {
        OpKind::Buy | OpKind::Sell => match op.slippage_bps {
            Some(bps) => MinOut::Late { slippage_bps: bps },
            None => MinOut::Unprotected,
        },
        OpKind::Create | OpKind::TransferSol | OpKind::TransferToken => MinOut::NotApplicable,
    }
}
