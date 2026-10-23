# Streak Gate: implementation plan (draft, not approved)

A rule-level **gate** that blocks (or allows) buys by what the group's recent coins did. The
study side (method W5 steps 3 and 6, [fingerprint-rule-method.md](../plans/strategies/fingerprint-rule-method.md))
finds the values; this plan is how the engine, the rule format, simulate and the editor carry
them. Nothing here is built. The user decides after the main rules (A and B) are found; two
parts are open and marked **OPEN**.

## 1. What it is and why it is not a metric

A metric is one coin's state at one moment (`m_family.metric @tag [span]`,
[_!___metrics](../plans/strategies/_!___metrics.md)). The gate reads the group's **history of
finished coins**, so it is a new rule layer, not a metric and not a fingerprint axis (an axis
selects which coins belong to the group; a gate must be tunable per rule, because rule A and
rule B want different gates).

```
  Rule
    gates[]        (new)   group history  : "the last N settled coins all did X within M min"
    buy conditions         one coin, now
    sell lines             one position, now
```

## 2. Requirements (from the user)

1. **The event is an ordinary coin condition list.** Its time limits live inside the
   conditions (a span such as the first 30 s). The gate has no separate `at_age` field. A gate
   with no time limit is valid ("the peak of the last 3 coins was over 50 SOL").
2. **N and M are gate fields.** The gate is true when the condition list was true on N coins in
   a row, all launched within the last M minutes.
3. **Action:** `block` (no buy while the gate is true) or `allow` (buy only while it is true).
4. **The previous N coins are never computed at a new coin's launch.** That is wrong and too
   late. The gate is kept up to date as coins settle; the buy path reads one flag.
5. **Extensible.** A new event is a new condition list, not new code. A missing coin read is
   added to the metric system as a normal coin metric or span. More than one gate per rule
   (all must pass). General for any fingerprint.
6. **Live and simulate give the same decision on every coin** (parity).

## 3. Rule format

```json
"gates": [
  {
    "name": "dump streak",
    "action": "block",
    "last": 2,
    "within_min": 30,
    "when": [
      { "metric": "m_price.drop_from_peak_pct", "span": "<first 30 s>", "is": [{"operator": ">=", "value": 70}] },
      { "metric": "m_state.peak_liquidity_sol", "span": "<first 30 s>", "is": [{"operator": "<",  "value": 30}] }
    ]
  }
]
```

- `when` uses the rule condition format and the same compiled reads (section 8 of the metric
  doc), so two equal references share one buffer.
- The metric names and the span spelling above are placeholders; the real ones are those the
  registry has or gains (section 7).
- Gates sit in the rule's parameter JSON, so no table migration. Old rules have none.

## 4. When a coin settles

Each coin of the group carries a result slot while it is unsettled:

| state | when |
| --- | --- |
| **true** | the first moment every condition of `when` holds; stays true |
| **false** | the condition can no longer become true: every time limit in it has passed without it holding, or the coin ended (dead, migrated, silent) |
| **unsettled** | neither yet; the coin is not counted in the streak |

A condition with no time limit settles true when it holds and false only when the coin ends.
A settled coin leaves the per-trade read, so the work per coin is bounded.

## 5. Group state and the buy check

Per (fingerprint, gate): a bounded list of the most recent settled results, each with the
coin's launch time and result, plus the current run of trues and the launch time of the
oldest coin in that run.

- A coin entering the list updates the run in O(1).
- **Buy check, constant time:** the run is at least N, and the oldest of those N launched within
  M minutes of now (compared when the buy fires; no timer). `block` stops the buy while this is
  true; `allow` stops it while this is false.
- The streak resumes by itself: one false result, or the oldest coin leaving the M window.

## 6. Live and simulate

- **Same code path in both**, driven by the same event stream in time order.
- **Warm-up:** live primes the list at start from the DB (the group's coins from the last M
  minutes plus the longest time limit in `when`, replayed through the same settle logic). A
  restart must not open a gate that was closed.
- **Simulate** feeds every coin of the fingerprint's group in time order, not only the coins a
  rule trades, and starts earlier by the same warm-up span.
- **Lake path:** it has no coin-by-coin replay. A rule with a gate fails closed there and uses
  simulate (the same way `prior_launches` is unavailable on lake paths).
- **Parity test:** the engine's gate decision equals the study's on every coin of a week.

## 7. Metric system (extensibility)

- Gate events read existing coin metrics. A missing exact read (peak pool at a moment, drop
  from the peak inside the first X seconds) is added as a normal coin metric or span, with its
  one definition, under the extension rules in the metric doc, section 9.
- The gate layer adds no metric family.
- The registry gains no per-event or per-tag copy; the event is the condition list.

## 8. Editor (text image)

```
GATES (checked before every buy; all must pass)                     [+ Add gate]
Gate 1: dump streak                                  [on] [dup] [del]
  Action  (•) Block buys   ( ) Allow buys only
  When the last [ 2 ] coins of the group, launched within [ 30 ] min,
  ALL had this happen:
    m_price.drop_from_peak_pct  [first 30 s]   >=  70    [x]
    m_state.peak_liquidity_sol  [first 30 s]   <   30    [x]
    [+ condition]                       (the buy-condition picker)
  Settles: true when all hold; false after 30 s or when the coin ends
  Live now: last 2 = [x 8kQ.. 31s ago] [ok F2a.. 2m ago]   -> OPEN
  Backtest: skipped 70 coins, 2 migrated, saved 0.75 to 1.6 SOL
```

The condition rows are the same component as the buy conditions. The "Live now" strip shows the
last N results with their age and the gate's current state; the backtest line is the lab's gate
table for the rule's fingerprint.

## 9. OPEN: what "last N" is ordered by

- **By settle time** (the list grows in time order; the streak is "the last N coins that
  settled"). Simple and incremental. A coin that settles late is counted late.
- **By launch time** (the streak is "the last N launched coins"). Matches the study's current
  definition, but a slow-settling coin would have to be inserted in the middle, and a coin
  still unsettled blocks the streak or is skipped.

The user is not sure yet. The study uses launch order with a coin counted once its limit has
passed, which is close to settle order for a short time limit. The choice is made with the final
T, X and N from the real rules.

## 10. OPEN: live decision timing

- The gate value at the moment a buy fires may differ from the value at the coin's launch (a
  coin ahead may settle between them). Two ways: the flag is read **when the buy fires** (newest
  results), or it is **frozen at the coin's creation** (what the owner's session looked like at
  launch). The study measures the second (the last coins finished before the launch).
- A launch-time buy and a buy at 30 s see different flags under the first way. This must
  match the study's fills exactly (the finding sets the metric).

The user is not sure yet; decided together with section 9.

## 11. Cost and checks

- One bounded list per (fingerprint, gate); updates only when a coin settles. No new work on
  the per-trade path beyond reading the gate's conditions on unsettled coins.
- Tests: settle logic (true, false, unsettled, no time limit), run and window arithmetic,
  restart warm-up equals the uninterrupted run, simulate equals live, lake path fails closed.
- Docs: the rule format (`strategies`), the metric doc (no new family), the editor.

## 12. Order of work

1. Main study first: rules A and B, final T, X, N from the rule loop (method W5 step 6).
2. Settle sections 9 and 10 with the user.
3. Approve this plan, then build: rule format and settle logic, group list, simulate feed,
   warm-up, editor, parity test.
