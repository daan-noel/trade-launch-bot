# If I sync a new token, does it update all trades of the token?
# I mean, does it compare all the old token trades with new ones?

No, it does not update or compare existing trades. Here's what actually happens on a sync:

1. It fetches the **entire** signature history for the bonding curve from Helius (full re-fetch every time).
2. It decodes each transaction into trade events and inserts each one via `trade_repo.insert`.
3. That insert is idempotent — duplicates are simply skipped, not updated:

```93:106:backend/src/storage/repositories/trade_repo.rs
    /// Insert a trade. Ignores duplicates (idempotent on replay).
    pub async fn insert(&self, trade: &Trade) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO trades
                (id, mint_address, wallet_address, trade_type,
                ...
            ON CONFLICT (tx_signature, leg_index) DO NOTHING
            "#,
        )
```

So the behavior is **additive only**:
- Trades already in the DB (same `tx_signature` + `leg_index`) are left **untouched** — no field is recompared or overwritten.
- Only genuinely new trades get inserted.

After inserting, it reloads *all* trades for the mint (`find_by_mint_all`) and recomputes the token's metrics/state from that full set — but the individual old trade rows themselves are never modified.

One contrast: the **token** record itself *is* upserted (`ON CONFLICT ... DO UPDATE`), so token metadata can change on sync. Trades cannot.

So if a previously-stored trade was somehow wrong, a re-sync will **not** fix it — you'd have to delete those rows first.