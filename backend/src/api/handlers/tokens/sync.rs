use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use futures_util::stream;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::{
    services::{
        helius_rpc::HeliusRpc,
        token_sync::{self, SyncCompleteEvent, TokenSyncContext, TokenSyncRequest},
    },
    state::app_state::AppState,
};

use super::tokens::TokenDetail;

#[derive(Deserialize)]
pub struct SyncTokenBody {
    pub mint_address: String,
    #[serde(default)]
    pub include_post_migrate: bool,
    #[serde(default)]
    pub incremental: bool,
}

#[derive(Serialize)]
struct ErrorBody {
    message: String,
}

#[derive(Serialize)]
struct StreamErrorEvent {
    #[serde(rename = "type")]
    event_type: &'static str,
    message: String,
}

/// `POST /api/token/sync` — NDJSON stream: progress lines, then `complete` or `error`.
pub async fn sync_token(
    state: web::Data<Arc<AppState>>,
    body: web::Json<SyncTokenBody>,
) -> impl Responder {
    let mint = body.mint_address.trim().to_string();

    if let Err(e) = token_sync::validate_mint_address(&mint) {
        return HttpResponse::BadRequest().json(ErrorBody {
            message: e.message().to_string(),
        });
    }

    let rpc = HeliusRpc::new(state.helius_rpc_url.clone());
    if let Err(e) = token_sync::preflight(&rpc, &mint, &state.pump_program_id).await {
        return HttpResponse::BadRequest().json(ErrorBody {
            message: e.message().to_string(),
        });
    }

    let (line_tx, line_rx) = mpsc::channel::<String>(128);

    let ctx = TokenSyncContext {
        db: state.db.clone(),
        token_cache: state.token_cache.clone(),
        helius_rpc_url: state.helius_rpc_url.clone(),
        pump_program_id: state.pump_program_id.clone(),
        pool_index: state.pool_index.clone(),
        pools_changed: state.pools_changed.clone(),
    };

    let req = TokenSyncRequest {
        mint_address: mint,
        include_post_migrate: body.include_post_migrate,
        incremental: body.incremental,
    };

    tokio::spawn(async move {
        match token_sync::run_token_sync(ctx, req, line_tx.clone()).await {
            Ok(output) => {
                let detail = TokenDetail::from(&output.state);
                let complete = serde_json::to_string(&SyncCompleteEvent {
                    event_type: "complete",
                    token: serde_json::to_value(detail).unwrap_or_default(),
                    trades: serde_json::to_value(&output.trades).unwrap_or_default(),
                })
                .unwrap_or_else(|_| {
                    r#"{"type":"error","message":"Failed to serialize response"}"#.to_string()
                });
                let _ = line_tx.send(complete + "\n").await;
            }
            Err(e) => {
                let line = serde_json::to_string(&StreamErrorEvent {
                    event_type: "error",
                    message: e.message().to_string(),
                })
                .unwrap_or_default()
                    + "\n";
                let _ = line_tx.send(line).await;
            }
        }
    });

    let body_stream = stream::unfold(line_rx, |mut rx| async {
        match rx.recv().await {
            Some(line) => Some((Ok::<_, actix_web::Error>(web::Bytes::from(line)), rx)),
            None => None,
        }
    });

    HttpResponse::Ok()
        .content_type("application/x-ndjson")
        .streaming(body_stream)
}

/// `POST /api/token/sync/preview` — estimate how many transactions a sync would
/// download for a mint (signatures only, no tx downloads). Returns `SyncPreview`
/// with new (Fetch New) and total (Fetch All) counts.
pub async fn preview_sync(
    state: web::Data<Arc<AppState>>,
    body: web::Json<SyncTokenBody>,
) -> impl Responder {
    let mint = body.mint_address.trim().to_string();

    if let Err(e) = token_sync::validate_mint_address(&mint) {
        return HttpResponse::BadRequest().json(ErrorBody {
            message: e.message().to_string(),
        });
    }

    let ctx = TokenSyncContext {
        db: state.db.clone(),
        token_cache: state.token_cache.clone(),
        helius_rpc_url: state.helius_rpc_url.clone(),
        pump_program_id: state.pump_program_id.clone(),
        pool_index: state.pool_index.clone(),
        pools_changed: state.pools_changed.clone(),
    };

    let req = TokenSyncRequest {
        mint_address: mint,
        include_post_migrate: body.include_post_migrate,
        incremental: body.incremental,
    };

    match token_sync::preview_sync(ctx, req).await {
        Ok(preview) => HttpResponse::Ok().json(preview),
        Err(e) => HttpResponse::BadRequest().json(ErrorBody {
            message: e.message().to_string(),
        }),
    }
}
