//! Shared analyzer DTOs. `ChainStats` is produced by the local swing analyzer
//! (`compute_chain_stats`) and consumed by the core token-list sort
//! (`build_tokens_list`), so the struct lives here in core; the analyzer
//! re-exports it to keep `analyzers::swing_analyzer::ChainStats` paths stable.

/// The three sortable "Chain of Swings" counts for a single token. Faithful
/// (counts-only) port of `swingChains.ts::computeChainStats` — the frontend keeps
/// the richer `longestChain` summary for charting; sorting needs only these.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ChainStats {
    /// Number of high→low swing pairs (complete up-then-down cycles).
    pub swing_pairs: u32,
    /// Pair count of the largest chain (≥ 2 linked pairs); 0 if none link.
    pub max_seq_pairs: u32,
    /// Number of chains (runs of ≥ 2 pairs linked within the latency budget).
    pub chain_count: u32,
}
