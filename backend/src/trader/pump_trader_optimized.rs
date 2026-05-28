// ============================================================
// pump_trader_optimized.rs
//
// Design goals:
//  - Zero-copy nonce acquisition: multi-slot hash cache with
//    in_use flags, no synchronous RPC on the hot path.
//  - Pre-built seed pool (buy templates) with async replenishment
//    so account creation instructions are always ready.
//  - Pre-built Jito tip and compute-budget instructions, built
//    once at init and reused every transaction.
//  - Retry loop on sell (up to 5 attempts, fresh nonce each time).
//  - Background prebuild of the *next* buy template immediately
//    after a buy succeeds or fails (like file 2), on top of the
//    pool refill (like file 1) — double safety net.
//  - base64 encoding (correct for the JSON-RPC "base64" param).
//  - Confirmation polling extracted into a reusable helper with
//    configurable retries; sell retries re-use it too.
// ============================================================

use crate::constants::{
    ASSOCIATED_TOKEN_PROGRAM_ID, EVENT_AUTHORITY, FEE_PROGRAM_ID, LAMPORTS_PER_SOL,
    PUMP_FUN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID, TOKEN_PROGRAM_ID,
};
use anyhow::{Context, Result};
use base64::{engine::general_purpose, Engine as _};
use rand::seq::SliceRandom;
use serde_json::json;
use solana_client::{nonce_utils, rpc_client::RpcClient};
use solana_sdk::{
    commitment_config::CommitmentConfig,
    compute_budget::ComputeBudgetInstruction,
    hash::Hash,
    instruction::{AccountMeta, Instruction},
    message::Message,
    nonce::State,
    pubkey::Pubkey,
    signature::{Keypair, Signature, Signer},
    transaction::Transaction,
};
use solana_system_interface::instruction as system_instruction;
use solana_system_interface::program as system_program;
use std::collections::{HashMap, HashSet};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash as StdHash, Hasher};
use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use tracing::{error, info, warn};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const JITO_TIP_ACCOUNTS: &[&str] = &[
    "9bnz4RShgq1hAnLnZbP8kbgBg1kEmcJBYQq3gQbmnSta",
    "4TQLFNWK8AovT1gFvda5jfw2oJeRMKEmw7aH6MGBJ3or",
    "2nyhqdwKcJZR2vcqCyrYsaPVdAnFoJjiksCXJ7hfEYgD",
    "wyvPkWjVZz1M8fHQnMMCDTQDbkManefNNhweYk5WkcF",
    "D2L6yPZ2FmmmTKPgzaMKdhu6EWZcTpLy1Vhx8uvZe7NZ",
    "3KCKozbAaF75qEU33jtzozcJ29yJuaLJTy2jFdzUY8bT",
    "2q5pghRs6arqVjRvT5gfgWfWcHWmw1ZuCzphgd5KfWGJ",
    "5VY91ws6B2hMmBFRsXkoAAdsPHBJwRfBht4DXox3xkwn",
    "4vieeGHPYPG2MmyPRcYjdiDmmhN3ww7hsFNap8pVN3Ey",
    "D1Mc6j9xQWgR1o1Z7yU5nVVXFQiAYx7FG9AW1aVfwrUM",
    "4ACfpUFoaSD9bfPdeu6DBt89gB6ENTeHBXCAi87NhDEE",
];

/// Minimum Jito tip in SOL.
const MIN_JITO_TIP_SOL: f64 = 0.0002;

/// How many times sell_token will retry before giving up.
const MAX_SELL_ATTEMPTS: usize = 5;

/// How many times we poll for transaction confirmation.
const CONFIRM_MAX_RETRIES: usize = 5;

/// Milliseconds between confirmation polls.
const CONFIRM_POLL_MS: u64 = 1_000;

/// Maximum spin-wait iterations when all nonce slots are in use.
const NONCE_MAX_WAIT_ITERS: usize = 200;

/// Sleep between nonce spin-wait iterations.
const NONCE_WAIT_SLEEP_MS: u64 = 20;

const PUMP_PROGRAM_UPGRADE_FEE_RECIPIENT: &str =
    "5YxQFdt3Tr9zJLvkFccqXVUwhdTWJQc1fFg2YPbxvxeD";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Pre-built account-creation instruction + the resulting token account address.
#[derive(Debug, Clone)]
struct BuyTemplate {
    create_with_seed_ix: Instruction,
    user_token_account: Pubkey,
}

