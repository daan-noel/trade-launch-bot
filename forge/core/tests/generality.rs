//! The key generalization proof (plan §7 step 4 / §8): a USDC-quoted token and a
//! SOL-quoted token flow through the SAME `trades` table and the SAME
//! `token_overview` / `trades_priced` views, USD-comparable, with NO schema
//! change. Also exercises the boot path (migrations + cagg setup) and the
//! repos' UNNEST array bindings against a real Postgres/TimescaleDB.
//!
//! DB-gated: set `PLATFORM_TEST_DATABASE_URL` to a DEDICATED throwaway database
//! (the test runs migrations + mutates seed rows). Self-skips otherwise so
//! `cargo test` stays green on machines without a DB.

use chrono::Utc;

use platform_core::config::Settings;
use platform_core::models::{NewToken, NewTrade, TokenMarketState};
use platform_core::storage::connect;
use platform_core::storage::repositories::{
    QuoteAssetRepo, TokenMarketStateRepo, TokenRepo, TradeRepo, WalletDictRepo,
};

const SOL_MINT: &str = "TESTGEN_SOL";
const USDC_MINT: &str = "TESTGEN_USDC";
const SOL_QID: i16 = 1;
const USDC_QID: i16 = 2;
const PUMP_FUN: i16 = 1;

fn mk_token(mint: &str, quote_id: i16) -> NewToken {
    NewToken {
        mint_address: mint.to_string(),
        launchpad_id: PUMP_FUN,
        quote_asset_id: quote_id,
        creator_wallet: "TESTGEN_creator".to_string(),
        is_own_launch: false,
        name: mint.to_string(),
        symbol: mint.to_string(),
        decimals: 6, // both tokens: 1e9 display supply (1e15 base units)
        token_program_id: None,
        initial_supply_base: Some(1_000_000_000_000_000),
        initial_buy_quote: None,
        creation_slot: Some(100),
        creation_tx_signature: format!("sig_{mint}"),
        ix_labels: None,
        meta: None,
        created_at: Some(Utc::now()),
    }
}

fn mk_trade(mint: &str, quote_id: i16, wallet_ref: i32, sig: &[u8]) -> NewTrade {
    NewTrade {
        mint_address: mint.to_string(),
        wallet_ref,
        launchpad_id: PUMP_FUN,
        market_kind: "bonding_curve".to_string(),
        quote_asset_id: quote_id,
        trade_type: "buy".to_string(),
        // 1e9 quote base units for 1e12 base tokens → raw ratio 1e-3
        amount_quote: 1_000_000_000,
        amount_base: 1_000_000_000_000,
        reserve_quote: Some(30_000_000_000),
        reserve_base: Some(1_000_000_000_000_000),
        slot: 100,
        tx_index: 0,
        leg_index: 0,
        block_time: Utc::now(),
        tx_signature: sig.to_vec(),
    }
}

