//! **Macros** — the high-level plan builders. Each expands one operator intent
//! ("launch this token with a dev-buy and 3 bundlers", "make volume", "exit all",
//! "sweep to treasury", "fund the pool") into a `Vec<Operation>` with the **correct
//! `deps`**, drawing ids from a shared [`IdSeq`] so several macros compose into one
//! [`Plan`] without id collisions.
//!
//! These own the dependency wiring so no call site hand-rolls it: a bundler buy
//! `deps` the create; a consolidate `deps` the exits it sweeps the proceeds of.
//! **Consolidate + fund emit a typed [`OpKind::TransferSol`] op — never a raw
//! `system_instruction::transfer`** — so every SOL move is on the auditable `Plan`.
//!
//! Amounts stay canonical ([`Amount`]) and variants stay catalog names — a macro
//! never bakes in a discriminator or a min_out; the provider validates and the
//! disguise sizes at send.

use crate::plan::{Amount, IdSeq, Intent, OpId, Operation, Role, WalletRef};

/// The default native-SOL transfer encoding for fund/consolidate legs (a plain
/// `system_instruction::transfer`; catalog variant `system_transfer`).
pub const DEFAULT_SOL_TRANSFER_VARIANT: &str = "system_transfer";

// ---------------------------------------------------------------------------
// bundle_launch
// ---------------------------------------------------------------------------

/// One launch-bundle co-buyer.
#[derive(Debug, Clone)]
pub struct BundlerLeg {
    pub wallet: WalletRef,
    /// SOL (lamports) this bundler spends — encoded as an `ExactQuote` buy.
    pub sol: u64,
    pub slippage_bps: Option<u64>,
    /// Hand-picked buy variant for this leg (`leg_structures[i].variant`). `None`
    /// ⇒ fall back to `BundleLaunch.buy_variant`, and the disguise stays free to
    /// swap the encoding (the un-authored, persona-driven path).
    pub variant: Option<String>,
    /// Hand-picked ix layout (decoration step order) for this leg. `None` ⇒ the
    /// builder's `canonical_buy` shape.
    pub layout: Option<Vec<executor_core::DecoStep>>,
}

/// The inputs to a bundled launch: a create, a dev-buy, and N bundler co-buys, all
/// on one fresh mint.
#[derive(Debug, Clone)]
pub struct BundleLaunch {
    pub mint: String,
    pub dev: WalletRef,
    /// The create encoding (`create` / `create_v2`).
    pub create_variant: String,
    /// The default buy encoding for the bundler legs (an `ExactQuote`/SOL-in variant).
    pub buy_variant: String,
    /// The dev-buy curve encoding — any of the four catalog buy variants (`buy`,
    /// `buy_exact_sol_in`, `buy_v2`, `buy_exact_quote_in_v2`). Independent of the
    /// bundler `buy_variant` so the dev buy can diverge from the co-buys (fingerprint
    /// diversity). The op is LOCKED so the disguise won't swap the operator's choice.
    pub dev_buy_variant: String,
    /// Dev-buy spend (lamports). `0` ⇒ create-only (no dev-buy op emitted).
    pub dev_buy_sol: u64,
    /// Dev-buy slippage; `None` keeps the historical unprotected launch leg
    /// (`min_out = 1`).
    pub dev_slippage_bps: Option<u64>,
    pub bundlers: Vec<BundlerLeg>,
}

