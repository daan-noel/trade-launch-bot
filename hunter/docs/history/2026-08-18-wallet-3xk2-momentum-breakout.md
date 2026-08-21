# Wallet 3Xk2 — momentum-breakout scalper, clone refuted on latency (2026-08-18)

`3Xk2EuuSwKgniGNA4XkB33YY4mnEvcMnTLNpkKgGa14X`, `wallet_dict.id = 1416`. Study window
2026-07-22..08-16, restricted to the **18 ingest-clean days** (see §7 trap 1). Measured
from this workstation's Postgres (`trades`, `tokens`); no Helius calls.

**Bottom line.** He was profitable and stable — **+2.62% per SOL cycled, 95% CI
[1.37, 3.93], IS +2.81 / OOS +2.45** — but he is **not** a member of the
dip-reversion family. He buys *breakouts*, at or above the 30 s high, into positive
flow. That inverts the family's latency economics: our measured +1 slot landing cost
**9.87% of entry price** on his signal, against +0.82% for `63ot`. An honest clone of
his exact entries lost in every configuration. The signal is real and unreachable.

---

## 1. His real book

| measure | value |
| --- | --- |
| episodes (clean days) | 5,844 over 18 days |
| completed (sold) | 5,725 — **+4.69%**, +192.8 SOL |
| never sold | 119 (2.0%), −83.1 SOL |
| **true book incl. bags as total loss** | **+2.62%** (+109.8 SOL on 4,194 deployed) |
| bootstrap 95% CI (mint-level resample, 2,000 draws) | **[1.37, 3.93]**, 100% of draws positive |
| IS 07-24..08-07 / OOS 08-08..08-15 | +2.81% / +2.45% |
| daily | 12 of 18 days positive; worst −1.60%, best +7.47% |
| concurrency | median 1, p90 2, max 5 |

~6.1 SOL/day on ~233 SOL/day of turnover. The return is on turnover, not size.

**Tip is not measurable locally** — `raw_txs` is purged by retention and `fee_lamports`
is NULL on foreign wallets, yet he pays a `System Program: Transfer` on **both** legs.
Sensitivity on the book: 0.001/leg gives +2.34%, 0.003 gives +1.78%, 0.005 gives +1.22%,
and it crosses zero at ~0.0095/leg. Quote +2.62% as the pre-tip ceiling.

## 2. Operational profile

- **Fully automated, 24/7.** Buys are flat across all hours (226-356/hr). No human rhythm.
- **Durable-nonce racer.** Every buy carried `AdvanceNonceAccount` plus
  `CreateAccountWithSeed` / `InitializeAccount3` (a seed-derived token account, not an
  ATA) plus a tip `Transfer`. Sells carried no nonce — **the entry is the latency-critical
  leg and he knows it**. Roughly half the buys routed through
  `B5wU3wugvJUzA7CcPSHcuS9B1QcWFL5TgVPmzBDKrAvp`, the rest hit `Pump.Fun: Buy` directly;
  35 legs routed through Axiom Trade. Same infrastructure class as `trunoest`.
- **One shot per token.** 5,244 of 6,018 mints got exactly one buy; 6,643 of 6,754 sells
  zeroed the balance exactly. **No re-entry ladder** — the structural break from
  `64hP`/`omego`, whose re-entry index carries their edge.
- **Fixed size.** Buys clustered on exact curve-side 0.60 / 0.72 / 0.84 / 0.96 SOL,
  p50 0.707 = **1.66% of vsol**, so his own fills carried a constant **+1.66% impact**.
- **Selectivity 1.05%** — 5,074 mints of 483,218 launched.
- **Young tokens, shallow pools.** Entry age p50 101 slots (~42 s); vsol p50 43.9
  (real liquidity ~13.1 SOL).
- **Same-slot, not +1.** 87.0% of his buys landed in the *same slot* as another wallet's
  print, 12.7% at +1. `64hP` is a +1 slot reactor at 76.9%; this wallet is a slot faster.
- **Not a copy-trader.** The top wallet printing immediately before him preceded 1.86%
  of his buys.

## 3. Entry — momentum breakout, the inverse of the dip family

Measured on **pre-trade** market state with his own legs excluded from every aggregate.

| | `3Xk2` | `64hP` | `63ot` | `omego` |
| --- | --- | --- | --- | --- |
| price vs 30 s high (p50) | **−1.1%** | −22.7% | −20.8% | −12.6% |
| at or **above** the 30 s high | **47.1%** | — | — | — |
| price vs 30 s low (p50) | **+35.7%** | — | +29% | — |
| prior-2 s net flow (p50) | **+1.97 SOL** | — | −2.4 SOL | — |

He bought strength: at the local high, after a ~36% run off the 30 s low, into
**positive** two-second flow. Prior-60 s heat was modest (42 prints / 28.2 SOL) — he was
*early to a young breakout*, not late to a hot one. His impact (1.66% of vsol) is far too
small to ignite the move himself, so unlike `trunoest` he followed flow rather than
manufacturing it.

**The `flow(2).net >= 0` gate that is wrong for the dip family is exactly right here.**

## 4. Exit — one wide trailing stop

Retrace off the in-hold peak, measured separately from peak gain (the armed-trail rule):

| MFE bucket | n | p50 retrace off peak |
| --- | --- | --- |
| <5% | 123 | −32.7% |
| 5-15 | 1,451 | −25.6% |
| 15-30 | 1,406 | −23.3% |
| 30-60 | 1,321 | −21.7% |
| 60-120 | 891 | −22.0% |
| 120%+ | 532 | −26.8% |

