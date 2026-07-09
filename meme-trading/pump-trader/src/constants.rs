//! Back-compat constant shims for external consumers (e.g. `live`) that read a
//! few protocol values in their original forms. The canonical homes are now
//! [`crate::protocol`] (addresses / unit conversions / layouts) and
//! [`crate::config`] (operational tuning). Kept intentionally minimal — new
//! internal code should reference `protocol` / `config` directly.

pub use crate::protocol::{LAMPORTS_PER_SOL, TOKEN_PROGRAM_ID};

/// Wrapped-SOL mint — base58 string form (the canonical [`crate::protocol::WSOL_MINT`]
/// is a `const Pubkey`; this `&str` form is for callers that compare mint strings).
pub const WSOL_MINT: &str = "So11111111111111111111111111111111111111112";
