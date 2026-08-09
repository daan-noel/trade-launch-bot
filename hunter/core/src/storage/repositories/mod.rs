pub mod amm_pool_facts_repo;
pub mod creation_stats_repo;
pub mod fingerprint_repo;
// `grouped_sweep_repo` stays in `backend`: it depends on the sweep engine's
// `ComboMetrics` aggregate (local), so it lands in `lab`, not core.
pub mod raw_tx_repo;
pub mod rule_repo;
pub mod settings_repo;
pub mod strategy_repo;
pub mod token_info_repo;
pub mod token_repo;
pub mod trade_repo;
pub mod wallet_dict_repo;
// There are no per-strategy repos: every strategy reads and writes through the one
// `strategy_repo` over `strategy_rules`/`strategy_runs`/`strategy_positions`.
pub mod wallet_profile_repo;
pub mod wallet_profile_tag_repo;
pub mod wallet_repo;
