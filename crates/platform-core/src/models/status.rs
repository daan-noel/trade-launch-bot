//! Launch/bundle lifecycle status enums — the ONE place these string values are
//! defined in Rust. Mirrors [`crate::venue::MarketKind`]: each `as_str()` must
//! match the SQL `CHECK` constraint on the corresponding column (see migration
//! `0006_status_check.sql`), and the roundtrip test pins the exact strings.
//!
//! These are stored as `TEXT` (not a PG enum) so a new state is a code + CHECK
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
}
