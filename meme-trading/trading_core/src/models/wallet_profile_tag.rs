use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct WalletProfileTag {
    pub id: Uuid,
    pub name: String,
    pub color: String,
    pub comment: Option<String>,
    pub created_at: DateTime<Utc>,
}
