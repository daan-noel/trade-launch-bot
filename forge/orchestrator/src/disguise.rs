//! **Disguise** (forge-only) — turn an [`Operation`] + its wallet's [`Persona`]
//! into a concrete, *landing-guaranteed* set of send parameters: which catalog
//! variant to encode it as, the CU limit, the CU price, and the tip. This is where
//! "personas, not independent per-field jitter" becomes bytes: every value is drawn
//! from the persona's ranges (so the wallet's ops cohere), yet each op seeds its
//! own jitter so two ops of the same wallet differ in CU/tip/variant.
//!
//! **Landing guarantees** (the disguise must never make a tx fail):
//!   - `cu_limit = real_consumption(cfg, chosen_variant) + persona_headroom` — the
//!     headroom is ≥ 0, so the limit is always ≥ the measured consumption for that
//!     path; a disguise can raise cost, never starve it.
//!   - `cu_price ≥ persona.cu_price.min > 0` — a snipe's priority fee is never
//!     dropped to zero; it stays landing-competitive.
//!   - the chosen variant is drawn ONLY from encodings the op's `Amount` can take
//!     (same `denom`) and the same curve/AMM `stage` as the original, so a disguise
//!     never turns a valid op into one the provider would reject.
//!
//! **Tips**: a snipe always tips (latency-critical); a non-snipe buy tips with the
//! persona's probability; a **sell defaults to no tip** (un-bundled / direct-send,
//! `tip: None`) — the plan's stated default. Create/transfer never tip.
//!
//! CU cost-model floors are read from `executor_core::config::ComputeBudgetCfg` —
//! the single source of truth the ix builders also size against — so the disguise
//! floor can't drift from real consumption.

use crate::personas::Persona;
use crate::plan::{Intent, OpKind, Operation};
use crate::provider::denom_accepts;
use crate::rng::SplitMix64;
use executor_core::config::ComputeBudgetCfg;
use pump_trader::{spec, Stage, VariantSpec};
use serde::Serialize;

/// A native SOL / SPL transfer's CU floor. The executor cost model
/// ([`ComputeBudgetCfg`]) sizes swaps/creates but has no transfer field (transfers
/// aren't a trade path); a system transfer costs ~150 CU and an SPL transfer a few
/// thousand, so this conservative floor lands either with headroom to spare.
pub const TRANSFER_CU: u32 = 10_000;

/// The concrete, ready-to-send disguise for one op. Serializable so it can ride the
/// dry-run preview / persisted plan (the operator sees exactly how a wallet will
/// look on-chain before anything sends).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Disguise {
    /// The catalog variant this op will encode as (may differ from the op's
    /// authored variant — same kind, denom, and stage; a persona-preferred
    /// encoding).
    pub variant: String,
    /// Compute-unit limit — always ≥ the measured real consumption for the chosen
    /// path (guaranteed to land).
    pub cu_limit: u32,
    /// Compute-unit price (micro-lamports per CU) — the priority-fee rate.
    pub cu_price_micro_lamports: u64,
    /// Jito tip in lamports, if this op tips. `None` for a sell / create / transfer
    /// by default, and for a non-snipe buy that didn't draw a tip.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tip_lamports: Option<u64>,
}

impl Disguise {
    /// Overwrite the op's authored variant with the disguised one (the single
    /// `Operation` field a disguise mutates; CU/price/tip are send-time params
    /// applied at the Phase F cutover, carried here for preview/persistence).
    pub fn apply_variant(&self, op: &mut Operation) {
        op.variant = self.variant.clone();
    }
}

/// The real (measured) CU consumption floor for a variant's path, read from the
/// executor cost-model SSOT. The disguise adds persona headroom ON TOP of this, so
/// the resulting `cu_limit` is always sufficient.
pub fn real_consumption_cu(cfg: &ComputeBudgetCfg, spec: &VariantSpec) -> u32 {
    use pump_trader::VariantKind::*;
    match (spec.kind, spec.stage) {
        (Create, _) => cfg.curve_create_cu,
        (Buy, Stage::Amm) | (Sell, Stage::Amm) => cfg.amm_cu,
        (Buy, _) => cfg.curve_buy_cu,
        (Sell, _) => cfg.curve_sell_cu,
        (TransferSol, _) | (TransferToken, _) => TRANSFER_CU,
    }
}

