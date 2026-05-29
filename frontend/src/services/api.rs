#[derive(Clone, PartialEq, Deserialize)]
pub struct RulePositionRecord {
    pub id: String,
    pub mint: String,
    pub wallet: String,
    pub entry_price: f64,
    pub exit_price: Option<f64>,
    pub entry_tx: String,
    pub exit_tx: Option<String>,
    pub status: String,
    pub strategy: String,
    pub rule_id: String,
    pub entry_amount: f64,
    pub exit_amount: Option<f64>,
    pub pnl_percent: Option<f64>,
    pub entry_time: Option<String>,
    pub exit_time: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Fetch all positions for a TPSL rule (by rule_id)
pub async fn fetch_rule_positions(rule_id: &str) -> Result<Vec<RulePositionRecord>, String> {
    let url = format!("{API_BASE}/api/strategies/tpsl/rules/{rule_id}/positions");
    let resp = Request::get(&url)
        .send()
        .await
        .map_err(|e| format!("Fetch error: {e}"))?;
    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }
    resp.json::<Vec<RulePositionRecord>>()
        .await
        .map_err(|e| format!("Parse error: {e}"))
}
use gloo_net::http::Request;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// Build-time config injected from `frontend/build.rs`.
include!(concat!(env!("OUT_DIR"), "/env_config.rs"));

// ── Token record — mirrors backend `TokenSummary` ─────────────────────────────

#[derive(Clone, PartialEq, Deserialize)]
pub struct TokenRecord {
    pub mint_address: String,
    pub name: String,
    pub symbol: String,
    pub creator_address: String,
    pub trade_count: u64,
    pub current_price: Option<f64>,
    pub volume_sol_total: f64,
    pub ath_price: Option<f64>,
    pub ath_timestamp: Option<String>,
    pub market_cap: Option<f64>,
    pub initial_buy_sol: Option<f64>,
    pub initial_supply_token: Option<u64>,
    pub token_amount: Option<u64>,
    pub max_sol_cost: Option<u64>,
    pub spendable_sol_in: Option<u64>,
    pub min_tokens_out: Option<u64>,
    pub cu_limit: Option<u64>,
    pub cu_price: Option<u64>,
    pub ix_labels_count: usize,
    #[serde(default)]
    pub instruction_labels: serde_json::Value,
    pub is_migrated: bool,
    pub is_mayhem_mode: bool,
    pub age: i64,
    pub created_at: String,
    pub create_tx_address: String,
    pub last_trade_at: Option<String>,
}

/// Result returned by [`fetch_tokens`].
#[derive(Deserialize)]
pub struct FetchTokensResult {
    pub total: usize,
    pub items: Vec<TokenRecord>,
}

/// Fetch a page of tokens from `GET /api/tokens`.
/// Filtering and pagination are handled server-side.
pub async fn fetch_tokens(
    search: &str,
    limit: i64,
    offset: i64,
) -> Result<FetchTokensResult, String> {
    let mut url = format!("{API_BASE}/api/tokens?limit={limit}&offset={offset}");
    if !search.is_empty() {
        url.push_str(&format!("&search={search}"));
    }
    let resp = Request::get(&url)
        .send()
        .await
        .map_err(|e| format!("Fetch error: {e}"))?;
    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }
    resp.json::<FetchTokensResult>()
        .await
        .map_err(|e| format!("Parse error: {e}"))
}

#[derive(Clone, PartialEq, Deserialize)]
pub struct TokenDetailRecord {
    pub mint_address: String,
    pub name: String,
    pub symbol: String,
    pub creator_address: String,
    pub bonding_curve_address: Option<String>,
    pub initial_supply_token: Option<u64>,
    pub initial_buy_sol: Option<f64>,
    pub token_amount: Option<u64>,
    pub max_sol_cost: Option<u64>,
    pub spendable_sol_in: Option<u64>,
    pub min_tokens_out: Option<u64>,
    pub cu_limit: Option<u64>,
    pub cu_price: Option<u64>,
    pub instruction_labels: Value,
    pub is_mayhem_mode: bool,
    pub create_tx_address: String,
    pub created_at: String,
    pub trade_count: Option<u64>,
    pub volume_sol_total: Option<f64>,
    pub market_cap: Option<f64>,
    pub current_price: Option<f64>,
    pub ath_price: Option<f64>,
    pub ath_timestamp: Option<String>,
    pub is_migrated: bool,
    pub unique_wallets_in_window: Option<usize>,
    pub last_trade_at: Option<String>,
}

pub async fn fetch_token_detail(mint: &str) -> Result<TokenDetailRecord, String> {
    let url = format!("{API_BASE}/api/tokens/{mint}");
    let resp = Request::get(&url)
        .send()
        .await
        .map_err(|e| format!("Fetch error: {e}"))?;
    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }
    resp.json::<TokenDetailRecord>()
        .await
        .map_err(|e| format!("Parse error: {e}"))
}

