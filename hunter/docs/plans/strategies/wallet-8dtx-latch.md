# Wallet `8dtx` — the ix-client latch, and where the edge actually sits

Re-derivation from the PG firehose 07-25..08-22 (11,018 buys / 8,880 mints), built around
the **decision point** rather than the landed fill. This file carries the decomposition and
the latch; the older behavioural spec is
[wallet-8dtx-logic.md](wallet-8dtx-logic.md), whose "the edge is selection, not the
trigger" verdict this measurement reverses.

## The book, restated

+489 SOL, mean **+5.90 %/trade**, **35.1 % win**, median hold **19.8 s**, 1 buy -> 1 sell.
Entry age is **not** a rule: p5 11 s, p50 98 s, p95 1,015 s. The tape is never silent at his
entries — the median gap to the previous print is **54 ms** — so "quiet" is a SOL statement,
never a print-count one.

## The decomposition

Three moment sets, all alive-matched and age-matched, his own trades excluded from every
window, and the final **400 ms dropped** so that his own landing slot cannot leak in.

| set | n | fwd60 | dead@3min |
| --- | ---: | ---: | ---: |
| random moment, token he never traded | 9,686 | 7.78 % | 40.0 % |
| random moment, token he did trade | 6,827 | 14.14 % | 12.2 % |
| his actual entries | 9,717 | **28.56 %** | 14.6 % |

**Token choice buys survival; timing buys upside** — but see the latch section: these are
one mechanism at two time scales, so the two rows must not be added.

**Token choice buys survival; timing buys upside.** The pick moves death 40.0 -> 12.2 % and
adds +6.4 pp; the moment adds a further +14.4 pp and no death protection at all. Roughly
31 % selection / 69 % timing.

## The trigger: a same-client burst

Burst = a maximal run of same-ix-client buys on a mint with consecutive gap <= 400 ms.
1.45 M bursts over his 8,880 mints, wallet 2720 excluded. Response = his buy landing within
0-1 slot of the burst's last tx. The lag histogram spikes **4.11 % at the burst slot,
2.00 % at +1, then flat 0.8 %** from +2 on, so the reaction is real and it is fast.

| axis | he responds | he ignores |
| --- | --- | --- |
| burst SOL | **band 0.3-4.0**, peak 1.0-2.0 (2.76 %) | <0.3 -> 0.21 % (lift 0.8, i.e. nothing); **>=4.0 -> 0.64 %** |
| tx count | 2-3 (peak 1.90 %) | 1 (0.91 %), 6+ (0.66 %) |
| wallets | **2** (1.77 %) | 1 (0.90 %) |
| pool depth | **vsol < 46** (2.6-2.8 %) | 45-60 -> 0.29 %; **60+ -> 0.00 %** |
| client | `sss5N9Bf7…` **9.97 % / 22.5x**, `B5wU3w…` 6.23 %, Maestro 3.19 %, Bloom 3.38 % | Axiom 1.11 %, GMGN 0.75 % |

Both sides are bounded. The upper cutoff is the informative one: bursts >= 4 SOL carry the
**highest** forward return (34.60 %) and he refuses them, so the ceiling encodes "the move
has already happened", not a size floor.

**A per-trade `max(sol)` cannot see any of this.** Three 0.3 SOL txs summing 0.9 read as
0.3 and the signal disappears. Group into bursts first, then band.

## His residual pick adds nothing on top

74,951 bursts pass band + depth + tx-count; he takes 4,222 (5.6 %). Priced identically at
**burst end + 1 slot** — an entry we can actually reach — the forward-60s maximum is:

| | n | fwd60 max | up10 |
| --- | ---: | ---: | ---: |
| bursts he SKIPPED | 70,655 | **29.60 %** | 60.2 % |
| bursts he TOOK | 4,222 | 26.33 % | 51.9 % |

The event carries the signal; his choice among qualifying events does not improve it. So the
selector does not need cloning — which reverses [wallet-8dtx-logic.md](wallet-8dtx-logic.md).
Caveats that keep this provisional: fwd60-max is an unrealizable oracle, skipped bursts
auto-correlate inside a token, and his taken set carries his own impact.