/// Draw the landing disguise for `op` under `persona`, using `rng` for jitter (the
/// caller seeds `rng` per-op — see [`for_op`] — so the draw is reproducible yet
/// varies op-to-op). Pure: no network, no signer.
pub fn draw(
    persona: &Persona,
    op: &Operation,
    cfg: &ComputeBudgetCfg,
    rng: &mut SplitMix64,
) -> Disguise {
    // 1) Choose the encoding: a persona-preferred variant that (a) matches the op's
    //    kind, (b) can encode the op's amount (same denom), and (c) stays on the
    //    same curve/AMM stage. Falls back to the authored variant if the persona
    //    offers no compatible one — the authored variant is always legal.
    let chosen = choose_variant(persona, op, rng);
    let chosen_spec = spec(&chosen).expect("chosen variant is a catalog entry");

    // 2) CU limit = measured floor + persona headroom (headroom ≥ 0 ⇒ always lands).
    let floor = real_consumption_cu(cfg, chosen_spec);
    let headroom = rng.range(persona.cu_headroom.min, persona.cu_headroom.max);
    let cu_limit = floor.saturating_add(headroom.min(u32::MAX as u64) as u32);

    // 3) CU price within the persona range; min > 0 guarantees a snipe never
    //    zero-fees. Clamp defensively in case a hand-built persona skipped validate.
    let cu_price_micro_lamports = rng.range(persona.cu_price.min.max(1), persona.cu_price.max.max(1));

    // 4) Tip policy: sells never tip by default; a snipe always tips; a non-snipe
    //    buy tips with the persona's probability; create/transfer never tip.
    let tip_lamports = tip(persona, op, rng);

    Disguise { variant: chosen, cu_limit, cu_price_micro_lamports, tip_lamports }
}

/// Convenience: seed a per-op RNG from the acting wallet + op id and draw. Same
/// wallet ⇒ same persona (assigned upstream) and a reproducible-but-distinct jitter
/// stream per op — the property Gate D checks (ops share a persona, differ in
/// jittered fields).
pub fn for_op(persona: &Persona, op: &Operation, cfg: &ComputeBudgetCfg) -> Disguise {
    let mut rng = SplitMix64::seeded(&op.wallet.pubkey, op.id as u64);
    draw(persona, op, cfg, &mut rng)
}

/// Pick a persona-preferred variant compatible with the op (kind + denom + stage),
/// or fall back to the op's authored variant.
fn choose_variant(persona: &Persona, op: &Operation, rng: &mut SplitMix64) -> String {
    let pool: &[String] = match op.kind {
        OpKind::Buy => &persona.buy_variants,
        OpKind::Sell => &persona.sell_variants,
        // Create/transfer aren't disguised by encoding choice — a create is a
        // create; keep whatever the op authored.
        _ => return op.variant.clone(),
    };

    let authored_stage = spec(&op.variant).map(|s| s.stage);
    let compatible: Vec<&String> = pool
        .iter()
        .filter(|name| match spec(name) {
            Some(s) => {
                s.kind == op.kind
                    && denom_accepts(s.denom, op.amount)
                    && Some(s.stage) == authored_stage
            }
            None => false,
        })
        .collect();

    if compatible.is_empty() {
        // No persona encoding fits this amount/stage — the authored variant already
        // passed prepare(), so keep it (never emit something the provider rejects).
        return op.variant.clone();
    }
    compatible[rng.index(compatible.len())].clone()
}

/// The tip decision for an op.
fn tip(persona: &Persona, op: &Operation, rng: &mut SplitMix64) -> Option<u64> {
    match op.kind {
        // Sells default un-bundled / direct-send: no tip.
        OpKind::Sell => None,
        OpKind::Buy => {
            let tips = match op.intent {
                // A snipe is latency-critical — always tips.
                Intent::Snipe => true,
                // Other buys tip with the persona's probability.
                _ => rng.chance(persona.non_snipe_tip_pct),
            };
            tips.then(|| rng.range(persona.tip_lamports.min, persona.tip_lamports.max))
        }
        // Create / transfer never tip.
        _ => None,
    }
}