Flat across the whole range, so **one trail at roughly −25%**, no take-profit, no separate
stop. Winners peaked +65.1% and exited −17.5% off it (hold 82.8 s); losers peaked +15.9%
and exited −28.8% off it (hold 39.8 s).

**The payoff is a lottery ticket.** 41.3% win rate, **median episode −5.6%**, mean +4.64%.
The top 1% of episodes (58 trades) produced +109.5 SOL — his *entire* book profit — while
the bottom 75% lost −533.8 SOL. A take-profit inverts this strategy; the wide trail is
load-bearing.

## 5. Clone — refuted, and the reason is latency alone

His exact entries handed over for free (perfect selection, perfect trigger slot), filled
at the first print at **S+1**, exited **per print** at the gapped price, dead-token
timeouts charged as total loss, 3.3% round trip:

| trail | mean, +1 slot (**honest**) | mean, zero latency | naive: dead exits marked at last print |
| --- | --- | --- | --- |
| 15% | **−1.67%** | **+8.00%** | −0.04% |
| 20% | −3.42% | +6.08% | +0.16% |
| 25% | −6.47% | +2.73% | +0.30% |
| 30% | −10.16% | −1.28% | +0.28% |
| 40% | −20.79% | −12.96% | −0.22% |

Two readings, both decisive:

- **The signal is real.** At zero latency a plain 15% trail earned +8.00% — *better* than
  his own +7.52% on the same anchor. Unlike `64hP`, his exit held no secret; a mechanical
  trail reproduces it.
- **One slot destroyed it.** Entry slippage at +1 slot was **mean +9.87% / p50 +8.28%**.
  Subtracting his own 1.66% impact still leaves ~7.6% of pure market movement in one slot.

### Latency cost scales with how close to the high a wallet buys

Same measurement, 1,500 buys each, 07-31..08-15:

| wallet | entry vs 30 s high | +1 slot slippage (p50) | (mean) |
| --- | --- | --- | --- |
| `63ot` | −20.8% | **+0.82%** | +0.89% |
| `omego` | −12.6% | +2.37% | +2.44% |
| `64hP` | −22.7% | +3.93% | +4.44% |
| **`3Xk2`** | **−1.1%** | **+9.24%** | +10.38% |

**This is the transferable finding.** A dip entry buys into falling price, so a slot of
delay is nearly free; a breakout entry buys into rising price and pays for it. Latency
tolerance is a *property of where in the move the entry sits*, and it can be read off the
dip-depth column before any simulation is run.

## 6. Selection is excellent and unreachable

Time-shifted control on his own mints, same 15% trail, honest accounting:

| entry anchor | n | mean |
| --- | --- | --- |
| −60 s | 5,844 | +35.73% |
| his slot +1 | 5,844 | −1.67% |
| +30 s | 5,809 | −13.03% |
| +60 s | 5,698 | −18.17% |
| +180 s | 5,320 | −28.85% |

The monotone decay confirms his timing beat every later entry — he was genuinely early
relative to the crowd. **The −60 s row is not an opportunity**: his median entry age is
42 s, so that anchor precedes token creation for most episodes and 57.6% of it landed on
the token's *first print*. It measures "snipe the launch of a token 3Xk2 will later buy",
which is look-ahead by construction.

## 7. Traps caught in this study

Two are new; the rest are the standing gates re-earning their place.

1. **Bag rate tracked our ingest health, not his behavior.** Raw counting gave 239 bags
   and a +0.48% book. Bag rate by day: 23.9% and 27.3% on 07-29/07-30 against a ~2.2%
   clean-day baseline — and those days ran at half the global print rate (07-29 00:00-02:00
   is missing outright). ~99 of the 239 "bags" are sells we never ingested. `64hP` was
   explicitly cleared on this test; this wallet failed it. **Gate: before attributing
   unsold positions to a wallet, plot bag rate against daily ingest volume.**
2. **Dead-token timeout fills.** Marking a trail that never fires at the last observed
   print books an exit at the peak of a token that stopped trading — 83.6% of timeout
   exits had **zero** prints in the following 5 minutes. This alone moved the clone from
   ~0% to −1.67…−20.79% and it grows with trail width, so it silently flatters exactly the
   configurations a search would promote. **Gate: a simulated exit must land on a print
   with liquidity after it, or be charged as a bag.**
3. **Own-impact anchoring** (standing). His fills carry a flat +1.66%; every price here is
   pre-trade spot.
4. **Own legs inside market aggregates** (standing). Wallet 1416 is excluded from all
   window aggregates and peak tracking.
5. **Look-ahead disguised as a control** (new instance). See §6.
6. **Unmeasurable cost reported as a range, not omitted.** His tip is invisible locally;
   §1 carries the sensitivity instead of an assumed zero.

## 8. Opinion

The wallet is a well-built, genuinely profitable momentum-breakout racer whose entire
edge is the ~9.9pp of price movement that happens in the one slot between his landing and
ours. There is no exit secret to recover and no selection filter to lift: his picks are
only valuable *before* he reveals them.

**Recommendation: do not clone.** The durable value is §5's latency-vs-dip-depth table —
it says a breakout entry needs same-slot landing to be worth attempting at all, and it
gives a cheap pre-simulation screen (entry dip depth) for whether any future copy target's
edge can survive our fills.

## 9. Data left in place

PG schema `x3k`: `tr`/`ep`/`ep2`/`ep3` (his trades and episodes), `mp`/`mps` (3.49M prints
on his mints with spot), `ent` (pre-trade entry state), `ex` (peak/retrace), `anchor`
(reachable +1 slot fills), `sim`/`sim2` (trail grid with liveness), `ctl` (time-shifted
control), `cmp` (cross-wallet slippage), `clean` (ingest-clean days). Drop when finished.
