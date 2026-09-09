# 2026-09-08 - convexity is reachable, and selection is the entire gap

Third run under the corrected workflow: raw tape as the universe, every condition a column, a funnel before
every number, re-entry unlimited with tickets rather than tokens, both legs at 115 ms through
`study-kernel/kernel.py`. Week of 2026-09-01 .. 09-07, 11.76 M curve prints, 115,646 tokens.

The question was the operator's: find tokens with up-convexity, find the events that start each up-move,
enter there, exit to fit the move, and take several episodes per token.

## 1. The convexity is real and much of it is reachable

An up-episode is a swing low to a swing high, confirmed by a 15 % price retracement. Over the week:

| episode size | count | per day | tokens | median duration |
| --- | ---: | ---: | ---: | ---: |
| >= 10 % | 143,073 | 20,439 | 49,345 | 6.2 s |
| >= 20 % | 118,740 | 16,963 | 41,874 | 7.0 s |
| >= 50 % | 52,916 | 7,559 | 26,441 | 10.1 s |
| >= 100 % | 19,233 | 2,748 | 14,191 | 12.9 s |

They repeat. Among tokens with 20+ prints, 62 % have at least one 50 % episode, and given one, the chance of
a second is 39.5 %. So a behavioural door and a re-entry book are both well founded.

**Reachability is not the problem.** Noticing a 50 % episode once it is 20 % above its low and filling
115 ms later still leaves a median +31.6 % ahead (p25 +15.3 %), with a median 6.4 seconds to the peak; the
115 ms fill itself costs 5.8 %. On 100 % episodes: +67 % left, 9.7 seconds, 12.4 % fill cost. Our latency does
not eat these moves.

## 2. But the episode cannot be detected while it is happening

Firing on every print that stands 10 / 20 / 40 % above the trailing 60 s low, every instance, re-entry
unlimited: -3.5 % to -12.6 % a trade, 0 of 7 days, on 29,000-48,000 tokens and every exit tested (trails,
take-profits, clocks). Worse than random fires, which book -5.5 % on a 30 s clock.

Adding the operator's door - the token has already COMPLETED a 50 % episode, counted only from the print
where the retracement confirms it - does not rescue it. 25,061 door tokens, 182,687 tickets, every event
(pullbacks of 30 / 50 / 70 % from the last confirmed peak, and rises) and every exit negative, 0 of 7 days.

The reachable +31.6 % is conditional on an episode having happened. The base rate is what kills it: most
20 % rises are not the start of a 50 % episode, and nothing on the tape says which are.

## 3. Where the edge actually is

The one positive number measured at our own seat and our own fill, all session: firing on the print that
starts the burst the three N4 wallets join books +2.6 to +6.7 % a trade on **their** picks. The same event
across the tape books -4 %. So the difference is selection, and they act on about 1 in 100 candidates.

Labelling every candidate print (a router buy of >= 0.5 SOL, age 10-900 s, reserve 33-60) by whether one of
the three bought within 2 s, then learning the label from decision-time features and pricing **every fire in
each cell**, never their picks:

| cell | acted on | skipped |
| --- | ---: | ---: |
| top score decile, 45 s clock | +3.29 % a trade, 6/7 days, n=2,093 | -2.85 %, 0/7 days, n=53,164 |
| top score decile, 30 % trail, 600 s cap | +5.88 %, 5/7 days | -5.52 %, 0/7 days |

Same cell, same exit, same fill, opposite signs. The model lifts their response rate from 0.09 % to 3.8 %
across deciles but every decile still loses, because inside each one the prints they skip lose.

Twenty-four features were tried: age, reserve, tape density at three windows, buy and sell flow, largest
recent buy, distinct wallets and builds, racer and router counts in the last 2 s, gap, distance off the 60 s
high, lifetime prints, creator-sold, plus token identity - metadata URI host, name and symbol length, symbol
case, compute-budget price and limit, mayhem and cashback flags. None of them separates their picks from
their skips.

## 4. What this says to do next

The edge is a token-selection edge, it survives our latency and our fill, and it is not in the price tape,
not in the machine census, and not in the token metadata this repository stores. What remains, and is
computable from data already held, is the **holder book**: reconstruct per-token supply state from our own
buy and sell prints - holder count, top-holder share, the share of supply still held by creation-slot
buyers, the share held by wallets that have never sold, fresh-wallet share. Those are exactly the facts a
terminal's safety panel shows a human before they buy (market model 1.4), and they are the one axis in that
panel we have never computed. That is the next study.

Scripts: `study-kernel/` - `episodes.py`, `breakout.py`, `breakout2.py`, `selection.py`, `selection2.py`.
