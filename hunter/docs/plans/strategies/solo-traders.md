# The 26 independent traders, and the five nodes they run

The roster's 77 wallets reduce to 43 actors: 7 machines hold 41 of the rows, and 36 wallets have
no co-selection partner. Of those 36, **26 carry a margin that clears its own sampling error.**
This file is that set - the only part of the roster where one row is one opinion - grouped into
the five decision nodes its members answer.

| where | what |
| --- | --- |
| `hunter/_local/solo-traders.csv` | one row per wallet, plain-language columns, all 36 with the 10 rejected marked `use_it = no` |
| `hunter/_local/solo-nodes.csv` | one row per node, every aggregate below, over the 26 only |
| `hunter/_local/solo-nodes/` | the same split one file per node - `0-node-summary.csv` then `1-hot-tape-re-entry.csv` .. `5-deep-age-big-clip.csv`, numbered by net SOL, each sorted `use_it = yes` first |
| `hunter/_local/roster-nodes.xlsx` | sheets `NODE SUMMARY` and `NODE 1..5`, the same rows colour-coded per node with rejected wallets greyed |
| [rb-coselect.py](rb-coselect.py) | establishes independence - which wallets are one machine |
| [rb-actor-sheet.py](rb-actor-sheet.py) | the ranking, `t` and its bootstrap; writes the workbook and `solo-traders-base.csv` |
| [rb-actor-tape-share.py](rb-actor-tape-share.py) | tape share on coins a wallet prints: wash vs reader (evidence 5.1) |
| [rb-solo-nodes.py](rb-solo-nodes.py) | the node anatomy in this file, the per-node files, and the `NODE *` sheets |

**Run order is `rb-actor-sheet.py` then `rb-solo-nodes.py`.** The first rewrites the whole
workbook, which drops the `NODE *` sheets; the second re-adds them and turns
`solo-traders-base.csv` into `solo-traders.csv`.

Window 08-27 .. 09-03, `census.rb_ep2`. Selection and the actor split:
[_!___evidence.md](_!___evidence.md) 5.1-5.2.

**Two data grains, and they are not interchangeable.** Outcome columns - margin, median trade,
win rate, tail share, hold, entry reserve, re-entry - come from **every** episode in the window.
Pre-entry context columns - silent slots before, prints before, buying share, distance below the
recent high - come from `census.rb_ctx`, a **random 150 episodes per wallet**: unbiased against
the full set on entry slot (two-sample KS `p` 0.32 to 0.98 on five wallets), but wallet-weighted
and carrying sampling error the outcome columns do not.

---

## 1. What makes a wallet eligible

Two cuts make the **roster**. A third screen makes an **instrument**.

**No co-selection partner.** Wallets whose coin lists either never intersect (one operator
splitting a work queue) or almost entirely coincide (one operator racing itself) collapse into
one actor. 41 rows go that way, leaving 36.

**A margin that clears its own noise.** The ranking column is `t` = return on spend divided by
its standard error, the error from a 2,000-resample per-episode bootstrap. Ten of the 36 sit
under `t = 2`; nine of those ten have a 90 % interval that crosses zero and three are outright
negative. They are dropped, leaving **26**.

**A reader, not volume manufacture** ([_!___derive.md](_!___derive.md) pick, 4.0). Tape share on
the coins he prints: a wash owns a large share and round-trips to about zero minus fees
(evidence 5.1). Ix book from lake `ix_labels`: `InitUserVolumeAccumulator`, bundled
`TransferChecked`, or `CreateCoinAndBuy` as the book is a hopper even when tape share is small.
`ApfmkS` stays a roster row (`t = 2.41`) and is not an instrument.

The first two cuts are necessary and neither is sufficient. A machine's wallet can have a huge
`t` - the 16-address machine's legs run tens of thousands of trades - and still be one opinion
counted sixteen times. `t >= 2` does not prove a reader.

`margin_if_10_best_trades_deleted` in the CSV is the fastest sanity check on any row: `64hP` goes
1.18 % to 1.07 %, while `9RNZnq` goes 8.25 % to 3.28 % and four of the ten rejected wallets turn
negative.

---

## 2. The five nodes at a glance

Every node answers the one question - *whose money is still coming, and what on the tape says
so* - with a different actor.

