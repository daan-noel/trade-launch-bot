//! `launcher` — create / dev-buy / bundle a token via pump-trader (LIVE box only).
//!
//! Orchestrates: `launch_templates` spec → pump.fun create tx → dev-buy (a
//! pump-trader buy) → optional Jito bundle of N buy legs → confirm → `launches`
//! row. The per-leg structure composer (variant + params + budget/tip) lives here,
//! not in pump-trader.

mod backup;
mod bundle;
mod bundle_execute;
mod confirm;
mod config;
mod dust_sweep;
mod funding_plan;
mod keystore;
mod metadata_upload;
mod probe;
mod service;
mod trader_config;
mod wallet_encrypt;
mod wallet_export;
mod wallet_funding;
mod wallet_pool;
mod wallet_verify;

pub use backup::run_backup;
pub use bundle::{
    compose_bundle_legs, leg_params, legs_from_json, legs_to_json, BundledLegPlan, BuyVariant,
    LegStructure, LegStructureRecipe,
};
pub use bundle_execute::{execute_bundle, BundleExecuteResult};
pub use confirm::spawn_bundle_confirm_watcher;

pub use config::{FundingConfig, LauncherSettings};
pub use dust_sweep::spawn_dust_sweep;
pub use funding_plan::{
    dev_launch_required_lamports, leg_required_lamports, FundPlan, FUNDING_HEADROOM_LAMPORTS,
};
pub use keystore::{
    read_keypair_bytes, write_envelope, write_envelope_to_keystore, EnvKek, Kek,
};
pub use metadata_upload::{
    create_metadata_template, update_metadata_template, NewMetadataTemplateRequest,
    UpdateMetadataTemplateRequest,
};
pub use probe::run_launch_probe;
pub use service::{execute_launch, LaunchRequest, LaunchResult, PumpfunTemplateParams};
pub use wallet_encrypt::run_wallet_encrypt;
pub use wallet_export::{export_wallet_base58, run_wallet_export, ExportedKey};
pub use wallet_funding::{
    fund_for_launch, fund_once, spawn_wallet_funding, DirectJittered, FundMode, FundReport,
    FundScope, FundingStrategy, StrategyParams, Transfer, WalletFundOutcome,
};
pub use wallet_pool::{generate_wallets, spawn_balance_poller, spawn_reservation_sweep};
pub use wallet_verify::run_wallet_verify;
