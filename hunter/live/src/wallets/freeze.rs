//! Wallets whose exits wait. A move sets this for the source while bags move,
//! then clears it. An exit that was already decided waits, then reads the
//! position row again and signs with whatever wallet holds the bag.

use std::collections::HashSet;
use std::sync::Mutex;
use tokio::sync::Notify;

pub struct WalletFreeze {
    frozen: Mutex<HashSet<String>>,
    notify: Notify,
}

impl WalletFreeze {
    pub fn new() -> Self {
        Self {
            frozen: Mutex::new(HashSet::new()),
            notify: Notify::new(),
        }
    }

    pub fn freeze(&self, wallet: &str) {
        self.frozen.lock().expect("wallet freeze").insert(wallet.to_string());
    }

    pub fn thaw(&self, wallet: &str) {
        self.frozen.lock().expect("wallet freeze").remove(wallet);
        self.notify.notify_waiters();
    }

    pub fn is_frozen(&self, wallet: &str) -> bool {
        self.frozen.lock().expect("wallet freeze").contains(wallet)
    }

    pub async fn wait(&self, wallet: &str) {
        loop {
            let notified = self.notify.notified();
            if !self.frozen.lock().expect("wallet freeze").contains(wallet) {
                return;
            }
            notified.await;
        }
    }
}

impl Default for WalletFreeze {
    fn default() -> Self {
        Self::new()
    }
}
