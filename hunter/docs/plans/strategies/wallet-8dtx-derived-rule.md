# The 8dtx-derived rule: clean burst + quiet + old enough

Reference sheet for the rule reverse-engineered from wallet 8dtx. Plain logic first, then
every number behind it, then what is tunable and what is not. Method, and the mistakes that
produce earlier wrong answers:
[trigger-ix-derivation-method.md](trigger-ix-derivation-method.md).

Corpus: full universe 2026-08-01..08-21, 264,289 causal fires / 117,596 mints. Every number
below is on the **225,010 fires on tokens 8dtx never trades**, so nothing reads a hindsight
label. Net is after 1.25 % fee plus about 2 % round-trip price impact at 1 % of pool.

## 1. The pure logic

**Wait for a token everybody stops watching. Buy the instant real people start arriving, and
only if the arrivals are people rather than bots.**

1. **Old enough** - the token is alive a while, not a fresh launch.
2. **Quiet** - almost nothing is bought in the last ~12 seconds.
3. **A burst** - 2 or more buys land in one slot, totalling 1.2 to 10 SOL.
4. **The burst is clean** - EVERY buy in it comes from a named retail router (Trojan, Photon,
   Bloom, Terminal, Axiom) **and carries an ATA create**, which means that buyer has never
   held this token before.

The fourth term is a **whitelist, not a blacklist**, and it is a **conjunction**. Both
halves are load-bearing and neither works alone: the ATA flag without a named program pays
**+0.30 %**, the named program without the flag pays +6.68 %, and together they pay
**+7.51 %**. Excluding the obvious machinery marker instead - throwaway-account builds -
reads **+0.99 %**, because a build carrying no catalogued marker is a machine nobody has
catalogued yet, not a person. Every gate measured, and how the engine states it:
[ix-gate-metrics.md](ix-gate-metrics.md).

Then hold about 8 seconds and sell. No stop. No take-profit. No trail.

### Why it works

The money is not price, it is **the next humans who buy after you**.

- **Quiet** means the sellers are finished. Nobody is left to dump on you.
- **A burst after quiet** means somebody decides this token is worth buying, from a standing
  start, on their own.
- **A router buy is a person deciding** - new information, and more people are about to
  notice the same thing.
- **A throwaway-account buy is a bot reacting** - no new information. It sees what you see.
  Nothing follows it, so you buy its exit.

That distinction is the edge. A gate that counts a bot buy and a human buy as the same
1.2 SOL has no edge at all.

**And that is why there is no exit rule.** This is not risk management, it is catching an
arrival wave. 2.3 % of these trades average +147 % and carry 56 % of all profit; 8.3 %
average +58.6 % and carry 77 %. Every stop and every take-profit cuts exactly those.

## 2. What the baseline number means

**Baseline = the trigger with no quality filter at all.** Fire whenever, in one slot:

- 2 or more buys, from 2 or more distinct build groups
- running total 1.2 to 10 SOL, evaluated as the transactions arrive
- pool `vsol <= 42` entering the slot
- prior-30-slot buy flow `<= 15 SOL`

Buy at the price left by the transaction that completes the gate, hold 50 slots, sell at the
last print. Charge the full cost.

```
225,010 fires / 110,528 mints    net -2.00 % per trade    31.9 % of trades clear cost
```

**That is the number to beat.** It is the honest measure of detecting the trigger with no
filtering, and it loses money. Everything below is what turns it positive.

## 3. Results

| gate | n | net | w1 | w2 | w3 | w4 | win | SOL |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| baseline (no filter) | 225,010 | **-2.00** | -1.43 | -2.53 | -1.85 | -2.12 | 31.9 % | - |
| + clean burst | 21,389 | +0.58 | 0.59 | -1.65 | 0.81 | 2.74 | 36.7 % | 96.5 |
| + quiet | 5,061 | +3.97 | 5.26 | 4.42 | 1.93 | 4.32 | 43.3 % | 156.0 |
| + old enough | **3,229** | **+5.96** | 6.19 | 5.67 | 5.24 | 6.62 | 50.8 % | 149.5 |
| **+ ATA (THE RULE)** | **1,379** | **+7.51** | 7.0 | 7.1 | 10.0 | 6.5 | **52.7 %** | 80.4 |

2,950 distinct mints at the router-only gate, about 110 to 128 trades a day, weekly SOL
38.1 / 34.0 / 31.4 / 46.1 at a 0.777 SOL clip.

