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
fn the_shared_cargo_cache_covers_cargo_home() {
    // Cargo's package-cache lock lives at $CARGO_HOME/.package-cache, one level
    // ABOVE registry/ and git/. Mounting only those two shares the crate unpack
    // dir across concurrently building images WITHOUT the lock that guards it,
    // and two builds then unpack the same crate on top of each other:
    //   failed to unpack package `zerovec v0.11.6` ... .cargo-ok: File exists.
    // One mount over the whole CARGO_HOME puts the lock inside the shared cache.
    for path in API_DOCKERFILES {
        let text = read(path);

        assert!(
            text.contains("ENV CARGO_HOME=/cargo"),
            "{path}: no `ENV CARGO_HOME=/cargo`. The cargo cache mount only guards \
             concurrent builds when it covers CARGO_HOME, lock file included."
        );
        assert!(
            text.contains("cargo install cargo-chef --locked --root /usr/local"),
            "{path}: install cargo-chef with `--root /usr/local`. With CARGO_HOME on a \
             cache mount, a plain `cargo install` drops the binary into that mount, which \
             is not persisted into the image layer."
        );

        let mut cargo_home_mounts = 0;
        for line in text.lines().map(str::trim) {
            if !line.contains("type=cache") {
                continue;
            }
            assert!(
                !line.contains("target=/usr/local/cargo/"),
                "{path}: mount CARGO_HOME (/cargo) itself, not a subdirectory of it — a \
                 registry/ or git/ mount shares the crate unpack dir without cargo's \
                 .package-cache lock, so parallel image builds corrupt each other:\n  {line}"
            );
            let mounts_cargo_home = line
                .split(',')
                .any(|f| f.trim().split_whitespace().next() == Some("target=/cargo"));
            if !mounts_cargo_home {
                continue;
            }
            cargo_home_mounts += 1;
            assert!(
                line.contains("id=cargo-home"),
                "{path}: the CARGO_HOME cache mount needs `id=cargo-home` so all four images \
                 share ONE crate download:\n  {line}"
            );
            assert!(
                !line.contains("sharing=locked"),
                "{path}: `sharing=locked` on the shared CARGO_HOME cache holds the mount for \
                 the full RUN, serialising every concurrent image build. Cargo's own lock \
                 already covers download/unpack — keep the default sharing=shared:\n  {line}"
            );
        }
        assert!(
            cargo_home_mounts >= 2,
            "{path}: expected the CARGO_HOME cache mount on every cargo RUN (chef install, \
             chef cook, build), found {cargo_home_mounts}"
        );
    }
}
