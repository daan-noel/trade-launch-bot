//! **Tags** — named trade lists a fingerprint keeps. Each tag splits a coin's trades in
//! two: the trades that carry it (`@volume`) and the rest (`@!volume`).
//!
//! A fingerprint's `tags` document names each tag and how a trade qualifies
//! ([`config`]); every coin the fingerprint matches then keeps one [`state::TagState`]
//! per tag a loaded rule reads. A tag is a list, not a meaning: `volume` (the dev's
//! volume-making trades), `dump` (the dev's dump sells), `targets` (wallets to copy) and
//! `working` (tool templates) are all the same thing with different matchers.
//!
//! Two built-in wallet classes need no config and are read only by
//! `m_holdings.bag_share_pct`: [`BUNDLED`] and [`PUBLIC_APP`].

pub mod config;
pub mod state;
#[cfg(test)]
mod state_tests;

use crate::hash::{fnv1a_bytes, FNV_OFFSET};

/// Wallets whose first buy of the coin landed in a slot where 3+ wallets made their
/// first buy with one ix shape.
pub const BUNDLED: &str = "bundled";
/// Wallets whose first buy went through an app with more than 100 buyers the day before.
pub const PUBLIC_APP: &str = "public_app";
/// The built-in wallet classes. A fingerprint may not define a tag with these names.
pub const BUILTIN: [&str; 2] = [BUNDLED, PUBLIC_APP];

/// Longest tag name.
pub const MAX_NAME_LEN: usize = 24;

/// A tag's identity inside one fingerprint: a hash of its name, so a read is a map
/// probe on two integers, never a string compare.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TagKey(pub u64);

impl TagKey {
    pub fn of(name: &str) -> Self {
        Self(fnv1a_bytes(FNV_OFFSET, name.as_bytes()))
    }
}

/// A condition's tag: which list, and which half of the split.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TagRef {
    pub key: TagKey,
    /// The name as written, interned — for labels and errors.
    pub name: &'static str,
    /// `@!name`: the trades WITHOUT the tag.
    pub negated: bool,
}

impl TagRef {
    /// Parse `volume` or `!volume`.
    pub fn parse(s: &str) -> Result<Self, String> {
        let s = s.trim();
        let (negated, name) = match s.strip_prefix('!') {
            Some(rest) => (true, rest.trim()),
            None => (false, s),
        };
        check_name(name)?;
        Ok(Self { key: TagKey::of(name), name: crate::intern::intern(name), negated })
    }

    /// `volume` / `!volume` — the JSON value.
    pub fn text(self) -> String {
        if self.negated {
            format!("!{}", self.name)
        } else {
            self.name.to_string()
        }
    }

    /// `@volume` / `@!volume` — how a label shows it.
    pub fn at(self) -> String {
        format!("@{}", self.text())
    }

    pub fn is_builtin(self) -> bool {
        BUILTIN.contains(&self.name)
    }
}

/// A tag name: `[a-z0-9_]`, 1 to [`MAX_NAME_LEN`] characters.
pub fn check_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > MAX_NAME_LEN {
        return Err(format!("tag name `{name}` must be 1 to {MAX_NAME_LEN} characters"));
    }
    if !name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_') {
        return Err(format!("tag name `{name}` may use only a-z, 0-9 and _"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_refs_parse_both_halves() {
        let t = TagRef::parse("volume").unwrap();
        assert!(!t.negated);
        assert_eq!(t.at(), "@volume");
        let n = TagRef::parse("!volume").unwrap();
        assert!(n.negated);
        assert_eq!(n.key, t.key);
        assert_eq!(n.at(), "@!volume");
        for bad in ["", "!", "Volume", "a b", "x".repeat(25).as_str()] {
            assert!(TagRef::parse(bad).is_err(), "{bad}");
        }
    }
}
