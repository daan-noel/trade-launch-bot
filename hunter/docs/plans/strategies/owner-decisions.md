# Owner decisions: when the owner pumps, holds, dumps and pumps again

**What this file does.** Once every trade is split into owner and outsider
([owner-split.md](owner-split.md)), this file is the method to find **why and when the owner
acts**: when it pumps, holds the price, rides, dumps, gives up, or pumps again. A trading rule is
then built from those decisions: buy when the owner commits, stay while it rides, sell just before
it dumps.

The method works for any launch group. Numbers measured so far are in section 6.

Numbers marked **start value** are first guesses, to be tuned on data.

---

## 1. The idea: each coin is a small business for the owner

- **It spends**: its own SOL to push the price up, and its time.
- **It earns**: outsiders' SOL. The owner's profit is what outsiders pay for its tokens.
- **Its risk**: outsiders holding a big bag can sell first and take the profit instead.

So the owner **keeps pumping while it expects outsiders to pay more than the pump costs, and dumps
when that stops**. Every reason in section 3 is one way to read that at a given moment.

The owner does not live on creator fees: its own trades are most of the coin's volume, so it pays
most of those fees itself.

## 2. The owner's decisions, and how each one shows on the chart

| decision | how we see it (start values) |
| --- | --- |
| commit | the owner puts 1 SOL or more into the coin within its first 5 s |
| pump step | owner buys push the price up 10 % or more |
| hold the band | the price stays within +-10 % for 20 s while the owner keeps buying and selling |
| one-shot dump | the price falls in one slot ([owner-split.md](owner-split.md), "fall") |
| waterfall dump | the price falls in several steps, less than 30 s apart |
| ride | the owner could cash out (its profit reached its usual level), but keeps buying and the price rises another 20 % or more |
| migrate | the coin completes its curve (on pump.fun, about 110 SOL in the pool) |
| give up | the owner dumps before its profit reaches its usual level |
| second pump | after a dump, the owner buys 1 SOL or more within 60 s and the price climbs 20 % or more from the bottom |

## 3. The owner's reasons

Each reason is something we can read at any moment of a coin, and the decision it should lead to.

**1. "I have made enough."** The owner dumps once its profit (what its tokens are worth now, minus
what it paid) reaches its usual cash-out amount.
*Example: on a 4.16 coin the owner usually cashes out at about 4.9 SOL of profit.*

**2. "Buyers are here - sell to them now."** When outsiders suddenly buy (1 SOL or more within 3 s)
and the owner is already near its profit level, it dumps into them.
*Example: owner profit is at 60 % of its usual level, then outsiders buy 1.4 SOL in 2 s: dump.*

**3. "Take the small crowd."** Early in the coin's life the owner dumps into a small outsider crowd
instead of waiting for its usual profit.
*Example: at age 13-16 s, outsiders buy 0.6 SOL over 10 s with the price at its peak and the owner's
profit at half its level: it dumps into them.*

**4. "Real buyers carry it - push it to the end."** If outsiders keep buying on their own after
the owner reached its profit level, it does not dump: it rides the coin up to migration, where the
reward is bigger.

**5. "Hold the price and wait."** The owner keeps the price flat in a band while waiting for the
next wave of buyers, then dumps into it.

**6. "My next coin needs me."** When the owner launches or starts pumping another coin, it dumps
this one.

**7. "Early holders are dangerous."** If outsiders bought a big bag early (or are already selling in
the first 10 s), they could dump on the owner, so it dumps first.

**8. "Sell while the pool can take my bag."** The bigger the owner's bag against the SOL in the
pool, the more its own sell crashes the price. It dumps while the pool can still absorb it.

**9. "Dump all at once, or bit by bit."** If buyers stop all at once, the owner dumps in one go. If
slow buyers keep trickling in, it sells a little into each one (a waterfall).

**10. "Is pumping still worth it?"** The owner watches how much outsider money each SOL of its
pumping brings in. When that falls, it stops pumping and dumps soon.
*Example: in the last 30 s the owner put in 2 SOL and outsiders 3 SOL (1.5 each); before, it was
3 per SOL: falling.*

**11. "The holders are gone - pumping again is safe."** After a dump, outsiders who held before
it sell their tokens. Once most of them have left, few outsiders are left who could sell into a
new pump, so the owner pumps again. If outsiders still hold a big bag, it does not pump again.
*Example: after the dump outsiders hold 2 % of the supply: second pump likely. They still hold
15 %: no second pump.*

**12. "Use what worked, launch when people are awake."** The owner reuses a coin name that worked
before, and launches more in the audience's hours (21-24 UTC).

**13. "I am working a session."** When the owner launches several coins close together, it is
actively working them; a lone coin is more often a test it drops.

