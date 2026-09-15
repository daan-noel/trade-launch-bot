# Stale-creator 2006 reverts - plan

**Status: code on `strategy-redesign`; deploy and the 24 h gates open.** Real curve buys
revert with Anchor `ConstraintSeeds` (2006) on mints whose curve creator changed after
launch, and every retry reverts again and pays its fee. Real sells carry the same
defect. Found during the [io-storm](io-storm-latency-plan.md) acceptance check. How it
works now: [execution-workflow](../plans/trade-execution/execution-workflow.md).

## Evidence (EC2, Sep 15, current runs of the two `+ Door` rules)

- 11 of 37 real entry decisions are `EntryFailed` on 2006: 2 mints, 5-6 attempts each.
  Real 2006 rows per day: 0 through Sep 13, 4 on Sep 14 (2 mints), 28 on Sep 15 (9 mints).
- Each retry's refresh logs the SAME `new_creator_vault` (`Aab9p...` x5 for `BmNNm8...`,
  `E9TL7...` x6 for `APkFgt...`), and reports it as a change every time.
- The vault derived offline from `tokens.creator_wallet` (the create-time creator)
  differs from the refreshed vault on both mints; the same PDA code reproduces the stored
  `bonding_curve_address`.
- Pump's `TradeEvent` carries the curve's current creator at byte 177 (8-byte
  discriminator included: after `real_token_reserves`, `fee_recipient`,
  `fee_basis_points`, `fee`). On 60 of 60 relay events, `PDA("creator-vault", that
  field)` is the `creator_vault` the transaction passed. The decoder reads nothing past
  `real_token_reserves` today.

## Root cause

1. The loop takes the creator for a buy (`decision_loop.rs`, `BuyOrder.creator`) and for a
   position's sells (`sinks.rs`, the position meta) from `token_cache` `creator_wallet`: the
   creator in the create event.
2. `buy_token_inner` derives the PDAs from the caller's creator and overwrites the trader's
   `token_pdas` on every buy (`shared/executor/pumpfun/src/trader/buy.rs`).
   `sell_token_once` re-applies the caller's creator on every non-healed call
   (`sell.rs`, `creator_override`).
3. After a 2006, `refresh_curve_creator_vault` writes the right vault. The next attempt
   passes the create-time creator again and puts the stale vault back:
   - buy: the refresh reads as a change every time, so `Retry` loops until the rule stops
     re-deciding;
   - sell: the second attempt reverts, the refresh reads as unchanged, and the exit goes
     `Fatal`. A held position whose creator changes cannot be sold by the bot. No real
     position has hit it (0 stuck real exits in 14 days).
4. `read_curve_routing` states that the creator is fixed at creation. It is not: pump
   `set_creator` rewrites `bonding_curve.creator`.

## Fix

One fact, one owner: the token cache keeps the curve's **current** creator per mint,
separate from `creator_wallet`, which stays the launch creator that rule metrics,
`prior_launches` and the copycat key are defined on.

| Step | Change | Crate |
| --- | --- | --- |
| 1 | Decode `creator` from `TradeEvent` (byte 177) into the neutral trade | `shared/ingest/pumpfun` (+ core type) |
| 2 | Every curve print writes it to the mint's cache entry, `curve_creator` | `hunter/live` token cache |
| 3 | `BuyOrder` and `SellOrder` take `curve_creator`, falling back to `creator_wallet` | `hunter/live` engine |
| 4 | After a 2006, `recheck_curve_creator` reads the chain creator (`get_creator_from_mint_pda`) and writes a changed one to `curve_creator` | `exec_real.rs` |
| 5 | Retry only when the chain creator differs from the one the reverted tx used; equal is `Fatal` | `exec_real.rs` |
| 6 | Correct the `read_curve_routing` comment | executor |

Cost on the hot path: one 32-byte copy per curve print and one field read per order. No
RPC and no Helius. The trigger print is itself a landed trade on the mint, so the buy uses
the creator that print was validated against. A 2006 then needs a `set_creator` between
the trigger print and our transaction.

Tests: the decoder reads the creator from a captured mainnet event and its PDA is the
vault that transaction passed (log and inner paths); a short event decodes without it;
`trade_creator` follows the newest print and never rewinds; a 2006 retries only on a
different creator. Forge compiles against the new `Trade` field.

## Acceptance (24 h after deploy)

| Gate | Target |
| --- | --- |
| real buy `EntryFailed` on 2006 | 0, or only a `set_creator` landing between trigger and send |
| real sell 2006 | 0 |
| `snipe_latency.decide_to_ack_ms` p90 | <= 50 ms (unchanged) |
| `trig_dec` (`created_at - target_time`) | 0-1 ms (unchanged) |

## Other open items from the same check

- **Open arms:** 25 % of arms older than 10 min read open (1 133 of 4 521), against the
  io-storm gate's 0-8 %. Ends are writing without errors, so this is not yet a leak.
  Next step is read-only: split open arms by whether their token is still tracked.
- **Slot delta:** 46 % same-slot entries against 67 % when healthy, on 26 fills (~15
  independent triggers). Decision to fill is healthy (74 ms p50). Measure only: the
  io-storm Phase 3 window with `LATENCY_TRACE=1`.

## Deploy

The server build follows the io-storm Phase 2 rule: only with every real rule inactive
and zero real positions open.
