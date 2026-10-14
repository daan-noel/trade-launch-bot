//! One trader per loaded wallet. The process looks a pubkey up here at fill
//! time. `DeployState.trader` stays the primary, for the pages that still
//! read a single wallet.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use anyhow::{bail, Context, Result};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signer};
use sqlx::PgPool;

use crate::trader::{PumpFunTrader, TraderConfig, SENDER_KEEP_WARM_MS};
use trading_core::config::FeeTuning;
use trading_core::storage::repositories::trader_wallet_repo::TraderWalletRepo;

use super::directory::WalletDirectory;
use super::freeze::WalletFreeze;
use super::keystore::{self, Kek};

pub struct Slot {
    pub name: String,
    pub trader: Arc<PumpFunTrader>,
    pub signer: Arc<Keypair>,
    pub nonces: Vec<Pubkey>,
}

#[derive(Clone)]
pub struct TraderFactory {
    pub rpc_url: String,
    pub sender_urls: Vec<String>,
    pub fees: FeeTuning,
}

impl TraderFactory {
    pub async fn build(
        &self,
        signer: Arc<Keypair>,
        nonces: Vec<Pubkey>,
    ) -> Arc<PumpFunTrader> {
        let as_signer: Arc<dyn Signer + Send + Sync> = signer;
        let mut cfg = TraderConfig::new(
            self.rpc_url.clone(),
            self.sender_urls.clone(),
            as_signer,
            nonces,
        );
        cfg.jito.min_sol = self.fees.jito_min_tip_sol;
        cfg.jito.max_sol = self.fees.jito_max_tip_sol;
        cfg.jito.percentile = self.fees.jito_tip_percentile;
        cfg.compute.price_micro_lamports = self.fees.cu_price_micro_lamports;
        cfg.cache.blockhash_refresh_ms = 10_000;
        cfg.cache.blockhash_max_age_ms = 30_000;
        cfg.nonce.refresh_first_delay_ms = 2_000;
        cfg.sender.keep_warm_ms = Some(SENDER_KEEP_WARM_MS);
        let mut trader = PumpFunTrader::new(Arc::new(cfg));
        if let Err(e) = trader.initialize().await {
            tracing::error!("wallet trader init failed, that wallet stays fail-closed: {e:#}");
        }
        Arc::new(trader)
    }
}

pub struct WalletRuntime {
    slots: RwLock<HashMap<String, Slot>>,
    primary: RwLock<String>,
    pub directory: Arc<WalletDirectory>,
    pub freeze: Arc<WalletFreeze>,
    factory: TraderFactory,
    keystore_dir: Option<PathBuf>,
    kek: Option<Kek>,
}

impl WalletRuntime {
    /// The env key, when the keystore has no rows yet. One slot, no files.
    pub fn legacy(pubkey: String, trader: Arc<PumpFunTrader>, nonces: Vec<Pubkey>, factory: TraderFactory) -> Self {
        let mut slots = HashMap::new();
        // The legacy signer lives inside the trader. Moves and imports need the keystore.
        slots.insert(
            pubkey.clone(),
            Slot {
                name: "main".to_string(),
                trader,
                // The env key stays inside the trader. This signer does not match
                // the pubkey, so a move or a nonce provision refuses until the
                // wallet is loaded from the keystore.
                signer: Arc::new(Keypair::new()),
                nonces,
            },
        );
        let directory = Arc::new(WalletDirectory::default());
        directory.set_primary(&pubkey);
        Self {
            slots: RwLock::new(slots),
            primary: RwLock::new(pubkey),
            directory,
            freeze: Arc::new(WalletFreeze::new()),
            factory,
            keystore_dir: None,
            kek: None,
        }
    }

    pub fn can_manage_keys(&self) -> bool {
        self.keystore_dir.is_some() && self.kek.is_some()
    }

    pub fn keystore_dir(&self) -> Option<&PathBuf> {
        self.keystore_dir.as_ref()
    }

