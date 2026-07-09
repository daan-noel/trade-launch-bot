//! Token Swing Analyzer — **moved** to `trading_core` (swing1 Phase 1a).
//!
//! The analyzer is now generic over [`TradeRow`] and lives at
//! `trading_core::strategies::swing_1::swing` so the SAME scan runs in this lab
//! endpoint (`Trade`), the backtest sweep (`CorpusTrade`), and — later — live
//! (`CachedTrade`). This module re-exports it so existing lab paths
//! (`crate::analyzers::swing_analyzer::{detect_swings, SwingLeg, …}`) keep
//! resolving unchanged.

pub use trading_core::strategies::swing_1::swing::{
    compute_chain_stats, detect_swings, ChainStats, SwingLeg, SwingParams, SwingType,
};