The last row is measured at a 1.5 SOL burst floor against a reconstructed router-only
control of **+6.68 %** on 2,368 fires - the same family as the +5.96 % row, which sits at
the looser 1.2 floor. ATA costs 42 % of the fires and a third of the total SOL to buy
+0.83 pp per trade. Collapsed to one trade per mint it reads **+7.62 %** over 1,334 mints,
52.4 % of them profitable, top 1 % holding 27.7 % of the profit.

**Fill sensitivity** (exit held constant, only the entry price varies):

| where you land | net |
| --- | --- |
| immediately behind the gate transaction | +6.18 |
| one buy later | +3.44 |
| dead last in the slot | **+0.58** |

Positive even at the worst in-slot fill. In 39.6 % of fires nothing lands behind the gate
transaction at all.

**Exit shape.** Plain timeout, n held constant: 5 slots +5.68, 15 +7.80, **20 +7.98**,
30 +7.91, 50 +7.71, 75 +7.67. A flat plateau from 15 to 50 slots.

Every stop and every take-profit is monotonically harmful: none +6.67, stop 0.85 +6.35,
0.90 +6.13, 0.95 +5.89; TP 1.60 +6.33, 1.30 +5.49, 1.15 +4.94. Trailing stops likewise:
none 6.67, trail 40 % 6.59, 25 % 6.29, 15 % 5.64.

**Concentration.** 2,950 mints, top 10 mints hold 14.8 % of profit, top 1 % of mints hold
31.0 %, and 50.5 % of mints are profitable. A broad edge, not a lottery.

## 4. What is tunable, and what is not

### Two smooth dials - trade rate against quality, freely

**Quiet threshold** (clean burst and age held):

| max prior-30-slot buy SOL | n | net | win | SOL |
| --- | --- | --- | --- | --- |
| 0.5 | 1,586 | 7.27 | 53.0 % | 89.6 |
| 1.0 | 1,956 | 7.06 | 53.3 % | 107.3 |
| 2.0 | 2,639 | 6.25 | 51.2 % | 128.1 |
| **3.0** | **3,229** | **5.96** | 50.8 % | 149.5 |
| 5.0 | 4,172 | 5.53 | 49.8 % | 179.4 |
| 8.0 | 5,167 | 4.88 | 48.2 % | 195.9 |
| 15.0 | 6,260 | 4.81 | 47.6 % | **234.2** |

**Age threshold** (clean burst and quiet held):

| min age | n | net | win | SOL |
| --- | --- | --- | --- | --- |
| 0 | 5,061 | 3.97 | 43.3 % | 156.0 |
| 25 slots (10 s) | 4,239 | 4.84 | 47.2 % | 159.4 |
| 50 slots (20 s) | 3,763 | 5.62 | 49.3 % | **164.3** |
| **75 slots (30 s)** | **3,229** | **5.96** | 50.8 % | 149.5 |
| 150 slots (60 s) | 2,282 | 6.68 | 52.9 % | 118.5 |
| 400 slots (160 s) | 1,156 | 7.30 | 55.4 % | 65.6 |
| 1000 slots | 516 | 6.61 | 56.8 % | 26.5 |

Both are monotone with no knee: **loosening buys trade count at a predictable cost per trade,
tightening buys quality at a predictable cost in volume.** Every row above is positive in all
four weeks. Total SOL peaks LOOSE (quiet 15, age 50); per-trade quality peaks TIGHT. Pick the
point that matches available capital and how many positions run at once.

### One cliff - do not tune this one

**Burst cleanliness** (quiet and age held):

| burst composition | n | net | win | SOL |
| --- | --- | --- | --- | --- |
| any burst | 58,148 | **-0.24** | 36.9 % | **-106.6** |
| 50 % or more router flow | 35,707 | +0.73 | 39.9 % | 201.5 |
| 80 % or more router flow | 16,559 | +1.60 | 43.0 % | 205.7 |
| **100 % router flow** | **3,313** | **+5.81** | 50.6 % | 149.7 |

Not a dial. Going from 80 % to 100 % multiplies net by 3.6x. **One bot transaction inside the
burst destroys it**, which is the mechanism restated as a measurement: the moment a machine is
in the burst, the burst is no longer evidence that people are arriving.

### Burst size - a floor, not a band

| burst SOL | n | net | win |
| --- | --- | --- | --- |
| 1.2 to 1.5 | 801 | 3.22 | 47.1 % |
| 1.5 to 2.0 | 911 | 7.26 | 51.9 % |
| 2.0 to 3.0 | 943 | 6.12 | 53.1 % |
| 3.0 to 5.0 | 495 | 6.61 | 49.3 % |
| 5.0 to 10.0 | 79 | 12.76 | 55.7 % |