/// Per-nonce-account slot held in the hash cache.
#[derive(Debug)]
struct NonceSlot {
    /// Most recently fetched blockhash for this nonce account.
    cached_hash: Option<Hash>,
    /// True while a transaction is in-flight using this slot.
    in_use: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct TokenPDAs {
    pub token_program: Pubkey,
    pub bonding_curve: Pubkey,
    pub bonding_curve_v2: Pubkey,
    pub associated_bonding_curve: Pubkey,
    pub creator_vault: Pubkey,
}

#[derive(Debug, Clone)]
pub struct GlobalAccount {
    pub global_pda: Pubkey,
    pub fee_recipient: Pubkey,
    pub global_volume_accumulator: Pubkey,
    pub user_volume_accumulator: Pubkey,
    pub fee_config: Pubkey,
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct TraderConfig {
    pub rpc_url: String,
    pub helius_sender_url: String,
    pub keypair: Keypair,
    pub nonce_accounts: Vec<String>,
    /// Micro-lamports per compute unit for priority fee.
    pub priority_fee_lamports: u64,
    /// How many buy templates to keep pre-built per token-program pool.
    pub buy_seed_pool_size: usize,
}

// ---------------------------------------------------------------------------
// Trader
// ---------------------------------------------------------------------------

pub struct PumpFunTrader {
    config: Arc<TraderConfig>,
    http: reqwest::Client,
    rpc: Arc<RpcClient>,

    // Set once in initialize()
    global_account: Option<GlobalAccount>,
    compute_budget_ixs: Vec<Instruction>,
    jito_tip_ix: Arc<Mutex<Option<Instruction>>>,

    // Nonce management
    nonce_pubkeys: Vec<Pubkey>,
    nonce_cursor: AtomicUsize,
    nonce_slots: Arc<Mutex<HashMap<Pubkey, NonceSlot>>>,

    // Diagnostic counters
    nonce_wait_events: AtomicUsize,
    nonce_wait_iters_total: AtomicUsize,

    // Per token-program buy template pools
    buy_pool_legacy: Arc<Mutex<Vec<BuyTemplate>>>,
    buy_pool_2022: Arc<Mutex<Vec<BuyTemplate>>>,
    buy_pool_misses_legacy: AtomicUsize,
    buy_pool_misses_2022: AtomicUsize,
    seed_counter: AtomicUsize,

    // Rent values (fetched once)
    token_account_space: u64,
    token_account_rent: u64,
    token_2022_account_space: u64,
    token_2022_account_rent: u64,

    // Static program IDs (parsed once)
    pump_program: Pubkey,
    system_program: Pubkey,
    assoc_token_program: Pubkey,
    event_authority: Pubkey,
    fee_program: Pubkey,
    upgrade_fee_recipient: Pubkey,

    // Per-token caches
    user_token_accounts: Arc<Mutex<HashMap<String, Pubkey>>>,
    token_pdas: Arc<Mutex<HashMap<String, TokenPDAs>>>,
}

impl PumpFunTrader {
    // -----------------------------------------------------------------------
    // Construction
    // -----------------------------------------------------------------------

