//! CHECK-constrained vocabularies — the ONE place these column string values are
//! defined in Rust: launch/bundle lifecycle status, plus the managed-wallet role
//! and lifecycle status. Mirrors [`crate::venue::MarketKind`]: each `as_str()`
//! must match the SQL `CHECK` constraint on the corresponding column (launch/
//! bundle status → migration `0006`; wallet `role` → `0002`, wallet `status` →
//! `0004`), and each roundtrip test pins the exact strings.
//!
//! These are stored as `TEXT` (not a PG enum) so a new value is a code + CHECK
//! edit, never an `ALTER TYPE`; the enum + CHECK are the drift guard.

use std::str::FromStr;

/// `launches.status` lifecycle: `pending` (inserted) → `created` (create tx
/// landed) | `failed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchStatus {
    Pending,
    Created,
    Failed,
}

impl LaunchStatus {
    /// The exact DB string (must match the SQL CHECK constraint).
    pub fn as_str(self) -> &'static str {
        match self {
            LaunchStatus::Pending => "pending",
            LaunchStatus::Created => "created",
            LaunchStatus::Failed => "failed",
        }
    }

    /// Every variant — the SSOT list the CHECK-constraint parity test iterates.
    pub const ALL: [LaunchStatus; 3] =
        [LaunchStatus::Pending, LaunchStatus::Created, LaunchStatus::Failed];
}

impl FromStr for LaunchStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(LaunchStatus::Pending),
            "created" => Ok(LaunchStatus::Created),
            "failed" => Ok(LaunchStatus::Failed),
            other => Err(format!("unknown launch status: {other}")),
        }
    }
}

/// `bundles.status` lifecycle: `planned` (inserted) → `submitting` → `submitted`
/// → terminal `landed` | `dropped` | `partial`; `failed` on a submit error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BundleStatus {
    Planned,
    Submitting,
    Submitted,
    Landed,
    Dropped,
    Partial,
    Failed,
}

impl BundleStatus {
    /// The exact DB string (must match the SQL CHECK constraint).
    pub fn as_str(self) -> &'static str {
        match self {
            BundleStatus::Planned => "planned",
            BundleStatus::Submitting => "submitting",
            BundleStatus::Submitted => "submitted",
            BundleStatus::Landed => "landed",
            BundleStatus::Dropped => "dropped",
            BundleStatus::Partial => "partial",
            BundleStatus::Failed => "failed",
        }
    }

    /// Every variant — the SSOT list the CHECK-constraint parity test iterates.
    pub const ALL: [BundleStatus; 7] = [
        BundleStatus::Planned,
        BundleStatus::Submitting,
        BundleStatus::Submitted,
        BundleStatus::Landed,
        BundleStatus::Dropped,
        BundleStatus::Partial,
        BundleStatus::Failed,
    ];
}

impl FromStr for BundleStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "planned" => Ok(BundleStatus::Planned),
            "submitting" => Ok(BundleStatus::Submitting),
            "submitted" => Ok(BundleStatus::Submitted),
            "landed" => Ok(BundleStatus::Landed),
            "dropped" => Ok(BundleStatus::Dropped),
            "partial" => Ok(BundleStatus::Partial),
            "failed" => Ok(BundleStatus::Failed),
            other => Err(format!("unknown bundle status: {other}")),
        }
    }
}

/// `managed_wallets.role` — what a wallet is for. CHECK in migration `0002`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalletRole {
    Dev,
    Bundler,
    Treasury,
    Trading,
}

impl WalletRole {
    /// The exact DB string (must match the SQL CHECK constraint).
    pub fn as_str(self) -> &'static str {
        match self {
            WalletRole::Dev => "dev",
            WalletRole::Bundler => "bundler",
            WalletRole::Treasury => "treasury",
            WalletRole::Trading => "trading",
        }
    }

    /// Every variant — the SSOT list the CHECK-constraint parity test iterates.
    pub const ALL: [WalletRole; 4] = [
        WalletRole::Dev,
        WalletRole::Bundler,
        WalletRole::Treasury,
        WalletRole::Trading,
    ];
}

impl FromStr for WalletRole {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "dev" => Ok(WalletRole::Dev),
            "bundler" => Ok(WalletRole::Bundler),
            "treasury" => Ok(WalletRole::Treasury),
            "trading" => Ok(WalletRole::Trading),
            other => Err(format!("unknown wallet role: {other}")),
        }
    }
}

/// `managed_wallets.status` — fresh-wallet-pool lifecycle: `generated` →
/// `funding` → `funded` → `reserved` → `used`; `retired` is terminal. `funding`
/// = a treasury→wallet SOL send is in flight (the wallet is atomically claimed
/// out of `generated` so a concurrent funder can't double-fund it); the balance
/// poller promotes `funding` → `funded` when the SOL lands. CHECK extended in
/// migration `0008` (originally `0004`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalletStatus {
    Generated,
    Funding,
    Funded,
    Reserved,
    Used,
    Retired,
}

