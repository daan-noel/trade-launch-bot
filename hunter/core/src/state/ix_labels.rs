//! Instruction-list text for a hash the live ingest just computed.
//!
//! The engine's trade row keeps the hash, not the strings. A volume list is stored
//! as those strings, so the ingest remembers each new hash here. One entry per
//! distinct list, capped, so a busy tape does not grow this without bound. A hash
//! that has aged out is skipped by the fingerprint write; the next print of that
//! list can queue it again only if the label still holds and the hash is remembered.

use std::collections::{HashMap, VecDeque};
use std::sync::{LazyLock, Mutex};

const CAP: usize = 2048;

struct Cache {
    order: VecDeque<u64>,
    map: HashMap<u64, Vec<String>>,
}

static CACHE: LazyLock<Mutex<Cache>> = LazyLock::new(|| {
    Mutex::new(Cache {
        order: VecDeque::new(),
        map: HashMap::new(),
    })
});

/// Remember `labels` for `hash` the first time this process sees it.
pub fn remember(hash: u64, labels: &[String]) {
    if labels.is_empty() {
        return;
    }
    let mut g = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if g.map.contains_key(&hash) {
        return;
    }
    if g.order.len() >= CAP {
        if let Some(old) = g.order.pop_front() {
            g.map.remove(&old);
        }
    }
    g.order.push_back(hash);
    g.map.insert(hash, labels.to_vec());
}

/// The labels stored for `hash`, if this process still has them.
pub fn labels(hash: u64) -> Option<Vec<String>> {
    let g = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    g.map.get(&hash).cloned()
}
