//! Which pubkey signs the next entry. The position keeps the pubkey it was
//! bought with; this map is read only when an entry fires.

use std::collections::HashMap;
use std::sync::RwLock;
use uuid::Uuid;

#[derive(Default)]
struct State {
    primary: String,
    by_rule: HashMap<Uuid, String>,
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
