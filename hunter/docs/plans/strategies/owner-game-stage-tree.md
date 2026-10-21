# The owner game and the stage tree: reference

What a launch group's coin does, how to read it live, and how a rule follows it. Written from
the 6ix study (fingerprint `--- 6ix:Buy`, `b1e512a7`); the method and the concepts apply to any
group. Numbers are 6ix: "recent" = 10-18 + 10-19 (271 coins), "old" = 10-01..10-09 (851 coins).
No local data for 10-16 and 10-17. The engine plan: [stage-tree-plan.md](stage-tree-plan.md).
Study files: `hunter/_local/study-6ix-buy/` (NOTES.md sections 16-16d).

---

## 1. The core idea in one picture

```
  The OWNER launches the coin, holds ~95 % of it, and trades it with many wallets.
  OUTSIDERS (snipers, traders, us) bring the only money he can take.

            +------------- the owner's loop (repeats all life) --------------+
            |                                                                 |
            v                                                                 |
     OWNER PUMPS  ----->  OUTSIDERS BUY  ----->  OWNER SELLS ON THEM  ----->  OUTSIDERS LEAVE
     (price up)           (a burst of new        (shake-out,                  (they sell out,
                           outsider money)        price down)                  he pumps again)
                                                      |
                                                      +--> sometimes this sell is THE DUMP

  So:  BUY  after outsiders leave or after a shake-out (he is about to pump again)
       HOLD through his pumps and shake-outs
       SELL into the next outsider burst (that money is what he is about to take)
```

The dump is not a random event: it follows fresh outsider money. 44 % of 6ix dumps come within
2 s of an outsider buy burst; 19 % while the owner is already net selling; 24 % show no sign.

---

## 2. Who is who

- **Owner** = the fingerprint's owner tag (`@volume` in 6ix): its wallet list, the creator, its
  ix shapes, `sticky`. **Outsider** = every trade without the tag (`@!volume`).
- **Our own trader wallets are never tagged** (engine rule since 10-20): they share ix shapes with
  launch tools, so before the fix every one of our 1,628 6ix trades read as owner. They are
  outsiders, as the owner sees them.
- A generic tag shape (`[Limit, Price, Sell]`) can still tag some sniper sells as owner: a known
  impurity of the 6ix tag, not measured yet.

---

## 3. Two layers: phase and situation

```
  PHASE      = WHEN in its life     -> moves FORWARD only     (floors of a building)
  SITUATION  = WHAT happens now     -> re-picked every step   (rooms on a floor)
  CELL       = PHASE x SITUATION    -> owns the buy / sell / hold lines
```

- A coin is in exactly one cell at every print or tick.
- Phases are cut by age first, then by the highest pool so far (`m_state.peak_liquidity_sol`).
  A phase's `until` reads only metrics that never fall, so the coin's phase is simply how
  many phases have ended: it never goes back, is known before the buy, and needs no memory.
- Situations are checked top to bottom; the first that holds wins; the last catches the rest.
- Different coins take different paths through the grid; every path is one coin's life.

### 6ix phases (found by a tree on what happens next, plateau-checked)

```
  P1  age < 1.9 s           launch + kill zone: every situation dumps 32-44 % (recent)
  P2  age 1.9 - 8.5 s       the owner decides: eat the snipers or shake them out
  P3  age 8.5 - 32 s        the loop runs fast
  P4a until peak pool >= 45 slow loop, dumps rare except after a burst
  P4b until peak pool >= 65 range: small edges
  P4c (to migration)        push to migration: buying here loses
```

### 6ix situations (all on existing metrics)

```
  burst        m_flow.buy_sol @!volume [1s] >= 0.5
  shake-held   m_flow.net_sol @volume [5s] <= -1.5   AND  m_flow.net_sol @!volume >= 0.5  AND  not leaving
  shake-light  m_flow.net_sol @volume [5s] <= -1.5
  leaving      m_flow.sell_sol @!volume [5s] >= 0.3  AND  m_flow.net_sol @!volume [5s] < 0
  pump-held    m_flow.net_sol @!volume >= 0.5
  pump-light   (everything else)
```

