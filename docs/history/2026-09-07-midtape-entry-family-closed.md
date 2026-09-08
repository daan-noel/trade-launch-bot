# 2026-09-07 - the mid-tape entry family is closed at our seat, and why

Second run of the day, under the corrected workflow: the raw tape is the universe and every condition is a
column, so no filter can hide inside the query that builds a table. One week, 2026-09-01 .. 09-07, 11.76 M
curve prints on 115,646 tokens, of which the token table now resolves 115,645. Both legs at 115 ms through
`study-kernel/kernel.py`, re-entry allowed, tickets rather than tokens, concurrency reported.

## The result in one line

Every tape-state condition tested is negative in every band, on every exit, on zero of seven days.

## What was tested

**Machine cadence.** A volume bot is a configured service, so its next prints should be committed before
they land. The configuration is genuinely visible: of 166,108 (token, build) buy runs, 14.3 % hold a fixed
clip and 1.8 % hold a fixed clip and a regular period. On those the next beat lands within 2x of the running
median 70-75 % of the time against 28 % for all runs, median ratio 1.000. So cadence is real and readable.

It is also worthless as a signal, for a reason worth keeping. The bots that run a metronome spend
0.001-0.05 SOL a beat, so their prints move no price; the thesis was that the bumps buy feed placement and
placement brings other people. Matched on age band, reserve band and prior-60 s density, forward arrivals
from other machines after a cadence confirmation equal the control everywhere (115 vs 114, 147 vs 149, 208
vs 217 at one minute). On the same tokens a random buy print is better than a confirmation print: peak
ratio 1.72 vs 1.42 over five minutes. **A running metronome marks a token that needs help, not one that is
getting it** - the bot bumps hardest when nothing organic is arriving.

**The extraction tell.** Per token, the buy build with the most post-creation buys is the push machine; the
tell is a sell by a wallet that bought through it. As a permission it is inverted: on a 30 s clock,
"push has not sold" reads -7.22 % a trade and "push has sold" -4.32 %, against -4.80 % for all fires. As a
cause-based exit it changes nothing. The unsold state marks a token whose fall has not happened yet.

**Everything else, on the same rows.** Router buys at 0.5 / 1 / 2 SOL, racer buys, creator permissions,
dip, active tape, silence-break, and clock / take-profit / trail / armed-trail exits. Bands of reserve
33-38 through 55-60, age 10-30 s through 300-900 s, density quiet through storm. Every cell negative.

## Why, and what it means for the next rule

The toll on an unmoved token is 3.2-4.0 %. Random fires book -5.5 % on a 30 s clock and -20 % on 300 s. The
gap is token decay, and it is monotone: older tokens lose less because the fall already happened, higher
reserves lose less because there is less left above them. No condition tested moves the sign, and the best
cells are merely closer to the toll.

So a buy-and-hold rule in the mid-life band cannot be positive by selecting an entry moment. A positive rule
has to come from somewhere else:

* a **graduation selector**, where the quantity captured is `(vsol_max / vsol_entry)^2 - 1` on tokens that
  actually reach the wall, held for minutes with a wide exit. This is the one family with a survivor in this
  repository ([campaign-break-money](../../hunter/docs/plans/strategies/campaign-break-money.md), the
  gap-burst harvest rule), and it is where the next effort belongs;
* or an **execution** edge, moving price rather than reacting to it.

## Workflow changes that stand

* The raw tape is the universe. Every study prints a funnel of tokens and prints surviving each column
  before it reports a number. The universe-filter bug is unreachable by construction, not by vigilance.
* Tickets, not tokens: re-entry is unlimited, one position at a time, concurrency and max-open reported.
* Features are windowed at the decision print, never cumulative since creation.
* The census is a relationship graph - launch build to creation-slot buy builds to the top post-creation buy
  machine to the paired sell build (`wk_tell.csv`, `wk_runs.csv`). The buy-to-sell pairing is sharp: router
  builds pair with their own sell build at 94-98 %, and small private operators show up as 5 to 76 wallets
  spanning thousands of tokens.

Scripts: `study-kernel/` (`census_graph.py`, `cadence.py`, `cadence_event.py`, `tell_rule.py`, `funnel.py`).
