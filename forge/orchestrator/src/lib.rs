//! `orchestrator` — the forge-only **brain** over the executor write-stack.
//!
//! It owns ONE uniform model of a trade and a batch of trades — the
//! [`plan::Operation`] / [`plan::Plan`] — keyed on orthogonal axes (mechanism ⊥
//! role ⊥ intent ⊥ venue ⊥ amount), and the [`provider`] that validates each op
//! against the venue variant catalog (SSOT) so an illegal tx is unrepresentable.
//!
//! Layering: this crate depends on `executor-core` (the neutral `VenueId` seam)
//! and `executor-pumpfun` (the `pump_trader` catalog + ix builders); it is
//! **forge-only** and LIVE-only — `hunter/live` calls the executor stack directly
//! (lean snipe, no plan/disguise), and neither lab links it.
//!
//! Module map:
//!   - `plan` / `provider` — `Operation`/`Plan` + the venue variant catalog gate.
//!   - `macros` — the plan builders (fund / bundle_launch / volume_make / exit /
//!     consolidate); `personas`/`disguise` — forge-only sticky-persona sampling.
//!   - `audit` — the fingerprint auditor: a mandatory zero-SOL gate that
//!     flags/rejects on-chain linkage + bot tells before execution.
//!
//! The launcher/manage flows build real txs by running a gated `Plan` through an
//! initialized `PumpFunTrader` (`launcher::plan_exec`).

pub mod audit;
pub mod disguise;
pub mod macros;
pub mod personas;
pub mod plan;
pub mod provider;
pub mod rng;

pub use plan::{
    Amount, IdSeq, Intent, OpId, OpKind, Operation, Plan, Role, Schedule, ScheduleSlot, VenueId,
    WalletRef,
};
pub use provider::{prepare, MinOut, PlanError, PreparedOp, PreparedPlan};
pub use macros::{
    bundle_launch, consolidate, exit, fund, volume_make, BundleLaunch, BundlerLeg, ExitLeg,
    FundTarget, Sweep, VolumeLeg,
};
pub use personas::{JitterRange, Persona, PersonaSet};
pub use disguise::{disguise_ops, Disguise};
pub use audit::{
    audit, audit_with, AuditConfig, AuditReport, Finding, Rule, SendProfile, Severity,
    PUMP_FEE_RECIPIENT_INDEX,
};
