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
/// `status` is the fresh-wallet-pool lifecycle (docs/plans/wallet/wallet-management.md):
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
    /// Token decimals (from `token_overview`) — divides `holding_base` to a human
    /// token amount. `None` until the create tx is ingested.
    pub token_decimals: Option<i16>,
    /// Quote-asset decimals (from `token_overview`) — lets the frontend render
    /// `holding_value_quote` in human SOL. `None` until the create tx is ingested.
    pub quote_decimals: Option<i16>,
    /// Tokens we still hold across all open positions for this mint, token base
    /// units (`SUM(balance_base)`). `None` when no positions are seeded/open.
    /// As-of the last on-chain reconcile — see [`LaunchRepo::list_page`].
    pub holding_base: Option<i64>,
    /// SOL paid into those open positions' current lots, quote base units
    /// (`SUM(cost_quote)`). Divide by `10^quote_decimals` for human SOL.
    pub holding_cost_quote: Option<i64>,
    /// SOL value of that holding, quote base units (`holding_base *
    /// current_price_quote`). Divide by `10^quote_decimals` for human SOL. `None`
    /// when we hold nothing or the token has no priced trades yet.
    pub holding_value_quote: Option<f64>,
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
/// model (docs/arch/launcher.md). Seeded from the launch/bundle fills,
/// reconciled against the on-chain token balance.
///
/// Amounts are exact base-unit integers (never baked-in floats): `balance_base`
/// is token base units held; `cost_quote` / `realized_quote` are quote base units
/// (the generalization of `_lamports`).
///
/// **PnL basis (the one definition).** Cost and proceeds cover the SAME lot: the
/// wallet's CURRENT lot, which opens on the first buy after its balance reached
/// zero (a closed lot keeps its figures until then). `cost_quote` is the SOL paid
/// into that lot, `realized_quote` the SOL its sells returned, each booked as the
/// wallet's whole-tx SOL flow (`trades.payer_net_lamports`, fees + tip + venue fee
/// included, once per signature) where the feed has it, else the leg's
/// `amount_quote`. So `pnl = realized_quote + value - cost_quote` is exactly what
/// the lot moved plus what it still holds, and `pnl_pct = pnl / cost_quote` —
/// [`TokenPosition::with_pnl`] computes both; no caller re-derives them.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct TokenPosition {
    pub id: Uuid,
    pub mint_address: String,
    /// FK → `managed_wallets.id` (our internal wallet key). Never the wallet's
    /// on-chain identity — that's `wallet_address` below. Never rendered in the UI.
    pub managed_wallet_id: Uuid,
    /// Denormalized `managed_wallets.address` (base58 pubkey) — the ONE canonical,
    /// user-facing wallet identity. This is what the holdings table renders and
    /// what correlates to the `creator_wallet` / `trades` feed (join by address).
    pub wallet_address: String,
    /// Denormalized `managed_wallets.role` (dev | bundler | …) so the holdings
    /// table can group by wallet class without a join.
    pub role: String,
    /// Canonical ATA the buys/sells route through (restart-safe reuse); `None`
    /// until first reconciled.
    pub token_account: Option<String>,
    pub balance_base: i64,
    /// SOL paid into the current lot, quote base units (see the PnL basis above).
    pub cost_quote: i64,
    /// SOL the current lot's sells returned, quote base units.
    pub realized_quote: i64,
    pub balance_checked_at: Option<DateTime<Utc>>,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A holdings row plus its value and PnL at the token's current spot price — the
/// shape the holdings API serves, so the UI renders these figures verbatim.
#[derive(Debug, Clone, Serialize)]
pub struct PositionView {
    #[serde(flatten)]
    pub position: TokenPosition,
    /// `balance_base * current_price_quote`, quote base units. `0` for an empty
    /// balance; `None` when the token has no price yet.
    pub value_quote: Option<f64>,
    /// `realized_quote + value_quote - cost_quote`, quote base units.
    pub pnl_quote: Option<f64>,
    /// `pnl_quote / cost_quote * 100`; `None` when nothing was paid.
    pub pnl_pct: Option<f64>,
}

