//! `orchestrator` — the forge-only **brain** over the executor write-stack.
//!
//! It owns ONE uniform model of a trade and a batch of trades — the
//! [`plan::Operation`] / [`plan::Plan`] — keyed on orthogonal axes (mechanism ⊥
//! role ⊥ intent ⊥ venue ⊥ amount), and the [`provider`] that validates each op
//! against the venue variant catalog (SSOT) so an illegal tx is unrepresentable.
//! [`dryrun`] renders a zero-SOL preview of a plan.
//!
//! Layering: this crate depends on `executor-core` (the neutral `VenueId` seam)
//! and `executor-pumpfun` (the `pump_trader` catalog + ix builders); it is
//! **forge-only** and LIVE-only — `hunter/live` calls the executor stack directly
//! (lean snipe, no plan/disguise), and neither lab links it.
//!
//! Phase map (executor-redesign-plan.md):
//!   - **C (here):** `Operation`/`Plan` + providers + dry-run.
//!   - **D:** `macros` (fund / bundle_launch / volume_make / exit / consolidate)
//!     + `disguise`/`personas`.
//!   - **E:** `audit` (fingerprint auditor).
//!   - **F:** wire the launcher/manage flows onto `Plan` and build real txs
//!     through an initialized `PumpFunTrader`.

pub mod dryrun;
pub mod plan;
pub mod provider;

pub use plan::{
    Amount, Funding, FundingEdge, Intent, OpId, OpKind, Operation, Plan, Role, Schedule,
    ScheduleSlot, VenueId, WalletRef,
};
pub use provider::{prepare, MinOut, PlanError, PreparedOp, PreparedPlan};
pub use dryrun::{dry_run, DryRunOp, DryRunReport};
