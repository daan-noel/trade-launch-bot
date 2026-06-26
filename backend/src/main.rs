mod sweep;
mod api;
pub use backend_core::{config, models};
pub use config::constants as constants;
mod ingest_laserstream;
mod analyzers;
mod state;
mod storage;
mod services;
mod strategies;
mod trader;

use anyhow::Context;
use solana_sdk::signature::Keypair;
use std::sync::Arc;
use tracing::{error, info, warn};

use actix_cors::Cors;
use actix_web::body::{EitherBody, MessageBody};
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::middleware::{from_fn, Next};
use actix_web::{web, App, HttpResponse, HttpServer};
use crate::trader::{PumpFunTrader, TraderConfig};

/// Optional bearer token gating mutating API requests, shared via `app_data`.
/// `None` disables the check entirely (the default, current behaviour).
#[derive(Clone)]
struct ApiAuth {
    token: Option<String>,
}

/// Middleware: require `Authorization: Bearer <token>` on mutating requests.
/// Preflight (OPTIONS) and safe reads (GET/HEAD) always pass. The check is
/// **fail-closed**: a mutating request is rejected when `API_AUTH_TOKEN` is not
/// configured, so a forgotten env var blocks real-SOL trades rather than
/// exposing them. `API_AUTH_TOKEN` is required at startup (see `Settings`), so
/// the `None` arm is only reachable if the server is somehow started without it.
async fn require_bearer_auth(
    req: ServiceRequest,
    next: Next<impl MessageBody + 'static>,
) -> Result<ServiceResponse<EitherBody<impl MessageBody>>, actix_web::Error> {
    use actix_web::http::{header::AUTHORIZATION, Method};

    let mutating = matches!(
        *req.method(),
        Method::POST | Method::PUT | Method::DELETE | Method::PATCH
    );
    let configured = req
        .app_data::<web::Data<ApiAuth>>()
        .and_then(|a| a.token.clone());

    let authorized = match (mutating, configured) {
        // Safe reads + preflight always pass.
        (false, _) => true,
        // Mutating + token configured → must match exactly.
        (true, Some(expected)) => req
            .headers()
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
            .map(|t| t == expected)
            .unwrap_or(false),
        // Mutating + NO token configured → deny (fail closed).
        (true, None) => false,
    };

    if authorized {
        Ok(next.call(req).await?.map_into_left_body())
    } else {
        let resp = HttpResponse::Unauthorized()
            .json(serde_json::json!({ "error": "unauthorized" }));
        Ok(req.into_response(resp).map_into_right_body())
    }
}

fn parse_wallet_keypair(base58_key: &str) -> anyhow::Result<Keypair> {
    let bytes = bs58::decode(base58_key)
        .into_vec()
        .context("Failed to decode WALLET_PRIVATE_KEY as base58")?;
    Keypair::from_bytes(&bytes)
        .context("Failed to construct Keypair from WALLET_PRIVATE_KEY bytes")
}

