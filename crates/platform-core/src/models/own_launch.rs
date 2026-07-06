//! Domain D — the own-launch domain (OUR wallets, templates, executed launches).
//!
//! SECURITY: `ManagedWallet.key_ref` is a REFERENCE to an external keystore/KMS,
//! never a raw private key. No secret bytes ever live in a model or the DB.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use uuid::Uuid;

/// One of OUR wallets. `role` ∈ dev | bundler | treasury | trading.
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
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
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
    pub created_at: DateTime<Utc>,
}