## Size, honestly

Against non-qualifying bursts on the same tokens: qualifying 29.78 % vs single-tx 24.57 %,
tiny-burst 23.77 %, deep-pool 18.00 %. Most of the separation is the **vsol < 46 depth
gate**; the burst shape adds ~5-6 pp of oracle upside on top of it. Modest, and unpriced.

## The latch is this same mechanism, integrated

An earlier pass scored a latch — which fleet has ever bought the mint — at
death@3min 42.5 -> 24.9 % on tokens he never traded. Those are the same fleets that burst,
so the latch is the burst signal accumulated over the token's life rather than a second,
independent effect. Do not add the two.

## Latency verdict

Repriced at a 1 s entry delay the his-vs-same-token timing gap collapses **14.4 -> 4.8 pp**.
That subsample conditions on a print landing in [+1 s, +2 s] and therefore flatters the
control, so the direction is safe and the magnitude is not. The asymmetry is structural:
a state does not decay while an event does. That measured decay is of GENERIC price drift,
not of the burst signal, so it does not license dropping the trigger — the burst table above
is priced at burst end + 1 slot and survives there.

## Measurement rules this study imposes

- Drop the final 400 ms of every window. His landing slot runs a ~100x client lift against
  background and is pure post-decision contamination.
- Never filter the control set on anything forward-looking (`trade_count`, `ath_price`).
  Sample every token and let dead windows be dead — 88 % of random tokens have no print at
  all in the 10 s before a matched moment, and that fact is itself the first result.
- Stratify every identity claim by prior trade count, or activity masquerades as selection.

## Artifacts

Schema `w8`: `buys`, `nb`, `ctrl`, `mom`/`mom2`, `feat`, `latch`, `fwd`, `pnl`.


## Never compare "his mints" against control

The set "tokens 8dtx bought" is defined by an event that happens after the burst being
scored, so bursts on it are conditioned on the token later climbing into his entry zone.
That conditioning alone manufactures the entire apparent gap: his mints read +7.21 IS /
+6.09 OOS at vsol<36, and control tokens conditioned on the same forward event - later
reaching vsol 36 - read +7.18 / +6.98.

One forward flag swamps every feature. Splitting vsol 36-46 bursts on whether the token
later reaches 46:

| | reaches 46+ | does not |
| --- | --- | --- |
| control | +8.74 / +11.52 | -26.66 / -26.88 |
| his mints | +8.46 / +7.42 | -9.75 / -9.44 |

At matched outcome control is as good or better. The only difference is the base rate of
reaching, 65.7% against 51.2%.

Choose the observation point without hindsight - a fixed token age works - compute features
from data strictly before it, and measure forward across every token in both sets. Re-run
any his-vs-control gap with the control conditioned on the same forward event before
believing it.

## Closed axes
Refuted on control money, IS/OOS, both pooled and inside vsol<36: ix composition at fixed
age, tool-arrival sequence (gap since a new tool, arrival acceleration, tool count, age of
the 2nd tool, first-2 signature, first router debut on a direct-only token), burst
recurrence, burst shape, background dip and sell share, and prior-activity density in every
form (cumulative trades, cumulative gross, trades per second).

Content reuse - prior launches sharing a `name`, `symbol`, or `meta.uri`, counted causally -
is the one axis that holds its sign across halves. His tokens carry an unseen name 46.1% of
the time against 32.6% control and 29.4% of all launches. It is worth 2-6pp, never reaches
zero, and dissolves once age enters the cell, so it stands as a quality tilt rather than the
gate. `meta.uri` covers only ~19% of tokens and covers both sides equally.

## His entry rule

Derive it by discriminating taken from refused bursts inside his own token set - 853,209
same-tool bursts on his 8,864 mints, 22,302 taken. Both classes share the same tokens, so
token selection is held constant and no control set enters the fit.

