//! `Fingerprint` — a token-creation shape shared by many strategy rules. Backs
//! the `fingerprints` table (0004 redesign schema).
//!
//! Matching semantics (implemented in `strategies::fingerprint`, Phase 2):
//! * **Exact** fields: `cu_limit`, `cu_price`, `ix_labels` (exact ordered
//!   sequence).
//! * **Bucket-matched** fields (via this row's own `bucket_size_amount`, SSOT
//!   [`crate::grouping::same_bucket`]): the five lamports axes.
//! * A `None` field is **not part of the fingerprint's identity** (ignored by
//!   the matcher). A token can match multiple fingerprints.
//!
//! Unit convention: lamports (`i64`) at rest, human SOL (`f64`) via the `*_sol`
//! accessors — conversion through the one shared `config::constants` pair.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::constants::lamports_to_sol;

/// A `fingerprints` row. See module docs for matching semantics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fingerprint {
    pub id: Uuid,
    /// Human-facing label.
    pub name: String,
    /// Exact-match compute-unit limit of the creation tx.
    pub cu_limit: Option<i64>,
    /// Exact-match compute-unit price of the creation tx.
    pub cu_price: Option<i64>,
    /// Creator's initial dev-buy size (bucket-matched).
    pub init_buy_lamports: Option<i64>,
    /// `max_sol_cost` arg of the initial buy instruction (bucket-matched).
    pub max_cost_lamports: Option<i64>,
    /// Spendable SOL the creator wallet held going in (bucket-matched).
    pub spendable_lamports_in: Option<i64>,
    /// Sum of buy SOL in the creation slot (bucket-matched; only settled after
    /// the creation slot closes — deferred first-slot gate).
    pub first_slot_buy_lamports: Option<i64>,
    /// Sum of sell SOL in the creation slot (bucket-matched; deferred as above).
    pub first_slot_sell_lamports: Option<i64>,
    /// SOL width of the bucket every bucket-matched axis uses.
    pub bucket_size_amount: f64,
    /// Exact ordered instruction-label sequence of the creation tx.
    pub ix_labels: Option<Vec<String>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Fingerprint {
    pub fn init_buy_sol(&self) -> Option<f64> {
        self.init_buy_lamports.map(lamports_to_sol)
    }

    pub fn max_cost_sol(&self) -> Option<f64> {
        self.max_cost_lamports.map(lamports_to_sol)
    }

    pub fn spendable_sol_in(&self) -> Option<f64> {
        self.spendable_lamports_in.map(lamports_to_sol)
    }

    pub fn first_slot_buy_sol(&self) -> Option<f64> {
        self.first_slot_buy_lamports.map(lamports_to_sol)
    }

    pub fn first_slot_sell_sol(&self) -> Option<f64> {
        self.first_slot_sell_lamports.map(lamports_to_sol)
    }

    /// Whether any matchable criterion is configured. The matcher requires at
    /// least one so an all-`None` fingerprint can never match everything.
    pub fn has_any_criterion(&self) -> bool {
        self.cu_limit.is_some()
            || self.cu_price.is_some()
            || self.init_buy_lamports.is_some()
            || self.max_cost_lamports.is_some()
            || self.spendable_lamports_in.is_some()
            || self.first_slot_buy_lamports.is_some()
            || self.first_slot_sell_lamports.is_some()
            || self.ix_labels.is_some()
    }
}
