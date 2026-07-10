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
/// create builder; `params` carries dev-buy + the leg_structures pool. Token
/// identity (name/symbol/uri) lives in the referenced `metadata_templates` row —
/// [`metadata_template_id`](Self::metadata_template_id) — not inlined in `params`,
/// so metadata has a single source of truth (see migration `0007`).
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct LaunchTemplate {
    pub id: Uuid,
    pub template_name: String,
    pub launchpad_id: i16,
    pub variant: String,
    pub quote_asset_id: i16,
    /// The token metadata (name/symbol/uri) this template launches with. `None`
    /// only for a legacy row whose metadata couldn't be backfilled — such a
    /// template can't launch until it's linked.
    pub metadata_template_id: Option<Uuid>,
    pub params: Json,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewLaunchTemplate {
    pub template_name: String,
    pub launchpad_id: i16,
    pub variant: String,
    pub quote_asset_id: i16,
    pub metadata_template_id: Option<Uuid>,
    pub params: Option<Json>,
}

/// Full-replace update — identical shape to [`NewLaunchTemplate`] (no partial
/// PATCH; matches this codebase's fixed-shape update pattern). A type alias so
/// the create/update bodies can't drift apart.
pub type UpdateLaunchTemplate = NewLaunchTemplate;

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

/// Enriched launch row for the "launched tokens" list — a [`Launch`] LEFT JOINed
/// to token identity + the `token_overview` derived view + the launch's bundle
/// status, so the frontend renders the list without N per-row follow-up fetches.
/// Read-only projection (no insert form); the derived display/USD fields come
/// straight from `token_overview` (the SSOT for decimals + USD), never recomputed.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct LaunchListRow {
    pub id: Uuid,
    pub template_id: Option<Uuid>,
    pub mint_address: String,
    pub variant: String,
    pub status: String,
    pub create_signature: Option<String>,
    pub dev_buy_quote: Option<i64>,
    pub bundle_id: Option<Uuid>,
    pub bundle_status: Option<String>,
    pub created_at: DateTime<Utc>,
    /// Token identity — `None` until the create tx is ingested into `tokens`.
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub trade_count: Option<i64>,
    pub is_migrated: Option<bool>,
    pub is_dead: Option<bool>,
    pub price_usd: Option<f64>,
    pub market_cap_usd: Option<f64>,
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

/// One wallet's holding of one launched token — the post-launch management read
/// model (token-management-plan.md Phase 1). Seeded from the launch/bundle fills,
/// reconciled against the on-chain token balance.
///
/// Amounts are exact base-unit integers (never baked-in floats): `balance_base`
/// is token base units held; `cost_quote` / `realized_quote` are quote base units
/// (the generalization of `_lamports`). PnL/USD is derived by the caller from the
/// token's price + decimals, never stored.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct TokenPosition {
    pub id: Uuid,
    pub mint_address: String,
    pub wallet_id: Uuid,
    /// Denormalized `managed_wallets.role` (dev | bundler | …) so the holdings
    /// table can group by wallet class without a join.
    pub role: String,
    /// Canonical ATA the buys/sells route through (restart-safe reuse); `None`
    /// until first reconciled.
    pub token_account: Option<String>,
    pub balance_base: i64,
    pub cost_quote: i64,
    pub realized_quote: i64,
    pub balance_checked_at: Option<DateTime<Utc>>,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// One executed post-launch management action — the audit row for a sell / buy /
/// consolidate (token-management-plan.md Phase 2). `plan` is the per-wallet legs
/// that actually ran (with their confirmed signatures); `selection` is what the
/// operator picked. Never a FK to `tokens` — an action can target a launch before
/// its create tx is ingested.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ManageAction {
    pub id: Uuid,
    pub mint_address: String,
    pub kind: String,
    pub sizing: String,
    pub selection: Json,
    pub plan: Json,
    pub status: String,
    pub legs_total: i32,
    pub legs_confirmed: i32,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

/// An armed take-profit sell ladder (token-management-plan.md Phase 4). `rungs` is
/// a JSONB array of `{ metric, threshold, pct, fired }`; the background evaluator
/// flips a rung's `fired` when the token crosses its milestone and fires a sell of
/// `pct` across `selection`. Never a FK to `tokens`.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SellLadder {
    pub id: Uuid,
    pub mint_address: String,
    pub selection: Json,
    pub rungs: Json,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A volume-making bot (token-management-plan.md Phase 5). The background
/// scheduler cycles a jittered buy (+ optional sell-back) across `selection`'s
/// wallets on a jittered interval, generating trade volume until its SOL budget or
/// max-cycle cap is hit. `config` is a `VolumeConfig` JSONB (size/interval ranges,
/// sell-back pct, budget); the row is the bot's SSOT. Never a FK to `tokens`.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct VolumeBot {
    pub id: Uuid,
    pub mint_address: String,
    pub status: String,
    pub selection: Json,
    pub config: Json,
    pub cycles_done: i32,
    /// Cumulative SOL (lamports) spent on buys — checked against the budget.
    pub spent_quote: i64,
    /// Cumulative notional (lamports) traded (buys + sell-backs) — the volume stat.
    pub volume_quote: i64,
    pub next_run_at: DateTime<Utc>,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
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
    /// The serialized `orchestrator::Plan` this bundle was gated + built from
    /// (Phase 2.F). The executor rebuilds legs from this and recomputes the
    /// deterministic disguises; `legs` above stays for the confirm watcher + UI.
    #[serde(default)]
    pub plan: Option<Json>,
    /// The serialized `orchestrator::AuditReport` captured at the gate — findings
    /// + score + hard_reject, for post-hoc inspection. Never read back to execute.
    #[serde(default)]
    pub audit: Option<Json>,
}
