# Slippage Logic in the Buy/Sell Flow

## The settings contract (what the operator types)

Slippage is a **blank-or-a-number** knob, one persisted key per side
(`trade.buy_slippage_bps` / `trade.sell_slippage_bps`), and **a typed number is
honored literally** — nothing sits between the percent typed in Settings and the
`min_out` the trader encodes. Blank is what carries the per-side policy:

| Field | Blank | A typed number |
| --- | --- | --- |
| Buy slippage % | `DEFAULT_SLIPPAGE_BPS` (2500 = 25%) | used exactly as typed |
| Sell slippage % | **no floor** — `min_out = 1`, sell all | used exactly as typed |

The asymmetry is deliberate and matches the operating intent: *a buy must land
even at some loss; a sell must clear the whole bag with no specific slippage.* A
buy with no opinion still gets protection; a sell with no opinion dumps.

`0` is **not** a spelling of "no limit" — it is rejected with a **400** at every
write door (`config::constants::validate_slippage_bps`, the ONE validator, called
by the settings write and by `POST /api/solana/wallet/sell`). Under literal
handling `0` would mean "revert on any movement at all", which is never intended;
blank is how you say "no floor". A no-floor *buy* is not a distinct spelling
either — type a large percent, bounded by `SLIPPAGE_MAX_BPS`.

**Only `SLIPPAGE_MAX_BPS` (5000 = 50%) still applies to a typed value.** A ceiling
only ever *loosens* a floor, so it can never turn a fill into a revert. The old
`SLIPPAGE_MIN_BPS = 10` floor is **deleted**: it clamped `0` — then the documented
"no floor" sentinel — up to `10`, the tightest possible non-zero floor, silently
inverting the value's meaning on the bot's own buy AND sell paths (an exit meant
to clear at any price during a dump instead reverted on 0.1% movement). A stored
`10` could be a genuine 0.1% or a clamped `0` and the two are not distinguishable,
so the slippage-reset migration deleted both keys from `app_settings` rather than
attempting a repair (a one-off data fix; it is a no-op on a fresh DB, so the squash
into `0001_init.sql` records it only as a note).

The legacy combined `trade.slippage_bps` key is **retired** by the same migration:
the buy chain is now one key, so a blank buy field falls to the default instead of
a stale legacy number.

Wire note: the settings PATCH treats these two fields as **three-state** (absent =
untouched, `null` = clear back to blank, number = set). A plain `Option<u64>`
collapses absent and `null`, which would make blank — a real state here —
unreachable once a number had been saved.

This is a rule for **every nullable setting**, not a slippage quirk: declare it
`Option<Option<T>>` behind `patch_field`. `max_committed_sol` shipped as a plain
`Option<f64>` and could therefore never be cleared — the UI sent `null`, the
handler read it as "absent", the write was skipped, and the 200 response
re-rendered the number the operator had just deleted. Silent in both directions
(no error, no log), which is why it is locked by
`a_nullable_setting_distinguishes_cleared_from_absent` in `system.rs`. A set
ceiling must also be `> 0`: `0` would persist as "a limit is configured" while
blocking every real buy, so it is a 400 and clearing the field is the off switch.

Frontend: `TradingSection.tsx` uses `step`/`min` of `0.01`, because the percent →
bps conversion is `Math.round(pct * 100)` and anything under 0.005% would round to
a `0` the API now rejects.

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
  reserve read** entirely.
- `Some(slip)` → computes expected tokens with the constant-product formula, then
  haircuts by slippage:

  ```
  net      = buy_lamports * (10000 - CURVE_FEE_BUFFER_BPS) / 10000
  expected = vt * net / (vq + net)               // tokens out at current reserves
  min_out  = expected * (10000 - slip) / 10000   (floored at 1)
  ```

  The reserve source for `(vt, vq)` depends on the caller (`buy_token_inner`'s
  `snipe_reserves: Option<(u128,u128)>` arg):
  - **Snipe path** (`buy_token_snipe`, 1B): the strategy passes the triggering
    event's virtual `(token, quote=lamports)` reserves — read from the in-memory
    `token_cache`, **not** an inline RPC. A snipe with no snapshot in hand falls
    back to `min_out = 1` (still no inline read) rather than blocking the buy.
  - **Manual path** (`buy_token`): no event reserves in hand, so it reads the
    curve on-chain via `curve_reserves` (the WS-cache-then-RPC fast path).

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

Same semantics as the curve: **`None` → `min_out = 1`** (no floor), on **both**
sides — verified 2026-07-27 at `amm.rs:71-72` (buy) and `:442-445` (sell). There is
no AMM-specific default slippage and no `AMM_DEFAULT_SLIPPAGE_BPS` constant. Do not
read that as "callers must pass `Some(bps)` explicitly" — `None` is accepted on both
AMM sides and means exactly what it means on the curve.

Buys always arrive as `Some(bps)` because `resolve_buy_slippage_bps` never returns
`None`; bot/manual sells pass `None` whenever the sell field is blank.

The AMM accounts for the full fee stack —
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

## Summary

**`slippage_bps = None` means the same thing on all four paths (curve buy, curve
sell, AMM buy, AMM sell): `min_out = 1`, no floor.** Callers that want a floor
pass `Some(bps)`.

The **API layer does not transform the value** on its way to the trader. It only
*rejects* `Some(0)` with a 400 (see "The settings contract" above) and caps a
typed value at `SLIPPAGE_MAX_BPS`. `Option<u64>` remains the executor's contract,
unchanged — forge's `MinOut::Unprotected` (`forge/orchestrator/src/provider.rs`) is
exactly "pass `None`", so there is no cross-product blast radius from this work.

### Accepted gap — the unprotected snipe fill

A curve buy carrying a real floor still fills at `min_out = 1` when there is **no
reserve snapshot in hand**: `buy.rs` logs `curve buy slippage: reserve read failed
(…); using min_out=1` and proceeds. A snipe that arrives before a snapshot is
therefore unprotected no matter what the settings layer says.

This is **wanted**, not a hole — under "buys must land", blocking the entry to
protect it defeats the point, and the alternative (an inline reserve read on the
latency-critical snipe path) is a hot-path RPC. Documented here so it is not
"fixed" later by accident.
