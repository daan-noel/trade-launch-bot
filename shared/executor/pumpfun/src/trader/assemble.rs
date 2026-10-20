// ============================================================
// assemble — the IxLayout interpreter.
//
// Turns a solana-free `executor_core::IxLayout` (the *shape* — which decoration
// ixs wrap the on-chain Core, and in what order) into a real `Vec<Instruction>`.
// This is the one place layout data becomes concrete instructions; it lives in
// the venue crate because it constructs `ComputeBudgetInstruction` +
// `system_instruction::transfer` (solana deps the layout types deliberately lack).
//
// The interpreter NEVER reshapes the Core block or the ATA ixs — it only orders
// the opaque blocks its caller hands it. Byte-for-byte, `assemble(canonical_*, …)`
// reproduces the pre-refactor hand-pushed sequences (pinned by golden tests below).
// ============================================================

use super::PumpFunTrader;
use executor_core::{draw_compute_budget, pick_layout, DecoStep, IxLayout};
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction, instruction::Instruction, pubkey::Pubkey,
    system_instruction,
};
use std::sync::OnceLock;

/// The opaque instruction blocks + decoration scalars an [`IxLayout`] orders. The
/// caller (bundle-buy / create builder) fills `core` (and `ata` for a standalone
/// buy) with the fixed on-chain ixs; `assemble` places them plus the CU/tip
/// decorations per the layout's step order.
pub struct IxParts {
    /// The opaque on-chain block: `[buy]`, `[create]`, or the fused
    /// `[create, ata, buy]`. Placed verbatim at the `Core` step.
    pub core: Vec<Instruction>,
    /// Buyer ATA creation ix(s) — one for a v1 buy, two (base + WSOL) for a v2
    /// buy, empty when the ATA is folded into `core` (create). Placed at the
    /// `CreateAta` step.
    pub ata: Vec<Instruction>,
    pub cu_limit: u32,
    pub cu_price: u64,
    pub tip_lamports: u64,
    pub tip_account: Pubkey,
    pub payer: Pubkey,
}

/// Walk `layout.steps`, emitting each step's instruction(s) in order. Steps that
/// carry a block (`Core`, `CreateAta`) extend from the corresponding `parts`
/// field; CU steps build a fresh compute-budget ix from the scalar; `Tip` builds
/// the inline transfer. A step absent from the layout is simply not emitted.
pub fn assemble(layout: &IxLayout, parts: IxParts) -> Vec<Instruction> {
    let mut out = Vec::with_capacity(layout.steps.len() + parts.core.len() + parts.ata.len());
    for step in &layout.steps {
        match step {
            DecoStep::CuLimit => out.push(ComputeBudgetInstruction::set_compute_unit_limit(
                parts.cu_limit,
            )),
            DecoStep::CuPrice => out.push(ComputeBudgetInstruction::set_compute_unit_price(
                parts.cu_price,
            )),
            DecoStep::CreateAta => out.extend_from_slice(&parts.ata),
            DecoStep::Core => out.extend_from_slice(&parts.core),
            // The Jito tip MUST stay an inline `system_instruction::transfer`:
            // Jito's auction scans static account keys and does not resolve ALTs,
            // so this ix can never be compressed behind a lookup table.
            DecoStep::Tip => out.push(system_instruction::transfer(
                &parts.payer,
                &parts.tip_account,
                parts.tip_lamports,
            )),
        }
    }
    out
}

/// Which configured CU limit ([`executor_core::ComputeBudgetCfg`]) a single-tx
/// buy/sell is sized from.
#[derive(Clone, Copy, Debug)]
pub(super) enum CuPath {
    CurveBuy,
    CurveSell,
    Amm,
}

