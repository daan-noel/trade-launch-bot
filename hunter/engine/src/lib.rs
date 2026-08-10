//! # hunter-engine — pure, deterministic strategy engine
//!
//! The one implementation of the fingerprint + metrics generic strategy engine
//! (design SSOT: `hunter/docs/arch/strategies.md`).
//! Live paper, live real, simulate/backtest, and sweep all drive **this** code;
//! they differ only in who produces events and who consumes effects — so
//! identical data yields identical decisions by construction.
//!
//! **Purity is enforced by the crate graph:** no tokio, no sqlx, no rand, no
//! clock (`chrono` is compiled without its `clock` feature — all time arrives
//! on events). The guard test below fails if the manifest regresses.
//!
//! Current contents (grows through the redesign phases):
//! * [`grouping`] — bucket SSOT (`bucket_index`/`same_bucket`/`group_key`),
//!   moved from `trading_core` (which re-exports it during the transition).
//! * [`metrics`] — the self-describing metric registry, condition evaluator,
//!   per-group compute (`snapshot`/`price_lifetime`/`flow_lifetime`/`flow_window`), the per-token
//!   fold [`metrics::track::TokenTrack`], and the sweep-precompute
//!   [`metrics::series::MetricSeries`].
//! * [`rule_params`] — typed, registry-checked `strategy_rules.params`.
//! * [`identity`] + [`dupe_guard`] — the `(name, symbol)` key a copycat re-launch
//!   re-uses, and the rolling memory that refuses to buy the same trap twice.
//! * [`readout`] — the fold's current reading of one rule's conditions, exposed
//!   read-only for a UI. On demand, never per event.

pub mod arm;
pub mod cap;
pub mod deadness;
pub mod dupe_guard;
pub mod event;
pub mod event_log;
pub mod fingerprint;
pub mod grouping;
mod hash;
pub mod identity;
pub mod metrics;
pub mod readout;
pub mod reduce;
pub mod rule_params;
pub mod state;

pub use cap::Cap;
pub use dupe_guard::DupeGuard;
pub use identity::{token_identity_hash, IdentityHash};
pub use event::{Effect, Event};
pub use event_log::LoggedEvent;
pub use reduce::{prime_trade, reduce};
pub use state::EngineState;

/// The decision-loop clock cadence, in milliseconds — the single source of truth
/// for the clock tick (plan decision 5). Sized under one Solana slot (~400 ms) so
/// time/quiet exits react sooner; price TP/SL still fire on Trade events and do
/// not wait for this tick. Both the live decision loop and the lab replay driver
/// derive their interval from this constant, and each asserts equality in a guard
/// test (plan 5.3), so the two paths can never sample metrics at a different cadence.
pub const TICK_MS: i64 = 200;

#[cfg(test)]
mod purity_guard {
    /// The purity promise as a test: the engine's manifest must never gain a
    /// dependency that can reach a runtime, DB, entropy, or the system clock.
    /// (String check on our own Cargo.toml — crude but exactly as strong as
    /// the promise needs.)
    #[test]
    fn manifest_has_no_impure_deps() {
        // Comments may *mention* the banned names (this promise is documented
        // in the manifest header) — only non-comment lines count.
        let manifest: String = include_str!("../Cargo.toml")
            .lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n");
        for banned in ["tokio", "sqlx", "rand", "actix", "reqwest"] {
            assert!(
                !manifest.contains(banned),
                "hunter-engine must stay pure: found banned dependency '{banned}' in Cargo.toml"
            );
        }
        assert!(
            manifest.contains("default-features = false"),
            "chrono must keep default features (incl. 'clock') disabled"
        );
    }
}
