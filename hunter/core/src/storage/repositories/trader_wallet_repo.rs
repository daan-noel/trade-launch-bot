//! Live trading-wallet registry. Filenames stay on the record type; the list
//! the API returns does not include them.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::trader_wallet::{
    OpenWalletPosition, TraderNonce, TraderWallet, TraderWalletRecord, WalletActivity, WalletMove,
    WalletRule,
};
use crate::storage::repositories::strategy_repo::PNL_LAMPORTS_SQL;

#[derive(Clone)]
pub struct TraderWalletRepo {
    pool: PgPool,
}

#[derive(sqlx::FromRow)]
struct WalletRow {
    pubkey: String,
    name: String,
    status: String,
    is_primary: bool,
    created_at: DateTime<Utc>,
    archived_at: Option<DateTime<Utc>>,
    nonce_count: i64,
    rule_count: i64,
    open_positions: i64,
    pnl_24h_lamports: i64,
}

#[derive(sqlx::FromRow)]
struct RecordRow {
    pubkey: String,
    name: String,
    status: String,
    key_ref: String,
    is_primary: bool,
}

#[derive(sqlx::FromRow)]
struct NonceRow {
    pubkey: String,
    authority: String,
    status: String,
}

#[derive(sqlx::FromRow)]
struct MoveRow {
    id: Uuid,
    from_wallet: String,
    to_wallet: String,
    step: String,
    error: Option<String>,
    started_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
}