"held" = outsiders hold 0.5 SOL or more net (life). "light" = they hold less. In the rule the
names are `snake_case` (`shake_held`) and "leaving" is a signal the two situations share.

---

## 4. The grid (6ix recommended rule)

```
                burst          shake-held   shake-light   leaving   pump-held  pump-light
  P1            SELL           -            -             -         -          -
  P2            SELL           hold         BUY           BUY       hold       hold
  P3            SELL           hold         BUY           BUY       hold       hold
  P4a           SELL >= 1.5    hold         BUY           BUY       hold       hold
  P4b           SELL           hold         hold          hold      hold       hold
  P4c           SELL           hold         hold          hold      hold       hold

  always : sell on the owner's dump bundle print (m_flow.trade_count @dump >= 1)
  entry  : a print in a BUY cell, price <= 40 % under its high, pool >= 5 SOL,
           and no chasing: m_price.rise_pct [5s] <= 27
```

- **Why the burst size grows in P4a:** a bigger pool absorbs a 0.5 SOL burst; the owner dumps on
  bigger money there (1.5-2.0 SOL is a plateau: better on 10-18, 10-19 and old).
- **Why no sell elsewhere:** every non-burst sell cell loses (up to -1.5 SOL recent). Holding
  through the loop is where the profit is.
- **Why no buy in a pump before 8.5 s:** -1.1 to -1.9 SOL recent. (Old data says the opposite,
  +1.06: the owner kills early pumps much more often now. Rules follow the recent regime.)

Scores (kernel, no situation hold):

| Version | Difference | Recent SOL | Without best 3 | Coins | Win | Old | wk |
| --- | --- | --- | --- | --- | --- | --- | --- |
| max PnL | P4a burst >= 2.0 | +2.24 | +1.60 | 122 | 50 % | +3.69 | +0.21 |
| recommended | P4a burst >= 1.5 | +2.17 | +1.54 | 122 | 50 % | +3.85 | +0.18 |
| safest | + sell all at pool 65 | +2.13 | +1.55 | 122 | 50 % | +3.84 | +0.18 |

Shuffle control: random sell timing +0.13, random buy timing +0.37, 0 of 40 draws reach the
real result. 11 of 13 recent bursts positive. The three versions differ by less than the noise.

---

## 5. One step of the engine (per print or tick)

```
  +----------------------------------------------+
  | 1. always lines        (dump print -> sell)  |
  | 2. next phase's `until` true? -> move 1 on   |
  | 3. first situation whose `when` holds        |
  | 4. lines of the MOST SPECIFIC cell entry     |   P4a x burst replaces * x burst
  +----------------------------------------------+
```

A wider entry (`* x burst`) never fires where a narrower one (`P4a x burst`) exists: the
narrower one replaces it. Otherwise an override could never raise a threshold.

---

## 6. One real coin (D2gan..., 10-19)

```
  age     pool  what the data shows                              cell               rule
  0.4 s   10.4  outsiders buy 0.74 SOL right away                P1 burst           (no buy in P1)
  1.2 s   10.1  snipers sell 0.61 SOL, owner keeps pumping       P1 pump-light      -
  3.9 s   16.9  outsiders hold nothing: they sold out            P2 leaving         BUY
 14.6 s   21.0  owner net sells 1.6 SOL, outsiders hold 0.2      P3 shake-light     hold (recovers)
 23.1 s   26.9  owner pumps, outsiders now hold 0.5              P3 pump-held       hold
 43.0 s   33.6  owner sells, outsiders hold 0.7                  P4a shake-held     hold
 47.9 s   23.9  outsiders buy 1.28 SOL in 1 s                    P4a BURST >= 1.5   SELL
 50.1 s    -    owner dumps everything                           (dump)             already out
```

Bought at pool 16.9, sold at 23.9: price moves with the pool squared, so about +32 % before fees.

