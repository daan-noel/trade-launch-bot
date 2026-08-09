//! Guard tests for the Docker build cache contract (`deploy/*/api.Dockerfile`).
//!
//! The four Rust images all mount the same paths, so their cache settings only
//! read as correct when you look at all four at once — which is exactly why a
//! regression here is invisible in review and shows up as "the cache stopped
//! working". No DB, no network; runs on every `cargo test`.
//!
//! Rationale for each invariant: deploy/hunter-live/api.Dockerfile's header.

#![cfg(test)]

use std::path::{Path, PathBuf};

/// Every Rust image built from the monorepo workspace.
const API_DOCKERFILES: [&str; 4] = [
    "deploy/hunter-live/api.Dockerfile",
    "deploy/hunter-lab/api.Dockerfile",
    "deploy/forge-live/api.Dockerfile",
    "deploy/forge-lab/api.Dockerfile",
];

/// Repo root, derived from this crate's manifest dir (`<root>/hunter/core`).
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("hunter/core is two levels below the repo root")
        .to_path_buf()
}

fn read(rel: &str) -> String {
    let path = repo_root().join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("deploy guard cannot read {}: {e}", path.display()))
}

#[test]
fn each_image_owns_a_distinct_target_cache() {
    let mut seen: Vec<(String, &str)> = Vec::new();

    for path in API_DOCKERFILES {
        let text = read(path);
        let mounts: Vec<&str> = text
            .lines()
            .map(str::trim)
            .filter(|l| l.contains("type=cache") && l.contains("target=/app/target"))
            .collect();
        assert!(
            !mounts.is_empty(),
            "{path}: no /app/target cache mount found — the guard is looking at the wrong thing"
        );

        for mount in mounts {
            let id = mount
                .split(',')
                .find_map(|f| f.trim().strip_prefix("id="))
                .map(|f| f.split_whitespace().next().unwrap_or(f).to_string())
                .unwrap_or_else(|| {
                    panic!(
                        "{path}: the /app/target cache mount has no `id=`. BuildKit defaults a \
                         cache id to its TARGET PATH, so all four Rust images would share ONE \
                         target dir; they resolve different feature sets over the same deps, so \
                         each build invalidates the previous one's artifacts — a permanent \
                         recompile ping-pong. Give this image its own id."
                    )
                });
            if let Some((_, other)) = seen.iter().find(|(seen_id, o)| *seen_id == id && *o != path)
            {
                panic!(
                    "{path} and {other} both use target cache id '{id}'. Each image needs its \
                     own, or their builds thrash each other's compiled dependencies."
                );
            }
            seen.push((id, path));
        }
    }
}

#[test]
fn the_shared_cargo_caches_are_not_locked() {
    // A `sharing=locked` mount holds its lock for the WHOLE RUN step, not just
    // while cargo touches it. On the registry/git mounts — which are shared
    // across all four images on purpose (same crates, one download) — that
    // serialises every concurrent image build behind whichever started first,
    // turning compose's parallel build into a sum of build times. Cargo does its
    // own locking on the registry (.package-cache), so shared access is safe.
    for path in API_DOCKERFILES {
        for line in read(path).lines().map(str::trim) {
            if !line.contains("type=cache") {
                continue;
            }
            let is_shared_cargo_cache = line.contains("target=/usr/local/cargo/registry")
                || line.contains("target=/usr/local/cargo/git");
            assert!(
                !(is_shared_cargo_cache && line.contains("sharing=locked")),
                "{path}: `sharing=locked` on a cross-image cargo cache serialises concurrent \
                 image builds for the full duration of the RUN. Drop it (the default, \
                 sharing=shared, is safe here):\n  {line}"
            );
        }
    }
}
