# Entry condition: is it his rule?

How to measure whether a condition is a trader's entry rule. This is the one place the method is
explained; [_!___derive.md](_!___derive.md) 5.0 points here. The counter is
`node-derivation/toolkit/hitrate.py`, and the test that proves it is
[_!___evidence.md](_!___evidence.md) 5.8.

**The question:** when the condition happens on the market, does he buy?

## 1. Count the chances

A chance is a moment the condition **becomes true** on any coin in the market - every coin, not
only the coins he traded.

- It stays true for several trades: still **one** chance.
- It turns off and on again within his reaction time (section 3): still **one** chance.

## 2. Skip the moments he couldn't buy

Don't count a chance when:

- he already holds that coin,
- he already holds his maximum number of coins (read from his own trades: the most he ever held
  at once),
- he is switched off: no trade of his for 30 minutes around that moment.

At those moments he could not buy, so they say nothing about his rule.

If his trades show a re-entry cooldown (he never buys a coin again within some time after
selling it), a chance inside that cooldown is skipped too. Measure it from his re-entries first;
without a measured cooldown, nothing is skipped.

## 3. Hit or miss

- **Hit:** he buys that coin within his **reaction time** after the chance.
- **Miss:** he doesn't.

His reaction time is how fast he answers what he reacts to. Measure it from his buys: for each
buy, count the slots back to the last chance on that coin. Most of his buys land at the same
small delay; where that pile ends is his reaction time. 8dtx2t's is **1 slot** (about 0.4 s).

His own trades never count as part of the condition. A chance must come before his buy.

## 4. Two numbers

- **Hit rate = hits / chances.** How close the condition is to his rule. 100 % means he buys
  every chance: it is his rule.
- **Cover = hits / all his buys.** How much of his buying the condition explains.

**Example.** In one day the condition gives 200 chances; he buys 150 of them; he made 500 buys
in total.

- Hit rate = 150 / 200 = **75 %**
- Cover = 150 / 500 = **30 %**

## 5. Read the result

| Hit rate | Cover | Meaning |
| --- | --- | --- |
| High | High | His rule |
| High | Low | Only a small piece of his rule - look for the wider condition with the same hit rate |
| Low | High | Part of his rule, but a condition is missing - add one |
| Low | Low | Not his rule |

**Luck check.** Move each chance to a random moment on the same coin and count again. That hit
rate is what timing alone gives; a real rule is far above it.

His whole logic is usually several rules. Each is measured on its own, and their covers add up
toward 100 %.

**8dtx2t today** (09-01 .. 09-06): his event E - a quiet coin woken by a new wallet's buy that
moves the price >= 1.9 % - has a hit rate of **0.77 %** (he buys 1 chance in 130), cover
**60 %**, luck 0.11 %. So E is part of his rule, and conditions are missing.

## 6. Trust it only if

- it has **at least 30 hits** - fewer can be luck,
- you build it on half the days and it gives the **same hit rate** on the other half.

## Why not lift

Lift only says "he buys here more often than usual". That can still mean he skips 97 of every
100 chances, so it never tells you how close a condition is to his rule. In the test, lift
ranked wrong conditions above the true ones (evidence 5.8).