/// Expand a bundled launch → `[create, dev_buy?, bundler_buys…]`. The dev-buy and
/// every bundler buy `deps` the create (they must land after it — same slot for a
/// Jito bundle). Ids are drawn from `seq`.
pub fn bundle_launch(seq: &mut IdSeq, p: &BundleLaunch) -> Vec<Operation> {
    let mut ops = Vec::with_capacity(2 + p.bundlers.len());

    let create_id = seq.next();
    ops.push(Operation::create(create_id, &p.create_variant, p.dev.clone(), p.mint.clone()));

    if p.dev_buy_sol > 0 {
        // The dev-buy carries its OWN encoding (`dev_buy_variant`), independent of the
        // bundler default. Locked (authored) so the disguise keeps the operator's
        // chosen variant — only CU/tip stay persona-jittered.
        ops.push(
            Operation::buy(
                seq.next(),
                &p.dev_buy_variant,
                Role::Dev,
                Intent::Snipe,
                p.dev.clone(),
                p.mint.clone(),
                Amount::ExactQuote(p.dev_buy_sol),
                p.dev_slippage_bps,
                vec![create_id],
            )
            .with_authored(None, true),
        );
    }

    for b in &p.bundlers {
        // Hand-picked variant when the operator authored one (`leg_structures`),
        // else the launch's default buy encoding. An authored variant is LOCKED so
        // the disguise won't swap it; an authored layout rides along either way.
        let authored = b.variant.is_some();
        let variant = b.variant.clone().unwrap_or_else(|| p.buy_variant.clone());
        ops.push(
            Operation::buy(
                seq.next(),
                &variant,
                Role::Bundler,
                Intent::Snipe,
                b.wallet.clone(),
                p.mint.clone(),
                Amount::ExactQuote(b.sol),
                b.slippage_bps,
                vec![create_id],
            )
            .with_authored(b.layout.clone(), authored),
        );
    }

    ops
}

// ---------------------------------------------------------------------------
// volume_make
// ---------------------------------------------------------------------------

/// One make-volume leg — a buy or a sell by a volume wallet. Both carry the
/// `MakeVolume` intent (wash/organic-looking churn), distinct from an `Exit`.
#[derive(Debug, Clone)]
pub enum VolumeLeg {
    /// A SOL-in buy (spend `sol` lamports).
    Buy { wallet: WalletRef, variant: String, sol: u64, slippage_bps: Option<u64> },
    /// A tokens-in sell (`tokens_base` units).
    Sell { wallet: WalletRef, variant: String, tokens_base: u64, slippage_bps: Option<u64> },
}

