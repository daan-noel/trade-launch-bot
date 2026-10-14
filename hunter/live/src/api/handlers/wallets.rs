//! Wallet registry. Responses carry pubkeys and names. The private key is
//! encrypted on import and is not returned.

use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use actix_web::{web, HttpResponse, Responder};
use serde::Deserialize;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signer;
use uuid::Uuid;

use crate::state::deploy_state::DeployState;
use crate::wallets::{self, key_ref_for, parse_secret, valid_name, write_keypair, WalletRuntime};
use trading_core::storage::repositories::trader_wallet_repo::TraderWalletRepo;

/// A cached balance older than this is re-read, in one batched call for every
/// stale wallet. The push feed refreshes a watched wallet only when it changes.
const BALANCE_MAX_AGE: Duration = Duration::from_secs(60);

#[derive(Deserialize)]
pub struct CreateBody {
    pub name: String,
}

#[derive(Deserialize)]
pub struct ImportBody {
    pub name: String,
    pub private_key: String,
}

#[derive(Deserialize)]
pub struct AssignBody {
    pub wallet: Option<String>,
}

#[derive(Deserialize)]
pub struct MoveBody {
    pub from: String,
    pub to: String,
}

#[derive(Deserialize)]
pub struct NonceBody {
    pub count: u8,
}

#[derive(Deserialize)]
pub struct AttachBody {
    pub nonces: Vec<String>,
}

#[derive(Deserialize)]
pub struct AdoptEnvBody {
    pub name: Option<String>,
}

pub async fn list(state: web::Data<Arc<DeployState>>) -> impl Responder {
    let repo = TraderWalletRepo::new(state.db.clone());
    let wallets = match repo.list().await {
        Ok(rows) => rows,
        Err(e) => return HttpResponse::InternalServerError().json(err(e.to_string())),
    };
    let pubkeys: Vec<&str> = wallets.iter().map(|w| w.pubkey.as_str()).collect();
    let balances = balances(&state.wallets, &pubkeys).await;
    let mut out = Vec::with_capacity(wallets.len());
    for (wallet, lamports) in wallets.iter().zip(balances) {
        let readiness = state.wallets.readiness(&wallet.pubkey, lamports);
        out.push(serde_json::json!({
            "pubkey": wallet.pubkey,
            "name": wallet.name,
            "status": wallet.status,
            "is_primary": wallet.is_primary,
            "nonce_count": wallet.nonce_count,
            "rule_count": wallet.rule_count,
            "open_positions": wallet.open_positions,
            "sol": lamports.map(|l| l as f64 / 1_000_000_000.0),
            "key_loaded": state.wallets.signer(&wallet.pubkey).map(|k| k.pubkey().to_string()) == Some(wallet.pubkey.clone()),
            "readiness": readiness,
            "readiness_reason": readiness.reason(),
            "init_error": state.wallets.init_error(&wallet.pubkey),
            "created_at": wallet.created_at,
            "archived_at": wallet.archived_at,
        }));
    }
    let open = repo.open_move().await.ok().flatten();
    let env = env_status(&state.wallets, &repo, &pubkeys).await;
    HttpResponse::Ok().json(serde_json::json!({ "wallets": out, "move": open, "env_wallet": env }))
}

