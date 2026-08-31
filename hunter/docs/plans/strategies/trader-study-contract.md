# Trader study contract

Read this before studying any wallet as a source of an island. It is the contract for
that work: what the user is asking for, how concentration works, and the mistakes that
look like progress. Wallet-specific leftover numbers live in their own files. This file
does not.

Search method: [convexity-search-workflow.md](convexity-search-workflow.md).
Fill and cost: [execution-costs.md](execution-costs.md),
[edge-at-real-latency.md](edge-at-real-latency.md).

---

## 1. The job

Find the **public tape event** the trader decides on. Send on that same moment.

His fills are a **thermometer**, not a template to copy 1-for-1. He can have more than one
family. A rule that catches a real family is enough even if it misses some of his trades.

The deliverable is a leftover rule in plain language, then money on the full tape. A
thermometer table with no leftover is not a result. Matching his trade count is not a
result. Matching a previous Python island's `n` is not a result.

---

## 2. The machine (three layers, never mixed)

```
Door        which tokens we even watch          create-time facts only
Event       the trigger we send on              the print that CROSSSES the rule
Permissions state already true, or we don't    age, quiet, trail, pool size, ...
```

- **Door is not the trigger.** A mint that fails the door stays off. A mint that passes is
  still waiting for the event.
- **Permissions are not the trigger.** They are already true at the event, or we skip.
  Scoring a permission on the unfiltered tape (poison + event mixed) is a different
  question and usually a false one.
- **The event is not "he bought."** The event is something anyone watching the tape can
  see: a gap, then a burst named by **ix build** (who built the tx: program + CU / ATA /
  nonce / seed / fee), not by wallet address, not by exact hash, not by "anything in
  front of him."

His own buys stay **out of the candidate pool**. Putting them in is lookahead: the
thermometer then reads "he fires when he buys."

---

## 3. Completing print

When the burst **crosses** its rule, that print is the fire. Example: several same-build
buys, ~0.3 each, sum reaches 0.9. That print is the send. Later buys in the same slot are
the race, not extra confirmation.

- Do not wait for the rest of the slot.
- Do not wait for the last print.
- Do not skip a slot because the *first* buy was a solo. `0.9 + 0.5` is a crowd on the
  second print. The fire is the print that makes it true, not the first print of the slot.
- Create / launch templates are not burst members. The mint's first slot is not this
  event. Consecutive buy-slots (no empty slot between them) are not a gap. If he
  lands in slot 2+ of a consecutive run, the gap is the empty stretch *before that
  run*, not the 1-slot step into his slot. Prints after the gap and before him are
  the wave, not proof the gap was absent.

Same-slot and next-slot spill are both measured. Pick the book the thermometer prefers. Do
not AND both into one leftover.

---

## 4. Thermometer vs money

| | Thermometer (`he1`) | Money |
| --- | --- | --- |
| Question | When this event prints, does he buy this mint in S or S+1? | If *we* send on that print, do we make money? |
| Population | Full tape, his fills removed | Full tape, not "his mints only" |
| Use | Which leftover cuts lift his habit | The score. SOL, trades/day, days-positive |
| Live rule | Never a gate | 95 ms fill, both legs, `pumpfun_impact` |

`he1` is lookahead (his send). A leftover that lifts `he1` is a *candidate*. It is not
green until money at 95 ms is green with enough trades.

A cut that lifts `he1` and kills the tape is a clone of his send set, not an island. The
book is a tape concentration, not a copy of his landings.

---

## 5. Concentration

Buy-everything is impossible. The unconcentrated pool is red. The work is to take a
leftover: a smaller set that is still the same event, with enough trades to be a rule.

Order, because each answer depends on the one above:

1. **Burst leftover** on this tape: templates that lift, new vs repeat, size band.
   Same-template and mixed both stay unless one of them *clearly* lifts. Do not pre-cut
   from an old island's template list.
2. **Door** on that leftover: only a cut that clearly lifts **and** still leaves hundreds
   of trades. A door that barely moves `he1` (ATA yes/no at lift ~1.0) is not a door.
3. **Permissions** on that leftover: quiet (not busy), already off the high, not
   brand-new. One at a time.
4. **Money** on the leftover at 95 ms. If it is red, there is no rule. If it is green
   with enough trades, widen the window. Do not add days to rescue a dishonest leftover.

**When to stop stacking.** Stop adding gates when first-per-mint trades/day fall under
~50. A leftover of ~30 trades/day that is green on 3 days is a lottery, not a rule. Do
not AND every KEEP until `n = 86`.

**If nothing lifts, do not cut.** Do not fall back to "keep the highest `he1%` anyway."
That forces a split that is not real (same vs mixed, ATA vs not) and then the next gates
run on the wrong pile.

