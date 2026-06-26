# B5 — Manual-buy: slippage-revert retry + confirm-timeout classification (Fix 5 + 5b) — P1

> Workstream B (buy-sell-failures). Two fixes that share signature-plumbing — do them together.
> **5b plumbs the signature that 5 needs**, so implement 5b's return-type change first.
> Paths are pre-crate-split — see [../README.md](../README.md#-path-caveat--line-refs-predate-the-crate-split).

## Fix 5b (do first) — manual-buy confirm-timeout classification

- **Files:** `manual_buy`; `buy_token` / `amm_buy` return types; the `confirm_transaction` caller.
- **Problem (C7/E5):** the manual buy's `confirm_transaction` has a ~2.35 s window
  ([constants.rs:193-203](../../pump-trader/src/constants.rs#L193-L203)). A timeout returns 500, but
  the durable-nonce buy may land later → tokens into an **untracked** balance (manual buy opens no
  position; only the read-only boot `wallet_reconcile` flags it).
- **Change:** have the curve/AMM buy return the **signature** (like `sell_token_once → Option<String>`,
  today it returns `bool`). On a `confirm_transaction` timeout, call `signature_state(&sig)`:
  - landed-success → `200 {"success": true, "pending": false}`;
  - pending → `200/202 {"pending": true, "signature": sig}` so the UI shows "submitted, confirming"
    instead of a hard failure, and (optionally) kick a wallet reconcile so a late-landing manual buy
    gets tracked;
  - reverted → 500 (or feed into Fix 5's retry).
- **Note:** plumbing the signature through `buy_token` / `amm_buy` is the bulk of the work and also
  unlocks Fix 5. Keep `amm_buy`'s existing `confirm` flag semantics.

## Fix 5 — manual-buy slippage-revert retry

- **File:** `manual_buy` [solana.rs:115-141](../../backend/src/api/handlers/trading/solana.rs#L115-L141).
- **Problem (A5):** single-shot; a transient `6003` (curve) / `6004` (AMM) → 500, user must re-click.
- **Change:** wrap the buy in a bounded retry (2–3 attempts). **Only retry on a proven on-chain revert**
  (no tokens bought) — mirror `classify_silent_send`
  ([real.rs:42-48](../../backend/src/strategies/tpsl_sniper_1/execution/real.rs#L42-L48)):
  `Some(false)` → resend; `Some(true)` / `None` / `Err` → stop (durable-nonce tx may land, re-sending
  risks a double-buy). Requires the buy to return its **signature** (from Fix 5b). `buy_token_inner`
  already re-reads curve reserves each call on the manual path, so a retry re-quotes automatically.

## Verification

- Simulate a slippage revert (tight slippage on a moving token) → bounded retry, no double-buy on a
  landed-but-unconfirmed tx.
- Simulate a confirm timeout → `200 {pending:true, signature}` not a 500.
- **Frontend:** if the buy/sell response shape changes, `npm run build` clean and update the
  manual-trade UI to handle the `pending` status.
- `cargo check -p backend-deploy` + `cargo check -p pump-trader` clean; clippy on touched files.
