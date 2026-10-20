//! Which pubkey signs the next entry. The position keeps the pubkey it was
//! bought with; this map is read only when an entry fires.

use std::collections::{HashMap, HashSet};
use std::sync::RwLock;
use uuid::Uuid;

#[derive(Default)]
struct State {
    primary: String,
    by_rule: HashMap<Uuid, String>,
    /// Wallets that take no new entry. Exits never read this.
    paused: HashSet<String>,
}

#[derive(Default)]
pub struct WalletDirectory {
    inner: RwLock<State>,
}

impl WalletDirectory {
    pub fn set_primary(&self, pubkey: &str) {
        self.inner.write().expect("wallet directory").primary = pubkey.to_string();
    }

    pub fn primary(&self) -> String {
        self.inner.read().expect("wallet directory").primary.clone()
    }

    pub fn replace_assignments(&self, rows: Vec<(Uuid, String)>) {
        self.inner.write().expect("wallet directory").by_rule = rows.into_iter().collect();
    }

    pub fn assign(&self, rule: Uuid, wallet: Option<String>) {
        let mut guard = self.inner.write().expect("wallet directory");
        match wallet {
            Some(wallet) => {
                guard.by_rule.insert(rule, wallet);
            }
            None => {
                guard.by_rule.remove(&rule);
            }
        }
    }

    pub fn replace_paused(&self, paused: HashSet<String>) {
        self.inner.write().expect("wallet directory").paused = paused;
    }

    pub fn set_paused(&self, pubkey: &str, paused: bool) {
        let mut guard = self.inner.write().expect("wallet directory");
        if paused {
            guard.paused.insert(pubkey.to_string());
        } else {
            guard.paused.remove(pubkey);
        }
    }

    pub fn is_paused(&self, pubkey: &str) -> bool {
        self.inner.read().expect("wallet directory").paused.contains(pubkey)
    }

    /// The pubkey an entry on `rule` signs with. Empty when no wallet is loaded.
    pub fn resolve(&self, rule: Uuid) -> String {
        let guard = self.inner.read().expect("wallet directory");
        guard
            .by_rule
            .get(&rule)
            .cloned()
            .unwrap_or_else(|| guard.primary.clone())
    }
}