impl PumpFunTrader {
    /// Wrap a single-tx buy/sell's `core` (and its account-creation `ata`
    /// prefix, empty when none) in the CU limit, CU price and Sender tip.
    ///
    /// With [`executor_core::FeeShapeCfg::enabled`] off, this is the configured
    /// limit/price in the canonical order `[cu_limit, cu_price, ata, core, tip]`.
    /// On, each call draws a CU pair whose fee is at most the configured pair's,
    /// a random Sender tip account, and a random legal order (60 with an `ata`
    /// block, 24 without). The tip amount is the same either way. Pure CPU, no
    /// RPC; a durable-nonce advance is still prepended at index 0 at signing.
    pub(super) fn decorate(
        &self,
        path: CuPath,
        ata: Vec<Instruction>,
        core: Vec<Instruction>,
        tip_level: u8,
    ) -> Vec<Instruction> {
        static CANONICAL: OnceLock<IxLayout> = OnceLock::new();
        let compute = &self.config.compute;
        let shape = &self.config.fee_shape;
        let floor_cu = match path {
            CuPath::CurveBuy => compute.curve_buy_cu,
            CuPath::CurveSell => compute.curve_sell_cu,
            CuPath::Amm => compute.amm_cu,
        };
        let (tip_account, tip_lamports) = self.sender_tip(tip_level);
        let (cu_limit, cu_price, layout) = if shape.enabled {
            let mut rng = rand::thread_rng();
            let (limit, price) =
                draw_compute_budget(floor_cu, compute.price_micro_lamports, shape, &mut rng);
            (limit, price, pick_layout(!ata.is_empty(), &mut rng))
        } else {
            (
                floor_cu,
                compute.price_micro_lamports,
                CANONICAL.get_or_init(IxLayout::canonical_buy),
            )
        };
        assemble(
            layout,
            IxParts {
                core,
                ata,
                cu_limit,
                cu_price,
                tip_lamports,
                tip_account,
                payer: self.config.signer.pubkey(),
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use executor_core::LayoutKind;

    fn ix(tag: u8) -> Instruction {
        Instruction {
            program_id: Pubkey::new_unique(),
            accounts: vec![],
            data: vec![tag],
        }
    }

    fn parts(core: Vec<Instruction>, ata: Vec<Instruction>) -> IxParts {
        IxParts {
            core,
            ata,
            cu_limit: 250_000,
            cu_price: 750_000,
            tip_lamports: 200_000,
            tip_account: Pubkey::new_unique(),
            payer: Pubkey::new_unique(),
        }
    }

    /// Canonical buy → the full 5-slot shape in the right order, ATA expanded to
    /// its 2 ixs (v2), Core placed opaque.
    #[test]
    fn canonical_buy_orders_all_slots() {
        let core = vec![ix(9)];
        let ata = vec![ix(1), ix(2)]; // base + wsol
        let out = assemble(&IxLayout::canonical_buy(), parts(core.clone(), ata.clone()));
        // [cu_limit, cu_price, ata0, ata1, core, tip]  → 6 ixs
        assert_eq!(out.len(), 6);
        assert_eq!(out[0].program_id, solana_sdk::compute_budget::id());
        assert_eq!(out[1].program_id, solana_sdk::compute_budget::id());
        assert_eq!(out[2].data, vec![1]);
        assert_eq!(out[3].data, vec![2]);
        assert_eq!(out[4].data, vec![9]); // Core, untouched
        assert_eq!(out[5].program_id, solana_sdk::system_program::id()); // tip transfer
    }

    /// A hand-built partial layout only emits its listed steps — the Core block's
    /// accounts are identical whether or not the decorations are present.
    #[test]
    fn partial_layout_emits_only_listed_steps() {
        let core = vec![ix(9)];
        let ata = vec![ix(1)];
        let lean = IxLayout {
            steps: vec![DecoStep::CreateAta, DecoStep::Core],
        };
        let out = assemble(&lean, parts(core.clone(), ata.clone()));
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].data, vec![1]);
        assert_eq!(out[1], core[0]); // Core placed opaque, byte-identical
                                     // Sanity: this lean layout is a legal non-snipe buy.
        assert!(lean.validate(LayoutKind::Buy, false).is_ok());
    }

    fn trader(shape_on: bool) -> PumpFunTrader {
        let mut cfg = crate::trader::TraderConfig::new(
            "http://localhost".into(),
            vec!["http://localhost".into()],
            std::sync::Arc::new(solana_sdk::signature::Keypair::new()),
            vec![Pubkey::new_unique()],
        );
        cfg.fee_shape.enabled = shape_on;
        PumpFunTrader::new(std::sync::Arc::new(cfg))
    }

    /// `(limit, price)` read back off the compute-budget ixs (tag 2 = limit u32,
    /// tag 3 = price u64, little-endian), each present exactly once.
    fn cu_pair(ixs: &[Instruction]) -> (u32, u64) {
        let (mut limit, mut price) = (None, None);
        for ix in ixs.iter().filter(|ix| ix.program_id == solana_sdk::compute_budget::id()) {
            match ix.data[0] {
                2 => assert!(limit.replace(u32::from_le_bytes(ix.data[1..5].try_into().unwrap())).is_none()),
                3 => assert!(price.replace(u64::from_le_bytes(ix.data[1..9].try_into().unwrap())).is_none()),
                t => panic!("unexpected compute-budget tag {t}"),
            }
        }
        (limit.unwrap(), price.unwrap())
    }

    /// Off: the configured pair, the fixed tip account, the canonical order —
    /// what every single-tx buy/sell sent before fee shaping existed.
    #[test]
    fn decorate_off_is_the_canonical_shape() {
        let t = trader(false);
        let out = t.decorate(CuPath::CurveBuy, vec![ix(1), ix(2)], vec![ix(9)], 0);
        assert_eq!(out.len(), 6);
        assert_eq!(cu_pair(&out), (110_000, 200_000));
        assert_eq!(out[0].program_id, solana_sdk::compute_budget::id());
        assert_eq!(out[1].program_id, solana_sdk::compute_budget::id());
        assert_eq!((out[2].data[0], out[3].data[0], out[4].data[0]), (1, 2, 9));
        assert_eq!(out[5].accounts[1].pubkey, t.sender_tip_account);
    }

    /// On: never a higher fee, always a valid Sender tip wallet, the account
    /// creation always ahead of the buy, and many distinct shapes.
    #[test]
    fn decorate_on_varies_the_shape_without_raising_the_fee() {
        let t = trader(true);
        let mut orders = std::collections::HashSet::new();
        let mut tips = std::collections::HashSet::new();
        for _ in 0..3_000 {
            let out = t.decorate(CuPath::CurveBuy, vec![ix(1), ix(2)], vec![ix(9)], 0);
            assert_eq!(out.len(), 6);
            let (l, p) = cu_pair(&out);
            assert!(u128::from(l) * u128::from(p) <= 110_000u128 * 200_000);
            let pos = |tag: u8| out.iter().position(|i| i.data == vec![tag]).unwrap();
            assert!(pos(1) + 1 == pos(2) && pos(2) < pos(9));
            let tip = out.iter().find(|i| i.program_id == solana_sdk::system_program::id()).unwrap();
            assert!(crate::protocol::HELIUS_SENDER_TIP_ACCOUNTS.contains(&tip.accounts[1].pubkey));
            tips.insert(tip.accounts[1].pubkey);
            orders.insert(out.iter().map(|i| (i.program_id, i.data.clone())).map(|(p, d)| {
                if p == solana_sdk::compute_budget::id() { d[0] } else if p == solana_sdk::system_program::id() { 100 } else { d[0] + 10 }
            }).collect::<Vec<u8>>());
        }
        assert_eq!(orders.len(), 60);
        assert_eq!(tips.len(), crate::protocol::HELIUS_SENDER_TIP_ACCOUNTS.len());
    }

    /// The tip is an inline transfer over the System program (never ALT-able).
    #[test]
    fn tip_is_inline_system_transfer() {
        let out = assemble(
            &IxLayout {
                steps: vec![DecoStep::Tip],
            },
            parts(vec![ix(9)], vec![]),
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].program_id, solana_sdk::system_program::id());
        assert_eq!(out[0].accounts.len(), 2);
    }
}
