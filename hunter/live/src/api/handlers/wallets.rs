//! Wallet registry. Responses carry pubkeys and names. The private key is
//! encrypted on import and is not returned.

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
use crate::wallets::{self, key_ref_for, name_error, parse_secret, write_keypair, WalletRuntime};
use trading_core::config::constants::{lamports_to_sol, sol_to_lamports};
use trading_core::storage::repositories::trader_wallet_repo::TraderWalletRepo;

/// How many activity rows the wallet drawer shows.
const ACTIVITY_LIMIT: i64 = 50;

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
pub struct SendBody {
    pub to: String,
    pub sol: f64,
    /// The destination may be outside the registry. The page asks for the
    /// source wallet's name before it sets this.
    #[serde(default)]
    pub external: bool,
}

#[derive(Deserialize)]
pub struct RenameBody {
    pub name: String,
}

#[derive(Deserialize)]
pub struct PreviewQuery {
    pub from: String,
}

pub async fn list(state: web::Data<Arc<DeployState>>) -> impl Responder {
    let repo = TraderWalletRepo::new(state.db.clone());
    let wallets = match repo.list().await {
        Ok(rows) => rows,
        Err(e) => return HttpResponse::InternalServerError().json(err(format!("{e:#}"))),
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
            "pnl_24h_sol": lamports_to_sol(wallet.pnl_24h_lamports),
            "sol": lamports.map(|l| lamports_to_sol(l as i64)),
            "key_loaded": state.wallets.signer(&wallet.pubkey).map(|k| k.pubkey().to_string()) == Some(wallet.pubkey.clone()),
            "readiness": readiness,
            "readiness_reason": readiness.reason(),
            "init_error": state.wallets.init_error(&wallet.pubkey),
            "load_error": state.wallets.load_error(&wallet.pubkey),
            "created_at": wallet.created_at,
            "archived_at": wallet.archived_at,
        }));
    }
    let open = repo.open_move().await.ok().flatten();
    // Active wallets and none primary: entries on rules without a wallet fail
    // closed until one is set.
    let primary_missing =
        wallets.iter().any(|w| w.status == "active") && !wallets.iter().any(|w| w.is_primary && w.status != "archived");
    HttpResponse::Ok().json(serde_json::json!({ "wallets": out, "move": open, "primary_missing": primary_missing }))
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

pub async fn create(state: web::Data<Arc<DeployState>>, body: web::Json<CreateBody>) -> impl Responder {
    let Some(dir) = state.wallets.keystore_dir().cloned() else {
        return HttpResponse::BadRequest().json(err("WALLET_KEYSTORE is not set"));
    };
    let Some(kek) = state.wallets.kek() else {
        return HttpResponse::BadRequest().json(err("WALLET_KEK is not set"));
    };
    let repo = TraderWalletRepo::new(state.db.clone());
    let primary = match check_new(&repo, &body.name).await {
        Ok(primary) => primary,
        Err(resp) => return resp,
    };
    let keypair = solana_sdk::signature::Keypair::new();
    let secret = keypair.to_bytes();
    let pubkey = keypair.pubkey().to_string();
    let key_ref = key_ref_for(&pubkey);
    if let Err(e) = write_keypair(&dir, &key_ref, &secret, kek) {
        return HttpResponse::BadRequest().json(err(format!("{e:#}")));
    }
    if let Err(e) = repo.insert(&pubkey, &body.name, &key_ref, primary).await {
        return HttpResponse::InternalServerError().json(err(format!("{e:#}")));
    }
    let _ = repo.log_activity(&pubkey, "created", serde_json::json!({ "name": body.name }), &[]).await;
    state
        .wallets
        .insert_loaded(pubkey.clone(), body.name.clone(), Arc::new(keypair), Vec::new(), primary)
        .await;
    HttpResponse::Ok().json(serde_json::json!({ "pubkey": pubkey, "name": body.name }))
}

