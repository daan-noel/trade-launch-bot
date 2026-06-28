//! Crate-owned error type — a `thiserror` boundary, no `anyhow`.
//!
//! Public *and* internal signatures return [`Result`] (`= Result<T, TradeError>`).
//! Callers branch on the semantic variants; the `#[from]` source wrappers let
//! `?` thread third-party errors through unchanged. A local [`Context`]
//! extension trait + [`bail`] macro preserve the ergonomics of the former
//! `anyhow` call sites (mapping ad-hoc messages into [`TradeError::Other`]).

use std::fmt::Display;

/// Crate result alias — every public and internal fallible fn returns this.
pub type Result<T> = std::result::Result<T, TradeError>;

/// Errors a trade path can produce. Semantic variants are matchable by callers;
/// the `#[from]` wrappers carry third-party source errors verbatim.
#[derive(thiserror::Error, Debug)]
pub enum TradeError {
    // --- semantic — callers branch on these ---
    #[error("invalid buy amount: {0}")]
    InvalidBuyAmount(String),
    #[error("slippage exceeded")]
    SlippageExceeded,
    /// A tx that LANDED on-chain and reverted (as opposed to one that never
    /// landed). `custom` is the program's Anchor error code when present. The
    /// retry path branches on this: a landed-revert on a min_out=1 sell is
    /// structural (re-sending only re-pays fees), so it stops early.
    #[error("transaction reverted on-chain (custom: {custom:?})")]
    Reverted { custom: Option<u32> },
    #[error("confirmation timed out")]
    ConfirmTimeout,
    #[error("not initialized")]
    NotInitialized,
    #[error("account/pool not found: {0}")]
    NotFound(String),

    // --- source wrappers — replace anyhow's context chains ---
    // `ClientError` / `nonce_utils::Error` are large (hundreds of bytes); box them
    // (manual `From`, below) so `TradeError` — and every hot-path `Result<_,
    // TradeError>` return slot — stays small. The rest are pointer-sized, so they
    // use `#[from]` directly and `?` converts them automatically.
    #[error(transparent)]
    Rpc(Box<solana_client::client_error::ClientError>),
    #[error("nonce account: {0}")]
    Nonce(Box<solana_client::nonce_utils::Error>),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Pubkey(#[from] solana_sdk::pubkey::ParsePubkeyError),
    #[error(transparent)]
    Signature(#[from] solana_sdk::signature::ParseSignatureError),
    #[error("signing: {0}")]
    Signer(#[from] solana_sdk::signer::SignerError),
    #[error("program: {0}")]
    Program(#[from] solana_sdk::program_error::ProgramError),
    #[error("pubkey derivation: {0}")]
    PubkeyDerive(#[from] solana_sdk::pubkey::PubkeyError),
    #[error("decode: {0}")]
    Bincode(#[from] bincode::Error),
    #[error("decode: {0}")]
    Slice(#[from] std::array::TryFromSliceError),

    /// Last-resort for ad-hoc messages with no semantic variant (the former
    /// `bail!` / `.context()` strings).
    #[error("{0}")]
    Other(String),
}

// Manual `From` for the boxed (large) source errors, so `?` still converts them
// while keeping the enum small.
impl From<solana_client::client_error::ClientError> for TradeError {
    fn from(e: solana_client::client_error::ClientError) -> Self {
        TradeError::Rpc(Box::new(e))
    }
}
impl From<solana_client::nonce_utils::Error> for TradeError {
    fn from(e: solana_client::nonce_utils::Error) -> Self {
        TradeError::Nonce(Box::new(e))
    }
}

/// Local replacement for `anyhow::Context`, so the migrated call sites keep
/// using `.context("…")?` / `.with_context(|| …)?`. Maps any error/`None` into
/// [`TradeError::Other`] with the supplied message prefix.
pub trait Context<T> {
    fn context<C: Display>(self, ctx: C) -> Result<T>;
    fn with_context<C: Display, F: FnOnce() -> C>(self, f: F) -> Result<T>;
}

impl<T, E: Display> Context<T> for std::result::Result<T, E> {
    fn context<C: Display>(self, ctx: C) -> Result<T> {
        self.map_err(|e| TradeError::Other(format!("{ctx}: {e}")))
    }
    fn with_context<C: Display, F: FnOnce() -> C>(self, f: F) -> Result<T> {
        self.map_err(|e| TradeError::Other(format!("{}: {e}", f())))
    }
}

impl<T> Context<T> for Option<T> {
    fn context<C: Display>(self, ctx: C) -> Result<T> {
        self.ok_or_else(|| TradeError::Other(ctx.to_string()))
    }
    fn with_context<C: Display, F: FnOnce() -> C>(self, f: F) -> Result<T> {
        self.ok_or_else(|| TradeError::Other(f().to_string()))
    }
}

/// `return Err(TradeError::Other(format!(...)))` — the migrated `anyhow::bail!`.
macro_rules! bail {
    ($($arg:tt)*) => {
        return ::core::result::Result::Err($crate::error::TradeError::Other(::std::format!($($arg)*)))
    };
}
pub(crate) use bail;