impl TokenPosition {
    /// Value + PnL of this row at `price_quote` (raw spot ratio, quote base units per
    /// token base unit) — the ONE implementation of the PnL basis documented on
    /// [`TokenPosition`].
    pub fn with_pnl(self, price_quote: Option<f64>) -> PositionView {
        let value_quote = if self.balance_base == 0 {
            Some(0.0)
        } else {
            price_quote.map(|p| self.balance_base as f64 * p)
        };
        let pnl_quote = value_quote.map(|v| self.realized_quote as f64 + v - self.cost_quote as f64);
        let pnl_pct = pnl_quote
            .filter(|_| self.cost_quote > 0)
            .map(|pnl| pnl / self.cost_quote as f64 * 100.0);
        PositionView { position: self, value_quote, pnl_quote, pnl_pct }
    }
}

/// One executed post-launch management action — the audit row for a sell / buy /
/// consolidate (docs/arch/launcher.md). `plan` is the per-wallet legs
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

/// An armed take-profit sell ladder (docs/arch/launcher.md). `rungs` is
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

/// A volume-making bot (docs/arch/launcher.md). The background
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
    /// How many times this bundle has been submitted to Jito. `0` before the first
    /// submit; each submit increments it. Doubles as the tip-escalation *level*
    /// (0 = configured percentile, 1 = p95, 2 = p99, …) so a re-bid after a
    /// `dropped` verdict bids higher than the attempt that just lost the auction.
    #[serde(default)]
    pub submit_attempts: i32,
    /// The serialized `orchestrator::Plan` this bundle was gated + built from
    ///. The executor rebuilds legs from this and recomputes the
    /// deterministic disguises; `legs` above stays for the confirm watcher + UI.
    #[serde(default)]
    pub plan: Option<Json>,
    /// The serialized `orchestrator::AuditReport` captured at the gate — findings
    /// + score + hard_reject, for post-hoc inspection. Never read back to execute.
    #[serde(default)]
    pub audit: Option<Json>,
    /// The create (+ dev-buy) leg's build inputs (name/symbol/uri/flags/variant/
    /// dev-buy), persisted so the confirm watcher's background re-bid can rebuild
    /// the atomic bundle's `tx0` create leg without the original launch request.
    /// `None` for a legacy bundle whose create landed on its own separate tx.
    #[serde(default)]
    pub create_args: Option<Json>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(balance_base: i64, cost_quote: i64, realized_quote: i64) -> TokenPosition {
        TokenPosition {
            id: Uuid::nil(),
            mint_address: "M".into(),
            managed_wallet_id: Uuid::nil(),
            wallet_address: "W".into(),
            role: "dev".into(),
            token_account: None,
            balance_base,
            cost_quote,
            realized_quote,
            balance_checked_at: None,
            status: "open".into(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    /// Paid 100, sold half for 80, the rest is worth 60: PnL 40 on 100 paid.
    #[test]
    fn pnl_is_proceeds_plus_value_minus_paid() {
        let v = pos(500, 100, 80).with_pnl(Some(0.12));
        assert_eq!(v.value_quote, Some(60.0));
        assert_eq!(v.pnl_quote, Some(40.0));
        assert_eq!(v.pnl_pct, Some(40.0));
    }

    /// A closed lot needs no price: its PnL is proceeds minus paid.
    #[test]
    fn closed_lot_prices_without_a_quote() {
        let v = pos(0, 100, 130).with_pnl(None);
        assert_eq!((v.value_quote, v.pnl_quote, v.pnl_pct), (Some(0.0), Some(30.0), Some(30.0)));
    }

    /// An open balance with no price has no value/PnL; nothing paid has no pct.
    #[test]
    fn unpriced_or_unpaid_rows_stay_empty() {
        let v = pos(10, 100, 0).with_pnl(None);
        assert_eq!((v.value_quote, v.pnl_quote, v.pnl_pct), (None, None, None));
        assert_eq!(pos(10, 0, 0).with_pnl(Some(1.0)).pnl_pct, None);
    }
}
