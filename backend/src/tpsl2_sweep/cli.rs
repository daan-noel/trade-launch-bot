//! `backend -- tpsl2-sweep …` entrypoint.
//!
//! Implemented as a backend subcommand (like `probe`) rather than a separate
//! `bin/` target because the crate has no library target — a `src/bin/*.rs`
//! could not import `crate::tpsl2_sweep`. Load corpus → sample params → sweep →
//! aggregate per combo → persist. TPSL2-specific; other strategies get their own
//! separate sweep subcommand.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

use crate::tpsl2_sweep::corpus::{
    load_or_build, CacheSource, Corpus, CorpusSource, DbSource, Selection,
};
use crate::tpsl2_sweep::strategy::{ParamSpace, Strategy, SweepMethod};
use crate::tpsl2_sweep::engine::run_sweep;
use crate::tpsl2_sweep::tpsl2_strategy::{Tpsl2Axes, Tpsl2Strategy};
use crate::models::tpsl2_sweep::{Tpsl2SweepResult, Tpsl2SweepRun};
use crate::models::Tpsl2Rule;
use crate::state::token_cache::{TokenCache, MAX_TRADES_RETAINED};
use crate::storage::repositories::tpsl2_sweep_repo::Tpsl2SweepRepo;
use crate::storage::repositories::tpsl2_strategy_rule_repo::Tpsl2StrategyRuleRepo;

/// Entry point dispatched from `main` when argv[1] == "tpsl2-sweep". `args` is
/// argv after the `tpsl2-sweep` token.
pub async fn run(pool: PgPool, token_cache: Option<Arc<TokenCache>>, args: Vec<String>) -> Result<()> {
    let opts = Opts::parse(&args);
    run_sweep_cmd(pool, token_cache, &opts).await
}

// ---------------------------------------------------------------------------
// Arg parsing
// ---------------------------------------------------------------------------

struct Opts {
    source_cache: bool,
    rule: Option<Uuid>,
    tokens: usize,
    method: SweepMethod,
    curve_only: bool,
    out: PathBuf,
}

impl Opts {
    fn parse(args: &[String]) -> Self {
        let mut o = Opts {
            source_cache: false,
            rule: None,
            tokens: 5_000,
            method: SweepMethod::Grid,
            curve_only: false,
            out: PathBuf::from("tpsl2-sweep-out"),
        };
        let mut i = 0;
        while i < args.len() {
            let a = args[i].as_str();
            let mut next = || {
                i += 1;
                args.get(i).cloned()
            };
            match a {
                "--curve-only" => o.curve_only = true,
                "--source" => {
                    o.source_cache = next().as_deref() == Some("cache");
                }
                "--rule" => o.rule = next().and_then(|s| Uuid::parse_str(&s).ok()),
                "--tokens" => o.tokens = next().and_then(|s| s.parse().ok()).unwrap_or(o.tokens),
                "--out" => {
                    if let Some(s) = next() {
                        o.out = PathBuf::from(s);
                    }
                }
                "--method" => {
                    if let Some(s) = next() {
                        o.method = parse_method(&s);
                    }
                }
                _ => {}
            }
            i += 1;
        }
        o
    }
}

/// `grid` | `random:N` | `lhs:N`.
pub fn parse_method(s: &str) -> SweepMethod {
    if let Some(n) = s.strip_prefix("random:") {
        SweepMethod::Random { n: n.parse().unwrap_or(500), seed: 42 }
    } else if let Some(n) = s.strip_prefix("lhs:") {
        SweepMethod::LatinHypercube { n: n.parse().unwrap_or(500), seed: 42 }
    } else {
        SweepMethod::Grid
    }
}

// ---------------------------------------------------------------------------
// Sweep
// ---------------------------------------------------------------------------

async fn resolve_tpsl2_rule(pool: &PgPool, rule: Option<Uuid>) -> Result<Tpsl2Rule> {
    let repo = Tpsl2StrategyRuleRepo::new(pool.clone());
    match rule {
        Some(id) => repo.find_by_id(id).await?.ok_or_else(|| anyhow!("no tpsl2 rule with id {id}")),
        None => repo
            .find_all()
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("no tpsl2 rules in DB — pass --rule <uuid> with a template rule")),
    }
}

