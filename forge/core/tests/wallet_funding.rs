//! Guard: `ManagedWalletRepo` funding-claim atomicity (docs/plans/wallet/wallet-management.md)
//! . A treasury→wallet send costs real SOL, so `claim_for_funding` MUST hand a
//! given `generated` wallet to exactly one concurrent funder, and `revert_funding`
//! MUST only touch wallets currently `funding` (never clobber a wallet that raced
//! ahead to `funded`).
//!
//! DB-gated: set `PLATFORM_TEST_DATABASE_URL` to a DEDICATED throwaway database
//! (runs migrations + mutates rows). Self-skips otherwise so `cargo test` stays
//! green on machines without a DB.

use platform_core::config::Settings;
use platform_core::models::{NewManagedWallet, WalletStatus};
use platform_core::storage::connect;
use platform_core::storage::repositories::ManagedWalletRepo;
use uuid::Uuid;

fn mk_wallet(role: &str) -> NewManagedWallet {
    let id = Uuid::new_v4();
    NewManagedWallet {
        address: format!("FUNDTEST_{id}"),
        label: Some("fund-test".to_string()),
        role: role.to_string(),
        key_ref: format!("fund-test-{id}.enc"),
        derivation_index: None,
    }
}

#[tokio::test]
async fn claim_for_funding_is_atomic_and_revert_is_scoped() {
    let Ok(url) = std::env::var("PLATFORM_TEST_DATABASE_URL") else {
        eprintln!(
            "SKIP wallet_funding: set PLATFORM_TEST_DATABASE_URL to a throwaway DB to run it"
        );
        return;
    };
    std::env::set_var("DATABASE_URL", &url);
    let settings = Settings::from_env().expect("settings");
    let pools = connect(&settings).await.expect("connect + migrate");
    let pool = &pools.hot;

    // Isolated role for this test so a shared DB's other rows don't interfere.
    let role = "trading";
    sqlx::query("DELETE FROM managed_wallets WHERE address LIKE 'FUNDTEST_%'")
        .execute(pool)
        .await
        .unwrap();

    const N: i64 = 8;
    for _ in 0..N {
        ManagedWalletRepo::insert(pool, &mk_wallet(role)).await.unwrap();
    }

    // Two funders race, each asking for the whole pool. SKIP LOCKED must hand each
    // wallet to exactly one — the union is disjoint and totals at most N.
    let (a, b) = tokio::join!(
        ManagedWalletRepo::claim_for_funding(pool, role, N, "funder-a"),
        ManagedWalletRepo::claim_for_funding(pool, role, N, "funder-b"),
    );
    let a = a.unwrap();
    let b = b.unwrap();

    let ids_a: std::collections::HashSet<Uuid> = a.iter().map(|w| w.id).collect();
    let overlap = b.iter().filter(|w| ids_a.contains(&w.id)).count();
    assert_eq!(overlap, 0, "a wallet was claimed by both funders (double-fund)");
    assert!(
        (a.len() + b.len()) as i64 <= N,
        "claimed more wallets than exist"
    );
    for w in a.iter().chain(b.iter()) {
        assert_eq!(w.status, WalletStatus::Funding.as_str());
        assert!(w.funding_source.is_some(), "funding_source stamped on claim");
    }

    // revert only touches `funding`. Promote one claimed wallet to `funded` (as
    // the poller would), then revert the whole claimed set: the funded one must
    // survive; the rest go back to `generated`.
    let all_claimed: Vec<Uuid> = a.iter().chain(b.iter()).map(|w| w.id).collect();
    let promoted = all_claimed[0];
    ManagedWalletRepo::record_balance(pool, promoted, 1_000_000_000, 1_000_000)
        .await
        .unwrap();

    let reverted = ManagedWalletRepo::revert_funding(pool, &all_claimed).await.unwrap();
    assert_eq!(
        reverted as usize,
        all_claimed.len() - 1,
        "revert skipped the wallet that already reached funded"
    );

    let survivor = ManagedWalletRepo::get(pool, promoted).await.unwrap().unwrap();
    assert_eq!(survivor.status, WalletStatus::Funded.as_str());
    for id in &all_claimed[1..] {
        let w = ManagedWalletRepo::get(pool, *id).await.unwrap().unwrap();
        assert_eq!(w.status, WalletStatus::Generated.as_str());
        assert!(w.funding_source.is_none(), "funding_source cleared on revert");
    }

    // Cleanup.
    sqlx::query("DELETE FROM managed_wallets WHERE address LIKE 'FUNDTEST_%'")
        .execute(pool)
        .await
        .unwrap();
}
