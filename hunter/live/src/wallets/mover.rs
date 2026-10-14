//! Move one wallet onto another, then archive the source.
//!
//! Order: stop new entries, wait until nothing is in flight, move bags, hand
//! nonce authority across, sweep SOL, verify. Each step is safe to run again.

use std::str::FromStr;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signer};
use solana_sdk::system_instruction;
use solana_sdk::transaction::Transaction;
use spl_associated_token_account::{
    get_associated_token_address_with_program_id,
    instruction::create_associated_token_account_idempotent,
};
use sqlx::PgPool;

use serde::Serialize;
use tracing::warn;
use trading_core::models::trader_wallet::{WalletMove, WalletRule};
use trading_core::storage::repositories::trader_wallet_repo::TraderWalletRepo;

use super::runtime::WalletRuntime;

/// Everything a move would carry, read before it starts.
#[derive(Debug, Serialize)]
pub struct MovePreview {
    pub rules: Vec<WalletRule>,
    pub primary: bool,
    pub tokens: Vec<PreviewToken>,
    pub nonces: usize,
    pub sol_lamports: u64,
    pub open_positions: usize,
    /// Positions still buying or selling; the move waits for these.
    pub in_flight: usize,
}

#[derive(Debug, Serialize)]
pub struct PreviewToken {
    pub mint: String,
    pub amount: u64,
    pub decimals: u8,
}

/// One token-account listing and one balance read of the source; the rest is Postgres.
pub async fn preview(pool: &PgPool, runtime: &WalletRuntime, from: &str) -> Result<MovePreview> {
    let repo = TraderWalletRepo::new(pool.clone());
    let record = repo
        .records()
        .await?
        .into_iter()
        .find(|r| r.pubkey == from)
        .context("source wallet is not registered")?;
    let source = runtime.trader(from).context("source wallet is not loaded")?;
    let tokens = source
        .get_all_token_accounts()
        .await?
        .into_iter()
        .filter(|h| h.amount > 0)
        .map(|h| PreviewToken { mint: h.mint, amount: h.amount, decimals: h.decimals })
        .collect();
    let open = repo.open_positions(from).await?;
    Ok(MovePreview {
        rules: repo.rules_for(from).await?,
        primary: record.is_primary,
        tokens,
        nonces: repo.nonces_for(from).await?.len(),
        sol_lamports: source.get_sol_balance().await?,
        in_flight: open.iter().filter(|p| INFLIGHT.contains(&p.status.as_str())).count(),
        open_positions: open.len(),
    })
}

const FEE_RESERVE: u64 = 10_000;
const INFLIGHT: &[&str] = &[
    "BuySubmitted",
    "ExitPending",
    "ExitUnconfirmed",
    "ExitStuck",
];

pub async fn start(
    pool: &PgPool,
    runtime: &Arc<WalletRuntime>,
    from: &str,
    to: &str,
) -> Result<WalletMove> {
    if !runtime.can_manage_keys() {
        bail!("wallet keystore is not configured");
    }
    if from == to {
        bail!("source and destination are the same wallet");
    }
    let repo = TraderWalletRepo::new(pool.clone());
    if repo.open_move().await?.is_some() {
        bail!("a move is already in progress");
    }
    let records = repo.records().await?;
    let source = records
        .iter()
        .find(|r| r.pubkey == from)
        .context("source wallet is not registered")?;
    let dest = records
        .iter()
        .find(|r| r.pubkey == to)
        .context("destination wallet is not registered")?;
    if source.status != "active" {
        bail!("source is not active");
    }
    if dest.status != "active" {
        bail!("destination is not active");
    }
    require_loaded(runtime, from)?;
    require_loaded(runtime, to)?;
    let mv = repo.insert_move(from, to).await?;
    let _ = repo
        .log_activity(from, "move_started", serde_json::json!({ "to": to }), &[])
        .await;
    // The driver advances it; the request returns at once.
    kick(pool.clone(), runtime.clone());
    Ok(mv)
}

/// Advance the open move once in the background. A busy lock means it is
/// already advancing.
pub fn kick(pool: PgPool, runtime: Arc<WalletRuntime>) {
    tokio::spawn(async move {
        if let Err(e) = resume(&pool, &runtime).await {
            warn!("wallet move: {e:#}");
        }
    });
}

/// How often the driver re-checks an open move that is waiting on in-flight work.
const DRIVE_EVERY: std::time::Duration = std::time::Duration::from_secs(5);

/// Keeps an open move going across waits and restarts. It retries a move that
/// is waiting (`inflight`) or was interrupted (no error); a move that failed
/// waits for a manual Resume, which re-runs the failed step.
pub fn spawn_driver(pool: PgPool, runtime: Arc<WalletRuntime>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let repo = TraderWalletRepo::new(pool.clone());
        loop {
            tokio::time::sleep(DRIVE_EVERY).await;
            let Ok(Some(mv)) = repo.open_move().await else {
                continue;
            };
            if mv.error.is_some() && mv.step != "inflight" {
                continue;
            }
            if let Err(e) = resume(&pool, &runtime).await {
                warn!("wallet move: {e:#}");
            }
        }
    })
}