/// Cached balances, with every missing or stale one read in a single
/// `getMultipleAccounts`. `None` when that read fails.
async fn balances(runtime: &WalletRuntime, pubkeys: &[&str]) -> Vec<Option<u64>> {
    let mut out: Vec<Option<u64>> = Vec::with_capacity(pubkeys.len());
    let mut stale: Vec<(usize, Pubkey)> = Vec::new();
    for (i, pubkey) in pubkeys.iter().enumerate() {
        let cached = runtime
            .trader(pubkey)
            .filter(|t| t.balance_cache_is_fresher_than(BALANCE_MAX_AGE))
            .and_then(|t| t.cached_balance_lamports());
        if cached.is_none() {
            if let Ok(pk) = Pubkey::from_str(pubkey) {
                stale.push((i, pk));
            }
        }
        out.push(cached);
    }
    if stale.is_empty() {
        return out;
    }
    let rpc = RpcClient::new(runtime.rpc_url().to_string());
    let keys: Vec<Pubkey> = stale.iter().map(|(_, pk)| *pk).collect();
    if let Ok(accounts) = rpc.get_multiple_accounts(&keys).await {
        for ((i, _), account) in stale.iter().zip(accounts) {
            let lamports = account.map(|a| a.lamports).unwrap_or(0);
            if let Some(trader) = runtime.trader(pubkeys[*i]) {
                trader.update_balance_lamports_cache(lamports);
            }
            out[*i] = Some(lamports);
        }
    }
    out
}

/// The env key's state for the "bring it in" banner. `None` when no env key is set.
async fn env_status(
    runtime: &WalletRuntime,
    repo: &TraderWalletRepo,
    registered: &[&str],
) -> Option<serde_json::Value> {
    let env = runtime.env_wallet()?;
    let attached: HashSet<String> = repo
        .nonces_for(&env.pubkey)
        .await
        .map(|rows| rows.into_iter().map(|n| n.pubkey).collect())
        .unwrap_or_default();
    let unattached = env.nonces.iter().filter(|n| !attached.contains(&n.to_string())).count();
    Some(serde_json::json!({
        "pubkey": env.pubkey,
        "registered": registered.contains(&env.pubkey.as_str()),
        "unattached_nonces": unattached,
        "can_register": runtime.can_manage_keys(),
    }))
}

pub async fn create(state: web::Data<Arc<DeployState>>, body: web::Json<CreateBody>) -> impl Responder {
    if !valid_name(&body.name) {
        return HttpResponse::BadRequest().json(err("name must be lowercase letters, digits, and dashes"));
    }
    let Some(dir) = state.wallets.keystore_dir().cloned() else {
        return HttpResponse::BadRequest().json(err("WALLET_KEYSTORE is not set"));
    };
    let Some(kek) = state.wallets.kek() else {
        return HttpResponse::BadRequest().json(err("WALLET_KEK is not set"));
    };
    let repo = TraderWalletRepo::new(state.db.clone());
    if let Err(resp) = name_free(&repo, &body.name).await {
        return resp;
    }
    let keypair = solana_sdk::signature::Keypair::new();
    let secret = keypair.to_bytes();
    let key_ref = key_ref_for(&body.name);
    if let Err(e) = write_keypair(&dir, &key_ref, &secret, kek) {
        return HttpResponse::BadRequest().json(err(e.to_string()));
    }
    let pubkey = keypair.pubkey().to_string();
    let primary = state.wallets.primary_pubkey().is_empty();
    if let Err(e) = repo.insert(&pubkey, &body.name, &key_ref, primary).await {
        return HttpResponse::InternalServerError().json(err(e.to_string()));
    }
    state
        .wallets
        .insert_loaded(pubkey.clone(), body.name.clone(), Arc::new(keypair), Vec::new(), primary)
        .await;
    HttpResponse::Ok().json(serde_json::json!({ "pubkey": pubkey, "name": body.name }))
}

