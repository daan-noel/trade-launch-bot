use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::Duration;
use sqlx::PgPool;
use tracing::info;

use crate::{
    config::constants::{
        total_supply_for, INITIAL_VIRTUAL_TOKEN_RESERVES, SEED_ACTIVITY_WINDOW_DAYS,
        SEED_TOKEN_LIMIT, SEED_TRADES_PER_MINT,
    },
    models::{token::Token, token_info::TokenInfo, trade::Trade},
    state::token_cache::{TokenCache, TokenState},
    storage::repositories::{
        token_info_repo::TokenInfoRepo,
        token_repo::TokenRepo,
        tpsl1_position_repo::Tpsl1PositionRepo,
        tpsl2_position_repo::Tpsl2PositionRepo,
        trade_repo::{SeedAgg, TradeRepo},
    },
};

/// Populate `token_cache` from the database so historical tokens and their trade
/// stats are available after a restart.
///
/// Runs **off the boot critical path** (spawned in `main`), so it never gates
/// ingest/HTTP startup. To stay race-safe against the concurrently-running live
/// pipeline, each token is built fully *then* inserted with `entry().or_insert`:
/// the strategy hot path never observes a half-filled trade buffer, and a token
/// the live path already created (a brand-new mint) is never clobbered.
///
/// Strategy (scoped to a bounded seed set so cold start scales with *recent*
/// activity, not total history):
///   1. Recent tokens — created within `SEED_ACTIVITY_WINDOW_DAYS`, capped at
///      `SEED_TOKEN_LIMIT` — plus every mint with an unsettled position (always
///      tracked regardless of age, or its open exit would strand).
///   2. Those mints' persisted `tokens_info` metrics.
///   3. One windowed scan of `trades` (`for_each_seed_mint`): newest
///      `SEED_TRADES_PER_MINT` per mint into the cache, lifetime aggregates carried
///      along — no separate aggregate pass. Unsettled-position mints first so their
///      exits resume soonest.
pub async fn seed_token_cache(pool: &PgPool, token_cache: Arc<TokenCache>) -> anyhow::Result<()> {
    let token_repo = TokenRepo::new(pool.clone());
    let trade_repo = TradeRepo::new(pool.clone());

    // 1a. Recent launches within the activity window (bounded on recency + count).
    let cutoff = chrono::Utc::now() - Duration::days(SEED_ACTIVITY_WINDOW_DAYS);
    let mut token_by_mint: HashMap<String, Token> = token_repo
        .find_recent_active(SEED_TOKEN_LIMIT, cutoff)
        .await?
        .into_iter()
        .map(|t| (t.mint_address.clone(), t))
        .collect();

    // 1b. Mints with an unsettled position must always be tracked — the live path
    // can't re-track an existing mint, so an open exit on a token older than the
    // window would otherwise strand. Pull in any that the window missed.
    let mut held_set: HashSet<String> = Tpsl1PositionRepo::new(pool.clone())
        .distinct_unsettled_mints()
        .await?
        .into_iter()
        .collect();
    held_set.extend(
        Tpsl2PositionRepo::new(pool.clone())
            .distinct_unsettled_mints()
            .await?,
    );
    let missing_held: Vec<String> = held_set
        .iter()
        .filter(|m| !token_by_mint.contains_key(*m))
        .cloned()
        .collect();
    if !missing_held.is_empty() {
        for token in token_repo.find_by_mints(&missing_held).await? {
            token_by_mint.insert(token.mint_address.clone(), token);
        }
    }

    let total = token_by_mint.len();
    if total == 0 {
        info!("Cache seed: no tokens in DB — nothing to load");
        return Ok(());
    }

    // 2. Persisted metrics for the seeded set.
    let seeded_mints: Vec<String> = token_by_mint.keys().cloned().collect();
    let info_by_mint: HashMap<String, TokenInfo> = TokenInfoRepo::new(pool.clone())
        .find_for(&seeded_mints)
        .await?
        .into_iter()
        .map(|i| (i.mint_address.clone(), i))
        .collect();

    // Process unsettled-position mints first so their exits resume soonest; the rest
    // follow. Mints with no trade rows never appear in the stream — handled after.
    let (held_present, rest): (Vec<String>, Vec<String>) = seeded_mints
        .iter()
        .cloned()
        .partition(|m| held_set.contains(m));

    // 3. Single windowed trade scan — build each mint's full state, then insert.
    let cap = SEED_TRADES_PER_MINT;
    for mints in [&held_present, &rest] {
        trade_repo
            .for_each_seed_mint(mints, cap, |mint, trades, agg| {
                if let Some(token) = token_by_mint.remove(&mint) {
                    let state = build_state(token, info_by_mint.get(&mint), Some(agg), trades);
                    token_cache.entry(mint).or_insert(state);
                }
            })
            .await?;
    }

    // Tokens with no trade rows (e.g. a just-created mint) never reached the stream —
    // seed them from their `tokens_info` metrics alone so they're still tracked.
    for (mint, token) in token_by_mint.drain() {
        let state = build_state(token, info_by_mint.get(&mint), None, Vec::new());
        token_cache.entry(mint).or_insert(state);
    }

    info!(
        held = held_present.len(),
        "Cache seed complete: {total} tokens loaded from DB"
    );

    Ok(())
}

