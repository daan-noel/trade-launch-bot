//! DB-gated guard for the dust-sweep fix: `managed_wallet_ids_with_open_positions`
//! must return exactly the wallets holding an OPEN token position — the set the
//! dust sweep skips so it never drains a token-holding wallet's gas SOL (which
//! would strand its sell at "confirmation timed out"). A `closed` (sold out) or
//! `dropped` (bundle never landed) position must NOT protect a wallet.
//!
//! DB-gated: set `PLATFORM_TEST_DATABASE_URL` to a DEDICATED throwaway database
//! (the test runs migrations + mutates seed rows). Self-skips otherwise so
//! `cargo test` stays green on machines without a DB.

use platform_core::config::Settings;
use platform_core::models::{NewManagedWallet, WalletRole};
use platform_core::storage::connect;
use platform_core::storage::repositories::{ManagedWalletRepo, TokenPositionRepo};

const MINT: &str = "TESTGUARD_MINT";
const W_OPEN: &str = "TESTGUARD_open";
const W_CLOSED: &str = "TESTGUARD_closed";
const W_DROPPED: &str = "TESTGUARD_dropped";
const W_NONE: &str = "TESTGUARD_none";

fn mk_wallet(addr: &str, role: WalletRole) -> NewManagedWallet {
    NewManagedWallet {
        address: addr.to_string(),
        label: None,
        role: role.as_str().to_string(),
        key_ref: format!("{addr}.enc"),
        derivation_index: None,
    }
}

#[tokio::test]
async fn open_positions_guard_returns_only_token_holders() {
    let Ok(url) = std::env::var("PLATFORM_TEST_DATABASE_URL") else {
        eprintln!(
            "SKIP dust_sweep_guard: set PLATFORM_TEST_DATABASE_URL to a throwaway DB to run it"
        );
        return;
    };
    std::env::set_var("DATABASE_URL", &url);
    let settings = Settings::from_env().expect("settings");
    let pools = connect(&settings).await.expect("connect + migrate");
    let pool = &pools.hot;

    // Clean slate (idempotent re-runs): positions first (they FK the wallets).
    cleanup(pool).await;

    let open = ManagedWalletRepo::insert(pool, &mk_wallet(W_OPEN, WalletRole::Dev)).await.unwrap();
    let closed =
        ManagedWalletRepo::insert(pool, &mk_wallet(W_CLOSED, WalletRole::Dev)).await.unwrap();
    let dropped = ManagedWalletRepo::insert(pool, &mk_wallet(W_DROPPED, WalletRole::Bundler))
        .await
        .unwrap();
    let none = ManagedWalletRepo::insert(pool, &mk_wallet(W_NONE, WalletRole::Dev)).await.unwrap();

    // open: freshly seeded row (status defaults `open`) — holds tokens / not yet
    // reconciled. MUST be protected.
    TokenPositionRepo::seed(pool, MINT, open.id, WalletRole::Dev.as_str(), 20_000_000).await.unwrap();
    // closed: seeded then reconciled to a 0 balance → `closed` (sold out). Sweepable.
    let closed_pos =
        TokenPositionRepo::seed(pool, MINT, closed.id, WalletRole::Dev.as_str(), 20_000_000)
            .await
            .unwrap();
    TokenPositionRepo::set_balance(pool, closed_pos.id, 0, Some("TESTGUARD_ta")).await.unwrap();
    // dropped: bundle never landed → `dropped` (never bought). Sweepable.
    TokenPositionRepo::seed_dropped(pool, MINT, dropped.id, WalletRole::Bundler.as_str())
        .await
        .unwrap();
    // none: no position row at all. Sweepable.

    let holders = TokenPositionRepo::managed_wallet_ids_with_open_positions(pool).await.unwrap();
    assert!(holders.contains(&open.id), "open-position wallet must be protected from sweep");
    assert!(!holders.contains(&closed.id), "closed (sold out) wallet must stay sweepable");
    assert!(!holders.contains(&dropped.id), "dropped (never bought) wallet must stay sweepable");
    assert!(!holders.contains(&none.id), "wallet with no position must stay sweepable");

    cleanup(pool).await;
}

async fn cleanup(pool: &sqlx::PgPool) {
    sqlx::query("DELETE FROM token_positions WHERE mint_address = $1")
        .bind(MINT)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM managed_wallets WHERE address = ANY($1)")
        .bind(vec![W_OPEN, W_CLOSED, W_DROPPED, W_NONE])
        .execute(pool)
        .await
        .unwrap();
}
