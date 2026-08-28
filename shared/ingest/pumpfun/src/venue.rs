//! The pump.fun ingest venue — the `IngestVenue` impl that plugs the pump
//! classify/decode/pool-derivation into the generic `ingest-core` engine.
//!
//! Owns the shared `PoolIndex` + resubscribe `Notify` so its [`Decoder`] and
//! every feed share exactly one instance: a pool auto-discovered on a
//! `TokenMigrated` event becomes a subscription account with no cross-task
//! hand-off.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use ingest_core::feed::StreamScope;
use ingest_core::proto::geyser::SubscribeUpdateTransaction;
use ingest_core::venue::{DecodeOutput, IngestVenue};
use tokio::sync::Notify;

use crate::config::IngestConfig;
use crate::decode::{Decoder, TxRelevance};
use crate::pool;
use crate::pool::PoolIndex;
use crate::protocol::Protocol;

/// Pump.fun venue: bonding-curve + PumpSwap AMM classify/decode over one shared
/// pool index.
pub struct PumpFunVenue {
    protocol: Arc<Protocol>,
    decoder: Decoder,
    pool_index: PoolIndex,
    pools_changed: Arc<Notify>,
}

impl PumpFunVenue {
    /// Assemble the venue. `track_amm` (from [`IngestConfig`]) decides whether the
    /// decoder attributes post-migration AMM swaps via the shared pool index.
    pub fn new(protocol: Protocol, config: &IngestConfig) -> Self {
        let protocol = Arc::new(protocol);
        let pool_index: PoolIndex = Arc::new(DashMap::new());
        let pools_changed = Arc::new(Notify::new());

        let mut decoder = Decoder::new(protocol.clone()).with_pools_changed(pools_changed.clone());
        if config.track_amm {
            decoder = decoder.with_pool_index(pool_index.clone());
        }

        Self {
            protocol,
            decoder,
            pool_index,
            pools_changed,
        }
    }
}

impl IngestVenue for PumpFunVenue {
    type Relevance = TxRelevance;

    fn filter_key(&self) -> &'static str {
        "pumpfun"
    }

    /// `program` adds the pump.fun program id; `pools` adds every tracked pool
    /// PDA. A feed carrying only pools must omit the program id — another feed
    /// has the curve, and leaving it in would pay the provider for it twice.
    /// With no pools tracked that is legitimately empty, and a server-filtered
    /// feed idles rather than subscribing to the whole chain.
    fn subscription_accounts(&self, scope: StreamScope) -> Vec<String> {
        let mut accounts =
            Vec::with_capacity(usize::from(scope.program) + if scope.pools { self.pool_index.len() } else { 0 });
        if scope.program {
            accounts.push(self.protocol.programs.pump_fun.base58.clone());
        }
        if scope.pools {
            accounts.extend(self.pool_index.iter().map(|e| e.key().clone()));
        }
        accounts
    }

    /// Thin adapter over [`Decoder::classify_accounts`] — the classify logic
    /// lives next to the decode it feeds (and next to the protocol bytes it
    /// compares), so `Curve`/`Create`/`Amm` is decided in exactly one place.
    fn classify(&self, update: &SubscribeUpdateTransaction) -> Option<TxRelevance> {
        self.decoder.classify_accounts(update.transaction.as_ref()?)
    }

    fn is_create_lane(relevance: TxRelevance) -> bool {
        matches!(relevance, TxRelevance::Create)
    }

    fn decode(
        &self,
        update: &SubscribeUpdateTransaction,
        relevance: TxRelevance,
        received_at: DateTime<Utc>,
    ) -> DecodeOutput {
        self.decoder.decode_relevant_pb(update, relevance, received_at)
    }

    fn derive_pool(&self, mint: &str) -> Option<String> {
        pool::derive_pool(mint, &self.protocol)
    }

    fn pool_index(&self) -> PoolIndex {
        self.pool_index.clone()
    }

    fn pools_changed(&self) -> Arc<Notify> {
        self.pools_changed.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn venue() -> PumpFunVenue {
        PumpFunVenue::new(Protocol::pump_fun(), &IngestConfig::default())
    }

    /// The scope is the whole curve/AMM split: one venue answers every feed's
    /// filter question, and only the scope differs.
    #[test]
    fn the_scope_decides_which_accounts_a_feed_watches() {
        let v = venue();
        let pump = v.protocol.programs.pump_fun.base58.clone();
        v.pool_index.insert("pool1".into(), "mint1".into());

        let all = v.subscription_accounts(StreamScope::ALL);
        assert!(all.contains(&pump) && all.contains(&"pool1".to_string()));

        // Another feed has the curve: the program id must not be paid for twice.
        let pools = v.subscription_accounts(StreamScope::POOLS);
        assert_eq!(pools, vec!["pool1".to_string()]);

        let curve = v.subscription_accounts(StreamScope::CURVE);
        assert_eq!(curve, vec![pump]);

        assert!(v.subscription_accounts(StreamScope::NONE).is_empty());
    }

    /// An empty pool set under a pools-only scope is legitimately empty — the
    /// supervisor reads that as "idle", never as "watch everything".
    #[test]
    fn a_pools_only_scope_with_no_pools_is_empty() {
        let v = venue();
        assert!(v.subscription_accounts(StreamScope::POOLS).is_empty());
    }
}