**Sample-size floor is absolute, not a fraction of the pool.** A floor of `len(pool)/50`
on 600k events is 12k. That only keeps mass templates and throws away the concentrated ones
(`n = 800` at 12x lift dies; `n = 25k` at 1.26x lift lives). For templates: `n >= 200`
and lift `>= 1.25`. For other axes: a small absolute floor (tens to low hundreds), not a
percent of the unconcentrated mass.

Re-score templates, door, and permissions on **this** tape. Do not paste cuts from a
previous island, a previous wallet, or a previous Python script.

---

## 6. Closed mistakes (the rule they produce)

Each of these has already happened. Doing it again is the same bug.

| Mistake | The rule |
| --- | --- |
| Bind the leftover to a previous island's `n`, cuts, or "first event wins the slot" | Leftover is derived on this tape. Matching old `n` is not the job. |
| Treat matching his trade count as success | One family does not have to cover every trade. Recall of his book is a check, not a target. |
| Skip a burst because the first buy in the slot was a solo | Fire on the print that makes 2+ wallets / sum-in-band true. |
| Pre-cut templates, door, or permissions before scoring | Open the pool. Score. Then leftover. |
| Put his own buys in the burst | Leave them out. Thermometer is response, not membership. |
| Wait for the last print / completed slot | Completing print = crossing print. |
| Score money on his mints only | Full tape. His mint list is not a gate. |
| Put `he1` or his mint list in the live rule | Thermometer only. |
| Map an engine metric that is not the same quantity | The finding sets the metric. `working_template_count` (on the fingerprint's list) is not `member_template_count` (every member grain in the slot). |
| Charge `B` in lamports against `vsol` in SOL | One unit. `B = 0.10` SOL against pool SOL. 125 bps/leg + own `B/vsol`. If every day is ~-100% with 0% wins, the unit is wrong, not the island. |
| AND every KEEP | Stop at ~50 trades/day. A thin green slice is not a rule. |
| Fallback-keep the highest band when nothing lifts | Leave that axis uncut. |
| Select the exit by its own score | Exit shape from mechanism, then money. |
| Report gross price move as PnL | Net, `pumpfun_impact`, both legs lagged. |
| Clone the wallet / fingerprint / send set | Public event, then leftover, then money. |
| Treat quiet as a leftover cut on any-slot 2-wallet bursts | Quiet is the event: a gap, then the named burst. Score how long the gap is. Do not leftover-cut a pool that never required a gap. |
| Measure gap as time/slot since the last print before the trader | That print is the wave after the gap. Gap = empty buy-slots *before this slot* (or before the consecutive-slot wave if he is in slot 2+). Prefix wallets after the gap are expected. Gap-to-him reading ~0 is not "no gap." |

---

## 7. Order of work on a new trader

Do not start from a previous leftover. Do not start from engine JSON.

1. **Open the pool.** Curve buys (or whatever venue this family is). His fills out.
   Every burst kind in: one template vs several, one wallet vs several, size bands. No
   pre-cut.
2. **Thermometer.** Which kinds does he actually fire on? Same slot and next slot,
   separately. Keep families separate.
3. **Burst leftover.** Templates with `n >= 200` and lift `>= 1.25`. New vs repeat if it
   lifts. Size if it lifts. Same vs mixed: both stay unless one clearly lifts.
4. **Door** on that leftover. Only if it lifts and leaves hundreds of trades.
5. **Permissions** on that leftover. Quiet, off the high, not brand-new. Stop at ~50
   trades/day.
6. **Money** at 95 ms, both legs, `pumpfun_impact`, one episode per mint first, then
   re-entry as a second book. If red: no rule. If green and thick enough: widen days.
   Do not retune the cell to chase `n`.

Say the leftover out loud before money:

> Two or more wallets, none of them seen on this mint before, sum 0.5-4 SOL, completing
> template in {this list}, after a quiet, already off the high, token not brand-new.

If you cannot say it, you do not have a leftover.

---

## 8. Cost, fill, units

- Fill = last print with `ts <= fire + 95 ms`. Both legs. The bot's measured lag is ~95 ms
  ([island-map.md](island-map.md)).
- Cost = 125 bps/leg + own impact `B/vsol` at `B = 0.10` SOL. `vsol` is SOL, not lamports.
- PnL % is money over capital, not a price ratio.
- A book with median negative and win rate ~30% can still be the island. Do not require
  win rate > 50% or a positive median. Do not cap the right tail with a take-profit.

If the first money pass is ~-100% with 0% wins on every day, stop. The cost unit is
wrong. Fix that before any leftover.

---

## 9. What a result looks like

A result is:

1. The event, in one sentence a trader would recognise.
2. The leftover cuts, each with why (lift, and trades still left).
3. Money at 95 ms: n, /day, mean, median, win, SOL, days-positive.
4. What it does not claim (families it misses, days it was not tested on).

It is not: a KEEP table, a comparison to a previous Python `n`, or an engine mapping of
a leftover that money has not passed.
