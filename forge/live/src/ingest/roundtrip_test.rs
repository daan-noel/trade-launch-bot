//! Ingest round-trip proof (plan §8) — WITHOUT the live network. Constructs
//! synthetic `ingest-laserstream` events, runs them through the SAME mappers +
//! repos the consumer uses, and reads them back: a decoded trade lands in
//! `trades` with `launchpad_id = pump_fun`, `quote_asset_id = SOL`, the reserve
//! pair populated, and `trades_priced` prices it correctly; the token lands via
//! the create event; the raw tx lands in `raw_txs`.
//!
//! DB-gated: set `PLATFORM_TEST_DATABASE_URL` to a throwaway DB; self-skips otherwise.

use chrono::Utc;

use ingest_laserstream::event::{RawTx as IlRawTx, Reserves, Side, TokenCreated, Trade, Venue};

use super::map;
use super::pumpfun::PumpFunAdapter;
use platform_core::config::Settings;
use platform_core::storage::connect;
use platform_core::storage::repositories::{
    QuoteAssetRepo, RawTxRepo, TokenRepo, TradeRepo, WalletDictRepo,
};
use platform_core::units::sol_to_lamports;
use platform_core::venue::LaunchpadAdapter;

const MINT: &str = "ROUNDTRIP_MINT";

#[tokio::test]
async fn pump_fun_events_project_onto_the_schema() {
    let Ok(url) = std::env::var("PLATFORM_TEST_DATABASE_URL") else {
        eprintln!("SKIP roundtrip: set PLATFORM_TEST_DATABASE_URL to a throwaway DB to run it");
        return;
    };
    std::env::set_var("DATABASE_URL", &url);
    let settings = Settings::from_env().unwrap();
    let pools = connect(&settings).await.unwrap();
    let pool = &pools.hot;

    // clean slate + SOL usd rate for the amount_usd spot-check.
    sqlx::query("DELETE FROM trades WHERE mint_address = $1").bind(MINT).execute(pool).await.unwrap();
    sqlx::query("DELETE FROM tokens WHERE mint_address = $1").bind(MINT).execute(pool).await.unwrap();
    QuoteAssetRepo::set_usd_rate(pool, 1, 150.0).await.unwrap();

    let adapter = PumpFunAdapter::resolve(pool).await.unwrap();

    // ---- create event → tokens row ----
    let tc = TokenCreated {
        mint: MINT.to_string(),
        creator: "ROUNDTRIP_creator".to_string(),
        name: "Roundtrip".to_string(),
        symbol: "RT".to_string(),
        token_program_id: Some("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA".to_string()),
        bonding_curve: Some("ROUNDTRIP_curve".to_string()),
        initial_supply: Some(1_000_000_000_000_000),
        initial_buy_sol: Some(0.5),
        initial_buy_instruction: None,
        cu_limit: Some(200_000),
        cu_price: Some(1_000),
        is_mayhem_mode: false,
        is_cashback_enabled: false,
        instruction_labels: vec!["Create".to_string()],
        signature: "create_sig".to_string(),
        slot: 100,
        block_time: Utc::now(),
        received_at: Utc::now(),
    };
    let token = map::token_created_to_row(&adapter, &tc);
    assert!(TokenRepo::insert(pool, &token).await.unwrap());

    // ---- trade event → trades row ----
    let sig_b58 = bs58::encode(vec![9u8; 64]).into_string();
    let trade = Trade {
        mint: MINT.to_string(),
        wallet: "ROUNDTRIP_wallet".to_string(),
        side: Side::Buy,
        sol: 1.5,                       // human-SOL mirror
        sol_lamports: 1_500_000_000,    // exact quote lamports (what forge persists)
        tokens: 1_000_000,              // raw base units
        price: 1.5e-6,
        signature: sig_b58,
        tx_index: 3,
        leg_index: 0,
        slot: 101,
        block_time: Utc::now(),
        received_at: Utc::now(),
        reserves: Reserves {
            virtual_sol: Some(30.0),
            virtual_token: Some(1_000_000_000),
            real_sol: Some(0.0),
            real_token: Some(0),
            virtual_sol_lamports: Some(30_000_000_000),
            real_sol_lamports: Some(0),
        },
        venue: Venue::Curve,
        instruction_type: "Buy".to_string(),
        instruction_labels: vec!["Buy".to_string()],
    };
    let wid = WalletDictRepo::intern(pool, &trade.wallet).await.unwrap();
    let row = map::trade_to_row(&adapter, wid, &trade).unwrap();
    assert_eq!(TradeRepo::insert_batch(pool, &[row]).await.unwrap(), 1);

    // ---- raw tx event → raw_txs row ----
    let raw = IlRawTx {
        signature: vec![9u8; 64],
        slot: 101,
        tx_index: 3,
        block_time: Utc::now(),
        payload: vec![0xDE, 0xAD, 0xBE, 0xEF],
    };
    assert_eq!(RawTxRepo::insert_batch(pool, &[map::raw_tx_to_row(&raw)]).await.unwrap(), 1);

    // ---- read back + assert the projection ----
    let ov = TokenRepo::overview(pool, MINT).await.unwrap().unwrap();
    assert_eq!(ov.quote_symbol, "SOL");
    assert_eq!(ov.launchpad_id, adapter.launchpad_id());

    let priced = TradeRepo::find_priced_by_mint(pool, MINT, 0).await.unwrap();
    assert_eq!(priced.len(), 1);
    let t = &priced[0];
    assert_eq!(t.launchpad_id, adapter.launchpad_id());
    assert_eq!(t.quote_asset_id, 1, "SOL");
    assert_eq!(t.market_kind, "bonding_curve");
    assert_eq!(t.trade_type, "buy");
    assert_eq!(t.amount_quote, sol_to_lamports(1.5)); // 1_500_000_000
    assert_eq!(t.amount_base, 1_000_000);
    assert_eq!(t.reserve_quote, Some(sol_to_lamports(30.0)));
    assert_eq!(t.reserve_base, Some(1_000_000_000));
    // raw ratios: exec = 1.5e9/1e6 = 1500 ; spot = 30e9/1e9 = 30
    approx(t.exec_price_quote.unwrap(), 1500.0, "exec ratio");
    approx(t.spot_price_quote.unwrap(), 30.0, "spot ratio");
    // amount_usd = 1.5 SOL × 150 = 225
    approx(t.amount_usd.unwrap(), 225.0, "amount usd");

    let raw_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM raw_txs WHERE tx_signature = $1",
    )
    .bind(vec![9u8; 64])
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(raw_count, 1, "raw tx landed");

    // cleanup
    sqlx::query("DELETE FROM trades WHERE mint_address = $1").bind(MINT).execute(pool).await.unwrap();
    sqlx::query("DELETE FROM tokens WHERE mint_address = $1").bind(MINT).execute(pool).await.unwrap();
}

fn approx(got: f64, want: f64, what: &str) {
    assert!((got - want).abs() < 1e-6 * want.max(1.0), "{what}: got {got}, want {want}");
}
