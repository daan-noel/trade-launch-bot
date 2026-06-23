# Real-Trading Hardening Plan (single wallet)

> Goal: make the bot **safe to run with real SOL** on a single wallet.

All hardening work is **shipped** — per-signature attribution, ExitPending recovery, and the full 3-layer buy-in-flight recovery (durable marker + write-ahead ordering + boot wallet sweep). Builds/tests/clippy clean. See `@project_plans/trade-execution/buy-in-flight-recovery.md` and `@docs/strategies.md`.

The only work left is the two deferred SOL-exposure guards below (the second builds on the first).

---

## Deferred

### 1. SOL balance-floor guard — don't spend the wallet to zero

A pre-buy gate so a bad run (or an `ExitPending` pile-up tying up SOL) can't drain the wallet below the SOL it needs for fees/tips/rent — which would brick its own ability to *sell*.

- Add `get_sol_balance(wallet)` to `pump-trader/query.rs`, refreshed **off the hot path** (never inline before a buy) and cached.
- Keep a live **committed-SOL counter** (Σ `buy_amount` of open real positions) in **shared state** (`AppState`/`PumpFunTrader`), read by **both** clones — not per-strategy `runtime_cache`, or each would see only half the commitments and both could pass on the same SOL.
- Pre-buy: require `cached_balance − reserve_floor − committed_sol ≥ buy_amount`, reserve floor = **0.02 SOL**; reject the buy if it fails.
- No DB table for exposure (it drifts).

### 2. `trade.max_committed_sol` — explicit exposure ceiling

A hard cap on total SOL tied up in open positions, on top of the balance-floor guard. Builds on the committed-SOL counter, so it's a ~one-check + one-setting add-on once #1 lands.

1. Add the key to the settings registry (`settings_repo.rs`, same pattern as `slippage_bps`).
2. Extend the balance-floor pre-buy gate to also require `committed_sol + buy_amount ≤ max_committed_sol`.
3. Add the field to the Settings page.

---

## Out of scope

- **Multi-wallet support — removed.** One wallet only. The single `Arc<PumpFunTrader>` and the existing `WALLET_PRIVATE_KEY` / `NONCE_ACCOUNTS` `.env` config stay as-is. Revisit only if the single-wallet path is proven safe and the need actually arises.
