//! The **variant catalog** — the single source of truth for every on-chain
//! instruction the pump.fun venue can emit. One `VariantSpec` row per ix, keyed
//! by `(venue, name)`, carrying its discriminator, mechanism, amount
//! denomination, and account-shape flags.
//!
//! Why this exists: variant selection used to live in three inconsistent places —
//! a free-text string match (`"pumpfun.create_v2"`), an enum→string→enum
//! round-trip (`BuyVariant` → persisted string → `BundleBuyVariant::parse`), and
//! hardcoded dispatch with no variant knob at all. The catalog collapses those:
//! [`valid_variants`] returns the legal subset for a venue, so an invalid
//! `(venue, variant)` pair is unrepresentable, and **adding a variant = one row
//! here + one builder arm, nowhere else**.
//!
//! The discriminators are `crate::protocol` constants (also used by the ix
//! builders), so the catalog and the builders cannot drift; `tests` guards it.

use crate::protocol;
use executor_core::venue::VenueId;

/// The tier-1 **mechanism** an instruction performs — venue-neutral and
/// orthogonal to role / intent / venue / amount (the orchestrator `Plan` keys
/// off these same axes). Choosing a variant changes *encoding*, never the kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VariantKind {
    Create,
    Buy,
    Sell,
    TransferSol,
    TransferToken,
}

/// Which pump.fun **stage** an ix targets. Curve vs AMM is a stage *inside* the
/// pump venue (not the venue axis): the plain `buy`/`sell` Anchor discriminators
/// are shared across both programs and disambiguated by `program_id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Curve,
    Amm,
    /// Not a swap (create / transfer).
    NA,
}

/// Which side of a swap the *exact* amount is denominated in; the other side is
/// bounded by the slippage floor. This is the "variant ⊥ amount" axis — a
/// canonical `Amount` is encoded per this denom, never conflated with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Denom {
    /// Spend an exact QUOTE (SOL/WSOL) amount; receive ≥ min base.
    ExactQuoteIn,
    /// Receive an exact BASE (token) amount; spend ≤ max quote.
    ExactBaseOut,
    /// Sell an exact BASE (token) amount; receive ≥ min quote.
    ExactBaseIn,
    /// No swap amount (create).
    None,
    /// Native SOL move.
    Sol,
    /// SPL token move.
    Token,
}

/// One on-chain instruction the executor can emit.
#[derive(Debug, Clone, Copy)]
pub struct VariantSpec {
    /// Stable snake identifier used in persistence / templates / logs.
    pub name: &'static str,
    pub venue: VenueId,
    pub stage: Stage,
    pub kind: VariantKind,
    pub denom: Denom,
    /// 8-byte Anchor discriminator (pump ixs). `None` for SPL/system ixs whose
    /// tag isn't an 8-byte Anchor disc (the builder uses the SPL/system helper).
    pub disc: Option<[u8; 8]>,
    /// AMM buys wrap SOL→WSOL (and unwrap on sell); curve legs never do.
    pub needs_wsol: bool,
    /// Uses the v2 bonding-curve account layout.
    pub v2_accounts: bool,
}

