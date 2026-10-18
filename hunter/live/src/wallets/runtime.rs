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
use trading_core::models::trader_wallet::TraderWalletRecord;
use trading_core::storage::repositories::trader_wallet_repo::TraderWalletRepo;

use super::directory::WalletDirectory;
use super::freeze::WalletFreeze;
use super::keystore::{self, Kek};
use super::readiness::{Readiness, SlotFacts};

pub struct Slot {
    pub name: String,
    pub trader: Arc<PumpFunTrader>,
    pub signer: Arc<Keypair>,
    pub nonces: Vec<Pubkey>,
    /// Why `initialize` failed. The trader then refuses every buy and sell.
    pub init_error: Option<String>,
}

#[derive(Clone)]
pub struct TraderFactory {
    pub rpc_url: String,
    pub sender_urls: Vec<String>,
    pub fees: FeeTuning,
}

impl TraderFactory {
    /// A trader on a throwaway key that is never initialized, so every buy,
    /// sell and claim bails at its "Not initialized" guard. It stands in for
    /// the process trader while no wallet is loaded; it makes no RPC call.
    fn inert(&self) -> Arc<PumpFunTrader> {
        Arc::new(PumpFunTrader::new(Arc::new(self.config(Arc::new(Keypair::new()), Vec::new()))))
    }

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
    /// Active rows whose key did not load on this box, and why. That wallet
    /// signs nothing; every other wallet runs.
    load_errors: RwLock<HashMap<String, String>>,
    /// Registry rows that are archived.
    archived: RwLock<HashSet<String>>,
    /// The process trader while no wallet is loaded. Signs nothing.
    inert: Arc<PumpFunTrader>,
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
            load_errors: RwLock::new(HashMap::new()),
            inert: factory.inert(),
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

    pub fn primary_pubkey(&self) -> String {
        self.primary.read().expect("wallets").clone()
    }

    /// The wallet unnamed rules resolve to. The next entry sees it; open bags do not.
    pub fn note_primary(&self, pubkey: &str) {
        self.directory.set_primary(pubkey);
        *self.primary.write().expect("wallets") = pubkey.to_string();
    }

    /// Why an active row's key did not load on this box.
    pub fn load_error(&self, pubkey: &str) -> Option<String> {
        self.load_errors.read().expect("wallets").get(pubkey).cloned()
    }

