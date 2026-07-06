//! Domain models — typed rows / DTOs, one module per domain.
//!
//! Amount fields are **exact base-unit integers** (`i64`), not baked-in human
//! floats: the generalization of meme-trading's `_lamports`/`_sol` rule. The
//! display/USD value depends on the referenced asset's decimals (a `quote_assets`
//! / token dimension), so conversion happens where the decimals are known — the
//! SQL views, or a caller holding the [`QuoteAsset`] — via [`crate::units`], never
//! hard-coded. Ratio fields (prices) stay `f64`.

pub mod dimensions;
pub mod token;
pub mod trade;

pub use dimensions::{Launchpad, Market, QuoteAsset};
pub use token::{NewToken, Token, TokenMarketState, TokenOverview, TokenSyncState};
pub use trade::{NewTrade, RawTx, Trade, TradePriced};