| node | n | trades | net SOL | margin | median trade | won | lost > 20 % | best 1 % of trades | entry reserve | headroom |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| hot-tape re-entry | 6 | 133,939 | 938.2 | 1.10 % | -2.07 % | 43.8 % | 13.6 % | **180.6 %** | 55.5 | +329 % |
| mid-tape one-shot | 7 | 24,381 | 427.9 | 1.73 % | **-3.34 %** | 40.5 % | **15.9 %** | 122.9 % | 42.4 | +634 % |
| instant launch | 6 | 18,860 | 205.8 | 1.31 % | -2.50 % | 33.3 % | 9.1 % | 116.0 % | **32.2** | **+1,176 %** |
| quiet deep-age | 4 | 20,610 | 110.3 | 1.12 % | -2.50 % | 37.9 % | 5.9 % | 112.2 % | 48.0 | +473 % |
| deep-age big clip | 3 | 1,671 | 68.2 | **4.03 %** | **-1.23 %** | **44.0 %** | **4.5 %** | **32.3 %** | 61.4 | +250 % |

`best 1 % of trades` is the share of the node's net profit produced by its best 1 % of trades.
Above 100 % means **the other 99 % collectively lose money.**

| node | hold p10 / p50 / p90 | re-entries | of those, below the last entry | max open at once | silent slots before | prints in the prior minute | buying share before entry | below the recent high |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| hot-tape re-entry | 2.5 / 19.9 / 138.3 s | **74.1 %** | 50.5 % | **25** | 1.0 | **110.5** | 53.9 % | 8.3 % |
| mid-tape one-shot | 4.1 / 22.7 / 87.9 s | 19.9 % | 49.5 % | 6 | 1.0 | 64.0 | 56.5 % | 4.8 % |
| instant launch | 3.1 / 13.9 / 80.1 s | **0.0 %** | - | 9 | 2.0 | 10.0 | **62.7 %** | **13.8 %** |
| quiet deep-age | 5.9 / **39.8** / **209.3** s | 56.6 % | 52.3 % | 9 | **3.5** | 10.0 | **43.9 %** | **-0.2 %** |
| deep-age big clip | 8.2 / 13.9 / 42.2 s | 6.9 % | 44.3 % | **3** | 2.0 | 61.0 | 53.9 % | 2.7 % |

### Three facts that hold across all five

**Every node has a negative median net trade**, from -1.23 % to -3.34 %. All five are convexity
harvesters carried by a tail, so a take-profit inverts every one of them. This is the standing
law of [_!___strategy.md](_!___strategy.md) 5.2, re-measured on a population with the machines
removed - it survives the correction.

**Four of the five are extreme lotteries.** Their best 1 % of trades produce 112 % to 181 % of
all net profit, and their best 10 % produce 295 % to 618 %. `deep-age big clip` is the single
exception at 32.3 % and 133.9 %, and it is also the smallest, slowest and highest-margin node.

**Hold is reactive, not a timer, in every node.** `p90 / p50` runs 3.0x to 5.3x, so no node exits
on a clock. The widest is `quiet deep-age` at 5.3x, the tightest `deep-age big clip` at 3.0x.

---

## 3. The nodes

### 3.1 Hot-tape re-entry - 6 wallets, 938.2 SOL

> *"The crowd is here and the cohort is still in. This flush is impatience, not the end, and the
> next wave restores the price."*

The busiest tape in the set: **110 prints in the minute before entry**, entry at reserve 55.5
with +329 % headroom, on a coin already about 200 s old. **74.1 % of its entries are re-entries**
on a coin it has traded before, and it holds up to **25 positions at once** - the widest book
here.

The re-entry is not averaging down. Of those re-entries, **50.5 % are below the previous entry
reserve** - a coin flip. They take the next flush wherever it falls rather than adding to a
losing position, which is what separates this node from a martingale.

Its book is the most tail-dependent of the five: the best 1 % of its trades produce **180.6 %**
of its net, the best 10 % produce 618 %. Median trade -2.07 %, and 13.6 % of trades lose more
than 20 %.

| # | wallet | t | margin | median trade | won | lost > 20 % | net SOL | trades | age s | hold s | clip | reserve |
| ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | `AbQcLH` | 7.64 | 2.03 % | -0.70 % | 47.8 % | 9.9 % | 129.8 | 10,171 | 176 | 19 | 0.49 | 49 |
| 2 | `64hP` | 7.06 | 1.18 % | -2.71 % | 42.9 % | 19.4 % | 488.8 | 42,034 | 107 | 24 | 0.95 | 51 |
| 3 | `omego` | 5.75 | 0.76 % | -0.52 % | 48.0 % | 10.6 % | 157.5 | 28,725 | 370 | 15 | 0.62 | 64 |
| 6 | `sssssw` | 5.29 | 0.68 % | -3.27 % | 37.8 % | 9.1 % | 61.1 | 38,866 | 341 | 20 | 0.22 | 56 |
| 9 | `8fStGV` | 4.76 | 1.76 % | **+12.02 %** | **68.3 %** | **23.2 %** | 16.8 | 4,876 | 99 | 9 | 0.10 | 72 |
| 14 | `49uohd` | 4.10 | 1.26 % | -2.50 % | 42.4 % | 14.6 % | 84.1 | 9,267 | 170 | 25 | 0.49 | 48 |

