//! ANALYSIS composition root — workstation only (never ships to EC2).
//!
//! Owns the analysis workload: `lake-export` (seal PG days → Parquet), sweeps,
//! backtests, wallet/bot analytics, simulate, and a thin HTTP surface. Runs with
//! NO signing keys and NO gRPC. Links the LAB half of the dep partition (`lake`)
//! and deliberately NOT ingest-host / launcher / pump-trader / ingest-laserstream.
//! Bodies land Phase 7+; this compiles today so the dep partition is enforced from
//! commit 1.

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    tracing::info!(
        lamports_per_sol = platform_core::units::LAMPORTS_PER_SOL,
        "lab bin scaffold — lake/sweeps/backtests/analytics wired in Phase 7+"
    );

    // Phase 7+: dotenv load → Settings → DbPools (local mirror) → thin HTTP +
    // lake-export subcommand. NO ingest, NO trader, NO keys.
    todo!("Phase 7: lab composition root (lake-export + analysis HTTP)")
}
