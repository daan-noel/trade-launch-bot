# 2026-08-17 — Parallel image builds corrupted the shared cargo registry cache

## Symptom

`docker compose up -d --build` (four images at once: hunter live/lab api+ui) failed in
`deploy/hunter-lab/api.Dockerfile`'s `cargo build --release --bin hunter-lab`:

```
error: failed to unpack package `zerovec v0.11.6`
Caused by: failed to open `/usr/local/cargo/registry/src/index.crates.io-*/zerovec-0.11.6/.cargo-ok`
Caused by: File exists (os error 17)
```

## Cause

The cache contract shared the crate download cache across all four Rust images by mounting
`/usr/local/cargo/registry` and `/usr/local/cargo/git`, and justified `sharing=shared` with
"cargo does its own locking on the registry (`.package-cache`)". Cargo does — but that lock
file lives at `$CARGO_HOME/.package-cache`, i.e. `/usr/local/cargo/.package-cache`, one level
ABOVE the two mounted subdirectories. Each build container therefore got its own private lock
file while sharing the directory the lock exists to protect. Two builds unpacked the same
crate into the same path concurrently; whichever lost the race hit `O_EXCL` on `.cargo-ok`.

Nothing in the failure pointed at concurrency, and a serial rebuild of the same image
succeeded, which read as a transient network/disk glitch.

## Fix

One cache mount over the whole `CARGO_HOME` (`ENV CARGO_HOME=/cargo`, mount `target=/cargo,
id=cargo-home`) in all four `deploy/*/api.Dockerfile`s, so `.package-cache` (plus
`.package-cache-mutate` and `.global-cache`, which had the same problem) sits inside the
shared cache and cargo's own locking works across containers. `sharing=shared` stayed:
cargo's lock covers only download/unpack, while `sharing=locked` would hold the mount for
the whole RUN and serialise every concurrent image build.

Because `CARGO_HOME` is now a cache mount, the chef stage installs cargo-chef with
`--root /usr/local` — a plain `cargo install` would drop the binary into the (non-persisted)
mount. `/usr/local/cargo` itself stays unmounted: it holds the rustup proxies PATH resolves
`cargo` through.

`config::deploy_guard` in hunter-core enforced the old rule with a test asserting registry/git
mounts were never `sharing=locked`; it now asserts the CARGO_HOME shape instead.

## Cleanup after the incident

Changing the mount to `/cargo` (id `cargo-home`) orphans the old
`/usr/local/cargo/registry` cache, so the poisoned unpack never gets read again — the next
build re-downloads the crates once. The orphan holds ~1-2 GB; leave it rather than reaching
for `docker builder prune --filter type=exec.cachemount`, which also deletes the target/
caches and buys a full cold recompile of libduckdb.

The per-image `target/` caches keep their ids, so the invalidated `cargo chef cook` layer
re-runs against already-compiled artifacts instead of rebuilding the dependency tree.

Staying on the old mount instead would have needed the poisoned entry dropped by hand — a
losing unpack can leave a half-written crate dir that the winner already marked `.cargo-ok`,
which resurfaces later as a bogus compile error:

```sh
printf 'FROM busybox
RUN --mount=type=cache,target=/usr/local/cargo/registry rm -rf /usr/local/cargo/registry/src
' | docker build --no-cache -
```

Dropping `registry/src` (the unpacked tree) forces a re-unpack from the `.crate` tarballs
still in `registry/cache` — no re-download.
