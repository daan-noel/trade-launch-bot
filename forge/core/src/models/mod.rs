//! Domain models — typed rows / DTOs, one module per domain.
//!
//! Amount fields are **exact base-unit integers** (`i64`), not baked-in human
//! floats: the generalization of hunter's `_lamports`/`_sol` rule. The
//! display/USD value depends on the referenced asset's decimals (a `quote_assets`
//! / token dimension), so conversion happens where the decimals are known — the
//! SQL views, or a caller holding the [`QuoteAsset`] — via [`crate::units`], never
//! hard-coded. Ratio fields (prices) stay `f64`.

pub mod dimensions;
pub mod metadata;
pub mod own_launch;
pub mod status;
pub mod token;
pub mod trade;

pub use dimensions::{Launchpad, Market, QuoteAsset};
pub use status::{
    BundleStatus, LadderStatus, LaunchStatus, ManageKind, ManageSizing, ManageStatus,
    PositionStatus, VolumeBotStatus, WalletRole, WalletStatus,
};
pub use metadata::{MetadataTemplate, NewMetadataTemplate};
pub use own_launch::{
    Bundle, Launch, LaunchListRow, LaunchTemplate, ManageAction, ManagedWallet, NewLaunch,
    NewLaunchTemplate, NewManagedWallet, SellLadder, TokenPosition, UpdateLaunchTemplate,
    VolumeBot,
};
pub use token::{NewToken, Token, TokenMarketState, TokenOverview, TokenSyncState};
pub use trade::{NewTrade, RawTx, Trade, TradePriced};
