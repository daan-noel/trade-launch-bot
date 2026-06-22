# Real-Trading Hardening Plan (single wallet)

> Goal: make the bot **safe to run with real SOL** on a single wallet.
> Built from an audit of the real (on-chain) execution path vs. the paper path.
> **Scope:** one wallet only — multi-wallet support was removed (see "Out of scope").

---

## Decisions already made (with you)

| # | Topic | Decision |
|---|-------|----------|
| 1 | Where the private key lives | In `.env` only — **never** in code, the database, or the UI. |
| 2 | Two positions on the same token | **Allowed** (e.g. `tpsl_sniper_1` and `tpsl_sniper_2` both fire on it). This forces *per-signature attribution* — each position tracks its own buy/sell transaction — not a `(wallet, mint)` shortcut. |
| 3 | Slippage config | Wire the existing global `trade.slippage_bps` setting into strategy buy+sell **now**; per-rule override later. |

## Ground rules (from CLAUDE.md)

- **Every change lands in BOTH `tpsl_sniper_1` and `tpsl_sniper_2`** — they are intentional clones.
- No new RPC call in the sell-confirm loop; read state from `runtime_cache`, not the DB per event.
- Update the matching `@docs/*.md` map after each logic change.
- Done = `cargo check --bin backend` + `cargo test --bin backend` + `cargo test -p pump-trader` clean, `cargo clippy` on touched code, `npm run build` clean.

---

# Hardening work

*(ship all of this before putting real SOL in)*

*All hardening steps shipped. 1C (per-signature attribution) landed in migration `0009` + `models/position.rs` (`entry_tx_signatures`/`exit_tx_signatures` arrays) + `trade_repo` (`find_fill_by_signature` / `sum_legs_by_signatures`) + both `execution/real.rs` clones. The API exposes the arrays and keeps `entry_tx`/`exit_tx` as the first/last leg for the existing positions-table display (no frontend change needed).*

---

## How we'll verify

**Gate before real SOL:**

- Builds + tests + clippy clean.
- Unit tests: RAII guard frees the `exiting` slot on panic; in-process reaper (1A-4) re-arms an `ExitPending` position whose guard isn't held; ExitPending re-arms on boot; `find_fill_by_signature` sums multi-leg; per-signature sell-confirm; buy `min_out` derived from the triggering event's reserves, `min_out=1` on missing reserves; `classify_sell_revert` maps slippage error codes → Retry and structural codes → StopFeeBurn.
- `cargo run -p backend -- probe simulate-sell` with a real slippage value (confirm slippage-vs-structural revert classification fires correctly).
- Live test on a funded throwaway wallet at **0.01 SOL**: single buy→exit, then **two concurrent rules on the same token** — confirm both attribute and exit independently (the heart of decision #2).

---

## Resolved decisions

1. **Big buys + slippage:** **fail-safe** — when a buy can't fill within slippage, **reject** it (don't fill at a bad price).

## Deferred (do later, no rework needed)

- **SOL balance-floor guard — don't spend the wallet to zero.** A pre-buy gate so a bad run (or an `ExitPending` pile-up tying up SOL) can't drain the wallet below the SOL it needs for fees/tips/rent — which would brick its own ability to *sell*. Add `get_sol_balance(wallet)` to `pump-trader/query.rs`, refreshed **off the hot path** (never inline before a buy) and cached; keep a live **committed-SOL counter** (Σ `buy_amount` of open real positions) in **shared state** (`AppState`/`PumpFunTrader`), read by **both** clones — not per-strategy `runtime_cache`, or each would see only half the commitments and both could pass on the same SOL. Pre-buy: require `cached_balance − reserve_floor − committed_sol ≥ buy_amount`, reserve floor = **0.02 SOL**; reject the buy if it fails. No DB table for exposure (it drifts).
- **`trade.max_committed_sol` — explicit exposure ceiling.** A hard cap on total SOL tied up in open positions, on top of the balance-floor guard above. To add: (1) add the key to the settings registry (`settings_repo.rs`, same pattern as `slippage_bps`); (2) extend the balance-floor pre-buy gate to also require `committed_sol + buy_amount ≤ max_committed_sol`; (3) add the field to the Settings page. Builds on the committed-SOL counter from the balance-floor guard, so it's a ~one-check + one-setting add-on once that lands.

## Out of scope

- **Multi-wallet support — removed.** One wallet only. The single `Arc<PumpFunTrader>` and the existing `WALLET_PRIVATE_KEY` / `NONCE_ACCOUNTS` `.env` config stay as-is; no trader registry, no `trading_wallets` table, no per-rule `wallet_id`, no Wallets page. Revisit only if the single-wallet path is proven safe and the need actually arises.
