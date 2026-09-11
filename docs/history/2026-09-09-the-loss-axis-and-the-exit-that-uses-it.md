# 2026-09-09: the loss axis, the agreement term, and the first exit that holds W while L falls

The second half of the same day as
[the-client-is-the-unit](2026-09-09-the-client-is-the-unit.md). That run left three things
open: agreement among the solo 26 had never been measured forward, the exit lab had never had a
selected entry to run on, and thirty days of prints was named as the binding constraint without
anyone checking whether thirty days exist.

## Thirty days do not exist

| store | span |
| --- | --- |
| `aa.pxf` | 08-30 17:48 .. 09-06 12:00 - the study week, and it *is* the tape |
| PG `trades` | **09-01 .. now**, 9 chunks, against a `drop_after 30 days` policy |
| `hunter/lake-data/trades/dt=*` | **09-01 .. 09-08**, 8 sealed days, 3.9 GB |

The retention policy is intact, so the 09-01 floor is a purge, not a prune - the same edge as
the fee-decode cutover. On 09-02 `min(block_time)` was 08-03; a week later it is 09-01.

**So the longer tape can only be accumulated, never exported.** It reaches thirty days around
2026-09-30, and the queue's C7 became a calendar item. The lake is the only durable copy and
its export is run by hand - 09-07 and 09-08 were both sealed late, on 09-09 - so a day missed
before PG's 30-day drop is a client lost for good.

## The user's door idea, measured and refuted

The proposal was to filter the market by the creation `ix_labels` groups the solo 26 trade, and
treat that as a door. Measured: they touch **143 distinct creation sequences**, and those cover
**95.3 % of tape coins and 97.3 % of prints**. Their own coins sit inside it 97.3 % of the time
against a pool of 95.3 % - **concentration 1.02**. Node by node, 86-95 % and 1.03-1.13. As a
door on the burst-start event it books **-23.07 SOL over 3,654 trades**, 65 clients, bootstrap
13.4 %.

**The creation structures these traders trade are the market, not a filter**, and 95.2 % of
slow-wall door coins were already inside it. It survives only as a weak exclude - it removes
4.7 % of coins.

## The second half of the idea is the strongest L-term in the program

Counting how many of the 26 independently bought the coin *before* our decision print, read
inside a reserve band so every trade in the comparison can actually reach -50 %:

| band | base | A >= 1 | A >= 2 | A >= 3 |
| --- | ---: | ---: | ---: | ---: |
| 42.43-50 | 36.05 % | 13.73 % | 10.79 % | **6.82 %** |
| 50-70 | 29.83 % | 13.32 % | 9.35 % | - |
| 70+ | 37.30 % | 9.09 % | - | - |

Three controls, because the 26 were selected for profit on a window that overlaps this tape:

* **activity-matched random wallets**, 20 draws: lift **0.82**. The null is not 1.00 - "anyone
  bought this coin before us" is already worth about 20 % off the -50 % rate, and that part is
  not judgement.
* **the ten roster wallets that failed the `t >= 2` profit cut**: lift **0.98**, inside the
  null's spread. Nothing.
* **outside the selection window.** The roster was fitted on 08-27..09-03. On 09-04 onward the
  solo lift is **0.37 and 0.18** - unchanged - while rejected stays at 0.98 and random at 0.82.

It stacks with the bundle term found earlier the same day: `bundle < 0.20 AND A >= 1` reads an
**8.77 %** L-rate and **+3.86 % a trade**, on a population that is -14.62 % a trade.

Agreement is a gate on the loss only. Firing *after* they land is still -10.4 % a trade,
because their impact is already in the price.

## The exit lab, on the first entry that clears the client gate

D/E/P fixed at slow-wall door + burst start + permission + two of the 26 already in; only X
moved. The static abort grid was not rerun. The cuts came from the sentence's own failure
modes, each read alone, under a trail, and in a **loss-only** form that can fire only while
price is below the fill.

**The answer is yes.** The arrivals-stall cut takes `L` from **31.5 to 10.1** while `W` holds
at **101.9** against 103.4, and break-even from **23.4 % to 9.0 %**. The static loss cap could
never do this - it cuts `L` 60 % and `W` 75 %. The basis says this book breaks even at
`L <= 16.3` holding `W` fixed; three state cells land under it.