/// The full catalog. One row per on-chain ix. Discriminators reference
/// `crate::protocol` so they cannot drift from the ix builders.
pub const CATALOG: &[VariantSpec] = &[
    // --- pump.fun curve buys (variant ⊥ amount: same Buy kind, different denom/encoding) ---
    VariantSpec {
        name: "buy",
        venue: VenueId::PumpFun,
        stage: Stage::Curve,
        kind: VariantKind::Buy,
        denom: Denom::ExactBaseOut,
        disc: Some(protocol::BUY_DISC),
        needs_wsol: false,
        v2_accounts: false,
    },
    VariantSpec {
        name: "buy_exact_sol_in",
        venue: VenueId::PumpFun,
        stage: Stage::Curve,
        kind: VariantKind::Buy,
        denom: Denom::ExactQuoteIn,
        disc: Some(protocol::BUY_EXACT_SOL_IN_DISC),
        needs_wsol: false,
        v2_accounts: false,
    },
    VariantSpec {
        name: "buy_v2",
        venue: VenueId::PumpFun,
        stage: Stage::Curve,
        kind: VariantKind::Buy,
        denom: Denom::ExactBaseOut,
        disc: Some(protocol::BUY_V2_DISC),
        needs_wsol: false,
        v2_accounts: true,
    },
    VariantSpec {
        name: "buy_exact_quote_in_v2",
        venue: VenueId::PumpFun,
        stage: Stage::Curve,
        kind: VariantKind::Buy,
        denom: Denom::ExactQuoteIn,
        disc: Some(protocol::BUY_EXACT_QUOTE_IN_V2_DISC),
        needs_wsol: false,
        v2_accounts: true,
    },
    // --- pump.fun curve sell ---
    VariantSpec {
        name: "sell",
        venue: VenueId::PumpFun,
        stage: Stage::Curve,
        kind: VariantKind::Sell,
        denom: Denom::ExactBaseIn,
        disc: Some(protocol::SELL_DISC),
        needs_wsol: false,
        v2_accounts: false,
    },
    // --- PumpSwap AMM (migrated pools): same Anchor disc as the curve, WSOL-wrapped ---
    VariantSpec {
        name: "amm_buy",
        venue: VenueId::PumpFun,
        stage: Stage::Amm,
        kind: VariantKind::Buy,
        denom: Denom::ExactBaseOut,
        disc: Some(protocol::BUY_DISC),
        needs_wsol: true,
        v2_accounts: false,
    },
    VariantSpec {
        name: "amm_sell",
        venue: VenueId::PumpFun,
        stage: Stage::Amm,
        kind: VariantKind::Sell,
        denom: Denom::ExactBaseIn,
        disc: Some(protocol::SELL_DISC),
        needs_wsol: true,
        v2_accounts: false,
    },
    // --- token create ---
    VariantSpec {
        name: "create",
        venue: VenueId::PumpFun,
        stage: Stage::NA,
        kind: VariantKind::Create,
        denom: Denom::None,
        disc: Some(protocol::CREATE_DISC),
        needs_wsol: false,
        v2_accounts: false,
    },
    VariantSpec {
        name: "create_v2",
        venue: VenueId::PumpFun,
        stage: Stage::NA,
        kind: VariantKind::Create,
        denom: Denom::None,
        disc: Some(protocol::CREATE_V2_DISC),
        needs_wsol: false,
        v2_accounts: true,
    },
    // --- native SOL + SPL token moves (venue-neutral; no Anchor disc) ---
    VariantSpec {
        name: "transfer_with_seed",
        venue: VenueId::PumpFun,
        stage: Stage::NA,
        kind: VariantKind::TransferSol,
        denom: Denom::Sol,
        disc: None,
        needs_wsol: false,
        v2_accounts: false,
    },
    VariantSpec {
        name: "spl_transfer",
        venue: VenueId::PumpFun,
        stage: Stage::NA,
        kind: VariantKind::TransferToken,
        denom: Denom::Token,
        disc: None,
        needs_wsol: false,
        v2_accounts: false,
    },
    VariantSpec {
        name: "spl_transfer_checked",
        venue: VenueId::PumpFun,
        stage: Stage::NA,
        kind: VariantKind::TransferToken,
        denom: Denom::Token,
        disc: None,
        needs_wsol: false,
        v2_accounts: false,
    },
];

/// Look up a variant by exact name (across all venues).
pub fn spec(name: &str) -> Option<&'static VariantSpec> {
    CATALOG.iter().find(|v| v.name == name)
}

/// The legal variants for `venue` — the subset a provider may draw from. An
/// `(venue, name)` pair not in this subset is rejected before any ix is built.
pub fn valid_variants(venue: VenueId) -> impl Iterator<Item = &'static VariantSpec> {
    CATALOG.iter().filter(move |v| v.venue == venue)
}

