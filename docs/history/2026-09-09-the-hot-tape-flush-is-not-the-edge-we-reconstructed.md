# 2026-09-09: the hot-tape flush, reconstructed from public tape and refuted

The third run of the same day, after
[the-client-is-the-unit](2026-09-09-the-client-is-the-unit.md) and
[the-loss-axis](2026-09-09-the-loss-axis-and-the-exit-that-uses-it.md) left the frozen sentence
waiting for 09-30. The question put to this run was where to spend the wait: the fifth and last
unmeasured solo node, hot-tape re-entry - `omegoM`, `64hP97`, `AbQcLH`, `sssssw`, `8fStGV`,
`49uohd`, six wallets and 133,939 trades.

It was chosen on three premises. Two of them turned out to be wrong, and the third held.

## The premises

**"The 115 ms seat is a tailwind on both legs here."** The recorded direction factor for a buy
into a flush is 0.889-0.943 - a 6-11 % discount at our fill. On the 17,204 fires this run
produced, the buy leg filled at a **mean of -0.66 %**, a **median of 0.00 %**, and cheaper than
the decision print only **22.5 %** of the time. The direction table was measured on dense burst
prints. A flush on an aged coin printing 20-80 times per 20 s is not dense enough for 115 ms to
move the fill at all. *A direction factor is a property of a tape density, not of a side.*

**"Absorption is the discriminator."** The story was that a flush which is being absorbed - buy
SOL still arriving while price falls, and the fall decelerating - separates a turning low from a
falling knife. Both halves are refuted, and both are **inverted**:

| term | lift against every buy on the tape |
| --- | --- |
| absorption, SOL bought per percent of price given up | **1.39 at the lowest bin, 0.66 at the highest** |
| deceleration, last 2 s of the fall against the 5 s pace | **0.57 where the fall has stopped, 1.48 where it accelerates** |

They buy **into** the flush, not after it turns, and where absorption is weakest.

This is not a fitting failure, it is the venue. Absorption is an order-book idea: buying that does
not move price because resting size eats it. A constant-product curve has no resting size - every
buy moves price by exactly its own arithmetic, and windowed flow **is** the price path. So
`buy SOL / (-price fall)` divides the signal by itself, and the conjunction "price fell AND buyers
were the larger side" holds on **32 of 95,645** real node buys. The whole family closes on the
venue, before any cell is measured.

**"Their tell is a state, not a print."** This one held. Buy-flow acceleration - SOL bought in the
last 5 s against the 5 s before it - carries a **2.15** lift in its top bin, and drawdown from the
coin's peak so far carries **2.04** below -50 %, monotone to 0.28 at the peak. Age is monotone
0.27 to 1.83. The moment is public and it is spellable with no wallet named.

## Two ways the cell could have been manufactured, closed before the book was read

The first pass put the node's own buy inside its own 5 s flow window and read a 2.44 lift on
acceleration. Every feature is now built from prints `0..i-1` and the reserve `v[i-1]`.

And the fire anchor: the state at index `k` is what a watcher holds when print `k-1` lands, so the
fill is the last print by `t[k-1] + 115 ms`. Anchoring at `k` fills later, and on a falling tape a
later fill is a **cheaper** buy - about half a print of optimism, pointing exactly the way this
cell was looking.

Then the node-blind control, which is the one that decides whether a flow term is a rule or a
disguise: recompute acceleration with the six node wallets' SOL removed from both windows. Lift
**2.15 -> 2.16**. It is public tape, not the operator's own print.

## The book

`dd <= -20 % . fall5 <= -5 % . acc >= 2 . n20 >= 20 . age >= 120 s . reserve 42.43-85`, one
position per coin, 0.2 SOL, `lag_115` on both legs, no door.

Eighteen exit families, from a 10 s clock to the shipped armed trail. **Every one negative.** The
best is a 10 s clock: 17,204 trades, **-2.82 % a trade**, **0 of 8 days** positive. About sixty
pre-registered cuts - the slow-wall door, the bundler exclusion, the creator permission, drawdown
crossed with acceleration, busyness, buy share, flush size, hour of day - are all negative too,
best of them **-2.56 %**. The hour control is flat, which is what a control should be.

The ticket floor passes **7 of 8 days** on 3,535 coins and 75 clients. The supply problem that
closed the frozen sentence does not exist here. Money is the entire failure.

