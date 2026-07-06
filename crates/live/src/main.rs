//! LIVE composition root — ships to EC2 (2vCPU / 4GB RAM).
//!
//! Owns the live workload: ingest (`ingest-host` over the borrowed
//! `ingest-laserstream` transport) + launcher + trading + a thin HTTP surface,
//! each a long-lived task under a `tokio::select!`. Links the LIVE half of the dep
//! partition (ingest-host + launcher, pulling pump-trader + ingest-laserstream) and
//! deliberately NOT `lake`/DuckDB. Bodies land Phase 6+; this compiles today so the
//! member graph + dep partition are enforced from commit 1.

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    // Sanity: prove the borrowed unit SSOT links from the live bin.
    tracing::info!(
        lamports_per_sol = platform_core::units::LAMPORTS_PER_SOL,
        "live bin scaffold — ingest/launcher/trading/HTTP wired in Phase 6+"
    );

    // Phase 6+: dotenv load → Settings → DbPools → tokio::select! over
    // ingest-host::spawn_ingest + launcher + HTTP server.
    todo!("Phase 6: live composition root (ingest → raw_txs/trades round-trip)")
}
