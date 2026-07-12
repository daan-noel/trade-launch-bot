//! Dynamic instruction layout — the **shape** half of a launch/bundle tx.
//!
//! A launch tx is a fixed on-chain `Core` instruction (create, or a buy) wrapped
//! by decoration ixs: a compute-unit limit, a compute-unit price, the buyer ATA
//! creation, and the inline Jito tip transfer. WHICH wrappers are present and in
//! what ORDER is the *layout* — orthogonal to the *variant* (which picks the core
//! ix's discriminator/accounts/encoding, owned by the pumpfun catalog).
//!
//! This module is **solana-free plain data** so the forge orchestrator (which
//! already depends on `executor_core` for [`crate::config::ComputeBudgetCfg`]) can
//! name these types when drawing per-persona layouts, without pulling solana. The
//! interpreter that turns a layout into real `Instruction`s lives in the venue
//! crate (`executor-pumpfun`'s `trader::assemble`), which owns the solana deps.
//!
//! Invariant (the hard boundary): a layout only reorders/omits DECORATIONS. It
//! never touches the `Core` ix's account list or discriminator — those are fixed
//! by the on-chain program and reshaping them just reverts. The Jito tip stays an
//! inline `system_instruction::transfer` (Jito's auction scans static keys and
//! does not resolve ALTs); the interpreter enforces that, the layout only places
//! the `Tip` step.

use serde::{Deserialize, Serialize};

/// One position in a launch/bundle transaction. `Core` is the opaque on-chain
/// block (a `[buy]`, a `[create]`, or the fused `[create, ata, buy]`); the rest
/// are decorations placed around it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecoStep {
    /// `ComputeBudgetInstruction::set_compute_unit_limit`.
    CuLimit,
    /// `ComputeBudgetInstruction::set_compute_unit_price` (the priority fee rate).
    CuPrice,
    /// Buyer ATA creation (idempotent) — for a v2 buy this expands to both the
    /// base ATA and the WSOL/quote ATA (the interpreter emits every ix it carries).
    CreateAta,
    /// The opaque on-chain instruction block. Exactly one per layout.
    Core,
    /// Inline `system_instruction::transfer` to the Jito tip account.
    Tip,
}

impl DecoStep {
    /// The stable snake_case slug (matches the serde rename) — SSOT for display
    /// (dry-run preview) and for keying a layout by its step sequence (audit).
    pub fn as_str(self) -> &'static str {
        match self {
            DecoStep::CuLimit => "cu_limit",
            DecoStep::CuPrice => "cu_price",
            DecoStep::CreateAta => "create_ata",
            DecoStep::Core => "core",
            DecoStep::Tip => "tip",
        }
    }
}

/// Which kind of tx a layout describes — governs a couple of validation rules
/// (a create folds its buyer ATA inside `Core`, so `CreateAta` is illegal there).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutKind {
    /// A bundle co-buy or dev-buy leg.
    Buy,
    /// A token create tx (dev-buy, if any, lives *inside* the fused `Core`).
    Create,
}

/// An ordered list of decoration steps around exactly one `Core`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IxLayout {
    pub steps: Vec<DecoStep>,
}

impl IxLayout {
    /// The current hardcoded bundle/dev buy shape:
    /// `[cu_limit, cu_price, create_ata, core, tip]`. Mirrors the pre-refactor
    /// `ixs.push` sequence in the pumpfun bundle-buy builder. For a v2 buy the
    /// single `CreateAta` step expands to base ATA **and** WSOL ATA at assemble
    /// time — the layout carries one step, the interpreter emits both ixs.
    pub fn canonical_buy() -> Self {
        Self {
            steps: vec![
                DecoStep::CuLimit,
                DecoStep::CuPrice,
                DecoStep::CreateAta,
                DecoStep::Core,
                DecoStep::Tip,
            ],
        }
    }

    /// The current hardcoded create shape: `[cu_limit, cu_price, core, tip]`.
    /// `Core` is `[create]` for a create-only tx, or the fused
    /// `[create, ata, buy]` when a dev buy is present — same layout, different
    /// `Core` contents. There is no standalone `CreateAta` step in a create
    /// layout (the buyer ATA is fused into `Core`).
    pub fn canonical_create() -> Self {
        Self {
            steps: vec![
                DecoStep::CuLimit,
                DecoStep::CuPrice,
                DecoStep::Core,
                DecoStep::Tip,
            ],
        }
    }

