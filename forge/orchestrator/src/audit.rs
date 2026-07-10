//! **Fingerprint auditor** (forge-only, mandatory) — the last gate before a `Plan`
//! executes. It reads the *whole* plan (ops + funding graph + schedule) and the
//! per-op **send shape** (the [`crate::disguise::Disguise`] plus the nonce / ATA /
//! account-list attributes the Phase F build step resolves) and looks for the
//! on-chain *tells* that link our wallets to each other or mark them as a bot:
//!
//!   1. **star funding** — one source fans SOL out to many wallets (a funding-graph
//!      star is the single strongest linkage tell);
//!   2. **equal amounts** — a cluster of legs (buys/sells or funding edges) that
//!      spend the *exact* same amount (e.g. everyone buys 0.02 SOL);
//!   3. **same-slot cluster** — many non-launch ops (exits / volume) crammed into
//!      one bundle / delay bucket so they land together;
//!   4. **direct own-graph token edge** — an SPL-token transfer straight from one of
//!      our wallets to another (a permanent, mint-tagged linkage on-chain);
//!   5. **constant CU / tip** — every op fee-bids identically (no persona spread);
//!   6. **synchronized `Bundler` exit** — the launch bundlers all sell in one slot;
//!   7. **nonce-account reuse** — two ops share a durable-nonce account (the
//!      invariant is a *fresh* nonce account per wallet);
//!   8. **`DurableNonce` on a non-latency op** — durable nonce is off-by-default for
//!      forge; riding it on a volume/exit op is a misconfiguration tell;
//!   9. **clustered `close_token_ata`** — several exits close their token ATA in the
//!      same breath (humans leave dust ATAs; bots sweep them);
//!  10. **account-shape integrity** — the pump fee recipient must sit at the fixed
//!      account index the curve-buy ix lays it at (see [`PUMP_FEE_RECIPIENT_INDEX`]);
//!      a reshaped account list is a **hard reject** (a malformed tx, not a stylistic
//!      tell) that `--allow-fingerprint` can *not* override.
//!
//! Every rule is a standalone, unit-testable fn over the audit context. The report
//! carries a fingerprint **score** (informational) and the findings; the pass/fail
//! decision is: a plan **fails** if it has any hard-reject finding, or any
//! fingerprint finding *unless* `allow_fingerprint` is set. Pure: zero SOL, zero
//! network.

use crate::disguise::Disguise;
use crate::plan::{Funding, Intent, OpId, OpKind, Operation, Plan, Role};
use serde::Serialize;
use std::collections::BTreeMap;

/// The account index the pump **curve-buy** ix fixes its fee recipient at
/// (`PUMP_CURVE_FEE_RECIPIENT`, the 18th account — see
/// `pump_trader::trader::buy` account layout). The account list is load-bearing:
/// the program reads the fee recipient positionally, so reshaping it breaks the tx.
/// This mirrors that layout; it is the shape the auditor asserts, not a second
/// source for the *pubkey* (which stays owned by the executor's `protocol` consts).
pub const PUMP_FEE_RECIPIENT_INDEX: usize = 17;

/// How bad a finding is. `Reject` is an un-overridable hard reject (a malformed tx);
/// `Warn` is a fingerprint tell that `allow_fingerprint` can wave through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Warn,
    Reject,
}

/// The rule that fired — a stable slug for the finding (so a UI/log can group them).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Rule {
    StarFunding,
    EqualAmounts,
    SameSlotCluster,
    DirectOwnGraphTokenEdge,
    ConstantCuTip,
    SynchronizedBundlerExit,
    NonceReuse,
    DurableNonceMisuse,
    ClusteredAtaClose,
    AccountShapeIntegrity,
}

/// One tell the auditor found. `op_ids` names the ops implicated so the operator
/// (or a fix pass) can see exactly what to reshape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub rule: Rule,
    pub severity: Severity,
    /// Fingerprint weight this finding contributes to the score (informational).
    pub weight: u32,
    pub message: String,
    pub op_ids: Vec<OpId>,
}

/// The audit result. `passed` is decided by [`AuditReport::passed`]; `score` is the
/// summed weight of the fingerprint (`Warn`) findings, for ranking/telemetry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuditReport {
    pub findings: Vec<Finding>,
    pub score: u32,
    pub hard_reject: bool,
}

