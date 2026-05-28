use crate::constants::{
    ASSOCIATED_TOKEN_PROGRAM_ID, EVENT_AUTHORITY, FEE_PROGRAM_ID, LAMPORTS_PER_SOL,
    PUMP_FUN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID, TOKEN_PROGRAM_ID,
};
use anyhow::{Context, Result};
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
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash as StdHash, Hasher};
use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use tracing::{error, info};

#[derive(Debug, Clone)]
struct BuyTemplate {
    create_with_seed_ix: Instruction,
    user_token_account: Pubkey,
}

#[derive(Debug, Clone)]
struct NonceSlot {
    cached_hash: Option<Hash>,
    in_use: bool,
}

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

const MIN_JITO_TIP: f64 = 0.0002;

const PUMP_PROGRAM_UPGRADE_FEE_RECIPIENT: &str = "5YxQFdt3Tr9zJLvkFccqXVUwhdTWJQc1fFg2YPbxvxeD";

#[derive(Debug)]
pub struct TraderConfig {
    pub rpc_url: String,
    pub helius_sender_url: Option<String>,
    pub keypair: Keypair,
    pub nonce_accounts: Vec<String>,
    pub priority_fee_lamports: u64,
    pub buy_seed_pool_size: usize,
}

impl TraderConfig {
    pub fn new(
        rpc_url: String,
        helius_sender_url: Option<String>,
        keypair: Keypair,
        nonce_accounts: Vec<String>,
        priority_fee_lamports: u64,
        buy_seed_pool_size: usize,
    ) -> Self {
        Self {
            rpc_url,
            helius_sender_url,
            keypair,
            nonce_accounts,
            priority_fee_lamports,
            buy_seed_pool_size,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GlobalAccount {
    pub global_pda: Pubkey,
    pub fee_recipient: Pubkey,
    pub global_volume_accumulator: Pubkey,
    pub user_volume_accumulator: Pubkey,
    pub fee_config: Pubkey,
}

#[derive(Debug, Clone, Copy)]
pub struct TokenPDAs {
    pub token_program: Pubkey,
    pub bonding_curve: Pubkey,
    pub bonding_curve_v2: Pubkey,
    pub associated_bonding_curve: Pubkey,
    pub creator_vault: Pubkey,
}

pub struct PumpFunTrader {
    config: Arc<TraderConfig>,
    client: reqwest::Client,
    rpc_client: Arc<Option<RpcClient>>,
    global_account: Option<GlobalAccount>,
    compute_budget_instructions: Vec<Instruction>,
    configured_nonce_pubkeys: Vec<Pubkey>,
    nonce_cursor: AtomicUsize,
    // Pre-calculated values for account creation
    token_account_space: u64,
    token_account_rent_lamports: u64,
    token_2022_account_space: u64,
    token_2022_account_rent_lamports: u64,
    pump_program: Pubkey,
    system_program: Pubkey,
    associated_token_program: Pubkey,
    event_authority: Pubkey,
    fee_program: Pubkey,
    // Cache of user token accounts (token_mint -> user_token_account)
    user_token_accounts: Arc<Mutex<HashMap<String, Pubkey>>>,
    // Cache of token PDAs (token_mint -> TokenPDAs)
    token_pdas: Arc<Mutex<HashMap<String, TokenPDAs>>>,
    // Shared pre-built instruction
    prebuilt_jito_tip: Arc<Mutex<Option<Instruction>>>,
    // Durable nonce state for all configured nonce accounts
    nonce_slots: Arc<Mutex<HashMap<Pubkey, NonceSlot>>>,
    // Pre-created seed account templates for concurrent buy submissions
    buy_seed_pool_legacy: Arc<Mutex<Vec<BuyTemplate>>>,
    buy_seed_pool_2022: Arc<Mutex<Vec<BuyTemplate>>>,
    seed_counter: AtomicUsize,
    nonce_wait_events: AtomicUsize,
    nonce_wait_iters_total: AtomicUsize,
    buy_seed_pool_misses_legacy: AtomicUsize,
    buy_seed_pool_misses_2022: AtomicUsize,
}

impl PumpFunTrader {
    fn pump_program_upgrade_fee_recipient() -> Pubkey {
        Pubkey::from_str(PUMP_PROGRAM_UPGRADE_FEE_RECIPIENT)
            .expect("Invalid pump upgrade fee recipient")
    }

    fn parse_and_store_nonce(
        list: &mut Vec<Pubkey>,
        nonce: &str,
        seen: &mut HashSet<Pubkey>,
        role: &str,
    ) -> Result<()> {
        if nonce.is_empty() {
            anyhow::bail!("{} nonce account must not be empty", role);
        }

        let pubkey = Pubkey::from_str(nonce)
            .with_context(|| format!("Invalid {} nonce account pubkey: {}", role, nonce))?;

        if seen.insert(pubkey) {
            list.push(pubkey);
        }

        Ok(())
    }

    fn collect_configured_nonce_pubkeys(&mut self) -> Result<()> {
        if self.config.nonce_accounts.is_empty() {
            anyhow::bail!("At least one nonce account is required");
        }

        self.configured_nonce_pubkeys.clear();

        let mut seen = HashSet::new();
        for nonce in self.config.nonce_accounts.clone() {
            Self::parse_and_store_nonce(
                &mut self.configured_nonce_pubkeys,
                &nonce,
                &mut seen,
                "shared",
            )?;
        }

        info!("✅ Configured {} shared nonce account(s)", seen.len(),);

        if self.config.buy_seed_pool_size == 0 {
            anyhow::bail!("buy_seed_pool_size must be at least 1");
        }

        info!(
            "🌱 Buy seed pool target size: {}",
            self.config.buy_seed_pool_size,
        );

        Ok(())
    }

    pub fn new(config: Arc<TraderConfig>) -> Self {
        let rpc_client = Some(RpcClient::new_with_commitment(
            config.rpc_url.clone(),
            CommitmentConfig::confirmed(),
        ));

        Self {
            config,
            client: reqwest::Client::new(),
            rpc_client: Arc::new(rpc_client),
            global_account: None,
            compute_budget_instructions: Vec::new(),
            configured_nonce_pubkeys: Vec::new(),
            nonce_cursor: AtomicUsize::new(0),
            token_account_space: 165, // Token base account size
            token_account_rent_lamports: 2_000_000, // Will be updated in initialize()
            token_2022_account_space: 182, // Token-2022 base account size
            token_2022_account_rent_lamports: 2_000_000, // Will be updated in initialize()
            pump_program: Pubkey::from_str(PUMP_FUN_PROGRAM_ID).unwrap(),
            system_program: system_program::id(),
            associated_token_program: Pubkey::from_str(ASSOCIATED_TOKEN_PROGRAM_ID).unwrap(),
            event_authority: Pubkey::from_str(EVENT_AUTHORITY).unwrap(),
            fee_program: Pubkey::from_str(FEE_PROGRAM_ID).unwrap(),
            user_token_accounts: Arc::new(Mutex::new(HashMap::new())),
            token_pdas: Arc::new(Mutex::new(HashMap::new())),
            prebuilt_jito_tip: Arc::new(Mutex::new(None)),
            nonce_slots: Arc::new(Mutex::new(HashMap::new())),
            buy_seed_pool_legacy: Arc::new(Mutex::new(Vec::new())),
            buy_seed_pool_2022: Arc::new(Mutex::new(Vec::new())),
            seed_counter: AtomicUsize::new(0),
            nonce_wait_events: AtomicUsize::new(0),
            nonce_wait_iters_total: AtomicUsize::new(0),
            buy_seed_pool_misses_legacy: AtomicUsize::new(0),
            buy_seed_pool_misses_2022: AtomicUsize::new(0),
        }
    }

    pub async fn initialize(&mut self) -> Result<()> {
        info!("🔧 Pre-fetching pump.fun global account...");

        self.global_account = Some(self.fetch_global_account().await?);

        info!("✅ Global account initialized");

        info!("💼 Wallet: {}", self.config.keypair.pubkey());

        // Log RPC URL
        info!("🌐 RPC URL: {}", self.config.rpc_url);

        // Pre-build compute budget instructions with fixed priority fee
        let priority_fee_lamports = self.config.priority_fee_lamports;
        self.compute_budget_instructions = vec![
            ComputeBudgetInstruction::set_compute_unit_limit(200_000),
            ComputeBudgetInstruction::set_compute_unit_price(priority_fee_lamports),
        ];
        info!("⚡ Using priority fee: {} lamports", priority_fee_lamports);

        // Parse and deduplicate configured nonce accounts.
        self.collect_configured_nonce_pubkeys()?;

        // Verify RPC client is initialized
        if self.rpc_client.is_none() {
            error!("❌ RPC client not initialized - check RPC URL in config");
            anyhow::bail!("RPC client is required but RPC URL not configured");
        }

        // Calculate proper rent for Token accounts
        let rpc_client = self.rpc_client.as_ref().as_ref().unwrap();
        self.token_account_rent_lamports = rpc_client
            .get_minimum_balance_for_rent_exemption(self.token_account_space as usize)
            .context("Failed to get minimum balance for rent exemption")?;
        self.token_2022_account_rent_lamports = rpc_client
            .get_minimum_balance_for_rent_exemption(self.token_2022_account_space as usize)
            .context("Failed to get minimum balance for rent exemption for Token-2022")?;

        // Pre-build Jito tip instruction with fixed tip amount (shared between buy and sell)
        let jito_tip_ix = {
            let tip_accounts = JITO_TIP_ACCOUNTS;
            let selected_tip = tip_accounts
                .choose(&mut rand::thread_rng())
                .context("Failed to select tip account")?;
            let jito_tip_lamports = (MIN_JITO_TIP * LAMPORTS_PER_SOL as f64) as u64;

            Some(system_instruction::transfer(
                &self.config.keypair.pubkey(),
                &Pubkey::from_str(selected_tip)?,
                jito_tip_lamports,
            ))
        };
        let jito_enabled = jito_tip_ix.is_some();
        *self.prebuilt_jito_tip.lock().await = jito_tip_ix;
        info!(
            "💸 Jito tip instruction pre-built: {}",
            if jito_enabled { "enabled" } else { "disabled" }
        );

        info!("🔧 Pre-fetching nonce hashes...");
        {
            let mut slots = self.nonce_slots.lock().await;
            slots.clear();

            let mut unique = HashSet::new();
            for nonce in self.configured_nonce_pubkeys.iter() {
                if !unique.insert(*nonce) {
                    continue;
                }

                let hash = self
                    .get_nonce_hash(nonce)
                    .with_context(|| format!("Failed to fetch nonce hash for {}", nonce))?;
                slots.insert(
                    *nonce,
                    NonceSlot {
                        cached_hash: Some(hash),
                        in_use: false,
                    },
                );
            }
        }
        info!(
            "✅ Nonce hashes pre-fetched for {} unique account(s)",
            self.nonce_slots.lock().await.len()
        );

        info!("🔧 Pre-building buy seed templates...");
        self.fill_buy_seed_pool(TOKEN_PROGRAM_ID).await?;
        self.fill_buy_seed_pool(TOKEN_2022_PROGRAM_ID).await?;

        info!("✅ Transaction templates pre-built");

        Ok(())
    }

    async fn fetch_global_account(&self) -> Result<GlobalAccount> {
        let pump_program = Pubkey::from_str(PUMP_FUN_PROGRAM_ID)?;

        let (global_pda, _) = Pubkey::find_program_address(&[b"global"], &pump_program);

        // Fetch the actual account data from Solana if RPC URL is available
        let fee_recipient = if !self.config.rpc_url.is_empty() {
            match self
                .fetch_fee_recipient_from_chain(&self.config.rpc_url, &global_pda)
                .await
            {
                Ok(recipient) => recipient,
                Err(e) => {
                    info!(
                        "⚠️  Failed to fetch fee_recipient from chain: {}. Using default.",
                        e
                    );
                    // Fallback to a known pump.fun fee recipient
                    Pubkey::from_str("CebN5WGQ4jvEPvsVU4EoHEpgzq1VV7AbicfhtW4xC9iM")
                        .unwrap_or_else(|_| pump_program)
                }
            }
        } else {
            // If no RPC URL, use a known pump.fun fee recipient
            Pubkey::from_str("CebN5WGQ4jvEPvsVU4EoHEpgzq1VV7AbicfhtW4xC9iM")
                .unwrap_or_else(|_| pump_program)
        };

        let (global_volume_accumulator, _) =
            Pubkey::find_program_address(&[b"global_volume_accumulator"], &pump_program);

        let wallet_pubkey = self.config.keypair.pubkey();

        let (user_volume_accumulator, _) = Pubkey::find_program_address(
            &[b"user_volume_accumulator", wallet_pubkey.as_ref()],
            &pump_program,
        );

        let fee_program = Pubkey::from_str(FEE_PROGRAM_ID)?;
        let (fee_config, _) =
            Pubkey::find_program_address(&[b"fee_config", pump_program.as_ref()], &fee_program);

        Ok(GlobalAccount {
            global_pda,
            fee_recipient,
            global_volume_accumulator,
            user_volume_accumulator,
            fee_config,
        })
    }

    async fn fetch_fee_recipient_from_chain(
        &self,
        rpc_url: &str,
        global_pda: &Pubkey,
    ) -> Result<Pubkey> {
        let rpc_client =
            RpcClient::new_with_commitment(rpc_url.to_string(), CommitmentConfig::confirmed());

        // Fetch account data
        let account = rpc_client
            .get_account(global_pda)
            .context("Failed to fetch global account")?;

        // Parse the account data to extract fee_recipient
        // Global account structure: discriminator(8) + initialized(1) + authority(32) + feeRecipient(32)
        // Fee recipient starts at offset 41 (8 + 1 + 32)
        if account.data.len() >= 73 {
            let fee_recipient_bytes = &account.data[41..73];
            let fee_recipient = Pubkey::try_from(fee_recipient_bytes)
                .context("Failed to parse fee_recipient from account data")?;

            Ok(fee_recipient)
        } else {
            anyhow::bail!("Global account data is too short (expected at least 73 bytes)")
        }
    }

    fn get_next_seed(&self) -> String {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let counter = self.seed_counter.fetch_add(1, Ordering::Relaxed);

        let mut hasher = DefaultHasher::new();
        timestamp.hash(&mut hasher);
        counter.hash(&mut hasher);
        let hash = hasher.finish();

        format!("{:x}", hash)
    }

    fn build_buy_template(&self, token_program_id: &str) -> Result<BuyTemplate> {
        let keypair = &self.config.keypair;
        let program_id = Pubkey::from_str(token_program_id)?;
        let seed = self.get_next_seed();
        let user_token_account = Pubkey::create_with_seed(&keypair.pubkey(), &seed, &program_id)?;

        let (space, rent_lamports) = if token_program_id == TOKEN_PROGRAM_ID {
            (self.token_account_space, self.token_account_rent_lamports)
        } else {
            (
                self.token_2022_account_space,
                self.token_2022_account_rent_lamports,
            )
        };

        let create_with_seed_ix = system_instruction::create_account_with_seed(
            &keypair.pubkey(),
            &user_token_account,
            &keypair.pubkey(),
            &seed,
            rent_lamports,
            space,
            &program_id,
        );

        Ok(BuyTemplate {
            create_with_seed_ix,
            user_token_account,
        })
    }

    async fn fill_buy_seed_pool(&self, token_program_id: &str) -> Result<()> {
        let target = self.config.buy_seed_pool_size;
        let pool = if token_program_id == TOKEN_PROGRAM_ID {
            &self.buy_seed_pool_legacy
        } else {
            &self.buy_seed_pool_2022
        };

        loop {
            let current_len = { pool.lock().await.len() };
            if current_len >= target {
                break;
            }

            let template = self.build_buy_template(token_program_id)?;
            pool.lock().await.push(template);
        }

        Ok(())
    }

    async fn acquire_buy_template(&self, token_program_id: &str) -> Result<BuyTemplate> {
        let pool = if token_program_id == TOKEN_PROGRAM_ID {
            &self.buy_seed_pool_legacy
        } else {
            &self.buy_seed_pool_2022
        };

        if let Some(template) = pool.lock().await.pop() {
            return Ok(template);
        }

        let miss_count = if token_program_id == TOKEN_PROGRAM_ID {
            self.buy_seed_pool_misses_legacy
                .fetch_add(1, Ordering::Relaxed)
                + 1
        } else {
            self.buy_seed_pool_misses_2022
                .fetch_add(1, Ordering::Relaxed)
                + 1
        };
        if miss_count % 25 == 0 {
            info!(
                "⚠️ buy seed pool misses for {} reached {}",
                token_program_id, miss_count
            );
        }

        self.build_buy_template(token_program_id)
    }

    fn replenish_buy_seed_pool_async(&self, token_program_id: &'static str) {
        let pool = if token_program_id == TOKEN_PROGRAM_ID {
            Arc::clone(&self.buy_seed_pool_legacy)
        } else {
            Arc::clone(&self.buy_seed_pool_2022)
        };
        let target = self.config.buy_seed_pool_size;
        let rpc_client = Arc::clone(&self.rpc_client);
        let keypair = self.config.keypair.insecure_clone();
        let token_account_space = self.token_account_space;
        let token_account_rent_lamports = self.token_account_rent_lamports;
        let token_2022_account_space = self.token_2022_account_space;
        let token_2022_account_rent_lamports = self.token_2022_account_rent_lamports;

        tokio::spawn(async move {
            let need_build = {
                let guard = pool.lock().await;
                guard.len() < target
            };
            if !need_build {
                return;
            }

            if rpc_client.is_none() {
                return;
            }

            let program_id = match Pubkey::from_str(token_program_id) {
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
                match Pubkey::create_with_seed(&keypair.pubkey(), &seed, &program_id) {
                    Ok(p) => p,
                    Err(_) => return,
                };
            let (space, rent_lamports) = if token_program_id == TOKEN_PROGRAM_ID {
                (token_account_space, token_account_rent_lamports)
            } else {
                (token_2022_account_space, token_2022_account_rent_lamports)
            };
            let create_with_seed_ix = system_instruction::create_account_with_seed(
                &keypair.pubkey(),
                &user_token_account,
                &keypair.pubkey(),
                &seed,
                rent_lamports,
                space,
                &program_id,
            );

            pool.lock().await.push(BuyTemplate {
                create_with_seed_ix,
                user_token_account,
            });
        });
    }

    async fn acquire_nonce(
        &self,
        candidates: &[Pubkey],
        cursor: &AtomicUsize,
    ) -> Result<(Pubkey, Hash)> {
        if candidates.is_empty() {
            anyhow::bail!("No nonce accounts configured");
        }

        const MAX_WAIT_ITERS: usize = 200;
        let mut waited_iters = 0usize;
        for _ in 0..MAX_WAIT_ITERS {
            let mut slots = self.nonce_slots.lock().await;
            let start = cursor.fetch_add(1, Ordering::Relaxed) % candidates.len();

            for offset in 0..candidates.len() {
                let nonce_pubkey = candidates[(start + offset) % candidates.len()];
                if let Some(slot) = slots.get_mut(&nonce_pubkey) {
                    if slot.in_use {
                        continue;
                    }
                    if let Some(hash) = slot.cached_hash {
                        slot.in_use = true;
                        if waited_iters > 0 {
                            let events = self.nonce_wait_events.fetch_add(1, Ordering::Relaxed) + 1;
                            self.nonce_wait_iters_total
                                .fetch_add(waited_iters, Ordering::Relaxed);
                            if events % 50 == 0 {
                                let total_wait =
                                    self.nonce_wait_iters_total.load(Ordering::Relaxed);
                                let avg_wait = total_wait as f64 / events as f64;
                                info!(
                                    "📊 nonce wait stats: events={}, avg_wait_iters={:.2}",
                                    events, avg_wait
                                );
                            }
                        }
                        return Ok((nonce_pubkey, hash));
                    }
                }
            }

            drop(slots);
            waited_iters += 1;
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        anyhow::bail!("No available nonce account with prefetched hash")
    }

    fn schedule_nonce_refresh(&self, nonce_pubkey: Pubkey) {
        let rpc_client = Arc::clone(&self.rpc_client);
        let slots = Arc::clone(&self.nonce_slots);

        tokio::spawn(async move {
            let refreshed_hash = (|| -> Result<Hash> {
                let client = rpc_client
                    .as_ref()
                    .as_ref()
                    .context("RPC client not initialized")?;

                let account = client
                    .get_account(&nonce_pubkey)
                    .with_context(|| format!("Failed to fetch nonce account {}", nonce_pubkey))?;

                let nonce_state = nonce_utils::state_from_account(&account)
                    .with_context(|| format!("Failed to parse nonce account {}", nonce_pubkey))?;

                match nonce_state {
                    State::Initialized(data) => Ok(data.blockhash()),
                    _ => anyhow::bail!("Nonce account {} not initialized", nonce_pubkey),
                }
            })();

            let mut guard = slots.lock().await;
            if let Some(slot) = guard.get_mut(&nonce_pubkey) {
                slot.cached_hash = refreshed_hash.ok();
                slot.in_use = false;
            }
        });
    }

    fn get_nonce_hash(&self, nonce_account_pubkey: &Pubkey) -> Result<Hash> {
        let rpc_client = self
            .rpc_client
            .as_ref()
            .as_ref()
            .context("RPC client not initialized")?;

        let account = match rpc_client.get_account(nonce_account_pubkey) {
            Ok(acc) => acc,
            Err(e) => {
                error!(
                    "❌ RPC error fetching nonce account {}: {:?}",
                    nonce_account_pubkey, e
                );
                return Err(anyhow::anyhow!("Failed to fetch nonce account {}: {}. Make sure the nonce account exists and RPC URL is accessible.", nonce_account_pubkey, e));
            }
        };

        let nonce_state = nonce_utils::state_from_account(&account).with_context(|| {
            format!(
                "Failed to parse nonce account data. Data length: {}, Owner: {}",
                account.data.len(),
                account.owner
            )
        })?;

        let nonce_hash = match nonce_state {
            State::Initialized(data) => data.blockhash(),
            _ => anyhow::bail!("Nonce account not initialized"),
        };

        let hash = nonce_hash;
        Ok(hash)
    }

    pub async fn buy_token(
        &self,
        token_mint: &str,
        creator: &str,
        _is_cashback: bool,
        token_program_id: &str,
        sol_amount: f64,
    ) -> Result<bool> {
        let buy_amount_lamports = (sol_amount * LAMPORTS_PER_SOL as f64) as u64;

        // Get pre-configured values
        let keypair = &self.config.keypair;

        let (nonce_account_buy_pubkey, nonce_hash) = self
            .acquire_nonce(&self.configured_nonce_pubkeys, &self.nonce_cursor)
            .await?;

        let result = async {
            let global_account = self
                .global_account
                .as_ref()
                .context("Global account not initialized")?;

            // Parse token-specific addresses (only mint needs parsing)
            let mint_pubkey = Pubkey::from_str(token_mint)?;
            let creator_pubkey = Pubkey::from_str(creator)?;
            let token_program_pubkey = Pubkey::from_str(token_program_id)?;

            // Derive PDAs (token-specific)
            let (bonding_curve_pda, _) = Pubkey::find_program_address(
                &[b"bonding-curve", mint_pubkey.as_ref()],
                &self.pump_program,
            );

            let (bonding_curve_v2_pda, _) = Pubkey::find_program_address(
                &[b"bonding-curve-v2", mint_pubkey.as_ref()],
                &self.pump_program,
            );

            let (associated_bonding_curve, _) = Pubkey::find_program_address(
                &[
                    bonding_curve_pda.as_ref(),
                    token_program_pubkey.as_ref(),
                    mint_pubkey.as_ref(),
                ],
                &self.associated_token_program,
            );

            // Derive creator vault PDA
            let (creator_vault, _) = Pubkey::find_program_address(
                &[b"creator-vault", creator_pubkey.as_ref()],
                &self.pump_program,
            );

            // Cache the PDAs for this token mint
            {
                let mut cache = self.token_pdas.lock().await;
                cache.insert(
                    token_mint.to_string(),
                    TokenPDAs {
                        token_program: token_program_pubkey,
                        bonding_curve: bonding_curve_pda,
                        bonding_curve_v2: bonding_curve_v2_pda,
                        associated_bonding_curve,
                        creator_vault,
                    },
                );
            }

            let buy_template = self.acquire_buy_template(token_program_id).await?;
            let create_with_seed_ix = buy_template.create_with_seed_ix;
            let user_token_account = buy_template.user_token_account;

            let jito_tip_ix = self.prebuilt_jito_tip.lock().await.clone();

            // Cache the user token account for this token mint
            {
                let mut cache = self.user_token_accounts.lock().await;
                cache.insert(token_mint.to_string(), user_token_account);
            }

            // Build instructions using pre-built components
            let mut instructions = Vec::new();

            // 1. Use pre-built compute budget instructions
            instructions.extend_from_slice(&self.compute_budget_instructions);

            // 2. CreateAccountWithSeed (pre-built)
            instructions.push(create_with_seed_ix);

            // 3. InitializeAccount3 instruction (Token-2022) or InitializeAccount (legacy)
            let init_account_ix = if token_program_id == TOKEN_PROGRAM_ID {
                spl_token::instruction::initialize_account3(
                    &token_program_pubkey,
                    &user_token_account,
                    &mint_pubkey,
                    &keypair.pubkey(),
                )?
            } else {
                spl_token_2022::instruction::initialize_account3(
                    &token_program_pubkey,
                    &user_token_account,
                    &mint_pubkey,
                    &keypair.pubkey(),
                )?
            };
            instructions.push(init_account_ix);

            // 4. Buy instruction (Buy_exact_sol_in)
            let min_token_amount = 1u64;
            let mut buy_data = vec![0x38, 0xfc, 0x74, 0x08, 0x9e, 0xdf, 0xcd, 0x5f];
            buy_data.extend_from_slice(&buy_amount_lamports.to_le_bytes());
            buy_data.extend_from_slice(&min_token_amount.to_le_bytes());

            instructions.push(Instruction {
                program_id: self.pump_program,
                accounts: vec![
                    AccountMeta::new_readonly(global_account.global_pda, false),
                    AccountMeta::new(global_account.fee_recipient, false),
                    AccountMeta::new(mint_pubkey, false),
                    AccountMeta::new(bonding_curve_pda, false),
                    AccountMeta::new(associated_bonding_curve, false),
                    AccountMeta::new(user_token_account, false),
                    AccountMeta::new(keypair.pubkey(), true),
                    AccountMeta::new_readonly(self.system_program, false),
                    AccountMeta::new_readonly(token_program_pubkey, false),
                    AccountMeta::new(creator_vault, false),
                    AccountMeta::new_readonly(self.event_authority, false),
                    AccountMeta::new_readonly(self.pump_program, false),
                    AccountMeta::new(global_account.global_volume_accumulator, false),
                    AccountMeta::new(global_account.user_volume_accumulator, false),
                    AccountMeta::new_readonly(global_account.fee_config, false),
                    AccountMeta::new_readonly(self.fee_program, false),
                    AccountMeta::new_readonly(bonding_curve_v2_pda, false),
                    AccountMeta::new(Self::pump_program_upgrade_fee_recipient(), false),
                ],
                data: buy_data,
            });

            // 5. Optional Jito tip (pre-built)
            if let Some(tip_ix) = jito_tip_ix {
                instructions.push(tip_ix);
            }

            // Build transaction - use nonce-based transaction
            let message = Message::new_with_nonce(
                instructions,
                Some(&keypair.pubkey()),
                &nonce_account_buy_pubkey,
                &keypair.pubkey(),
            );

            let mut transaction = Transaction::new_unsigned(message);
            transaction.sign(&[keypair], nonce_hash);

            // Serialize and send
            use base64::{engine::general_purpose, Engine as _};
            let transaction_base64 =
                general_purpose::STANDARD.encode(&bincode::serialize(&transaction)?);

            match self.send_via_helius_sender(&transaction_base64).await {
                Ok(_signature) => Ok(true),
                Err(e) => {
                    error!(
                        "❌ BUY FAILED: {} - token: {}, sol: {}",
                        e, token_mint, sol_amount
                    );
                    Err(e)
                }
            }
        }
        .await;

        self.schedule_nonce_refresh(nonce_account_buy_pubkey);
        if token_program_id == TOKEN_PROGRAM_ID {
            self.replenish_buy_seed_pool_async(TOKEN_PROGRAM_ID);
        } else {
            self.replenish_buy_seed_pool_async(TOKEN_2022_PROGRAM_ID);
        }

        result
    }

    pub async fn sell_token(
        &self,
        token_mint: &str,
        is_cashback: bool,
        token_amount: u64,
        creator: Option<&str>,
    ) -> Result<bool> {
        if self.configured_nonce_pubkeys.is_empty() {
            anyhow::bail!("Nonce accounts are not configured");
        }

        const MAX_SELL_ATTEMPTS: usize = 5;
        let mut last_error: Option<anyhow::Error> = None;

        for sell_attempt in 0..MAX_SELL_ATTEMPTS {
            let nonce_allocation = self
                .acquire_nonce(&self.configured_nonce_pubkeys, &self.nonce_cursor)
                .await;

            let (nonce_account_sell_pubkey, nonce_hash) = match nonce_allocation {
                Ok(value) => value,
                Err(e) => {
                    last_error = Some(e);
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                }
            };

            info!(
                "🔁 Sell attempt {}/{} for {} using nonce account {}",
                sell_attempt + 1,
                MAX_SELL_ATTEMPTS,
                token_mint,
                nonce_account_sell_pubkey,
            );

            let attempt_result = self
                .execute_sell_transaction(
                    token_mint,
                    is_cashback,
                    token_amount,
                    creator,
                    &nonce_account_sell_pubkey,
                    nonce_hash,
                )
                .await;

            self.schedule_nonce_refresh(nonce_account_sell_pubkey);

            match attempt_result {
                Ok(true) => return Ok(true),
                Ok(false) => {
                    let err = anyhow::anyhow!(
                        "Sell transaction returned false on attempt {}/{}",
                        sell_attempt + 1,
                        MAX_SELL_ATTEMPTS,
                    );
                    error!("❌ {}", err);
                    last_error = Some(err);
                }
                Err(e) => {
                    error!(
                        "❌ Sell attempt {}/{} failed for {}: {}",
                        sell_attempt + 1,
                        MAX_SELL_ATTEMPTS,
                        token_mint,
                        e,
                    );
                    last_error = Some(e);
                }
            }

            if sell_attempt < MAX_SELL_ATTEMPTS - 1 {
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        }

        if let Some(err) = last_error {
            return Err(anyhow::anyhow!(
                "Sell transaction failed after {} attempts: {}",
                MAX_SELL_ATTEMPTS,
                err
            ));
        }

        anyhow::bail!("Sell failed after {} attempts", MAX_SELL_ATTEMPTS)
    }

    async fn execute_sell_transaction(
        &self,
        token_mint: &str,
        is_cashback: bool,
        token_amount: u64,
        creator: Option<&str>,
        nonce_account_sell_pubkey: &Pubkey,
        nonce_hash: Hash,
    ) -> Result<bool> {
        // Get pre-configured values
        let keypair = &self.config.keypair;

        let global_account = self
            .global_account
            .as_ref()
            .context("Global account not initialized")?;

        // Parse token-specific addresses (only mint needs parsing)
        let mint_pubkey = Pubkey::from_str(token_mint)?;

        // Get cached PDAs (should exist from buy)
        let mut token_pdas = {
            let cache = self.token_pdas.lock().await;
            cache
                .get(token_mint)
                .copied()
                .context("Token PDAs not found in cache. Must buy before selling.")?
        };

        // If a creator is provided, recalculate the creator vault PDA
        if let Some(creator_str) = creator {
            let creator_pubkey = Pubkey::from_str(creator_str)?;
            let (correct_creator_vault, _) = Pubkey::find_program_address(
                &[b"creator-vault", creator_pubkey.as_ref()],
                &self.pump_program,
            );
            if token_pdas.creator_vault != correct_creator_vault {
                info!(
                    "🔄 Using updated creator vault for {} (from {} to {})",
                    token_mint, token_pdas.creator_vault, correct_creator_vault
                );
                token_pdas.creator_vault = correct_creator_vault;
            }
        }

        // Get cached user token account (should exist from buy)
        let user_token_account = {
            let cache = self.user_token_accounts.lock().await;
            cache
                .get(token_mint)
                .copied()
                .context("User token account not found. Must buy before selling.")?
        };

        // Get pre-built Jito tip (shared between buy and sell)
        let jito_tip_ix = self.prebuilt_jito_tip.lock().await.clone();

        // Build instructions using pre-built components
        let mut instructions = Vec::new();

        // 1. Use pre-built compute budget instructions
        instructions.extend_from_slice(&self.compute_budget_instructions);

        // 2. Sell instruction (Sell)
        let min_sol_output = 1u64;
        let mut sell_data = vec![0x33, 0xe6, 0x85, 0xa4, 0x01, 0x7f, 0x83, 0xad];
        sell_data.extend_from_slice(&token_amount.to_le_bytes());
        sell_data.extend_from_slice(&min_sol_output.to_le_bytes());

        // Build accounts vec
        let mut accounts = vec![
            AccountMeta::new_readonly(global_account.global_pda, false),
            AccountMeta::new(global_account.fee_recipient, false),
            AccountMeta::new(mint_pubkey, false),
            AccountMeta::new(token_pdas.bonding_curve, false),
            AccountMeta::new(token_pdas.associated_bonding_curve, false),
            AccountMeta::new(user_token_account, false),
            AccountMeta::new(keypair.pubkey(), true),
            AccountMeta::new_readonly(self.system_program, false),
            AccountMeta::new(token_pdas.creator_vault, false),
            AccountMeta::new_readonly(token_pdas.token_program, false),
            AccountMeta::new_readonly(self.event_authority, false),
            AccountMeta::new_readonly(self.pump_program, false),
            AccountMeta::new_readonly(global_account.fee_config, false),
            AccountMeta::new_readonly(self.fee_program, false),
        ];

        if is_cashback {
            accounts.push(AccountMeta::new(
                global_account.user_volume_accumulator,
                false,
            ));
        }

        accounts.push(AccountMeta::new(token_pdas.bonding_curve_v2, false));
        accounts.push(AccountMeta::new(
            Self::pump_program_upgrade_fee_recipient(),
            false,
        ));

        instructions.push(Instruction {
            program_id: self.pump_program,
            accounts,
            data: sell_data,
        });

        // 3. Optional Jito tip (pre-built)
        if let Some(tip_ix) = jito_tip_ix {
            instructions.push(tip_ix);
        }

        let message = Message::new_with_nonce(
            instructions,
            Some(&keypair.pubkey()),
            nonce_account_sell_pubkey,
            &keypair.pubkey(),
        );

        let mut transaction = Transaction::new_unsigned(message);
        transaction.sign(&[keypair], nonce_hash);

        // Serialize and send
        use base64::{engine::general_purpose, Engine as _};
        let transaction_base64 =
            general_purpose::STANDARD.encode(&bincode::serialize(&transaction)?);

        match self.send_via_helius_sender(&transaction_base64).await {
            Ok(_signature) => Ok(true),
            Err(e) => {
                error!(
                    "❌ SELL FAILED: {} - token: {}, amount: {}",
                    e, token_mint, token_amount
                );
                Err(e)
            }
        }
    }

    async fn send_via_helius_sender(&self, transaction_base64: &str) -> Result<String> {
        let sender_url = self
            .config
            .helius_sender_url
            .as_ref()
            .map(|s| s.as_str())
            .unwrap_or("https://sender.helius-rpc.com/fast");

        let request_body = json!({
            "jsonrpc": "2.0",
            "id": chrono::Utc::now().timestamp_millis().to_string(),
            "method": "sendTransaction",
            "params": [
                transaction_base64,
                {
                    "encoding": "base64",
                    "skipPreflight": true,
                    "maxRetries": 0,
                }
            ]
        });

        let response = self
            .client
            .post(sender_url)
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            anyhow::bail!("Helius Sender HTTP error: {}", error_text);
        }

        let json: serde_json::Value = response.json().await?;

        if let Some(error) = json.get("error") {
            anyhow::bail!("Helius Sender JSON-RPC error: {:?}", error);
        }

        let signature_str = json
            .get("result")
            .and_then(|r| r.as_str())
            .context("No signature in response")?;

        let signature = Signature::from_str(signature_str)?;

        let rpc_client = self
            .rpc_client
            .as_ref()
            .as_ref()
            .context("RPC client not initialized")?;

        // Wait for the transaction to be confirmed
        // If it times out here, the outer sell_token loop will rebuild and retry
        let mut retries = 3;
        while retries > 0 {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let status = rpc_client.get_signature_status(&signature)?;
            match status {
                Some(Ok(())) => {
                    return Ok(signature_str.to_string());
                }
                Some(Err(e)) => {
                    error!("❌ Transaction {} failed with error: {:?}", signature, e);
                    anyhow::bail!("Transaction failed: {:?}", e);
                }
                None => {
                    retries -= 1;
                }
            }
        }

        anyhow::bail!(
            "Transaction confirmation timed out for signature: {}",
            signature
        );
    }
}
