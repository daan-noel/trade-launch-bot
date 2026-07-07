//! Domain D — the own-launch domain (OUR wallets, templates, executed launches).
//!
//! SECURITY: `ManagedWallet.key_ref` is a REFERENCE to an external keystore/KMS,
//! never a raw private key. No secret bytes ever live in a model or the DB.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use uuid::Uuid;

/// One of OUR wallets. `role` ∈ dev | bundler | treasury | trading.
///
/// `status` is the fresh-wallet-pool lifecycle (docs/wallet-pool-plan.md Phase 1):
/// `generated` -> `funded` -> `reserved` -> `used` -> `retired`. `used` and
/// `retired` are terminal — never re-selectable by the atomic claim query.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ManagedWallet {
    pub id: Uuid,
    pub address: String,
    pub label: Option<String>,
    pub role: String,
    /// External keystore / KMS reference — NEVER a raw private key.
    #[serde(skip_serializing)]
    pub key_ref: String,
    pub derivation_index: Option<i32>,
    pub status: String,
    /// Free-text funding audit note (manual funding only — no hop graph yet).
    pub funding_source: Option<String>,
    pub reserved_by_launch_id: Option<Uuid>,
    pub reserved_at: Option<DateTime<Utc>>,
    /// Last observed native SOL balance (lamports) — pool bookkeeping, not a
    /// trade `amount_quote`/`amount_base` (no quote asset applies to a wallet's
    /// own gas balance).
    pub balance_lamports: Option<i64>,
    pub balance_checked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

impl ManagedWallet {
    /// Full-fidelity export INCLUDING `key_ref` — for the wallet-pool Phase 4
    /// backup/restore file ONLY (a local disk write, never an HTTP response).
    /// Every other consumer must go through the normal `Serialize` impl above,
    /// which skips `key_ref`.
    pub fn to_backup_json(&self) -> Json {
        serde_json::json!({
            "id": self.id,
            "address": self.address,
            "label": self.label,
            "role": self.role,
            "key_ref": self.key_ref,
            "derivation_index": self.derivation_index,
            "status": self.status,
            "funding_source": self.funding_source,
            "reserved_by_launch_id": self.reserved_by_launch_id,
            "reserved_at": self.reserved_at,
            "balance_lamports": self.balance_lamports,
            "balance_checked_at": self.balance_checked_at,
            "created_at": self.created_at,
        })
    }
}

#[derive(Debug, Clone)]
pub struct NewManagedWallet {
    pub address: String,
    pub label: Option<String>,
    pub role: String,
    pub key_ref: String,
    pub derivation_index: Option<i32>,
}

/// An authored launch spec (typed + JSONB brain). `variant` selects an audited
/// create builder; `params` carries metadata + dev-buy + the leg_structures pool.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct LaunchTemplate {
    pub id: Uuid,
    pub template_name: String,
    pub launchpad_id: i16,
    pub variant: String,
    pub quote_asset_id: i16,
    pub params: Json,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewLaunchTemplate {
    pub template_name: String,
    pub launchpad_id: i16,
    pub variant: String,
    pub quote_asset_id: i16,
    pub params: Option<Json>,
}

/// An executed launch record. `dev_buy_quote` is quote base units; `bundle_id` is
/// the phase-2 soft ref; `status` is open text (default 'pending').
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Launch {
    pub id: Uuid,
    pub template_id: Option<Uuid>,
    pub mint_address: String,
    pub launchpad_id: i16,
    pub variant: String,
    pub quote_asset_id: i16,
    pub dev_wallet_id: Option<Uuid>,
    pub create_signature: Option<String>,
    pub dev_buy_quote: Option<i64>,
    pub bundle_id: Option<Uuid>,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewLaunch {
    pub template_id: Option<Uuid>,
    pub mint_address: String,
    pub launchpad_id: i16,
    pub variant: String,
    pub quote_asset_id: i16,
    pub dev_wallet_id: Option<Uuid>,
    pub dev_buy_quote: Option<i64>,
    pub status: Option<String>,
}

/// Phase-2 seam — atomic Jito bundle of a launch's buy legs. `legs` is the
/// per-leg structure descriptor pool (audited variant + budget/tip).
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Bundle {
    pub id: Uuid,
    pub launch_id: Uuid,
    pub status: String,
    pub tip_quote: Option<i64>,
    pub legs: Json,
    /// Jito `sendBundle` result id (set at submit time).
    pub jito_bundle_id: Option<String>,
    /// Base58 tx signature per leg, in leg order (set at submit time) — the
    /// confirm watcher checks each against the ingested `trades` feed.
    pub leg_signatures: Vec<String>,
    pub submitted_at: Option<DateTime<Utc>>,
    pub confirmed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}