impl AuditReport {
    /// Whether this plan may execute. A hard reject (malformed account shape) is
    /// never overridable; any other fingerprint finding rejects the plan unless
    /// `allow_fingerprint` is set (the `--allow-fingerprint` escape hatch).
    pub fn passed(&self, allow_fingerprint: bool) -> bool {
        if self.hard_reject {
            return false;
        }
        allow_fingerprint || !self.findings.iter().any(|f| f.severity == Severity::Warn)
    }

    /// The findings of a given rule (test/inspection helper).
    pub fn of(&self, rule: Rule) -> impl Iterator<Item = &Finding> {
        self.findings.iter().filter(move |f| f.rule == rule)
    }

    /// Did any rule of this kind fire?
    pub fn has(&self, rule: Rule) -> bool {
        self.findings.iter().any(|f| f.rule == rule)
    }
}

/// The per-op **send shape** the auditor inspects — the disguise plus the send-time
/// attributes the Phase F build step resolves (which durable-nonce account the op
/// would use, whether it rides a durable nonce, whether a sell closes its token ATA,
/// and the built account list + which account must sit at the fee-recipient index).
/// All are optional/defaulted so a *pre-build* Plan-only audit still runs the graph /
/// amount / schedule / token-edge rules; the build-shape rules simply find nothing
/// to inspect until a profile carries the data.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SendProfile {
    pub op_id: OpId,
    /// The drawn disguise (CU limit / price / tip). Feeds the constant-CU/tip rule.
    pub disguise: Option<Disguise>,
    /// The durable-nonce account this op would consume, if any. Feeds nonce-reuse.
    pub nonce_account: Option<String>,
    /// Whether this op rides a `DurableNonce` (off-by-default for forge).
    pub durable_nonce: bool,
    /// Whether this (sell) op closes the token ATA after selling.
    pub close_token_ata: bool,
    /// The built ix account metas, in order (base58). Empty pre-build.
    pub accounts: Vec<String>,
    /// The account that MUST occupy [`PUMP_FEE_RECIPIENT_INDEX`] in `accounts`
    /// (the resolved `PUMP_CURVE_FEE_RECIPIENT`). Checked only when `accounts` is
    /// present.
    pub fee_recipient: Option<String>,
}

impl SendProfile {
    /// A bare profile for an op (no build-shape data) — the pre-Phase-F case.
    pub fn new(op_id: OpId) -> Self {
        Self { op_id, ..Default::default() }
    }
}

/// Tunable thresholds. Defaults are conservative; the lab can tighten them once the
/// clustering job characterizes real traffic.
#[derive(Debug, Clone, Copy)]
pub struct AuditConfig {
    pub fee_recipient_index: usize,
    /// One funding source fanning to ≥ this many targets is a star.
    pub star_fan_out_min: usize,
    /// ≥ this many legs at the identical amount is an equal-amounts cluster.
    pub equal_amount_min_cluster: usize,
    /// ≥ this many non-launch ops in one slot is a same-slot cluster.
    pub same_slot_min_cluster: usize,
    /// ≥ this many ops sharing one `(cu_price, tip)` pair is a constant-fee tell.
    pub constant_fee_min_cluster: usize,
    /// ≥ this many exits closing their ATA is a clustered-ATA-close tell.
    pub ata_close_min_cluster: usize,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            fee_recipient_index: PUMP_FEE_RECIPIENT_INDEX,
            star_fan_out_min: 3,
            equal_amount_min_cluster: 3,
            same_slot_min_cluster: 3,
            constant_fee_min_cluster: 3,
            ata_close_min_cluster: 2,
        }
    }
}

/// Audit a `Plan` alone (pre-build): runs the funding / amount / schedule / token
/// -edge rules. The build-shape rules (CU/tip, nonce, ATA, account list) need
/// [`SendProfile`]s — use [`audit_with`].
pub fn audit(plan: &Plan) -> AuditReport {
    audit_with(plan, &[], AuditConfig::default())
}

/// The full audit: every rule, over the plan + its per-op send profiles.
pub fn audit_with(plan: &Plan, profiles: &[SendProfile], cfg: AuditConfig) -> AuditReport {
    let ctx = Ctx { plan, profiles, cfg };
    let mut findings = Vec::new();

    findings.extend(rule_star_funding(&ctx));
    findings.extend(rule_equal_amounts(&ctx));
    findings.extend(rule_same_slot_cluster(&ctx));
    findings.extend(rule_direct_own_graph_token_edge(&ctx));
    findings.extend(rule_constant_cu_tip(&ctx));
    findings.extend(rule_synchronized_bundler_exit(&ctx));
    findings.extend(rule_nonce_reuse(&ctx));
    findings.extend(rule_durable_nonce_misuse(&ctx));
    findings.extend(rule_clustered_ata_close(&ctx));
    findings.extend(rule_account_shape_integrity(&ctx));

    let hard_reject = findings.iter().any(|f| f.severity == Severity::Reject);
    let score = findings
        .iter()
        .filter(|f| f.severity == Severity::Warn)
        .map(|f| f.weight)
        .sum();
    AuditReport { findings, score, hard_reject }
}

