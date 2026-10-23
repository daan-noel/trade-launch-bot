# Fingerprint case: 6ix (`--- 6ix:Buy`)

The 6ix group's reference: identity, data, the owner's game, the stage-tree rules in the DB, the
AMM exit and the case's mistakes. The method is [fingerprint-rule-method.md](fingerprint-rule-method.md).

**Method note.** The 6ix rules come from an earlier, search-based form of the method: splits
PICK / HOLD / OLD instead of the recency window, a greedy per-cell AND search kept by a luck test
and leave-one-session-out, row-weighted outcome labels (UP / DOWN / DUMP / FLAT in 20 s) instead
of per-coin onset lifts. They are a worked example, not a template: a new run on 6ix follows the
method from W0.

---

## 1. Identity

| What | Value |
| --- | --- |
| Fingerprint | `--- 6ix:Buy`, id `b1e512a7` |
| Owner tag | `@volume` (wallet list, creator, ix shapes, `sticky`) |
| Creation buy | 8.9-11.8 SOL on every recent coin (one narrow band: never a cut) |
| Migration | about 3 % of coins |
| Study folder | `hunter/_local/study-6ix-buy/` (NOTES.md sections 10 and 16-16d) |

Known tag impurity: a generic shape (`[Limit, Price, Sell]`) can tag some sniper sells as owner;
not measured.

---

## 2. Data and regimes

```
  per session:  coins | died < 3 s | owner trades per coin | life
  10-01..10-15     died<3s  3-15 %   owner trades  75-180   life mostly 250-340 s   (calm habit)
  10-18 / 10-19    died<3s  7-17 %   owner trades  80-260                            (normal sessions)
  14 h UTC         died<3s 43-52 %   owner trades  26-41                             (kill sessions)

  splits used: PICK 10-18 .. 10-19 18h | HOLD 10-19 21h .. 10-20 | OLD 10-01 .. 10-15
```

- The dev makes no 6ix coins on 10-16 and 10-17 (the DB holds trades those days).
- "Died" = the pool crashes; no coin has a dump print within 3 s, so the dump tag is not the
  kill signal.

---

## 3. The owner's game

```
  The OWNER launches the coin, holds ~95 % of it, and trades it with many wallets.
  OUTSIDERS (snipers, traders, us) bring the only money he can take.

     OWNER PUMPS --> OUTSIDERS BUY --> OWNER SELLS ON THEM --> OUTSIDERS LEAVE --+
         ^           (fresh money)     (shake-out or THE DUMP)                    |
         +-----------------------------------------------------------------------+

  BUY   after outsiders leave or after a shake-out (he is about to pump again)
  HOLD  through his pumps and shake-outs
  SELL  into the next outsider burst (that money is what he is about to take)
  SELL  at once on his dump print
```

- The dump follows fresh outsider money: 44 % of dumps come within 2 s of an outsider buy burst,
  19 % while he is already net selling, 24 % show no sign.
- Curve vs AMM: on the curve the owner tag is reliable and the signal is WHO acts; on the AMM new
  owner wallets join, swings are big and fast, and the signal is WHAT the price does.

### Life funnel (recent, coins entering each stage)

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

---

## 4. Phases and situations

```
  P1  age < 1.9 s           launch + kill zone: every situation dumps 32-44 % (recent)
  P2  age 1.9 - 8.5 s       the owner decides: eat the snipers or shake them out
  P3  age 8.5 - 32 s        the loop runs fast
  P4a until peak pool >= 45 slow loop, dumps rare except after a burst
  P4b until peak pool >= 65 range: small edges
  P4c (to migration)        push to migration: buying here loses
```

| Situation (first match) | Engine terms |
| --- | --- |
| burst | `m_flow.buy_sol @!volume [1s] >= 0.5` |
| shake-held | `m_flow.net_sol @volume [5s] <= -1.5` AND `m_flow.net_sol @!volume >= 0.5` AND not leaving |
| shake-light | `m_flow.net_sol @volume [5s] <= -1.5` |
| leaving | `m_flow.sell_sol @!volume [5s] >= 0.3` AND `m_flow.net_sol @!volume [5s] < 0` |
| pump-held | `m_flow.net_sol @!volume >= 0.5` |
| pump-light | the rest |

"held" = outsiders hold 0.5 SOL or more net (life).

---

## 5. The grid (recommended rule)

```
                burst          shake-held   shake-light   leaving   pump-held  pump-light
  P1            SELL           -            -             -         -          -
  P2            SELL           hold         BUY           BUY       hold       hold
  P3            SELL           hold         BUY           BUY       hold       hold
  P4a           SELL >= 1.5    hold         BUY           BUY       hold       hold
  P4b           SELL           hold         hold          hold      hold       hold
  P4c           SELL           hold         hold          hold      hold       hold

  always : sell on the owner's dump print (m_flow.trade_count @dump >= 1)
  entry  : a print in a BUY cell, price <= 40 % under its high, pool >= 5 SOL,
           no chasing: m_price.rise_pct [5s] <= 27
```

- The burst size grows in P4a: a bigger pool absorbs 0.5 SOL; 1.5-2.0 SOL is a plateau.
- Every non-burst sell cell loses (up to -1.5 SOL recent): holding through the loop pays.
- No buy in a pump before 8.5 s: -1.1 to -1.9 SOL recent (old data says +1.06; the recent regime rules).

| Version | Difference | Recent SOL | Without best 3 | Coins | Win | Old |
| --- | --- | --- | --- | --- | --- | --- |
| max PnL | P4a burst >= 2.0 | +2.24 | +1.60 | 122 | 50 % | +3.69 |
| recommended | P4a burst >= 1.5 | +2.17 | +1.54 | 122 | 50 % | +3.85 |
| safest | + sell all at pool 65 | +2.13 | +1.55 | 122 | 50 % | +3.84 |