async fn run_sweep_cmd(pool: PgPool, token_cache: Option<Arc<TokenCache>>, o: &Opts) -> Result<()> {
    std::fs::create_dir_all(&o.out).context("creating output dir")?;

    let sel = Selection {
        mints: None,
        token_cap: o.tokens,
        created_after: None,
        per_mint_cap: MAX_TRADES_RETAINED as i64,
        curve_only: o.curve_only,
    };

    // Load the corpus once (strategy-independent), preferring the compact Parquet
    // cache from a prior run with the same selection. `load_or_build` reads it
    // back via mmap when present, else loads from the source and writes it.
    let cache_key = format!(
        "{}-t{}-curve{}",
        if o.source_cache { "cache" } else { "db" },
        o.tokens,
        o.curve_only as u8
    );
    let corpus = if o.source_cache {
        let cache = token_cache
            .clone()
            .ok_or_else(|| anyhow!("--source cache needs the live token cache (run in-process)"))?;
        load_or_build(&CacheSource::new(cache), &sel, &o.out, &cache_key).await?
    } else {
        load_or_build(&DbSource::new(pool.clone()), &sel, &o.out, &cache_key).await?
    };
    tracing::info!(
        tokens = corpus.token_count(),
        trades = corpus.trade_count(),
        hash = %corpus.hash,
        "tpsl2-sweep: corpus loaded"
    );
    if corpus.tokens.is_empty() {
        return Err(anyhow!("corpus is empty — widen the selection"));
    }

    let source = if o.source_cache { "cache" } else { "db" };
    let base = resolve_tpsl2_rule(&pool, o.rule).await?;
    let strategy = Tpsl2Strategy::new(base, Tpsl2Axes::default());
    let params = strategy.sample(o.method);
    // CLI is an isolated/offline process — let the sweep use the full rayon pool.
    let run = sweep_engine(pool, strategy, params, corpus, o.method, o.rule, source, None).await?;
    println!(
        "tpsl2-sweep done: run {} — {} tokens × {} combos saved.",
        run.id, run.token_count, run.combo_count
    );
    Ok(())
}

/// Config for an in-process, cache-sourced sweep triggered over HTTP.
pub struct CacheSweepConfig {
    pub rule_id: Option<Uuid>,
    pub tokens: usize,
    pub method: SweepMethod,
    pub curve_only: bool,
}

/// Run a sweep **in-process against the live `TokenCache`** (zero DB reads for
/// the corpus), persist it, and return the run. Used by the HTTP trigger so a
/// sweep optimizes on exactly the hot window the live strategy sees. The rayon
/// pool is bounded (see [`bounded_sweep_threads`]) so the CPU-heavy sweep can't
/// starve the live trading hot path. No Parquet disk cache — the live cache is
/// already the fast source.
pub async fn run_cache_sweep(
    pool: PgPool,
    token_cache: Arc<TokenCache>,
    cfg: CacheSweepConfig,
) -> Result<Tpsl2SweepRun> {
    let sel = Selection {
        mints: None,
        token_cap: cfg.tokens,
        created_after: None,
        per_mint_cap: MAX_TRADES_RETAINED as i64,
        curve_only: cfg.curve_only,
    };
    let corpus = CacheSource::new(token_cache).load(&sel).await?;
    tracing::info!(
        tokens = corpus.token_count(),
        trades = corpus.trade_count(),
        "tpsl2-sweep: corpus loaded from live cache"
    );
    if corpus.tokens.is_empty() {
        return Err(anyhow!("live cache holds no eligible tokens — nothing to sweep"));
    }
    let base = resolve_tpsl2_rule(&pool, cfg.rule_id).await?;
    let strategy = Tpsl2Strategy::new(base, Tpsl2Axes::default());
    let params = strategy.sample(cfg.method);
    sweep_engine(
        pool,
        strategy,
        params,
        corpus,
        cfg.method,
        cfg.rule_id,
        "cache",
        Some(bounded_sweep_threads()),
    )
    .await
}