---

## 7. The life funnel (6ix recent, coins entering each stage)

```
  271 created
   |-- kill zone 0.5-3 s ........ 25 % dump
  202
   |-- early swings 3-10 s ...... 42 % dump
   |-- best pool < 20 ........... 32 % dump      (81 coins)
   |-- best pool 20-45 .......... 56 % dump      (90 coins)
   |-- best pool 45-65 .......... 44 % dump      (39 coins)
   |-- best pool 65-85 .......... 64 % dump, 36 % migrate (22 coins)
```

Old period: same shape, fewer early kills (kill zone 9 %), 53 % of the 65+ coins migrate.
Creation buy: 8.9-11.8 SOL on every recent 6ix coin (no other band). Migration: 3 %.

---

## 8. How the map was found (the method, reusable for any group)

```
  1. KERNEL     raw trades -> rows = prints + 0.2 s ticks; a decision fills on the state
                115 ms later; our buy impact and fees included. Check it against the engine
                first (6ix: 269 / 269 coins within 0.003 SOL).
  2. OUTCOME    for every moment: what comes first within 20 s from the fill:
                UP +15 % | DOWN -15 % | DUMP (death crash within 5 s) | FLAT
  3. PHASES     a tree on live state predicts the outcome; its age / pool cuts are the phases
  4. SITUATIONS owner mode x outsider mode -> outcome per combination; merge the ones that
                behave alike; check each cut-off sits on a plateau (spread moves <= 3 %)
  5. CELLS      buy / sell worth of every cell in SOL, one change at a time
  6. AND LINES  conditions inside a cell (e.g. the P4a burst size), one at a time
  7. CHECKS     choose on one day, confirm on the next (10-18 -> 10-19), old period,
                shuffle control, without the best 3 coins, per burst
  8. ENGINE     the same rule in the engine, engine vs kernel per coin
```

---

## 9. Rules of logic (every condition must pass)

- A window fits inside the age where it is read (no `[30s]` check at age 5 s).
- A tag must be able to fire before the decision (the dump tag fires only at the dump).
- The direction matches the story ("outsiders hold a lot" -> the owner dumps on them).
- A condition is not already true for most coins at our fill.
- Every combination is reached by 20+ coins, inside observed values.
- A stage is a situation of the game, never an event we must exit before (a dump print is
  not a stage), and never a fact that is always true (the owner's first sell comes at once,
  because he makes volume from the first second).
- A choice made on one data split must hold on the split it did not see.

---

## 10. Mistakes made in this study, and the rule each one gives

| Mistake | Rule |
| --- | --- |
| A 0.5 s situation hold looked +0.17 better on a simple rule and was carried into the final ones, where it cost -0.13 | Re-score every setting on the final rule; a gain on one rule is not a gain on another |
| "No buy in P2 when outsiders leave" chosen on mixed evidence (10-18 -0.04, old -0.17) | Keep a change only when it holds on every split |
| "Safest = sell at pool 45" lowered every split | A version that only loses is not safer; safety is a cap on exposure at equal results |
| Late entries were first scored with buys on already-crashed coins | Every entry needs the alive guard (trail <= 40 %, pool >= 5) |
| A cell format that ran every matching entry would have let a wide line undo an override | The most specific cell entry replaces the wider ones |
| Our wallets read as owner in every metric | Own wallets are never tagged (engine rule) |
| Exits that catch "the dump is starting" (price -12 % in 1 s with owner selling) | They cut the shake-outs that recover and lose; the no-sign dumps stay a cost |

---

## 11. Status (10-20)

- Own-wallet rule: in the engine, lab simulate, replay inspect, rule readout, metric series.
  Not in the grouped sweep or entry-context reads.
- Stage tree: built in the engine and lab simulate ([stage-tree-plan.md](stage-tree-plan.md));
  the three 6ix rules are paper, inactive rules `6ix tree max_pnl / recommended / safest`.
- Not covered: after migration (almost no AMM data); 10-16 and 10-17 (no local data).