## The number that closes it, and it is not the toll

The first reading of this run said the toll ate an edge we had correctly found. That reading was
wrong, and the error is worth more than the result: it compared **their net margin** to **our
gross move**. The roster's `margin` column is already net of the same 125 bps
(`rb-solo-nodes.py:140`, `net = sol_out*0.9875 - sol_in*1.0125`).

Decomposed on one basis, the clock-20 book is:

| term | per round trip |
| --- | ---: |
| the price move we capture | **+0.29 %** |
| pump.fun protocol fee, 125 bps x 2 | -2.47 % |
| tip + priority, 0.000225 SOL x 2 at a 0.2 SOL clip | -0.22 % |
| our own impact, `B/vsol` x 2 | -0.70 % |
| **booked** | **-3.10 %** |

**At a zero fee this cell still loses 0.63 % a trade.** And the node's 1.10 % net implies a price
move of **+3.65 %**. We capture **+0.29 %**.

So the reconstruction did **not** work. It found terms that separate their buys from the tape at
a lift of 2.0-2.2, and separating their buys is not the same thing as capturing their return.
Three and a half points of move are somewhere we did not look - in *which* flush, or in *when*
inside it, or in the exit. The toll is a fixed 3.4 % entrance fee and it is not the reason this
cell is red; it is the reason a cell like this can never be rescued by a small edge.

Two points on the toll itself, since it decides what is worth trying next: **2.47 of the 3.4
points are pump.fun's own protocol fee**, which we do not set and cannot avoid on the curve. The
tip and priority are ours and are already small (0.22 pp at a 0.2 SOL clip). Our impact is ours
and is a sizing choice - the cheapest clip at reserve 50 is `sqrt(0.000225 x 50) = 0.106 SOL`,
worth 0.16 pp against the 0.2 we use. **So the toll floor is about 2.7 %, and no engineering
moves it much.** A strategy whose average move is a few percent is structurally impossible for us
whatever we do; only a book whose winners are tens of percent can pay this entrance fee.

## The event fires on its own, and it still lands in their habitat

A worry worth answering in the record, because the answer surprised the run: does firing a
tape-state event amount to "buy when they buy"? It does not - no wallet appears in the firing
condition, and the event fires on the whole tape - but measured against their trades as a ruler,
it lands almost entirely where they trade anyway:

| | |
| --- | ---: |
| fires on a coin the node never touches | **3.5 %** |
| fires within 5 s of one of their buys | **47.2 %** |
| of THEIR buys, the share the rule fires on at all | **4.28 %** |

*The state is their habitat.* A public term reproduced a private habit's coin list and roughly
half its seconds with nothing named. That is the first time in this program a tape-state event has
done that, and it is the one durable positive from this node.

The money then grades cleanly by that distance, same clock-20 exit:

| population | trades | price move | per trade |
| --- | ---: | ---: | ---: |
| on a coin they never touch | 524 | **-20.37 %** | -23.12 % |
| every fire | 14,869 | +0.29 % | -3.09 % |
| on a coin they do touch | 14,345 | +1.05 % | -2.35 % |
| **within 2 s of one of their buys** | 4,921 | **+2.24 %** | **-1.21 %** |
| over 120 s away | 563 | -0.22 % | -3.57 % |

Arriving *before* them beats arriving after by 0.27 points, so front-running this node is not a
lever and their own impact is not what the trade is made of.

## The ceiling, which closes the node rather than the sentence

Hand the rule a perfect oracle for both the coin and the second - their own wallets, which
7.4 law 20 forbids shipping - and read every exit family. Best of eight: **-1.17 % a trade,
1 of 8 days**. Every one negative.

At that second they take +3.65 % after their own impact; we take +2.24 % gross, +1.54 % after
ours. The residual 2.1 points is exit choice - they decide when to sell, we read a fixed clock -
and no fixed exit in the grid recovers it.

**So the closure is not "we cannot find their moment". It is "their moment, at our size and our
seat, is not worth a 3.4 % toll".** Two things in the node were still never tested and neither
runs without a reason first: a size small against the flush, and an event firing seconds *after*
the flush ends. Neither is worth much against a ceiling that is 1.2 points under water.

Scripts: `node-derivation/hot-tape/cvx_hottape.py`, `cvx_hottape_book.py`, `cvx_hottape_sel.py`.
Evidence 6.10.