**`8fStGV` is the wrong shape for this node and should be read separately.** It wins 68 % of its
trades at a median of +12 %, and loses more than 20 % on 23 % of them - the inverse of everyone
else here, who lose small constantly and are carried by rare large winners. It also enters at
reserve 71.5, only +160 % headroom, on the densest tape in the roster, with the smallest clip
(0.10 SOL). It sells small pops and occasionally gets caught, which is a scalper, not a
harvester.

### 3.2 Mid-tape one-shot - 7 wallets, 427.9 SOL

> *"This up-move is starting, and I want one ticket on it."*

Enters at reserve 42.4 - **+634 % headroom**, the second deepest - about a minute into the coin's
life, on a moderate tape of 64 prints. Takes essentially **one shot per coin** (19.9 % re-entries)
and watches few coins at a time (max 6 open).

It is the harshest node to run: the **worst median trade at -3.34 %** and the **highest rate of
trades losing more than 20 %, at 15.9 %**, against a book carried by a best-1 % share of 122.9 %.
It buys close to the recent high - 4.8 % below it - into a tape that is 56.5 % buying, which is to
say into strength.

**This node is open, and the missing term is the door.** Fired on the burst-start print - the
router or terminal buy of 0.5-1 SOL that opens the burst - and filled at 115 ms on both legs, it
books **+2.6 to +6.7 % a trade on every exit family** on **8dtx's coins**, and
**-1.5 to -13 %** on the full tape with no door ([_!___evidence.md](_!___evidence.md) 5.4). That
ceiling does not transfer to the two largest books: on `9999hu` / `88887Q` coins at age >= 15 s
the same seat is +0.39 %/trade or red, and neither wallet names burst START (lift 1.02 / 1.14).
Create cgroup include is the market (concentration 0.87-1.16). Their prints stay out; their mint
list is never a door ([_!___evidence.md](_!___evidence.md) 7, mid-tape rows). `8dtx` and `3Xk2` run durable-nonce
racer builds: what is closed is copying their fill, not the decision they make. `3Xk2Eu` has no
prints on the last-leg tape. `9Uq8GV` names the event (buy >= 1 lift 4.75) and leftover on their
coins is +4.12 %/trade at clock 45; public doors besides slow-wall are red. `8aaRWu` names a
structure restart on every-leg study (`structure_burst` lift 4.84 @ 25-50 ms; priced burst_start
5.95) and leftover behind a 115 ms fill PASSES (parent union cost 1.20 %, peak +8.84 %); 6.1
first terms are mvk >= 1.29 %, nstruct >= 3, sell_run <= 0; 6.2 occupancy of those terms is red
0/6 (best mvk -1.51 %); next is 7.1 (occupancy red and leftover green is the door test, not
phase 8). E is this print; re-entry is R.
`ApfmkS` is volume manufacture, not a reader: `InitUserVolumeAccumulator` and
bundled `TransferChecked` are unique to it in the 26, tape share is 0.45 % of SOL, and 5.1 is
flat; it is not the next instrument. All seven members are measured
([_!___evidence.md](_!___evidence.md) 7, mid-tape rows). Multi-trade is 14-26 % of their mints; the
close-to-reopen gap is 56 s p50. The four unpriced facts at episode open are the market
and do not fill D ([_!___evidence.md](_!___evidence.md) 7, mid-tape episodes row). Token remaining (holder
book) does not fill D either ([_!___evidence.md](_!___evidence.md) 7, mid-tape holder book row).

