//! Whether a wallet can sign the next entry. One verdict per wallet, read by
//! the Wallets page and by every write that points entries at a wallet.

use serde::Serialize;

/// Below this cached balance a ready wallet reports `NeedsSol`. A warning only:
/// SOL comes and goes, so it never blocks an assignment.
pub const LOW_SOL_LAMPORTS: u64 = 10_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Readiness {
    Ready,
    NeedsSol,
    NeedsNonces,
    InitFailed,
    NoKey,
}

/// What the runtime knows about one loaded wallet.
pub struct SlotFacts {
    pub key_matches: bool,
    pub nonce_count: usize,
    pub init_failed: bool,
}

impl Readiness {
    /// The first missing piece, in the order it has to be fixed. Zero nonces
    /// always fails the trader's init too, so it is named before the init error.
    pub fn of(slot: Option<&SlotFacts>, lamports: Option<u64>) -> Self {
        let Some(slot) = slot.filter(|s| s.key_matches) else {
            return Self::NoKey;
        };
        if slot.nonce_count == 0 {
            return Self::NeedsNonces;
        }
        if slot.init_failed {
            return Self::InitFailed;
        }
        match lamports {
            Some(l) if l < LOW_SOL_LAMPORTS => Self::NeedsSol,
            _ => Self::Ready,
        }
    }

    /// The trader can sign. Low SOL still signs; the balance-floor guard decides each buy.
    pub fn can_sign(self) -> bool {
        matches!(self, Self::Ready | Self::NeedsSol)
    }

    pub fn reason(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::NeedsSol => "low SOL",
            Self::NeedsNonces => "wallet has no nonce accounts",
            Self::InitFailed => "wallet trader failed to start",
            Self::NoKey => "wallet key is not loaded on this box",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(key_matches: bool, nonce_count: usize, init_failed: bool) -> SlotFacts {
        SlotFacts { key_matches, nonce_count, init_failed }
    }

    #[test]
    fn the_first_missing_piece_names_the_state() {
        assert_eq!(Readiness::of(None, Some(1)), Readiness::NoKey);
        assert_eq!(Readiness::of(Some(&slot(false, 3, false)), None), Readiness::NoKey);
        assert_eq!(Readiness::of(Some(&slot(true, 0, true)), None), Readiness::NeedsNonces);
        assert_eq!(Readiness::of(Some(&slot(true, 3, true)), None), Readiness::InitFailed);
        assert_eq!(Readiness::of(Some(&slot(true, 3, false)), Some(0)), Readiness::NeedsSol);
        assert_eq!(Readiness::of(Some(&slot(true, 3, false)), Some(LOW_SOL_LAMPORTS)), Readiness::Ready);
        // An unknown balance is not a warning.
        assert_eq!(Readiness::of(Some(&slot(true, 3, false)), None), Readiness::Ready);
    }

    #[test]
    fn low_sol_still_signs_and_nothing_else_does() {
        assert!(Readiness::Ready.can_sign());
        assert!(Readiness::NeedsSol.can_sign());
        assert!(!Readiness::NeedsNonces.can_sign());
        assert!(!Readiness::InitFailed.can_sign());
        assert!(!Readiness::NoKey.can_sign());
    }
}
