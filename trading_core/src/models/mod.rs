pub mod grouped_sweep;
pub mod ingest;
pub mod paper_run;
pub mod portfolio;
pub mod position;
pub mod raw_tx;
pub mod strategy;
pub mod token;
pub mod token_info;
pub mod swing1_strategy_rule;
pub mod token_sync_state;
pub mod tpsl1_strategy_rule;
pub mod tpsl2_strategy_rule;
pub mod trade;
pub mod wallet;
pub mod wallet_profile;
pub mod wallet_profile_tag;

// Re-export types for convenience
pub use paper_run::{PaperRun, PaperRunStatus};
pub use portfolio::{unrealized_pnl, UnrealizedPnl};
pub use position::{Position, PositionResponse, PositionStatus};
pub use raw_tx::RawTx;
pub use strategy::{
    PositionsSummary, StrategyPosition, StrategyRule, StrategyRun, StrategyRunMetrics,
};
pub use token::Token;
pub use token_sync_state::TokenSyncState;
// Old per-strategy rule models — retained until Phase 2 (strategy registry unify)
// rewires their consumers onto the unified `strategy::*` types above.
pub use swing1_strategy_rule::Swing1Rule;
pub use tpsl1_strategy_rule::Tpsl1Rule;
pub use tpsl2_strategy_rule::Tpsl2Rule;