pub async fn resume(pool: &PgPool, runtime: &WalletRuntime) -> Result<WalletMove> {
    let Ok(_lock) = runtime.move_lock.try_lock() else {
        bail!("the move is already advancing");
    };
    let repo = TraderWalletRepo::new(pool.clone());
    let Some(mv) = repo.open_move().await? else {
        bail!("no move is in progress");
    };
    if let Err(e) = advance(pool, runtime, &mv).await {
        let step = repo
            .open_move()
            .await
            .ok()
            .flatten()
            .map(|row| row.step)
            .unwrap_or_else(|| mv.step.clone());
        let _ = repo
            .set_move_step(mv.id, &step, Some(&e.to_string()), false)
            .await;
        return Err(e);
    }
    match repo.open_move().await? {
        Some(mv) => Ok(mv),
        None => repo
            .get_move(mv.id)
            .await?
            .context("move row missing"),
    }
}

pub(super) fn require_loaded(runtime: &WalletRuntime, pubkey: &str) -> Result<()> {
    match runtime.signer(pubkey) {
        Some(key) if key.pubkey().to_string() == pubkey => Ok(()),
        _ => bail!("wallet {pubkey} is not loaded"),
    }
}

async fn advance(pool: &PgPool, runtime: &WalletRuntime, mv: &WalletMove) -> Result<()> {
    let repo = TraderWalletRepo::new(pool.clone());
    let mut step = mv.step.clone();
    loop {
        match step.as_str() {
            "quiesce" => {
                quiesce(pool, runtime, &mv.from_wallet, &mv.to_wallet).await?;
                step = "inflight".into();
                repo.set_move_step(mv.id, &step, None, false).await?;
            }
            "inflight" => {
                let open = repo.open_positions(&mv.from_wallet).await?;
                if open.iter().any(|p| INFLIGHT.contains(&p.status.as_str())) {
                    repo.set_move_step(
                        mv.id,
                        "inflight",
                        Some("waiting for in-flight entries and sells"),
                        false,
                    )
                    .await?;
                    return Ok(());
                }
                step = "bags".into();
                repo.set_move_step(mv.id, &step, None, false).await?;
            }
            "bags" => {
                runtime.freeze.freeze(&mv.from_wallet);
                let moved = move_tokens(runtime, &repo, &mv.from_wallet, &mv.to_wallet).await;
                runtime.freeze.thaw(&mv.from_wallet);
                moved?;
                step = "nonces".into();
                repo.set_move_step(mv.id, &step, None, false).await?;
            }
            "nonces" => {
                move_nonces(runtime, &repo, &mv.from_wallet, &mv.to_wallet).await?;
                step = "sol".into();
                repo.set_move_step(mv.id, &step, None, false).await?;
            }
            "sol" => {
                sweep_sol(runtime, &mv.from_wallet, &mv.to_wallet).await?;
                step = "verify".into();
                repo.set_move_step(mv.id, &step, None, false).await?;
            }
            "verify" => {
                verify(&repo, &mv.from_wallet).await?;
                repo.set_status(&mv.from_wallet, "archived").await?;
                runtime.unload(&mv.from_wallet);
                repo.set_move_step(mv.id, "done", None, true).await?;
                let detail = serde_json::json!({ "from": mv.from_wallet, "to": mv.to_wallet });
                let _ = repo.log_activity(&mv.from_wallet, "move_done", detail.clone(), &[]).await;
                let _ = repo.log_activity(&mv.to_wallet, "move_received", detail, &[]).await;
                return Ok(());
            }
            "done" => return Ok(()),
            other => bail!("unknown move step {other}"),
        }
    }
}

async fn quiesce(pool: &PgPool, runtime: &WalletRuntime, from: &str, to: &str) -> Result<()> {
    let repo = TraderWalletRepo::new(pool.clone());
    let source_primary = repo
        .records()
        .await?
        .into_iter()
        .any(|r| r.pubkey == from && r.is_primary);
    repo.set_status(from, "draining").await?;
    repo.retarget_rules(from, to).await?;
    if source_primary {
        repo.set_primary(to).await?;
    }
    runtime.reload_directory(pool).await?;
    Ok(())
}

async fn move_tokens(
    runtime: &WalletRuntime,
    repo: &TraderWalletRepo,
    from: &str,
    to: &str,
) -> Result<()> {
    let source = runtime.trader(from).context("source trader")?;
    let holdings = source.get_all_token_accounts().await?;
    let positions = repo.open_positions(from).await?;
    let dest = Pubkey::from_str(to)?;
    let source_key = runtime.signer(from).context("source key")?;
    for holding in &holdings {
        if holding.amount == 0 {
            continue;
        }
        let program = Pubkey::from_str(&holding.token_program_id)?;
        let mint = Pubkey::from_str(&holding.mint)?;
        let source_account = Pubkey::from_str(&holding.token_account)?;
        let dest_account = get_associated_token_address_with_program_id(&dest, &mint, &program);
        send(
            runtime,
            &source_key,
            vec![
                create_associated_token_account_idempotent(
                    &source_key.pubkey(),
                    &dest,
                    &mint,
                    &program,
                ),
                token_transfer(
                    &program,
                    &mint,
                    &source_account,
                    &dest_account,
                    &source_key.pubkey(),
                    holding.amount,
                    holding.decimals,
                )?,
            ],
            &[],
        )
        .await?;
        close_if_empty(runtime, &source_key, &program, &source_account, &dest).await?;
    }
    let dest_ata_for = |mint: &str, program: &str| -> Result<String> {
        let mint = Pubkey::from_str(mint)?;
        let program = Pubkey::from_str(program).unwrap_or_else(|_| spl_token::id());
        Ok(get_associated_token_address_with_program_id(&dest, &mint, &program).to_string())
    };
    for position in positions {
        let program = position
            .token_program_id
            .clone()
            .unwrap_or_else(|| spl_token::id().to_string());
        let account = dest_ata_for(&position.mint_address, &program)?;
        repo.repoint_position(position.id, to, &account).await?;
    }
    Ok(())
}

