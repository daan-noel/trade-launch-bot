# Token sync: how does a re-sync treat existing trades?

> Status (2026-06-11): updated. The old "full re-fetch every time + trades are
> never updated" answer below is superseded — sync is now dual-mode and trade
> rows are upserted on replay. See `backend/src/services/token_sync.rs` and
> `backend/src/services/helius_rpc.rs`.

## Two sync modes

Sync is no longer a single "re-fetch the whole history" path. The mode is chosen
by `req.incremental`:

- **Fetch All** (`incremental = false`) — full backfill via the archival
  `getTransactionsForAddress` ("gTFA"), one cursor-paginated call returning full
  transactions at ~0.1 credit/tx (vs 1 credit/tx for per-sig `getTransaction`).
  Deliberately re-fetches the entire history so decoder fixes propagate (see
  upsert below). See `acquire_full_via_gtfa`.
- **Fetch New** (`incremental = true`) — pages `getSignaturesForAddress` from the
  last curve watermark, then **dedups against trades already saved**
  (`TradeRepo::saved_signatures`, e.g. rows live ingest persisted ahead of the
  sync) and only `getTransaction`s the genuinely-new signatures. Cheaper than
  paying gTFA's per-tx rate over a range live ingest mostly already has.

(LaserStream replay fast-path: a recently-synced token's Fetch New tries the
LaserStream replay window first — zero Helius credits — before the RPC path. It is
**on by default** when `HELIUS_LASERSTREAM_URL` is set, gated by that URL's presence
plus a watermark-age window (`REPLAY_WINDOW_SECS`, 20 h) — there is **no**
`SYNC_REPLAY_FETCH_NEW` env toggle. See `try_replay` in `token_sync.rs` and
`services/laserstream_replay.rs`.)

## Are existing trade rows updated?

**Yes, now — partially.** Trade insert is idempotent but is an **upsert**, not a
skip:

```98:117:backend/src/storage/repositories/trade_repo.rs
    /// Insert a trade. On replay, refresh the decoded price/reserve columns so
    /// decoder fixes (e.g. AMM pre- vs post-swap reserves) propagate on re-sync,
    /// while preserving identity/time columns (`id`, `received_at`).
    pub async fn insert(&self, trade: &Trade) -> anyhow::Result<()> {
        ...
            ON CONFLICT (tx_signature, leg_index) DO UPDATE SET
                price_per_token        = EXCLUDED.price_per_token,
                virtual_sol_reserves   = EXCLUDED.virtual_sol_reserves,
                virtual_token_reserves = EXCLUDED.virtual_token_reserves,
                real_sol_reserves      = EXCLUDED.real_sol_reserves,
                real_token_reserves    = EXCLUDED.real_token_reserves
```

So on a conflicting `(tx_signature, leg_index)`:

- The **decoded** columns (price + virtual/real reserves) are **refreshed** from
  the re-decoded transaction — this is what makes a Fetch All re-sync repair old
  rows after a decoder fix (e.g. the AMM pre-swap-reserves bug).
- Identity/time columns (`id`, `received_at`) and the amount columns are **left
  as-is**.

After inserting, sync reloads *all* trades for the mint (`find_by_mint_all`) and
recomputes the token's metrics/state from that full set. The token record itself
is also upserted (`ON CONFLICT ... DO UPDATE`), so token metadata can change on
sync.

**Bottom line:** to propagate a decoder fix to historical rows, run **Fetch All**
(it re-decodes and upserts the price/reserve columns). Fetch New only touches
genuinely-new trades.