/// Assemble a complete `TokenState` from a token row, its optional persisted
/// metrics, and (for mints that have traded) the seed scan's per-mint aggregates +
/// capped trade run. Built whole before it ever enters the cache, so a concurrent
/// reader never sees a partial buffer. Precedence mirrors the previous two-pass
/// seed: persisted `tokens_info` first, then fresh trade aggregates override the
/// live-derived counters; reserves/market-cap fill only where still unset.
fn build_state(
    token: Token,
    info: Option<&TokenInfo>,
    agg: Option<SeedAgg>,
    trades: Vec<Trade>,
) -> TokenState {
    let mut state = TokenState::new(token);

    if let Some(info) = info {
        if let Some(ath_price) = info.ath_price {
            state.ath_price = Some(ath_price);
            state.ath_timestamp = info.ath_timestamp;
        }
        state.volume_sol_total = info.volume;
        state.trade_count = info.trade_count as u64;
        state.last_trade_at = info.last_trade_at;
        state.market_cap = info.market_cap;
        state.current_price = info.current_price;
        state.is_migrated = info.is_migrated;
        state.last_synced_at = info.last_synced_at;
    }

    if let Some(agg) = agg {
        state.trade_count = agg.lifetime_count;
        state.volume_sol_total = agg.lifetime_volume;
        state.last_trade_at = Some(agg.last_trade_at);
        state
            .initial_virtual_token_reserves
            .get_or_insert(INITIAL_VIRTUAL_TOKEN_RESERVES);
        if state.current_virtual_token_reserves.is_none() {
            state.current_virtual_token_reserves = agg.current_reserves;
        }
        if state.market_cap.is_none() {
            if let Some(price) = agg.newest_price {
                state.market_cap = Some(total_supply_for(state.token.is_mayhem_mode) * price);
            }
        }
    }

    for trade in &trades {
        // Intern the wallet into the token's `u32` namespace (Phase B step 2) before
        // pushing — same path as the live append, so seeded rows match live rows.
        let cached = state.intern_trade(trade);
        state.push_trade_capped(cached);
    }

    // Prime the dead-token liquidity signal from the seeded tail: the newest trade
    // (by block_time) that carries a real-reserve snapshot. The live path maintains
    // this field incrementally in `add_trade`, but seed bypasses that via
    // `push_trade_capped`, so populate it directly here. One-time, off the hot path.
    state.current_real_sol_reserves = state
        .trades
        .iter()
        .filter(|t| t.real_sol_reserves.is_some())
        .max_by_key(|t| t.block_time)
        .and_then(|t| t.real_sol_reserves);

    state
}

#[cfg(test)]
mod tests {
    //! `build_state` precedence is a pure unit test (no DB). The cap / window /
    //! position-safety-net behaviours touch Postgres and are `#[ignore]`d like the
    //! other DB tests; run against a local Postgres:
    //!   $env:DATABASE_URL = "postgres://postgres:1220@localhost:5432/meme_bot"
    //!   cargo test --bin backend seed:: -- --ignored --nocapture
    use super::*;
    use crate::models::trade::TradeType;
    use chrono::Utc;
    use sqlx::postgres::PgPoolOptions;
    use sqlx::PgPool;
    use uuid::Uuid;

    fn uniq(prefix: &str) -> String {
        format!("{prefix}{}", Uuid::new_v4().simple())
    }

