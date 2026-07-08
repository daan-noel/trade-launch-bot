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
}
