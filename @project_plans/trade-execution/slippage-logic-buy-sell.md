# Slippage Logic in the Buy/Sell Flow

## The two code paths

The trader has two venues, each with its own slippage handling:

| | **Bonding curve** (pre-migration) | **PumpSwap AMM** (post-migration) |
| --- | --- | --- |
| Buy | `buy.rs` (~159-174) | `amm.rs` (~203-207) |
| Sell | `sell.rs` (~211-224) | `amm.rs` (~276-278) |

> Line numbers below are approximate (the files drift); the named symbols are the
> source of truth. All paths are in the `pump-trader/src/trader/` crate.

Both take `slippage_bps: Option<u64>`, and in every case slippage is enforced
**on-chain** by encoding a `min_out` floor into the instruction data — the
pump / pump_amm program reverts the tx if the actual fill falls below it. The
Rust code only *computes* that floor; the chain enforces it.

## Bonding curve

The key design choice is that **`None` = no slippage protection**, and it's
deliberate — it's the latency-critical snipe path.

**Buy** (`buy.rs:166`) sets `min_tokens_out` in the
`buy_exact_sol_in(spendable, min_tokens_out)` instruction:

- `None` → `min_out = 1` (effectively "fill at any price"), and **skips the
  reserve read** entirely. This is what `buy_token_snipe` uses to avoid an RPC
  round-trip on a fresh-token snipe.
- `Some(slip)` → reads the curve's virtual reserves via `curve_reserves`,
  computes expected tokens with the constant-product formula, then haircuts by
  slippage:

  ```
  net      = buy_lamports * (10000 - CURVE_FEE_BUFFER_BPS) / 10000
  expected = vt * net / (vq + net)               // tokens out at current reserves
  min_out  = expected * (10000 - slip) / 10000   (floored at 1)
  ```

**Sell** (`sell.rs:190`) is the mirror image, setting `min_sol_output` in
`sell(amount, min_sol_output)`:

```
gross   = vq * token_amount / (vt + token_amount)   // SOL out
net     = gross * (10000 - CURVE_FEE_BUFFER_BPS) / 10000
min_out = net * (10000 - slip) / 10000   (floored at 1)
```

Two safety behaviors worth noting:

- **`CURVE_FEE_BUFFER_BPS = 200`** (`constants.rs`, ~L180) — a conservative 2% fee
  allowance subtracted before the slippage haircut. It deliberately
  *over*-estimates the fee so a fee misestimate only loosens protection, never
  causes a false revert (the real curve fee is ~1%).
- **Fail-open on read error**: if `curve_reserves` fails, both paths log a
  warning and fall back to `min_out = 1`, so a flaky RPC read never blocks a
  trade (`buy.rs:176`, `sell.rs:199`).

## PumpSwap AMM

Different default semantics: `None` here means **use the default 5%**, not "no
protection" — `AMM_DEFAULT_SLIPPAGE_BPS = 500` (`constants.rs`, ~L172). The AMM
path is never the snipe path, so it always has reserves cached
(`amm_reserves_cached`) and always applies a floor.

The AMM also accounts for the full fee stack —
`lp_fee_bps + protocol_fee_bps + coin_creator_fee_bps` — read from on-chain
config, rather than the curve's fixed buffer.

**Buy** (`amm.rs:201-206`) is **exact-base-out**: it computes `base_amount_out`
(min tokens) and passes `spendable` as `max_quote_amount_in` (the spend cap).
The slippage haircut makes it *request fewer tokens* so the actual SOL cost
stays under the cap:

```
quote_net       = spendable * (10000 - fee_bps) / 10000
base_out        = cp_amount_out(quote_net, quote_res, base_res)
base_amount_out = base_out * (10000 - slip) / 10000
```

**Sell** (`amm.rs:275-277`) computes `min_quote_out`:

```
gross         = cp_amount_out(token_amount, base_res, quote_res)
net           = gross * (10000 - fee_bps) / 10000
min_quote_out = net * (10000 - slip) / 10000
```

where `cp_amount_out` (`amm.rs:761`) is the standard constant-product output:
`reserve_out * amount_in / (reserve_in + amount_in)`.

## Summary of the semantic gotcha

The one thing to keep straight: **`slippage_bps = None` means opposite things
on the two venues.** On the curve it disables protection (min_out = 1) for
snipe latency; on the AMM it falls back to a 5% default. So a caller that wants
"no slippage limit" on the curve gets a hard 5% floor if the same token has
migrated to the AMM.

Note the **API layer floors the value before it reaches the trader**: the
manual buy/sell endpoints (`resolve_slippage` in `api/handlers/trading/solana.rs`)
and the settings write both `clamp(SLIPPAGE_MIN_BPS, SLIPPAGE_MAX_BPS)` (10 bps
.. 5000 bps), so an explicit `Some(0)` — which would compute a `min_out` ≈
`expected` and revert on any movement at all — can't be supplied through the
HTTP API. `None` (no protection) is still distinct from `Some(0)` and unaffected.
