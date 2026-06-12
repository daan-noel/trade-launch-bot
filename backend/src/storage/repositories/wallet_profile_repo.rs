use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::{
    wallet::Wallet,
    wallet_profile::{ProfileType, WalletProfile, WalletProfileWithWallets},
};

use super::wallet_profile_tag_repo::WalletProfileTagRepo;
use super::wallet_repo::WalletRepo;

pub struct WalletProfileRepo {
    pool: PgPool,
}

#[derive(sqlx::FromRow)]
struct ProfileRow {
    id: Uuid,
    name: String,
    #[sqlx(rename = "type")]
    profile_type: String,
    tag_ids: Vec<Uuid>,
    created_at: DateTime<Utc>,
}

impl TryFrom<ProfileRow> for WalletProfile {
    type Error = anyhow::Error;

    fn try_from(r: ProfileRow) -> Result<Self, Self::Error> {
        let profile_type = ProfileType::try_from(r.profile_type.as_str())
            .map_err(|e| anyhow::anyhow!(e))?;
        Ok(Self {
            id: r.id,
            name: r.name,
            profile_type,
            tag_ids: r.tag_ids,
            created_at: r.created_at,
        })
    }
}

impl WalletProfileRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn insert(&self, name: &str, profile_type: ProfileType) -> anyhow::Result<WalletProfile> {
        let id = Uuid::new_v4();
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO wallet_profiles (id, name, type, tag_ids, created_at) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(id)
        .bind(name)
        .bind(profile_type.as_str())
        .bind::<Vec<Uuid>>(vec![])
        .bind(now)
        .execute(&self.pool)
        .await?;

        Ok(WalletProfile {
            id,
            name: name.to_string(),
            profile_type,
            tag_ids: vec![],
            created_at: now,
        })
    }

    pub async fn update(&self, id: Uuid, name: &str, profile_type: ProfileType) -> anyhow::Result<()> {
        sqlx::query("UPDATE wallet_profiles SET name=$1, type=$2 WHERE id=$3")
            .bind(name)
            .bind(profile_type.as_str())
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_tag_ids(&self, id: Uuid, tag_ids: &[Uuid]) -> anyhow::Result<()> {
        sqlx::query("UPDATE wallet_profiles SET tag_ids=$1 WHERE id=$2")
            .bind(tag_ids)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete(&self, id: Uuid) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM wallet_profiles WHERE id=$1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn find_by_id(&self, id: Uuid) -> anyhow::Result<Option<WalletProfile>> {
        let row = sqlx::query_as::<_, ProfileRow>(
            "SELECT id, name, type, tag_ids, created_at FROM wallet_profiles WHERE id=$1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(WalletProfile::try_from).transpose()
    }

    pub async fn list(&self) -> anyhow::Result<Vec<WalletProfile>> {
        let rows = sqlx::query_as::<_, ProfileRow>(
            "SELECT id, name, type, tag_ids, created_at FROM wallet_profiles ORDER BY created_at ASC",
        )
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(WalletProfile::try_from).collect()
    }

    pub async fn find_with_wallets(&self, id: Uuid) -> anyhow::Result<Option<WalletProfileWithWallets>> {
        let Some(profile) = self.find_by_id(id).await? else {
            return Ok(None);
        };
        let wallets = WalletRepo::new(self.pool.clone()).list_by_profile(id).await?;
        let tags = WalletProfileTagRepo::new(self.pool.clone())
            .find_by_ids(&profile.tag_ids)
            .await?;
        Ok(Some(WalletProfileWithWallets {
            id: profile.id,
            name: profile.name,
            profile_type: profile.profile_type,
            created_at: profile.created_at,
            wallets,
            tags,
        }))
    }

    pub async fn list_with_wallets(&self) -> anyhow::Result<Vec<WalletProfileWithWallets>> {
        let profiles = self.list().await?;
        let wallet_repo = WalletRepo::new(self.pool.clone());
        let tag_repo = WalletProfileTagRepo::new(self.pool.clone());

        // Collect all unique tag_ids across all profiles for a single DB round-trip.
        let all_tag_ids: Vec<Uuid> = {
            let mut ids: Vec<Uuid> = profiles.iter().flat_map(|p| p.tag_ids.iter().copied()).collect();
            ids.sort_unstable();
            ids.dedup();
            ids
        };
        let all_tags = tag_repo.find_by_ids(&all_tag_ids).await?;

        // All wallets for all profiles in one query, grouped in memory — instead
        // of one list_by_profile per profile (the previous N+1).
        let profile_ids: Vec<Uuid> = profiles.iter().map(|p| p.id).collect();
        let mut wallets_by_profile: std::collections::HashMap<Uuid, Vec<Wallet>> =
            std::collections::HashMap::new();
        for wallet in wallet_repo.list_by_profiles(&profile_ids).await? {
            wallets_by_profile
                .entry(wallet.profile_id)
                .or_default()
                .push(wallet);
        }

        let mut result = Vec::with_capacity(profiles.len());
        for p in profiles {
            let wallets: Vec<Wallet> = wallets_by_profile.remove(&p.id).unwrap_or_default();
            let tags = all_tags
                .iter()
                .filter(|t| p.tag_ids.contains(&t.id))
                .cloned()
                .collect();
            result.push(WalletProfileWithWallets {
                id: p.id,
                name: p.name,
                profile_type: p.profile_type,
                created_at: p.created_at,
                wallets,
                tags,
            });
        }
        Ok(result)
    }
}