    /// Landing-safety rails. Rejects any layout that would produce an invalid or
    /// unlandable tx *before* it reaches the interpreter or the send gate:
    ///
    /// 1. exactly one `Core`;
    /// 2. `CreateAta` (if present) precedes `Core`;
    /// 3. a snipe leg must include `CuPrice` and `Tip` (no priority fee / no tip
    ///    ⇒ it will not win the auction);
    /// 4. a `Create` layout must not carry a standalone `CreateAta` (folded into
    ///    `Core`).
    ///
    /// CU steps are otherwise independent — an absent `CuLimit`/`CuPrice` means
    /// the runtime default budget, allowed for non-snipe legs and forbidden for
    /// snipes by rule 3.
    pub fn validate(&self, kind: LayoutKind, is_snipe: bool) -> Result<(), &'static str> {
        let core_count = self.steps.iter().filter(|s| **s == DecoStep::Core).count();
        if core_count != 1 {
            return Err("layout must contain exactly one Core step");
        }
        let core_pos = self
            .steps
            .iter()
            .position(|s| *s == DecoStep::Core)
            .expect("core_count == 1 guarantees a Core");
        if let Some(ata_pos) = self.steps.iter().position(|s| *s == DecoStep::CreateAta) {
            if kind == LayoutKind::Create {
                return Err("create layout must not carry a standalone CreateAta (folded into Core)");
            }
            if ata_pos > core_pos {
                return Err("CreateAta must precede Core");
            }
        }
        if is_snipe {
            if !self.steps.contains(&DecoStep::CuPrice) {
                return Err("a snipe leg must include CuPrice");
            }
            if !self.steps.contains(&DecoStep::Tip) {
                return Err("a snipe leg must include Tip");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_buy_shape_and_validity() {
        let l = IxLayout::canonical_buy();
        assert_eq!(
            l.steps,
            vec![
                DecoStep::CuLimit,
                DecoStep::CuPrice,
                DecoStep::CreateAta,
                DecoStep::Core,
                DecoStep::Tip
            ]
        );
        // A bundle co-buy is a snipe — must pass with CuPrice + Tip present.
        assert!(l.validate(LayoutKind::Buy, true).is_ok());
    }

    #[test]
    fn canonical_create_shape_and_validity() {
        let l = IxLayout::canonical_create();
        assert_eq!(
            l.steps,
            vec![DecoStep::CuLimit, DecoStep::CuPrice, DecoStep::Core, DecoStep::Tip]
        );
        assert!(l.validate(LayoutKind::Create, false).is_ok());
    }

    #[test]
    fn rule_1_rejects_zero_or_two_core() {
        let none = IxLayout { steps: vec![DecoStep::CuLimit, DecoStep::Tip] };
        assert!(none.validate(LayoutKind::Buy, false).is_err());
        let two = IxLayout { steps: vec![DecoStep::Core, DecoStep::Core] };
        assert!(two.validate(LayoutKind::Buy, false).is_err());
    }

    #[test]
    fn rule_2_rejects_create_ata_after_core() {
        let bad = IxLayout { steps: vec![DecoStep::Core, DecoStep::CreateAta] };
        assert_eq!(
            bad.validate(LayoutKind::Buy, false),
            Err("CreateAta must precede Core")
        );
    }

    #[test]
    fn rule_3_snipe_requires_cu_price_and_tip() {
        let no_tip = IxLayout {
            steps: vec![DecoStep::CuPrice, DecoStep::CreateAta, DecoStep::Core],
        };
        assert_eq!(no_tip.validate(LayoutKind::Buy, true), Err("a snipe leg must include Tip"));
        let no_price = IxLayout {
            steps: vec![DecoStep::CreateAta, DecoStep::Core, DecoStep::Tip],
        };
        assert_eq!(
            no_price.validate(LayoutKind::Buy, true),
            Err("a snipe leg must include CuPrice")
        );
        // The same price/tip-less layout is fine for a non-snipe leg.
        assert!(no_tip.validate(LayoutKind::Buy, false).is_ok());
    }

    #[test]
    fn rule_4_create_rejects_standalone_create_ata() {
        let bad = IxLayout {
            steps: vec![DecoStep::CuLimit, DecoStep::CreateAta, DecoStep::Core, DecoStep::Tip],
        };
        assert!(bad.validate(LayoutKind::Create, false).is_err());
    }

    #[test]
    fn serde_round_trips() {
        let l = IxLayout::canonical_buy();
        let json = serde_json::to_string(&l).unwrap();
        // snake_case rename applied.
        assert_eq!(json, r#"{"steps":["cu_limit","cu_price","create_ata","core","tip"]}"#);
        let back: IxLayout = serde_json::from_str(&json).unwrap();
        assert_eq!(back, l);
    }
}