#[derive(Clone, PartialEq, Deserialize)]
pub struct RuleRecord {
    pub id: String,
    pub rule_name: String,
    pub p_initial_buy_sol: Option<f64>,
    pub p_cu_limit: Option<u64>,
    pub p_cu_price: Option<u64>,
    pub p_max_sol_cost: Option<f64>,
    pub p_spendable_sol_in: Option<f64>,
    pub p_max_holding_tokens: Option<u64>,
    pub p_total_max_trade_tokens: Option<u64>,
    pub p_ix_labels: Value,
    pub trade_mode: String,
    pub buy_amount: f64,
    pub take_profit: f64,
    pub stop_loss: f64,
    pub tolerance_pct: f64,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Serialize)]
pub struct CreateRuleRequest {
    pub rule_name: String,
    pub p_initial_buy_sol: Option<f64>,
    pub p_cu_limit: Option<u64>,
    pub p_cu_price: Option<u64>,
    pub p_max_sol_cost: Option<f64>,
    pub p_spendable_sol_in: Option<f64>,
    pub p_max_holding_tokens: Option<u64>,
    pub p_total_max_trade_tokens: Option<u64>,
    pub p_ix_labels: Value,
    pub trade_mode: String,
    pub buy_amount: f64,
    pub take_profit: f64,
    pub stop_loss: f64,
    pub tolerance_pct: Option<f64>,
}

#[derive(Clone, PartialEq, Deserialize)]
pub struct SimulatedTokenResult {
    pub mint: String,
    pub symbol: String,
    pub entry_price: f64,
    pub entry_amount: f64,
    pub entry_tx: String,
    pub entry_time: String,
    pub exit_price: Option<f64>,
    pub exit_tx: Option<String>,
    pub exit_time: Option<String>,
    pub holding_secs: Option<i64>,
    pub pnl_percent: Option<f64>,
    pub pnl_sol: Option<f64>,
    /// "TakeProfit", "StopLoss", or "Open"
    pub exit_reason: String,
    pub total_trades: usize,
}

#[derive(Clone, PartialEq, Deserialize)]
pub struct LiveModeResponse {
    pub live: bool,
}

#[derive(Serialize)]
pub struct UpdateLiveModeRequest {
    pub live: bool,
}

#[derive(Clone, PartialEq, Deserialize)]
pub struct SolPriceResponse {
    pub usd_rate: Option<f64>,
}

pub async fn fetch_sol_price() -> Result<Option<f64>, String> {
    let url = format!("{API_BASE}/api/system/price");
    let resp = Request::get(&url)
        .send()
        .await
        .map_err(|e| format!("Fetch error: {e}"))?;
    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let result = resp
        .json::<SolPriceResponse>()
        .await
        .map_err(|e| format!("Parse error: {e}"))?;
    Ok(result.usd_rate)
}

pub async fn fetch_live_mode() -> Result<bool, String> {
    let url = format!("{API_BASE}/api/system/live");
    let resp = Request::get(&url)
        .send()
        .await
        .map_err(|e| format!("Fetch error: {e}"))?;
    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let result = resp
        .json::<LiveModeResponse>()
        .await
        .map_err(|e| format!("Parse error: {e}"))?;
    Ok(result.live)
}

pub async fn set_live_mode(live: bool) -> Result<bool, String> {
    let url = format!("{API_BASE}/api/system/live");
    let request = Request::put(&url)
        .json(&UpdateLiveModeRequest { live })
        .map_err(|e| format!("Request error: {e}"))?;
    let resp = request
        .send()
        .await
        .map_err(|e| format!("Request error: {e}"))?;
    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let result = resp
        .json::<LiveModeResponse>()
        .await
        .map_err(|e| format!("Parse error: {e}"))?;
    Ok(result.live)
}

pub async fn fetch_tpsl_rules() -> Result<Vec<RuleRecord>, String> {
    let url = format!("{API_BASE}/api/strategies/tpsl/rules");
    let resp = Request::get(&url)
        .send()
        .await
        .map_err(|e| format!("Fetch error: {e}"))?;
    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }
    resp.json::<Vec<RuleRecord>>()
        .await
        .map_err(|e| format!("Parse error: {e}"))
}

pub async fn create_tpsl_rule(req: &CreateRuleRequest) -> Result<RuleRecord, String> {
    let url = format!("{API_BASE}/api/strategies/tpsl/rules");
    let request = Request::post(&url)
        .json(req)
        .map_err(|e| format!("Create error: {e}"))?;
    let resp = request
        .send()
        .await
        .map_err(|e| format!("Create error: {e}"))?;
    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }
    resp.json::<RuleRecord>()
        .await
        .map_err(|e| format!("Parse error: {e}"))
}

#[derive(Clone, PartialEq, Deserialize)]
pub struct MatchedTokenRecord {
    pub mint: String,
    pub symbol: String,
    pub name: String,
    pub created_at: String,
    pub initial_buy_sol: Option<f64>,
    pub cu_limit: Option<u64>,
    pub cu_price: Option<u64>,
}

