# A2 — Successful real buys never become "Holding" (Error 2)

> Workstream A (tpsl-realtime). Run after [A1](01-concurrency-caps-inflight.md) — A1 removes
> most of the overload that triggers this. Apply to **both** TPSL1 + TPSL2 (clones).
> Paths are pre-crate-split — see [../README.md](../README.md#-path-caveat--line-refs-predate-the-crate-split).

## Report

10+ tokens had a buy tx sent; the real buys **succeeded on-chain** (confirmed on GMGN), but
**none** were saved as `Holding`.

## How a buy becomes Holding (normal path)

Confirmed **only via the `trades` gRPC feed**, never RPC:

1. `buy_until_filled_or_give_up` submits the buy, persists the signature, flips the row to
   `BuySubmitted` via the write-ahead `on_signed` hook
   ([real.rs:356-357](../../backend/src/strategies/tpsl_sniper_1/execution/real.rs#L356-L357)).
2. Polls the feed: `poll_feed_until_entry_fill` → `adopt_existing_fill_if_present` →
   `trade_repo.find_fill_by_signature(wallet, mint, sig)`
   ([real.rs:469-555](../../backend/src/strategies/tpsl_sniper_1/execution/real.rs#L469-L555)).
   First matching `trades` row → `update_entry` → `Holding`.
3. **Adoption window is short:** `BUY_POLL_MAX_ATTEMPTS = 12 × BUY_POLL_INTERVAL_MS = 1000ms`
   ≈ **12 s** ([execution/mod.rs:14-16](../../backend/src/strategies/tpsl_sniper_1/execution/mod.rs#L14-L16)).
   On timeout, one on-chain status check
   ([real.rs:415-449](../../backend/src/strategies/tpsl_sniper_1/execution/real.rs#L415-L449)):
   landed-but-not-indexed → `WaitThenSettle` polls one more ~12 s window, then returns the
   position still **unentered**.

## Root cause — overload + an over-aggressive inline cleanup

Matching itself is fine (wallet + mint + exact submitted signature). What fails is **timing
under overload**, compounded by a destructive inline cleanup:

1. **Overload (caused by Error 1).** [A1](01-concurrency-caps-inflight.md) fired 10+
   simultaneous real buys on the single EC2 box (2 vCPU / 4 GB, IO-bound). The
   `trades` feed → DB-writer → index pipeline fell behind, so the bot's own buy rows weren't
   queryable within the ~12 s (×2) adoption window. Every buy timed out unentered.
2. **Inline cleanup deletes landed-but-unindexed positions.** When the buy task returns
   unentered, the spawned task immediately deletes the position —
   [service.rs:373-385](../../backend/src/strategies/tpsl_sniper_1/service.rs#L373-L385):
   ```rust
   if let Ok(Some(pos)) = position_repo.find_by_id(position_id).await {
       if pos.entry_price.is_none() {          // BuySubmitted, fill not yet indexed
           trader.release_sol_for_position(...);
           let _ = position_repo.delete_position(position_id).await;  // ⚠️ orphans tokens
           runtime.remove_position(&pos);
       }
   }
   ```
   This deletes **any** unentered position — including the `WaitThenSettle` case where the buy
   **provably landed on-chain**. The tokens are now held with no position tracking them → never
   marked Holding, never sold.
3. **It pre-empts the safe reaper.** The periodic `redrive_orphaned_buy_submitted`
   ([service.rs:926-1004](../../backend/src/strategies/tpsl_sniper_1/service.rs#L926-L1004),
   ticked at service.rs:131) is designed to **never delete a `BuySubmitted` row that might own
   tokens** — it re-runs `adopt_existing_fill_if_present` once the row indexes and drops only if
   **every** submitted signature is a *confirmed revert*. The inline cleanup deletes the row
   first, so the reaper never sees it. The inline path directly contradicts the reaper's safety
   model.

**Net:** genuinely-successful buys → unentered past the window → deleted inline → tokens
orphaned. That's why **all** of them failed, not a random few.

## Fix (recommended)

1. **Remove the destructive inline cleanup** at
   [service.rs:373-385](../../backend/src/strategies/tpsl_sniper_1/service.rs#L373-L385) — or
   gate it to delete **only** when every submitted signature is a confirmed on-chain revert
   (mirror the reaper's `classify_submitted_buy` / `BuyRecoveryVerdict`). Leave landed-or-unknown
   buys as `BuySubmitted` and let `redrive_orphaned_buy_submitted` own adopt/wait/drop.
2. **Add `release_sol_for_position` to the reaper's confirmed-revert drop branch**
   ([service.rs:983-988](../../backend/src/strategies/tpsl_sniper_1/service.rs#L983-L988)).
   The reaper calls `delete_position` + `remove_position` but never releases the SOL commitment.
   The inline cleanup (service.rs:378) was the only call site — once removed, every
   confirmed-revert handled by the reaper leaks committed SOL, and the budget tracker eventually
   refuses all new buys until restart. Fix:
   `self.trader.release_sol_for_position(&position.id.to_string()).await` immediately before/after
   `remove_position` in the revert-drop branch.
3. **A1 removes most of the trigger** — without 10+ concurrent buys the indexing pipeline keeps
   up and the ~12 s window is sufficient. The two fixes are complementary: A1 stops the overload,
   this stops orphaning when a buy does outrun the window.

## Scope & done

- Mirror in **TPSL1 + TPSL2** (same inline-cleanup and reaper logic).
- `cargo check -p backend-deploy` clean; clippy on touched code; verify the reaper's SOL-release
  path with a unit/integration check.