// ---------------------------------------------------------------------------
// context + helpers
// ---------------------------------------------------------------------------

struct Ctx<'a> {
    plan: &'a Plan,
    profiles: &'a [SendProfile],
    cfg: AuditConfig,
}

/// The slot an op lands in, for same-slot clustering. A scheduled op keys on its
/// bundle (if bundled) else its delay; an unscheduled op shares the `Unscheduled`
/// key with every other unscheduled op (no spacing ⇒ they fire together).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SlotKey {
    Bundle(u32),
    Delay(u64),
    Unscheduled,
}

impl Ctx<'_> {
    /// The pubkeys of wallets we control (managed-pool rows). An external
    /// counterparty has no `managed_wallet_id`, so a transfer *to* an own pubkey is the
    /// linkable own-graph edge.
    fn own_pubkeys(&self) -> std::collections::BTreeSet<&str> {
        self.plan
            .ops
            .iter()
            .filter(|o| o.wallet.managed_wallet_id.is_some())
            .map(|o| o.wallet.pubkey.as_str())
            .collect()
    }

    fn slot_of(&self, op_id: OpId) -> SlotKey {
        match self.plan.schedule.slots.iter().find(|s| s.op_id == op_id) {
            Some(s) => match s.bundle {
                Some(b) => SlotKey::Bundle(b),
                None => SlotKey::Delay(s.delay_ms),
            },
            None => SlotKey::Unscheduled,
        }
    }
}

fn finding(rule: Rule, severity: Severity, weight: u32, message: String, op_ids: Vec<OpId>) -> Finding {
    Finding { rule, severity, weight, message, op_ids }
}

// ---------------------------------------------------------------------------
// rules — each a standalone, unit-testable fn over the context
// ---------------------------------------------------------------------------

/// 1) Star funding: one source funds ≥ `star_fan_out_min` distinct targets.
fn rule_star_funding(ctx: &Ctx) -> Vec<Finding> {
    star_sources(&ctx.plan.funding, ctx.cfg.star_fan_out_min)
        .into_iter()
        .map(|(src, n)| {
            finding(
                Rule::StarFunding,
                Severity::Warn,
                40,
                format!("funding source {src} fans out to {n} wallets (star ≥ {})", ctx.cfg.star_fan_out_min),
                Vec::new(),
            )
        })
        .collect()
}

/// Pure sub-fn (unit-testable without a whole plan): sources funding ≥ `min`
/// distinct targets, with their fan-out count.
fn star_sources(funding: &Funding, min: usize) -> Vec<(String, usize)> {
    let mut fan: BTreeMap<&str, std::collections::BTreeSet<&str>> = BTreeMap::new();
    for e in &funding.edges {
        fan.entry(e.from.as_str()).or_default().insert(e.to.as_str());
    }
    fan.into_iter()
        .filter(|(_, tos)| tos.len() >= min)
        .map(|(from, tos)| (from.to_string(), tos.len()))
        .collect()
}

/// 2) Equal amounts: ≥ `equal_amount_min_cluster` swap legs at the identical
/// (kind, amount). Everyone-buys-0.02-SOL is the canonical tell.
fn rule_equal_amounts(ctx: &Ctx) -> Vec<Finding> {
    // Group swap ops by (kind, amount encoding). Create/transfer are excluded — a
    // create has no amount and equal funding/consolidate amounts are their own,
    // separately-scheduled concern.
    let mut groups: BTreeMap<(&'static str, u64, &'static str), Vec<OpId>> = BTreeMap::new();
    for op in &ctx.plan.ops {
        let kind_tag = match op.kind {
            OpKind::Buy => "Buy",
            OpKind::Sell => "Sell",
            _ => continue,
        };
        let (v, tag) = amount_key(op);
        groups.entry((kind_tag, v, tag)).or_default().push(op.id);
    }
    groups
        .into_iter()
        .filter(|(_, ops)| ops.len() >= ctx.cfg.equal_amount_min_cluster)
        .map(|((kind_tag, v, tag), ops)| {
            let n = ops.len();
            finding(
                Rule::EqualAmounts,
                Severity::Warn,
                30,
                format!("{n} {kind_tag} legs at the identical amount ({v} {tag})"),
                ops,
            )
        })
        .collect()
}