#[tokio::test]
async fn generalization_usdc_and_sol_share_the_same_views() {
    let Ok(url) = std::env::var("PLATFORM_TEST_DATABASE_URL") else {
        eprintln!("SKIP generality: set PLATFORM_TEST_DATABASE_URL to a throwaway DB to run it");
        return;
    };
    std::env::set_var("DATABASE_URL", &url);
    let settings = Settings::from_env().expect("settings");

    // connect() applies migrations + cagg setup — plan §8 "migrations apply clean".
    let pools = connect(&settings).await.expect("connect + migrate");
    let pool = &pools.hot;

    // Clean slate (idempotent re-runs). tokens cascade to state/markets/sync.
    sqlx::query("DELETE FROM trades WHERE mint_address = ANY($1)")
        .bind(vec![SOL_MINT, USDC_MINT])
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tokens WHERE mint_address = ANY($1)")
        .bind(vec![SOL_MINT, USDC_MINT])
        .execute(pool)
        .await
        .unwrap();

    // Give SOL a USD rate (the seed leaves it NULL for the poller); USDC seeded ≈1.
    QuoteAssetRepo::set_usd_rate(pool, SOL_QID, 150.0).await.unwrap();
    let usdc = QuoteAssetRepo::get(pool, USDC_QID).await.unwrap().unwrap();
    assert_eq!(usdc.usd_rate, Some(1.0), "USDC seeded at usd_rate=1");

    // Insert both tokens + market state via the repos.
    assert!(TokenRepo::insert(pool, &mk_token(SOL_MINT, SOL_QID)).await.unwrap());
    assert!(TokenRepo::insert(pool, &mk_token(USDC_MINT, USDC_QID)).await.unwrap());

    for mint in [SOL_MINT, USDC_MINT] {
        TokenMarketStateRepo::upsert(
            pool,
            &TokenMarketState {
                mint_address: mint.to_string(),
                current_price_quote: Some(0.001), // raw ratio quote_bu/base_bu
                ath_price_quote: Some(0.001),
                ath_at: Some(Utc::now()),
                volume_quote: 1_000_000_000,
                trade_count: 1,
                last_trade_at: Some(Utc::now()),
                is_dead: false,
                is_migrated: false,
                updated_at: Utc::now(),
            },
        )
        .await
        .unwrap();
    }

    // Trades via the UNNEST batch path (validates array bindings incl. Option/bytea).
    let wid = WalletDictRepo::intern(pool, "TESTGEN_wallet").await.unwrap();
    let inserted = TradeRepo::insert_batch(
        pool,
        &[
            mk_trade(SOL_MINT, SOL_QID, wid, &[0xA1]),
            mk_trade(USDC_MINT, USDC_QID, wid, &[0xA2]),
        ],
    )
    .await
    .unwrap();
    assert_eq!(inserted, 2, "both trade rows land via UNNEST");

    // ---- token_overview: same view, USD-comparable across quotes ----
    let sol = TokenRepo::overview(pool, SOL_MINT).await.unwrap().unwrap();
    let usd = TokenRepo::overview(pool, USDC_MINT).await.unwrap().unwrap();

    assert_eq!(sol.quote_symbol, "SOL");
    assert_eq!(usd.quote_symbol, "USDC");
    // supply 1e9 display tokens × price → mcap; ×usd_rate → USD.
    // SOL:  1000 SOL  × 150 = 150_000 USD ;  USDC: 1_000_000 × 1 = 1_000_000 USD
    approx(sol.market_cap_quote.unwrap(), 1_000.0, "SOL mcap (quote)");
    approx(sol.market_cap_usd.unwrap(), 150_000.0, "SOL mcap (USD)");
    approx(usd.market_cap_quote.unwrap(), 1_000_000.0, "USDC mcap (quote)");
    approx(usd.market_cap_usd.unwrap(), 1_000_000.0, "USDC mcap (USD)");

    // ---- trades_priced: same view, amount_usd comparable across quotes ----
    let sol_tr = TradeRepo::find_priced_by_mint(pool, SOL_MINT, 0).await.unwrap();
    let usd_tr = TradeRepo::find_priced_by_mint(pool, USDC_MINT, 0).await.unwrap();
    assert_eq!(sol_tr.len(), 1);
    assert_eq!(usd_tr.len(), 1);
    approx(sol_tr[0].amount_usd.unwrap(), 150.0, "SOL trade amount (USD)");
    approx(usd_tr[0].amount_usd.unwrap(), 1_000.0, "USDC trade amount (USD)");
    // raw exec ratio identical (1e-3) though quotes differ — proves the store is
    // decimals-agnostic and USD scaling lives only in the view.
    approx(sol_tr[0].exec_price_quote.unwrap(), 0.001, "SOL exec ratio");
    approx(usd_tr[0].exec_price_quote.unwrap(), 0.001, "USDC exec ratio");

    // Cleanup so the throwaway DB stays re-runnable.
    sqlx::query("DELETE FROM trades WHERE mint_address = ANY($1)")
        .bind(vec![SOL_MINT, USDC_MINT])
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tokens WHERE mint_address = ANY($1)")
        .bind(vec![SOL_MINT, USDC_MINT])
        .execute(pool)
        .await
        .unwrap();
}

fn approx(got: f64, want: f64, what: &str) {
    assert!(
        (got - want).abs() < 1e-6 * want.max(1.0),
        "{what}: got {got}, want {want}"
    );
}