Luck control: random sell timing +0.13, random buy timing +0.37, 0 of 40 draws reach the real
result. The three versions differ by less than the noise.

### One real coin (D2gan..., 10-19)

```
  age     pool  what the data shows                              cell               rule
  0.4 s   10.4  outsiders buy 0.74 SOL right away                P1 burst           (no buy in P1)
  3.9 s   16.9  outsiders hold nothing: they sold out            P2 leaving         BUY
 14.6 s   21.0  owner net sells 1.6 SOL, outsiders hold 0.2      P3 shake-light     hold (recovers)
 43.0 s   33.6  owner sells, outsiders hold 0.7                  P4a shake-held     hold
 47.9 s   23.9  outsiders buy 1.28 SOL in 1 s                    P4a BURST >= 1.5   SELL
 50.1 s    -    owner dumps everything                           (dump)             already out
```

---

## 6. Tuning result (search, luck test, leave-one-session-out)

- Rule A: the luck runs find +0.4..+0.9 on PICK, the real search +0.3..+1.5, so most found
  conditions are luck. Only "no sell on a P3 burst" is found in 6 of 6 runs: kept.
- Rule B: only "no buy in P2 shake-light" is found in 6 of 6 and positive in every hidden group.
- Extras that do not pass: position lines (stop, trail, time held), early kill after the fill
  (ceiling 0.16 SOL, best line +0.03 on 5 trades), part sells, session brake.

---

## 7. AMM exit (rule B)

```
  45 migrated coins, rule B's positions (paid 2.26 SOL in all):
     sell at migration .................. +12.8 SOL
     sell at each coin's 1 h high ....... +87   SOL   (perfect timing: impossible)
     hold to 1 h ........................ +0.15 SOL   (nearly every coin dies)
  median coin: high about 10 min after migration, dead about 17 min after
```

Exit, the first of (each line with `m_state.on_curve = 0`):

```
  m_price.rise_pct  [30s] >= 100      spike: the price doubled over its 30 s low -> take profit
  m_price.trail_pct [amm] >= 50       crash backstop: half below the high since migration
```

| Exit | last week (13 coins) | without best 2 | better / worse | older 32 coins |
| --- | --- | --- | --- | --- |
| trail [amm] 40 | +3.08 | +2.26 | - | +19.6 |
| spike + backstop | +7.36 (engine +7.44) | +3.22 | 10 / 3 | +16.4 |

Neighbours (spike 10-60 s, +50..+200 %, drop 50-70 %): +5.4 .. +7.4. Time limits help only the
older coins; a single owner line scores alone but worsens the best basic exit.

---

## 8. Engine and saved rules

- Parity: A engine +3.98 vs kernel +3.97, 95 % of coins within 0.003 (10-06 .. 10-19); B holds
  all 18 migrated coins on the curve; the AMM exit matches 13 of 13 coins.
- Paper, inactive rules in the DB: `6ix tree max_pnl`, `6ix tree recommended`, `6ix tree safest`,
  `6ix tree tuned max_pnl` (recommended minus the P3 burst sell), `6ix tree tuned hold_migrated`
  (the recommended buys minus P2 shake-light, the dump print as the only curve sell, the AMM exit
  of section 7).

---

## 9. 6ix mistakes (each now a rule in the method, "Mistakes")

| Mistake | Rule it gives |
| --- | --- |
| A 0.5 s situation hold scored +0.17 on a simple rule and -0.13 on the final one | re-score every setting on the final rule |
| "No buy in P2 when outsiders leave" chosen on mixed evidence | keep a change only when the recency window agrees |
| "Safest = sell at pool 45" lowered every split | a version that only loses is not safer |
| Late entries bought already-crashed coins | every entry carries the alive test |
| Our wallets read as owner | our wallets are never tagged |
| Exits on "the dump is starting" (price -12 % in 1 s with owner selling) | they cut shake-outs that recover |
| Loose search guards (4 trades): A +2.17 -> +4.35 with 11 conditions, HOLD gained only from the first | 8+ trades, without best 3, plateau |
| B's sells checked only on held rows sold all 3 unseen migrated coins | B's held share on all bought coins |
| An AMM exit carried by one coin (+3 of +6.8) | without the best 2, leave-one-coin-out |
| AMM drop line at 40 % fired on the post-migration shake-out | backstop wider than the swing |
| The study's 60 s window read AMM prints only | `[ws]` crosses the venue in the engine |
| Labels and outcomes counted per row, not per coin at onset | method L3 |

---

## 10. Streak gate (measured 10-22)

Coins 10-08..10-22: 377, 15 migrated. The owner is a list of 68 wallets plus the creator; there
is no launch bundle trick as in 5ix.

| Event | Last-3 gate: skipped | Next coin had it: skipped vs bought | Migrated skipped |
| --- | --- | --- | --- |
| pool died within 5 s | 34 | 68 % vs 28 % | 0 of 15 |
| pool died within 10 s | 70 | 77 % vs 40 % | 1 of 15 |
| owner sold half his bag within 5 s | 78 | 76 % vs 41 % | 3 of 15 |
| owner sold half his bag within 10 s | 196 | 87 % vs 68 % | 7 of 15 |
| dump print within 10 s | 0 | - | - |

The owner sells half his bag within 10 s on 78 % of coins that live on, and the dump print
comes late, so only the tag-free pool-death event works here.

---

## 11. Open

- The 6ix rules predate the current method (onset, per-coin lift, recency window, W5 order).
  A rerun from W0 is the way to update them.
- The tag impurity of section 1 is unmeasured.