pub async fn fetch_matched_tokens(rule_id: &str) -> Result<Vec<MatchedTokenRecord>, String> {
    let url = format!("{API_BASE}/api/strategies/tpsl/rules/{rule_id}/matched");
    let resp = Request::get(&url)
        .send()
        .await
        .map_err(|e| format!("Fetch error: {e}"))?;
    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }
    resp.json::<Vec<MatchedTokenRecord>>()
        .await
        .map_err(|e| format!("Parse error: {e}"))
}

pub async fn simulate_tpsl_rule(rule_id: &str) -> Result<Vec<SimulatedTokenResult>, String> {
    let url = format!("{API_BASE}/api/strategies/tpsl/rules/{rule_id}/simulate");
    let resp = Request::get(&url)
        .send()
        .await
        .map_err(|e| format!("Fetch error: {e}"))?;
    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }
    resp.json::<Vec<SimulatedTokenResult>>()
        .await
        .map_err(|e| format!("Parse error: {e}"))
}

#[derive(Serialize)]
pub struct UpdateRuleRequest {
    pub rule_name: Option<String>,
    pub buy_amount: Option<f64>,
    pub take_profit: Option<f64>,
    pub stop_loss: Option<f64>,
    pub p_initial_buy_sol: Option<Option<f64>>,
    pub p_cu_limit: Option<Option<u64>>,
    pub p_cu_price: Option<Option<u64>>,
    pub p_ix_labels: Option<Option<Value>>,
    pub p_max_sol_cost: Option<Option<f64>>,
    pub p_spendable_sol_in: Option<Option<f64>>,
    // Outer Option = field present; inner Option = value or explicit null
    pub p_max_holding_tokens: Option<Option<u64>>,
    pub p_total_max_trade_tokens: Option<Option<u64>>,
    pub tolerance_pct: Option<f64>,
    pub is_active: Option<bool>,
    pub trade_mode: Option<String>,
}

pub async fn update_tpsl_rule(
    rule_id: &str,
    req: &UpdateRuleRequest,
) -> Result<RuleRecord, String> {
    let url = format!("{API_BASE}/api/strategies/tpsl/rules/{rule_id}");
    let request = Request::put(&url)
        .json(req)
        .map_err(|e| format!("Update error: {e}"))?;
    let resp = request
        .send()
        .await
        .map_err(|e| format!("Update error: {e}"))?;
    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }
    resp.json::<RuleRecord>()
        .await
        .map_err(|e| format!("Parse error: {e}"))
}

pub async fn delete_tpsl_rule(rule_id: &str) -> Result<(), String> {
    let url = format!("{API_BASE}/api/strategies/tpsl/rules/{rule_id}");
    let resp = Request::delete(&url)
        .send()
        .await
        .map_err(|e| format!("Delete error: {e}"))?;
    if resp.ok() || resp.status() == 204 {
        Ok(())
    } else {
        Err(format!("HTTP {}", resp.status()))
    }
}

// ── Analysis records ─────────────────────────────────────────────────────────

#[derive(Clone, PartialEq, Deserialize)]
pub struct AnalysisRecord {
    pub analyzer_name: String,
    pub score: f64,
    pub indicators: Vec<String>,
    pub computed_at: String,
}

#[derive(Deserialize)]
pub struct AnalysisListResult {
    pub total: i64,
    pub items: Vec<AnalysisRecord>,
}

pub async fn fetch_analysis(limit: i64, offset: i64) -> Result<AnalysisListResult, String> {
    let url = format!("{API_BASE}/api/analysis?limit={limit}&offset={offset}");
    let resp = Request::get(&url)
        .send()
        .await
        .map_err(|e| format!("Fetch error: {e}"))?;
    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }
    resp.json::<AnalysisListResult>()
        .await
        .map_err(|e| format!("Parse error: {e}"))
}

// ── Creator profiles ─────────────────────────────────────────────────────────

#[derive(Clone, PartialEq, Deserialize)]
pub struct CreatorRecord {
    pub wallet_address: String,
    pub tokens_created: i32,
    pub total_volume_sol: f64,
    pub suspiciousness_score: f64,
    pub wash_trade_score: f64,
    pub last_analyzed_at: Option<String>,
}

#[derive(Deserialize)]
pub struct CreatorListResult {
    pub total: i64,
    pub items: Vec<CreatorRecord>,
}

pub async fn fetch_creators(limit: i64, offset: i64) -> Result<CreatorListResult, String> {
    let url = format!("{API_BASE}/api/creators?limit={limit}&offset={offset}");
    let resp = Request::get(&url)
        .send()
        .await
        .map_err(|e| format!("Fetch error: {e}"))?;
    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }
    resp.json::<CreatorListResult>()
        .await
        .map_err(|e| format!("Parse error: {e}"))
}