**Not reasons** (measured, they do not hold):
- A fixed timer, a fixed price, a fixed budget, or a fixed amount of outsider money as the dump
  trigger.
- The owner's buys minus its sells over the last few seconds: the owner buys and sells at the same
  time, so this flips all the time, long before any dump.
- Creator fees as the owner's income (section 1).

## 4. How a reason is tested

1. **Write down every moment.** For each coin, one row per trade, with what each reason reads at
   that moment - using only the past, never a later trade.
2. **Mark the decisions** of section 2 on the same rows.
3. **Does the decision follow the reason?** A **chance** is the reason turning true (after being
   false for more than W seconds) while the owner still holds; a **hit** is the decision landing
   within W seconds after it. **Hit rate** = hits / chances; **cover** = decisions with a hit before
   them / all decisions. *Example: outsiders buying 1 SOL in 3 s turns true 622 times before 682
   cash-out dumps; 110 dumps follow within 3 s: hit rate 18 %, cover 16 %.*
4. **Does it pay as an exit?** Book it: buy at the first trade at age 1 s or more, sell when the
   reason fires, both fills 0.115 s after the decision, 0.03 SOL. A reason that reads the owner well
   but books no better than holding is not an exit.
5. **Is there time to act?** The gap between the reason and the decision must leave room for our
   order (we fill about 0.1 s later).
6. **Recent days first, old days to check.** Tune on the last ~2 weeks; it counts only if it also
   holds on the 2 weeks before.
7. **Look at the misses.** When the reason holds but the owner does not act, what happens next (a
   ride, a migration, a slow death)? That is where a rule loses money.

## 5. From reasons to a trading rule

| part of the rule | built from |
| --- | --- |
| which coins to trade | 12, 13, and the commit decision |
| when to buy | right after the owner commits, unless reason 7 holds |
| when to sell before a dump | 1, 2, 3, 10 |
| when to stay | 4 |
| how to sell in a waterfall | 9, read once the first step down lands |
| when to buy again | 11 |

Every line of a rule names the reason it reads.

**An exit is a sequence of stages, one per owner decision.** The owner does not run one trigger: it
cashes out early, or rides, then dumps into a burst, or carries the coin to migration. The exit
follows it stage by stage:

| stage | sell when | the owner's decision it reads |
| --- | --- | --- |
| open | its profit reaches its level, or 60 % of it while outsiders buy 1 SOL in 3 s, before age 20 s | 1 + 2: a quick cash-out |
| open -> ride | the same signal at age 20 s or later: do not sell, ride | 4: outside money carries the coin |
| ride, first 30 s | outsiders buy 2 SOL over 10 s | 2: it dumps into the burst |
| any stage | the pool reaches 110 SOL | migrate |
| any stage | the owner's dump instruction prints | it is out |
| any stage | 1,000 s after the buy | the slowest migrations |

A decision that looks the same as another one at the moment it starts has no stage: the exit pays
for it, and the entry has to avoid it (section 6, the early dump).

## 6. Measured so far

On 7ix (case file: [node-derivation/launch-group-7ix.md](node-derivation/launch-group-7ix.md)):

