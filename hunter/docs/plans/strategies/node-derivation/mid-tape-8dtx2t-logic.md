# 8dtx2t: its entry and exit as plain reasoning

What 8dtx2t buys and sells on, and the likely human reason behind each part. Every number
names the chain row of [mid-tape-rule-3.md](mid-tape-rule-3.md) it comes from, or says it has
none. The numbers are measured; each "why" is an inference from them. Study fires run
09-01 .. 09-06 12:00, the holdout 09-06 12:00 .. 09-10. Price grows as vsol squared: launch
vsol 30, the curve completes (migration) at vsol 115.

## Who he is

- **A bot, not a person clicking.** His buy lands a median 75 ms after the print he answers,
  his sell a median 51 ms after the exit print (scratch measure, no chain row).
- **Many small tickets, cut fast, ride the rare winner.** 2,306 positions, 419 a day, median
  hold 16.1 s, his close +23.79 SOL on 6/6 days (row "3-4 8dtx2t all"). Most trades lose a
  little; the top 1 % carries most of the money (14.47 SOL of it, row "exit big winners 8dtx2t all").

## Entry: "a sleeping coin just got woken by a real buyer"

**The event (E)** - all three at once:

1. **The coin is quiet:** no print for >= 0.4 s before the buy.
2. **The buy moves the price >= 1.90 %:** a real size, not dust.
3. **The buyer is new to the coin:** its first trade on it.

E holds 4.65 % of his buys against a 0.70 % base, and covers 61.6 % of his positions
(workflow "Next on 8dtx2t").

**Why he expects a rise:** a quiet coin plus a fresh buyer spending real money is a new
decision, not churn or a holder adding. A green jump after silence is visible, so others
follow, and he wants in before them. The tape agrees: fires that 2+ other prints answer
inside 83 ms book +8.23 % at the fire's own price; fires nobody answers book -1.41 % (row "E
fire race at our seat 8dtx2t all").

**Which wake-ups he takes (about 1 in 120).** His quiet picks against the other quiet fires,
same direction on fit and test days (row "E fire his quiet picks 8dtx2t all"):

| His picks (p50) | Others (p50) | Likely reason |
| --- | --- | --- |
| vsol 40.6 | 51.9 | Cheap: price about 1.9x launch, about 8x of room to vsol 115 |
| best vsol so far 47 | 64 | Never pumped: no buyers stuck at an old top waiting to sell into the rise |
| 50 SOL bought so far, 1.36 buys in 10 s | 114 SOL, 3.43 buys | Early, not late into a crowded move |
| holders never sold 0.87, top holder 0.15 | 0.79, 0.10 | Little selling pressure |
| buyer wallet up on its other coins 0.49 | 0.38 | Follows buyers that win elsewhere (see below) |
| bundled launch share 0.008, first-slot buy 4.9 SOL | 0.12, 8.4 SOL | Avoids insider launches that dump |
| copycat names before 1 | 3 | Avoids copycat coins |

**What we cannot reach:** his quiet picks book +3.04 % a trade at our seat, yet no filter shaped
like them pays: every cut and tree on those facts is red on 0/6 days (row "E fire his quiet
picks 8dtx2t all"). His own buy is not the reason they pay. Rebooked on the coin as it reads
without his prints, they still book +2.30 % (row "E fire his own push removed 8dtx2t all"), and
the gap to +3.04 % is inside what resampling the same trades produces. What the two books share
is that 3 trades of 296 carry them: the other 293 lose either way, where his own close leaves
+9.32 SOL under its top 1 %. So at our seat his picks have no body to copy, and the open
question is not his push but how he finds the 1 wake-up in 120 that runs.

The one row of that table tested on its own confirms it. "The buyer wins on its other coins"
tells his pick (AUC 0.61 on the fit days, 0.63 on the test days) and tells a trade that pays
0.49 and 0.49, which is chance. It marks a fire the crowd answers fast: tighten it and the price
move inside our 83 ms window grows from 3.05 to 5.17 %, which pays +3.38 % a trade at the fire's
own price and -4.00 % at ours. Every cut on it books -3.3 to -4.2 % on 0/6 days, and the fires
whose buyer has no record at all are the least bad (row "E fire the buyer's record 8dtx2t all").
A fact that separates his picks is not therefore a fact that makes money.

## Exit: "prove it fast, or I am out; if it works, give it room"

Four rules; the first one crossed sells (workflow "Next on 8dtx2t"):

| Rule | Likely reason | Example |
| --- | --- | --- |
| **Early cut:** held >= 5 s AND >= 5 % under the fill | A wake-up shows at once; a fast drop means nobody follows | Buy 1.00, at 5 s 0.95: sell |
| **Static abort:** held >= 10 s AND under the fill | 10 s is enough; no follow-through means the idea failed | Buy 1.00, at 10 s 0.99: sell |
| **Curved trail:** fall from the best >= max(10 %, 5.88 x best^0.3) | Bigger runs swing harder; a tight stop throws out the rare winner that pays for everything | Best +10 %: may fall 11.7 %. +50 %: 19.0 %. +100 %: 23.4 %. +300 %: 32.5 % |
| **Sell before the wall:** vsol >= 110 | The curve ends at 115, where the price jumps and early holders dump | Coin reaches vsol 110: sell all |

The four book 22.78 SOL against his 23.79 on the study, and 15.98 against his 15.91 on the
holdout, top 1 % 7.87 against 7.78 (row "exit holdout 8dtx2t all"). The exit is read.

Red on his exit: a sell threshold, silence, a buyer arriving, a price target, every shape of
the fall after the top. He sometimes holds a dip past the trail while buyers keep arriving and
the top is young, but no rule built from that moves the book (row "exit hold past the curve
8dtx2t all").

## The whole logic in one line

He buys the first real buyer to wake a cheap, clean, quiet coin; cuts it within 5-10 s if
nobody follows; rides a run with a stop that loosens as the gain grows; sells before the curve
ends.

## Open

1. **How he picks 1 wake-up in 120.** The table shows what his picks look like, yet those
   filters lose at our seat.
2. **Whether wallets copy his buys.** His own push is not what his picks pay on, so a following
   crowd is what is left of "being himself".

Every row of the picks table that is booked on its own is red, the buyer's record included, so a
third open line needs a fact the table does not already carry.