/// The comparable amount value + a unit tag, so `ExactQuote(x)` and `Sol(x)` don't
/// collide across denominations.
fn amount_key(op: &Operation) -> (u64, &'static str) {
    use crate::plan::Amount::*;
    match op.amount {
        ExactQuote(v) => (v, "quote"),
        ExactBase(v) => (v, "base_out"),
        ExactBaseIn(v) => (v, "base_in"),
        Sol(v) => (v, "sol"),
        Token(v) => (v, "token"),
        None => (0, "none"),
    }
}

/// 3) Same-slot cluster: ≥ `same_slot_min_cluster` non-launch ops share one slot.
/// Launch snipes (create + dev/bundler `Snipe` buys) are *meant* to be atomic — a
/// Jito bundle — so they're excluded; this targets exits / volume clustered together.
fn rule_same_slot_cluster(ctx: &Ctx) -> Vec<Finding> {
    let mut by_slot: BTreeMap<SlotKey, Vec<OpId>> = BTreeMap::new();
    for op in &ctx.plan.ops {
        if is_launch_op(op) {
            continue;
        }
        by_slot.entry(ctx.slot_of(op.id)).or_default().push(op.id);
    }
    by_slot
        .into_iter()
        .filter(|(_, ops)| ops.len() >= ctx.cfg.same_slot_min_cluster)
        .map(|(slot, ops)| {
            let n = ops.len();
            finding(
                Rule::SameSlotCluster,
                Severity::Warn,
                30,
                format!("{n} non-launch ops share slot {slot:?} (land together)"),
                ops,
            )
        })
        .collect()
}

/// A launch op is the create or a `Snipe`-intent buy — the legitimately-atomic
/// bundle, excluded from same-slot clustering.
fn is_launch_op(op: &Operation) -> bool {
    op.kind == OpKind::Create || (op.kind == OpKind::Buy && op.intent == Intent::Snipe)
}

/// 4) Direct own-graph token edge: a `TransferToken` whose target is one of *our*
/// wallets — a permanent, mint-tagged linkage on-chain (SOL consolidation is
/// expected and not flagged here; a raw token move between own wallets is not).
fn rule_direct_own_graph_token_edge(ctx: &Ctx) -> Vec<Finding> {
    let own = ctx.own_pubkeys();
    ctx.plan
        .ops
        .iter()
        .filter(|op| op.kind == OpKind::TransferToken)
        .filter_map(|op| {
            let target = op.target.as_deref()?;
            own.contains(target).then(|| {
                finding(
                    Rule::DirectOwnGraphTokenEdge,
                    Severity::Warn,
                    50,
                    format!("op {} transfers tokens directly to own wallet {target}", op.id),
                    vec![op.id],
                )
            })
        })
        .collect()
}

/// 5) Constant CU / tip: ≥ `constant_fee_min_cluster` disguised ops share one
/// exact `(cu_price, tip)` pair — no persona spread, a bot tell.
fn rule_constant_cu_tip(ctx: &Ctx) -> Vec<Finding> {
    let mut groups: BTreeMap<(u64, Option<u64>), Vec<OpId>> = BTreeMap::new();
    for p in ctx.profiles {
        if let Some(d) = &p.disguise {
            groups
                .entry((d.cu_price_micro_lamports, d.tip_lamports))
                .or_default()
                .push(p.op_id);
        }
    }
    // Only a tell when the WHOLE disguised set collapses onto few identical pairs.
    // A single identical pair spanning ≥ threshold ops is the fingerprint.
    groups
        .into_iter()
        .filter(|(_, ops)| ops.len() >= ctx.cfg.constant_fee_min_cluster)
        .map(|((price, tip), ops)| {
            let n = ops.len();
            finding(
                Rule::ConstantCuTip,
                Severity::Warn,
                30,
                format!("{n} ops fee-bid identically (cu_price={price}, tip={tip:?})"),
                ops,
            )
        })
        .collect()
}

/// 6) Synchronized `Bundler` exit: ≥ 2 bundler sells land in the same slot.
fn rule_synchronized_bundler_exit(ctx: &Ctx) -> Vec<Finding> {
    let mut by_slot: BTreeMap<SlotKey, Vec<OpId>> = BTreeMap::new();
    for op in &ctx.plan.ops {
        if op.kind == OpKind::Sell && op.role == Role::Bundler {
            by_slot.entry(ctx.slot_of(op.id)).or_default().push(op.id);
        }
    }
    by_slot
        .into_iter()
        .filter(|(_, ops)| ops.len() >= 2)
        .map(|(slot, ops)| {
            let n = ops.len();
            finding(
                Rule::SynchronizedBundlerExit,
                Severity::Warn,
                40,
                format!("{n} bundler sells synchronized in slot {slot:?}"),
                ops,
            )
        })
        .collect()
}

