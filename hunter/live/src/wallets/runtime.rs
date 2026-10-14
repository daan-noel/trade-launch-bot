//! One trader per loaded wallet. The process looks a pubkey up here at fill
//! time. `DeployState.trader` stays the primary, for the pages that still
//! read a single wallet.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, RwLock};

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
use super::readiness::{Readiness, SlotFacts};

/// The name an unregistered env wallet runs under.
pub const ENV_WALLET_NAME: &str = "main";

pub struct Slot {
    pub name: String,
    pub trader: Arc<PumpFunTrader>,
    pub signer: Arc<Keypair>,
    pub nonces: Vec<Pubkey>,
    /// Why `initialize` failed. The trader then refuses every buy and sell.
    pub init_error: Option<String>,
}

/// `WALLET_PRIVATE_KEY` and `NONCE_ACCOUNTS`, kept so the Wallets page can
/// bring them into the registry.
#[derive(Clone)]
pub struct EnvWallet {
    pub pubkey: String,
    pub nonces: Vec<Pubkey>,
}

/// The env trader built at boot, before the registry is read.
pub struct EnvTrader {
    pub trader: Arc<PumpFunTrader>,
    pub signer: Arc<Keypair>,
    pub nonces: Vec<Pubkey>,
    pub init_error: Option<String>,
}

#[derive(Clone)]
pub struct TraderFactory {
    pub rpc_url: String,
    pub sender_urls: Vec<String>,
    pub fees: FeeTuning,
}

impl TraderFactory {
    fn config(&self, signer: Arc<dyn Signer + Send + Sync>, nonces: Vec<Pubkey>) -> TraderConfig {
        let mut cfg = TraderConfig::new(
            self.rpc_url.clone(),
            self.sender_urls.clone(),
            signer,
            nonces,
        );
        cfg.jito.min_sol = self.fees.jito_min_tip_sol;
        cfg.jito.max_sol = self.fees.jito_max_tip_sol;
        cfg.jito.percentile = self.fees.jito_tip_percentile;
        cfg.compute.price_micro_lamports = self.fees.cu_price_micro_lamports;
        // The push feed keeps the blockhash ~400 ms fresh and re-arms nonce slots,
        // so the pollers are stall fallbacks: a 10 s watchdog tick, a 30 s max age
        // inside the ~60 s hash window, and a 2 s first delay so the nonce push wins.
        cfg.cache.blockhash_refresh_ms = 10_000;
        cfg.cache.blockhash_max_age_ms = 30_000;
        cfg.nonce.refresh_first_delay_ms = 2_000;
        // The Sender drops an idle socket after 10 s; the pinger keeps it warm.
        cfg.sender.keep_warm_ms = Some(SENDER_KEEP_WARM_MS);
        cfg
    }

    /// A failed init is kept as the slot's error, never fatal: the trader then
    /// fail-closes every trade on its own and the rest of the process stays up.
    pub async fn build(
        &self,
        signer: Arc<dyn Signer + Send + Sync>,
        nonces: Vec<Pubkey>,
    ) -> (Arc<PumpFunTrader>, Option<String>) {
        let mut trader = PumpFunTrader::new(Arc::new(self.config(signer, nonces)));
        let init_error = match trader.initialize().await {
            Ok(()) => None,
            Err(e) => {
                tracing::error!("wallet trader init failed, that wallet stays fail-closed: {e:#}");
                Some(format!("{e:#}"))
            }
        };
        (Arc::new(trader), init_error)
    }
}

pub struct WalletRuntime {
    slots: RwLock<HashMap<String, Slot>>,
    primary: RwLock<String>,
    env: RwLock<Option<EnvWallet>>,
    /// Registry rows that are archived. The env key never revives one.
    archived: RwLock<HashSet<String>>,
    pub directory: Arc<WalletDirectory>,
    pub freeze: Arc<WalletFreeze>,
    factory: TraderFactory,
    keystore_dir: Option<PathBuf>,
    kek: Option<Kek>,
    /// The push feed's account list, set once ingest is wired. Every change to
    /// the loaded wallets or their nonces is published here.
    account_watch: OnceLock<tokio::sync::watch::Sender<Vec<String>>>,
    /// One move advances at a time, whether the driver or a request runs it.
    pub move_lock: tokio::sync::Mutex<()>,
}