    pub fn kek(&self) -> Option<&Kek> {
        self.kek.as_ref()
    }

    pub fn factory(&self) -> &TraderFactory {
        &self.factory
    }

    pub fn rpc_url(&self) -> &str {
        &self.factory.rpc_url
    }

    pub fn is_empty(&self) -> bool {
        self.slots.read().expect("wallets").is_empty()
    }

    /// No loaded wallets and no keystore. Rule resolution falls back to the
    /// process wallet string. Unit tests that never sign use this.
    pub fn offline() -> Self {
        Self {
            slots: RwLock::new(HashMap::new()),
            primary: RwLock::new(String::new()),
            directory: Arc::new(WalletDirectory::default()),
            freeze: Arc::new(WalletFreeze::new()),
            factory: TraderFactory {
                rpc_url: String::new(),
                sender_urls: Vec::new(),
                fees: FeeTuning::defaults(),
            },
            keystore_dir: None,
            kek: None,
        }
    }

    /// The env-key trader, used when the registry has no loaded wallet yet.
    pub fn adopt_legacy(&self, pubkey: String, trader: Arc<PumpFunTrader>, nonces: Vec<Pubkey>) {
        self.directory.set_primary(&pubkey);
        *self.primary.write().expect("wallets") = pubkey.clone();
        self.slots.write().expect("wallets").insert(
            pubkey,
            Slot {
                name: "main".to_string(),
                trader,
                signer: Arc::new(Keypair::new()),
                nonces,
            },
        );
    }

    pub fn primary_pubkey(&self) -> String {
        self.primary.read().expect("wallets").clone()
    }

    /// The wallet unnamed rules resolve to. The next entry sees it; open bags do not.
    pub fn note_primary(&self, pubkey: &str) {
        self.directory.set_primary(pubkey);
        *self.primary.write().expect("wallets") = pubkey.to_string();
    }

    pub fn primary_trader(&self) -> Arc<PumpFunTrader> {
        let pubkey = self.primary_pubkey();
        self.trader(&pubkey)
            .unwrap_or_else(|| self.slots.read().expect("wallets").values().next().unwrap().trader.clone())
    }

    pub fn trader(&self, pubkey: &str) -> Option<Arc<PumpFunTrader>> {
        self.slots.read().expect("wallets").get(pubkey).map(|s| s.trader.clone())
    }

    pub fn signer(&self, pubkey: &str) -> Option<Arc<Keypair>> {
        self.slots.read().expect("wallets").get(pubkey).map(|s| s.signer.clone())
    }

    /// A pushed account update: a wallet balance, or a nonce account one of the
    /// loaded traders owns.
    pub fn on_account(&self, pubkey: &str, lamports: u64, data: &[u8], slot: u64) {
        if let Some(trader) = self.trader(pubkey) {
            trader.update_balance_lamports_cache(lamports);
            return;
        }
        let Ok(pk) = pubkey.parse::<Pubkey>() else {
            return;
        };
        let slots = self.slots.read().expect("wallets");
        for slot_row in slots.values() {
            if slot_row.nonces.contains(&pk) {
                slot_row.trader.on_nonce_account_update(&pk, data, slot);
                return;
            }
        }
    }

    pub fn watch_accounts(&self) -> Vec<String> {
        let slots = self.slots.read().expect("wallets");
        let mut out = Vec::new();
        for (pubkey, slot) in slots.iter() {
            out.push(pubkey.clone());
            for nonce in &slot.nonces {
                out.push(nonce.to_string());
            }
        }
        out
    }

    pub async fn reload_directory(&self, pool: &PgPool) -> Result<()> {
        let repo = TraderWalletRepo::new(pool.clone());
        let rows = repo.assignments().await?;
        self.directory.replace_assignments(rows);
        if let Some(primary) = repo
            .records()
            .await?
            .into_iter()
            .find(|r| r.is_primary)
        {
            self.directory.set_primary(&primary.pubkey);
            *self.primary.write().expect("wallets") = primary.pubkey;
        }
        Ok(())
    }