| family | term |
| --- | --- |
| liquidity | `vsol <= 46.24`, real reserves <= 16.24 SOL |
| ix | same-tool 400ms burst, `burst_sol > 0.30` |
| time | token `age > 19s` |
| price action | `trail > -64%` from the token peak |
| flow cap | `net30 <= 11.08 SOL` |
| own state | first entry on this token |

Precision 14.24% against a 2.30% base - 6.2x lift - capturing 55% of his trades at an 8.9%
fire rate, stable across all five weeks. Adding `gap_prev <= 67ms` tightens it to 7.9x at a
3.74% fire rate, close to his own concentration, holding 29.5% recall and five weeks.

The flow cap and the busy-tape term both cut against a lull reading: he takes the burst on an
active tape and skips the frenzy.

The rule's taken bursts return +5.57% net on his tokens against his real book of +5.90% per
trade, which confirms the pipeline prices him correctly.

## The rule is the whole of his entry logic

Under his own rule on his own tokens, the bursts he SKIPS return +6.14% net at a 60.0% win
rate against +5.57% and 56.2% for the ones he takes, with skips ahead in four of five weeks.
Nothing further separates his entries, so no additional moment-level filter is hiding.

The same rule on random tokens returns -3.14% net and is negative every week, against a
-4.19% all-burst baseline. His edge is the gap between the tokens he watches and random
ones, and it is settled before any burst fires. A token history of prior rule-firings does
not reproduce it - returns are flat and negative at 0, 1-2, 3-9 and 10+ prior firings.

Measurements taken on tokens he bought carry the conditioning described above, so treat the
rule as established and the token-universe gap as unproven.

## Group bursts by slot, not by a time gap

Regrouping bursts on `(mint, tool, slot)` instead of a 400ms gap sharpens the fit and
exposes a term the time window hides.

| grouping | best stable leaf | fire rate | recall |
| --- | --- | --- | --- |
| 400ms gap | 18.17% precision, 7.9x lift | 3.74% | 29.5% |
| slot | 21.13% precision, 8.3x lift | 2.23% | 18.5% |

A 2.23% fire rate sits on his own 2.55% base, so the slot form reproduces his concentration
directly.

The term that only appears on the slot axis is **how many distinct tools burst in the same
slot**. His take rate rises monotonically with it - one tool 1.65%, two 4.06%, three 6.66%,
four 8.93% - a 5.4x gradient on a single feature. A 400ms window spans two slots and smears
that simultaneity, which is why the ix axis reads inert under time grouping.

Slot grouping also separates two things the old `gap_prev` conflated: bundle tightness inside
a multi-tx burst (median 3-31ms) and the tape gap ahead of a single-tx burst (median 0.53s).
Under slot grouping the first is implicit, and `pre_gap` - the gap to the last print strictly
before the burst - carries tape silence on its own.

Metric windows are time-keyed (`window_size_sec`), so the same-slot term is not expressible
today. Trades already carry `slot` and canonical order is slot -> tx_index -> leg, so a
`window_slots` param alongside `window_size_sec` in `flow_window`/`flow_split` closes it.

## Multi-tool same-slot does not survive its own fill

Priced on control slot bursts with honest `ret30` and 3.0% costs, the gate is monotone at
slot-0 pricing - one tool -5.30, two -2.56, three +1.73, four or more +7.92 - and it is not a
size proxy, since at a matched `slot_sol` three-plus tools beats one tool by 4.5pp in the 3-8
SOL band and 10.9pp above 8 SOL.

Repricing entry one slot later collapses it: four-plus tools falls to +1.85 and three tools
to -1.24, with +1.58 at two slots. Around 6pp of the 7.92 sits inside the signal slot and is
unreachable.

What survives the lag is unstable. Four-plus tools at +1 slot runs -6.73, +2.63, +0.60,
+8.34, -6.57 by week against per-week standard errors of 1.3-2.5pp, so the swing is regime
variation rather than sampling noise, and one week carries the average. Adding the other rule
terms degrades it further, from -0.35 alone to -3.52 with the full stack.

Treat the term as the sharpest available descriptor of his decision and not as a rule.
