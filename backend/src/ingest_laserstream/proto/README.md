# Geyser protobufs (LaserStream ingest)

`geyser.proto` + `solana-storage.proto` are the Yellowstone Geyser definitions
(vendored from `yellowstone-grpc-proto`), plus one local addition:
`SubscribeRequest.from_slot = 11` (reconnect replay).

The Rust bindings are **committed** under `../generated/` (`geyser.rs`,
`solana.storage.confirmed_block.rs`, `mod.rs`). There is **no build-time `protoc`
step** — the runtime only needs `tonic` + `prost`. This is deliberate: the
`yellowstone-grpc-proto` crate force-builds `protoc` from C++ source
(`protobuf-src`, needs `make`/`sh`), which fails on Windows. Committing the
generated code makes the build work everywhere with no toolchain.

## Regenerating (only when a `.proto` changes)

Codegen runs once in a Linux container (so the host needs nothing but Docker).

1. Create a throwaway generator crate at `C:\Users\X\protogen` (outside the
   workspace so it doesn't join it):

   `Cargo.toml`

   ```toml
   [package]
   name = "protogen"
   version = "0.0.0"
   edition = "2021"
   publish = false

   [dependencies]
   tonic-build = "=0.10.2"   # match the backend's tonic 0.10 / prost 0.12
   prost-build = "=0.12.6"
   ```

   `src/main.rs`

   ```rust
   fn main() -> Result<(), Box<dyn std::error::Error>> {
       let mut config = prost_build::Config::new();
       config.include_file("mod.rs"); // self-wiring nested module tree
       tonic_build::configure()
           .build_server(false) // client only
           .out_dir("/out")
           .compile_with_config(config, &["/proto/geyser.proto"], &["/proto"])?;
       eprintln!("OK: generated Geyser bindings into /out");
       Ok(())
   }
   ```

2. Run the generator (PowerShell; adjust the repo path if needed):

   ```powershell
   docker run --rm -e CARGO_TARGET_DIR=/tmp/target `
     -v "C:\Users\X\protogen:/gen" `
     -v "f:\pumpfun\meme-trading\backend\src\ingest_laserstream\proto:/proto:ro" `
     -v "f:\pumpfun\meme-trading\backend\src\ingest_laserstream\generated:/out" `
     -w /gen rust:1-bookworm `
     bash -c "apt-get update -qq && apt-get install -y -qq protobuf-compiler && cargo run --quiet"
   ```

3. `cargo check -p backend` to confirm the new bindings compile.