pub async fn import(state: web::Data<Arc<DeployState>>, body: web::Json<ImportBody>) -> impl Responder {
    let Some(dir) = state.wallets.keystore_dir().cloned() else {
        return HttpResponse::BadRequest().json(err("WALLET_KEYSTORE is not set"));
    };
    let Some(kek) = state.wallets.kek() else {
        return HttpResponse::BadRequest().json(err("WALLET_KEK is not set"));
    };
    let secret = match parse_secret(&body.private_key) {
        Ok(secret) => secret,
        Err(e) => return HttpResponse::BadRequest().json(err(format!("{e:#}"))),
    };
    let keypair = match solana_sdk::signature::Keypair::from_bytes(&secret) {
        Ok(keypair) => keypair,
        Err(e) => return HttpResponse::BadRequest().json(err(format!("{e:#}"))),
    };
    let pubkey = keypair.pubkey().to_string();
    let repo = TraderWalletRepo::new(state.db.clone());
    let records = match repo.records().await {
        Ok(rows) => rows,
        Err(e) => return HttpResponse::InternalServerError().json(err(format!("{e:#}"))),
    };
    if let Some(existing) = records.iter().find(|r| r.pubkey == pubkey) {
        return HttpResponse::BadRequest().json(err(format!("this key is already registered as {}", existing.name)));
    }
    let primary = match check_new(&repo, &body.name).await {
        Ok(primary) => primary,
        Err(resp) => return resp,
    };
    let key_ref = key_ref_for(&pubkey);
    if let Err(e) = write_keypair(&dir, &key_ref, &secret, kek) {
        return HttpResponse::BadRequest().json(err(format!("{e:#}")));
    }
    if let Err(e) = repo.insert(&pubkey, &body.name, &key_ref, primary).await {
        return HttpResponse::InternalServerError().json(err(format!("{e:#}")));
    }
    let _ = repo.log_activity(&pubkey, "imported", serde_json::json!({ "name": body.name }), &[]).await;
    // An imported key may already hold SOL and own nonce accounts. A failed read
    // still loads the wallet bare; Refresh reads it again.
    let (nonces, sync) = match wallets::sync_new(&state.db, &state.wallets, &keypair.pubkey()).await {
        Ok((nonces, report)) => (nonces, Ok(report)),
        Err(e) => (Vec::new(), Err(format!("{e:#}"))),
    };
    let trader = state
        .wallets
        .insert_loaded(pubkey.clone(), body.name.clone(), Arc::new(keypair), nonces, primary)
        .await;
    if let Ok(report) = &sync {
        trader.update_balance_lamports_cache(report.sol_lamports);
    }
    HttpResponse::Ok().json(serde_json::json!({ "pubkey": pubkey, "name": body.name, "sync": sync }))
}

/// A new wallet's name is free, and whether it becomes primary: it does when
/// no active row is. Read from the registry, never from what is loaded.
async fn check_new(repo: &TraderWalletRepo, name: &str) -> Result<bool, HttpResponse> {
    let records = repo.records().await.map_err(|e| HttpResponse::InternalServerError().json(err(format!("{e:#}"))))?;
    if let Some(e) = name_error(&records, name, None) {
        return Err(HttpResponse::BadRequest().json(err(e)));
    }
    Ok(!records.iter().any(|r| r.is_primary && r.status != "archived"))
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
            let _ = repo.log_activity(&pubkey, "set_primary", serde_json::json!({}), &[]).await;
            HttpResponse::Ok().json(serde_json::json!({ "pubkey": pubkey }))
        }
        Ok(false) => HttpResponse::BadRequest().json(err("wallet is not active")),
        Err(e) => HttpResponse::InternalServerError().json(err(format!("{e:#}"))),
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
            Err(e) => return HttpResponse::InternalServerError().json(err(format!("{e:#}"))),
        };
        if !records.iter().any(|r| r.pubkey == *wallet && r.status == "active") {
            return HttpResponse::BadRequest().json(err("wallet is not active"));
        }
        if let Err(resp) = require_can_sign(&state.wallets, wallet) {
            return resp;
        }
    }
    if let Err(e) = repo.set_assignment(rule_id, body.wallet.as_deref()).await {
        return HttpResponse::InternalServerError().json(err(format!("{e:#}")));
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
        Err(e) => HttpResponse::InternalServerError().json(err(format!("{e:#}"))),
    }
}

pub async fn begin_move(
    state: web::Data<Arc<DeployState>>,
    body: web::Json<MoveBody>,
) -> impl Responder {
    match wallets::start_move(&state.db, &state.wallets, &body.from, &body.to).await {
        Ok(mv) => HttpResponse::Ok().json(mv),
        Err(e) => HttpResponse::BadRequest().json(err(format!("{e:#}"))),
    }
}

pub async fn resume_move(state: web::Data<Arc<DeployState>>) -> impl Responder {
    match wallets::resume_move(&state.db, &state.wallets).await {
        Ok(mv) => HttpResponse::Ok().json(mv),
        Err(e) => HttpResponse::BadRequest().json(err(format!("{e:#}"))),
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
        Err(e) => HttpResponse::BadRequest().json(err(format!("{e:#}"))),
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
        Err(e) => HttpResponse::BadRequest().json(err(format!("{e:#}"))),
    }
}

pub async fn preview_move(
    state: web::Data<Arc<DeployState>>,
    query: web::Query<PreviewQuery>,
) -> impl Responder {
    match wallets::preview_move(&state.db, &state.wallets, &query.from).await {
        Ok(preview) => HttpResponse::Ok().json(preview),
        Err(e) => HttpResponse::BadRequest().json(err(format!("{e:#}"))),
    }
}

/// Re-read one wallet's balance and nonce accounts from the chain and store them.
pub async fn sync(state: web::Data<Arc<DeployState>>, path: web::Path<String>) -> impl Responder {
    match wallets::sync_wallet(&state.db, &state.wallets, &path.into_inner()).await {
        Ok(report) => HttpResponse::Ok().json(report),
        Err(e) => HttpResponse::BadRequest().json(err(format!("{e:#}"))),
    }
}