/// Rayon threads for the in-process sweep: leave ≥2 cores for the live trading
/// hot path (ingest / sell-confirm), at least 1.
fn bounded_sweep_threads() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get().saturating_sub(2).max(1))
        .unwrap_or(2)
}

/// Run the CPU-bound sweep off the async runtime, then persist the run + its
/// ranked per-combo rows. Returns the persisted run. `max_threads` bounds the
/// rayon pool (`Some` for the in-process HTTP path); `None` uses the global pool
/// (CLI / isolated process).
async fn sweep_engine(
    pool: PgPool,
    strategy: Tpsl2Strategy,
    params: Vec<<Tpsl2Strategy as ParamSpace>::Params>,
    corpus: Corpus,
    method: SweepMethod,
    rule_id: Option<Uuid>,
    source: &str,
    max_threads: Option<usize>,
) -> Result<Tpsl2SweepRun> {
    if params.is_empty() {
        return Err(anyhow!("param space is empty"));
    }

    // Capture what we need after the corpus/strategy move into the blocking task.
    let corpus_hash = corpus.hash.clone();
    let token_count = corpus.token_count() as i32;
    let combo_count = params.len() as i32;
    let combo_params: Vec<serde_json::Value> =
        params.iter().map(|p| strategy.params_json(p)).collect();

    let (stats, metrics) = tokio::task::spawn_blocking(move || -> Result<_> {
        match max_threads {
            // Bound the pool so the sweep can't saturate every core in the live
            // process. `install` makes the inner `par_iter` use this pool.
            Some(n) => {
                let pool = rayon::ThreadPoolBuilder::new()
                    .num_threads(n)
                    .thread_name(|i| format!("tpsl2-sweep-{i}"))
                    .build()
                    .map_err(|e| anyhow!("rayon pool build failed: {e}"))?;
                pool.install(|| run_sweep(&strategy, &params, &corpus))
            }
            None => run_sweep(&strategy, &params, &corpus),
        }
    })
    .await??;

    // Fold the metrics + their param JSON into persistable rows.
    let results: Vec<Tpsl2SweepResult> = metrics
        .iter()
        .map(|m| Tpsl2SweepResult {
            combo_id: m.combo_id as i32,
            params: combo_params[m.combo_id as usize].clone(),
            n_fired: m.n_fired as i64,
            n_open: m.n_open as i64,
            n_closed: m.n_closed as i64,
            win_rate: m.win_rate,
            total_pnl_sol: m.total_pnl_sol,
            mean_pnl_pct: m.mean_pnl_pct,
            median_pnl_pct: m.median_pnl_pct,
            p90_pnl_pct: m.p90_pnl_pct,
            best_pnl_pct: m.best_pnl_pct,
            worst_pnl_pct: m.worst_pnl_pct,
            std_pnl_pct: m.std_pnl_pct,
            profit_factor: m.profit_factor,
            score: m.score,
            expectancy_sol: m.expectancy_sol,
            avg_holding_secs: m.avg_holding_secs,
            median_holding_secs: m.median_holding_secs,
            exit_take_profit: m.exit_take_profit as i32,
            exit_stop_loss: m.exit_stop_loss as i32,
            exit_trailing: m.exit_trailing as i32,
            exit_stall: m.exit_stall as i32,
            exit_time: m.exit_time as i32,
            exit_liquidity: m.exit_liquidity as i32,
            exit_cohort: m.exit_cohort as i32,
            exit_open: m.exit_open as i32,
        })
        .collect();

    let run = Tpsl2SweepRun {
        id: Uuid::new_v4(),
        rule_id,
        source: source.to_string(),
        method: method.tag().to_string(),
        token_count,
        combo_count,
        corpus_hash: Some(corpus_hash),
        created_at: Utc::now(),
    };
    Tpsl2SweepRepo::new(pool).save_run(&run, &results).await?;

    tracing::info!(
        run_id = %run.id,
        tokens = stats.tokens,
        combos = stats.combos,
        evals = stats.rows,
        fired = stats.fired,
        rows = results.len(),
        "tpsl2-sweep: done"
    );
    Ok(run)
}