/// Expand make-volume legs → independent `MakeVolume` buy/sell ops on `mint`
/// (`Role::Volume`). Legs have no cross-deps (they churn independently); scheduling
/// / spacing is the `Schedule`'s job, not a dep chain.
pub fn volume_make(seq: &mut IdSeq, mint: &str, legs: &[VolumeLeg]) -> Vec<Operation> {
    legs.iter()
        .map(|leg| match leg {
            VolumeLeg::Buy { wallet, variant, sol, slippage_bps } => Operation::buy(
                seq.next(),
                variant,
                Role::Volume,
                Intent::MakeVolume,
                wallet.clone(),
                mint.to_string(),
                Amount::ExactQuote(*sol),
                *slippage_bps,
                vec![],
            ),
            VolumeLeg::Sell { wallet, variant, tokens_base, slippage_bps } => Operation::sell_with(
                seq.next(),
                variant,
                Role::Volume,
                Intent::MakeVolume,
                wallet.clone(),
                mint.to_string(),
                *tokens_base,
                *slippage_bps,
                vec![],
            ),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// exit
// ---------------------------------------------------------------------------

/// One position to close.
#[derive(Debug, Clone)]
pub struct ExitLeg {
    pub wallet: WalletRef,
    /// The wallet's role (dev/bundler/volume) — preserved for the auditor.
    pub role: Role,
    pub variant: String,
    pub tokens_base: u64,
    pub slippage_bps: Option<u64>,
}

/// Expand exit legs → independent `Sell` / `Exit` ops on `mint`. Returns the ops
/// and (as their ids) the sell set a following `consolidate` should `deps`.
pub fn exit(seq: &mut IdSeq, mint: &str, legs: &[ExitLeg]) -> Vec<Operation> {
    legs.iter()
        .map(|leg| {
            Operation::sell(
                seq.next(),
                &leg.variant,
                leg.role,
                leg.wallet.clone(),
                mint.to_string(),
                leg.tokens_base,
                leg.slippage_bps,
                vec![],
            )
        })
        .collect()
}

// ---------------------------------------------------------------------------
// consolidate
// ---------------------------------------------------------------------------

/// One wallet to sweep to treasury.
#[derive(Debug, Clone)]
pub struct Sweep {
    pub wallet: WalletRef,
    /// The swept wallet's role (bundler/volume) — the signer's role, which the
    /// auditor reads for a star-shaped own-graph tell.
    pub role: Role,
    pub lamports: u64,
}

/// Expand a consolidate → typed `TransferSol` / `Consolidate` ops, each sweeping a
/// wallet's SOL to `treasury`. Every op `deps` `after` (the exit ids whose proceeds
/// it consolidates), so a sweep never runs before the sell that funded it.
pub fn consolidate(
    seq: &mut IdSeq,
    treasury: &str,
    sweeps: &[Sweep],
    after: &[OpId],
) -> Vec<Operation> {
    sweeps
        .iter()
        .map(|s| {
            Operation::transfer_sol_as(
                seq.next(),
                DEFAULT_SOL_TRANSFER_VARIANT,
                s.role,
                Intent::Consolidate,
                s.wallet.clone(),
                treasury.to_string(),
                s.lamports,
                after.to_vec(),
            )
        })
        .collect()
}

// ---------------------------------------------------------------------------
// fund
// ---------------------------------------------------------------------------

/// One wallet to fund and by how much.
#[derive(Debug, Clone)]
pub struct FundTarget {
    pub wallet: WalletRef,
    pub lamports: u64,
}

/// Expand a fund → typed `TransferSol` / `Fund` ops, each moving SOL from `source`
/// (the treasury/funding wallet) to a target. Independent (no deps). The typed op
/// replaces the raw `system_instruction::transfer` funding bypass — same plain
/// transfer, now auditable.
pub fn fund(seq: &mut IdSeq, source: &WalletRef, targets: &[FundTarget]) -> Vec<Operation> {
    targets
        .iter()
        .map(|t| {
            Operation::transfer_sol_as(
                seq.next(),
                DEFAULT_SOL_TRANSFER_VARIANT,
                Role::Treasury,
                Intent::Fund,
                source.clone(),
                t.wallet.pubkey.clone(),
                t.lamports,
                vec![],
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::prepare;
    use crate::plan::Plan;
    use solana_sdk::signature::{Keypair, Signer};

    fn addr() -> String {
        Keypair::new().pubkey().to_string()
    }
    fn w() -> WalletRef {
        WalletRef::new(addr())
    }

    /// bundle_launch wires every buy to `deps` the create, and the whole plan
    /// dry-runs clean through the provider.
    #[test]
    fn bundle_launch_wires_deps_and_validates() {
        let mut seq = IdSeq::new(0);
        let p = BundleLaunch {
            mint: addr(),
            dev: w(),
            create_variant: "create_v2".to_string(),
            buy_variant: "buy_exact_sol_in".to_string(),
            dev_buy_variant: "buy_exact_sol_in".to_string(),
            dev_buy_sol: 1_500_000,
            dev_slippage_bps: None,
            bundlers: vec![
                BundlerLeg { wallet: w(), sol: 800_000, slippage_bps: Some(500), variant: None, layout: None },
                BundlerLeg { wallet: w(), sol: 900_000, slippage_bps: Some(500), variant: None, layout: None },
            ],
        };
        let ops = bundle_launch(&mut seq, &p);
        assert_eq!(ops.len(), 4, "create + dev-buy + 2 bundlers");
        let create_id = ops[0].id;
        for buy in &ops[1..] {
            assert_eq!(buy.deps, vec![create_id], "every buy deps the create");
        }
        let plan = Plan::for_mint(p.mint.clone(), ops);
        prepare(&plan).expect("launch plan must validate");
    }

    /// A full compose — fund → launch → volume → exit → consolidate — shares one
    /// IdSeq (unique ids) and validates end to end; the consolidate deps the exits.
    #[test]
    fn composed_plan_shares_idseq_and_validates() {
        let mint = addr();
        let treasury = addr();
        let dev = w();
        let vol1 = w();
        let vol2 = w();

        let mut seq = IdSeq::new(0);
        let mut ops = Vec::new();

        ops.extend(fund(
            &mut seq,
            &WalletRef::new(treasury.clone()),
            &[FundTarget { wallet: dev.clone(), lamports: 5_000_000 }],
        ));
        ops.extend(bundle_launch(
            &mut seq,
            &BundleLaunch {
                mint: mint.clone(),
                dev: dev.clone(),
                create_variant: "create_v2".to_string(),
                buy_variant: "buy_exact_sol_in".to_string(),
                dev_buy_variant: "buy_exact_sol_in".to_string(),
                dev_buy_sol: 1_000_000,
                dev_slippage_bps: None,
                bundlers: vec![],
            },
        ));
        ops.extend(volume_make(
            &mut seq,
            &mint,
            &[
                VolumeLeg::Buy { wallet: vol1.clone(), variant: "buy_exact_sol_in".to_string(), sol: 500_000, slippage_bps: Some(300) },
                VolumeLeg::Sell { wallet: vol1.clone(), variant: "sell".to_string(), tokens_base: 10_000, slippage_bps: Some(300) },
            ],
        ));
        let exit_ops = exit(
            &mut seq,
            &mint,
            &[ExitLeg { wallet: vol2.clone(), role: Role::Volume, variant: "sell".to_string(), tokens_base: 50_000, slippage_bps: Some(500) }],
        );
        let exit_ids: Vec<OpId> = exit_ops.iter().map(|o| o.id).collect();
        ops.extend(exit_ops);
        ops.extend(consolidate(
            &mut seq,
            &treasury,
            &[Sweep { wallet: vol2.clone(), role: Role::Volume, lamports: 2_000_000 }],
            &exit_ids,
        ));

        // Ids are unique (shared seq).
        let mut ids: Vec<OpId> = ops.iter().map(|o| o.id).collect();
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n, "shared IdSeq ⇒ unique ids");

        // The consolidate op deps the exit.
        let consolidate_op = ops.last().unwrap();
        assert_eq!(consolidate_op.intent, Intent::Consolidate);
        assert_eq!(consolidate_op.deps, exit_ids, "consolidate deps the exits");

        let plan = Plan::for_mint(mint, ops);
        prepare(&plan).expect("composed plan must validate through the providers");
    }

    /// Gate D: volume_make + exit produce a plan where a wallet's ops share its
    /// persona but differ in jittered CU/tip/variant; sells default `tip: None`.
    #[test]
    fn gate_d_persona_coherence_and_sell_no_tip() {
        use crate::disguise::for_op;
        use crate::personas::PersonaSet;
        use executor_core::config::ComputeBudgetCfg;

        let set = PersonaSet::builtin();
        let cfg = ComputeBudgetCfg::default();
        let mint = addr();
        let vol = w(); // ONE wallet doing several volume legs

        let mut seq = IdSeq::new(0);
        let mut ops = volume_make(
            &mut seq,
            &mint,
            &[
                VolumeLeg::Buy { wallet: vol.clone(), variant: "buy_exact_sol_in".to_string(), sol: 500_000, slippage_bps: Some(300) },
                VolumeLeg::Buy { wallet: vol.clone(), variant: "buy_exact_sol_in".to_string(), sol: 600_000, slippage_bps: Some(300) },
                VolumeLeg::Buy { wallet: vol.clone(), variant: "buy_exact_sol_in".to_string(), sol: 700_000, slippage_bps: Some(300) },
            ],
        );
        ops.extend(exit(
            &mut seq,
            &mint,
            &[ExitLeg { wallet: vol.clone(), role: Role::Volume, variant: "sell".to_string(), tokens_base: 25_000, slippage_bps: Some(500) }],
        ));

        // Same wallet ⇒ same persona for every op.
        let persona_name = set.assign(&vol.pubkey).name.clone();
        for op in &ops {
            assert_eq!(set.assign(&op.wallet.pubkey).name, persona_name);
        }

        // Disguise each op; the sell has no tip, and the buy disguises vary.
        let disguises: Vec<_> = ops
            .iter()
            .map(|op| for_op(set.assign(&op.wallet.pubkey), op, &cfg))
            .collect();

        // The sell (last op) defaults to no tip.
        assert_eq!(disguises.last().unwrap().tip_lamports, None, "sell defaults to no tip");

        // The three buy disguises are not all identical (per-op jitter).
        let buys = &disguises[..3];
        let all_same = buys.iter().all(|d| *d == buys[0]);
        assert!(!all_same, "a wallet's volume buys must differ in jittered CU/tip/variant");
    }
}
