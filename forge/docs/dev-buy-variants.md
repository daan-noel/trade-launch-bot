# Dev-buy ix variants

The launch dev-buy now selects among **all four** pump.fun curve-buy encodings
(`buy`, `buy_exact_sol_in`, `buy_v2`, `buy_exact_quote_in_v2`) — the same catalog the
bundler co-buys already draw from. Previously the dev buy was hard-pinned to
`buy_exact_sol_in`. The dev-buy encoding is chosen **independently** of the bundler
`buy_variant`, so the dev buy can diverge from the co-buys (on-chain fingerprint
diversity).

## Design (SSOT)

The per-variant buy instruction (account list + arg encoding + the v2 WSOL quote ATA)
lives in ONE place: `PumpFunTrader::build_curve_buy_core` in
`shared/executor/pumpfun/src/trader/bundle_buy.rs`, returning a `BuyCore { buy_ix,
extra_atas }`.

- **Bundler co-buy** (`build_bundle_leg_tx`): draws a `BuyCore`, then `assemble`s the
  leg's CU/tip/ATA wrappers around it. Base token ATA first, then `extra_atas` (the
  v2 WSOL ATA), then the opaque `Core = [buy]`.
- **Dev buy** (`create.rs::dev_buy_core_ixs`): draws the SAME `BuyCore` and fuses
  `[base_ata, ..extra_atas, buy_ix]` straight into the create tx's `Core` block. So a
  dev buy of variant *X* is byte-for-byte identical to a co-buy of variant *X* (buyer
  = the dev/creator wallet). The hot-path standalone buy keeps its own
  `curve_buy_ix` — untouched.

The dev buy is carried as `pump_trader::DevBuy { sol, lamports, slippage_bps,
variant }`.

## Tokens-out slippage rule

The two **tokens-out** encodings (`buy`, `buy_v2`, `Denom::ExactBaseOut`) buy an exact
token amount capped by `max_sol_cost`. Their token amount is the reserve-derived
`min_out` floor, so they REQUIRE a `slippage_bps` — without one `min_out` is `1` and
the dev buy would buy ~1 token instead of spending its budget. The **SOL-in**
encodings (`buy_exact_sol_in`, `buy_exact_quote_in_v2`) spend the budget directly, so
`None` slippage stays the historical unprotected launch behaviour.

Enforced in `service.rs::PumpfunTemplateParams::validate_dev_buy` (rejects a
tokens-out dev variant with no slippage, and any non-buy / unknown variant) and
mirrored client-side in `LaunchTemplatesPage.tsx` (disables Save + shows a warning).

## Wiring

- Template param: `launch_templates.params.dev_buy_variant` (optional; `None` ⇒
  `buy_exact_sol_in`). Resolved via `PumpfunTemplateParams::dev_buy_variant_name`.
- Plan: `orchestrator::BundleLaunch.dev_buy_variant` — the dev-buy op is authored
  (`lock_variant`) so the disguise keeps the operator's choice (CU/tip still jitter).
- Persisted create leg: `CreateLegArgs.dev_buy_variant` (serde default
  `buy_exact_sol_in`, so bundles persisted before this change still deserialize). Read
  on every submit + re-bid so the create leg stays byte-identical.
- Bundler-less (legacy) launch: `create_token{,_v2}_and_dev_buy` take a
  `BundleBuyVariant`.

## v2 dev buy — tx size caveat

A v2 dev-buy variant (`buy_v2`, or `buy_exact_quote_in_v2` **with cashback**) fuses a
second ATA (the WSOL quote ATA) plus the ~27-account v2 buy list into the create tx,
which is already ALT-dependent (`PUMP_LAUNCH_ALT`). The launch ALT must carry the v2
buy accounts for it to fit the 1232 B limit. **Not yet exercised with real SOL** — do
a mainnet size check before relying on a v2 dev buy (the v1 encodings — `buy`,
`buy_exact_sol_in` — carry no size risk).
