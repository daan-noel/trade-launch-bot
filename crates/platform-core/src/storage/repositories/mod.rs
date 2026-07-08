//! Repositories — one per table. Read/write boundary between the DB and the
//! typed models. Amount columns are exact base-unit integers; display/USD
//! conversion lives in the SQL views (or a caller holding a [`crate::models::QuoteAsset`]).
//!
//! Grouped by domain into files; each struct is a single table's repo.

pub mod dimensions;
pub mod feed;
pub mod metadata;
pub mod own_launch;
pub mod token;

pub use dimensions::{LaunchpadRepo, MarketRepo, QuoteAssetRepo};
pub use feed::{RawTxRepo, TradeRepo, WalletDictRepo};
pub use metadata::MetadataTemplateRepo;
pub use own_launch::{
    BundleRepo, LaunchRepo, LaunchTemplateRepo, ManageActionRepo, ManagedWalletRepo, SellLadderRepo,
    TokenPositionRepo,
};
pub use token::{TokenMarketStateRepo, TokenRepo, TokenSyncStateRepo};