**The loss-only condition is the entire result.** The same cuts allowed to fire above the fill:
burst-dies **-14.40 SOL**, flow-reverses **-15.77**, pusher-sells **-19.80**, with `W`
collapsing from about 100 to 8-14. A cut that can fire while the position is ahead is not a
cut, it is a clock. It has to be keyed to the **fill** price, not the decision price - they
differ at 115 ms, and the first pass keyed it wrongly and read too favourably.

**And a better distribution is not more money.** Cutting early frees the position, so 867
trades become 1,940; 1,073 extra round trips at 3.2-4 % is about 7.7 SOL of toll against 4.3
earned back, and the book falls 13.03 to 9.59.

What won on the gate that decides now was the **shipped armed trail** - arm +21 %, trail 36 %,
unarmed stop -43.75 %, cap 1200 s, the reserve 10/20/-25 squared: **+10.98 SOL, 6 of 6 days,
worst day +0.09, hold +0.92, top client 66.0 %, leave-one-out +3.74, bootstrap 94.5 %** against
a 95 % bar. It is two-regime by construction, which is the shape the story asks for.

## Where that leaves the sentence

```
D  slow-wall launch door, previous day 20+ launches, slow-wall rate >= 5 %, not bundler
E  burst START: router or Terminal buy >= 0.5 SOL, first print of that tool's run after
   >= 2 slots of that tool quiet, not a racer
P  age 60-900 s . vsol 33-81 . creator has not sold . >= 2 of the solo 26 already in
X  arm +21 % / trail 36 % / unarmed stop -43.75 % / cap 1200 s
R  one position per token   S  0.2 SOL   seat lag_115
```

850 trades, +10.98 SOL, +6.46 % a trade, 53.9 first-per-mint a day, positive on every one of
six days. It clears the floor, the money, the walk-forward and leave-one-client-out. It misses
the tail gate and sits at 94.5 % on the client bootstrap against 95 %, on 16 clients.

It is frozen as written and it waits for 2026-09-30, when the tape holds thirty days and about
150 clients. Nothing else is added to it in the meantime.

## Correction, same day, after the implementation audit

The sentence above was pointed at every recorded cause of a study positive dying on
implementation. Six checks passed and **two failed, both of them mine**
([_!___evidence.md](../../hunter/docs/plans/strategies/_!___evidence.md) 4.8).

**Passed.** Timestamps are real milliseconds (8 distinct per 9-print slot, 205 ms span), so
`lag_115` is a genuine 115 ms model. The door's `runners` column is the slow wall exactly -
curve peak >= 60 SOL and peak >= 60 s after birth, previous UTC day, the same
`launch_build_day_stats` the engine stamps. `aa.pxf` is the full curve tape: 98,338 mints in
both it and `trades` on the shared window, rows within 0.04 %. The seat decays gently - four
times worse on both legs still books +8.71 against +10.98, and zero lag is 1.98x, so the fill
model costs half the book and is honestly charged. Concurrency peaks at ten positions and
2.0 SOL, so the engine's cap will not bite.

**Failed 1: the ticket floor is a per-day refusal and I reported a mean.** The cell's tickets
run **16, 177, 210, 24, 28, 9** a day - over fifty on **two of six days**, reported as "53.9 a
day, floor ok". The two big days are exactly the two days the single carrying client was alive,
so the mean **launders the client concentration through the gate**: two failures cancelling
into an apparent pass.

**Failed 2: `A >= 2` is a wallet-identity gate.** "At least two of these 26 named wallets
already bought this coin" is the readers' mint list used as a gate, refused by the super-root
`CLAUDE.md`, by the strategy file twice, and by the workflow's own refusal list. I put it in the
frozen sentence. The agreement measurement stands as a finding; the term comes out.

**Removing it made the sentence better**, which settles it: +10.98 -> **+13.75 SOL**, top client
66 % -> **64 %**, build bootstrap 94.5 % -> **95.9 %** - the first cell in this program to clear
that bar.

**And the floor cannot be fixed by dropping a term.** The creator permission is the term that
makes the money *and* removes the tickets: drop it and the floor clears 6 of 6 days while the
book is **-17.26 SOL**; drop the age/reserve band instead and tickets clear 7 of 7 while
leave-one-client-out is **-17.37**. On this tape the money and the tickets are the same two
days, so there is **no configuration with both** - which is a sharper statement than "short of
clients". There is not yet a sentence to prove.

The corrected frozen sentence carries no wallet term, clears money and the client gate, and
fails the tail gate and the per-day floor. It still waits for 09-30.
