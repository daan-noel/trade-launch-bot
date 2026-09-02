# Copy trade — one target wallet, curve entry, both-venue exit

The strategy is one sentence: **a token is tracked from creation; while it is on the
bonding curve, if the target buys and every filter holds, we buy; when the target
sells — on the curve or on the AMM — we sell.** A token he never buys on the curve is
never entered.

It ships as vocabulary, not as a decision path. `entry_event`, `entry`, the array-form
exit DNF and the fingerprint `metric_config` carrier all exist; the only new thing is
one metric subject: *what a named wallet did on this token*.

Seeded by [`hunter/scripts/seed-copy-trade-rule.sql`](../../../scripts/seed-copy-trade-rule.sql),
pinned by `hunter/engine/tests/copy_trade_rule.rs`.

## The two groups

`m_copy` is the lifetime; `m_copy_window` is the same four quantities over a trailing
window. Both are fingerprint-scoped and read ONE list — `m_copy.target_wallets`.

| metric | meaning | unit |
| --- | --- | --- |
| `buy_sol` | SOL the listed wallets bought, **every leg** | SOL |
| `buy_count` | buy **transactions** — leg 0s, so a four-leg bundle counts once | count |
| `sell_sol` | SOL the listed wallets sold, every leg | SOL |
| `sell_count` | sell transactions — leg 0s | count |

`buy_sol / buy_count` is therefore SOL per transaction, not per leg. The split is
[`m_dump_ix`](metrics-reference.md#dump-builds-m_dump_ix--m_dump_ix_window)'s, and for
the same reason: every leg moves the price, but a decision is made once per
transaction.

**Scope: every print a listed wallet signed.** Curve and AMM, launch creates included.
The subject is what that wallet did, and a hidden exclusion inside the group would be a
second, invisible rule. Venue is the *engine's* business, not the metric's — see
"Curve-only is free" below.

## The trigger is the window; the lifetime is a filter

`m_copy.buy_count >= 1` **latches**. Once he has bought, it is true for the rest of the
token's life and the rule fires on every later print. A copy trigger is always
`m_copy_window` on a short window:

| window | what it fires on |
| --- | --- |
| `1p` | this print alone. Three split buys are three separate fires. |
| `1sl` | his whole slot. One fire for a split burst, sized by its total. |

Which of the two is right is a measurement, not a preference — `1p` is the fastest seat
and `1sl` is the one that can see a split buy's real size. Sweep both.

The lifetime group is where "he has already put 2 SOL in" and "he has not sold yet"
live. `m_copy.sell_count = 0` on entry is a deliberate **one-way door**: `sell_count` is
monotonic, so the upper bound in `= 0` permanently disarms the token the moment he
takes anything off. That is the intent — once he is out, this token is over for us.

## The rule

```json
"entry_lock": "slot",
"entry_event": {
  "m_copy_window": { "window_size_prints": 1, "buy_sol": [{"operator": ">=", "value": 0.5}] }
},
"entry": {
  "m_state": { "time": [{"operator": ">=", "value": 30}],
               "liquidity": [{"operator": ">=", "value": 10}] },
  "m_copy":  { "sell_count": [{"operator": "=", "value": 0}] }
},
"exit": [
  { "m_copy_window": { "window_size_prints": 1, "sell_sol": [{"operator": ">", "value": 0}] } },
  { "m_position": { "held": [{"operator": ">=", "value": 600}] } }
]
```

Extending it is adding a metric to `entry` — any existing group, in the terms it was
derived in. Two rules the shape depends on:

- **Floors only on a monotonic entry metric.** `m_state.time >= 30` is the age door.
  `time <= N` would permanently disarm the token at N seconds, and a target's entry
  ages run out past 15 minutes.
- **The exit needs a clause that does not depend on him.** The backstop clause
  (`m_position.held`, or a TP/SL) is what stops a target who never sells — or whose
  sell the feed misses — from stranding a bag.

## Curve-only is free, and the AMM exit costs nothing either

There is one admission lane: `TokenCreated`. Nothing else puts a mint in the cache, so
a token is only ever tracked from birth.

- `Event::Migrated` disarms an armed rule, so **no entry can fire after migration** —
  the "buy only on the bonding curve" half needs no venue term in the rule at all.
- An **open position rides migration out**; AMM trades keep pricing it, and
  `HeldPoolGate` keeps the PumpSwap pool subscribed for as long as a real position is
  unsettled. So the exit's `m_copy_window.sell_sol` reads his AMM sell without
  `track_post_migration` and without an ingest change.

`MAX_SNIPE_AGE_SECS = 30` is replay protection on `TokenCreated`, **not** a buy
deadline. Idle eviction is 45 minutes and exempts a token with an open position.

## The target list

One field on the fingerprint, compiled once at `RulesReloaded`:

```json
"metric_config": { "m_copy": { "target_wallets": ["<base58>"] } }
```

- **One rule per target.** A shared list would make one fire indistinguishable from
  another's, and the seat, the size gate and the exit are per-target questions.
- **The fingerprint is a wildcard.** A copy rule's selectivity is the wallet, not the
  token's creation axes; the fingerprint exists to carry the list.
- **List the target's own address.** The match is against the wallet the *venue*
  credited, so a router PDA there reads as hundreds of thousands of unrelated people
  (see the wallet-attribution rule in [hunter/CLAUDE.md](../../../CLAUDE.md)).
- **No list ⇒ NaN, never 0.** A rule pointed at an unconfigured fingerprint does
  nothing at all rather than buying everything: `NaN` fails `buy_sol >= 0.5` *and*
  `sell_count = 0`.

## Measuring it

The rule runs on the ONE kernel, so simulate is the measurement. The configuration:

- `fill_model = lag_115`, `cost_model = pumpfun_impact` — the defaults, and the only
  honest ones. A copy trade is a latency bet before it is anything else.
- `curve_only: true`. Offline there is no `Event::Migrated` (it is emitted by the live
  bin alone), so a corpus that carries AMM legs would fire entries on his AMM buys —
  a live/offline divergence, not a wider sample.
- **The number to read first: how many sim positions are still open at the token's
  last curve print.** That is the whole cost of `curve_only`. Small ⇒ ship it and treat
  the AMM sell arm as safety-only, unvalidated offline. Material ⇒ the lake needs a
  migration point per mint, `CorpusTrade` has to stop dropping `venue`, and the offline
  producer has to emit `Migrated` — in that order, before any AMM-exit number is worth
  reading.
- Positions closed at migration are their own bucket. Migration is a *success* event,
  so blending them into the book flatters it.

Then: a seat sweep at d = 0 / 1 / 2 slots behind his print, and a matched control on
non-target prints of the same tokens under the same gates. Never split the book into
"his tokens" and the rest — that is the selection artifact every prior wallet study
died of.