impl WalletRuntime {
    fn empty(factory: TraderFactory, keystore_dir: Option<PathBuf>, kek: Option<Kek>) -> Self {
        Self {
            slots: RwLock::new(HashMap::new()),
            primary: RwLock::new(String::new()),
            env: RwLock::new(None),
            archived: RwLock::new(HashSet::new()),
            directory: Arc::new(WalletDirectory::default()),
            freeze: Arc::new(WalletFreeze::new()),
            factory,
            keystore_dir,
            kek,
            account_watch: OnceLock::new(),
            move_lock: tokio::sync::Mutex::new(()),
        }
    }

    /// Hand the push feed's sender over; later wallet changes resubscribe it.
    pub fn set_account_watch(&self, tx: tokio::sync::watch::Sender<Vec<String>>) {
        let _ = self.account_watch.set(tx);
    }

    fn publish_watch(&self) {
        if let Some(tx) = self.account_watch.get() {
            let next = self.watch_accounts();
            tx.send_if_modified(|current| {
                if *current == next {
                    return false;
                }
                *current = next;
                true
            });
        }
    }

    /// The env key alone, with no keystore: one slot, no files.
    pub fn legacy(env: EnvTrader, factory: TraderFactory) -> Self {
        let runtime = Self::empty(factory, None, None);
        runtime.adopt_env(env);
        runtime
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
        Self::empty(
            TraderFactory {
                rpc_url: String::new(),
                sender_urls: Vec::new(),
                fees: FeeTuning::defaults(),
            },
            None,
            None,
        )
    }

    /// Run the env key unless the registry already loaded that pubkey or
    /// archived it. It becomes primary only when the registry names none.
    pub fn adopt_env(&self, env: EnvTrader) {
        let pubkey = env.signer.pubkey().to_string();
        *self.env.write().expect("wallets") = Some(EnvWallet {
            pubkey: pubkey.clone(),
            nonces: env.nonces.clone(),
        });
        if self.archived.read().expect("wallets").contains(&pubkey)
            || self.slots.read().expect("wallets").contains_key(&pubkey)
        {
            return;
        }
        self.slots.write().expect("wallets").insert(
            pubkey.clone(),
            Slot {
                name: ENV_WALLET_NAME.to_string(),
                trader: env.trader,
                signer: env.signer,
                nonces: env.nonces,
                init_error: env.init_error,
            },
        );
        if self.primary_pubkey().is_empty() {
            self.note_primary(&pubkey);
        }
    }

    /// The env key's pubkey and nonce accounts, when `WALLET_PRIVATE_KEY` is set.
    pub fn env_wallet(&self) -> Option<EnvWallet> {
        self.env.read().expect("wallets").clone()
    }