#[derive(sqlx::FromRow)]
struct ActivityRow {
    id: i64,
    kind: String,
    detail: serde_json::Value,
    signatures: Vec<String>,
    at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct AssignRow {
    rule_id: Uuid,
    wallet: String,
}

#[derive(sqlx::FromRow)]
struct PositionRow {
    id: Uuid,
    mint_address: String,
    token_account: Option<String>,
    token_program_id: Option<String>,
    status: String,
    entry_token_amount: Option<i64>,
}

impl TraderWalletRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn list(&self) -> sqlx::Result<Vec<TraderWallet>> {
        let rows = sqlx::query_as::<_, WalletRow>(&format!(
            "SELECT w.pubkey, w.name, w.status, w.is_primary, w.created_at, w.archived_at, \
                    (SELECT count(*) FROM trader_nonces n WHERE n.authority = w.pubkey AND n.status = 'live') AS nonce_count, \
                    (SELECT count(*) FROM rule_wallets r WHERE r.wallet = w.pubkey) AS rule_count, \
                    (SELECT count(*) FROM strategy_positions p \
                       WHERE p.wallet = w.pubkey AND p.mode = 'real' \
                         AND p.status NOT IN ('End', 'EntryFailed')) AS open_positions, \
                    (SELECT COALESCE(SUM({PNL_LAMPORTS_SQL}), 0)::BIGINT FROM strategy_positions sp \
                       WHERE sp.wallet = w.pubkey AND sp.mode = 'real' AND sp.status = 'End' \
                         AND sp.entry_lamports IS NOT NULL \
                         AND sp.exit_time >= now() - interval '24 hours') AS pnl_24h_lamports \
             FROM trader_wallets w \
             ORDER BY w.is_primary DESC, w.created_at"
        ))
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| TraderWallet {
            pubkey: r.pubkey,
            name: r.name,
            status: r.status,
            is_primary: r.is_primary,
            created_at: r.created_at,
            archived_at: r.archived_at,
            nonce_count: r.nonce_count,
            rule_count: r.rule_count,
            open_positions: r.open_positions,
            pnl_24h_lamports: r.pnl_24h_lamports,
        }).collect())
    }

    pub async fn rename(&self, pubkey: &str, name: &str) -> sqlx::Result<()> {
        sqlx::query("UPDATE trader_wallets SET name = $2 WHERE pubkey = $1")
            .bind(pubkey)
            .bind(name)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// An archived wallet back to active, never primary.
    pub async fn restore(&self, pubkey: &str) -> sqlx::Result<bool> {
        let n = sqlx::query(
            "UPDATE trader_wallets SET status = 'active', archived_at = NULL \
             WHERE pubkey = $1 AND status = 'archived'",
        )
        .bind(pubkey)
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(n == 1)
    }

    /// The rules that name `wallet`, by name.
    pub async fn rules_for(&self, wallet: &str) -> sqlx::Result<Vec<WalletRule>> {
        let rows: Vec<(Uuid, String, bool)> = sqlx::query_as(
            "SELECT r.id, r.rule_name, r.is_active FROM rule_wallets w \
             JOIN strategy_rules r ON r.id = w.rule_id \
             WHERE w.wallet = $1 ORDER BY r.rule_name",
        )
        .bind(wallet)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(rule_id, rule_name, is_active)| WalletRule { rule_id, rule_name, is_active })
            .collect())
    }

    pub async fn log_activity(
        &self,
        wallet: &str,
        kind: &str,
        detail: serde_json::Value,
        signatures: &[String],
    ) -> sqlx::Result<()> {
        sqlx::query(
            "INSERT INTO wallet_activity (wallet, kind, detail, signatures) VALUES ($1, $2, $3, $4)",
        )
        .bind(wallet)
        .bind(kind)
        .bind(detail)
        .bind(signatures)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn activity(&self, wallet: &str, limit: i64) -> sqlx::Result<Vec<WalletActivity>> {
        let rows = sqlx::query_as::<_, ActivityRow>(
            "SELECT id, kind, detail, signatures, at FROM wallet_activity \
             WHERE wallet = $1 ORDER BY at DESC, id DESC LIMIT $2",
        )
        .bind(wallet)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| WalletActivity {
                id: r.id,
                kind: r.kind,
                detail: r.detail,
                signatures: r.signatures,
                at: r.at,
            })
            .collect())
    }

    pub async fn records(&self) -> sqlx::Result<Vec<TraderWalletRecord>> {
        let rows = sqlx::query_as::<_, RecordRow>(
            "SELECT pubkey, name, status, key_ref, is_primary FROM trader_wallets",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| TraderWalletRecord {
                pubkey: r.pubkey,
                name: r.name,
                status: r.status,
                key_ref: r.key_ref,
                is_primary: r.is_primary,
            })
            .collect())
    }

    pub async fn insert(
        &self,
        pubkey: &str,
        name: &str,
        key_ref: &str,
        primary: bool,
    ) -> sqlx::Result<()> {
        let mut tx = self.pool.begin().await?;
        if primary {
            sqlx::query("UPDATE trader_wallets SET is_primary = false WHERE is_primary")
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query(
            "INSERT INTO trader_wallets (pubkey, name, status, key_ref, is_primary) \
             VALUES ($1, $2, 'active', $3, $4)",
        )
        .bind(pubkey)
        .bind(name)
        .bind(key_ref)
        .bind(primary)
        .execute(&mut *tx)
        .await?;
        tx.commit().await
    }

    pub async fn set_primary(&self, pubkey: &str) -> sqlx::Result<bool> {
        let mut tx = self.pool.begin().await?;
        let updated = sqlx::query(
            "UPDATE trader_wallets SET is_primary = true \
             WHERE pubkey = $1 AND status = 'active'",
        )
        .bind(pubkey)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if updated == 0 {
            tx.rollback().await?;
            return Ok(false);
        }
        sqlx::query(
            "UPDATE trader_wallets SET is_primary = false WHERE pubkey <> $1 AND is_primary",
        )
        .bind(pubkey)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(true)
    }

    pub async fn set_status(&self, pubkey: &str, status: &str) -> sqlx::Result<()> {
        let archived = if status == "archived" {
            Some(Utc::now())
        } else {
            None
        };
        sqlx::query(
            "UPDATE trader_wallets SET status = $2, \
                    is_primary = CASE WHEN $2 = 'archived' THEN false ELSE is_primary END, \
                    archived_at = COALESCE($3, archived_at) \
             WHERE pubkey = $1",
        )
        .bind(pubkey)
        .bind(status)
        .bind(archived)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn nonces_for(&self, authority: &str) -> sqlx::Result<Vec<TraderNonce>> {
        let rows = sqlx::query_as::<_, NonceRow>(
            "SELECT pubkey, authority, status FROM trader_nonces \
             WHERE authority = $1 AND status = 'live' ORDER BY pubkey",
        )
        .bind(authority)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| TraderNonce {
                pubkey: r.pubkey,
                authority: r.authority,
                status: r.status,
            })
            .collect())
    }

    /// The caller has the chain's word that `authority` owns `pubkey`, so a row
    /// naming another authority, or retired, is overwritten.
    pub async fn insert_nonce(&self, pubkey: &str, authority: &str) -> sqlx::Result<()> {
        sqlx::query(
            "INSERT INTO trader_nonces (pubkey, authority, status) VALUES ($1, $2, 'live') \
             ON CONFLICT (pubkey) DO UPDATE SET authority = EXCLUDED.authority, status = 'live'",
        )
        .bind(pubkey)
        .bind(authority)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn set_nonce_authority(&self, nonce: &str, authority: &str) -> sqlx::Result<()> {
        sqlx::query("UPDATE trader_nonces SET authority = $2 WHERE pubkey = $1")
            .bind(nonce)
            .bind(authority)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn assignments(&self) -> sqlx::Result<Vec<(Uuid, String)>> {
        let rows = sqlx::query_as::<_, AssignRow>("SELECT rule_id, wallet FROM rule_wallets")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(|r| (r.rule_id, r.wallet)).collect())
    }

    pub async fn set_assignment(&self, rule_id: Uuid, wallet: Option<&str>) -> sqlx::Result<()> {
        match wallet {
            None => {
                sqlx::query("DELETE FROM rule_wallets WHERE rule_id = $1")
                    .bind(rule_id)
                    .execute(&self.pool)
                    .await?;
            }
            Some(wallet) => {
                sqlx::query(
                    "INSERT INTO rule_wallets (rule_id, wallet) VALUES ($1, $2) \
                     ON CONFLICT (rule_id) DO UPDATE SET wallet = EXCLUDED.wallet",
                )
                .bind(rule_id)
                .bind(wallet)
                .execute(&self.pool)
                .await?;
            }
        }
        Ok(())
    }

    /// Point every rule that names `from` at `to`.
    pub async fn retarget_rules(&self, from: &str, to: &str) -> sqlx::Result<u64> {
        let res = sqlx::query("UPDATE rule_wallets SET wallet = $2 WHERE wallet = $1")
            .bind(from)
            .bind(to)
            .execute(&self.pool)
            .await?;
        Ok(res.rows_affected())
    }

    pub async fn open_positions(&self, wallet: &str) -> sqlx::Result<Vec<OpenWalletPosition>> {
        let rows = sqlx::query_as::<_, PositionRow>(
            "SELECT id, mint_address, token_account, token_program_id, status, entry_token_amount \
             FROM strategy_positions \
             WHERE wallet = $1 AND mode = 'real' AND status NOT IN ('End', 'EntryFailed') \
             ORDER BY created_at",
        )
        .bind(wallet)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| OpenWalletPosition {
                id: r.id,
                mint_address: r.mint_address,
                token_account: r.token_account,
                token_program_id: r.token_program_id,
                status: r.status,
                entry_token_amount: r.entry_token_amount,
            })
            .collect())
    }

    pub async fn repoint_position(
        &self,
        id: Uuid,
        wallet: &str,
        token_account: &str,
    ) -> sqlx::Result<()> {
        sqlx::query(
            "UPDATE strategy_positions SET wallet = $2, token_account = $3 WHERE id = $1",
        )
        .bind(id)
        .bind(wallet)
        .bind(token_account)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_move(&self, id: Uuid) -> sqlx::Result<Option<WalletMove>> {
        let row = sqlx::query_as::<_, MoveRow>(
            "SELECT id, from_wallet, to_wallet, step, error, started_at, finished_at \
             FROM wallet_moves WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(move_from_row))
    }

    pub async fn open_move(&self) -> sqlx::Result<Option<WalletMove>> {
        let row = sqlx::query_as::<_, MoveRow>(
            "SELECT id, from_wallet, to_wallet, step, error, started_at, finished_at \
             FROM wallet_moves WHERE finished_at IS NULL ORDER BY started_at DESC LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(move_from_row))
    }

    pub async fn insert_move(&self, from: &str, to: &str) -> sqlx::Result<WalletMove> {
        let row = sqlx::query_as::<_, MoveRow>(
            "INSERT INTO wallet_moves (from_wallet, to_wallet, step) \
             VALUES ($1, $2, 'quiesce') \
             RETURNING id, from_wallet, to_wallet, step, error, started_at, finished_at",
        )
        .bind(from)
        .bind(to)
        .fetch_one(&self.pool)
        .await?;
        Ok(move_from_row(row))
    }

    pub async fn set_move_step(
        &self,
        id: Uuid,
        step: &str,
        error: Option<&str>,
        finished: bool,
    ) -> sqlx::Result<()> {
        sqlx::query(
            "UPDATE wallet_moves SET step = $2, error = $3, \
                    finished_at = CASE WHEN $4 THEN now() ELSE NULL END \
             WHERE id = $1",
        )
        .bind(id)
        .bind(step)
        .bind(error)
        .bind(finished)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

fn move_from_row(r: MoveRow) -> WalletMove {
    WalletMove {
        id: r.id,
        from_wallet: r.from_wallet,
        to_wallet: r.to_wallet,
        step: r.step,
        error: r.error,
        started_at: r.started_at,
        finished_at: r.finished_at,
    }
}