Raising the floor from 1.2 to 1.5 SOL is worth about 3 points and costs a quarter of the
volume. The upper cap earns nothing on this corpus; large clean bursts are the best cells.

## 5. What does NOT improve it

**The creation fingerprint adds nothing, and inverts.** Inside the rule:

| creator initial buy | n | net |
| --- | --- | --- |
| 0.2 to 1.0 SOL | 308 | +3.23 |
| 1 to 2 SOL | 381 | +2.95 |
| **2 to 5 SOL** | **1,613** | **+6.82** |
| 5 SOL and above | 749 | +6.49 |

On the raw tape the 0.2-1.0 band pays +3.15 and the 2-5 band loses -3.40. Inside the rule the
ranking reverses. `cu_price` and the launch build signature behave the same way, with the best
cells being builds that lose on the open tape. **The launch screen and the moment screen are
substitutes, not complements** - the rule already selects tokens good enough to survive, go
quiet, and still attract real routers.

**First-on-mint adds almost nothing once ATA is required** - +0.36 pp, and on its own it
is *worse* than not having it (3.95 % against 5.91 %). ATA is the structural version of the
same idea and carries the value without per-token wallet state. Tables:
[ix-gate-metrics.md](ix-gate-metrics.md).

**The state axes are already priced in.** Inside the rule, `trail <= -15 %` reads +6.08,
`net30 <= 0` reads +5.92 and `vsol <= 36` reads +6.61, against a base of +5.96. None is worth
the volume it costs. The liquidity cap in particular ranks near the top for *predicting the
wallet's fire* and adds nothing to *return*.

## 6. How this is derived

1. **Score every slot that carries a buy**, with token state controlled rather than filtered.
   A silence-only corpus is blind to 45.9 % of the wallet's own buys.
2. **Separate triggers from racers by symmetry** - a trigger lands ahead of the wallet and
   almost never behind; a co-detector lands on both sides.
3. **Score every candidate by MONEY, not only by P(the wallet fires).** These are different
   questions, and the terms that top one routinely rank last on the other.
4. **Group buys by build signature** - router program plus machinery markers - never by a
   hand-written class list.
5. **Concentrate before pricing.** A gate firing 24x more often than the wallet measures its
   own looseness, not the wallet's edge.
6. **Hold n constant** in every late-fill curve, and test every threshold week by week on
   tokens the wallet never touches.

## 7. The engine rule is not yet this rule

The rule in local PG (`8dtx-derived - clean burst + quiet + old enough`, paper, inactive)
does **not** reproduce the numbers above and must not be activated. The window machinery
is verified - `lab/tests/slot_window_parity.rs` folds the real tape and the engine fires
where the SQL says, 1285 of 1286 - so the gap is in what the rule SAYS, not in what the
engine does with it.

| gate, on one 1,500-mint sample | fires | net |
| --- | --- | --- |
| the rule as authored | 205 | -0.16 |
| + 2 or more distinct build groups | 41 | +1.21 |
| the derivation gate | 34 | **+5.10** |

Engine simulate over those same mints (signal fill, real-impact costs): 350 trades,
**-18.97 %**, 14.3 % win.

**What is missing.** The `m_flow_window` group has no count of **distinct builds** in a
window, so "2 or more distinct build groups" - a term of the baseline in section 2 - is
unstated. `unique_wallets` is the only near-neighbour and it is the wallet axis, which
this derivation is not allowed to use; the term needs its own metric (distinct ordered
`ix_labels` hashes), which is a metric extension, not a re-authoring.

**And a residual beyond it.** With the build term added the fire counts nearly agree
(41 against 34) while the money does not (+1.21 against +5.10). The derivation checks
purity at its OWN gate transaction - `allsol` is the running buy total there
(`allsol = cum_sol` on all 225,010 rows), so `rsol >= allsol` is running router purity at
that point - whereas the engine fires at the first instant every condition holds, which
can be an earlier transaction in the same slot. Closing that is the next step.

## 8. Not yet established

- **No fresh-day forward test.** All four weeks are 08-01..08-21. The rule is stable across
  them and has never seen a day outside them.
- **Price impact is assumed** at about 2 % round trip for 1 % of pool. At 0.777 SOL into pools
  this small it may be light, and it is charged flat rather than per pool.
- **The in-slot race is untested live.** The spread between landing behind the gate
  transaction (+6.18) and landing last (+0.58) is what execution decides.