/// One-shot, no-/low-SOL probes for the tx-latency changes. Runs after the
/// trader is initialized (so the tip cache and nonce slots are warm) and exits
/// before any DB/ingest/HTTP startup. Subcommands:
///   probe ladder [levels]                      — Jito tip escalation ladder
///                                                (read-only, zero SOL)
///   probe fanout [lamports] [--tip] [--confirm] — fan-out self-transfer to all
///                                                sender endpoints (base fee only;
///                                                senders require --tip to accept)
///   probe check-nonces                         — read-only audit that every
///                                                configured nonce account's
///                                                authority is the wallet (zero SOL)
///   probe simulate-sell <mint> [amount] [slippage_bps] [--cashback]
///                                              — simulate a real curve sell
///                                                against live state (zero SOL)
///   probe simulate-buy <mint> <sol> [slippage_bps]
///                                              — simulate a real curve buy
///                                                against live state (zero SOL)
///   probe simulate-amm-buy <mint> <sol> [slippage_bps]
///                                              — simulate a real PumpSwap AMM buy
///                                                of a migrated token (zero SOL)
///   probe simulate-amm-sell <mint> [amount] [slippage_bps]
///                                              — simulate a real PumpSwap AMM sell
///                                                of a migrated token (zero SOL)
async fn run_probe(trader: &PumpFunTrader, args: Vec<String>) -> anyhow::Result<()> {
    const LPS: f64 = pump_trader::constants::LAMPORTS_PER_SOL as f64;
    match args.first().map(String::as_str).unwrap_or("") {
        "ladder" => {
            let levels: u8 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(5);
            let ladder = trader.probe_tip_ladder(levels).await?;
            println!("Jito tip escalation ladder (live tip-floor):");
            for (lvl, lamports) in ladder {
                println!(
                    "  level {lvl}: {lamports:>9} lamports  ({:.6} SOL)",
                    lamports as f64 / LPS
                );
            }
        }
        "fanout" => {
            let lamports: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);
            let include_tip = args.iter().any(|a| a == "--tip");
            let do_confirm = args.iter().any(|a| a == "--confirm");
            println!(
                "Fan-out self-transfer: {lamports} lamports, tip={include_tip}, confirm={do_confirm}"
            );
            let report = trader
                .probe_fanout_self_transfer(lamports, include_tip, do_confirm)
                .await?;
            for r in &report.results {
                match &r.outcome {
                    Ok(sig) => println!("  ✅ {:>5}ms  {}  -> {sig}", r.elapsed_ms, r.url),
                    Err(e) => println!("  ❌ {:>5}ms  {}  -> {e}", r.elapsed_ms, r.url),
                }
            }
            match (report.confirm_ms, &report.confirmed) {
                (Some(ms), Some(Ok(()))) => println!("  confirmed in {ms}ms"),
                (Some(ms), Some(Err(e))) => println!("  confirm failed in {ms}ms: {e}"),
                _ => {}
            }
        }
        "check-nonces" => {
            // Read-only audit of the FULL nonce pool: confirms every configured
            // nonce account's authority is the current wallet (so a re-auth
            // landed everywhere, not just on the slots a fan-out probe used).
            let wallet = trader.wallet_pubkey();
            let checks = trader.check_nonce_authorities().await;
            println!("Nonce authorization audit — wallet {wallet}:");
            let mut ok = 0usize;
            for c in &checks {
                match (&c.error, c.matches_wallet, c.authority) {
                    (Some(e), _, _) => println!("  ❌ {}  ERROR: {e}", c.pubkey),
                    (None, true, _) => {
                        ok += 1;
                        println!("  ✅ {}  authority == wallet", c.pubkey);
                    }
                    (None, false, Some(auth)) => {
                        println!("  ❌ {}  authority = {auth}  (NOT the wallet)", c.pubkey)
                    }
                    (None, false, None) => println!("  ❌ {}  not an initialized nonce", c.pubkey),
                }
            }
            println!("  {ok}/{} nonce accounts authorized to the wallet", checks.len());
        }
        "holdings" => {
            let holdings = trader.get_all_token_accounts().await?;
            if holdings.is_empty() {
                println!("Wallet holds no token accounts.");
            } else {
                println!("Wallet holdings ({}):", holdings.len());
                for h in holdings {
                    println!(
                        "  {}  {} raw ({} UI)  acct={}",
                        h.mint, h.amount, h.ui_amount, h.token_account
                    );
                }
            }
        }
        "simulate-sell" => {
            let mint = args
                .get(1)
                .context("usage: probe simulate-sell <mint> [amount] [slippage_bps] [--cashback]")?;
            // Positional numeric args after the mint: [0] = amount (raw base
            // units), [1] = slippage_bps. Both optional; `--flags` are skipped.
            let positional: Vec<&String> =
                args.iter().skip(2).filter(|s| !s.starts_with("--")).collect();
            // Amount defaults to the wallet's full on-chain balance so the sim
            // mirrors a real full exit.
            let amount: u64 = match positional.first() {
                Some(s) => s.parse().context("amount must be a u64 (raw base units)")?,
                None => {
                    let bal = trader.get_token_balance(&trader.wallet_pubkey(), mint).await?;
                    println!(
                        "No amount given — using on-chain balance: {} raw ({} UI)",
                        bal.amount, bal.ui_amount
                    );
                    if bal.amount == 0 {
                        anyhow::bail!("wallet holds 0 of {mint} — nothing to simulate");
                    }
                    bal.amount
                }
            };
            let slippage_bps: Option<u64> = match positional.get(1) {
                Some(s) => Some(s.parse().context("slippage_bps must be a u64")?),
                None => None,
            };
            let is_cashback = args.iter().any(|a| a == "--cashback");
            println!("Simulating curve SELL — slippage_bps: {slippage_bps:?}");
            let outcome = trader
                .simulate_curve_sell(mint, amount, slippage_bps, is_cashback)
                .await?;
            print_sim_outcome(&outcome);
        }
        "simulate-buy" => {
            let mint = args
                .get(1)
                .context("usage: probe simulate-buy <mint> <sol> [slippage_bps]")?;
            let positional: Vec<&String> =
                args.iter().skip(2).filter(|s| !s.starts_with("--")).collect();
            let sol: f64 = positional
                .first()
                .context("usage: probe simulate-buy <mint> <sol> [slippage_bps]")?
                .parse()
                .context("sol must be a float")?;
            let slippage_bps: Option<u64> = match positional.get(1) {
                Some(s) => Some(s.parse().context("slippage_bps must be a u64")?),
                None => None,
            };
            println!("Simulating curve BUY — {sol} SOL, slippage_bps: {slippage_bps:?}");
            let outcome = trader.simulate_curve_buy(mint, sol, slippage_bps).await?;
            print_sim_outcome(&outcome);
        }
        "simulate-amm-buy" => {
            let mint = args
                .get(1)
                .context("usage: probe simulate-amm-buy <mint> <sol> [slippage_bps]")?;
            let positional: Vec<&String> =
                args.iter().skip(2).filter(|s| !s.starts_with("--")).collect();
            let sol: f64 = positional
                .first()
                .context("usage: probe simulate-amm-buy <mint> <sol> [slippage_bps]")?
                .parse()
                .context("sol must be a float")?;
            let slippage_bps: Option<u64> = match positional.get(1) {
                Some(s) => Some(s.parse().context("slippage_bps must be a u64")?),
                None => None,
            };
            // Resolve the token's SPL program (legacy/2022) on-chain — same source
            // the live AMM buy uses. Warn (don't block) if the mint isn't migrated:
            // the sim itself is the source of truth, and a non-AMM mint just reverts.
            let routing = trader.resolve_buy_routing(mint).await?;
            if !routing.is_migrated {
                println!("⚠️  {mint} resolves as NOT migrated — use `simulate-buy` for a curve buy.");
            }
            println!(
                "Simulating AMM BUY — {sol} SOL, token_program={}, slippage_bps: {slippage_bps:?}",
                routing.token_program_id
            );
            let outcome = trader
                .simulate_amm_buy(mint, &routing.token_program_id, sol, None, slippage_bps)
                .await?;
            print_sim_outcome(&outcome);
        }
        "simulate-amm-sell" => {
            let mint = args
                .get(1)
                .context("usage: probe simulate-amm-sell <mint> [amount] [slippage_bps]")?;
            let positional: Vec<&String> =
                args.iter().skip(2).filter(|s| !s.starts_with("--")).collect();
            let routing = trader.resolve_buy_routing(mint).await?;
            if !routing.is_migrated {
                println!("⚠️  {mint} resolves as NOT migrated — use `simulate-sell` for a curve sell.");
            }
            // Amount defaults to the wallet's full on-chain balance (mirrors a real
            // full exit), same as the curve `simulate-sell`.
            let amount: u64 = match positional.first() {
                Some(s) => s.parse().context("amount must be a u64 (raw base units)")?,
                None => {
                    let bal = trader.get_token_balance(&trader.wallet_pubkey(), mint).await?;
                    println!(
                        "No amount given — using on-chain balance: {} raw ({} UI)",
                        bal.amount, bal.ui_amount
                    );
                    if bal.amount == 0 {
                        anyhow::bail!("wallet holds 0 of {mint} — nothing to simulate");
                    }
                    bal.amount
                }
            };
            let slippage_bps: Option<u64> = match positional.get(1) {
                Some(s) => Some(s.parse().context("slippage_bps must be a u64")?),
                None => None,
            };
            println!(
                "Simulating AMM SELL — token_program={}, slippage_bps: {slippage_bps:?}",
                routing.token_program_id
            );
            let outcome = trader
                .simulate_amm_sell(mint, amount, &routing.token_program_id, None, None, slippage_bps)
                .await?;
            print_sim_outcome(&outcome);
        }
        "cashback-status" => {
            let pots = trader.cashback_status().await?;
            println!("Cashback status (read-only):");
            let mut total_wsol = 0u64;
            for p in &pots {
                let claimable = p.claimable();
                total_wsol += claimable;
                println!(
                    "  [{}] uva={} exists={}",
                    p.label, p.pda, p.exists
                );
                println!(
                    "      cashback: earned={} claimed={} -> claimable={} lamports ({:.6} SOL)",
                    p.cashback_earned,
                    p.total_cashback_claimed,
                    claimable,
                    claimable as f64 / LPS
                );
                let stable = p.stable_claimable();
                if p.stable_earned != 0 || stable != 0 {
                    println!(
                        "      stable:   earned={} claimed={} -> claimable={} (raw stable-mint units)",
                        p.stable_earned, p.stable_claimed, stable
                    );
                }
            }
            println!(
                "  TOTAL claimable WSOL cashback: {total_wsol} lamports ({:.6} SOL)",
                total_wsol as f64 / LPS
            );
        }
        "claim-cashback" => {
            let execute = args.iter().any(|a| a == "--execute");
            println!(
                "Cashback claim ({}):",
                if execute { "EXECUTE — sending" } else { "simulate-only" }
            );
            let outcomes = trader.claim_cashback(execute).await?;
            if outcomes.is_empty() {
                println!("  Nothing claimable in either pot — nothing to do.");
            }
            for o in &outcomes {
                if o.is_stable {
                    println!(
                        "  [{}] claimable={} raw stable-mint units (claimed as an SPL balance)",
                        o.label, o.claimable
                    );
                } else {
                    println!(
                        "  [{}] claimable={} lamports ({:.6} SOL)",
                        o.label,
                        o.claimable,
                        o.claimable as f64 / LPS
                    );
                }
                match (&o.err, &o.signature, o.simulated) {
                    (None, _, true) => println!(
                        "      ✅ simulation passed — CU consumed: {:?}",
                        o.units_consumed
                    ),
                    (Some(e), _, true) => println!("      ❌ simulation reverted: {e}"),
                    (None, Some(sig), false) => println!("      ✅ sent — sig: {sig}"),
                    (Some(e), _, false) => println!("      ❌ send failed: {e}"),
                    (None, None, false) => println!("      ⚠️  sent but no signature returned"),
                }
                if o.simulated && !o.logs.is_empty() {
                    println!("      --- logs ---");
                    for line in &o.logs {
                        println!("        {line}");
                    }
                }
            }
        }
        other => anyhow::bail!(
            "unknown probe '{other}'. Use: ladder | fanout | simulate-buy | simulate-sell | \
             simulate-amm-buy | simulate-amm-sell | holdings | cashback-status | \
             claim-cashback [--execute] | compact-sweeps [tpsl1|tpsl2]"
        ),
    }
    Ok(())
}