/// The page's Refresh: re-read the registry (rows and key files a sync or
/// another session added, archives, renames, the primary), then [`sync`] every
/// active wallet, one after another. A failure is that wallet's entry, never
/// the whole reply.
pub async fn sync_all(state: web::Data<Arc<DeployState>>) -> impl Responder {
    if let Err(e) = state.wallets.reload(&state.db).await {
        return HttpResponse::InternalServerError().json(err(format!("reload wallets: {e:#}")));
    }
    let repo = TraderWalletRepo::new(state.db.clone());
    let records = match repo.records().await {
        Ok(rows) => rows,
        Err(e) => return HttpResponse::InternalServerError().json(err(format!("{e:#}"))),
    };
    let mut out = serde_json::Map::new();
    for record in records.iter().filter(|r| r.status == "active") {
        let result = wallets::sync_wallet(&state.db, &state.wallets, &record.pubkey).await;
        let entry = match result {
            Ok(report) => serde_json::json!({ "ok": report }),
            Err(e) => serde_json::json!({ "error": format!("{e:#}") }),
        };
        out.insert(record.pubkey.clone(), entry);
    }
    HttpResponse::Ok().json(out)
}

pub async fn nonces(state: web::Data<Arc<DeployState>>, path: web::Path<String>) -> impl Responder {
    match wallets::list_nonces(&state.db, &state.wallets, &path.into_inner()).await {
        Ok(rows) => HttpResponse::Ok().json(rows),
        Err(e) => HttpResponse::BadRequest().json(err(format!("{e:#}"))),
    }
}

pub async fn activity(state: web::Data<Arc<DeployState>>, path: web::Path<String>) -> impl Responder {
    let repo = TraderWalletRepo::new(state.db.clone());
    match repo.activity(&path.into_inner(), ACTIVITY_LIMIT).await {
        Ok(rows) => HttpResponse::Ok().json(rows),
        Err(e) => HttpResponse::InternalServerError().json(err(format!("{e:#}"))),
    }
}

pub async fn send_sol(
    state: web::Data<Arc<DeployState>>,
    path: web::Path<String>,
    body: web::Json<SendBody>,
) -> impl Responder {
    let lamports = sol_to_lamports(body.sol);
    if !body.sol.is_finite() || lamports <= 0 {
        return HttpResponse::BadRequest().json(err("amount must be above zero"));
    }
    let from = path.into_inner();
    match wallets::send_sol(&state.db, &state.wallets, &from, &body.to, lamports as u64, body.external).await {
        Ok(signature) => HttpResponse::Ok().json(serde_json::json!({ "signature": signature })),
        Err(e) => HttpResponse::BadRequest().json(err(format!("{e:#}"))),
    }
}

pub async fn rename(
    state: web::Data<Arc<DeployState>>,
    path: web::Path<String>,
    body: web::Json<RenameBody>,
) -> impl Responder {
    match wallets::rename(&state.db, &state.wallets, &path.into_inner(), body.name.trim()).await {
        Ok(()) => HttpResponse::Ok().json(serde_json::json!({ "name": body.name.trim() })),
        Err(e) => HttpResponse::BadRequest().json(err(format!("{e:#}"))),
    }
}

/// Start the wallet's trader again on the nonce set it runs, after an init
/// that failed on something passing (an RPC error at boot).
pub async fn restart(state: web::Data<Arc<DeployState>>, path: web::Path<String>) -> impl Responder {
    let pubkey = path.into_inner();
    let Some(nonces) = state.wallets.nonces(&pubkey) else {
        return HttpResponse::BadRequest().json(err("wallet is not loaded"));
    };
    if let Err(e) = state.wallets.rebuild(&pubkey, nonces).await {
        return HttpResponse::BadRequest().json(err(format!("{e:#}")));
    }
    let init_error = state.wallets.init_error(&pubkey);
    let repo = TraderWalletRepo::new(state.db.clone());
    let _ = repo
        .log_activity(&pubkey, "restarted", serde_json::json!({ "init_error": init_error }), &[])
        .await;
    HttpResponse::Ok().json(serde_json::json!({ "init_error": init_error }))
}

pub async fn archive(state: web::Data<Arc<DeployState>>, path: web::Path<String>) -> impl Responder {
    match wallets::archive(&state.db, &state.wallets, &path.into_inner()).await {
        Ok(()) => HttpResponse::Ok().json(serde_json::json!({ "archived": true })),
        Err(e) => HttpResponse::BadRequest().json(err(format!("{e:#}"))),
    }
}

pub async fn restore(state: web::Data<Arc<DeployState>>, path: web::Path<String>) -> impl Responder {
    match wallets::restore(&state.db, &state.wallets, &path.into_inner()).await {
        Ok(()) => HttpResponse::Ok().json(serde_json::json!({ "restored": true })),
        Err(e) => HttpResponse::BadRequest().json(err(format!("{e:#}"))),
    }
}

fn err(error: impl Into<String>) -> serde_json::Value {
    serde_json::json!({ "error": error.into() })
}
