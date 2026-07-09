//! **Personas** (forge-only) — a *sticky* behavioral template per wallet that
//! drives how its ops are disguised. The whole point is **personas, not
//! independent per-field jitter**: a wallet doesn't pick a fresh random CU, tip,
//! and variant from thin air on every op; it is *assigned* one persona (stable for
//! its lifetime) and every disguise it draws stays inside that persona's ranges. So
//! a wallet reads as one coherent actor across its ops, not statistical noise.
//!
//! **Offline derivation.** The shipped [`PersonaSet`] is meant to come from a
//! `forge/lab` clustering job over real pump traffic (cluster observed
//! CU-limit / cu-price / tip / variant behavior → persona templates →
//! `personas.config`), loaded here via [`PersonaSet::from_config`]. It is **never**
//! computed on a hot path. [`PersonaSet::builtin`] is a small hand-authored starter
//! set so forge/live runs before that job exists.
//!
//! hunter/live never links this crate — it keeps its lean snipe path with no
//! per-tx persona sampling.

use crate::rng::fnv1a;
use serde::{Deserialize, Serialize};

/// An inclusive `[min, max]` sampling range for a jittered integer field. `min ==
/// max` pins the value (no jitter). Serialized as `{ "min": .., "max": .. }` so a
/// lab-derived `personas.config` is human-readable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct JitterRange {
    pub min: u64,
    pub max: u64,
}

impl JitterRange {
    pub const fn new(min: u64, max: u64) -> Self {
        JitterRange { min, max }
    }
    /// A fixed value (no jitter).
    pub const fn fixed(v: u64) -> Self {
        JitterRange { min: v, max: v }
    }
}

/// One behavioral template. A wallet assigned this persona draws every disguise
/// from these ranges/variant pools, so its ops cohere.
///
/// Invariants the disguise sampler relies on (checked by [`Persona::validate`]):
///   - `cu_price.min > 0` — a snipe never drops its priority fee to zero, so a
///     disguise always keeps a landing-competitive fee.
///   - `buy_variants` / `sell_variants` are non-empty and name catalog variants of
///     the right kind (validated against the SSOT at load time).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Persona {
    /// Stable label (e.g. `"aggressive_sniper"`, `"patient_accumulator"`) — appears
    /// in the dry-run/audit so an operator can see which actor a wallet plays.
    pub name: String,
    /// Buy encodings this persona uses (catalog variant names of kind `Buy`). The
    /// disguise sampler intersects these with the encodings the op's `Amount` can
    /// actually take, so a persona can prefer `buy_exact_sol_in` yet still fall back
    /// to a compatible encoding when the amount is denominated the other way.
    pub buy_variants: Vec<String>,
    /// Sell encodings this persona uses (kind `Sell`).
    pub sell_variants: Vec<String>,
    /// CU **headroom** added ABOVE the measured real consumption for the op's path.
    /// Always ≥ 0, so `cu_limit = real_consumption + headroom ≥ real_consumption`
    /// — every disguise is guaranteed to have enough compute to land.
    pub cu_headroom: JitterRange,
    /// Compute-unit price (micro-lamports per CU) range. `min` must be > 0.
    pub cu_price: JitterRange,
    /// Tip range (lamports) for a *tipped* op (a bundled snipe, or a non-snipe buy
    /// that draws a tip). Sells default to no tip regardless (see the sampler).
    pub tip_lamports: JitterRange,
    /// Probability (0..=100) that a **non-snipe** buy carries a tip. A snipe always
    /// tips (latency-critical); a sell never tips by default.
    pub non_snipe_tip_pct: u8,
}

impl Persona {
    /// Structural checks the disguise sampler assumes. Returns the offending field
    /// name on failure so [`PersonaSet::from_config`] can report which template is
    /// malformed. Catalog-name legality is checked separately (needs the venue).
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.name.is_empty() {
            return Err("name");
        }
        if self.cu_price.min == 0 {
            return Err("cu_price.min must be > 0 (a snipe never zero-fees)");
        }
        if self.buy_variants.is_empty() {
            return Err("buy_variants");
        }
        if self.sell_variants.is_empty() {
            return Err("sell_variants");
        }
        if self.non_snipe_tip_pct > 100 {
            return Err("non_snipe_tip_pct");
        }
        Ok(())
    }
}

/// The shipped set of personas + the sticky assignment. In production this is
/// deserialized from the lab-derived `personas.config`; [`builtin`](Self::builtin)
/// is the starter fallback.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonaSet {
    pub personas: Vec<Persona>,
}

impl PersonaSet {
    /// The **sticky** persona for a wallet — a stable hash of its pubkey mapped
    /// into the set, so the same wallet always plays the same actor across every
    /// op and every plan (never re-rolled per op). Empty sets are unrepresentable
    /// (`builtin`/`from_config` guarantee ≥ 1), but guard anyway.
    pub fn assign(&self, wallet_pubkey: &str) -> &Persona {
        debug_assert!(!self.personas.is_empty(), "PersonaSet must be non-empty");
        let idx = (fnv1a(wallet_pubkey.as_bytes()) as usize) % self.personas.len().max(1);
        &self.personas[idx.min(self.personas.len() - 1)]
    }

    /// Load a lab-derived config (JSON), validating every template's structure and
    /// that its variant names are legal catalog entries for the pump venue (so a
    /// stale/typo'd config can never ship a variant the provider would later
    /// reject). Rejects an empty set.
    pub fn from_config(json: &str) -> Result<PersonaSet, String> {
        let set: PersonaSet =
            serde_json::from_str(json).map_err(|e| format!("persona config parse: {e}"))?;
        set.validate()?;
        Ok(set)
    }