    fn token(mint: &str, created_at: chrono::DateTime<Utc>) -> Token {
        Token::new(
            mint.to_string(),
            uniq("creator-"),
            "Seed Test".into(),
            "SEED".into(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            false,
            serde_json::Value::Array(vec![]),
            uniq("create-sig-"),
            created_at,
        )
    }

    /// `build_state` precedence — no DB. Fresh trade aggregates override persisted
    /// `tokens_info` for the live-derived counters, while market_cap keeps the
    /// persisted value (info wins) and reserves fill only where unset.
    #[test]
    fn build_state_aggregate_overrides_info_but_market_cap_keeps_persisted() {
        let tok = token("MINT-bs", Utc::now());
        let info = TokenInfo::new(
            "MINT-bs".into(),
            Some(9.0),       // ath_price
            None,            // ath_timestamp
            None,            // age
            111.0,           // volume (stale — agg must override)
            Some(42.0),      // market_cap (persisted — must WIN over trade-derived)
            7,               // trade_count (stale — agg must override)
            None,            // last_trade_at
            Some(3.0),       // current_price
            false,
            true, // is_migrated
            Utc::now(),
            Utc::now(),
            None,
        );
        let agg = SeedAgg {
            lifetime_count: 50,
            lifetime_volume: 500.0,
            last_trade_at: Utc::now(),
            current_reserves: Some(1234.0),
            newest_price: Some(2.0),
        };

        let state = build_state(tok, Some(&info), Some(agg), Vec::new());

        assert_eq!(state.trade_count, 50, "agg lifetime_count overrides info");
        assert_eq!(state.volume_sol_total, 500.0, "agg volume overrides info");
        assert_eq!(state.market_cap, Some(42.0), "persisted market_cap wins");
        assert_eq!(state.current_virtual_token_reserves, Some(1234.0));
        assert!(state.is_migrated, "is_migrated carried from info");
        assert_eq!(state.ath_price, Some(9.0));

        // No persisted market_cap → derive from the newest trade price.
        let mut info2 = info.clone();
        info2.market_cap = None;
        let agg2 = SeedAgg {
            lifetime_count: 1,
            lifetime_volume: 1.0,
            last_trade_at: Utc::now(),
            current_reserves: None,
            newest_price: Some(2.0),
        };
        let state2 = build_state(token("MINT-bs", Utc::now()), Some(&info2), Some(agg2), Vec::new());
        assert!(
            state2.market_cap.is_some(),
            "market_cap derived from newest price when not persisted"
        );
    }

    async fn test_pool() -> Option<PgPool> {
        let url = std::env::var("DATABASE_URL").ok()?;
        PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .ok()
    }

    /// Cap + single-scan aggregates: `for_each_seed_mint` keeps only the newest
    /// `cap` trades per mint (chronological), while the lifetime count/volume and
    /// the newest trade's price/reserves are carried from the *full* history in the
    /// same scan.
    #[tokio::test]
    #[ignore = "requires a local Postgres (DATABASE_URL); run with --ignored"]
    async fn for_each_seed_mint_caps_history_and_carries_lifetime_aggregates() {
        let Some(pool) = test_pool().await else { return };
        let trade_repo = TradeRepo::new(pool.clone());
        let mint = uniq("MINT-cap-");
        let wallet = uniq("W-cap-");
        let base = Utc::now();

        // 5 trades, slot/time ascending: sol_amounts 1..5, reserves 101..105.
        let mut sigs = Vec::new();
        for i in 1..=5i64 {
            let sig = uniq("sig-cap-");
            sigs.push(sig.clone());
            let mut t = crate::models::trade::Trade::new(
                mint.clone(),
                wallet.clone(),
                TradeType::Buy,
                i as f64,                          // sol_amount
                10.0,                              // token_amount → price = i/10
                sig,
                i as u64,                          // slot
                base + chrono::Duration::seconds(i),
            );
            t.virtual_token_reserves = Some(100.0 + i as f64);
            trade_repo.insert(&t).await.expect("insert trade");
        }

        let mut seen: Vec<(String, Vec<crate::models::trade::Trade>, SeedAgg)> = Vec::new();
        trade_repo
            .for_each_seed_mint(std::slice::from_ref(&mint), 3, |m, trades, agg| {
                seen.push((m, trades, agg));
            })
            .await
            .expect("seed scan");

        assert_eq!(seen.len(), 1, "exactly one mint group");
        let (got_mint, trades, agg) = &seen[0];
        assert_eq!(got_mint, &mint);
        assert_eq!(trades.len(), 3, "capped to newest 3 of 5");
        assert_eq!(trades[0].sol_amount, 3.0, "oldest kept = trade 3 (chronological)");
        assert_eq!(trades[2].sol_amount, 5.0, "newest kept = trade 5");
        assert_eq!(agg.lifetime_count, 5, "lifetime count spans full history");
        assert_eq!(agg.lifetime_volume, 15.0, "lifetime volume = 1+2+3+4+5");
        assert_eq!(agg.current_reserves, Some(105.0), "reserves from newest trade");
        assert_eq!(agg.newest_price, Some(0.5), "newest price = 5/10");

        let _ = sqlx::query("DELETE FROM trades WHERE mint_address = $1")
            .bind(&mint)
            .execute(&pool)
            .await;
    }

    /// Activity window: `find_recent_active` returns a token created inside the
    /// window and excludes one created before it, regardless of that token's trades.
    #[tokio::test]
    #[ignore = "requires a local Postgres (DATABASE_URL); run with --ignored"]
    async fn find_recent_active_excludes_tokens_outside_the_window() {
        let Some(pool) = test_pool().await else { return };
        let token_repo = TokenRepo::new(pool.clone());
        let now = Utc::now();
        let recent = uniq("MINT-recent-");
        let old = uniq("MINT-old-");
        token_repo.insert(&token(&recent, now)).await.expect("insert recent");
        token_repo
            .insert(&token(&old, now - chrono::Duration::days(30)))
            .await
            .expect("insert old");

        let cutoff = now - chrono::Duration::days(SEED_ACTIVITY_WINDOW_DAYS);
        let mints: HashSet<String> = token_repo
            .find_recent_active(SEED_TOKEN_LIMIT, cutoff)
            .await
            .expect("query")
            .into_iter()
            .map(|t| t.mint_address)
            .collect();

        assert!(mints.contains(&recent), "in-window token seeded");
        assert!(!mints.contains(&old), "out-of-window token excluded");

        for m in [&recent, &old] {
            let _ = sqlx::query("DELETE FROM tokens WHERE mint_address = $1")
                .bind(m)
                .execute(&pool)
                .await;
        }
    }

    /// Keyset paging: `find_page_before` tiles the table newest-first with no gap
    /// or duplicate across the `(created_at, mint_address)` tiebreak (incl. two rows
    /// sharing a `created_at`), and the optional `[since, until)` window clips to
    /// exactly the in-range rows.
    #[tokio::test]
    #[ignore = "requires a local Postgres (DATABASE_URL); run with --ignored"]
    async fn find_page_before_tiles_keyset_and_clips_window() {
        let Some(pool) = test_pool().await else { return };
        let token_repo = TokenRepo::new(pool.clone());
        // A unique base so the run's rows are isolable, despite scanning the whole table.
        let tag = uniq("");
        let now = Utc::now();
        // t0 newest … t4 oldest, 1 min apart; t2a/t2b SHARE t2's timestamp to force
        // the mint-address tiebreak across a page boundary.
        let t = |secs: i64| now - chrono::Duration::seconds(secs);
        let rows = [
            (format!("MINT-kp-0-{tag}"), t(0)),
            (format!("MINT-kp-1-{tag}"), t(60)),
            (format!("MINT-kp-2a-{tag}"), t(120)),
            (format!("MINT-kp-2b-{tag}"), t(120)),
            (format!("MINT-kp-3-{tag}"), t(180)),
            (format!("MINT-kp-4-{tag}"), t(240)),
        ];
        for (mint, created) in &rows {
            token_repo.insert(&token(mint, *created)).await.expect("insert");
        }
        let ours: HashSet<&String> = rows.iter().map(|(m, _)| m).collect();

        // Page the whole table at limit=2; keep only our run's mints. Pages must tile
        // with no gap/dupe and arrive in (created_at, mint) DESC order.
        let mut cursor: Option<(chrono::DateTime<Utc>, String)> = None;
        let mut seen: Vec<(chrono::DateTime<Utc>, String)> = Vec::new();
        loop {
            let page = token_repo
                .find_page_before(cursor.clone(), 2, None, None)
                .await
                .expect("page");
            if page.is_empty() {
                break;
            }
            cursor = page
                .last()
                .map(|t| (t.created_at, t.mint_address.clone()));
            for tk in &page {
                if ours.contains(&tk.mint_address) {
                    seen.push((tk.created_at, tk.mint_address.clone()));
                }
            }
            if page.len() < 2 {
                break;
            }
        }

        assert_eq!(seen.len(), rows.len(), "every row seen exactly once (no gap/dupe)");
        let mut sorted = seen.clone();
        sorted.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
        assert_eq!(seen, sorted, "rows arrive in (created_at, mint) DESC order");

        // Window: since = t(180) inclusive, until = t(60) exclusive → t2a, t2b, t3.
        let windowed: HashSet<String> = token_repo
            .find_page_before(None, 1000, Some(t(180)), Some(t(60)))
            .await
            .expect("windowed")
            .into_iter()
            .map(|tk| tk.mint_address)
            .filter(|m| ours.contains(m))
            .collect();
        assert!(windowed.contains(&format!("MINT-kp-2a-{tag}")), "in-window kept");
        assert!(windowed.contains(&format!("MINT-kp-2b-{tag}")), "in-window kept");
        assert!(windowed.contains(&format!("MINT-kp-3-{tag}")), "since is inclusive");
        assert!(!windowed.contains(&format!("MINT-kp-1-{tag}")), "until is exclusive");
        assert!(!windowed.contains(&format!("MINT-kp-4-{tag}")), "older than since dropped");

        for (mint, _) in &rows {
            let _ = sqlx::query("DELETE FROM tokens WHERE mint_address = $1")
                .bind(mint)
                .execute(&pool)
                .await;
        }
    }

    /// `collect_matching_tokens` retains only the tokens its `keep` predicate accepts
    /// and forwards the `[since, until)` window (here: a row outside the window is not
    /// even scanned, so it can't match even though `keep` would accept it).
    #[tokio::test]
    #[ignore = "requires a local Postgres (DATABASE_URL); run with --ignored"]
    async fn collect_matching_tokens_keeps_only_matches_within_window() {
        let Some(pool) = test_pool().await else { return };
        let token_repo = TokenRepo::new(pool.clone());
        let tag = uniq("");
        let now = Utc::now();
        let keep_in = format!("MINT-cm-keep-in-{tag}");
        let drop_in = format!("MINT-cm-drop-in-{tag}");
        let keep_out = format!("MINT-cm-keep-out-{tag}"); // matches `keep` but pre-window
        token_repo.insert(&token(&keep_in, now)).await.expect("insert");
        token_repo.insert(&token(&drop_in, now)).await.expect("insert");
        token_repo
            .insert(&token(&keep_out, now - chrono::Duration::days(10)))
            .await
            .expect("insert");

        // Window starts 1 day ago, so `keep_out` (10 days old) is outside it.
        let since = Some(now - chrono::Duration::days(1));
        let got: HashSet<String> = crate::strategies::analysis::collect_matching_tokens(
            &token_repo,
            since,
            None,
            |t| t.mint_address.contains("keep"),
        )
        .await
        .expect("scan")
        .into_iter()
        .map(|t| t.mint_address)
        .collect();

        assert!(got.contains(&keep_in), "matching, in-window token kept");
        assert!(!got.contains(&drop_in), "non-matching token dropped by `keep`");
        assert!(!got.contains(&keep_out), "matching token outside the window not scanned");

        for m in [&keep_in, &drop_in, &keep_out] {
            let _ = sqlx::query("DELETE FROM tokens WHERE mint_address = $1")
                .bind(m)
                .execute(&pool)
                .await;
        }
    }

    /// Position safety net: `distinct_unsettled_mints` surfaces a mint with a
    /// Holding position (so the seed always tracks it, even outside the window) and
    /// omits one whose only position has settled (`End`).
    #[tokio::test]
    #[ignore = "requires a local Postgres (DATABASE_URL); run with --ignored"]
    async fn distinct_unsettled_mints_returns_open_omits_settled() {
        let Some(pool) = test_pool().await else { return };
        let rule_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO tpsl1_strategy_rules (id, rule_name, buy_amount, p_exit_take_profit, p_exit_stop_loss)
             VALUES ($1, $2, 0.1, 1.5, 0.5)",
        )
        .bind(rule_id)
        .bind(uniq("rule-"))
        .execute(&pool)
        .await
        .expect("insert rule");

        let open_mint = uniq("MINT-open-");
        let settled_mint = uniq("MINT-settled-");
        for (mint, status) in [(&open_mint, "Holding"), (&settled_mint, "End")] {
            sqlx::query(
                "INSERT INTO tpsl1_real_positions
                   (mint, wallet, entry_price, entry_amount, entry_tx, status, strategy, rule_id)
                 VALUES ($1, $2, 0.001, 1000.0, $3, $4, 'tpsl1', $5)",
            )
            .bind(mint)
            .bind(uniq("W-pos-"))
            .bind(uniq("entry-sig-"))
            .bind(status)
            .bind(rule_id)
            .execute(&pool)
            .await
            .expect("insert position");
        }

        let mints: HashSet<String> = Tpsl1PositionRepo::new(pool.clone())
            .distinct_unsettled_mints()
            .await
            .expect("query")
            .into_iter()
            .collect();

        assert!(mints.contains(&open_mint), "Holding mint is unsettled");
        assert!(!mints.contains(&settled_mint), "End mint is settled");

        let _ = sqlx::query("DELETE FROM tpsl1_real_positions WHERE rule_id = $1")
            .bind(rule_id)
            .execute(&pool)
            .await;
        let _ = sqlx::query("DELETE FROM tpsl1_strategy_rules WHERE id = $1")
            .bind(rule_id)
            .execute(&pool)
            .await;
    }
}