/// Disguise every op in a slice under a persona set — assigns each op's acting
/// wallet its sticky persona, then draws a per-op disguise. Returns the disguises
/// aligned to the input order. This is what the launcher/manage flows call (Phase F)
/// to size + encode a whole `Plan`.
pub fn disguise_ops(
    set: &crate::personas::PersonaSet,
    ops: &[Operation],
    cfg: &ComputeBudgetCfg,
) -> Vec<Disguise> {
    ops.iter()
        .map(|op| {
            let persona = set.assign(&op.wallet.pubkey);
            for_op(persona, op, cfg)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::personas::PersonaSet;
    use crate::plan::{Amount, Operation, Role, WalletRef};
    use solana_sdk::signature::{Keypair, Signer};

    fn addr() -> String {
        Keypair::new().pubkey().to_string()
    }

    /// A snipe buy always tips and never zero-fees; its CU limit clears the floor.
    #[test]
    fn snipe_always_tips_and_clears_cu_floor() {
        let set = PersonaSet::builtin();
        let cfg = ComputeBudgetCfg::default();
        let wallet = WalletRef::new(addr());
        let op = Operation::buy(
            0,
            "buy_exact_sol_in",
            Role::Dev,
            Intent::Snipe,
            wallet.clone(),
            addr(),
            Amount::ExactQuote(1_000_000),
            Some(500),
            vec![],
        );
        let persona = set.assign(&op.wallet.pubkey);
        let d = for_op(persona, &op, &cfg);

        assert!(d.tip_lamports.is_some(), "a snipe must tip");
        assert!(d.cu_price_micro_lamports > 0, "a snipe never zero-fees");
        // buy_exact_sol_in is curve; limit must clear the curve-buy floor.
        assert!(d.cu_limit >= cfg.curve_buy_cu, "cu_limit must cover real consumption");
    }

    /// A sell defaults to no tip.
    #[test]
    fn sell_defaults_to_no_tip() {
        let set = PersonaSet::builtin();
        let cfg = ComputeBudgetCfg::default();
        let op = Operation::sell(0, "sell", Role::Volume, WalletRef::new(addr()), addr(), 5_000, Some(500), vec![]);
        let persona = set.assign(&op.wallet.pubkey);
        let d = for_op(persona, &op, &cfg);
        assert_eq!(d.tip_lamports, None, "sells default to no tip");
        assert!(d.cu_limit >= cfg.curve_sell_cu);
    }

    /// The disguised variant always keeps the op's amount encodable (denom-safe),
    /// so a disguised op still passes the provider.
    #[test]
    fn disguised_variant_stays_denom_compatible() {
        let set = PersonaSet::builtin();
        let cfg = ComputeBudgetCfg::default();
        // ExactQuote amount ⇒ only SOL-in encodings are compatible.
        let op = Operation::buy(
            0,
            "buy_exact_sol_in",
            Role::Volume,
            Intent::MakeVolume,
            WalletRef::new(addr()),
            addr(),
            Amount::ExactQuote(1_000_000),
            Some(300),
            vec![],
        );
        let persona = set.assign(&op.wallet.pubkey);
        let d = for_op(persona, &op, &cfg);
        let s = spec(&d.variant).unwrap();
        assert!(denom_accepts(s.denom, op.amount), "disguise must stay denom-compatible");
        assert_eq!(s.kind, OpKind::Buy);
    }

    /// Same wallet ⇒ same persona across ops, but per-op jitter differs — the Gate D
    /// property. We build several ops for ONE wallet and assert the persona is
    /// identical while the drawn disguises are not all identical.
    #[test]
    fn same_wallet_shares_persona_but_jitter_varies() {
        let set = PersonaSet::builtin();
        let cfg = ComputeBudgetCfg::default();
        let wallet = WalletRef::new(addr());
        let persona_name = set.assign(&wallet.pubkey).name.clone();

        let mut disguises = Vec::new();
        for id in 0..8u32 {
            let op = Operation::buy(
                id,
                "buy_exact_sol_in",
                Role::Volume,
                Intent::MakeVolume,
                wallet.clone(),
                addr(),
                Amount::ExactQuote(1_000_000),
                Some(300),
                vec![],
            );
            // Persona is a pure function of the (unchanged) wallet ⇒ always the same.
            assert_eq!(set.assign(&op.wallet.pubkey).name, persona_name);
            disguises.push(for_op(set.assign(&op.wallet.pubkey), &op, &cfg));
        }

        // The 8 disguises must not be byte-identical (per-op jitter is real).
        let all_same = disguises.iter().all(|d| *d == disguises[0]);
        assert!(!all_same, "per-op jitter must vary the disguise across a wallet's ops");
    }

    /// Reproducibility: the same op re-drawn yields the same disguise (replayable).
    #[test]
    fn draw_is_reproducible() {
        let set = PersonaSet::builtin();
        let cfg = ComputeBudgetCfg::default();
        let op = Operation::buy(
            3,
            "buy_exact_sol_in",
            Role::Bundler,
            Intent::Snipe,
            WalletRef::new(addr()),
            addr(),
            Amount::ExactQuote(2_000_000),
            Some(500),
            vec![],
        );
        let persona = set.assign(&op.wallet.pubkey);
        assert_eq!(for_op(persona, &op, &cfg), for_op(persona, &op, &cfg));
    }
}
