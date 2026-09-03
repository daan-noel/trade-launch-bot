# Trader study contract

The contract for Phase 3 of [market-model-and-workflow.md](market-model-and-workflow.md).
Read it before studying any wallet.

Fill and cost: [execution-costs.md](execution-costs.md),
[edge-at-real-latency.md](edge-at-real-latency.md).

---

## 1. What a trader study is for

A profitable trader is a **reader of the machine**. He recognises a dev campaign starting
and rides it. So he is an **instrument**: he tells you which tape signatures carry
follow-through and which ones a professional skips. He is never the rule.

The deliverable is **evidence about the dev's decision node** — which
`(gap, tool, count, size)` signatures he fires on, which he declines, and what separates
them — stated as one causal sentence a trader would recognise. His fills are a
thermometer, not a template.

Not a result: a thermometer table with no sentence; matching his trade count; reproducing
a previous script's `n`.

**A refuted hypothesis about him closes the hypothesis, never him.** A daily-profitable
trader runs a derivable procedure over public data; failure to find it indicts the
hypothesis space.

**Never build a factor on wallet identity.** Operators rotate wallets; the durable axis is
the ix structure of the transaction. A wallet is the subject of a study, never a term in a
rule.

---

## 2. The machine (four layers, never mixed)

```
Door        which tokens we even watch     create-time facts only (launch machinery)
Event       the trigger we send on         the print that CROSSES the rule
Permissions state already true, or skip    age, depth, phase, off-the-high, ...
Exit        how we keep the harvest        shape from the story, after the gates
```

- **Door is not the trigger.** A mint that fails the door stays off. A mint that passes is
  still waiting for the event.
- **Permissions are not the trigger.** They are already true at the event, or we skip.
- **The event is not "he bought."** It is what anyone watching the tape sees: a gap, then
  a burst named by **ix build** — program + CU / ATA / nonce / seed / fee — never by wallet
  address, never by exact hash, never by "anything in front of him."

His own buys stay **out of the candidate pool**. Putting them in is look-ahead: the
thermometer then reads "he fires when he buys."

Racer signatures — builds carrying `CreateAccountWithSeed`, and the swarm that follows a
visible trigger — mark that *someone else's* rule already fired. They are confirmation
evidence about the event, never the entry.

---

## 3. Completing print

When the burst **crosses** its rule, that print is the fire.

- Do not wait for the rest of the slot, or for the last print.
- Do not skip a slot because the *first* buy was a solo: `0.9 + 0.5` is a crowd on the
  second print. The fire is the print that makes the rule true.
- Create/launch templates are not burst members. The mint's first slot is not this event.
- Consecutive buy-slots (no empty slot between them) are not a gap. If he lands in slot 2+
  of a consecutive run, the gap is the empty stretch *before that run*. Prints after the
  gap and before him are the wave, not proof the gap was absent.
- Measure the gap as empty buy-slots **before the slot** (or before the consecutive-slot
  wave). A gap measured to him reading ~0 is not "no gap."

Same-slot and next-slot spill are measured separately. Do not AND them into one rule.

---

## 4. Thermometer vs money

| | Thermometer (`he1`) | Money |
| --- | --- | --- |
| Question | when this event prints, does he buy this mint in S or S+1? | if *we* send on that print, do we make money? |
| Population | full tape, his fills removed | full tape, never "his mints only" |
| Use | which signatures concentrate his habit | the score: SOL, trades/day, days-positive |
| Live rule | never a gate | 95 ms fill, both legs, `pumpfun_impact` |

`he1` is look-ahead (his send). A signature that lifts `he1` is a *candidate*, green only
when money at 95 ms is green with enough trades.

**Rank on total net SOL.** Mean, median and win rate are description. Ranking on mean
percent selects thin high-percentage pockets; the same event ranked on SOL reads green at
100+ trades/day.

**Measure lift on the grain the book collapses to** — one episode per mint. Per-print and
per-mint lift disagree and can invert (2.68x per print against 0.74x per mint on the same
event); the per-mint number is the one that predicts money.

**The lift bar belongs to the event, never to the gates.** Lift is how a signature is
named and how a dead one is thrown out. Doors and permissions are ranked on money with
lift merely reported: requiring them to lift forces the rule to keep tracking him, which
is cloning by another route. The largest single money term ever measured had lift **0.69**
— it anti-tracked the trader, and no lift-gated search can reach it.

---

## 5. Concentration

Buy-everything is impossible; the unconcentrated pool is red. The work is a smaller set
that is still the same event, with enough trades to be a rule.