    pub async fn insert_loaded(
        &self,
        pubkey: String,
        name: String,
        signer: Arc<Keypair>,
        nonces: Vec<Pubkey>,
        primary: bool,
    ) -> Arc<PumpFunTrader> {
        let trader = self.factory.build(signer.clone(), nonces.clone()).await;
        self.slots.write().expect("wallets").insert(
            pubkey.clone(),
            Slot {
                name,
                trader: trader.clone(),
                signer,
                nonces,
            },
        );
        if primary || self.primary_pubkey().is_empty() {
            *self.primary.write().expect("wallets") = pubkey.clone();
            self.directory.set_primary(&pubkey);
        }
        trader
    }

    pub async fn rebuild(&self, pubkey: &str, nonces: Vec<Pubkey>) -> Result<()> {
        let signer = self
            .signer(pubkey)
            .context("wallet is not loaded")?;
        let name = self
            .slots
            .read()
            .expect("wallets")
            .get(pubkey)
            .map(|s| s.name.clone())
            .context("wallet is not loaded")?;
        let trader = self.factory.build(signer.clone(), nonces.clone()).await;
        self.slots.write().expect("wallets").insert(
            pubkey.to_string(),
            Slot {
                name,
                trader,
                signer,
                nonces,
            },
        );
        Ok(())
    }
}

/// Load every active and draining wallet. `None` when the keystore is off or
/// the registry is empty, so the env key keeps signing.
pub async fn load_keystore(
    pool: &PgPool,
    factory: TraderFactory,
) -> Result<Option<WalletRuntime>> {
    let dir = match std::env::var("WALLET_KEYSTORE") {
        Ok(dir) if !dir.trim().is_empty() => PathBuf::from(dir),
        _ => return Ok(None),
    };
    let passphrase = std::env::var("WALLET_KEK").context("WALLET_KEYSTORE is set and WALLET_KEK is missing")?;
    let kek = Kek::from_passphrase(&passphrase);
    let repo = TraderWalletRepo::new(pool.clone());
    let records = repo.records().await?;
    if records.is_empty() {
        return Ok(Some(WalletRuntime {
            slots: RwLock::new(HashMap::new()),
            primary: RwLock::new(String::new()),
            directory: Arc::new(WalletDirectory::default()),
            freeze: Arc::new(WalletFreeze::new()),
            factory,
            keystore_dir: Some(dir),
            kek: Some(kek),
        }));
    }
    let runtime = WalletRuntime {
        slots: RwLock::new(HashMap::new()),
        primary: RwLock::new(String::new()),
        directory: Arc::new(WalletDirectory::default()),
        freeze: Arc::new(WalletFreeze::new()),
        factory,
        keystore_dir: Some(dir.clone()),
        kek: Some(kek),
    };
    for record in &records {
        if record.status == "archived" {
            continue;
        }
        let keypair = keystore::read_keypair(&dir, &record.key_ref, runtime.kek.as_ref().unwrap())
            .with_context(|| format!("wallet {} ({})", record.name, record.pubkey))?;
        if keypair.pubkey().to_string() != record.pubkey {
            bail!("keystore file {} does not match pubkey {}", record.key_ref, record.pubkey);
        }
        let nonces = repo
            .nonces_for(&record.pubkey)
            .await?
            .into_iter()
            .map(|n| n.pubkey.parse::<Pubkey>())
            .collect::<Result<Vec<_>, _>>()
            .context("nonce pubkey")?;
        runtime
            .insert_loaded(
                record.pubkey.clone(),
                record.name.clone(),
                Arc::new(keypair),
                nonces,
                record.is_primary,
            )
            .await;
    }
    if runtime.primary_pubkey().is_empty() {
        if let Some(first) = records.iter().find(|r| r.status == "active") {
            runtime.directory.set_primary(&first.pubkey);
            *runtime.primary.write().expect("wallets") = first.pubkey.clone();
        }
    }
    runtime.reload_directory(pool).await?;
    Ok(Some(runtime))
}
