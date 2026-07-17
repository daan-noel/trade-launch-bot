//! `live` — the live-trading binary. Composition root for the deploy
//! stack: LaserStream ingest, the tpsl1/tpsl2 strategies, the trader, and the
//! deploy HTTP surface (core routes + deploy routes). Mirrors the old
//! `backend/src/main.rs` minus all sweep/backtest/local-only wiring.

use live::{api, ingest, seed, services, state, strategies, trader};

use trading_core::{config, models};
use config::constants;

use anyhow::Context;
use solana_sdk::signature::Keypair;
use std::sync::Arc;
use tracing::{error, info, warn};

use actix_cors::Cors;
use actix_web::middleware::from_fn;
use actix_web::{web, App, HttpServer};

// Bearer-auth gate (`ApiAuth` + `require_bearer_auth`) is the shared, fail-closed
// `http_auth` middleware — one SSOT copy across hunter live/lab + forge live.
use http_auth::{require_bearer_auth, ApiAuth};

use live::trader::{PumpFunTrader, TraderConfig};

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
        "consolidate-dryrun" => {
            // Zero-SOL verification of the pre-buy consolidation path: enumerate
            // every account for the mint, derive the canonical ATA, and simulate the
            // `[create_ata_idempotent, transfer_checked, close]` tx for each orphan
            // against live chain state. No tx is sent.
            let mint = args
                .get(1)
                .context("usage: probe consolidate-dryrun <mint>")?;
            let routing = trader.resolve_buy_routing(mint).await?;
            let accounts = trader.get_all_token_accounts_for_mint(mint).await?;
            println!(
                "Accounts held for {mint} ({}): token_program={}",
                accounts.len(),
                routing.token_program_id
            );
            for (pk, bal) in &accounts {
                println!("  acct={pk}  balance={bal} raw");
            }
            let sims = trader
                .simulate_consolidation(mint, routing.token_program)
                .await?;
            if sims.is_empty() {
                println!("✅ No orphans — only the canonical ATA (or nothing). Consolidation is a no-op.");
            } else {
                println!("Simulating {} orphan consolidation tx(s):", sims.len());
                for (orphan, balance, outcome) in &sims {
                    println!("\n  orphan={orphan}  balance={balance} raw");
                    print_sim_outcome(outcome);
                }
            }
        }
        "sweep-sell-dryrun" => {
            // Zero-SOL verification of the manual sweep-sell path: enumerate every
            // account for the mint and simulate a full sell FROM EACH (passing the
            // account as the override), routed curve vs AMM by live migration status.
            // No tx is sent.
            let mint = args
                .get(1)
                .context("usage: probe sweep-sell-dryrun <mint> [slippage_bps]")?;
            let positional: Vec<&String> =
                args.iter().skip(2).filter(|s| !s.starts_with("--")).collect();
            let slippage_bps: Option<u64> = match positional.first() {
                Some(s) => Some(s.parse().context("slippage_bps must be a u64")?),
                None => None,
            };
            let routing = trader.resolve_buy_routing(mint).await?;
            let accounts = trader.get_all_token_accounts_for_mint(mint).await?;
            println!(
                "Sweep-sell dry-run for {mint} ({} account(s)) — migrated={}, token_program={}",
                accounts.len(),
                routing.is_migrated,
                routing.token_program_id
            );
            let mut simulated_any = false;
            for (pk, bal) in &accounts {
                if *bal == 0 {
                    println!("\n  acct={pk}  balance=0 — skip (nothing to sell, would just be closed)");
                    continue;
                }
                simulated_any = true;
                let acct_str = pk.to_string();
                println!("\n  acct={pk}  balance={bal} raw — simulating full sell:");
                let outcome = if routing.is_migrated {
                    trader
                        .simulate_amm_sell(
                            mint,
                            *bal,
                            &routing.token_program_id,
                            None,
                            Some(acct_str.as_str()),
                            slippage_bps,
                        )
                        .await?
                } else {
                    // Curve sim resolves the account cache-first; warm the cache to THIS
                    // account so the simulated sell targets the swept account.
                    trader.resolve_cached_token_account(mint).await.ok();
                    trader
                        .simulate_curve_sell(mint, *bal, slippage_bps, routing.cashback_enabled)
                        .await?
                };
                print_sim_outcome(&outcome);
            }
            if !simulated_any {
                println!("\n✅ No non-empty accounts — nothing to sweep-sell.");
            }
        }
        "cashback-status" => {
            let pots = trader.cashback_status().await?;
            println!("Cashback status (read-only):");
            let mut total_wsol = 0u64;
            for p in &pots {
                let claimable = p.claimable();
                total_wsol += claimable;
                println!("  [{}] uva={} exists={}", p.label, p.pda, p.exists);
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
            "unknown probe '{other}'. Use: ladder | fanout | check-nonces | simulate-buy | \
             simulate-sell | simulate-amm-buy | simulate-amm-sell | consolidate-dryrun | \
             sweep-sell-dryrun | holdings | cashback-status | claim-cashback [--execute]"
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

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> anyhow::Result<()> {
    // Load .env before anything else
    dotenvy::dotenv().ok();

    // Tracing / logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                // The bin crate is `hunter_live` (bin target `hunter-live`), where all
                // of main.rs's startup logs live; `live` is the lib crate. Enable both
                // (plus the shared `trading_core` lib) or the process runs silently and
                // looks idle.
                .unwrap_or_else(|_| {
                    "hunter_live=info,live=info,trading_core=info,sqlx=error".into()
                }),
        )
        .init();

    // Config. Shared settings load for both bins; the live bin additionally
    // requires the Helius endpoints (fail fast at boot) and loads its own trading
    // credentials (`TradingSecrets` — wallet key, nonce accounts, Sender URLs),
    // which lab never touches.
    let settings = config::Settings::from_env().context("Failed to load configuration")?;
    settings
        .require_live_endpoints()
        .context("live bin is missing a required Helius endpoint")?;
    let secrets = live::config::TradingSecrets::from_env()
        .context("Failed to load live trading credentials")?;

    // HTTP bind — live reads its own LIVE_HOST/LIVE_PORT (docker's HOST/PORT wins).
    // Default matches the deploy LIVE_API_PORT so local + docker use the same port.
    let http_host = config::resolve_host("LIVE_HOST");
    let http_port = config::resolve_port("LIVE_PORT", 8130)?;

    info!(
        host = %http_host,
        port = http_port,
        pump_program = constants::PUMP_FUN_PROGRAM_ID,
        "Configuration loaded"
    );

    // The trader takes an `Arc<dyn Signer>` (HSM/remote-signer-ready) and parsed
    // nonce pubkeys, with all tuning at the crate's `Default`s (see `TraderConfig`).
    let signer: Arc<dyn solana_sdk::signature::Signer + Send + Sync> = Arc::new(
        parse_wallet_keypair(&secrets.wallet_private_key)
            .context("Failed to parse trader wallet private key")?,
    );
    let nonce_accounts = secrets
        .nonce_accounts
        .iter()
        .map(|s| s.parse::<solana_sdk::pubkey::Pubkey>())
        .collect::<Result<Vec<_>, _>>()
        .context("Failed to parse NONCE_ACCOUNTS as pubkeys")?;
    let mut trader_config = TraderConfig::new(
        settings.helius_rpc_url.clone(),
        secrets.helius_sender_urls.clone(),
        signer,
        nonce_accounts.clone(),
    );
    // RPC-reduction tuning (docs/plans/rpc/helius-rpc-reduction-plan.md): the
    // LaserStream push hooks below keep the blockhash cache ~400 ms fresh and
    // re-arm nonce slots at feed speed, so the executor's pollers are demoted to
    // stall fallbacks. 10 s watchdog tick (fetches only when the feed stalls);
    // 30 s max-age stays well inside the ~60 s hash validity window; the 2 s
    // first-delay lets the nonce push win before the fallback poll spends reads.
    trader_config.cache.blockhash_refresh_ms = 10_000;
    trader_config.cache.blockhash_max_age_ms = 30_000;
    trader_config.nonce.refresh_first_delay_ms = 2_000;
    let trader_config = Arc::new(trader_config);

    let mut trader = PumpFunTrader::new(trader_config);
    trader
        .initialize()
        .await
        .context("Failed to initialize PumpFunTrader")?;
    let trader = Arc::new(trader);

    // Probe mode: `cargo run -p live -- probe <subcommand>` runs a
    // one-shot, no-/low-SOL validation of the latency changes against live infra,
    // then exits before touching the DB / ingest / HTTP server. See `run_probe`.
    if std::env::args().nth(1).as_deref() == Some("probe") {
        return run_probe(&trader, std::env::args().skip(2).collect()).await;
    }

    // Database — connect the three workload-isolated pools and run migrations. `db`
    // (hot) backs ingest/strategy/maintenance/seed/caches; `api_db` backs the fast
    // HTTP handlers; `batch_db` is connected for the shared `CoreState` shape (the
    // sweep/backtest jobs that use it live in the local bin).
    let trading_core::storage::postgres::DbPools {
        hot: db,
        api: api_db,
        batch: batch_db,
    } = trading_core::storage::postgres::connect(&settings).await?;

    // Boot wallet-balance sweep (buy-in-flight recovery, Phase 3 backstop): list
    // the wallet's on-chain token accounts once and flag any balance no open
    // position across either clone accounts for — a manual transfer, a failed
    // marker persist, or any bug the durable `BuySubmitted` marker doesn't cover.
    // Read-only + advisory (logs for review, never sells/deletes), spawned off the
    // boot critical path so its RPC scan never delays ingest/HTTP startup.
    {
        let trader = trader.clone();
        let strategy_repo =
            trading_core::storage::repositories::strategy_repo::StrategyRepo::new(db.clone());
        tokio::spawn(async move {
            if let Err(e) = services::wallet_reconcile::reconcile_wallet_holdings(
                &trader, &strategy_repo,
            )
            .await
            {
                warn!("Boot wallet reconcile failed (advisory only): {e}");
            }
        });
    }

    // Background SOL-balance refresh: keeps `PumpFunTrader::can_commit_buy` accurate
    // without ever hitting the RPC on the hot buy path. Polls every 60 s (the cache
    // feeds the buy affordability gate, so it stays at 60 s rather than a longer
    // telemetry-grade interval); the first tick fires immediately so the cache is
    // non-empty before the first snipe fires.
    {
        let trader = trader.clone();
        tokio::spawn(async move {
            loop {
                match trader.get_sol_balance().await {
                    Ok(lamports) => trader.update_balance_lamports_cache(lamports),
                    Err(e) => warn!("SOL balance refresh failed (guard stays on last cached value): {e}"),
                }
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            }
        });
    }

    // In-memory caches (shared between services and API handlers)
    let token_cache = Arc::new(state::token_cache::TokenCache::new());

    // Seed the cache off the boot critical path: ingest/HTTP start immediately and
    // the cache hydrates in the background (build-then-insert keeps it race-safe vs
    // the live pipeline). A failure is logged, not fatal — the system still runs and
    // the cache fills from live events. See `crate::seed`.
    {
        let db = db.clone();
        let token_cache = token_cache.clone();
        tokio::spawn(async move {
            let started = std::time::Instant::now();
            match seed::seed_token_cache(&db, token_cache).await {
                Ok(()) => info!("Cache seed task finished in {:?}", started.elapsed()),
                Err(e) => error!("Cache seed task failed (cache will fill from live events): {e}"),
            }
        });
    }

    let (sse_tx, _) = tokio::sync::broadcast::channel::<models::ingest::SseEvent>(512);

    // Load the persisted settings document and hold it in a watch channel as the
    // in-memory source of truth, so a policy set in a previous run is in force
    // before the first event arrives.
    let settings_repo = trading_core::storage::repositories::settings_repo::SettingsRepo::new(db.clone());
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
    // One unified, strategy-agnostic runtime cache (replaces the tpsl1/tpsl2 caches);
    // rules are dispatched by `strategy_id` through the registry inside it.
    let strategy_cache =
        Arc::new(trading_core::strategies::runtime_cache::StrategyRuntimeCache::new());
    // Install the cold-lane sender so position transitions broadcast
    // `tpsl_positions_changed` deltas straight from the cache funnel.
    strategy_cache.set_sse_sender(sse_tx.clone());
    strategy_cache
        .load_from_db(&trading_core::storage::repositories::strategy_repo::StrategyRepo::new(
            db.clone(),
        ))
        .await
        .context("Failed to load strategy runtime cache")?;

    // Shared (wallet, mint) wakeup hub: the DbWriter signals it once a trade is
    // persisted; the live buy/sell confirm loops await it instead of polling the
    // DB on a fixed timer (they keep their timeout as a fallback).
    let trade_signals = Arc::new(state::trade_signals::TradeSignals::new());

    // ── Ingest transport: LaserStream (Yellowstone gRPC) ──
    // Feeds the shared token cache / strategy / SSE / DB. Yields the pool→mint
    // index + migration signal (for DeployState), the strategy receiver, and the
    // long-lived task handles the supervising select watches.
    // Push feeds riding the same LaserStream subscription (RPC-reduction Phase 2):
    // block metas feed the recent-blockhash cache (steady state: zero
    // `getLatestBlockhash`), nonce-account updates re-arm durable-nonce slots at
    // feed speed (the executor's post-send poll becomes a stall fallback). Both
    // callbacks are cheap parses running on the transport task.
    let push_hooks = {
        let bh_trader = trader.clone();
        let nonce_trader = trader.clone();
        ingest_laserstream::PushHooks {
            watch_accounts: nonce_accounts.iter().map(|pk| pk.to_string()).collect(),
            on_block_meta: Some(Box::new(move |slot, blockhash| {
                if let Ok(hash) = blockhash.parse::<solana_sdk::hash::Hash>() {
                    bh_trader.set_cached_blockhash(hash, slot);
                }
            })),
            on_account: Some(Box::new(move |slot, pubkey, data| {
                if let Ok(pk) = pubkey.parse::<solana_sdk::pubkey::Pubkey>() {
                    nonce_trader.on_nonce_account_update(&pk, data, slot);
                }
            })),
        }
    };

    let ingest_result = ingest::spawn_ingest(
        settings.helius_laserstream_url.clone(),
        settings.helius_api_key.clone(),
        db.clone(),
        token_cache.clone(),
        sse_tx.clone(),
        settings_tx.subscribe(),
        live_rx,
        Arc::new(trader::TraderHookBridge(trader.clone())),
        trade_signals.clone(),
        push_hooks,
    ).await;
    let pool_index = ingest_result.pool_index;
    let pools_changed = ingest_result.pools_changed;
    let strategy_rx = ingest_result.strategy_rx;
    let consumer_task = ingest_result.consumer_task;
    let db_writer_task = ingest_result.db_writer_task;
    let ingest_handle = ingest_result.ingest_handle;

    // Bridge the operator live-mode toggle to the ingest transport. The host
    // `live_tx` (flipped by `PUT /api/system/live`) and the ingest crate's own
    // internal live gate are *separate* watch channels; `start()` only seeds the
    // transport with the boot value, so without this forwarder a runtime toggle
    // never reaches the gRPC stream — it stays paused (or running) regardless of
    // the UI. Re-applies the current value first, then mirrors every change.
    {
        let ingest_handle = ingest_handle.clone();
        let mut live_rx = live_tx.subscribe();
        tokio::spawn(async move {
            ingest_handle.set_live(*live_rx.borrow_and_update());
            while live_rx.changed().await.is_ok() {
                ingest_handle.set_live(*live_rx.borrow_and_update());
            }
        });
    }

    // (The token-cache eviction task is spawned after the engine loop below, so its
    // held-mint exemption can also consult the engine's live position registry.)

    // Built after the transport branch so `DeployState` shares the active
    // pipeline's pool→mint index and migration signal with the HTTP handlers (a
    // token sync registers a migrated token's pool so live ingest subscribes
    // immediately). `api_db` backs the fast (core) handlers; `batch_db` is held by
    // `CoreState` for shape parity with the local bin.
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
    // The legacy unified service is still constructed so `DeployState` + the legacy
    // rule/position handlers compile (deleted in Phase 7); its background reapers are
    // **not** spawned — the generic engine now owns `strategy_positions`, and running
    // the old reapers alongside it would race over the same rows.
    let strategy_service = strategies::StrategyService::new(
        db.clone(),
        trader.clone(),
        strategy_cache.clone(),
        token_cache.clone(),
        sse_tx.clone(),
        trade_signals.clone(),
        settings_tx.subscribe(),
    );

    // ── The generic fingerprint + metrics engine (strategy redesign, Phase 4) ──
    // THE serialized decision loop, driven by the same ingest ping channel the old
    // `StrategyRunner` drained (`strategy_rx`), a 500 ms tick, and confirmed fills.
    let rule_repo =
        trading_core::storage::repositories::rule_repo::RuleRepo::new(db.clone());
    let fingerprint_repo =
        trading_core::storage::repositories::fingerprint_repo::FingerprintRepo::new(db.clone());
    let engine_handles = strategies::engine::spawn_engine(strategies::engine::EngineDeps {
        ping_rx: strategy_rx,
        rule_repo: rule_repo.clone(),
        fp_repo: fingerprint_repo.clone(),
        strategy_repo:
            trading_core::storage::repositories::strategy_repo::StrategyRepo::new(db.clone()),
        trade_repo: trading_core::storage::repositories::trade_repo::TradeRepo::new(db.clone()),
        token_cache: token_cache.clone(),
        trader: trader.clone(),
        trade_signals: trade_signals.clone(),
        sse_tx: sse_tx.clone(),
        settings: settings_tx.subscribe(),
    });
    let strategies::engine::EngineHandles {
        handle: engine_handle,
        armed: engine_armed,
        positions: engine_positions,
        task: strategy_task,
    } = engine_handles;

    // Recovery reaper backstop: settles durable `strategy_positions` rows the engine
    // loop can't resolve itself (a dead sell task's stuck `ExitPending`, an ambiguous
    // buy's abandoned `BuySubmitted`). Fire-and-forget — advisory cleanup, never fatal.
    let _engine_reaper = strategies::engine::reapers::spawn_reaper(
        trading_core::storage::repositories::strategy_repo::StrategyRepo::new(db.clone()),
    );

    let deploy_state = Arc::new(state::deploy_state::DeployState::new(
        core_state.clone(),
        trader.clone(),
        strategy_service.clone(),
        engine_handle,
        engine_armed,
        rule_repo,
        fingerprint_repo,
        pool_index,
        pools_changed,
        trade_signals.clone(),
        live_tx.clone(),
    ));

    // Pin a SlotAnchor once at startup so incremental token-sync replay paths can
    // estimate accurate block_time for historical frames (instead of stamping now()).
    // Best-effort: an RPC failure just leaves the anchor as None and the replay path
    // falls back to now() — the same behavior as before this fix.
    {
        let rpc = trading_core::services::helius_rpc::HeliusRpc::new(
            settings.helius_rpc_url.clone(),
        );
        let ds = deploy_state.clone();
        tokio::spawn(async move {
            match rpc.get_slot().await {
                Ok(slot) => match rpc.get_block_time(slot).await {
                    Ok(Some(unix_ts)) => {
                        use chrono::TimeZone;
                        if let Some(time) = chrono::Utc.timestamp_opt(unix_ts, 0).single() {
                            let anchor = ingest_laserstream::slot_anchor::SlotAnchor::new(slot, time);
                            ds.set_slot_anchor(anchor).await;
                            info!(slot, "SlotAnchor pinned for replay block_time estimation");
                        }
                    }
                    Ok(None) => warn!("getBlockTime returned null for slot {slot} — anchor not pinned"),
                    Err(e) => warn!("getBlockTime failed — replay block_time will use now(): {e}"),
                },
                Err(e) => warn!("getSlot failed — replay block_time will use now(): {e}"),
            }
        });
    }

    // NOTE: the live bin no longer runs the token-list DB-base refresher. On live the
    // `GET /api/tokens` full list is paged directly from Postgres (see
    // `api::handlers::tokens::list`), so the in-RAM `token_list` snapshot is kept
    // tracking-only (its `db_base` stays empty) — the 4 GB EC2 box holds only the
    // tracked subset, never the 100K+ universe. `lab` still runs the refresher to
    // build a full in-RAM snapshot (it has the RAM and wants the speed).

    // Bound the in-memory token cache: evict mints that have gone quiet beyond the
    // activity window and hold no open position. The held-mint exemption now consults
    // BOTH the legacy runtime cache (historical rows) and the generic engine's live
    // position registry, so an engine-held token's price feed is never evicted mid-hold.
    {
        let token_cache = token_cache.clone();
        let sc = strategy_cache.clone();
        let positions = engine_positions.clone();
        let evict_repo =
            trading_core::storage::repositories::token_info_repo::TokenInfoRepo::new(db.clone());
        tokio::spawn(state::token_cache::run_token_cache_eviction(
            token_cache,
            move |mint: &str| sc.is_mint_held(mint) || positions.is_mint_held(mint),
            evict_repo,
        ));
    }

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
        let bind_addr = format!("{http_host}:{http_port}");
        info!(addr = %bind_addr, workers = settings.http_workers, "Starting HTTP server");
        // Render each SSE event to wire bytes exactly once and fan the shared
        // frame out to all connections, instead of every subscriber re-rendering
        // (and re-reading the token cache) per event. Only needed when serving HTTP.
        tokio::spawn(
            trading_core::api::handlers::system::run_sse_render_bridge(core_state.clone()),
        );
        let http_core = core_state.clone();
        let http_deploy = deploy_state.clone();
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
                .app_data(web::Data::new(api_auth.clone()))
                .app_data(trading_core::api::json_error_config())
                .app_data(trading_core::api::query_error_config())
                .configure(trading_core::api::configure_core_routes)
                .configure(api::configure_deploy_routes)
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
    let outcome: anyhow::Result<()> = tokio::select! {
        res = consumer_task  => Err(task_fault("Ingest consumer", res)),
        res = db_writer_task => Err(task_fault("DbWriter", res)),
        res = strategy_task => Err(task_fault("Strategy engine", res)),
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