| reason | what the tape shows |
| --- | --- |
| 1 | the crew tag alone reads the owner's profit at its first dump as tight: on 4.16 coins 3.95 / 4.90 / 6.32 SOL (10 % / median / 75 %). On the full split it spreads: just before the cash-out dump (the owner-majority fall where it sells most), 4.16 coins 1.6 / 3.7 / 7.2 SOL (25 % / median / 75 %), the dev's own wallets 2.6 / 4.4 / 7.6; the dump's size is tighter (the owner sells 26 / 39 / 50 SOL) |
| 2 | outsiders buying 1 SOL or more in 3 s: hit rate 11.5 / 17.7 %, cover 15 / 16 % against the cash-out dump (W 3 s, lag median 1.8-2.0 s); as the only exit, +12.5 / +17.2 % a trade, 12 of 17 and 12 of 13 days |
| 3 | of the dev's dumps that land on a held trade, 62 % come before any signal: age 13-16 s (median), the dev's profit at half its level, outsiders buying 0.6 SOL over 10 s, the price at its peak. At that print, coins that dump and coins that ride read alike (age, outsider SOL, outsider buyers, pool, the dev's buying pace); every early-sell stage tried books 0.02-0.46 SOL below holding |
| 4 | coins that migrate reach the owner's profit level later (median age 34 s, against 7 s) |
| 5 | the price reaches its dump level 25-60 s before the dump, and holds there |
| 7 | outsiders selling more than 1.3 SOL by age 10 s: the trade books -8.8 / -24.5 % |
| 12 | reused name: +21 / +26 % a trade; born 21-24 UTC +34 / +29 %, against 11-15 UTC -9 / +10 % |
| 13 | no other launch in the last 10 min: -12.1 / -6.5 % a trade; four or more: +13.4 / +7.4 % |
| fixed triggers | a fixed time, price, budget or outsider amount starts the dump within 5 s on 19 % of coins at most |
| buys minus sells | selling when it turns negative: -1.8 .. -3.4 % a trade |

(Pairs like "+21 / +26" are the two periods of the case file.)

**Exit test on 7ix.** Buy at the first trade at age 1 s or more; both orders fill 0.115 s after the
decision; 0.03 SOL a trade; max_cost 0.13 / 0.65 / 4.16. The usual profit level was set on coins of
09-01 .. 09-14 (0.35 / 0.72 / 4.48 SOL). The owner here is the earlier, simpler split (the crew tag
plus wallet groups), not the full method of [owner-split.md](owner-split.md).

| sell when | 09-01 .. 14 | 09-15 .. 27 | days in profit |
| --- | ---: | ---: | --- |
| reasons 1 + 2: owner profit at 60 % of its level AND outsiders buy 1 SOL in 3 s | +20.9 % | +24.2 % | 13 of 14, 11 of 12 |
| reason 1 alone: owner profit at its level | +3.9 % | +6.6 % | 10 of 14, 10 of 12 |
| the owner stops buying while the price drops | -6.9 % | +12.7 % | fails: it comes after the crash |
| the owner's bag shrinks below its usual low | -4.1 % | -5.4 % | fails: the bag swings while the owner trades both ways |
| perfect exit (knowing the future) | +27.5 % | +29.0 % | |

**Staged exit on the full split** ([owner-split.md](owner-split.md)). The stages of section 5;
the owner's profit is its own wallets' (the creator, the create transaction, the creation-slot
buys, the program); the levels are its median profit at its first sell wave on 09-02 .. 14 (0.13
2.85, 0.65 1.48, 4.16 3.90 SOL); max_cost 0.13 / 0.65 / 4.16 with 44 SOL or more in the pool at the
buy; fills 0.115 s after the decision, 0.03 SOL.

| | 09-02 .. 14 | 09-15 .. 10-01 |
| --- | ---: | ---: |
| trades | 529 | 218 |
| % a trade / median, bursts of 1 SOL over 3 s and 2 SOL over 10 s | +16.7 / -46.7 | +30.0 / -3.6 |
| days in profit | 10 of 13 | 10 of 14 |
| bursts of 0.75 SOL over 3 s and 1.5 SOL over 10 s | +22.0, 13 of 13 days | +30.4, 12 of 14 days |
| bursts read as everyone but the dev's own wallets (the paid machines count), 1 / 2 SOL | +21.2, 13 of 13 days | +33.7, 12 of 14 days |
| the dev's profit replaced by the full owner's profit | +15.3 | +19.0 |
| plus an early stage (before 20 s, outsiders 0.75 SOL over 10 s, price at its peak) | +13.5, median -0.2, 12 of 13 days | +21.3, median +3.1, 12 of 14 days |

Exits on 09-02 .. 14: quick cash-out 84 at +13 %; ride burst 56 at +104 %; pool at 110 SOL 41 at
+404 %; the dev's dump instruction 344 at -46 %. The perfect exit books +107 % a trade on every coin
bought at age 1 s.

The burst sizes belong to the flow they read: real outsiders buy less than outsiders plus the paid
machines, so their sizes are smaller, and both readings book the same. In the engine, over 09-02 ..
10-01 with the engine's crew (no creation-slot buyers), the stored reading leads: BROAD +24.7 %
against +19.0 % a trade, FINAL +49.0 % against +41.0 %
([7ix-crew-rule-engine.md](../../roadmap/7ix-crew-rule-engine.md), section 4).

**Which coins.** The dev dumps early on about half its coins whatever the first second shows (65 /
53 % of door coins). A door raises the result by picking bigger rides, not fewer early dumps: 6 or
fewer prints by 1 s with a name an earlier 7ix coin used books +34.8 / +44.9 % a trade (165 / 105
coins), its early dumps 59 / 47 %. Outsiders already in for 0.5-1.5 SOL by 1 s book +2.7 / +11.0 %.

How the 7ix owner dumps: 41 % in one go (within 3 s), 39 % over 3-60 s, 19 % over more than 60 s.
Nothing measured so far warns of a waterfall before it starts.

On 6ix, buying at age 1 s loses even with the perfect exit (-8.3 / -6.7 %): the price does not
rise while the volume network trades the coin. There, the open question is when to buy, not when
to sell.
