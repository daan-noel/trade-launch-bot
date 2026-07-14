//! `launcher` — create / dev-buy / bundle a token via pump-trader (LIVE box only).
//!
//! Orchestrates: `launch_templates` spec → pump.fun create tx → dev-buy (a
//! pump-trader buy) → optional Jito bundle of N buy legs → confirm → `launches`
//! row. The per-leg structure composer (variant + params + budget/tip) lives here,
//! not in pump-trader.

mod alt;
mod backup;
mod bundle;
mod bundle_execute;
mod bundle_simulate;
mod confirm;
mod config;
mod dust_sweep;
mod events;
mod funding_plan;
mod keystore;
mod launch_sim_matrix;
mod manage;
mod metadata_upload;
mod plan_exec;
mod plan_pipeline;
mod probe;
mod service;
mod trader_config;
mod wallet_encrypt;
mod wallet_export;
mod wallet_funding;
mod wallet_lifecycle;
mod wallet_pool;
mod wallet_sweep;
mod wallet_transfer;
mod wallet_verify;

pub use alt::run_create_alt;
pub use backup::run_backup;
pub use bundle::{
    legs_from_json, legs_to_json, BundledLegPlan, BuyVariant, LegStructure, LegStructureRecipe,
};
pub use bundle_execute::{execute_bundle, BundleExecuteResult};
pub use bundle_simulate::run_bundle_simulate;
pub use confirm::spawn_bundle_confirm_watcher;

pub use config::{FundingConfig, LauncherSettings, ManageConfig};
pub use events::{EventSink, LaunchStatusEvent};
pub use funding_plan::{
    dev_launch_required_lamports, leg_required_lamports, FundPlan, FUNDING_HEADROOM_LAMPORTS,
};
pub use keystore::{
    read_keypair_bytes, write_envelope, write_envelope_to_keystore, EnvKek, Kek,
};
pub use launch_sim_matrix::run_launch_sim_matrix;
pub use manage::{
    arm_ladder, build_plan, execute_action, load_positions, read_positions, reconcile_positions,
    spawn_ladder_evaluator, spawn_volume_scheduler, start_volume_bot, ActionPlan, LadderRung,
    ManageRequest, PlanLeg, VolumeConfig, WalletSelection,
};
pub use metadata_upload::{
    create_metadata_template, update_metadata_template, NewMetadataTemplateRequest,
    UpdateMetadataTemplateRequest,
};
pub use probe::run_launch_probe;
pub use service::{
    execute_launch, InsufficientDevBalance, LaunchRequest, LaunchResult, PumpfunTemplateParams,
};
pub use wallet_encrypt::run_wallet_encrypt;
pub use wallet_export::{export_wallet_base58, run_wallet_export, ExportedKey};
pub use wallet_funding::{
    fund_for_launch, fund_once, DirectJittered, FundMode, FundReport, FundScope, FundingStrategy,
    StrategyParams, Transfer, WalletFundOutcome,
};
pub use wallet_lifecycle::spawn_wallet_lifecycle;
pub use wallet_pool::{generate_wallets, refresh_all_balances};
pub use wallet_sweep::{
    consolidate_all, sweep_used_and_retired, SweepReport, WalletSweepOutcome,
};
pub use wallet_transfer::{transfer_between_wallets, TransferAmount, TransferReport};
pub use wallet_verify::run_wallet_verify;
