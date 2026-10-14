//! Wallet registry. Responses carry pubkeys and names. The private key is
//! encrypted on import and is not returned.

use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use serde::Deserialize;
use solana_sdk::signature::Signer;
use uuid::Uuid;

use crate::state::deploy_state::DeployState;
use crate::wallets::{self, key_ref_for, parse_secret, valid_name, write_keypair};
use trading_core::storage::repositories::trader_wallet_repo::TraderWalletRepo;

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

pub async fn list(state: web::Data<Arc<DeployState>>) -> impl Responder {
    let repo = TraderWalletRepo::new(state.db.clone());
    let wallets = match repo.list().await {
        Ok(rows) => rows,
        Err(e) => return HttpResponse::InternalServerError().json(err(e.to_string())),
    };
    let mut out = Vec::with_capacity(wallets.len());
    for wallet in wallets {
        let sol = match state.wallets.trader(&wallet.pubkey) {
            Some(trader) => trader.get_sol_balance().await.ok().map(|l| l as f64 / 1_000_000_000.0),
            None => None,
        };
        out.push(serde_json::json!({
            "pubkey": wallet.pubkey,
            "name": wallet.name,
            "status": wallet.status,
            "is_primary": wallet.is_primary,
            "nonce_count": wallet.nonce_count,
            "rule_count": wallet.rule_count,
            "open_positions": wallet.open_positions,
            "sol": sol,
            "key_loaded": state.wallets.signer(&wallet.pubkey).map(|k| k.pubkey().to_string()) == Some(wallet.pubkey.clone()),
            "created_at": wallet.created_at,
            "archived_at": wallet.archived_at,
        }));
    }
    let open = repo.open_move().await.ok().flatten();
    HttpResponse::Ok().json(serde_json::json!({ "wallets": out, "move": open }))
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
    let keypair = solana_sdk::signature::Keypair::new();
    let secret = keypair.to_bytes();
    let key_ref = key_ref_for(&body.name);
    if let Err(e) = write_keypair(&dir, &key_ref, &secret, kek) {
        return HttpResponse::InternalServerError().json(err(e.to_string()));
    }
    let pubkey = keypair.pubkey().to_string();
    let repo = TraderWalletRepo::new(state.db.clone());
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
    let key_ref = key_ref_for(&body.name);
    if let Err(e) = write_keypair(&dir, &key_ref, &secret, kek) {
        return HttpResponse::InternalServerError().json(err(e.to_string()));
    }
    let repo = TraderWalletRepo::new(state.db.clone());
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

pub async fn set_primary(
    state: web::Data<Arc<DeployState>>,
    path: web::Path<String>,
) -> impl Responder {
    let pubkey = path.into_inner();
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

fn err(error: impl Into<String>) -> serde_json::Value {
    serde_json::json!({ "error": error.into() })
}
