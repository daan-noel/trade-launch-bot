# 2026-09-10: the hot-tape node worked end to end, and what it is made of is their own arrival

A full pass on one node at the user's direction, with two design points the user supplied that
changed the method for everything after it. Evidence 1.5 to 1.8.

## The gate: they are slow, and they share a trigger

Six wallets, 95,645 buys. When one opens a position another buys within 2 s at **6-9x** a
same-coin time-shuffled null, and the lift decays 9.1 -> 3.7 -> 2.2 across 2 s / 10 s / 60 s. Six
independent readers inside the same two seconds means the trigger is public and on the tape.

Their delay from a public trigger to their order landing is **p50 0.4-2.2 s** (64hP 2.183 s), and
a 115 ms fill from that trigger lands at an earlier print than theirs **68-85 %** of the time.
Assumption-free, the spread inside a 2 s agreement cluster is p50 0.429 s. **We are 4-19x faster
than the people who make this trade work, and they are profitable anyway** - so the edge survives
a second of delay.

Their buying is also not a burst. Gaps between consecutive buys on one coin are UNDER-dispersed
at short lags (64hP 1.8 % under 5 s against an exponential null's 3.9 %) and over-dispersed at
20-60 s. It is a refractory period, not a session.

## The user's design: compare inside one mint

*If a wallet buys one mint several times, those decisions share the Telegram channel, the
narrative, the creator, the launch build and the hour. So compare their buy moments against
non-buy moments ON THE SAME MINT, and any surviving difference has to be on-chain.* Off-chain
data is excluded by construction, and the door is held constant so the event is measured alone.

Every instrument step before this one compared their buys against a sample of the whole tape and
therefore mixed "they chose this coin" with "they chose this second".

The second half of the idea mattered as much: **each coin needs its own ruler.** A -5 % dip on a
coin that dips 15 % a minute is nothing. Features z-scored against the coin's own prior history
beat absolute ones almost everywhere - `wake` 0.605 -> `wake_z` 0.660, `vol20` 0.550 -> 0.591,
`n60` 0.475 -> 0.418.

## What they look for, and the bind

**LULL then WAKE**: an aged coin, deep below its own peak, unusually quiet for a minute, that
starts printing hard in the last two seconds with real SOL on both sides. It also explains the
slow reaction - after a lull there is no crowd to race.

But the two halves have opposite properties:

| half | dwell | reaction cost at 115 ms | within-coin lift |
| --- | ---: | ---: | ---: |
| WAKE | 0.216 s | **+5.98 %** | 4.5 |
| LULL | 24-52 s | -1.0 % | ~1 |

**The half that carries the information has already moved the price; the half we can reach in
time carries none.** The +5.98 % is measured - the move between the state a watcher held and our
own fill. It also refutes lull-then-wake as their trigger: if reacting at 115 ms costs 6 %, their
0.4-2.2 s costs more, and they are profitable. Coverage said it quietly - the conjunction covers
1.2 % of their buys.

## The model: their moment IS learnable, and it is worthless

Hard thresholds were the mistake. `wake_z` sits at 0.660 across all 93,137 of their buys, so
cutting at `wake_z >= 2` kept the 2 % where the price had already jumped. Fitting the whole vector
at once, split by COIN so a coin never appears in both halves, reads **AUC 0.7212** on held-out
coins using only coin-relative features, with the top decile inside a coin holding 44.4 % of their
buys against a 20 % base.

A control artifact was found and priced on the way: with PRINT controls a control's gap is the
full inter-arrival interval while a case sits inside one; with TIME controls a random second is
usually quiet (cases 0.156 s vs controls 4.163 s). Neither is neutral, so the gap family was
deleted and the signal held - 0.7678 -> 0.7315, and the two opposite biases bracket it.

**First time this program has reproduced a professional's timing out of sample. And it books
-4.24 to -10.62 %/trade, 0 of 8 days, with a reaction cost of only -0.5 to +1.2 %.** The fill is
cheap. We can stand at their moment, ahead of them, and lose five percent.

## Where the money actually is

Splitting the model's own fires put **11.6 points in the coin**: -2.63 % on a coin they touch,
-14.21 % on one they never touch. So the door census ran - and the winning group is not creation
structure (again near 1.0 concentration, as in 6.8) but **early liveliness**: half the tape is
dead by age 60 s and carries 7 % of their trades, while coins at reserve 50-70 by 60 s are 9 % of
coins and 34 % of their trades.

**No public door rescues the book.** Best is -6.29 %, 0 of 8 days.

Because the 11.6 points were never the coin:

| | trades | per trade | days positive |
| --- | ---: | ---: | :---: |
| one of them buys INSIDE our hold | 7,258 | **+2.37 %** | **7/8** |
| they touch the coin but not during our hold | 7,847 | -8.02 % | 0/8 |
| they never appear at all | 8,984 | -12.71 % | 0/8 |

**The node is incoming demand, and these six are the demand.** Arrivals happen on 30.1 % of
fires; with arrivals at +2.37 % and the rest at -10.53 %, break-even needs **81.6 %**. Seven
abort-on-no-response exit families across five thresholds cannot close it - cutting the
non-arrivals also cuts the arrivals, and the toll is per ticket.

## What survives

The node is closed with its mechanism named, which is worth more than the four earlier node
closures that named none. Three things carry forward and are now method, not opinion:

* **the within-mint control**, which kills off-chain confounding by construction;
* **the coin-local basis** - one ruler cannot serve 11,765 coins;
* **the reaction cost**, reported beside every event from now on. Over about 2 % and an event is
  unreachable whatever its lift.

And one standing result is sharpened: *instruments must MOVE price, not react to it.* Everything
measured here says we can be early, cheap and right about the moment, and still lose - because
being early to a move that nobody else joins is not an edge.

Scripts: `node-derivation/hot-tape/cvx_hot_sessions.py`, `cvx_hot_latency.py`, `cvx_hot_event.py`,
`cvx_hot_event2.py`, `cvx_hot_event3.py`, `cvx_hot_book.py`, `cvx_hot_lull.py`,
`cvx_hot_model.py`, `cvx_hot_model2.py`, `cvx_hot_score.py`, `cvx_hot_door.py`.