| # | wallet | t | margin | median trade | won | lost > 20 % | net SOL | trades | age s | hold s | clip | reserve |
| ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 12 | `8dtx` | 4.49 | 2.65 % | -4.33 % | 27.9 % | 2.6 % | 86.4 | 4,429 | 126 | 17 | 0.65 | 41 |
| 13 | `88887Q` | 4.25 | 1.52 % | -2.50 % | 43.4 % | 13.2 % | 119.1 | 6,484 | 49 | 23 | 1.20 | 39 |
| 20 | `3Xk2` | 3.52 | 3.03 % | **-7.50 %** | 38.9 % | **27.7 %** | 51.5 | 2,384 | 45 | 49 | 0.72 | 44 |
| 23 | `9999hu` | 3.02 | 1.33 % | -4.36 % | 42.0 % | 23.5 % | 130.2 | 7,510 | 22 | 26 | 1.23 | 42 |
| 24 | `8aaRWu` | 2.81 | 4.20 % | +1.26 % | 52.0 % | 21.7 % | 12.1 | 941 | 76 | 23 | 0.28 | 51 |
| 25 | `9Uq8GV` | 2.62 | 1.39 % | -1.76 % | 44.8 % | 11.3 % | 15.6 | 1,929 | 130 | 16 | 0.59 | 52 |
| 26 | `ApfmkS` | 2.41 | 1.72 % | +1.72 % | 55.3 % | 8.1 % | 13.0 | 704 | 159 | 9 | 0.95 | 53 |

### 3.3 Instant launch - 6 wallets, 205.8 SOL, and it is really two nodes

> *"Snipers and retail chase every launch for a few seconds."*

Entry at reserve **32.2** against a curve that opens at 30: essentially the floor, **+1,176 %
headroom**, the most of any node. Ten prints on the tape, two silent slots before, **zero
re-entry** - one shot and move on.

The node splits cleanly in two, and the split shows in one column.

**The floor lottery - `ocBBRK`, `BYXYdw`, `ArAqZH`.** Median trade **exactly -2.50 %**, which is
precisely the two-leg fee at 125 bps a leg, and **0.0 % of their trades lose more than 20 %**.
Their median trade therefore sees **no price movement at all**: they buy at the curve floor, hold
60 to 120 seconds, and most of the time nothing happens and they pay only the toll. Win rates of
13.7 %, 14.8 % and 15.7 % confirm it. The entire book is the 1.2 % to 2.4 % of trades that gain
more than 50 %. It is a pure lottery ticket, and the reason the downside is zero is structural: at
the floor there is nothing left to fall.

**The launch flip - `97jv9p`, `ADkquS`, `4HgMCR`.** Holds 3 to 10 seconds, wins 38 % to 59 % of
trades, and carries a real 19 % to 20 % rate of losses beyond 20 %, `4HgMCR` excepted at 0.9 %.
`97jv9p` posts a **+6.72 % median trade**, the second best in the whole set.

| # | wallet | t | margin | median trade | won | lost > 20 % | net SOL | trades | age s | hold s | clip | reserve | shape |
| ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 5 | `97jv9p` | 5.32 | 1.76 % | **+6.72 %** | 58.6 % | 19.1 % | 33.5 | 4,633 | 7 | 10 | 0.40 | 39 | flip |
| 10 | `ADkquS` | 4.62 | 2.32 % | -2.18 % | 45.2 % | 20.3 % | 32.4 | 4,043 | 6 | 10 | 0.35 | 39 | flip |
| 11 | `ocBBRK` | 4.52 | 0.88 % | -2.50 % | 13.7 % | **0.0 %** | 40.5 | 5,196 | 11 | 60 | 0.89 | 31 | lottery |
| 16 | `BYXYdw` | 3.91 | 1.26 % | -2.50 % | 14.8 % | **0.0 %** | 47.6 | 1,914 | 8 | 75 | 1.98 | 32 | lottery |
| 21 | `ArAqZH` | 3.07 | 1.28 % | -2.50 % | 15.7 % | **0.0 %** | 47.9 | 1,893 | 7 | 120 | 1.98 | 32 | lottery |
| 22 | `4HgMCR` | 3.07 | 1.10 % | -1.50 % | 38.2 % | 0.9 % | 3.8 | 1,181 | 3 | 4 | 0.35 | 32 | flip |

Both halves are **closed to us by the seat**: entry inside 11 seconds of creation is the launch
reactor node, consumed inside about two slots ([_!___strategy.md](_!___strategy.md) 5.2).

### 3.4 Quiet deep-age - 4 wallets, 110.3 SOL

> *"The machine that pushes this old coin has restarted. Someone decided to spend again."*

The oldest coins in the set by a wide margin - median entry age **1,091 s**, about eighteen
minutes - on the quietest tape: **10 prints** in the prior minute and **3.5 silent slots**
immediately before entry, the longest gap of any node. That gap is the campaign-rider signature.