    pub fn new(config: Arc<TraderConfig>) -> Self {
        let rpc = Arc::new(RpcClient::new_with_commitment(
            config.rpc_url.clone(),
            CommitmentConfig::confirmed(),
        ));

        Self {
            config,
            http: reqwest::Client::new(),
            rpc,
            global_account: None,
            compute_budget_ixs: Vec::new(),
            jito_tip_ix: Arc::new(Mutex::new(None)),
            nonce_pubkeys: Vec::new(),
            nonce_cursor: AtomicUsize::new(0),
            nonce_slots: Arc::new(Mutex::new(HashMap::new())),
            nonce_wait_events: AtomicUsize::new(0),
            nonce_wait_iters_total: AtomicUsize::new(0),
            buy_pool_legacy: Arc::new(Mutex::new(Vec::new())),
            buy_pool_2022: Arc::new(Mutex::new(Vec::new())),
            buy_pool_misses_legacy: AtomicUsize::new(0),
            buy_pool_misses_2022: AtomicUsize::new(0),
            seed_counter: AtomicUsize::new(0),
            token_account_space: 165,
            token_account_rent: 2_000_000,
            token_2022_account_space: 182,
            token_2022_account_rent: 2_000_000,
            pump_program: Pubkey::from_str(PUMP_FUN_PROGRAM_ID).unwrap(),
            system_program: system_program::id(),
            assoc_token_program: Pubkey::from_str(ASSOCIATED_TOKEN_PROGRAM_ID).unwrap(),
            event_authority: Pubkey::from_str(EVENT_AUTHORITY).unwrap(),
            fee_program: Pubkey::from_str(FEE_PROGRAM_ID).unwrap(),
            upgrade_fee_recipient: Pubkey::from_str(PUMP_PROGRAM_UPGRADE_FEE_RECIPIENT).unwrap(),
            user_token_accounts: Arc::new(Mutex::new(HashMap::new())),
            token_pdas: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    // -----------------------------------------------------------------------
    // Initialization  (call once before any buy/sell)
    // -----------------------------------------------------------------------

    pub async fn initialize(&mut self) -> Result<()> {
        info!("🔧 Initializing PumpFunTrader...");

        // 1. Global account
        self.global_account = Some(self.fetch_global_account().await?);
        info!("✅ Global account initialized");

        // 2. Rent exemption amounts (one RPC call each)
        self.token_account_rent = self
            .rpc
            .get_minimum_balance_for_rent_exemption(self.token_account_space as usize)
            .context("Failed to get rent for token account")?;
        self.token_2022_account_rent = self
            .rpc
            .get_minimum_balance_for_rent_exemption(self.token_2022_account_space as usize)
            .context("Failed to get rent for token-2022 account")?;

        // 3. Compute budget instructions — built once, cloned per tx
        self.compute_budget_ixs = vec![
            ComputeBudgetInstruction::set_compute_unit_limit(200_000),
            ComputeBudgetInstruction::set_compute_unit_price(self.config.priority_fee_lamports),
        ];
        info!(
            "⚡ Priority fee: {} lamports/cu",
            self.config.priority_fee_lamports
        );

        // 4. Jito tip instruction — built once, reused every tx
        let jito_lamports = (MIN_JITO_TIP_SOL * LAMPORTS_PER_SOL as f64) as u64;
        let tip_account = JITO_TIP_ACCOUNTS
            .choose(&mut rand::thread_rng())
            .context("No Jito tip accounts")?;
        *self.jito_tip_ix.lock().await = Some(system_instruction::transfer(
            &self.config.keypair.pubkey(),
            &Pubkey::from_str(tip_account)?,
            jito_lamports,
        ));
        info!("💸 Jito tip: {} lamports → {}", jito_lamports, tip_account);

        // 5. Parse & deduplicate nonce accounts
        self.collect_nonce_pubkeys()?;

        // 6. Pre-fetch all nonce hashes
        info!("🔧 Pre-fetching nonce hashes...");
        {
            let mut slots = self.nonce_slots.lock().await;
            for &pubkey in &self.nonce_pubkeys {
                let hash = self.fetch_nonce_hash_sync(&pubkey)?;
                slots.insert(pubkey, NonceSlot { cached_hash: Some(hash), in_use: false });
            }
        }
        info!(
            "✅ Nonce hashes cached for {} account(s)",
            self.nonce_pubkeys.len()
        );

        // 7. Pre-build buy seed pools for both token programs
        info!("🌱 Pre-building buy seed pools (target={})", self.config.buy_seed_pool_size);
        self.fill_buy_pool(TOKEN_PROGRAM_ID).await?;
        self.fill_buy_pool(TOKEN_2022_PROGRAM_ID).await?;
        info!("✅ Buy seed pools ready");

        info!(
            "🚀 PumpFunTrader ready — wallet: {}",
            self.config.keypair.pubkey()
        );
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Buy
    // -----------------------------------------------------------------------

    pub async fn buy_token(
        &self,
        token_mint: &str,
        creator: &str,
        token_program_id: &str,
        sol_amount: f64,
    ) -> Result<bool> {
        let t0 = Instant::now();
        let buy_lamports = (sol_amount * LAMPORTS_PER_SOL as f64) as u64;
        let keypair = &self.config.keypair;

        // ── Acquire nonce (non-blocking hot path) ──────────────────────────
        let (nonce_pubkey, nonce_hash) = self.acquire_nonce().await?;

        let result: Result<bool> = async {
            let global = self.global_account.as_ref().context("Not initialized")?;

            // Parse addresses
            let mint = Pubkey::from_str(token_mint)?;
            let creator_pubkey = Pubkey::from_str(creator)?;
            let token_program = Pubkey::from_str(token_program_id)?;

            // Derive PDAs
            let (bonding_curve, _) = Pubkey::find_program_address(
                &[b"bonding-curve", mint.as_ref()],
                &self.pump_program,
            );
            let (bonding_curve_v2, _) = Pubkey::find_program_address(
                &[b"bonding-curve-v2", mint.as_ref()],
                &self.pump_program,
            );
            let (assoc_bonding_curve, _) = Pubkey::find_program_address(
                &[bonding_curve.as_ref(), token_program.as_ref(), mint.as_ref()],
                &self.assoc_token_program,
            );
            let (creator_vault, _) = Pubkey::find_program_address(
                &[b"creator-vault", creator_pubkey.as_ref()],
                &self.pump_program,
            );

            // Cache PDAs immediately — sell needs them
            self.token_pdas.lock().await.insert(
                token_mint.to_string(),
                TokenPDAs {
                    token_program,
                    bonding_curve,
                    bonding_curve_v2,
                    associated_bonding_curve: assoc_bonding_curve,
                    creator_vault,
                },
            );

            // ── Buy template from pool (zero alloc on hot path) ────────────
            let template = self.acquire_buy_template(token_program_id).await?;
            let user_token_account = template.user_token_account;

            // Cache user token account for sell
            self.user_token_accounts
                .lock()
                .await
                .insert(token_mint.to_string(), user_token_account);

            // ── Assemble instructions ───────────────────────────────────────
            let mut ixs = Vec::with_capacity(6);
            ixs.extend_from_slice(&self.compute_budget_ixs);
            ixs.push(template.create_with_seed_ix);

            // InitializeAccount3
            let init_ix = if token_program_id == TOKEN_PROGRAM_ID {
                spl_token::instruction::initialize_account3(
                    &token_program, &user_token_account, &mint, &keypair.pubkey(),
                )?
            } else {
                spl_token_2022::instruction::initialize_account3(
                    &token_program, &user_token_account, &mint, &keypair.pubkey(),
                )?
            };
            ixs.push(init_ix);

            // Buy_exact_sol_in
            let mut buy_data = vec![0x38, 0xfc, 0x74, 0x08, 0x9e, 0xdf, 0xcd, 0x5f];
            buy_data.extend_from_slice(&buy_lamports.to_le_bytes());
            buy_data.extend_from_slice(&1u64.to_le_bytes()); // min_token_amount = 1
            ixs.push(Instruction {
                program_id: self.pump_program,
                accounts: vec![
                    AccountMeta::new_readonly(global.global_pda, false),
                    AccountMeta::new(global.fee_recipient, false),
                    AccountMeta::new(mint, false),
                    AccountMeta::new(bonding_curve, false),
                    AccountMeta::new(assoc_bonding_curve, false),
                    AccountMeta::new(user_token_account, false),
                    AccountMeta::new(keypair.pubkey(), true),
                    AccountMeta::new_readonly(self.system_program, false),
                    AccountMeta::new_readonly(token_program, false),
                    AccountMeta::new(creator_vault, false),
                    AccountMeta::new_readonly(self.event_authority, false),
                    AccountMeta::new_readonly(self.pump_program, false),
                    AccountMeta::new(global.global_volume_accumulator, false),
                    AccountMeta::new(global.user_volume_accumulator, false),
                    AccountMeta::new_readonly(global.fee_config, false),
                    AccountMeta::new_readonly(self.fee_program, false),
                    AccountMeta::new_readonly(bonding_curve_v2, false),
                    AccountMeta::new(self.upgrade_fee_recipient, false), // April 28 upgrade
                ],
                data: buy_data,
            });

            // Jito tip (pre-built, just clone the ix)
            if let Some(tip) = self.jito_tip_ix.lock().await.clone() {
                ixs.push(tip);
            }

            // ── Sign & send ─────────────────────────────────────────────────
            let tx = self.build_nonce_tx(ixs, &nonce_pubkey, nonce_hash, keypair)?;
            let sig = self.send_transaction(&tx).await?;

            info!(
                "📤 Buy sent — sig: {} | SOL: {} | {}ms",
                sig, sol_amount, t0.elapsed().as_millis()
            );

            // ── Confirm ─────────────────────────────────────────────────────
            self.confirm_transaction(&sig, CONFIRM_MAX_RETRIES).await?;

            info!("✅ Buy confirmed — sig: {} | {}ms", sig, t0.elapsed().as_millis());
            Ok(true)
        }
        .await;

        // ── Always: release nonce, refill pool, prebuild next template ──────
        self.schedule_nonce_refresh(nonce_pubkey);
        self.replenish_pool_async(token_program_id);
        // Eagerly prebuild ONE more template in background (like file 2)
        self.prebuild_one_template_async(token_program_id);

        result
    }

    // -----------------------------------------------------------------------
    // Sell  (with retry loop)
    // -----------------------------------------------------------------------

    pub async fn sell_token(
        &self,
        token_mint: &str,
        token_amount: u64,
        creator_override: Option<&str>,
        is_cashback: bool,
    ) -> Result<bool> {
        let mut last_err: Option<anyhow::Error> = None;

        for attempt in 0..MAX_SELL_ATTEMPTS {
            let (nonce_pubkey, nonce_hash) = match self.acquire_nonce().await {
                Ok(v) => v,
                Err(e) => {
                    last_err = Some(e);
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                }
            };

            info!(
                "🔁 Sell attempt {}/{} — token: {} nonce: {}",
                attempt + 1, MAX_SELL_ATTEMPTS, token_mint, nonce_pubkey
            );

            let res = self
                .execute_sell(token_mint, token_amount, creator_override,
                              is_cashback, &nonce_pubkey, nonce_hash)
                .await;

            self.schedule_nonce_refresh(nonce_pubkey);

            match res {
                Ok(true) => return Ok(true),
                Ok(false) => {
                    let e = anyhow::anyhow!("Sell returned false on attempt {}", attempt + 1);
                    error!("❌ {}", e);
                    last_err = Some(e);
                }
                Err(e) => {
                    error!("❌ Sell attempt {} failed: {}", attempt + 1, e);
                    last_err = Some(e);
                }
            }

            if attempt < MAX_SELL_ATTEMPTS - 1 {
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        }

        Err(last_err.unwrap_or_else(|| {
            anyhow::anyhow!("Sell failed after {} attempts", MAX_SELL_ATTEMPTS)
        }))
    }

    // -----------------------------------------------------------------------
    // Sell inner
    // -----------------------------------------------------------------------

    async fn execute_sell(
        &self,
        token_mint: &str,
        token_amount: u64,
        creator_override: Option<&str>,
        is_cashback: bool,
        nonce_pubkey: &Pubkey,
        nonce_hash: Hash,
    ) -> Result<bool> {
        let t0 = Instant::now();
        let keypair = &self.config.keypair;
        let global = self.global_account.as_ref().context("Not initialized")?;

        let mint = Pubkey::from_str(token_mint)?;

        // Read cached PDAs (must exist from buy)
        let mut pdas = self
            .token_pdas
            .lock()
            .await
            .get(token_mint)
            .copied()
            .context("Token PDAs not cached — buy must precede sell")?;

        // Allow caller to override creator vault (e.g. after a creator update)
        if let Some(creator_str) = creator_override {
            let creator = Pubkey::from_str(creator_str)?;
            let (vault, _) = Pubkey::find_program_address(
                &[b"creator-vault", creator.as_ref()],
                &self.pump_program,
            );
            if pdas.creator_vault != vault {
                info!("🔄 Creator vault updated: {} → {}", pdas.creator_vault, vault);
                pdas.creator_vault = vault;
            }
        }

        // Read cached user token account (must exist from buy)
        let user_token_account = self
            .user_token_accounts
            .lock()
            .await
            .get(token_mint)
            .copied()
            .context("User token account not cached — buy must precede sell")?;

        // ── Assemble instructions ───────────────────────────────────────────
        let mut ixs = Vec::with_capacity(5);
        ixs.extend_from_slice(&self.compute_budget_ixs);

        // Sell (Sell exact token in)
        let mut sell_data = vec![0x33, 0xe6, 0x85, 0xa4, 0x01, 0x7f, 0x83, 0xad];
        sell_data.extend_from_slice(&token_amount.to_le_bytes());
        sell_data.extend_from_slice(&1u64.to_le_bytes()); // min_sol_output = 1

        let mut accounts = vec![
            AccountMeta::new_readonly(global.global_pda, false),
            AccountMeta::new(global.fee_recipient, false),
            AccountMeta::new(mint, false),
            AccountMeta::new(pdas.bonding_curve, false),
            AccountMeta::new(pdas.associated_bonding_curve, false),
            AccountMeta::new(user_token_account, false),
            AccountMeta::new(keypair.pubkey(), true),
            AccountMeta::new_readonly(self.system_program, false),
            AccountMeta::new(pdas.creator_vault, false),
            AccountMeta::new_readonly(pdas.token_program, false),
            AccountMeta::new_readonly(self.event_authority, false),
            AccountMeta::new_readonly(self.pump_program, false),
            AccountMeta::new_readonly(global.fee_config, false),
            AccountMeta::new_readonly(self.fee_program, false),
        ];

        // Cashback requires the user_volume_accumulator account
        if is_cashback {
            accounts.push(AccountMeta::new(global.user_volume_accumulator, false));
        }

        accounts.push(AccountMeta::new(pdas.bonding_curve_v2, false));
        accounts.push(AccountMeta::new(self.upgrade_fee_recipient, false));

        ixs.push(Instruction {
            program_id: self.pump_program,
            accounts,
            data: sell_data,
        });

        // Jito tip
        if let Some(tip) = self.jito_tip_ix.lock().await.clone() {
            ixs.push(tip);
        }

        // ── Sign & send ─────────────────────────────────────────────────────
        let tx = self.build_nonce_tx(ixs, nonce_pubkey, nonce_hash, keypair)?;
        let sig = self.send_transaction(&tx).await?;

        info!(
            "📤 Sell sent — sig: {} | amount: {} | {}ms",
            sig, token_amount, t0.elapsed().as_millis()
        );

        self.confirm_transaction(&sig, CONFIRM_MAX_RETRIES).await?;

        info!("✅ Sell confirmed — sig: {} | {}ms", sig, t0.elapsed().as_millis());
        Ok(true)
    }

    // -----------------------------------------------------------------------
    // Transaction helpers
    // -----------------------------------------------------------------------

    fn build_nonce_tx(
        &self,
        instructions: Vec<Instruction>,
        nonce_account: &Pubkey,
        nonce_hash: Hash,
        keypair: &Keypair,
    ) -> Result<Transaction> {
        let msg = Message::new_with_nonce(
            instructions,
            Some(&keypair.pubkey()),
            nonce_account,
            &keypair.pubkey(),
        );
        let mut tx = Transaction::new_unsigned(msg);
        tx.sign(&[keypair], nonce_hash);
        Ok(tx)
    }

    async fn send_transaction(&self, tx: &Transaction) -> Result<String> {
        let encoded = general_purpose::STANDARD.encode(bincode::serialize(tx)?);

        let body = json!({
            "jsonrpc": "2.0",
            "id": chrono::Utc::now().timestamp_millis().to_string(),
            "method": "sendTransaction",
            "params": [
                encoded,
                { "encoding": "base64", "skipPreflight": true, "maxRetries": 0 }
            ]
        });

        let resp = self
            .http
            .post(&self.config.helius_sender_url)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            anyhow::bail!("HTTP error from sender: {}", resp.text().await?);
        }

        let json: serde_json::Value = resp.json().await?;

        if let Some(err) = json.get("error") {
            anyhow::bail!("JSON-RPC error: {:?}", err);
        }

        json.get("result")
            .and_then(|r| r.as_str())
            .map(str::to_string)
            .context("No signature in response")
    }

    /// Poll the RPC until the transaction is confirmed or retries are exhausted.
    async fn confirm_transaction(&self, signature: &str, max_retries: usize) -> Result<()> {
        let sig = Signature::from_str(signature)?;

        for i in 0..max_retries {
            tokio::time::sleep(Duration::from_millis(CONFIRM_POLL_MS)).await;

            match self.rpc.get_signature_status(&sig)? {
                Some(Ok(())) => return Ok(()),
                Some(Err(e)) => anyhow::bail!("Transaction failed on-chain: {:?}", e),
                None => {
                    if i + 1 < max_retries {
                        info!("⏳ Confirmation pending ({}/{})", i + 1, max_retries);
                    }
                }
            }
        }

        anyhow::bail!("Confirmation timed out for {}", signature)
    }

    // -----------------------------------------------------------------------
    // Nonce management
    // -----------------------------------------------------------------------

    fn collect_nonce_pubkeys(&mut self) -> Result<()> {
        if self.config.nonce_accounts.is_empty() {
            anyhow::bail!("At least one nonce account is required");
        }
        if self.config.buy_seed_pool_size == 0 {
            anyhow::bail!("buy_seed_pool_size must be >= 1");
        }

        let mut seen = HashSet::new();
        self.nonce_pubkeys.clear();

        for raw in &self.config.nonce_accounts {
            if raw.is_empty() {
                anyhow::bail!("Nonce account string must not be empty");
            }
            let pk = Pubkey::from_str(raw)
                .with_context(|| format!("Invalid nonce pubkey: {}", raw))?;
            if seen.insert(pk) {
                self.nonce_pubkeys.push(pk);
            }
        }

        info!("✅ {} unique nonce account(s) configured", self.nonce_pubkeys.len());
        Ok(())
    }

    /// Acquire the next available nonce slot (round-robin, spin-wait if all busy).
    async fn acquire_nonce(&self) -> Result<(Pubkey, Hash)> {
        if self.nonce_pubkeys.is_empty() {
            anyhow::bail!("No nonce accounts configured");
        }

        let mut waited = 0usize;

        for _ in 0..NONCE_MAX_WAIT_ITERS {
            let mut slots = self.nonce_slots.lock().await;
            let start = self.nonce_cursor.fetch_add(1, Ordering::Relaxed) % self.nonce_pubkeys.len();

            for offset in 0..self.nonce_pubkeys.len() {
                let pk = self.nonce_pubkeys[(start + offset) % self.nonce_pubkeys.len()];
                if let Some(slot) = slots.get_mut(&pk) {
                    if !slot.in_use {
                        if let Some(hash) = slot.cached_hash {
                            slot.in_use = true;
                            if waited > 0 {
                                let events =
                                    self.nonce_wait_events.fetch_add(1, Ordering::Relaxed) + 1;
                                self.nonce_wait_iters_total
                                    .fetch_add(waited, Ordering::Relaxed);
                                if events % 50 == 0 {
                                    let avg = self.nonce_wait_iters_total.load(Ordering::Relaxed)
                                        as f64
                                        / events as f64;
                                    info!("📊 Nonce wait: events={} avg_iters={:.1}", events, avg);
                                }
                            }
                            return Ok((pk, hash));
                        }
                    }
                }
            }

            drop(slots);
            waited += 1;
            tokio::time::sleep(Duration::from_millis(NONCE_WAIT_SLEEP_MS)).await;
        }

        anyhow::bail!("All nonce slots busy after {} iters", NONCE_MAX_WAIT_ITERS)
    }

    /// After a tx is sent, refresh the nonce hash in the background and clear in_use.
    fn schedule_nonce_refresh(&self, nonce_pubkey: Pubkey) {
        let rpc = Arc::clone(&self.rpc);
        let slots = Arc::clone(&self.nonce_slots);

        tokio::spawn(async move {
            let result = tokio::task::spawn_blocking(move || -> Result<Hash> {
                let account = rpc
                    .get_account(&nonce_pubkey)
                    .with_context(|| format!("get_account failed for {}", nonce_pubkey))?;
                match nonce_utils::state_from_account(&account)? {
                    State::Initialized(data) => Ok(data.blockhash()),
                    _ => anyhow::bail!("Nonce account {} not initialized", nonce_pubkey),
                }
            })
            .await;

            let mut guard = slots.lock().await;
            if let Some(slot) = guard.get_mut(&nonce_pubkey) {
                match result {
                    Ok(Ok(hash)) => slot.cached_hash = Some(hash),
                    Ok(Err(e)) => {
                        warn!("⚠️  Failed to refresh nonce {}: {}", nonce_pubkey, e);
                        slot.cached_hash = None;
                    }
                    Err(e) => {
                        warn!("⚠️  Nonce refresh task panicked for {}: {}", nonce_pubkey, e);
                        slot.cached_hash = None;
                    }
                }
                slot.in_use = false;
            }
        });
    }

    fn fetch_nonce_hash_sync(&self, pubkey: &Pubkey) -> Result<Hash> {
        let account = self
            .rpc
            .get_account(pubkey)
            .with_context(|| format!("Failed to fetch nonce account {}", pubkey))?;
        match nonce_utils::state_from_account(&account)? {
            State::Initialized(data) => Ok(data.blockhash()),
            _ => anyhow::bail!("Nonce account {} not initialized", pubkey),
        }
    }

    // -----------------------------------------------------------------------
    // Buy template pool
    // -----------------------------------------------------------------------

    fn pool_for(&self, token_program_id: &str) -> &Arc<Mutex<Vec<BuyTemplate>>> {
        if token_program_id == TOKEN_PROGRAM_ID {
            &self.buy_pool_legacy
        } else {
            &self.buy_pool_2022
        }
    }

    fn next_seed(&self) -> String {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let ctr = self.seed_counter.fetch_add(1, Ordering::Relaxed);
        let mut h = DefaultHasher::new();
        ts.hash(&mut h);
        ctr.hash(&mut h);
        format!("{:x}", h.finish())
    }

    fn build_template(&self, token_program_id: &str) -> Result<BuyTemplate> {
        let kp = &self.config.keypair;
        let program_id = Pubkey::from_str(token_program_id)?;
        let seed = self.next_seed();
        let user_token_account = Pubkey::create_with_seed(&kp.pubkey(), &seed, &program_id)?;

        let (space, rent) = if token_program_id == TOKEN_PROGRAM_ID {
            (self.token_account_space, self.token_account_rent)
        } else {
            (self.token_2022_account_space, self.token_2022_account_rent)
        };

        let ix = system_instruction::create_account_with_seed(
            &kp.pubkey(),
            &user_token_account,
            &kp.pubkey(),
            &seed,
            rent,
            space,
            &program_id,
        );

        Ok(BuyTemplate { create_with_seed_ix: ix, user_token_account })
    }

    async fn fill_buy_pool(&self, token_program_id: &str) -> Result<()> {
        let pool = self.pool_for(token_program_id);
        let target = self.config.buy_seed_pool_size;
        loop {
            if pool.lock().await.len() >= target {
                break;
            }
            let t = self.build_template(token_program_id)?;
            pool.lock().await.push(t);
        }
        Ok(())
    }

    /// Pop a template from the pool; build one on-the-fly on a miss (with logging).
    async fn acquire_buy_template(&self, token_program_id: &str) -> Result<BuyTemplate> {
        if let Some(t) = self.pool_for(token_program_id).lock().await.pop() {
            return Ok(t);
        }

        // Pool miss — build inline and track
        let count = if token_program_id == TOKEN_PROGRAM_ID {
            self.buy_pool_misses_legacy.fetch_add(1, Ordering::Relaxed) + 1
        } else {
            self.buy_pool_misses_2022.fetch_add(1, Ordering::Relaxed) + 1
        };
        if count % 25 == 0 {
            warn!("⚠️  Buy pool miss #{} for {}", count, token_program_id);
        }

        self.build_template(token_program_id)
    }

    /// Refill the pool up to target in background (pool-level replenishment).
    fn replenish_pool_async(&self, token_program_id: &str) {
        // Determine which static string to use so we can 'static the spawn
        let prog_id: &'static str = if token_program_id == TOKEN_PROGRAM_ID {
            TOKEN_PROGRAM_ID
        } else {
            TOKEN_2022_PROGRAM_ID
        };

        let pool = Arc::clone(self.pool_for(prog_id));
        let target = self.config.buy_seed_pool_size;
        let kp = self.config.keypair.insecure_clone();
        let (space, rent) = if prog_id == TOKEN_PROGRAM_ID {
            (self.token_account_space, self.token_account_rent)
        } else {
            (self.token_2022_account_space, self.token_2022_account_rent)
        };

        tokio::spawn(async move {
            if pool.lock().await.len() >= target {
                return;
            }

            let program_id = match Pubkey::from_str(prog_id) {
                Ok(p) => p,
                Err(_) => return,
            };
            let seed = format!(
                "{:x}",
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or_default()
            );
            let user_token_account =
                match Pubkey::create_with_seed(&kp.pubkey(), &seed, &program_id) {
                    Ok(p) => p,
                    Err(_) => return,
                };
            let ix = system_instruction::create_account_with_seed(
                &kp.pubkey(),
                &user_token_account,
                &kp.pubkey(),
                &seed,
                rent,
                space,
                &program_id,
            );
            pool.lock().await.push(BuyTemplate { create_with_seed_ix: ix, user_token_account });
        });
    }

    /// Eagerly prebuild one extra template in background right after a buy
    /// (borrowed from file 2's post-buy rebuild pattern).
    fn prebuild_one_template_async(&self, token_program_id: &str) {
        let prog_id: &'static str = if token_program_id == TOKEN_PROGRAM_ID {
            TOKEN_PROGRAM_ID
        } else {
            TOKEN_2022_PROGRAM_ID
        };

        let pool = Arc::clone(self.pool_for(prog_id));
        let kp = self.config.keypair.insecure_clone();
        let (space, rent) = if prog_id == TOKEN_PROGRAM_ID {
            (self.token_account_space, self.token_account_rent)
        } else {
            (self.token_2022_account_space, self.token_2022_account_rent)
        };

        tokio::spawn(async move {
            let program_id = match Pubkey::from_str(prog_id) {
                Ok(p) => p,
                Err(_) => return,
            };
            let seed = format!(
                "{:x}",
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or_default()
            );
            if let Ok(account) = Pubkey::create_with_seed(&kp.pubkey(), &seed, &program_id) {
                let ix = system_instruction::create_account_with_seed(
                    &kp.pubkey(),
                    &account,
                    &kp.pubkey(),
                    &seed,
                    rent,
                    space,
                    &program_id,
                );
                pool.lock().await.push(BuyTemplate {
                    create_with_seed_ix: ix,
                    user_token_account: account,
                });
            }
        });
    }

    // -----------------------------------------------------------------------
    // Global account
    // -----------------------------------------------------------------------

    async fn fetch_global_account(&self) -> Result<GlobalAccount> {
        let pump = self.pump_program;
        let (global_pda, _) = Pubkey::find_program_address(&[b"global"], &pump);

        // Fetch fee_recipient from chain (offset 41 in account data)
        let fee_recipient = {
            let account = self
                .rpc
                .get_account(&global_pda)
                .context("Failed to fetch pump global account")?;
            if account.data.len() < 73 {
                anyhow::bail!("Global account data too short");
            }
            Pubkey::try_from(&account.data[41..73])
                .context("Failed to parse fee_recipient")?
        };

        let (global_volume_accumulator, _) =
            Pubkey::find_program_address(&[b"global_volume_accumulator"], &pump);

        let wallet = self.config.keypair.pubkey();
        let (user_volume_accumulator, _) = Pubkey::find_program_address(
            &[b"user_volume_accumulator", wallet.as_ref()],
            &pump,
        );

        let fee_prog = self.fee_program;
        let (fee_config, _) =
            Pubkey::find_program_address(&[b"fee_config", pump.as_ref()], &fee_prog);

        Ok(GlobalAccount {
            global_pda,
            fee_recipient,
            global_volume_accumulator,
            user_volume_accumulator,
            fee_config,
        })
    }
}