1. **Signature** on this tape: which builds, how many, what size band, new vs repeat.
2. **Door** on that set: rank candidates by money, one at a time.
3. **Permissions** on that set: one at a time, ranked by money.
4. **Money** at 95 ms. Red means no rule.

A filter is kept only while it **raises total net SOL**, and total SOL is self-limiting: it
stops rising once a cut removes more good trades than bad. **Never thin the book below ~50
first-per-mint trades/day** — that floor is a refusal, not a target, and a 30-trade/day
book green on 3 days is a lottery.

**Do not trim the sentence on held-out performance** — that is fitting the holdout. A
nine-term sentence truncated to three on that basis priced −0.15 SOL on fresh days where
the untrimmed one priced +0.25.

**If nothing lifts, do not cut.** Forcing a split that is not real sends the next gate to
work on the wrong pile.

**The sample-size floor is absolute, not a fraction of the pool.** `len(pool)/50` on 600k
events is 12k, which keeps only mass templates and discards concentrated ones. Use
`n >= 200` and lift `>= 1.25` for templates; a small absolute floor elsewhere.

Re-score everything on **this** tape. Never paste cuts from a previous study.

---

## 6. Closed mistakes

| Mistake | The rule |
| --- | --- |
| Treat matching his trade count as success | One family need not cover every trade. Recall is a check, not a target |
| Skip a burst because the first buy was a solo | Fire on the print that makes the rule true |
| Pre-cut templates, door or permissions before scoring | Open the pool. Score. Then concentrate |
| Put his own buys in the burst | Leave them out. The thermometer is response, not membership |
| Wait for the last print or the completed slot | Completing print = crossing print |
| Score money on his mints only | Full tape. His mint list is not a gate |
| Put `he1` or his mint list in the live rule | Thermometer only |
| Compare "his mints" against a control conditioned on the same later event | Forward conditioning. At matched outcome the control reads better |
| Map an engine metric that is not the same quantity | The finding sets the metric |
| Charge `B` in lamports against `vsol` in SOL | One unit: 125 bps/leg + own `B/vsol`. Every day at ~−100% with 0% wins means the unit is wrong |
| AND every keeper until the book is thin | Stop while SOL is still rising; never below ~50 trades/day |
| Select the exit by its own score, or before the gates | Exit shape from the story, then money. The unfiltered pool is ~87% dying tokens, so any sweep on it returns the shortest clock |
| Rank on mean percent | Rank on total net SOL |
| Require every door and permission to lift `he1` | Lift names the event; gates are ranked on money |
| Score lift per print when the book is one episode per mint | Measure on the grain the book collapses to |
| Trim the sentence because a shorter prefix scores better held out | Fitting the holdout |
| Find the fill by scanning `block_time` | `block_time` is the ingest clock. Locate the fire by `(mint, slot, tx_index)`, bound the scan by slot |
| Report gross price move as PnL | Net, `pumpfun_impact`, both legs lagged |
| Clone the wallet, fingerprint or send set | Public event, then concentration, then money |
| Treat quiet as a cut on any-slot bursts | The gap is part of the event, not a filter on a pool that never required one |
| Conclude "the trader is closed" | Only a hypothesis closes |

---

## 7. Cost, fill, units

- Fill = the last qualifying print at or before fire + 95 ms, **both legs**.
- **The two clocks do different jobs.** The *fill* uses wall clock deliberately:
  `block_time` is the ingest stamp, the right clock for a reaction lag. Every *metric and
  gate* uses chain order — sort by `(slot, tx_index)`, never by `ts`, and locate the fire
  print by its key. A bare `ts <=` filter folds later-in-the-slot prints the rule cannot
  have seen, because `block_time` runs backward against chain order on a small fraction of
  pairs. That is look-ahead and it reads as a real edge. Bound a scan by slot, so one
  out-of-order stamp cannot truncate it.
- Cost = 125 bps/leg + own impact `B/vsol` at `B = 0.10` SOL. `vsol` is SOL, not lamports.
- PnL % is money over capital, not a price ratio.
- A book with a negative median and a ~30% win rate can still be the right one. Do not
  require a positive median. Do not cap the right tail with a take-profit.

If the first money pass is ~−100% with 0% wins on every day, stop: the cost unit is wrong.

---

## 8. What a result looks like

1. The dev-side story, in one sentence a trader would recognise.
2. The signature and the cuts, each with why — lift, and trades still left.
3. Money at 95 ms: n, per day, mean, median, win, SOL, days-positive.
4. What it does not claim — families it misses, days it was not tested on.

It is not a keeper table, a comparison to a previous script's `n`, or an engine mapping of
something money has not passed.
