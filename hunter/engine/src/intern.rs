//! A process-wide string interner for the few names a compiled rule carries by value —
//! tag names and line labels — so the types that hold them stay `Copy` and a hot-path
//! read never clones a string.
//!
//! Called only while parsing a rule or a fingerprint (cold), never per event. Each
//! distinct string is leaked once and reused forever; the set of names a process ever
//! sees is the set of names its rules spell, which is small and bounded by authoring.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

fn table() -> &'static Mutex<HashSet<&'static str>> {
    static TABLE: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(HashSet::new()))
}

/// The one `&'static str` for `s`.
pub fn intern(s: &str) -> &'static str {
    let mut t = table().lock().unwrap_or_else(|p| p.into_inner());
    if let Some(&hit) = t.get(s) {
        return hit;
    }
    let leaked: &'static str = Box::leak(s.to_owned().into_boxed_str());
    t.insert(leaked);
    leaked
}

#[cfg(test)]
mod tests {
    #[test]
    fn one_pointer_per_string() {
        let a = super::intern("volume");
        let b = super::intern(&String::from("volume"));
        assert!(std::ptr::eq(a, b));
    }
}
