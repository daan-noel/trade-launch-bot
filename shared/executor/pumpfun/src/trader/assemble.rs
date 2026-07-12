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

use executor_core::{DecoStep, IxLayout};
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction, instruction::Instruction, pubkey::Pubkey,
    system_instruction,
};

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
            DecoStep::CuLimit => {
                out.push(ComputeBudgetInstruction::set_compute_unit_limit(parts.cu_limit))
            }
            DecoStep::CuPrice => {
                out.push(ComputeBudgetInstruction::set_compute_unit_price(parts.cu_price))
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use executor_core::LayoutKind;

    fn ix(tag: u8) -> Instruction {
        Instruction { program_id: Pubkey::new_unique(), accounts: vec![], data: vec![tag] }
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
        let lean = IxLayout { steps: vec![DecoStep::CreateAta, DecoStep::Core] };
        let out = assemble(&lean, parts(core.clone(), ata.clone()));
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].data, vec![1]);
        assert_eq!(out[1], core[0]); // Core placed opaque, byte-identical
        // Sanity: this lean layout is a legal non-snipe buy.
        assert!(lean.validate(LayoutKind::Buy, false).is_ok());
    }

    /// The tip is an inline transfer over the System program (never ALT-able).
    #[test]
    fn tip_is_inline_system_transfer() {
        let out = assemble(
            &IxLayout { steps: vec![DecoStep::Tip] },
            parts(vec![ix(9)], vec![]),
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].program_id, solana_sdk::system_program::id());
        assert_eq!(out[0].accounts.len(), 2);
    }
}
