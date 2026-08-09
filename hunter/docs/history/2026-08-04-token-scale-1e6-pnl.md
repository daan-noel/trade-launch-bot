# Paper PnL% pinned at −100% — a `TOKEN_SCALE = 1e6` that only cancelled halfway (2026-08-04)

**Symptom.** Every **closed paper** position rendered its PnL% cell at −100%, while the
SOL PnL figure beside it was correct.

**Cause.** `exec_paper.rs` and `lab/strategies/replay.rs` both applied a
`TOKEN_SCALE = 1e6` factor when synthesizing a fill. That factor **cancels out of SOL
PnL** — `sol = token_amount × price` and `token_amount = sol / price` scale inversely —
so PnL and every ratio-based exit condition stayed correct, and nothing looked wrong on
the money path. What it did not cancel out of was the **stored** token count, which went
in 1e6× too high. `record_sell_fill` then computed
`exit_price = exit_sol / sold_token_amount`, 1e6× too small, and the percentage derived
from it floored at −100%.

This is why it survived: a scaling error that is self-cancelling in the aggregate is
invisible until some consumer reads one of the two factors on its own.

**Fix.** A `Fill::price` is the feed's `price_per_token` = **SOL per RAW token unit**
(`Trade::new`: `amount_sol / token_amount`, count in raw units) — the same convention
`entry_price`/`exit_price` and the real executor already used. A synthesized paper/sim
fill sizes `token_amount = sol / price` and prices a leg `sol = token_amount × price`,
**never** through a `10^decimals` factor.

**Consequences that outlive the fix.**

- Rows written before it carry `entry_token_amount` 1e6× high. Any close that sizes its
  leg as `price × tokens` books a 1e6× fantasy PnL and can overflow `bigint`. **Size from
  cost basis × price ratio instead** — `entry_lamports` was always right and a ratio is
  scale-free, so it is correct on both sides of the fix and identical on a consistent row.
  ([`@arch/position-lifecycle.md`](../arch/position-lifecycle.md) §2.2.)
- **Corollary for tests:** a corpus priced at `1.0` buys a *one-unit* bag, so any
  `sell_bps` ladder quantizes to 0/1 units. The sweep parity guard prices its scale-out
  corpora at `RAW_PX = 1e-6` for exactly this reason.

**The rule this produced.** A unit factor applied on both sides of a product is not
"harmless because it cancels" — it is a latent wrong value in whichever factor gets stored
and read alone later.
