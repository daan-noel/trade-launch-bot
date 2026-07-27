//! Deterministic resolution of **directory paths that arrive from the env**.
//!
//! A relative path in `.env` (`EVENT_LOG_DIR=./event_log`) is otherwise resolved
//! against the process **CWD**, which is not stable: `dotenvy::dotenv()` walks *up*
//! the tree to find `hunter/.env`, so `cargo run -p hunter-live` picks up the same
//! `.env` from `Bot/`, `Bot/hunter/`, or `Bot/hunter/live/` — and then writes the
//! log into three different directories. The live recorder and the lab replay
//! inspector read the same key from two different bins, so they silently disagreed
//! on where the log lives.
//!
//! The anchor makes it stable: a relative path is joined to **the directory holding
//! the `.env` that was actually loaded**. Rules:
//!
//! * absolute path ⇒ used verbatim (Docker sets `EVENT_LOG_DIR=/var/lib/hunter/...`)
//! * relative + a `.env` was loaded ⇒ joined to that file's parent (⇒ `hunter/`)
//! * relative + no `.env` (the container image excludes it — see `.dockerignore`)
//!   ⇒ left CWD-relative, i.e. the image's fixed `WORKDIR /app`
//!
//! Install once per bin, immediately after `dotenvy`, via [`install_dotenv_anchor`].

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Directory of the `.env` `dotenvy` loaded at boot. Unset ⇒ no anchor (tests,
/// and the container, where no `.env` is baked into the image).
static ANCHOR: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Record the loaded `.env` so relative env paths resolve against it. Pass the
/// value of `dotenvy::dotenv().ok()` straight through. Call once after `dotenvy`
/// in each bin's `main`; a second call is ignored (first wins) so probes and
/// re-entrant boot paths don't panic.
pub fn install_dotenv_anchor(dotenv_path: Option<PathBuf>) {
    let dir = dotenv_path.and_then(|p| p.parent().map(Path::to_path_buf));
    let _ = ANCHOR.set(dir);
}

/// Resolve one env-supplied path against the anchor (see the module docs). Takes a
/// `Path` so an `OsString` straight out of `env::var_os` needs no lossy conversion.
pub fn resolve_path(raw: impl AsRef<Path>) -> PathBuf {
    let raw = raw.as_ref();
    if raw.is_absolute() {
        return raw.to_path_buf();
    }
    match ANCHOR.get().and_then(Option::as_ref) {
        Some(anchor) => anchor.join(raw),
        None => raw.to_path_buf(),
    }
}

/// [`resolve_path`] for a string value, trimming stray whitespace first (env values
/// and request fields both pick it up).
pub fn resolve(raw: &str) -> PathBuf {
    resolve_path(raw.trim())
}

/// Read `key` from the env (falling back to `default`) and [`resolve`] it. The
/// one entry point for "a directory configured in `.env`".
pub fn dir_from_env(key: &str, default: &str) -> PathBuf {
    resolve(&std::env::var(key).unwrap_or_else(|_| default.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Absolute input is never rewritten — the container path must survive intact
    /// whether or not an anchor was installed.
    #[test]
    fn absolute_passes_through() {
        let abs = if cfg!(windows) { "C:/var/lib/hunter/event_log" } else { "/var/lib/hunter/event_log" };
        assert_eq!(resolve(abs), PathBuf::from(abs));
    }

    /// With no anchor (the in-image case) a relative path stays CWD-relative, so
    /// the fixed `WORKDIR /app` still yields `/app/event_log`.
    #[test]
    fn relative_without_anchor_is_unchanged() {
        // `ANCHOR` is process-wide; this test only asserts the un-anchored branch,
        // which is what an uninstalled OnceLock yields.
        if ANCHOR.get().and_then(Option::as_ref).is_none() {
            assert_eq!(resolve("./event_log"), PathBuf::from("./event_log"));
        }
    }
}
