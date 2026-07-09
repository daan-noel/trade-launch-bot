use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::wallet::Wallet;
use super::wallet_profile_tag::WalletProfileTag;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileType {
    Mine,
    Trader,
    Whale,
    Dev,
}

impl ProfileType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Mine => "mine",
            Self::Trader => "trader",
            Self::Whale => "whale",
            Self::Dev => "dev",
        }
    }
}

impl TryFrom<&str> for ProfileType {
    type Error = String;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
            "mine" => Ok(Self::Mine),
            "trader" => Ok(Self::Trader),
            "whale" => Ok(Self::Whale),
            "dev" => Ok(Self::Dev),
            other => Err(format!("unknown profile type: {other}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletProfile {
    pub id: Uuid,
    pub name: String,
    pub profile_type: ProfileType,
    pub tag_ids: Vec<Uuid>,
    pub created_at: DateTime<Utc>,
}

/// Profile with its wallets and resolved tags (used for API responses).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletProfileWithWallets {
    pub id: Uuid,
    pub name: String,
    pub profile_type: ProfileType,
    pub created_at: DateTime<Utc>,
    pub wallets: Vec<Wallet>,
    pub tags: Vec<WalletProfileTag>,
}
