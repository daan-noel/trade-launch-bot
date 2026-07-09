# Restructure inventory + rename map (captured at Part 0.2)

Snapshot of the workspace BEFORE the hunter/forge restructure, and the target
rename map. Diff every rename against this checklist.

## Convention chosen for Part 1 (no-behavior-change reshuffle)
- Rename `[package] name` **only**. Keep each crate's `[lib] name` / `[[bin]] name`
  target unchanged so the 120+ source `use trading_core::` / `use platform_core::`
  references need **zero edits**.
- In dependents, keep the existing dependency **key** and add `package = "<new-name>"`
  + updated `path`. (Cargo: import name = dependency key, so `use` paths stay valid.)
- Bin names (`live`, `lab`, `slp-live`, `slp-lab`) are globally unique -> kept as-is,
  so Docker `--bin` flags stay valid.

## Package rename map
| old folder | old pkg | new folder | new pkg | lib/bin target (unchanged) |
|---|---|---|---|---|
| meme-trading/trading_core | trading_core | hunter/core | hunter-core | lib trading_core |
| meme-trading/live | live | hunter/live | hunter-live | bin live |
| meme-trading/lab | lab | hunter/lab | hunter-lab | bin lab |
| meme-trading/ingest-websocket | ingest-websocket | shared/ingest/websocket | ingest-websocket | lib ingest_websocket |
| solana-launch-platform/crates/platform-core | platform-core | forge/core | forge-core | lib platform_core |
| solana-launch-platform/crates/slp-live | slp-live | forge/live | forge-live | bin slp-live |
| solana-launch-platform/crates/slp-lab | slp-lab | forge/lab | forge-lab | bin slp-lab |
| solana-launch-platform/crates/launcher | launcher | forge/launcher | forge-launcher | lib launcher |
| solana-launch-platform/crates/lake | lake | forge/lab/src/lake (module) | — folded — | — |
| solana-launch-platform/crates/ingest-host | ingest-host | forge/live/src/ingest (module) | — folded — | — |
| shared/pump-trader | pump-trader | shared/pump-trader (Part 2 splits) | pump-trader | lib pump_trader |
| shared/ingest-laserstream | ingest-laserstream | shared/ingest-laserstream (Part 3 splits) | ingest-laserstream | lib ingest_laserstream |

## Path-deps to repoint
- meme-trading/ingest-websocket -> trading_core (../trading_core)
- meme-trading/live -> trading_core, ingest-laserstream (../../shared), pump-trader (../../shared)
- meme-trading/lab -> trading_core
- SLP ingest-host/lake/launcher/slp-lab/slp-live -> platform-core, lake, ingest-host, launcher
- workspace.dependencies: pump-trader, ingest-laserstream (paths unchanged in Part 1)

## Bins (deploy `--bin` flags — names preserved)
- live, lab, slp-live, slp-lab

## Secrets relocated out of repo (Part 0.3)
- meme-trading/aws-ec2-key.pem -> ~/.ssh/aws-ec2-key.pem (was untracked/ignored)
- solana-launch-platform/wallet-backups/ -> ~/restructure-secrets-offline/wallet-backups/
  (managed_wallets.json + README.txt were **tracked** -> git rm --cached; .gitignore now `wallet-backups/`)
- solana-launch-platform/keystore/ -> LEFT IN PLACE (already gitignored; read by the
  app at runtime, moving it offline would break local signing). Flagged deviation.