pub async fn import(state: web::Data<Arc<DeployState>>, body: web::Json<ImportBody>) -> impl Responder {
    if !valid_name(&body.name) {
        return HttpResponse::BadRequest().json(err("name must be lowercase letters, digits, and dashes"));
    }
    let Some(dir) = state.wallets.keystore_dir().cloned() else {
        return HttpResponse::BadRequest().json(err("WALLET_KEYSTORE is not set"));
    };
    let Some(kek) = state.wallets.kek() else {
        return HttpResponse::BadRequest().json(err("WALLET_KEK is not set"));
    };
    let secret = match parse_secret(&body.private_key) {
        Ok(secret) => secret,
        Err(e) => return HttpResponse::BadRequest().json(err(e.to_string())),
    };
    let keypair = match solana_sdk::signature::Keypair::from_bytes(&secret) {
        Ok(keypair) => keypair,
        Err(e) => return HttpResponse::BadRequest().json(err(e.to_string())),
    };
    let pubkey = keypair.pubkey().to_string();
    let repo = TraderWalletRepo::new(state.db.clone());
    let records = match repo.records().await {
        Ok(rows) => rows,
        Err(e) => return HttpResponse::InternalServerError().json(err(e.to_string())),
    };
    if let Some(existing) = records.iter().find(|r| r.pubkey == pubkey) {
        return HttpResponse::BadRequest().json(err(format!("this key is already registered as {}", existing.name)));
    }
    // The env key is already running with its nonce accounts. Registering it
    // keeps that trader and attaches NONCE_ACCOUNTS instead of starting it bare.
    if state.wallets.env_wallet().is_some_and(|env| env.pubkey == pubkey) {
        return match wallets::adopt_env(&state.db, &state.wallets, &body.name).await {
            Ok(report) => HttpResponse::Ok().json(serde_json::json!({ "pubkey": pubkey, "name": body.name, "nonces": report })),
            Err(e) => HttpResponse::BadRequest().json(err(e.to_string())),
        };
    }
    if let Err(resp) = name_free(&repo, &body.name).await {
        return resp;
    }
    let key_ref = key_ref_for(&body.name);
    if let Err(e) = write_keypair(&dir, &key_ref, &secret, kek) {
        return HttpResponse::BadRequest().json(err(e.to_string()));
    }
    let primary = state.wallets.primary_pubkey().is_empty();
    if let Err(e) = repo.insert(&pubkey, &body.name, &key_ref, primary).await {
        return HttpResponse::InternalServerError().json(err(e.to_string()));
    }
    state
        .wallets
        .insert_loaded(pubkey.clone(), body.name.clone(), Arc::new(keypair), Vec::new(), primary)
        .await;
    HttpResponse::Ok().json(serde_json::json!({ "pubkey": pubkey, "name": body.name }))
}

async fn name_free(repo: &TraderWalletRepo, name: &str) -> Result<(), HttpResponse> {
    match repo.records().await {
        Ok(rows) if rows.iter().any(|r| r.name == name) => {
            Err(HttpResponse::BadRequest().json(err(format!("a wallet named {name} already exists"))))
        }
        Ok(_) => Ok(()),
        Err(e) => Err(HttpResponse::InternalServerError().json(err(e.to_string()))),
    }
}

/// Refuses a wallet whose trader cannot sign: entries pointed at it would fail
/// closed with nothing on the page to say why.
fn require_can_sign(runtime: &WalletRuntime, pubkey: &str) -> Result<(), HttpResponse> {
    let readiness = runtime.readiness(pubkey, None);
    if readiness.can_sign() {
        return Ok(());
    }
    Err(HttpResponse::BadRequest().json(err(format!("wallet cannot sign: {}", readiness.reason()))))
}

pub async fn set_primary(
    state: web::Data<Arc<DeployState>>,
    path: web::Path<String>,
) -> impl Responder {
    let pubkey = path.into_inner();
    if let Err(resp) = require_can_sign(&state.wallets, &pubkey) {
        return resp;
    }
    let repo = TraderWalletRepo::new(state.db.clone());
    match repo.set_primary(&pubkey).await {
        Ok(true) => {
            state.wallets.note_primary(&pubkey);
            HttpResponse::Ok().json(serde_json::json!({ "pubkey": pubkey }))
        }
        Ok(false) => HttpResponse::BadRequest().json(err("wallet is not active")),
        Err(e) => HttpResponse::InternalServerError().json(err(e.to_string())),
    }
}