impl WalletStatus {
    /// The exact DB string (must match the SQL CHECK constraint).
    pub fn as_str(self) -> &'static str {
        match self {
            WalletStatus::Generated => "generated",
            WalletStatus::Funding => "funding",
            WalletStatus::Funded => "funded",
            WalletStatus::Reserved => "reserved",
            WalletStatus::Used => "used",
            WalletStatus::Retired => "retired",
        }
    }

    /// Every variant — the SSOT list the CHECK-constraint parity test iterates.
    pub const ALL: [WalletStatus; 6] = [
        WalletStatus::Generated,
        WalletStatus::Funding,
        WalletStatus::Funded,
        WalletStatus::Reserved,
        WalletStatus::Used,
        WalletStatus::Retired,
    ];
}

impl FromStr for WalletStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "generated" => Ok(WalletStatus::Generated),
            "funding" => Ok(WalletStatus::Funding),
            "funded" => Ok(WalletStatus::Funded),
            "reserved" => Ok(WalletStatus::Reserved),
            "used" => Ok(WalletStatus::Used),
            "retired" => Ok(WalletStatus::Retired),
            other => Err(format!("unknown wallet status: {other}")),
        }
    }
}

/// `token_positions.status` — post-launch holdings lifecycle: `open` (we hold, or
/// have partially sold) → `closed` (balance fully drained to 0). CHECK in
/// migration `0010`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionStatus {
    Open,
    Closed,
}

impl PositionStatus {
    /// The exact DB string (must match the SQL CHECK constraint).
    pub fn as_str(self) -> &'static str {
        match self {
            PositionStatus::Open => "open",
            PositionStatus::Closed => "closed",
        }
    }

    /// Every variant — the SSOT list the CHECK-constraint parity test iterates.
    pub const ALL: [PositionStatus; 2] = [PositionStatus::Open, PositionStatus::Closed];
}

impl FromStr for PositionStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "open" => Ok(PositionStatus::Open),
            "closed" => Ok(PositionStatus::Closed),
            other => Err(format!("unknown position status: {other}")),
        }
    }
}

/// `manage_actions.kind` — which post-launch management primitive an action ran.
/// The full set is defined now (Phase 2 wires `Sell`; `Buy`/`Consolidate` land in
/// Phase 3) so the CHECK never needs re-migrating. CHECK in migration `0011`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManageKind {
    Sell,
    Buy,
    Consolidate,
}

impl ManageKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ManageKind::Sell => "sell",
            ManageKind::Buy => "buy",
            ManageKind::Consolidate => "consolidate",
        }
    }

    pub const ALL: [ManageKind; 3] = [ManageKind::Sell, ManageKind::Buy, ManageKind::Consolidate];
}

impl FromStr for ManageKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "sell" => Ok(ManageKind::Sell),
            "buy" => Ok(ManageKind::Buy),
            "consolidate" => Ok(ManageKind::Consolidate),
            other => Err(format!("unknown manage kind: {other}")),
        }
    }
}

/// `manage_actions.sizing` — how a plan's per-wallet leg sizes were computed.
/// Full set defined now (Phase 2 wires `PctOfHoldings`); the rest land with their
/// primitives. CHECK in migration `0011`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManageSizing {
    /// Sell a percentage of each selected wallet's holdings (0–100).
    PctOfHoldings,
    /// Sell across wallets until an approximate SOL proceeds target is met.
    ToSolTarget,
    /// A fixed token base-unit amount per wallet.
    FixedBase,
    /// A fixed SOL (lamports) spend per wallet (buy side).
    FixedSol,
    /// Full-balance sweep (consolidate).
    Sweep,
}

impl ManageSizing {
    pub fn as_str(self) -> &'static str {
        match self {
            ManageSizing::PctOfHoldings => "pct_of_holdings",
            ManageSizing::ToSolTarget => "to_sol_target",
            ManageSizing::FixedBase => "fixed_base",
            ManageSizing::FixedSol => "fixed_sol",
            ManageSizing::Sweep => "sweep",
        }
    }

    pub const ALL: [ManageSizing; 5] = [
        ManageSizing::PctOfHoldings,
        ManageSizing::ToSolTarget,
        ManageSizing::FixedBase,
        ManageSizing::FixedSol,
        ManageSizing::Sweep,
    ];
}

impl FromStr for ManageSizing {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pct_of_holdings" => Ok(ManageSizing::PctOfHoldings),
            "to_sol_target" => Ok(ManageSizing::ToSolTarget),
            "fixed_base" => Ok(ManageSizing::FixedBase),
            "fixed_sol" => Ok(ManageSizing::FixedSol),
            "sweep" => Ok(ManageSizing::Sweep),
            other => Err(format!("unknown manage sizing: {other}")),
        }
    }
}

