//! The pump.fun ingest venue — the `IngestVenue` impl that plugs the pump
//! classify/decode/pool-derivation into the generic `ingest-core` transport.
//!
//! Owns the shared `PoolIndex` + resubscribe `Notify` so its [`Decoder`] and the
//! transport task share exactly one instance: a pool auto-discovered on a
//! `TokenMigrated` event becomes a subscription account with no cross-task
//! hand-off. Construction mirrors the pre-split `Ingest::start` wiring
//! (`with_pools_changed` always; `with_pool_index` only when `track_amm`).

use std::sync::Arc;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use tokio::sync::Notify;

use ingest_core::proto::geyser::SubscribeUpdateTransaction;
use ingest_core::venue::{DecodeOutput, IngestVenue, PoolIndex};

use crate::config::IngestConfig;
use crate::decode::{Decoder, TxRelevance};
use crate::pool;
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

    fn subscription_accounts(&self) -> Vec<String> {
        let mut accounts = Vec::with_capacity(self.pool_index.len() + 1);
        accounts.push(self.protocol.programs.pump_fun.base58.clone());
        accounts.extend(self.pool_index.iter().map(|e| e.key().clone()));
        accounts
    }

    /// Thin adapter over [`Decoder::classify_accounts`] — the classify logic
    /// lives next to the decode it feeds (and next to the protocol bytes it
    /// compares), so `Curve`/`Create`/`Amm` is decided in exactly one place.
    fn classify(&self, update: &SubscribeUpdateTransaction) -> Option<TxRelevance> {
        self.decoder.classify_accounts(update.transaction.as_ref()?)
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