Two columns make the logic concrete. It buys at **-0.2 % below the recent high**, that is, at or
slightly *above* it - into strength, not into a dip - while only **43.9 % of the SOL** traded just
before entry is buying. On a near-silent tape a single modest buy sets a new high, so "at the
high" and "hardly anything is trading" are the same state, and that state is what they act on.

Downside is the smallest of any node with real activity: only **5.9 % of trades lose more than
20 %**, because an old coin on a dead tape has little left to give up. Holds are the longest,
p50 39.8 s and p90 209.3 s.

| # | wallet | t | margin | median trade | won | lost > 20 % | net SOL | trades | age s | hold s | clip | reserve |
| ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 4 | `GZmUDs` | 5.43 | 2.05 % | -2.41 % | 41.5 % | 2.1 % | 39.6 | 2,207 | 232 | 15 | 0.88 | 41 |
| 7 | `GjKjTm` | 5.06 | 3.68 % | -3.62 % | 41.3 % | 19.7 % | 13.2 | 3,633 | 1,041 | 145 | 0.10 | 52 |
| 18 | `86ugEi` | 3.74 | 0.56 % | -2.27 % | 35.8 % | 1.3 % | 25.6 | 8,365 | 1,382 | 23 | 0.54 | 51 |
| 19 | `7Q6RcQ` | 3.59 | 1.05 % | -2.74 % | 37.5 % | 5.5 % | 31.9 | 6,405 | 1,083 | 52 | 0.47 | 46 |

The public leftover of this node at 115 ms is **red on the doors on this tape**: the print they
follow is the first size buy after token silence, that burst lasts ~80 ms, and a 115 ms fill
lands after it ([_!___evidence.md](_!___evidence.md) 7, the C5 rows). The slow-wall door against this
node's event is the one cell still unrun.

### 3.5 Deep-age big clip - 3 wallets, 68.2 SOL, and the only node that is not a lottery

> *"Few coins are worth a real position. This is one, so size it."*

The smallest book here by trade count - **1,671 trades** against 133,939 for the hot-tape node -
and the best on every quality measure: **margin 4.03 %**, **win rate 44.0 %**, **median trade
-1.23 %** (the least negative), and only **4.5 % of trades losing more than 20 %**.

Its distinguishing property is the tail. Its best 1 % of trades produce **32.3 %** of its net and
its best 10 % produce 133.9 %, against 112-181 % and 295-618 % everywhere else. **It is the only
node whose ordinary trades pay for themselves.** It also runs the tightest book: at most **3
positions at once**, and the largest clips at 1.00 SOL median.

It buys deep into a coin's life at reserve 61.4, which leaves the **least headroom of any node,
+250 %**. That is the trade it makes: less upside per ticket, far less variance, and a real
position behind each one.

| # | wallet | t | margin | median trade | won | lost > 20 % | net SOL | trades | age s | hold s | clip | reserve |
| ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 8 | `D9Uite` | 4.77 | 5.78 % | **+0.75 %** | **52.8 %** | 8.0 % | 36.4 | 411 | 236 | 20 | 1.48 | 57 |
| 15 | `pau23U` | 3.97 | 1.79 % | -1.60 % | 40.5 % | 2.6 % | 15.5 | 1,049 | 336 | 12 | 0.49 | 65 |
| 17 | `9RNZnq` | 3.90 | 8.25 % | -1.66 % | 44.1 % | 7.1 % | 16.3 | 211 | 274 | 34 | 1.00 | 50 |

**Read the size of this node before reading its quality.** 1,671 trades across three wallets is
the thinnest evidence in the file, and `9RNZnq`'s headline 8.25 % rests on 211 trades with a 90 %
interval of 4.90 to 11.89. The shape is interesting; the level is not established.

The public leftover of this node at 115 ms is **red**: a size buy on a live mid-life tape
is not his tell (response equals the base), and he starts the burst himself half the time
([_!___evidence.md](_!___evidence.md) 7, the G1 row). The on-tape remainder is which coin (C6).

---

## 4. Agreement on coins rises steeply with entry age

Co-selection lift between two of the 26 is not flat across the set. Against the geometric mean of
the pair's entry age, Spearman is **0.794** on 325 pairs (`p = 7e-72`):

| both enter at | pairs | median lift | pairs at lift >= 2 |
| --- | ---: | ---: | ---: |
| under 30 s | 60 | 0.17 | 0.0 % |
| 30-120 s | 125 | 0.96 | 17.6 % |
| 120-400 s | 108 | 2.26 | 71.3 % |
| over 400 s | 32 | 3.50 | 96.9 % |

