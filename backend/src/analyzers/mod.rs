#![allow(dead_code)]

pub mod analyzer_service;
pub mod creator_analyzer;
pub mod volume_analyzer;

pub use creator_analyzer::CreatorAnalyzer;
pub use volume_analyzer::VolumeAnalyzer;

/// Minimum number of trades in the sliding window before volume analysis runs.
const MIN_TRADES_FOR_VOLUME_ANALYSIS: u64 = 5;

/// How often (in trade count) the volume analyzer re-runs for a token.
/// E.g. at trade 10, 20, 30, etc.
const VOLUME_ANALYSIS_INTERVAL: u64 = 10;