    /// The process trader the single-wallet pages are built with at boot: the
    /// primary, else any loaded wallet, else an inert trader that signs
    /// nothing. Entries never fall back to it; with no primary named, an entry
    /// on a rule without a wallet fails closed.
    pub fn primary_trader(&self) -> Arc<PumpFunTrader> {
        let pubkey = self.primary_pubkey();
        if let Some(trader) = self.trader(&pubkey) {
            return trader;
        }
        let slots = self.slots.read().expect("wallets");
        slots.values().next().map_or_else(|| self.inert.clone(), |s| s.trader.clone())
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

    /// Run `f` on every loaded wallet's trader, under the read lock and without
    /// allocating. The per-trade feed hooks fan out through this, so `f` must
    /// not block or touch the wallet registry.
    pub fn for_each_trader(&self, mut f: impl FnMut(&Arc<PumpFunTrader>)) {
        for slot in self.slots.read().expect("wallets").values() {
            f(&slot.trader);
        }
    }

    /// Release `position_id`'s SOL earmark on whichever trader holds it. The
    /// earmark lives on the trader of the wallet that signed the buy, and a
    /// release through any other trader is a silent no-op that strands that
    /// wallet's `max_committed_sol` budget until restart.
    pub fn release_commitment(&self, position_id: &str) {
        self.for_each_trader(|trader| trader.release_sol_for_position(position_id));
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

    /// The pubkey a position was bought with and the trader that signs for and
    /// reads that wallet's bag. A paper sentinel or an empty field means
    /// `fallback`'s wallet. A wallet not loaded here keeps its pubkey (the
    /// `trades` net is still that wallet's) and has no trader: nothing signs for
    /// it or reads its token accounts until it loads.
    pub fn for_position(
        &self,
        recorded: &str,
        fallback: &Arc<PumpFunTrader>,
    ) -> (String, Option<Arc<PumpFunTrader>>) {
        if recorded.is_empty() || recorded == "paper" {
            return (fallback.wallet_pubkey(), Some(fallback.clone()));
        }
        (recorded.to_string(), self.trader(recorded))
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
        // Only a row names the primary. None named leaves it empty, and an
        // entry on a rule without a wallet then fails closed.
        let primary = repo
            .records()
            .await?
            .into_iter()
            .find(|r| r.is_primary && r.status != "archived")
            .map(|r| r.pubkey)
            .unwrap_or_default();
        self.note_primary(&primary);
        Ok(())
    }

    /// Bring the loaded set in line with the registry without a restart:
    /// load rows that are new here (a sync, another session), unload archived
    /// ones, follow renames and nonce sets, and re-read the primary and the
    /// rule assignments. Holds `move_lock` so a move never sees it half done.
    pub async fn reload(&self, pool: &PgPool) -> Result<()> {
        let _move = self.move_lock.lock().await;
        let repo = TraderWalletRepo::new(pool.clone());
        let records = repo.records().await?;
        for record in &records {
            if record.status == "archived" {
                if self.slots.read().expect("wallets").contains_key(&record.pubkey) {
                    self.unload(&record.pubkey);
                }
                self.archived.write().expect("wallets").insert(record.pubkey.clone());
                continue;
            }
            self.archived.write().expect("wallets").remove(&record.pubkey);
            let nonces = record_nonces(&repo, &record.pubkey).await?;
            let loaded = self.slots.read().expect("wallets").get(&record.pubkey).map(|s| s.nonces.clone());
            match loaded {
                Some(running) => {
                    self.rename(&record.pubkey, &record.name);
                    let same: HashSet<Pubkey> = running.into_iter().collect();
                    if same != nonces.iter().copied().collect::<HashSet<_>>() {
                        self.rebuild(&record.pubkey, nonces).await?;
                    }
                }
                None => self.load_record(record, nonces).await,
            }
        }
        self.reload_directory(pool).await
    }

    /// Decrypt one row's key and start its trader. A key that is missing,
    /// does not decrypt, or names another pubkey is kept as that wallet's
    /// load error; nothing else stops.
    async fn load_record(&self, record: &TraderWalletRecord, nonces: Vec<Pubkey>) {
        match self.read_record_key(record) {
            Ok(keypair) => {
                self.load_errors.write().expect("wallets").remove(&record.pubkey);
                self.insert_loaded(record.pubkey.clone(), record.name.clone(), Arc::new(keypair), nonces, false)
                    .await;
            }
            Err(e) => {
                tracing::error!(wallet = %record.pubkey, name = %record.name, "wallet key did not load, that wallet signs nothing: {e:#}");
                self.load_errors.write().expect("wallets").insert(record.pubkey.clone(), format!("{e:#}"));
            }
        }
    }

    fn read_record_key(&self, record: &TraderWalletRecord) -> Result<Keypair> {
        let dir = self.keystore_dir.as_ref().context("WALLET_KEYSTORE is not set")?;
        let kek = self.kek.as_ref().context("WALLET_KEK is not set")?;
        let keypair = keystore::read_keypair(dir, &record.key_ref, kek)?;
        if keypair.pubkey().to_string() != record.pubkey {
            bail!("keystore file {} does not match pubkey {}", record.key_ref, record.pubkey);
        }
        Ok(keypair)
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
        if primary {
            self.note_primary(&pubkey);
        }
        self.publish_watch();
        trader
    }

    pub async fn rebuild(&self, pubkey: &str, nonces: Vec<Pubkey>) -> Result<()> {
        let (signer, name, old) = {
            let slots = self.slots.read().expect("wallets");
            let slot = slots.get(pubkey).context("wallet is not loaded")?;
            (slot.signer.clone(), slot.name.clone(), slot.trader.clone())
        };
        let (trader, init_error) = self.factory.build(signer.clone(), nonces.clone()).await;
        // The SOL earmarks of this wallet's open positions live on the trader. A fresh
        // trader starts at zero, so carry them over or `max_committed_sol` forgets them.
        for entry in old.position_commitments.iter() {
            trader.commit_sol_for_position(entry.key().clone(), *entry.value());
        }
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

/// The keystore directory and passphrase from the environment. Both are
/// required: every key this process signs with lives in the keystore.
pub fn keystore_env() -> Result<(PathBuf, Kek)> {
    let dir = match std::env::var("WALLET_KEYSTORE") {
        Ok(dir) if !dir.trim().is_empty() => trading_core::config::env_paths::resolve(&dir),
        _ => bail!("WALLET_KEYSTORE is not set"),
    };
    let passphrase = std::env::var("WALLET_KEK").context("WALLET_KEK is not set")?;
    Ok((dir, Kek::from_passphrase(&passphrase)))
}

async fn record_nonces(repo: &TraderWalletRepo, pubkey: &str) -> Result<Vec<Pubkey>> {
    repo.nonces_for(pubkey)
        .await?
        .into_iter()
        .map(|n| n.pubkey.parse::<Pubkey>())
        .collect::<Result<Vec<_>, _>>()
        .context("nonce pubkey")
}

/// Load every active and draining wallet. A wallet whose key does not load
/// is shown with its error and signs nothing. With no wallet loaded the
/// process still boots: ingest, paper trading and the pages run, and every
/// real entry fails closed until a wallet is imported or created.
pub async fn load_keystore(pool: &PgPool, factory: TraderFactory) -> Result<WalletRuntime> {
    let (dir, kek) = keystore_env()?;
    let runtime = WalletRuntime::empty(factory, Some(dir), Some(kek));
    runtime.reload(pool).await?;
    if runtime.is_empty() {
        let errors = runtime.load_errors.read().expect("wallets").len();
        tracing::error!(
            unloaded = errors,
            "no wallet is loaded; real entries fail closed until one is imported or created on the Wallets page"
        );
    }
    Ok(runtime)
}

/// One wallet's trader alone, by name or pubkey, else the primary. `probe`
/// signs with it; no other wallet is decrypted.
pub async fn load_one(pool: &PgPool, factory: TraderFactory, wanted: Option<&str>) -> Result<Arc<PumpFunTrader>> {
    let (dir, kek) = keystore_env()?;
    let repo = TraderWalletRepo::new(pool.clone());
    let records = repo.records().await?;
    let record = match wanted {
        Some(w) => records
            .iter()
            .find(|r| r.pubkey == w || keystore::same_name(&r.name, w))
            .with_context(|| format!("no wallet named {w}"))?,
        None => records
            .iter()
            .find(|r| r.is_primary && r.status != "archived")
            .context("no primary wallet; pass --wallet <name>")?,
    };
    if record.status == "archived" {
        bail!("wallet {} is archived", record.name);
    }
    let runtime = WalletRuntime::empty(factory, Some(dir), Some(kek));
    let keypair = runtime.read_record_key(record)?;
    let nonces = record_nonces(&repo, &record.pubkey).await?;
    let (trader, init_error) = runtime.factory.build(Arc::new(keypair), nonces).await;
    if let Some(e) = init_error {
        bail!("wallet {} trader init failed: {e}", record.name);
    }
    Ok(trader)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime() -> WalletRuntime {
        let factory = TraderFactory {
            rpc_url: "http://127.0.0.1:1".into(),
            sender_urls: Vec::new(),
            fees: FeeTuning::defaults(),
        };
        WalletRuntime::empty(factory, None, None)
    }

    /// A position whose wallet is not loaded gets no trader, never the
    /// fallback: the fallback's key would sign a sell that cannot move this
    /// bag, and its token accounts would read the bag as gone.
    #[tokio::test]
    async fn unloaded_position_wallet_gets_no_trader() {
        let rt = runtime();
        let fallback = rt.factory.inert();
        let absent = Pubkey::new_unique().to_string();

        let (wallet, trader) = rt.for_position(&absent, &fallback);
        assert_eq!(wallet, absent);
        assert!(trader.is_none());

        for sentinel in ["", "paper"] {
            let (wallet, trader) = rt.for_position(sentinel, &fallback);
            assert_eq!(wallet, fallback.wallet_pubkey());
            assert!(trader.is_some_and(|t| Arc::ptr_eq(&t, &fallback)));
        }

        let loaded = rt.factory.inert();
        let pubkey = loaded.wallet_pubkey();
        rt.slots.write().unwrap().insert(
            pubkey.clone(),
            Slot {
                name: "w".into(),
                trader: loaded.clone(),
                signer: Arc::new(Keypair::new()),
                nonces: Vec::new(),
                init_error: None,
            },
        );
        let (_, trader) = rt.for_position(&pubkey, &fallback);
        assert!(trader.is_some_and(|t| Arc::ptr_eq(&t, &loaded)));
    }

    fn load_inert(rt: &WalletRuntime, name: &str) -> Arc<PumpFunTrader> {
        let trader = rt.factory.inert();
        rt.slots.write().unwrap().insert(
            trader.wallet_pubkey(),
            Slot {
                name: name.into(),
                trader: trader.clone(),
                signer: Arc::new(Keypair::new()),
                nonces: Vec::new(),
                init_error: None,
            },
        );
        trader
    }

    /// A buy signed by a non-primary wallet earmarks SOL on THAT wallet's
    /// trader. Releasing through the primary alone is a no-op, and six such
    /// leaks pinned a wallet at its `max_committed_sol` ceiling for good.
    #[tokio::test]
    async fn release_commitment_frees_the_signing_wallets_earmark() {
        let rt = runtime();
        let primary = load_inert(&rt, "primary");
        let other = load_inert(&rt, "other");
        let pg_id = uuid::Uuid::new_v4().to_string();

        other.commit_sol_for_position(pg_id.clone(), 50_000_000);
        primary.release_sol_for_position(&pg_id);
        assert_eq!(other.committed_lamports(), 50_000_000, "the primary cannot release it");

        rt.release_commitment(&pg_id);
        assert_eq!(other.committed_lamports(), 0);
        assert_eq!(primary.committed_lamports(), 0);
        rt.release_commitment(&pg_id);
        assert_eq!(other.committed_lamports(), 0, "idempotent");
    }
}