pub async fn assign_rule(
    state: web::Data<Arc<DeployState>>,
    path: web::Path<Uuid>,
    body: web::Json<AssignBody>,
) -> impl Responder {
    let rule_id = path.into_inner();
    let repo = TraderWalletRepo::new(state.db.clone());
    if let Some(wallet) = &body.wallet {
        let records = match repo.records().await {
            Ok(rows) => rows,
            Err(e) => return HttpResponse::InternalServerError().json(err(e.to_string())),
        };
        if !records.iter().any(|r| r.pubkey == *wallet && r.status == "active") {
            return HttpResponse::BadRequest().json(err("wallet is not active"));
        }
        if let Err(resp) = require_can_sign(&state.wallets, wallet) {
            return resp;
        }
    }
    if let Err(e) = repo.set_assignment(rule_id, body.wallet.as_deref()).await {
        return HttpResponse::InternalServerError().json(err(e.to_string()));
    }
    state.wallets.directory.assign(rule_id, body.wallet.clone());
    HttpResponse::Ok().json(serde_json::json!({ "rule_id": rule_id, "wallet": body.wallet }))
}

pub async fn assignments(state: web::Data<Arc<DeployState>>) -> impl Responder {
    let repo = TraderWalletRepo::new(state.db.clone());
    match repo.assignments().await {
        Ok(rows) => {
            let rows: Vec<_> = rows
                .into_iter()
                .map(|(rule_id, wallet)| serde_json::json!({ "rule_id": rule_id, "wallet": wallet }))
                .collect();
            HttpResponse::Ok().json(rows)
        }
        Err(e) => HttpResponse::InternalServerError().json(err(e.to_string())),
    }
}

pub async fn begin_move(
    state: web::Data<Arc<DeployState>>,
    body: web::Json<MoveBody>,
) -> impl Responder {
    match wallets::start_move(&state.db, &state.wallets, &body.from, &body.to).await {
        Ok(mv) => HttpResponse::Ok().json(mv),
        Err(e) => HttpResponse::BadRequest().json(err(e.to_string())),
    }
}

pub async fn resume_move(state: web::Data<Arc<DeployState>>) -> impl Responder {
    match wallets::resume_move(&state.db, &state.wallets).await {
        Ok(mv) => HttpResponse::Ok().json(mv),
        Err(e) => HttpResponse::BadRequest().json(err(e.to_string())),
    }
}

pub async fn provision(
    state: web::Data<Arc<DeployState>>,
    path: web::Path<String>,
    body: web::Json<NonceBody>,
) -> impl Responder {
    let pubkey = path.into_inner();
    match wallets::provision_nonces(&state.db, &state.wallets, &pubkey, body.count).await {
        Ok(n) => HttpResponse::Ok().json(serde_json::json!({ "created": n })),
        Err(e) => HttpResponse::BadRequest().json(err(e.to_string())),
    }
}

pub async fn attach(
    state: web::Data<Arc<DeployState>>,
    path: web::Path<String>,
    body: web::Json<AttachBody>,
) -> impl Responder {
    let pubkey = path.into_inner();
    match wallets::attach_nonces(&state.db, &state.wallets, &pubkey, &body.nonces).await {
        Ok(report) => HttpResponse::Ok().json(report),
        Err(e) => HttpResponse::BadRequest().json(err(e.to_string())),
    }
}

pub async fn adopt_env(
    state: web::Data<Arc<DeployState>>,
    body: web::Json<AdoptEnvBody>,
) -> impl Responder {
    let name = body.name.as_deref().unwrap_or(wallets::ENV_WALLET_NAME);
    match wallets::adopt_env(&state.db, &state.wallets, name).await {
        Ok(report) => HttpResponse::Ok().json(report),
        Err(e) => HttpResponse::BadRequest().json(err(e.to_string())),
    }
}

fn err(error: impl Into<String>) -> serde_json::Value {
    serde_json::json!({ "error": error.into() })
}