Under 30 s, independent professionals **anti-select** each other: a median lift of 0.17 means they
land on the same coin far less often than chance. That is the `instant launch` node, and it says
the coin choice there is close to arbitrary between them - consistent with its book being a
lottery on a floor entry. Over 400 s, which is `quiet deep-age` and `deep-age big clip` territory,
97 % of pairs agree.

So a coin's identity carries a shared signal a professional can read only once the coin has some
life in it. This is consistent with the standing token-level result and points the same way: the
thing worth detecting is which coin, not which moment.

**The size of the effect is confounded and the residual is not measured here.** The pool of coins
alive at 400 s is far smaller and far more selected than the pool born; the popularity-weighted
null absorbs part of that and not all of it. Read the direction and the ordering, not the levels.

---

## 5. What this set is for

A trader here is an **instrument**, never a template. He marks which coin, at which age, a
professional judges worth buying - and every margin in this file sits at *his* fill, 115 ms better
than ours.

Reachability differs by node, and the seat decides it, not the margin:

| node | open to us? | why |
| --- | --- | --- |
| instant launch | **no** | entry inside 11 s of creation; consumed in about two slots |
| mid-tape one-shot | **open, and the door is found** | behind the slow-wall launch door with the permission the burst-start print books **+6.97 % a trade** on 986 trades and clears the client gate at 95.9 %, against -3.33 behind the best public door ([_!___evidence.md](_!___evidence.md) 6.4). It fails the tail and the per-day ticket floor - 16/177/210/24/28/9 a day, over fifty twice in six (4.8) |
| hot-tape re-entry | **red here** at lag_115 | terms that separate their buys from the tape at lift 2.0-2.2 capture a **+0.29 %** price move, where this node's 1.10 % NET margin implies **+3.65 %** (evidence 7, the H1 row). At a zero fee the cell still loses. **`margin` in this file is net of the 125 bps fee** (`rb-solo-nodes.py` 140), so convert before comparing: `move = (margin + 2.5)/0.9875`. The lag does NOT pay on the buy leg on an aged coin: mean -0.66 %, cheaper than the decision print only 22.5 % of the time - a direction factor is a property of tape DENSITY, not of a side |
| quiet deep-age | **red here** at lag_115 | the burst they follow lasts ~80 ms, so the fill is after it. Campaign-break v0 is a different sentence and is red too at 115 ms (evidence 7) |
| deep-age big clip | **red here** at lag_115 | a public size print is not the tell (response = base). He starts the burst half the time (evidence 7) |

**What the 26 are for, measured (C6).** Their creation-sequence set is **not** a door: 143
sequences covering 95.3 % of coins and 97.3 % of prints, concentration 1.02, and -23.07 SOL
when traded as one. What they carry is **agreement** - how many of them are already in a coin
at our decision print - and that is the strongest single term this program has on the **loss**
axis: a 0.37 lift on the -50 % rate against 0.82 for activity-matched random wallets and 0.98
for the ten roster wallets that failed the profit cut, holding outside the window the roster
was fitted on ([_!___evidence.md](_!___evidence.md) 6.8). **It never enters a sentence** - naming their
coins is wallet identity and their mint list is not a gate - and it never gates an entry, since
firing after they land is -10.4 % a trade. It is a thermometer: it says the L axis is real, and
an ix-structure twin has to reproduce it.

The usable question is the one section 4 opens: among these 26, a pair that agrees on a coin is
two genuinely independent opinions, which a machine's sixteen addresses can never be. Whether that
agreement predicts anything forward is **unmeasured**.

## 6. What it does not claim

Membership is selection - the population is picked for being profitable - so levels are
tautological and only ordering carries. `t >= 2` is not a reader: measure tape share and the ix
book before FIND E (section 1; derive 4.0). No entry in this file is priced at `lag_115`, none has a
holdout, and the five nodes are descriptions of habit confirmed by outcome, not candidate rules. A
node becomes a rule only through the full-tape test in [_!___strategy.md](_!___strategy.md) 7.4
and Phase 5.

The node boundaries are drawn on habit fields and confirmed by the outcome and context columns;
they are not a clustering with a stability test behind them. Two of the five contain a member
whose shape argues it belongs elsewhere - `8fStGV` in hot-tape re-entry, and the flip-versus-
lottery split inside instant launch - and both are named above rather than smoothed away.
