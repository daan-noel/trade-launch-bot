# Price Chart Logic (pump.fun token candlestick chart)

Reference for how the candlestick/price chart is drawn from trade data — focused on **prices**. Reuse as a prompt/spec.

---

## 1. Data flow (high level)
- Backend decodes each on-chain tx → stores a `Trade` row per swap leg.
- Frontend fetches trades (`GET /api/tokens/{mint}/trades`) → builds OHLC candles **client-side**.
- Chart lib: `lightweight-charts` (`CandlestickSeries` / `LineSeries`).
- All OHLC math: `frontend-react/src/components/token-price-chart/chartBars.ts`. Rendering: `TokenPriceChart.tsx`. Types: `types.ts`.

## 2. Units (critical — get these right or prices are off by orders of magnitude)
- `sol_amount`: SOL (lamports ÷ 1e9).
- `token_amount`: **raw base units, NOT decimal-scaled** (pump.fun tokens = 6 decimals → 1 token = 1e6 raw units).
- All reserves: virtual_sol/real_sol in **SOL**; virtual_token/real_token in **raw units**.
- Therefore price = **SOL per raw token unit** (typical magnitude ~1e-13). Display values look like `218e-15`.
- `TOKEN_TOTAL_SUPPLY = 1e15` raw (= 1e9 tokens × 1e6). Market cap (SOL) = `TOKEN_TOTAL_SUPPLY × spot`.
- First Entry Price (FEP) = `initial_buy_sol / initial_supply_token` (same raw units → consistent).

## 3. Per-trade price selection (`tradeSpotPriceSol`)
- Use **bonding-curve spot** = `virtual_sol_reserves / virtual_token_reserves` (GMGN-style marginal price). ← primary
- Else **pool spot** (post-migration AMM) = `real_sol_reserves / real_token_reserves`.
- Else fall back to **execution price** `price_per_token = sol_amount / token_amount`. ← last resort only
- **Do NOT chart execution price as the primary.** A big trade's avg fill (execution) lags the post-trade spot; charting it shifts the visible move to the next candle.
- Reserves in the `TradeEvent` are **post-trade**. The genesis bar's open is reconstructed from the *pre-trade* spot via constant-product `k = vsol·vtoken` (`preTradeSpotPriceSol`), so the first candle doesn't collapse.

## 4. Trade ordering (decides each candle's open/close — easy to get wrong)
- Base sort `compareTradesChronologically`: `slot` → `block_time` → `tx_signature` → `leg_index`.
- Problem: many trades can share **one slot** (separate txs). `block_time` is second-precision; `leg_index` only orders legs *within one tx*; `received_at` = ingest wall-clock (useless for history). So same-slot order falls back to **signature alphabetical = arbitrary**.
- Fix: within each same-slot group, reorder by the **bonding-curve reserve chain** (`orderSlotGroupByReserveChain`):
  - Each trade snapshots **post-trade** token reserves. Pre-trade reserve = `post + token_amount` (buy) or `post − token_amount` (sell).
  - Consecutive trades chain exactly: `post_tokens(prev) == pre_tokens(next)`.
  - Find the unique head (its pre-state is no other trade's post-state), then walk the chain → true execution order. Exact (raw units are integers, ±1 epsilon for safety).
  - Bail out unchanged if reserves/amounts missing or chain isn't a single clean line (never makes it worse).
- Why it matters: candle **close = last trade in the bucket (in execution order)**. Wrong order → close lands on an arbitrary trade → big move appears on the wrong candle.

## 5. Time bucketing
- Bucket key = `floor(block_time_seconds / intervalSec) * intervalSec`. Intervals: 1s/30s/1m/5m.
- Alt "slot mode": bucket by raw `slot` (step 1).
- Dust filter: skip trades with `sol_amount < 1e-5` SOL (`MIN_CHART_SOL`, mirrors backend `MIN_TRADE_SOL`).

## 6. OHLC construction (`buildContinuousBars` — GMGN-style "continuous")
- `open` = **previous bar's close** (NOT the bucket's first trade). First bar seeded from pre-trade spot.
- `close` = last trade's price in the bucket (execution order).
- `high` = `max(open, ...bucketPrices)`; `low` = `min(open, ...bucketPrices)`.
- Empty interval → flat bar (O=H=L=C = prev close). Optional trim of empty bars.

## 7. Volume / flow tooltip (Net / In / Out / Δ)
- Per bucket: `inflow` = Σ buy `sol_amount`, `outflow` = Σ sell `sol_amount`.
- `Net` = inflow − outflow; `In` = inflow; `Out` = outflow.
- `Δ%` = `(close − open) / open × 100` (bar price change).
- Computed in `BarFlowFields.tsx`.

## 8. ATH / current price markers
- ATH line: backend `tokens_info.ath_price` (display-converted only, not recomputed on frontend).
- Detail panel "Current Price"/"ATH" + ATH/FEP, Current/FEP: from backend `current_price`/`ath_price` vs FEP.
- Chart's own last-price line = last bar's close (lightweight-charts `lastValueVisible`).
- Display: SOL→USD via rate; unit prefix ◎ (SOL) or $ (USD).

## 9. Backend price fields (source of the above)
- `models/trade.rs`: `price_per_token` (execution), `curve_spot_price()` (vsol/vtok), `pool_spot_price()`, `chart_spot_price()`.
- `state/token_cache.rs` `add_trade`: `current_price` + `ath_price` (running max). ATH persisted authoritatively (heals on re-sync), not `GREATEST`.
- Authoritative trade data comes from the pump `TradeEvent` (amounts + reserves).

## 10. Known pitfalls / correctness rules (bugs already fixed — keep these invariants)
- **Truncated logs**: large/bundled txs get their `Program data:` log cut by Solana → no event → lossy balance-delta fallback (wrong SOL, no reserves). **Decode the `TradeEvent` from the `emit_cpi!` inner instruction instead** (Anchor tag `e445a52e51cb9a1d` + `TRADE_EVENT_DISCRIMINATOR`); inner-ix data is never truncated. (`ingest_laserstream/decoder/` —
`decode_trade_events_from_inner_ixs`)
- **Token units in fallback**: must read `uiTokenAmount.amount` (raw) not `uiAmount` (decimal) — else price inflated by 1e6.
- **ATH stickiness**: store ATH authoritatively from the recompute so a re-sync can lower a previously over-stated value.
- **Same-slot ordering**: never trust signature order for OHLC; use the reserve chain (§4).
- **Spot vs execution**: chart from reserve spot, not `price_per_token` (§3).

## 11. Key files
- `frontend-react/src/components/token-price-chart/chartBars.ts` — price selection, ordering, reserve chain, bucketing, OHLC.
- `frontend-react/src/components/token-price-chart/TokenPriceChart.tsx` — rendering / lightweight-charts wiring.
- `frontend-react/src/components/token-price-chart/{types.ts, BarFlowFields.tsx, constants.ts}`.
- `backend/src/models/trade.rs`, `backend/src/ingest_laserstream/decoder/` (decoder split into `parse.rs`/`trade.rs`/`create.rs`/`instructions.rs`; WS `ingest/` removed), `backend/src/state/token_cache.rs`, `backend/src/config/constants.rs`.