/// Whether `name` is a legal variant for `venue`.
pub fn is_valid(venue: VenueId, name: &str) -> bool {
    valid_variants(venue).any(|v| v.name == name)
}

/// The legal variants for `venue` of a given `kind` (e.g. all buy encodings).
pub fn valid_of_kind(
    venue: VenueId,
    kind: VariantKind,
) -> impl Iterator<Item = &'static VariantSpec> {
    valid_variants(venue).filter(move |v| v.kind == kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `valid_variants(PumpFun)` lists only pump variants, and every legal buy
    /// encoding shares the `Buy` kind — proving variant ⊥ amount (encoding varies,
    /// mechanism is fixed).
    #[test]
    fn valid_subset_is_pumpfun_only_and_kind_stable() {
        let all: Vec<_> = valid_variants(VenueId::PumpFun).collect();
        assert_eq!(all.len(), CATALOG.len(), "every catalog row is a pump variant today");
        for v in valid_variants(VenueId::PumpFun) {
            assert_eq!(v.venue, VenueId::PumpFun);
        }
        let buys: Vec<_> = valid_of_kind(VenueId::PumpFun, VariantKind::Buy).collect();
        assert!(buys.len() >= 4, "buy, buy_exact_sol_in, buy_v2, buy_exact_quote_in_v2 (+amm_buy)");
        for b in &buys {
            assert_eq!(b.kind, VariantKind::Buy);
        }
    }

    /// An invalid variant name is unrepresentable — `is_valid` rejects it, so a
    /// provider can never build an off-catalog ix.
    #[test]
    fn unknown_variant_is_rejected() {
        assert!(is_valid(VenueId::PumpFun, "buy_exact_sol_in"));
        assert!(is_valid(VenueId::PumpFun, "create_v2"));
        assert!(!is_valid(VenueId::PumpFun, "buy_exact_quote_in")); // non-v2 quote-in isn't a real ix
        assert!(!is_valid(VenueId::PumpFun, "flash_loan"));
        assert!(spec("nope").is_none());
    }

    /// SSOT guard: the catalog discriminators ARE the `protocol` constants the ix
    /// builders emit, so the two can never drift (mirrors `protocol_constants_ssot`).
    #[test]
    fn discriminators_match_protocol_ssot() {
        assert_eq!(spec("buy").unwrap().disc, Some(protocol::BUY_DISC));
        assert_eq!(
            spec("buy_exact_sol_in").unwrap().disc,
            Some(protocol::BUY_EXACT_SOL_IN_DISC)
        );
        assert_eq!(spec("buy_v2").unwrap().disc, Some(protocol::BUY_V2_DISC));
        assert_eq!(
            spec("buy_exact_quote_in_v2").unwrap().disc,
            Some(protocol::BUY_EXACT_QUOTE_IN_V2_DISC)
        );
        assert_eq!(spec("sell").unwrap().disc, Some(protocol::SELL_DISC));
        assert_eq!(spec("create").unwrap().disc, Some(protocol::CREATE_DISC));
        assert_eq!(spec("create_v2").unwrap().disc, Some(protocol::CREATE_V2_DISC));
        // AMM shares the curve buy/sell Anchor discriminator (program disambiguates).
        assert_eq!(spec("amm_buy").unwrap().disc, spec("buy").unwrap().disc);
        assert_eq!(spec("amm_sell").unwrap().disc, spec("sell").unwrap().disc);
    }

    /// Only AMM legs wrap WSOL; every name is unique.
    #[test]
    fn wsol_and_uniqueness_invariants() {
        for v in CATALOG {
            if v.stage == Stage::Amm {
                assert!(v.needs_wsol, "{} is an AMM leg and must wrap WSOL", v.name);
            } else {
                assert!(!v.needs_wsol, "{} is not an AMM leg", v.name);
            }
        }
        for (i, a) in CATALOG.iter().enumerate() {
            for b in &CATALOG[i + 1..] {
                assert_ne!(a.name, b.name, "duplicate catalog name {}", a.name);
            }
        }
    }
}
