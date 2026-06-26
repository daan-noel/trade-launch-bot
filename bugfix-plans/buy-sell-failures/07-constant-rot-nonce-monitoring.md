# B7 — Program-constant rot + nonce monitoring (Fix 7) — P2 (operational)

> Workstream B (buy-sell-failures). Observability, not a code-path fix. No new infra spend — respect
> the EC2 connection-count guardrail.
> Paths are pre-crate-split — see [../README.md](../README.md#-path-caveat--line-refs-predate-the-crate-split).

## Constant rot (A2 / A6)

The curve fee recipient and AMM layout constants are hardcoded and reverse-engineered. A pump.fun
rotation or AMM layout upgrade reverts **every** trade of that kind until the constant is updated
(memory `fee-recipient-rotation-bug`).

- **Add a metric/alert** that fires when the **rate** of `6000` (curve) or AMM structural / `Overflow`
  reverts spikes **across many distinct mints** — the signature of a rotation/layout change (vs. a
  single bad token).
- **Document the update runbook:** verify the new account against a live swap + a zero-SOL
  `simulate-*` probe before shipping the constant. No automatic on-chain discovery — keep it a guarded
  manual update.
- Constants at risk: `PUMP_CURVE_FEE_RECIPIENT` ([constants.rs:56](../../pump-trader/src/constants.rs#L56)),
  `PUMP_AMM_BUYBACK_FEE_RECIPIENT`, `PUMP_AMM_CASHBACK_GLOBAL`, the AMM account-list blocks
  ([amm.rs:468-512](../../pump-trader/src/trader/amm.rs#L468-L512)).

## Nonce (C1–C3)

- Surface `nonce_wait_events` / the "All nonce slots busy" bail
  ([nonce.rs:102](../../pump-trader/src/trader/nonce.rs#L102)) and the `check_nonce_authorities` result
  as metrics / log alerts.
- A frequent busy-bail means the pool needs resizing — but **respect the EC2 connection-count
  guardrail** (new pools require shrinking something else).

## Verification

- Confirm the metrics emit and an alert fires on a synthetic spike.
- No new pools / no raised limits without offsetting another consumer.