/// 7) Nonce-account reuse: a durable-nonce account consumed by more than one op
/// (the invariant is a *fresh* nonce account per wallet).
fn rule_nonce_reuse(ctx: &Ctx) -> Vec<Finding> {
    let mut by_nonce: BTreeMap<&str, Vec<OpId>> = BTreeMap::new();
    for p in ctx.profiles {
        if let Some(n) = &p.nonce_account {
            by_nonce.entry(n.as_str()).or_default().push(p.op_id);
        }
    }
    by_nonce
        .into_iter()
        .filter(|(_, ops)| ops.len() > 1)
        .map(|(nonce, ops)| {
            let n = ops.len();
            finding(
                Rule::NonceReuse,
                Severity::Warn,
                40,
                format!("nonce account {nonce} reused across {n} ops (must be fresh per wallet)"),
                ops,
            )
        })
        .collect()
}

/// 8) `DurableNonce` on a non-latency op: durable nonce is off-by-default for forge;
/// a non-`Snipe` op riding it is a misconfiguration / linkable tell.
fn rule_durable_nonce_misuse(ctx: &Ctx) -> Vec<Finding> {
    ctx.profiles
        .iter()
        .filter(|p| p.durable_nonce)
        .filter_map(|p| {
            let op = ctx.plan.op(p.op_id)?;
            (op.intent != Intent::Snipe).then(|| {
                finding(
                    Rule::DurableNonceMisuse,
                    Severity::Warn,
                    20,
                    format!("op {} rides a durable nonce on a non-latency ({}) op", op.id, op.intent.as_str()),
                    vec![op.id],
                )
            })
        })
        .collect()
}

/// 9) Clustered `close_token_ata`: ≥ `ata_close_min_cluster` exits close their token
/// ATA — humans leave dust ATAs; a synchronized close is a bot tell.
fn rule_clustered_ata_close(ctx: &Ctx) -> Vec<Finding> {
    let ops: Vec<OpId> = ctx
        .profiles
        .iter()
        .filter(|p| p.close_token_ata)
        .filter(|p| ctx.plan.op(p.op_id).map(|o| o.kind == OpKind::Sell).unwrap_or(false))
        .map(|p| p.op_id)
        .collect();
    if ops.len() >= ctx.cfg.ata_close_min_cluster {
        let n = ops.len();
        vec![finding(
            Rule::ClusteredAtaClose,
            Severity::Warn,
            25,
            format!("{n} exits close their token ATA in one plan (bot tell)"),
            ops,
        )]
    } else {
        Vec::new()
    }
}

