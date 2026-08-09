# Flow-scalper — what the 2026-07-28 investigation settled

Durable conclusions from the effort that tried to turn the omego dip-reversion
pattern into a rule. The blow-by-blow (ladder tables, phase plan) is gone; the raw
rows survive in [`../../roadmap/data/flow-scalper-ladder.csv`](../../roadmap/data/flow-scalper-ladder.csv).

Companions: [execution-costs.md](execution-costs.md) (the cost model this produced),
[armed-trailing-stop.md](armed-trailing-stop.md) (the `arm_above_pct` feature),
[wallet-analysis.md](wallet-analysis.md) (the wallet analyses themselves).

## The headline: the pattern is real, and it does not clear the fee

Measured from **cash flow**, not price paths — 6,273 legs / 451 mints over the local
window:

| | SOL |
| --- | --- |
| spent on buys | 2,661.17 |
| received on sells | 2,709.28 |
| **gross** | **+48.11** (= **+1.81% of turnover**) |
| protocol fee @ 125 bps/leg | −67.13 |
| tip + priority | −6.43 |
| **net on completed round trips** | **−25.45** |

**His gross edge is +1.81% against a 2.53% round-trip fee.** That one line explains
every negative result the investigation produced: the tuned rule stalling at
break-even, copy-trading being negative at *zero* latency, and four separate entry
gates adding nothing. There was never 2 pp of edge to find.

He is net positive only via **positions he never closes** — 72 of 451 mints, marked
at 83.00 SOL, and those marks survive scrutiny (top-3 concentration 13.2%, median bag
1.39 SOL, and **0 of 72** had zero trades after his last leg). He sells ~81% into the
scalp and lets ~19% ride; on winners that residual marks ~7× its cost basis.

**So his edge is a runner tranche, and the scalp round trip is a fee-paying wash that
funds it.** At the time of this measurement `hunter-engine` had **no partial-exit
concept** — no tranche, scale-out, or sell-percentage anywhere in `arm.rs`/`reduce.rs`;
every exit closed 100%. The strategy shape we were tuning was structurally incapable of
the thing that makes him money. That is not a tuning gap — and it is what motivated the
scale-out ladder shipped 2026-07-29 ([partial-exits.md](partial-exits.md)), which closed
the capability but has not yet been re-measured against this pattern.

> `64hP97Bwr5...` is the same family with better economics (+2.54%/SOL cycled net of
> fees) and **is** the right template — see the `64hP` section of
> [wallet-analysis.md](wallet-analysis.md), including the `fs2-*` rule ladder built
> from it.

## Three mechanical defects, all real, all fixed

Each is backed by *mechanism* evidence, not just PnL — which matters, because the PnL
deltas mostly sat inside the noise floor (below).

1. **An unarmed `retrace` is a hard stop from entry.** `PositionCtx::at_fill` seeds
   `peak = entry_price`, and exit metrics only OR, so `retrace >= 3 AND pnl >= 2` was
   unauthorable. 65.4% of the reference wallet's winners dip >3% off their running
   peak *before* winning. Fixed by `m_position.arm_above_pct` — see
   [armed-trailing-stop.md](armed-trailing-stop.md).

2. **`m_price_lifetime.stall` silently caps every hold at ~15 s.** This is the big
   one and it is not obvious from the rule JSON. `stall` is *seconds since the last
   all-time high*, and it resets only on a **new** high. A dip-entry rule is below its
   high by construction, so the clock never resets; `can_enter` also refuses while an
   exit metric holds, so `stall < 15` doubles as a hidden **entry** filter (only
   tokens that peaked in the last 15 s). Median entry is already 8.3 s past the ATH,
   leaving ~7 s of headroom. **No position in 445 episodes ever held longer than
   16 s**, against a reference trader whose median hold is 22.5 s.

   > Use **`m_position.held >= N`** for a time stop. Same per-trade result with 32%
   > more throughput, because `held` bounds the hold *without* also filtering entries.
   > `price_lifetime.rs` is authoritative on `stall`'s definition; a one-line summary
   > in `metrics/mod.rs` reading "since the price last moved" is what makes this
   > invisible.

3. **Re-entry amplifies selection quality, in whichever direction it points.** He
   re-enters good picks and improves with episode index; we re-entered bad ones and
   compounded them. Keep it off until an entry edge is demonstrated — but note this is
   a property of *our* selection, not a general rule.

## Exits are fine; the gap is entry timing

The decisive isolation: apply our exact exit policy to **his** entry moments (with
`held >= 120` forcing resolution, so nothing falls back to his exits). All 8 policies
came out net **positive** (+0.55 to +1.08%); the same exits on **our** entries were
all net negative. Gap ~1.7 pp, while every exit knob combined spans 0.6 pp.

Exit tuning is saturated. The residual deficit is **entry timing**, and four attempts
to find it in window features all failed — which is itself evidence it may not be
recoverable from what we ingest.

## Copy-trading him is dead

Mirroring his buys with our (good) exits, priced with real impact at 0.2 SOL:

| delay behind him | net %/episode |
| --- | --- |
| 0 slots (instant — unreachable) | +0.36 |
| 1 slot (0.4 s) | −0.13 |
| 2 slots (0.8 s) | −0.59 |
| 5 slots (2.0 s) | −0.76 |

Negative at every reachable latency, and our feed only sees **confirmed** trades, so
same-block is impossible. At 1.0 SOL it is −2.45% even at zero delay. This is not a
"be faster" problem — there is no latency at which it turns positive.

## Methodology rules earned here

**Post-hoc bucket selection on this dataset does not generalise. Four for four.**
`range`, `unique_wallets`, an apparent selection signal that turned out to be a
`stall` artefact, and `m_price_window.rise <= 1` — which bucketed at +1.28%/ep
in-sample and measured **−0.757%/ep** live, failing to survive even a change of
*exit* config, let alone a new window. Always run the live ladder before believing a
bucketing result, and never write one into a rule doc first.

**Mind the noise floor.** Per-episode PnL has a standard deviation of **9–15%**, so
at typical ladder sample sizes the standard error is ±0.27 to ±0.66 pp — *larger than
most differences the tables rank*. Check `n` and spread before quoting an ordering.
Ladder runs are also not independent samples (same market, different rules), so a
paired or bootstrapped comparison over matched tokens is the right way to settle
secondary claims; the naive independent-sample t-stats are the conservative prior.

Worked example of how easily this misleads: the fixes moved the rule from −1.88%/ep
(t = −4.33, solidly losing) to break-even. In-sample that looked like a +2.0 pp
improvement. Out of sample, against the same geometry on an untouched window, it was
**+0.465 pp (t = 0.71)** — a quarter the size and not distinguishable from zero. The
*sign* replicated; the *magnitude* did not. Variance also more than doubled
(sd 6.20 → 13.06), mechanically, because the unarmed trail and the stall cap had both
been truncating the right tail.

## Anything measured before 2026-07-28 is optimistic

Two independent errors, both in the same direction: the fee was modelled at 100 bps
instead of 125, and nothing charged our own price impact at all. At the 1.0 SOL
sizing every ladder in this effort used, that is roughly **3 pp per round trip** of
cost that was never charged. Numbers from this investigation — including the
break-even claim above — are unreliable at that resolution.
See [execution-costs.md](execution-costs.md).
