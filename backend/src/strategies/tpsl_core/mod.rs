//! Shared engine for the TPSL sniper strategies (Phase 5.1 unification).
//!
//! `tpsl_sniper_1` and `tpsl_sniper_2` are near-byte-identical clones differing
//! only by naming, the (now unified) `TpslRule` model, and tpsl2's scalp logic.
//! This module is being grown to hold the single shared engine; the strategies
//! become thin instantiations over a `TpslVariant`. See `TPSL_UNIFICATION_PLAN.md`.
//!
//! Built incrementally so every step compiles and is behavior-preserving:
//! - step 2 (here): `repos` — the `PositionRepo` / `PaperRepo` / `RuleRepo`
//!   traits + impls for the six concrete repos (additive; not yet consumed).

pub mod repos;
