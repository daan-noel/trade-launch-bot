pub mod cashback;
pub mod portfolio;
pub mod rent;
pub mod solana;

pub use cashback::*;
pub use portfolio::*;
pub use rent::*;
pub use solana::*;

use std::sync::Arc;

use actix_web::HttpResponse;
use serde::Deserialize;

use crate::state::deploy_state::DeployState;
use crate::trader::PumpFunTrader;

/// `?wallet=<pubkey>` on the wallet-scoped reads and actions. Absent = the primary.
#[derive(Debug, Default, Deserialize)]
pub struct WalletParam {
    #[serde(default)]
    pub wallet: Option<String>,
}

/// The trader `wallet` names, or a 400 the handler returns as is.
pub(crate) fn trader_or_400(
    state: &DeployState,
    wallet: Option<&str>,
) -> Result<Arc<PumpFunTrader>, HttpResponse> {
    state
        .trader_for(wallet)
        .map_err(|e| HttpResponse::BadRequest().json(serde_json::json!({ "error": e })))
}