    /// With no primary named anywhere, the first loaded wallet by name.
    pub fn fallback_primary(&self) {
        if !self.primary_pubkey().is_empty() {
            return;
        }
        let first = {
            let slots = self.slots.read().expect("wallets");
            slots
                .iter()
                .min_by(|a, b| a.1.name.cmp(&b.1.name))
                .map(|(pubkey, _)| pubkey.clone())
        };
        if let Some(pubkey) = first {
            self.note_primary(&pubkey);
        }
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

    /// Every loaded wallet's trader, by name.
    pub fn loaded_traders(&self) -> Vec<Arc<PumpFunTrader>> {
        let slots = self.slots.read().expect("wallets");
        let mut rows: Vec<(&String, &Slot)> = slots.iter().collect();
        rows.sort_by(|a, b| a.1.name.cmp(&b.1.name));
        rows.into_iter().map(|(_, s)| s.trader.clone()).collect()
    }

    pub fn signer(&self, pubkey: &str) -> Option<Arc<Keypair>> {
        self.slots.read().expect("wallets").get(pubkey).map(|s| s.signer.clone())
    }

    pub fn nonces(&self, pubkey: &str) -> Option<Vec<Pubkey>> {
        self.slots.read().expect("wallets").get(pubkey).map(|s| s.nonces.clone())
    }

    pub fn init_error(&self, pubkey: &str) -> Option<String> {
        self.slots.read().expect("wallets").get(pubkey).and_then(|s| s.init_error.clone())
    }

    /// Drop an archived wallet's trader. Its key file stays; a restore reloads it.
    pub fn unload(&self, pubkey: &str) {
        self.slots.write().expect("wallets").remove(pubkey);
        self.archived.write().expect("wallets").insert(pubkey.to_string());
        self.publish_watch();
    }

    /// An archived wallet back in service: its key from the file, its nonces from the registry.
    pub async fn restore(&self, pubkey: &str, name: &str, key_ref: &str, nonces: Vec<Pubkey>) -> Result<()> {
        let dir = self.keystore_dir.as_ref().context("WALLET_KEYSTORE is not set")?;
        let kek = self.kek.as_ref().context("WALLET_KEK is not set")?;
        let keypair = keystore::read_keypair(dir, key_ref, kek)?;
        if keypair.pubkey().to_string() != pubkey {
            bail!("keystore file {key_ref} does not match pubkey {pubkey}");
        }
        self.archived.write().expect("wallets").remove(pubkey);
        self.insert_loaded(pubkey.to_string(), name.to_string(), Arc::new(keypair), nonces, false)
            .await;
        Ok(())
    }

    pub fn rename(&self, pubkey: &str, name: &str) {
        if let Some(slot) = self.slots.write().expect("wallets").get_mut(pubkey) {
            slot.name = name.to_string();
        }
    }

    /// Whether `pubkey` can sign the next entry, given its cached balance.
    pub fn readiness(&self, pubkey: &str, lamports: Option<u64>) -> Readiness {
        let slots = self.slots.read().expect("wallets");
        let facts = slots.get(pubkey).map(|s| SlotFacts {
            key_matches: s.signer.pubkey().to_string() == pubkey,
            nonce_count: s.nonces.len(),
            init_failed: s.init_error.is_some(),
        });
        Readiness::of(facts.as_ref(), lamports)
    }

    /// The pubkey a position was bought with and the trader that sees that
    /// wallet's bag. A paper sentinel or an empty field means `fallback`'s wallet;
    /// a wallet not loaded here keeps its pubkey (the `trades` net is still that
    /// wallet's) and signs nothing new.
    pub fn for_position(
        &self,
        recorded: &str,
        fallback: &Arc<PumpFunTrader>,
    ) -> (String, Arc<PumpFunTrader>) {
        if recorded.is_empty() || recorded == "paper" {
            return (fallback.wallet_pubkey(), fallback.clone());
        }
        let trader = self.trader(recorded).unwrap_or_else(|| fallback.clone());
        (recorded.to_string(), trader)
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
        // Sorted so an unchanged set compares equal and never resubscribes.
        out.sort_unstable();
        out.dedup();
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
            self.note_primary(&primary.pubkey);
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
        let (trader, init_error) = self.factory.build(signer.clone(), nonces.clone()).await;
        self.slots.write().expect("wallets").insert(
            pubkey.clone(),
            Slot {
                name,
                trader: trader.clone(),
                signer,
                nonces,
                init_error,
            },
        );
        if primary || self.primary_pubkey().is_empty() {
            self.note_primary(&pubkey);
        }
        self.publish_watch();
        trader
    }

    pub async fn rebuild(&self, pubkey: &str, nonces: Vec<Pubkey>) -> Result<()> {
        let (signer, name) = {
            let slots = self.slots.read().expect("wallets");
            let slot = slots.get(pubkey).context("wallet is not loaded")?;
            (slot.signer.clone(), slot.name.clone())
        };
        let (trader, init_error) = self.factory.build(signer.clone(), nonces.clone()).await;
        self.slots.write().expect("wallets").insert(
            pubkey.to_string(),
            Slot {
                name,
                trader,
                signer,
                nonces,
                init_error,
            },
        );
        self.publish_watch();
        Ok(())
    }
}

/// Load every active and draining wallet. `None` when the keystore is off, so
/// the env key keeps signing alone. The primary stays empty when no row claims
/// it; the caller adopts the env key, then [`WalletRuntime::fallback_primary`].
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
    let runtime = WalletRuntime::empty(factory, Some(dir.clone()), Some(kek));
    *runtime.archived.write().expect("wallets") = records
        .iter()
        .filter(|r| r.status == "archived")
        .map(|r| r.pubkey.clone())
        .collect();
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
    // `insert_loaded` names the first loaded wallet primary; only a row may.
    if !records.iter().any(|r| r.is_primary && r.status != "archived") {
        runtime.note_primary("");
    }
    runtime.reload_directory(pool).await?;
    Ok(Some(runtime))
}