    /// Structural + catalog validation shared by `from_config` and the builtin
    /// guard test.
    pub fn validate(&self) -> Result<(), String> {
        use executor_core::venue::VenueId;
        use pump_trader::{valid_of_kind, VariantKind};

        if self.personas.is_empty() {
            return Err("persona set is empty".to_string());
        }
        let buys: Vec<&str> = valid_of_kind(VenueId::PumpFun, VariantKind::Buy)
            .map(|s| s.name)
            .collect();
        let sells: Vec<&str> = valid_of_kind(VenueId::PumpFun, VariantKind::Sell)
            .map(|s| s.name)
            .collect();
        for p in &self.personas {
            p.validate().map_err(|f| format!("persona {:?}: {f}", p.name))?;
            for v in &p.buy_variants {
                if !buys.contains(&v.as_str()) {
                    return Err(format!("persona {:?}: {v:?} is not a catalog Buy variant", p.name));
                }
            }
            for v in &p.sell_variants {
                if !sells.contains(&v.as_str()) {
                    return Err(format!(
                        "persona {:?}: {v:?} is not a catalog Sell variant",
                        p.name
                    ));
                }
            }
        }
        Ok(())
    }

    /// A small hand-authored starter set (3 archetypes) for forge/live before the
    /// lab clustering job ships a real `personas.config`. Values are conservative
    /// placeholders — the CU headroom keeps every disguise landing, the cu_price
    /// floors stay competitive, and the tip ranges differ per archetype so wallets
    /// don't all fee-bid identically (a constant-tip tell the auditor flags).
    pub fn builtin() -> Self {
        PersonaSet {
            personas: vec![
                // Aggressive sniper: SOL-in curve buys, high fees + tips, always
                // races. Suits dev/bundler snipe wallets.
                Persona {
                    name: "aggressive_sniper".to_string(),
                    buy_variants: vec![
                        "buy_exact_sol_in".to_string(),
                        "buy_exact_quote_in_v2".to_string(),
                    ],
                    sell_variants: vec!["sell".to_string()],
                    cu_headroom: JitterRange::new(20_000, 60_000),
                    cu_price: JitterRange::new(250_000, 450_000),
                    tip_lamports: JitterRange::new(1_000_000, 3_000_000),
                    non_snipe_tip_pct: 60,
                },
                // Patient accumulator: tokens-out buys, modest fees, rarely tips
                // outside a snipe. Suits volume/accumulate wallets.
                Persona {
                    name: "patient_accumulator".to_string(),
                    buy_variants: vec!["buy".to_string(), "buy_v2".to_string()],
                    sell_variants: vec!["sell".to_string()],
                    cu_headroom: JitterRange::new(10_000, 40_000),
                    cu_price: JitterRange::new(120_000, 220_000),
                    tip_lamports: JitterRange::new(200_000, 900_000),
                    non_snipe_tip_pct: 20,
                },
                // Balanced churner: mixes both buy encodings, mid fees — the
                // make-volume workhorse.
                Persona {
                    name: "balanced_churner".to_string(),
                    buy_variants: vec![
                        "buy".to_string(),
                        "buy_exact_sol_in".to_string(),
                        "buy_v2".to_string(),
                        "buy_exact_quote_in_v2".to_string(),
                    ],
                    sell_variants: vec!["sell".to_string()],
                    cu_headroom: JitterRange::new(15_000, 50_000),
                    cu_price: JitterRange::new(150_000, 320_000),
                    tip_lamports: JitterRange::new(400_000, 1_500_000),
                    non_snipe_tip_pct: 35,
                },
            ],
        }
    }
}

impl Default for PersonaSet {
    fn default() -> Self {
        PersonaSet::builtin()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_is_valid_against_catalog() {
        PersonaSet::builtin().validate().expect("builtin personas must be catalog-legal");
    }

    #[test]
    fn assignment_is_sticky() {
        let set = PersonaSet::builtin();
        let w = "SoMeWaLLeTpUbKey1111111111111111111111111111";
        let a = set.assign(w).name.clone();
        let b = set.assign(w).name.clone();
        assert_eq!(a, b, "same wallet must always draw the same persona");
    }

    #[test]
    fn from_config_rejects_off_catalog_variant() {
        let bad = r#"{"personas":[{
            "name":"x","buy_variants":["flash_loan"],"sell_variants":["sell"],
            "cu_headroom":{"min":0,"max":0},"cu_price":{"min":1,"max":1},
            "tip_lamports":{"min":0,"max":0},"non_snipe_tip_pct":0}]}"#;
        assert!(PersonaSet::from_config(bad).is_err());
    }

    #[test]
    fn from_config_rejects_zero_cu_price() {
        let bad = r#"{"personas":[{
            "name":"x","buy_variants":["buy"],"sell_variants":["sell"],
            "cu_headroom":{"min":0,"max":0},"cu_price":{"min":0,"max":10},
            "tip_lamports":{"min":0,"max":0},"non_snipe_tip_pct":0}]}"#;
        assert!(PersonaSet::from_config(bad).is_err());
    }

    #[test]
    fn builtin_round_trips_through_config() {
        let json = serde_json::to_string(&PersonaSet::builtin()).unwrap();
        let back = PersonaSet::from_config(&json).expect("builtin config must reload");
        assert_eq!(back, PersonaSet::builtin());
    }
}