fn token_transfer(
    program: &Pubkey,
    mint: &Pubkey,
    source: &Pubkey,
    dest: &Pubkey,
    owner: &Pubkey,
    amount: u64,
    decimals: u8,
) -> Result<solana_sdk::instruction::Instruction> {
    if *program == spl_token_2022::id() {
        spl_token_2022::instruction::transfer_checked(
            program, source, mint, dest, owner, &[], amount, decimals,
        )
        .context("token-2022 transfer")
    } else {
        spl_token::instruction::transfer_checked(
            program, source, mint, dest, owner, &[], amount, decimals,
        )
        .context("token transfer")
    }
}

async fn close_if_empty(
    runtime: &WalletRuntime,
    owner: &Arc<Keypair>,
    program: &Pubkey,
    account: &Pubkey,
    dest: &Pubkey,
) -> Result<()> {
    let ix = if *program == spl_token_2022::id() {
        spl_token_2022::instruction::close_account(program, account, dest, &owner.pubkey(), &[])
    } else {
        spl_token::instruction::close_account(program, account, dest, &owner.pubkey(), &[])
    };
    let ix = match ix {
        Ok(ix) => ix,
        Err(_) => return Ok(()),
    };
    // A non-empty account rejects the close. The tokens already moved, so a
    // failure here only leaves rent behind for a later sweep.
    let _ = send(runtime, owner, vec![ix], &[]).await;
    Ok(())
}

async fn move_nonces(
    runtime: &WalletRuntime,
    repo: &TraderWalletRepo,
    from: &str,
    to: &str,
) -> Result<()> {
    let source = runtime.signer(from).context("source key")?;
    let dest = Pubkey::from_str(to)?;
    let nonces = repo.nonces_for(from).await?;
    for nonce in &nonces {
        let nonce_pk = Pubkey::from_str(&nonce.pubkey)?;
        send(
            runtime,
            &source,
            vec![system_instruction::authorize_nonce_account(
                &nonce_pk,
                &source.pubkey(),
                &dest,
            )],
            &[],
        )
        .await?;
        repo.set_nonce_authority(&nonce.pubkey, to).await?;
    }
    let parsed = repo
        .nonces_for(to)
        .await?
        .into_iter()
        .map(|n| n.pubkey.parse())
        .collect::<Result<Vec<Pubkey>, _>>()?;
    runtime.rebuild(to, parsed).await?;
    Ok(())
}

async fn sweep_sol(runtime: &WalletRuntime, from: &str, to: &str) -> Result<()> {
    let trader = runtime.trader(from).context("source trader")?;
    let balance = trader.get_sol_balance().await?;
    if balance <= FEE_RESERVE {
        return Ok(());
    }
    let source = runtime.signer(from).context("source key")?;
    let dest = Pubkey::from_str(to)?;
    send(
        runtime,
        &source,
        vec![system_instruction::transfer(
            &source.pubkey(),
            &dest,
            balance - FEE_RESERVE,
        )],
        &[],
    )
    .await?;
    Ok(())
}

async fn verify(repo: &TraderWalletRepo, from: &str) -> Result<()> {
    let open = repo.open_positions(from).await?;
    if !open.is_empty() {
        bail!("{} open positions still name the source", open.len());
    }
    let nonces = repo.nonces_for(from).await?;
    if !nonces.is_empty() {
        bail!("{} nonce accounts still name the source", nonces.len());
    }
    Ok(())
}

pub(super) async fn send(
    runtime: &WalletRuntime,
    payer: &Arc<Keypair>,
    ixs: Vec<solana_sdk::instruction::Instruction>,
    extra: &[&Keypair],
) -> Result<String> {
    let rpc = RpcClient::new(runtime.rpc_url().to_string());
    let blockhash = rpc.get_latest_blockhash().await?;
    let mut signers: Vec<&Keypair> = Vec::with_capacity(1 + extra.len());
    signers.push(payer.as_ref());
    signers.extend_from_slice(extra);
    let tx = Transaction::new_signed_with_payer(&ixs, Some(&payer.pubkey()), &signers, blockhash);
    let sig = rpc
        .send_and_confirm_transaction(&tx)
        .await
        .context("send")?;
    Ok(sig.to_string())
}