/// `manage_actions.status` lifecycle: `planned` (previewed/recorded) → `executing`
/// → terminal `completed` (all legs confirmed) | `partial` (some legs failed) |
/// `failed` (setup failed, no leg landed). CHECK in migration `0011`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManageStatus {
    Planned,
    Executing,
    Completed,
    Partial,
    Failed,
}

impl ManageStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ManageStatus::Planned => "planned",
            ManageStatus::Executing => "executing",
            ManageStatus::Completed => "completed",
            ManageStatus::Partial => "partial",
            ManageStatus::Failed => "failed",
        }
    }

    pub const ALL: [ManageStatus; 5] = [
        ManageStatus::Planned,
        ManageStatus::Executing,
        ManageStatus::Completed,
        ManageStatus::Partial,
        ManageStatus::Failed,
    ];
}

impl FromStr for ManageStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "planned" => Ok(ManageStatus::Planned),
            "executing" => Ok(ManageStatus::Executing),
            "completed" => Ok(ManageStatus::Completed),
            "partial" => Ok(ManageStatus::Partial),
            "failed" => Ok(ManageStatus::Failed),
            other => Err(format!("unknown manage status: {other}")),
        }
    }
}

/// `sell_ladders.status` lifecycle: `armed` (evaluating) → `done` (all rungs
/// fired) | `cancelled` (operator). CHECK in migration `0012`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LadderStatus {
    Armed,
    Done,
    Cancelled,
}

impl LadderStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            LadderStatus::Armed => "armed",
            LadderStatus::Done => "done",
            LadderStatus::Cancelled => "cancelled",
        }
    }

    pub const ALL: [LadderStatus; 3] =
        [LadderStatus::Armed, LadderStatus::Done, LadderStatus::Cancelled];
}

impl FromStr for LadderStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "armed" => Ok(LadderStatus::Armed),
            "done" => Ok(LadderStatus::Done),
            "cancelled" => Ok(LadderStatus::Cancelled),
            other => Err(format!("unknown ladder status: {other}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_status_roundtrips() {
        for s in LaunchStatus::ALL {
            assert_eq!(LaunchStatus::from_str(s.as_str()).unwrap(), s);
        }
        assert_eq!(LaunchStatus::Pending.as_str(), "pending");
        assert!(LaunchStatus::from_str("landed").is_err());
    }

    #[test]
    fn bundle_status_roundtrips() {
        for s in BundleStatus::ALL {
            assert_eq!(BundleStatus::from_str(s.as_str()).unwrap(), s);
        }
        assert_eq!(BundleStatus::Planned.as_str(), "planned");
        assert!(BundleStatus::from_str("pending").is_err());
    }

    #[test]
    fn wallet_role_roundtrips() {
        for r in WalletRole::ALL {
            assert_eq!(WalletRole::from_str(r.as_str()).unwrap(), r);
        }
        assert_eq!(WalletRole::Dev.as_str(), "dev");
        assert!(WalletRole::from_str("admin").is_err());
    }

    #[test]
    fn wallet_status_roundtrips() {
        for s in WalletStatus::ALL {
            assert_eq!(WalletStatus::from_str(s.as_str()).unwrap(), s);
        }
        assert_eq!(WalletStatus::Generated.as_str(), "generated");
        assert!(WalletStatus::from_str("pending").is_err());
    }

    #[test]
    fn position_status_roundtrips() {
        for s in PositionStatus::ALL {
            assert_eq!(PositionStatus::from_str(s.as_str()).unwrap(), s);
        }
        assert_eq!(PositionStatus::Open.as_str(), "open");
        assert!(PositionStatus::from_str("done").is_err());
    }

    #[test]
    fn manage_kind_roundtrips() {
        for k in ManageKind::ALL {
            assert_eq!(ManageKind::from_str(k.as_str()).unwrap(), k);
        }
        assert_eq!(ManageKind::Sell.as_str(), "sell");
        assert!(ManageKind::from_str("burn").is_err());
    }

    #[test]
    fn manage_sizing_roundtrips() {
        for s in ManageSizing::ALL {
            assert_eq!(ManageSizing::from_str(s.as_str()).unwrap(), s);
        }
        assert_eq!(ManageSizing::PctOfHoldings.as_str(), "pct_of_holdings");
        assert!(ManageSizing::from_str("half").is_err());
    }

    #[test]
    fn manage_status_roundtrips() {
        for s in ManageStatus::ALL {
            assert_eq!(ManageStatus::from_str(s.as_str()).unwrap(), s);
        }
        assert_eq!(ManageStatus::Completed.as_str(), "completed");
        assert!(ManageStatus::from_str("nope").is_err());
    }

    #[test]
    fn ladder_status_roundtrips() {
        for s in LadderStatus::ALL {
            assert_eq!(LadderStatus::from_str(s.as_str()).unwrap(), s);
        }
        assert_eq!(LadderStatus::Armed.as_str(), "armed");
        assert!(LadderStatus::from_str("firing").is_err());
    }
}
