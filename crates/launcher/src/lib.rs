//! `launcher` — create / dev-buy / bundle a token via pump-trader (LIVE box only).
//!
//! Orchestrates: `launch_templates` spec → pump.fun create tx → dev-buy (a
//! pump-trader buy) → optional Jito bundle of N buy legs → confirm → `launches`
//! row. The per-leg structure composer (variant + params + budget/tip) lives here,
//! not in pump-trader.

mod bundle;
mod bundle_execute;
mod confirm;
mod config;
mod keystore;
mod probe;
mod service;
mod trader_config;
mod wallet_encrypt;
mod wallet_pool;

pub use bundle::{
    compose_bundle_legs, leg_params, legs_from_json, legs_to_json, BuyVariant, BundledLegPlan,
    LegStructure, LegStructureRecipe, StoredBundleLeg,
};
pub use bundle_execute::{execute_bundle, BundleExecuteResult};
pub use confirm::spawn_bundle_confirm_watcher;

pub use config::LauncherSettings;
pub use keystore::{
    read_keypair_bytes, write_envelope, write_envelope_to_keystore, EnvKek, Kek,
};
pub use probe::run_launch_probe;
pub use service::{execute_launch, LaunchRequest, LaunchResult, PumpfunTemplateParams};
pub use wallet_encrypt::run_wallet_encrypt;
pub use wallet_pool::{generate_wallets, spawn_balance_poller, spawn_reservation_sweep};