/// 10) Account-shape integrity: a declared fee recipient MUST sit at the fixed
/// account index. A reshaped list is a malformed tx — a **hard reject** that
/// `--allow-fingerprint` can never override.
fn rule_account_shape_integrity(ctx: &Ctx) -> Vec<Finding> {
    let idx = ctx.cfg.fee_recipient_index;
    ctx.profiles
        .iter()
        .filter(|p| !p.accounts.is_empty())
        .filter_map(|p| {
            let fee = p.fee_recipient.as_deref()?;
            let ok = p.accounts.get(idx).map(|a| a == fee).unwrap_or(false);
            (!ok).then(|| {
                let at = p.accounts.iter().position(|a| a == fee);
                finding(
                    Rule::AccountShapeIntegrity,
                    Severity::Reject,
                    100,
                    format!(
                        "op {}: fee recipient must be at account index {idx}, found at {at:?} (len {})",
                        p.op_id,
                        p.accounts.len()
                    ),
                    vec![p.op_id],
                )
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disguise::for_op;
    use crate::macros::{bundle_launch, consolidate, exit, volume_make, BundleLaunch, BundlerLeg, ExitLeg, Sweep, VolumeLeg};
    use crate::personas::PersonaSet;
    use crate::plan::{
        Amount, FundingEdge, IdSeq, Intent, Operation, Plan, Role, ScheduleSlot, WalletRef,
    };
    use executor_core::config::ComputeBudgetCfg;
    use solana_sdk::signature::{Keypair, Signer};

    fn addr() -> String {
        Keypair::new().pubkey().to_string()
    }
    fn managed() -> WalletRef {
        WalletRef::managed(uuid::Uuid::new_v4(), addr())
    }

    // ---- per-rule unit tests -------------------------------------------------

    #[test]
    fn star_funding_fires_on_fan_out() {
        let src = addr();
        let funding = Funding {
            edges: (0..4)
                .map(|_| FundingEdge { from: src.clone(), to: addr(), lamports: 1_000_000 })
                .collect(),
        };
        assert_eq!(star_sources(&funding, 3).len(), 1, "one source fans to 4 targets");
        // A jit pattern (each target a distinct source) is NOT a star.
        let spread = Funding {
            edges: (0..4).map(|_| FundingEdge { from: addr(), to: addr(), lamports: 1_000_000 }).collect(),
        };
        assert!(star_sources(&spread, 3).is_empty());
    }

    #[test]
    fn equal_amounts_fires_on_identical_legs() {
        let mint = addr();
        let ops: Vec<Operation> = (0..3)
            .map(|i| {
                Operation::buy(
                    i,
                    "buy_exact_sol_in",
                    Role::Volume,
                    Intent::MakeVolume,
                    managed(),
                    mint.clone(),
                    Amount::ExactQuote(20_000_000), // everyone buys 0.02 SOL
                    Some(300),
                    vec![],
                )
            })
            .collect();
        let plan = Plan::for_mint(mint, ops);
        let report = audit(&plan);
        assert!(report.has(Rule::EqualAmounts));
    }

    #[test]
    fn direct_own_graph_token_edge_fires() {
        // A token transfer whose target is one of our own wallets. `b` is known to
        // be ours because it acts elsewhere in the plan (a wallet is "ours" when a
        // WalletRef carries a managed_wallet_id — the auditor sees that on acting ops).
        let mint = addr();
        let a = managed();
        let b = managed();
        let b_acts = Operation::buy(
            0, "buy_exact_sol_in", Role::Volume, Intent::MakeVolume, b.clone(), mint.clone(),
            Amount::ExactQuote(500_000), Some(300), vec![],
        );
        let edge = Operation {
            id: 1,
            kind: OpKind::TransferToken,
            venue: crate::plan::VenueId::PumpFun,
            variant: "spl_transfer".to_string(),
            role: Role::Volume,
            intent: Intent::Consolidate,
            amount: Amount::Token(1_000),
            slippage_bps: None,
            wallet: a,
            target: Some(b.pubkey.clone()),
            deps: vec![],
        };
        let plan = Plan::for_mint(mint, vec![b_acts, edge]);
        assert!(audit(&plan).has(Rule::DirectOwnGraphTokenEdge));
    }

    #[test]
    fn account_shape_integrity_is_hard_reject() {
        let fee = addr();
        // A mangled list: fee recipient at index 5, not 17.
        let mut accounts: Vec<String> = (0..20).map(|_| addr()).collect();
        accounts[5] = fee.clone();
        let profile = SendProfile {
            op_id: 0,
            accounts,
            fee_recipient: Some(fee),
            ..SendProfile::new(0)
        };
        let op = Operation::buy(
            0, "buy_exact_sol_in", Role::Dev, Intent::Snipe, managed(), addr(),
            Amount::ExactQuote(1_000_000), None, vec![],
        );
        let plan = Plan::for_mint(addr(), vec![op]);
        let report = audit_with(&plan, &[profile], AuditConfig::default());
        assert!(report.hard_reject, "mangled account list is a hard reject");
        assert!(!report.passed(true), "--allow-fingerprint can NOT override a hard reject");
    }

    #[test]
    fn correct_account_shape_passes() {
        let fee = addr();
        let mut accounts: Vec<String> = (0..20).map(|_| addr()).collect();
        accounts[PUMP_FEE_RECIPIENT_INDEX] = fee.clone();
        let profile = SendProfile { op_id: 0, accounts, fee_recipient: Some(fee), ..SendProfile::new(0) };
        let op = Operation::buy(
            0, "buy_exact_sol_in", Role::Dev, Intent::Snipe, managed(), addr(),
            Amount::ExactQuote(1_000_000), None, vec![],
        );
        let plan = Plan::for_mint(addr(), vec![op]);
        assert!(!audit_with(&plan, &[profile], AuditConfig::default()).hard_reject);
    }

    #[test]
    fn nonce_reuse_fires() {
        let nonce = addr();
        let profiles = vec![
            SendProfile { op_id: 0, nonce_account: Some(nonce.clone()), ..SendProfile::new(0) },
            SendProfile { op_id: 1, nonce_account: Some(nonce), ..SendProfile::new(1) },
        ];
        let plan = Plan { ops: vec![], ..Default::default() };
        assert!(audit_with(&plan, &profiles, AuditConfig::default()).has(Rule::NonceReuse));
    }

    #[test]
    fn durable_nonce_on_volume_fires_but_not_on_snipe() {
        let mint = addr();
        let snipe = Operation::buy(0, "buy_exact_sol_in", Role::Dev, Intent::Snipe, managed(), mint.clone(), Amount::ExactQuote(1_000_000), None, vec![]);
        let vol = Operation::buy(1, "buy_exact_sol_in", Role::Volume, Intent::MakeVolume, managed(), mint.clone(), Amount::ExactQuote(500_000), Some(300), vec![]);
        let plan = Plan::for_mint(mint, vec![snipe, vol]);
        let profiles = vec![
            SendProfile { op_id: 0, durable_nonce: true, ..SendProfile::new(0) },
            SendProfile { op_id: 1, durable_nonce: true, ..SendProfile::new(1) },
        ];
        let report = audit_with(&plan, &profiles, AuditConfig::default());
        let hits: Vec<_> = report.of(Rule::DurableNonceMisuse).collect();
        assert_eq!(hits.len(), 1, "only the non-snipe op is flagged");
        assert_eq!(hits[0].op_ids, vec![1]);
    }

    // ---- Gate E: naive plan rejected, clean plan passes ---------------------

    /// A deliberately-naive plan: star fund, equal 0.02 legs, same-slot sells, a
    /// direct own-graph token transfer, a reused nonce, and a mangled account list.
    #[test]
    fn gate_e_naive_plan_is_rejected() {
        let mint = addr();
        let treasury = addr();
        let w: Vec<WalletRef> = (0..3).map(|_| managed()).collect();

        // star fund: treasury → 3 wallets
        let funding = Funding {
            edges: w.iter().map(|x| FundingEdge { from: treasury.clone(), to: x.pubkey.clone(), lamports: 20_000_000 }).collect(),
        };

        let mut ops = Vec::new();
        // equal 0.02 SOL buys (3 legs, identical)
        for (i, x) in w.iter().enumerate() {
            ops.push(Operation::buy(
                i as u32, "buy_exact_sol_in", Role::Volume, Intent::MakeVolume, x.clone(), mint.clone(),
                Amount::ExactQuote(20_000_000), Some(300), vec![],
            ));
        }
        // same-slot sells (unscheduled ⇒ all fire together)
        for (i, x) in w.iter().enumerate() {
            ops.push(Operation::sell(
                (10 + i) as u32, "sell", Role::Volume, x.clone(), mint.clone(), 100_000, Some(300), vec![],
            ));
        }
        // direct own-graph token edge: w[0] → w[1]
        ops.push(Operation {
            id: 20,
            kind: OpKind::TransferToken,
            venue: crate::plan::VenueId::PumpFun,
            variant: "spl_transfer".to_string(),
            role: Role::Volume,
            intent: Intent::Consolidate,
            amount: Amount::Token(5_000),
            slippage_bps: None,
            wallet: w[0].clone(),
            target: Some(w[1].pubkey.clone()),
            deps: vec![],
        });

        let plan = Plan { mint_address: Some(mint), ops, funding, schedule: Default::default() };

        // profiles: a reused nonce across two ops + a mangled account list.
        let nonce = addr();
        let fee = addr();
        let mut mangled: Vec<String> = (0..20).map(|_| addr()).collect();
        mangled[3] = fee.clone(); // fee recipient at the WRONG index
        let profiles = vec![
            SendProfile { op_id: 0, nonce_account: Some(nonce.clone()), ..SendProfile::new(0) },
            SendProfile { op_id: 1, nonce_account: Some(nonce), ..SendProfile::new(1) },
            SendProfile { op_id: 2, accounts: mangled, fee_recipient: Some(fee), ..SendProfile::new(2) },
        ];

        let report = audit_with(&plan, &profiles, AuditConfig::default());
        assert!(report.hard_reject, "mangled account list ⇒ hard reject");
        assert!(!report.passed(false));
        assert!(!report.passed(true), "hard reject is not overridable");
        // The fingerprint tells all fired.
        for rule in [
            Rule::StarFunding,
            Rule::EqualAmounts,
            Rule::SameSlotCluster,
            Rule::DirectOwnGraphTokenEdge,
            Rule::NonceReuse,
            Rule::AccountShapeIntegrity,
        ] {
            assert!(report.has(rule), "expected {rule:?} to fire");
        }
        assert!(report.score > 0);
    }

    /// A properly scheduled + persona'd plan passes: a real launch bundle (atomic,
    /// excluded), volume/exit spread across delays with varied amounts, a SOL-only
    /// consolidate (no token edge), persona-drawn disguises (varied fees), fresh
    /// nonces, and a correctly-shaped account list.
    #[test]
    fn gate_e_clean_plan_passes() {
        let set = PersonaSet::builtin();
        let cfg = ComputeBudgetCfg::default();
        let mint = addr();
        let treasury = addr();
        let dev = managed();
        let vols: Vec<WalletRef> = (0..3).map(|_| managed()).collect();

        let mut seq = IdSeq::new(0);
        let mut ops = Vec::new();

        // Launch bundle (atomic snipe — excluded from same-slot).
        ops.extend(bundle_launch(&mut seq, &BundleLaunch {
            mint: mint.clone(),
            dev: dev.clone(),
            create_variant: "create_v2".to_string(),
            buy_variant: "buy_exact_sol_in".to_string(),
            dev_buy_sol: 1_500_000,
            dev_slippage_bps: None,
            bundlers: vec![],
        }));

        // Volume: varied amounts, distinct wallets.
        ops.extend(volume_make(&mut seq, &mint, &[
            VolumeLeg::Buy { wallet: vols[0].clone(), variant: "buy_exact_sol_in".to_string(), sol: 511_111, slippage_bps: Some(300) },
            VolumeLeg::Buy { wallet: vols[1].clone(), variant: "buy_exact_sol_in".to_string(), sol: 733_333, slippage_bps: Some(300) },
        ]));

        // Exit + SOL consolidate (no token edge).
        let exit_ops = exit(&mut seq, &mint, &[
            ExitLeg { wallet: vols[0].clone(), role: Role::Volume, variant: "sell".to_string(), tokens_base: 41_234, slippage_bps: Some(500) },
            ExitLeg { wallet: vols[1].clone(), role: Role::Volume, variant: "sell".to_string(), tokens_base: 58_912, slippage_bps: Some(500) },
        ]);
        let exit_ids: Vec<OpId> = exit_ops.iter().map(|o| o.id).collect();
        ops.extend(exit_ops);
        ops.extend(consolidate(&mut seq, &treasury, &[
            Sweep { wallet: vols[0].clone(), role: Role::Volume, lamports: 1_234_567 },
            Sweep { wallet: vols[1].clone(), role: Role::Volume, lamports: 2_345_678 },
        ], &exit_ids));

        // Schedule: spread the non-launch ops across distinct delays so nothing
        // shares a slot.
        let slots: Vec<ScheduleSlot> = ops
            .iter()
            .filter(|o| !is_launch_op(o))
            .enumerate()
            .map(|(i, o)| ScheduleSlot { op_id: o.id, bundle: None, delay_ms: (i as u64 + 1) * 1500 })
            .collect();

        // jit funding (each target a distinct source — no star).
        let funding = Funding {
            edges: vols.iter().map(|v| FundingEdge { from: addr(), to: v.pubkey.clone(), lamports: 3_000_000 }).collect(),
        };

        let plan = Plan { mint_address: Some(mint), ops: ops.clone(), funding, schedule: crate::plan::Schedule { slots } };

        // Persona-drawn disguises (varied fees) + fresh nonce per op + correct
        // account shape on the swap legs.
        let profiles: Vec<SendProfile> = ops
            .iter()
            .map(|op| {
                let persona = set.assign(&op.wallet.pubkey);
                let disguise = for_op(persona, op, &cfg);
                let fee = addr();
                let mut accounts: Vec<String> = (0..20).map(|_| addr()).collect();
                accounts[PUMP_FEE_RECIPIENT_INDEX] = fee.clone();
                SendProfile {
                    op_id: op.id,
                    disguise: Some(disguise),
                    nonce_account: Some(addr()), // fresh per op
                    durable_nonce: false,
                    close_token_ata: false,
                    accounts,
                    fee_recipient: Some(fee),
                }
            })
            .collect();

        let report = audit_with(&plan, &profiles, AuditConfig::default());
        assert!(!report.hard_reject, "clean plan has no hard reject");
        assert!(report.passed(false), "clean plan must pass: {:?}", report.findings);
        assert_eq!(report.score, 0, "clean plan has zero fingerprint score: {:?}", report.findings);
    }
}
