//! Post-launch token management (token-management-plan.md).
//!
//! - `positions` — the holdings read model (Phase 1): seed from launch/bundle,
//!   reconcile balance + realized proceeds.
//! - `model` — request + plan DTOs (`ManageRequest` → `ActionPlan` of `PlanLeg`s).
//! - `plan` — turn a request into a previewable plan (pure DB reads).
//! - `execute` — run a plan's legs via pump-trader + record the audit row.
//!
//! The three primitives are Sell / Buy / Consolidate over a wallet selection.
//! Phase 2 wires **Sell**; Buy/Consolidate/ladders/volume land in later phases.

mod execute;
mod model;
mod plan;
mod positions;

pub use execute::execute_action;
pub use model::{ActionPlan, ManageRequest, PlanLeg, WalletSelection};
pub use plan::build_plan;
pub use positions::load_positions;