/// Print a [`SimOutcome`] from the curve buy/sell simulation engine: pass/revert
/// line (with the custom program error code on a revert), per-account SOL/token
/// deltas, then the program logs.
fn print_sim_outcome(o: &pump_trader::SimOutcome) {
    const LPS: f64 = pump_trader::constants::LAMPORTS_PER_SOL as f64;
    if o.success {
        println!("✅ simulation passed — CU consumed: {:?}", o.units_consumed);
    } else {
        println!(
            "❌ simulation reverted: {}\n   custom error: {:?} | CU consumed: {:?}",
            o.err.as_deref().unwrap_or("?"),
            o.custom_error,
            o.units_consumed
        );
    }
    for d in &o.accounts {
        let sol = d.lamports_delta() as f64 / LPS;
        match d.token_delta() {
            Some(t) => println!(
                "  {}  SOL Δ {:+.9} ({:+} lamports)  token Δ {:+}",
                d.pubkey, sol, d.lamports_delta(), t
            ),
            None => println!(
                "  {}  SOL Δ {:+.9} ({:+} lamports)",
                d.pubkey, sol, d.lamports_delta()
            ),
        }
    }
    println!("--- logs ---");
    for line in &o.logs {
        println!("  {line}");
    }
}

/// One-time storage retention of the existing grouped-sweep `_results` rows.
/// `probe compact-sweeps [tpsl1|tpsl2]` (no arg = every strategy). For each group
/// it runs the same [`sweep::retention::retained_combo_ids`] the write path uses,
/// deletes the non-surviving combos in one statement, then `VACUUM (FULL)`s the
/// table to physically reclaim disk. Offline-only — the `VACUUM` takes an
/// `ACCESS EXCLUSIVE` lock. Idempotent: a second run finds the rows already
/// pruned and deletes nothing.
async fn run_compact_sweeps(db: &sqlx::PgPool, args: Vec<String>) -> anyhow::Result<()> {
    use storage::repositories::grouped_sweep_repo::GroupedSweepRepo;
    let cfg = sweep::retention::RetentionCfg::default();
    // Worst-case retained rows/group (11 metrics × (top+bottom) values × cap); a
    // group exceeding it signals a retention bug, so we flag it loudly.
    let bound = 11 * (cfg.top_n + cfg.bottom_n) * cfg.cap_per_value;

    let strategies: Vec<&str> = match args.first().map(String::as_str) {
        Some(s) if !s.is_empty() => vec![s],
        _ => sweep::registry::strategy_ids().to_vec(),
    };

    for sid in strategies {
        let tables = sweep::registry::tables_for(sid)
            .with_context(|| format!("unknown strategy '{sid}' (no grouped-sweep tables)"))?;
        let repo = GroupedSweepRepo::new(db.clone(), tables);
        let groups = repo.list_all_groups_for_compaction().await?;
        println!("compact-sweeps [{sid}]: {} group(s) in {}", groups.len(), tables.results);

        let (mut total_before, mut total_kept, mut max_kept) = (0u64, 0u64, 0usize);
        for (group_id, best_combo_id) in &groups {
            let metrics = repo.fetch_combo_metrics_for_group(*group_id).await?;
            let before = metrics.len();
            let keep = sweep::retention::retained_combo_ids(&metrics, *best_combo_id as u32, &cfg);
            let keep_ids: Vec<i32> = keep.iter().map(|&id| id as i32).collect();
            let deleted = repo.delete_combos_except(*group_id, &keep_ids).await?;
            let kept = before as u64 - deleted;

            total_before += before as u64;
            total_kept += kept;
            max_kept = max_kept.max(kept as usize);
            if kept as usize > bound {
                println!(
                    "  ⚠️  group {group_id}: kept {kept} > bound {bound} — retention may be miscounting"
                );
            }
        }
        println!(
            "  rows {total_before} -> {total_kept} (deleted {}, max kept/group {max_kept}, bound {bound})",
            total_before - total_kept
        );
        println!("  VACUUM (FULL, ANALYZE) {} — reclaiming disk (ACCESS EXCLUSIVE)…", tables.results);
        repo.vacuum_full_results().await?;
        println!("  ✅ {sid} done");
    }
    Ok(())
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> anyhow::Result<()> {
    // Load .env before anything else
    dotenvy::dotenv().ok();

    // Tracing / logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "backend=info,sqlx=error".into()),
        )
        .init();

    // Config
    let settings = config::Settings::from_env().context("Failed to load configuration")?;

    info!(
        host = %settings.host,
        port = settings.port,
        pump_program = constants::PUMP_FUN_PROGRAM_ID,
        "Configuration loaded"
    );

    // Compaction probe: `probe compact-sweeps [tpsl1|tpsl2]` is DB-only (no trader
    // / ingest / HTTP), so dispatch it here before the trader init's network calls.
    // One-time storage retention of the existing grouped-sweep rows; see
    // `run_compact_sweeps`.
    if std::env::args().nth(1).as_deref() == Some("probe")
        && std::env::args().nth(2).as_deref() == Some("compact-sweeps")
    {
        let pools = storage::postgres::connect(&settings).await?;
        return run_compact_sweeps(&pools.hot, std::env::args().skip(3).collect()).await;
    }

    let trader_config = Arc::new(TraderConfig {
        rpc_url: settings.helius_rpc_url.clone(),
        helius_sender_urls: settings.helius_sender_urls.clone(),
        keypair: parse_wallet_keypair(&settings.wallet_private_key)
            .context("Failed to parse trader wallet private key")?,
        nonce_accounts: settings.nonce_accounts.clone(),
    });

    let mut trader = PumpFunTrader::new(trader_config);
    trader
        .initialize()
        .await
        .context("Failed to initialize PumpFunTrader")?;
    let trader = Arc::new(trader);

    // Probe mode: `cargo run -p backend -- probe <subcommand>` runs a one-shot,
    // no-/low-SOL validation of the latency changes against live infra, then
    // exits before touching the DB / ingest / HTTP server. See `run_probe`.
    if std::env::args().nth(1).as_deref() == Some("probe") {
        return run_probe(&trader, std::env::args().skip(2).collect()).await;
    }

    // Database — connect the three workload-isolated pools and run migrations. `db`
    // (hot) backs ingest/strategy/maintenance/seed/caches; `api_db` backs the fast
    // HTTP handlers; `batch_db` backs the long DB-heavy jobs (sweeps + backtests) so
    // they can't starve the dashboard reads.
    let storage::postgres::DbPools {
        hot: db,
        api: api_db,
        batch: batch_db,
    } = storage::postgres::connect(&settings).await?;

    // Crash recovery (Phase 4): a killed process can leave a grouped sweep stuck at
    // `status = 'running'`. The single-flight gate allows one sweep at a time and
    // none can be live at boot, so any `running` run is an orphan — mark it
    // `cancelled` (its already-persisted groups stay) so the UI stops showing it as
    // live. Best-effort: a failure here is logged, not fatal.
    for strategy_id in sweep::registry::strategy_ids() {
        if let Some(tables) = sweep::registry::tables_for(strategy_id) {
            match storage::repositories::grouped_sweep_repo::GroupedSweepRepo::new(db.clone(), tables)
                .reconcile_orphaned_runs()
                .await
            {
                Ok(0) => {}
                Ok(n) => warn!("grouped sweep: marked {n} orphaned '{strategy_id}' run(s) cancelled"),
                Err(e) => error!("grouped sweep: orphaned-run reconcile for '{strategy_id}' failed: {e}"),
            }
        }
    }

    // Boot wallet-balance sweep (buy-in-flight recovery, Phase 3 backstop): list
    // the wallet's on-chain token accounts once and flag any balance no open
    // position across either clone accounts for — a manual transfer, a failed
    // marker persist, or any bug the durable `BuySubmitted` marker doesn't cover.
    // Read-only + advisory (logs for review, never sells/deletes), spawned off the
    // boot critical path so its RPC scan never delays ingest/HTTP startup.
    {
        let trader = trader.clone();
        let tpsl1_repo =
            storage::repositories::tpsl1_position_repo::Tpsl1PositionRepo::new(db.clone());
        let tpsl2_repo =
            storage::repositories::tpsl2_position_repo::Tpsl2PositionRepo::new(db.clone());
        tokio::spawn(async move {
            if let Err(e) =
                services::wallet_reconcile::reconcile_wallet_holdings(&trader, &tpsl1_repo, &tpsl2_repo)
                    .await
            {
                warn!("Boot wallet reconcile failed (advisory only): {e}");
            }
        });
    }

    // Background SOL-balance refresh: keeps `PumpFunTrader::can_commit_buy` accurate
    // without ever hitting the RPC on the hot buy path. Polls every 30 s; the first
    // tick fires immediately so the cache is non-empty before the first snipe fires.
    {
        let trader = trader.clone();
        tokio::spawn(async move {
            loop {
                match trader.get_sol_balance().await {
                    Ok(lamports) => trader.update_sol_balance_cache(lamports),
                    Err(e) => warn!("SOL balance refresh failed (guard stays on last cached value): {e}"),
                }
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            }
        });
    }

    // In-memory caches (shared between services and future API handlers)
    let token_cache = Arc::new(state::token_cache::TokenCache::new());

    // Seed the cache off the boot critical path: ingest/HTTP start immediately and
    // the cache hydrates in the background (build-then-insert keeps it race-safe vs
    // the live pipeline). A failure is logged, not fatal — the system still runs and
    // the cache fills from live events. See `storage::seed`.
    {
        let db = db.clone();
        let token_cache = token_cache.clone();
        tokio::spawn(async move {
            let started = std::time::Instant::now();
            match storage::seed::seed_token_cache(&db, token_cache).await {
                Ok(()) => info!("Cache seed task finished in {:?}", started.elapsed()),
                Err(e) => error!("Cache seed task failed (cache will fill from live events): {e}"),
            }
        });
    }

    let (sse_tx, _) = tokio::sync::broadcast::channel::<models::ingest::SseEvent>(512);

    // Load the persisted settings document and hold it in a watch channel as the
    // in-memory source of truth, so a policy set in a previous run is in force
    // before the first event arrives.
    let settings_repo = storage::repositories::settings_repo::SettingsRepo::new(db.clone());
    let app_settings = settings_repo
        .load_all()
        .await
        .context("Failed to load app settings")?;
    info!(
        track_mayhem = app_settings.track_mayhem,
        track_post_migration = app_settings.track_post_migration,
        live = app_settings.live,
        "Settings loaded"
    );

    // Seed live mode from the persisted toggle so a restart resumes the
    // operator's last on/off choice instead of always booting paused.
    let (live_tx, live_rx) = tokio::sync::watch::channel(app_settings.live);

    let (settings_tx, _) = tokio::sync::watch::channel(app_settings);

    let (sol_price_tx, _sol_price_rx) = tokio::sync::watch::channel::<Option<f64>>(None);
    let sol_price = Arc::new(sol_price_tx);
    let tpsl1_cache = Arc::new(strategies::tpsl_sniper_1::Tpsl1RuntimeCache::new(sse_tx.clone()));
    tpsl1_cache
        .load_from_db(&db)
        .await
        .context("Failed to load TPSL1 runtime cache")?;
    let tpsl2_cache = Arc::new(strategies::tpsl_sniper_2::Tpsl2RuntimeCache::new(sse_tx.clone()));
    tpsl2_cache
        .load_from_db(&db)
        .await
        .context("Failed to load TPSL2 runtime cache")?;

    // Shared (wallet, mint) wakeup hub: the DbWriter signals it once a trade is
    // persisted; the live buy/sell confirm loops await it instead of polling the
    // DB on a fixed timer (they keep their timeout as a fallback).
    let trade_signals = Arc::new(state::trade_signals::TradeSignals::new());

    // ── Ingest transport: LaserStream (Yellowstone gRPC) ──
    // Feeds the shared token cache / strategy / SSE / DB. Yields the pool→mint
    // index + migration signal (for DeployState), the strategy receiver, and the
    // long-lived task handles the supervising select watches.
    let (pool_index, pools_changed, strategy_rx, producer_task, pipeline_task, db_writer_task) = {
        info!("Ingest transport: LaserStream (gRPC)");
        let (db_tx, db_rx, strategy_tx, strategy_rx) =
            ingest_laserstream::pipeline::IngestPipeline::channel_pair();
        // Tier B: the pipeline channel carries the typed protobuf update (shared
        // via Arc), not a pre-built Helius `Value` — the blob synthesis moved
        // off the ingest hot path into the DbWriter. The client's curve-vs-AMM
        // relevance verdict rides along so the decoder doesn't re-scan the logs.
        // Buffer sized to absorb a short volume burst (migration wave / hot-token
        // spike) so it never stalls back to the gRPC socket (consumer-lag guard).
        let (update_tx, update_rx) = tokio::sync::mpsc::channel::<(
            std::sync::Arc<ingest_laserstream::proto::geyser::SubscribeUpdateTransaction>,
            ingest_laserstream::decoder::TxRelevance,
        )>(4096);

        // Weak sender handles for the queue-depth diagnostics logger — taken before
        // the senders are moved into the pipeline / client, and weak so the logger
        // never keeps a channel alive past ingest shutdown.
        let update_tx_weak = update_tx.downgrade();
        let db_tx_weak = db_tx.downgrade();
        let strategy_tx_weak = strategy_tx.downgrade();

        let pipeline = ingest_laserstream::pipeline::IngestPipeline::new(
            constants::PUMP_FUN_PROGRAM_ID.to_string(),
            token_cache.clone(),
            db_tx,
            strategy_tx,
            sse_tx.clone(),
            settings_tx.subscribe(),
            trader.clone(),
            trade_signals.clone(),
        );
        let pool_index = pipeline.pool_index();
        let pools_changed = pipeline.pools_changed();
        let shed_counters = pipeline.shed_counters();

        // End-to-end ingest liveness: the DbWriter stamps this on every committed
        // batch (real progress, measured at the sink). A dedicated OS-thread watchdog
        // force-exits the process only when the heartbeat goes stale *while the DB
        // write queue is backed up* — a genuine downstream `.await` wedge that task
        // supervision and the in-stream idle-reconnect both miss — so it self-heals
        // via restart, but a merely-slow writer or a quiet/idle queue never trips it.
        // A clone goes to the watchdog; the DbWriter takes the original (below).
        let heartbeat = state::ingest_health::IngestHeartbeat::new();
        let db_tx_pending = db_tx_weak.clone();
        state::ingest_health::spawn_watchdog(
            heartbeat.clone(),
            live_tx.subscribe(),
            settings_tx.subscribe(),
            // True while the DB write queue holds undrained ops. Sync atomic reads,
            // safe from the watchdog's OS thread; a dropped sender (shutdown) reads
            // as "no work" so the watchdog holds fire.
            move || {
                db_tx_pending
                    .upgrade()
                    .map(|tx| tx.capacity() < tx.max_capacity())
                    .unwrap_or(false)
            },
        );

        let producer_task = tokio::spawn(ingest_laserstream::client::run(
            settings.helius_laserstream_url.clone(),
            settings.helius_api_key.clone(),
            constants::PUMP_FUN_PROGRAM_ID.to_string(),
            update_tx,
            live_rx,
            pool_index.clone(),
            pools_changed.clone(),
        ));

        tokio::spawn(ingest_laserstream::pipeline::run_pool_subscription_refresh(
            token_cache.clone(),
            pool_index.clone(),
            pools_changed.clone(),
            constants::PUMP_FUN_PROGRAM_ID.to_string(),
            settings_tx.subscribe(),
        ));

        // Periodic backpressure diagnostics: logs hot-path queue depths + shed
        // counts every 10s (Step 0 consumer-lag visibility).
        tokio::spawn(ingest_laserstream::pipeline::run_queue_depth_logger(
            update_tx_weak,
            db_tx_weak,
            strategy_tx_weak,
            shed_counters,
        ));

        // Weekly partition maintenance for raw_transactions (~2-month retention).
        tokio::spawn(ingest_laserstream::maintenance::run_partition_maintenance(
            db.clone(),
        ));

        let pipeline_task = tokio::spawn(pipeline.run(update_rx));
        let db_writer = ingest_laserstream::db_writer::DbWriter::new(
            db.clone(),
            trade_signals.clone(),
            heartbeat,
        );
        let db_writer_task = tokio::spawn(db_writer.run(db_rx));

        (
            pool_index,
            pools_changed,
            strategy_rx,
            producer_task,
            pipeline_task,
            db_writer_task,
        )
    };

    // Bound the in-memory token cache: evict mints that have gone quiet beyond the
    // activity window and hold no open position, so the cache doesn't grow one
    // entry per created mint for the life of the process. The held-mint exemption
    // reads the strategy runtime caches' in-memory holding indexes (paper + real),
    // so no DB round trip and no coupling into `strategies/`.
    {
        let token_cache = token_cache.clone();
        let tpsl1 = tpsl1_cache.clone();
        let tpsl2 = tpsl2_cache.clone();
        let evict_repo =
            storage::repositories::token_info_repo::TokenInfoRepo::new(db.clone());
        tokio::spawn(state::token_cache::run_token_cache_eviction(
            token_cache,
            move |mint: &str| tpsl1.is_mint_held(mint) || tpsl2.is_mint_held(mint),
            evict_repo,
        ));
    }

    // Built after the transport branch so `DeployState` shares the active
    // pipeline's pool→mint index and migration signal with the HTTP handlers (a
    // token sync registers a migrated token's pool so live ingest subscribes
    // immediately). `api_db` backs the fast (core) handlers; `batch_db` backs the
    // heavy sweep/backtest jobs — both connected up front alongside hot-path `db`.
    //
    // The three narrow states are the deploy/local crate split's seam: `CoreState`
    // holds the mode-agnostic handles; `DeployState`/`LocalState` add their own and
    // `Deref` to the shared `Arc<CoreState>`. Every field is an Arc/PgPool/
    // watch/broadcast handle, so the per-mode states cost only refcount bumps.
    let core_state = Arc::new(state::core_state::CoreState::new(
        api_db,
        batch_db,
        settings.helius_rpc_url.clone(),
        settings.helius_laserstream_url.clone(),
        settings.helius_api_key.clone(),
        constants::PUMP_FUN_PROGRAM_ID.to_string(),
        token_cache.clone(),
        sse_tx.clone(),
        settings_tx.clone(),
        sol_price.clone(),
    ));
    let deploy_state = Arc::new(state::deploy_state::DeployState::new(
        core_state.clone(),
        trader.clone(),
        tpsl1_cache.clone(),
        tpsl2_cache.clone(),
        pool_index,
        pools_changed,
        trade_signals.clone(),
        live_tx.clone(),
    ));
    let local_state = Arc::new(state::local_state::LocalState::new(core_state.clone()));

    // Keep the token-list DB base fresh so `GET /api/tokens` reflects the whole
    // seeded universe (tokens + persisted stats), not just mints still resident in
    // the live cache after idle eviction. Fire-and-forget like the eviction sweep.
    tokio::spawn(state::token_list_cache::run_token_list_db_refresh(
        core_state.token_repo(),
        core_state.token_list.clone(),
    ));

    let strategy_runner = strategies::StrategyRunner::new(
        db.clone(),
        trader.clone(),
        token_cache.clone(),
        tpsl1_cache,
        tpsl2_cache,
        sse_tx.clone(),
        trade_signals.clone(),
        settings_tx.subscribe(),
    );
    let strategy_task = tokio::spawn(strategy_runner.run(strategy_rx));

    // Initialize SOL price cache immediately, then start the poller.
    match services::sol_price::fetch_latest_sol_price().await {
        Ok(price) => {
            info!("Initial SOL/USD price: ${price:.2}");
            let _ = sol_price.send(Some(price));
        }
        Err(err) => {
            warn!("Initial SOL/USD price fetch failed: {err}");
        }
    }

    // Service: SOL price polling — updates the in-memory SOL/USD cache
    let price_task = tokio::spawn(services::sol_price::run_poller(sol_price.clone()));

    let server_task = if settings.http_enabled {
        let bind_addr = format!("{}:{}", settings.host, settings.port);
        info!(addr = %bind_addr, workers = settings.http_workers, "Starting HTTP server");
        // Render each SSE event to wire bytes exactly once and fan the shared
        // frame out to all connections, instead of every subscriber re-rendering
        // (and re-reading the token cache) per event. Only needed when serving HTTP.
        tokio::spawn(api::handlers::system::run_sse_render_bridge(
            core_state.clone(),
        ));
        let http_core = core_state.clone();
        let http_deploy = deploy_state.clone();
        let http_local = local_state.clone();
        let http_workers = settings.http_workers;
        let cors_allowed_origin = settings.cors_allowed_origin.clone();
        let api_auth = ApiAuth {
            token: settings.api_auth_token.clone(),
        };
        // `API_AUTH_TOKEN` is required by `Settings::from_env`, so this is always
        // `Some` here; the middleware is fail-closed regardless.
        info!("API auth enabled (fail-closed): mutating requests require a bearer token");
        let http_server = HttpServer::new(move || {
            let allowed_origin = cors_allowed_origin.as_str();

            let cors = if allowed_origin == "*" {
                Cors::default()
                    .allow_any_origin()
                    .allowed_methods(vec!["GET", "POST", "PUT", "DELETE", "OPTIONS"])
                    .allowed_header(actix_web::http::header::CONTENT_TYPE)
                    .allowed_header(actix_web::http::header::ACCEPT)
                    .max_age(3600)
            } else {
                Cors::default()
                    .allowed_origin(allowed_origin)
                    .allowed_methods(vec!["GET", "POST", "PUT", "DELETE", "OPTIONS"])
                    .allowed_header(actix_web::http::header::CONTENT_TYPE)
                    .allowed_header(actix_web::http::header::ACCEPT)
                    .max_age(3600)
            };

            App::new()
                .wrap(from_fn(require_bearer_auth))
                .wrap(cors)
                .app_data(web::Data::new(http_core.clone()))
                .app_data(web::Data::new(http_deploy.clone()))
                .app_data(web::Data::new(http_local.clone()))
                .app_data(web::Data::new(api_auth.clone()))
                .configure(api::configure)
        })
        .workers(http_workers)
        .bind(&bind_addr)
        .context("Failed to bind HTTP server")?;
        Some(tokio::spawn(http_server.run()))
    } else {
        info!("HTTP disabled (HTTP_ENABLED=false) — bot-only mode");
        None
    };

    info!("System running — waiting for events");

    // Every long-lived task is supposed to run for the process lifetime, so ANY
    // of them resolving is a fault — whether it returned cleanly, errored, or
    // panicked. Bind each arm's `JoinHandle` result and surface a fault as an
    // `Err`, so `main` exits non-zero and a supervisor restarts the process.
    // (The old `_ = task` arms logged and then returned `Ok(())`, so a panicked
    // ingest/strategy task looked like a clean shutdown and was never restarted.)
    let outcome: anyhow::Result<()> = tokio::select! {
        res = producer_task => Err(task_fault("Ingest producer", res)),
        res = pipeline_task  => Err(task_fault("Ingest pipeline", res)),
        res = db_writer_task => Err(task_fault("DbWriter", res)),
        res = strategy_task => Err(task_fault("Strategy runner", res)),
        res = price_task    => Err(task_fault("SOL price poller", res)),
        res = async {
            match server_task {
                Some(task) => task.await,
                None => std::future::pending().await,
            }
        } => match res {
            // A clean HTTP stop is a legitimate shutdown (e.g. Ctrl-C), not a fault.
            Ok(Ok(())) => {
                info!("HTTP server stopped");
                Ok(())
            }
            Ok(Err(e)) => Err(anyhow::anyhow!("HTTP server error: {e}")),
            Err(e) => Err(anyhow::anyhow!("HTTP server task panicked: {e}")),
        },
    };

    if let Err(ref e) = outcome {
        error!("Fatal: {e} — exiting non-zero so the supervisor restarts the process");
    }
    outcome
}

/// Classify a long-lived task's `JoinHandle` result as a fatal error. These
/// tasks never return in normal operation, so a clean `Ok` return is as much a
/// fault as a panic; either way the process must exit non-zero. Generic over the
/// task's output so it works for `()` and `Result<_>` tasks alike.
fn task_fault<T>(name: &str, res: Result<T, tokio::task::JoinError>) -> anyhow::Error {
    match res {
        Ok(_) => anyhow::anyhow!("{name} task exited unexpectedly"),
        Err(e) if e.is_panic() => anyhow::anyhow!("{name} task panicked: {e}"),
        Err(e) => anyhow::anyhow!("{name} task aborted: {e}"),
    }
}
