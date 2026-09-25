# Launch-door rule: shipping rule v2 in the engine

The rule in [the evidence ledger](../plans/strategies/_!___evidence.md) 3.1 and 4.5
(door = launch build whose recent tokens keep running, event = the second independent
buyer after the launch scramble, permission = the creator has not sold, exit = armed
trail 10 / 20 / 1200) needs **two** additions before it is authorable in the ONE kernel:
a door feed that stamps two creation-time axes, and one anchored crowd metric. Nothing
here is a second decision path, and the exits, the census gates and the permission need
no engine change at all.

Governing rules: the finding sets the metric (root `CLAUDE.md`); ONE decision kernel
([arch/strategies.md](../arch/strategies.md)); an axis is one `AxisDef` plus one reader
arm ([fingerprint-ranges.md](../plans/strategies/fingerprint-ranges.md)).

## 1. What the study's terms mean, checked against its own exports

Every row below is re-derived from `w_tok.csv` / `w_fires.csv` / `w_fires_v.csv` /
`w_daily.csv`, on the shipping door (previous day, 20+ launches, 10 %+ runner rate),
creator not sold, trail 10 / 20 cap 1200, 0.2 SOL, slot-end fill.

| check | result |
| --- | --- |
| the headline book reproduces | 1,498 fires, +38.55 SOL, +12.87 % a trade, 9 of 9 periods, worst +0.25, forward +8.54 |
| the permission is load-bearing | creator-sold entries read +1.53 % a trade, 5 of 9 periods, **-4.70 SOL forward** |
| the bundler exclusion does anything | **no** - zero bundler-group fires in any of the three doors, so the term is dropped |
| the event needs the machine unit | **no** - second distinct WALLET reads +39.67 SOL / +12.95 % / 9 of 9 / forward +9.18, at or above the machine unit |
| the money needs exits into silence | **no** - exits landing on an observed print are 646 fires, +28.36 SOL, +21.95 % a trade, 8 of 9; the 852 silence exits add +10.19 SOL at +5.98 % |
| did the walk-forward cover the shipping door? | **no** - its grid held the hourly and all-history soft doors plus a daily door on the old wall label, never the daily soft door |

Those shape the build. The wallet unit removes a new hasher from the plan entirely, and
the silence split says the edge survives the exits the engine is least able to reproduce.

**The walk-forward gap is now closed** (`scratchpad/walkfwd.py`, `walkfwd2.py`). Re-running
the study's own walk-forward with the daily soft door present, over 11,050 configurations
(17 doors x 5 permissions x 13 events x 10 exits), selection still costs nothing: 32.81 SOL
graded forward against 33.21 for the final pick in-sample on the same periods, and 34.47
for the best single configuration in hindsight.

Constraining the grid to daily doors alone - what the engine wants, since it makes the door
one query a day and a creation stamp that cannot move intraday - **costs 0.29 SOL over
seven graded periods, 0.9 %**. The daily-only walk-forward then picks the *same*
configuration on every period, which is the strongest available evidence that selection is
not doing the work:

| | door | event | exit | fires | SOL | per trade | periods positive | worst |
| --- | --- | --- | --- | ---: | ---: | ---: | ---: | ---: |
| **what ships** | daily, 20+ launches, **8 %+** runners | second buyer, gate **5 s** | trail 10/20 cap 1200 | 1,982 | **+43.72** | +11.03 % | 9 of 9 | +0.84 |
| the doc's box | daily, 20+, 10 %+ | second buyer, gate 10 s | trail 10/20 cap 1200 | 1,498 | +38.55 | +12.87 % | 9 of 9 | +0.25 |

Two terms move, and both move **inside plateaus the study already measured** (8-10 % on
20+ launches; the 5 / 10 / 20 s gates reading alike), so this is picking the middle of a
flat region rather than a spike - and the walk-forward chose it without seeing the future.

## 1b. The unarmed exit is a leak, and the census closes part of it

Splitting that book by whether the trail ever armed (`scratchpad/v3_sim.py`, `v3_search.py`,
`v3_final.py`):

| | fires | SOL |
| --- | ---: | ---: |
| the trail armed (price reached +10 %) | 363 | **+78.69** |
| never armed - held to the 1200 s cap with no stop at all | 1,619 | **-34.96** |

Four fifths of entries never arm, and the trail line reads only once the position is up the
arm gate (the `armed` stage), so those positions carry no stop and the median hold is the
cap itself. A **-25 % hard stop that applies only while unarmed** recovers 5 SOL of that; it
is not a full fix, because on a curve silence freezes the price and holding to the cap is
often no worse than stopping out. The rest of the fix is not taking those entries.

My exit simulator is validated per fire against the study's on the shape both share:
3,821 fires, maximum difference 0.00 SOL. Every number below comes off that simulator.

Selection is then made twice, each time on the **seven in-sample periods only**, with one
stated objective, and the two forward periods read once at the end
(`scratchpad/v4_honest.py`, `v4b.py`; grid = 17 exits x 232 gate sets = 3,944
configurations).

| rule | fires | SOL | per trade | periods | worst | win | median hold | forward |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| v2 as it stands | 1,982 | +43.72 | +11.03 % | 9 of 9 | +0.84 | 31.0 % | 1200 s (the cap) | +7.58 |
| **most money**: v2 plus the -25 % unarmed stop, **no gates** | 1,982 | **+46.18** | +11.65 % | 9 of 9 | +0.90 | 30.1 % | 235 s | +8.35 |
| **best per trade**: plus buy share 70 %+ over 15 s, depth 40+, no racer/bundler/farm buy before entry | 478 | +27.55 | **+28.81 %** | 9 of 9 | +0.65 | **44.8 %** | 51 s | +4.98 |

**The gates do not add total money, and saying otherwise was my error.** Walk-forward on
the money objective is unstable and keeps returning to no gates at all; it grades 25.48 SOL
against 34.72 for its own final pick in-sample, so that grid is genuinely overfittable. An
earlier draft of this section named a gated rule "strictly better" - that rule was chosen by
reading the forward column, and honest in-sample selection does not choose it.

**On the per-trade objective the gates do survive.** That walk-forward converges from the
third period onward on exactly the three gates above and grades **19.57 % a trade on 483
forward fires**. So the gates convert money into rate and safety rather than creating money:
a quarter of the trades, two and a half times the per-trade return, a 45 % win rate, and a
51 s median hold instead of 20 minutes.

Robustness: the money rule loses a period without `c8dd1d79` (13 % of fires, +31.10, 8 of
9). The rate rule leans harder on it (47 % of fires) but still reads +13.36 and 9 of 9
without it.

**Where the census earns its place.** As an *event* it does not: waiting for a terminal, or
for a second non-noise machine, reads worse than the plain second-machine arrival. As a
*filter* it does. A racer arriving is worth nothing - 284 such fires, -0.32 SOL, positive in
2 of 9 periods - and of every single gate tried, "the arrival is not a racer" is the best
one for money (+46.50, worst +0.98) while "no racer, bundler or farm bought before entry" is
the best for the floor (+1.27) and holds its rate forward (+13.95 %). Both margins over no
gate at all are small enough to sit inside noise on this sample, which is why the money rule
above carries no gate. The list is shippable either way: 100 build identities cover 91 % of
noise-role buys and 200 cover 96 %, an ordinary tag of exact `ix_shape`s.

The thresholds below are the money rule; the three gates are the safety variant.


## 2. The two rules, authored

Both rules share one fingerprint (the door), one event, one permission and one size.
They differ in three entry gates and in their exit lines. Everything below is a JSON
`params` bag on a `strategy_rules` row (size in its `buy_amount_lamports` column) plus a
`fingerprints` row, with no code branch of its own.

| study term | engine term | status |
| --- | --- | --- |
| DOOR: launch build (exact ordered creation `ix_labels`) whose previous-day tokens (20+) reached curve peak reserve 60 with the peak 60 s or more after birth, at 8 %+ | fingerprint axes `build_launches >= 20`, `build_runner_bps >= 800`, stamped at `TokenCreated` from the day's door snapshot | **new** (section 4) |
| EVENT: the first buy at age 5 s or later by the second distinct buyer other than the creator | `enter.event: m_crowd.buyer_count [age5s] >= 2`, `enter.lock: "slot"` | **new metric** (section 3) |
| EVENT: no later than 300 s | `enter.filters: m_state.age_sec <= 300` | exists |
| PERMISSION: the creator has not sold | money rule: tag `volume = {"match": {"creator": true}}` (not `sticky`), `m_flow.sell_count @volume = 0`. Rate rule (whose `volume` holds the noise list): tag `dump = {"match": {"creator": true}, "side": "sell"}`, `m_flow.sell_sol @dump <= 0` | exists (the `creator` matcher) |
| PERMISSION: when the creator field never trades, the creator is the creation-slot first buyer | creator-hash fallback resolved at `FirstSlotSettled` (section 5) | **new**, small |
| GATE (rate rule): buy share 70 %+ over 15 s | `m_flow.buy_share_pct [15s] >= 70` | exists |
| GATE (rate rule): curve depth 40+ vsol | `m_state.liquidity_sol >= 10` (real reserves = `vsol - 30`) | exists |
| GATE (rate rule): no racer / bundler / farm buy before entry | the noise-build list as tag `volume = {"match": {"ix_shape": [...]}}` (no `creator`, not `sticky`), read as `m_flow.buy_count @volume = 0` | exists |
| EXIT: armed trail 10 / 20, a -25 % stop while unarmed, cap 1200 s | stages `start` / `armed` plus an `always` cap line | exists |
| SIZE 0.2 SOL, one entry per token | the row's `buy_amount_lamports = 200000000`, no TP/SL, no `reentry` | exists |

**Two tags, never one list.** The rate rule reads the creator (the sell permission) and the
noise builds (the buy gate). They cannot share a tag: tagging both makes
`m_flow.buy_count @volume = 0` mean "no noise buy AND no creator buy", and the creator's own
launch buy is nearly always there, so that gate blocks every entry.

The money rule has no noise list and keeps the exact study spelling on `volume`:
`{"match": {"creator": true}}`, not `sticky`, with `m_flow.sell_count @volume = 0`. That counts
EVERY leg of a sell, which is what the study's `cr_sold` counts.

The rate rule puts the noise list on `volume` and the permission on a second tag, `dump`
(`{"match": {"creator": true}, "side": "sell"}`). Two things hold for that permission:

* **A tag with no matcher that fires is not a permission.** `ix_shape: []` alone tags
  nothing, `m_flow.sell_sol @dump` reads 0 on every token, and `<= 0` passes everything - a
  gate that looks like a permission and is not one. The `creator` matcher is what makes it one.
* **Prints, not transactions.** `m_flow.sell_tx_count` counts transactions (leg 0s);
  `m_flow.sell_count` and `m_flow.sell_sol` count every leg. A creator selling as leg 2 of a
  bundle is caught by one and missed by the other. So the rate rule spells the permission
  `m_flow.sell_sol @dump <= 0` - SOL over every matching leg, `<= 0` rather than `= 0`
  because `=` on `m_flow.sell_sol` carries a 0.1 SOL tolerance (the registry's `eq_tolerance`).

The two rules therefore take **two fingerprint rows**: identical `criteria`, different
`tags` (which is row identity anyway). Section 9's gate runs the money rule under both
spellings and diffs the rejection sets, which is the only thing that proves they mean the
same permission.

**The fallback if the noise gate is not worth it.** Dropping it leaves the rate rule on two
gates, both `m_state` / `m_flow`, with the money rule's `volume` permission: 625 fires,
+28.12 SOL, 22.50 % a trade, 9 of 9 periods, worst +0.42, win 43.5 %. The noise gate is worth
0.57 SOL and 6.3 pp a trade over that, on 147 fires.

`TokenTrack::seed_creator` re-seeds the creator on every tag, which is also what makes the
section 5 fallback reach the permission at `FirstSlotSettled` without a second code path.

### 2a. The money rule

```json
{
  "enter": {
    "event":   [ {"metric": "m_crowd.buyer_count", "span": "age5s", "is": [{"operator": ">=", "value": 2}]} ],
    "filters": [ {"metric": "m_state.age_sec", "is": [{"operator": "<=", "value": 300}]},
                 {"metric": "m_flow.sell_count", "tag": "volume", "is": [{"operator": "=", "value": 0}]} ],
    "lock": "slot"
  },
  "always": [
    {"if": [{"metric": "m_position.held_sec", "is": [{"operator": ">=", "value": 1200}]}], "sell": true}
  ],
  "stages": [
    {"name": "start", "on": [
      {"if": [{"metric": "m_position.retrace_pct", "is": [{"operator": ">=", "value": 20}]},
              {"metric": "m_position.pnl_pct",     "is": [{"operator": ">=", "value": 10}]}], "sell": true},
      {"if": [{"metric": "m_position.pnl_pct", "is": [{"operator": "<=", "value": -25}]}], "sell": true},
      {"if": [{"metric": "m_position.pnl_pct", "is": [{"operator": ">=", "value": 10}]}], "go": "armed"}
    ]},
    {"name": "armed", "on": [
      {"if": [{"metric": "m_position.retrace_pct", "is": [{"operator": ">=", "value": 20}]}], "sell": true}
    ]}
  ]
}
```

`start` is the unarmed position and `armed` the armed one: the trail sells only in `armed`,
and the -25 % stop only in `start` - the leak fix, and the only stop an unarmed position has.
A `go` takes effect from the next print, so `start` also reads the trail on the print that
crosses +10 % (its first line), where the study already counts the trail as armed. The
1,200 s cap is an `always` line, read in both stages.

### 2b. The rate rule

Same bag, plus three entry gates and a tighter exit:

```json
  "enter": { "filters": [
    {"metric": "m_state.age_sec",       "is": [{"operator": "<=", "value": 300}]},
    {"metric": "m_state.liquidity_sol", "is": [{"operator": ">=", "value": 10}]},
    {"metric": "m_flow.sell_sol",  "tag": "dump",   "is": [{"operator": "<=", "value": 0}]},
    {"metric": "m_flow.buy_count", "tag": "volume", "is": [{"operator": "=",  "value": 0}]},
    {"metric": "m_flow.buy_share_pct", "span": "15s", "is": [{"operator": ">=", "value": 70}]}
  ] },
  "stages": [
    {"name": "start", "on": [
      {"if": [{"metric": "m_position.retrace_pct", "is": [{"operator": ">=", "value": 20}]},
              {"metric": "m_position.pnl_pct",     "is": [{"operator": ">=", "value": 5}]}], "sell": true},
      {"if": [{"metric": "m_position.pnl_pct", "is": [{"operator": "<=", "value": -25}]}], "sell": true},
      {"if": [{"metric": "m_position.held_sec", "is": [{"operator": ">=", "value": 180}]},
              {"metric": "m_position.pnl_pct",  "is": [{"operator": "<",  "value": 5}]}], "sell": true},
      {"if": [{"metric": "m_position.pnl_pct", "is": [{"operator": ">=", "value": 5}]}], "go": "armed"}
    ]},
    {"name": "armed", "on": [
      {"if": [{"metric": "m_position.retrace_pct", "is": [{"operator": ">=", "value": 20}]}], "sell": true}
    ]}
  ]
```

The third `start` line is the `bail180` term: a position that has not armed by 180 s is
closed; its `pnl_pct < 5` keeps it off the print that arms. The `always` cap keeps closing a
position that armed and then went quiet.

## 3. `m_crowd.buyer_count [age5s]` - the one new metric

A new span kind, because the subject (how many distinct buyers this token has drawn since
an age anchor) has no basis among the trailing windows: `m_crowd.unique_wallets` is
trailing, counts every trade rather than buys, and counts the creator.

| | |
| --- | --- |
| quantity | `m_crowd.buyer_count` - distinct wallets, other than the creator, that have BOUGHT this token at or after the anchor. Count. |
| span | since age, `age<N>s` (`age5s` here) - the anchor. Buys before it never enter the set. |
| state | one small wallet set per anchor on `TokenTrack` (`AgeAnchor`, `metrics/crowd_after_age.rs`); two rules on one anchor share it |

**The creator exclusion is definitional, not a param.** The creator's own launch buy
is not somebody arriving, and `m_crowd.unique_wallets` remains the count-everyone
reading. One fewer param is one fewer dedup axis on the hot path.

**The anchor cannot be replaced by an `m_state.age_sec >= 5` filter.** That filter
would let a set seeded during the launch scramble satisfy `>= 2` at the first print
past 5 s on nearly every token, which is a different rule with a different book. The
finding sets the metric.

### 3a. The set is capped, and the cap is derived

A rule only ever asks whether the count has reached a threshold, so `cap` is the
largest value any loaded condition on `m_crowd.buyer_count` names under this anchor, plus
one. Once the set holds `cap` wallets it stops inserting and the metric reports `cap`.
Every operator stays exact at that cap: `>= 2` passes, `<= 5` fails at `cap = 6`,
`= 3` fails at 6. A hot token drawing 3,000 buyers costs three entries, not three
thousand, and the set is a `SmallVec<[u64; 4]>` with a linear scan rather than a hash
set - at a cap of 3 the scan is cheaper than the hash.

Without the derived cap this is the one part of the design that grows without a bound,
which is why the cap is part of the metric and not an optimisation.

Cost per buy on an armed token: one compare against the creator hash, one compare of
age against the anchor, and a scan of at most `cap` entries while the set is open.
Nothing at all once it closes, and nothing on a sell. No tick work, so no
`ClockHorizons` field: the metric can only move on a print.

## 4. The door feed

### 4a. Two columns: the curve peak and when it happened

The label is the token's **maximum curve reserve** and the **time of that maximum**,
not a first crossing. The distinction is the whole point of the term: a token that
touches reserve 70 at 30 s and fades has an early peak and is not a runner, while one
still climbing at 200 s is. `tokens_info` carries neither, and its `ath_price` /
`ath_timestamp` pair cannot stand in: it is a price over **every** print, so a
migrated token's peak can be an AMM high minutes after the curve was done.

So the cache latches the term itself, on curve prints only:

* `TokenState.curve_peak_reserve_sol: Option<f64>` and `curve_peak_at:
  Option<DateTime>` - the priced reserve (`vsol`) of the highest curve print and its
  block time. One compare per curve print, a field write only on a new high.
* Both flushed through the existing `TokenMetricsWrite` upsert. No new write op.
* Migration `00NN_curve_peak.sql` adds the two columns. EC2 before the next
  `db-incremental-sync.ps1` (the sync copies `tokens_info`). Backfilled once on the
  workstation from `trades`, never on the server.

`RESERVE_RUNNER_SOL = 60.0` and `RUNNER_MIN_AGE_SECS = 60` live beside
`PUMP_GRADUATION_REAL_SOL` in `token_math.rs`, never as literals.

### 4b. `launch_build_doors` (one bounded query a day, off the loop)

```sql
CREATE TABLE launch_build_doors (
    day         DATE    NOT NULL,   -- the door as it stands at 00:00 UTC of this day
    ix_labels   JSONB   NOT NULL,   -- exact ordered creation labels = the build
    launches    INTEGER NOT NULL,   -- tokens of this build created on day - 1
    runners     INTEGER NOT NULL,   -- of those: curve peak >= 60 reserve, peak 60 s+
                                    -- after birth, and the peak happened before `day`
    PRIMARY KEY (day, ix_labels)
);
```

One repo fn (`LaunchDoorRepo::compute_day(day)`) is the only SQL that fills it:
`tokens` LEFT JOIN `tokens_info` over one day of creations, `GROUP BY ix_labels`.
About 25k rows, never `trades`. The build key is hashed at load with the engine's
`trade_keys::ix_hash` over the stored labels, so the hasher exists in one place and SQL
never re-implements it.

* **lab first**: simulate reads the rows for every day of its corpus and computes the
  days the table lacks through the same fn, so a past day is graded on the door the
  live engine would have had that morning. This is all the reconciliation gate needs.
* **live after the gate**: a `door_refresh` task beside `reload_scheduler`. At boot it
  loads today's row set (computing it if absent), then at 00:00:30 UTC each day
  computes the new day, inserts it (`ON CONFLICT DO NOTHING`, so a restart never
  rewrites a day the engine already traded on) and hands the map to the loop as
  `EngineCommand::ReloadDoors`. A failed refresh keeps the previous map and logs at
  error level, the same contract as an unprimed `prior_launches`.

### 4c. Two axes on `TokenFingerprint`

| axis | unit | value |
| --- | --- | --- |
| `build_launches` | count | `launches` for this token's build in the current door map |
| `build_runner_bps` | bps (new `AxisUnit::Bps`, shown as %) | `runners * 10000 / launches` |

Both `Option<u32>`, `None` when the build is absent from the map, which fails a
configured axis closed - an unseen build never arms. Stamped in `reduce` at
`TokenCreated` before `match_all` runs, exactly where `prior_launches` is stamped:

```rust
let build = trade_keys::ix_hash_opt(&tf.ix_labels);
if let Some(door) = build.and_then(|h| state.build_doors.get(&h)) {
    tf.build_launches = Some(door.launches);
    tf.build_runner_bps = Some(door.runner_bps);
}
```

`EngineState.build_doors` is replaced whole by `Event::BuildDoorsReloaded`. The stamp
is creation-time only, so no `cross_epoch` bump: a token keeps the door it was born
under, which is the study's term. The event is recorded to the event log like
`RulesReloaded`, so boot recovery re-stamps a replayed creation under the map that was
live at the time.

Mirrors that move in the same commit: `AXES` and `AxisId::ALL`, the TS
`fingerprintAxes.ts` table and its `AxisUnit`, the dashboard SQL mirror in
`fingerprint_axes.rs`, `observed_axes` (both `None`), the lake loader (`None`, so a
door rule is simulate-only and not sweepable - recorded in `sim-parity.md`), and the
axis guard tests.

## 5. The creator fallback

The study identifies the creator as the wallet on the creation event, and falls back
to the **first buyer of the creation slot** when that wallet has no trading identity
at all (the `5ix:Transfer` client). Live cannot know "never trades anywhere", so the
engine uses the observable form: if the creator's wallet has not bought this token by
the time the creation slot settles, `FirstSlotSettled` re-points the creator hash at
the first creation-slot buyer, through the existing `TokenTrack::seed_creator` (which
already re-seeds every `FlowState`). That settlement point already runs before any
entry, so the fallback costs one comparison per token and no new event.

This is a **definition change to an existing term**, and it moved the old rule's
result, so the reconciliation gate confirms the engine and the study resolve the same
wallet on the same tokens - not merely that the money agrees.

## 6. Workflow

```
 INGEST                                       DAILY 00:00:30 UTC, off the loop
 +----------------------------------+         +-------------------------------------+
 | curve print -> new reserve high? |         | LaunchDoorRepo::compute_day(D)      |
 |   curve_peak_reserve_sol         |  batch  |   tokens x tokens_info, created D-1  |
 |   curve_peak_at            (NEW) | ------> |   GROUP BY ix_labels                |
 +----------------------------------+ upsert  |   launches, runners                 |
                                              |     (peak >= 60 reserve, peak 60 s+ |
                                              |      after birth, peak before D)    |
                                              +------------------+------------------+
                                                                 | INSERT launch_build_doors
                                                                 v
                                              Event::BuildDoorsReloaded
                                                                 |
 ENGINE, the one serialized loop                                 v
 TokenCreated --> ix_hash(creation ix_labels) --> stamp build_launches, build_runner_bps
             --> fingerprint match:  build_launches   >= 20
                                     build_runner_bps >= 800
                     |
                     | armed: ~1-2k tokens a day. The other ~24k never enter state.tokens.
                     v
 FirstSlotSettled --> creator hash falls back to the creation-slot first buyer
                      when the creator never bought
                     |
 each BUY    --> m_crowd.buyer_count [age5s]  (creator compare, age compare,
             |                             scan of <= 3 while the set is open)
 each SELL   --> creator match (tag volume or dump)  (one wallet-hash compare)
             --> m_flow.buy_share_pct [15s]  (rate rule only)
                     |
 enter.event:   m_crowd.buyer_count [age5s] >= 2       lock: slot
 enter.filters: creator has not sold  AND  m_state.age_sec <= 300
                (rate rule also: liquidity_sol >= 10, buy_share_pct [15s] >= 70,
                 m_flow.buy_count @volume = 0 over the noise-build list)
                     |
                     | SubmitBuy 0.2 SOL   ~240 fires a day (money) / ~60 (rate)
                     v
 Holding --> start:  pnl_pct <= -25 -> sell;  pnl_pct >= 10 -> go armed
             armed:  retrace_pct >= 20 -> sell
             always: held_sec >= 1200 -> sell   |   Dead
```

## 7. What it costs on the hot path

| path | added work |
| --- | --- |
| ingest, per curve print | one `f64` compare; two field writes on a new reserve high |
| decision loop, `TokenCreated` | one FNV hash over the creation labels plus one map get (about 100 ns against the 461 us a creation already costs) |
| decision loop, per print | zero for tokens the door does not arm; for armed tokens two compares and a scan of at most `cap` entries on a BUY while the set is open, plus the existing creator compare |
| decision loop, tick | nothing new - `m_crowd.buyer_count` cannot move on a tick, so no `ClockHorizons` field and the settled-tick skip is untouched |
| PG | one `GROUP BY` over one day of `tokens` once a day, off the loop; no `trades` scan anywhere |
| RAM | the door map, a few hundred entries; at most `cap` wallet hashes per armed token per anchor |

The door is what keeps all of this small: it arms roughly 1-2k of the ~25k daily
creations, and every per-print cost above is paid only by an armed token.

## 8. Order of work

1. `m_crowd.buyer_count` on the since-age span: the anchor, the capped set, the registry
   walk test, the TS registry mirror. Test: two rules on anchors 5 and 30 read different
   values on one token; a token drawing 3,000 buyers holds `cap` entries.
2. `curve_peak_reserve_sol` / `curve_peak_at`: cache latch, upsert columns, migration,
   workstation backfill. Test: a path peaking at reserve 70 at 30 s records 30 s, not
   the later 65-reserve print; an AMM print never moves either field.
3. `launch_build_doors` + `LaunchDoorRepo::compute_day`. Test: the fn over a fixture
   day reproduces the study's door rows for that day (`scratchpad/w1.sql`).
4. Engine: the two axes, `BuildDoor`, `BuildDoorsReloaded`, the stamp, event-log round
   trip, axis guard tests, TS mirror. Creator fallback at `FirstSlotSettled`.
5. Lab: simulate loads per-day doors; sweep records the axes unsupported.
6. **Reconciliation gate** (section 9). Only after it passes:
7. Live `door_refresh` task; boot order = prime `prior_launches`, load doors, reload
   rules, recover.
8. Paper on EC2 for a week, then the operator sizes it. Set `max_concurrent_tokens`
   first: about 240 fires a day holding a median 235 s is roughly 1 open at once on
   average, but the daily fire count swings tenfold across the sample, so the cap and
   the SOL commit are sized off the busy day, not the mean.

## 9. The reconciliation gate: what it measured

Both rules are authored and simulated. The window is **2026-08-31 .. 2026-09-07**,
where the sealed lake ends; the study's book is recomputed over the same window and
on the engine's own door and wallet unit, so the two sides share a universe before a
single SOL figure is compared (`scratchpad/wallet_target.py`).

Run as: `fill_model: slot_end`, `cost_model: pumpfun_impact`, `curve_only: true`,
0.2 SOL, copycat guard off.

### The controls, in the order they were checked

**1. The door, day by day.** `curve_peak_reserve_sol` / `curve_peak_at` backfilled
from `trades` for 08-30..09-06 (~152k tokens), then `launch_build_day_stats` computed
for eight days (121-170 builds a day, 7-14 of them open).

| check | result |
| --- | --- |
| peak value vs the study's own peak table | 21,903 / 21,906 equal |
| runner LABEL (peak >= 60 reserve, 60 s+ after birth, before the day) | **21,906 / 21,906 equal** |
| DOOR VERDICT per token | **14,550 / 14,571 equal** |

The 21 disagreements are one build (`dd493bac`) where the engine counts 22 launches
and the study 3: the study's door was computed over its own filtered universe, which
truncates a build's count. The engine's count is over the whole `tokens` table, which
is what live will use.

**2. The creator, token by token.** All 353 tokens where the engine first refused to
fire carry the SAME creator wallet as the study. The disagreement was the FALLBACK,
not the identity - see the fix below.

**3. The fire set, before any money.**

| | fires |
| --- | ---: |
| target (study terms, engine door, wallet unit) | 1,772 |
| engine | 3,165 |
| **shared** | **1,772** |
| target-only | **0** |
| engine-only | 1,393, of which 1,273 (91 %) are tokens the study never simulated |

**4-6. The money, on the shared 1,772.**

| exit | fires | engine | target | gap | engine hold | target hold |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `retrace_pct >= 36` | 744 | +33.79 | +33.13 | +0.65 | 62.5 s | 63.0 s |
| `pnl_pct <= -43.75` | 231 | -22.74 | -22.46 | -0.28 | 14.0 s | 14.2 s |
| `held_sec >= 1200` | 145 | +54.00 | +53.34 | +0.67 | - | 1200 s |
| `Dead` | 652 | -21.39 | -21.34 | **-0.06** | 184 s | 1200 s |
| **total** | **1,772** | **+43.66** | **+42.67** | **+0.99 (2.3 %)** | | |

**98.1 % of fires agree within 0.01 SOL** (median absolute difference 0.0006 SOL),
and every one of the seven days agrees in sign and to within 0.4 SOL. The gap splits
into two measured parts: the fee basis below carries **+0.55**, and **17 fires (1.0 %)**
carry **+0.38** of the rest - sixteen of them `Dead`, where the engine cuts a silent
token at a median 184 s and the study rides it to the 1200 s cap, and one `retrace_pct`
fire that arms on a different print. On the two exits that need no deadness call the
books are equal to the SOL: `pnl_pct <= -43.75` and `held_sec >= 1200` both reconcile to
**0.0000** after the fee basis.

**7. The rate rule, against its own target.** The same construction, carrying the rate
exit (arm 5 %, trail 20 %, unarmed stop 25 %, unarmed bail 180 s) and the two gates the
engine ships, with `buy_share` and `depth` recomputed at the WALLET fire print rather
than read off the machine-unit row (`scratchpad/rate_target.py`):

| | fires | SOL |
| --- | ---: | ---: |
| target | 554 | +28.51 |
| fee-basis prediction | | +28.86 |
| **engine** | **554** | **+28.86** |
| target-only | **0** | |
| engine-only | 1,003, of which 963 the study never simulated | |

**Residual after the fee basis: -0.00 SOL.** 99.1 % of fires within 0.01 SOL, the
largest single-fire difference 0.017, and every exit reason exact to 0.0000 except six
`Dead` fires at -0.005. The rate rule reconciles to the fee basis and nothing else.

### The two books beside the python backtest as published

The target above is the study's own terms recomputed on the engine's door and wallet
unit. The published python book is a different universe over a longer window, so the
ladder moves one term at a time and each rung's cost is visible:

| | MONEY fires | MONEY SOL | RATE fires | RATE SOL |
| --- | ---: | ---: | ---: | ---: |
| python, as published (own universe, 9 periods) | 1,982 | +46.18 | 478 | +27.55 |
| restricted to the engine's window, 08-31..09-06 | 1,719 | +42.57 | 422 | +26.90 |
| + the engine's door, and lake tokens only | 1,719 | +42.57 | 422 | +26.90 |
| + the WALLET event unit (**= the target**) | 1,772 | +42.67 | 554 | +28.51 |
| **ENGINE, the same fires** | **1,772** | **+43.66** | **554** | **+28.86** |
| ENGINE, the full door population | 3,165 | +42.14 | 1,557 | +31.57 |

The window costs 8 % of the money book and 2 % of the rate book, purely because it is
seven days instead of nine. The door and lake rungs cost **nothing**, which is itself a
control: every python fire inside the window is already an engine-door token present in
the lake. The event unit is worth +0.10 SOL on the money rule and +1.61 on the rate
rule, both in the study's own direction - it measured the wallet unit at or above the
machine unit before either was authored.

### Two implementation errors the gate caught

* **Exit thresholds are stated in vsol, not price.** The study's arm / trail / stop
  are percentages of the priced reserve; price is `vsol^2 / k`, so every one of them
  squares. Authored as price percentages the rule was a far tighter rule wearing the
  same name: `-25 %` became `-43.75 %`, `20 %` became `36 %`, `+10 %` became
  `+21 %`. Before the fix the shared book read -2.61 SOL against target with the
  unarmed stop firing at a 7 s median instead of 116 s; after it, 14.0 s against
  14.2 s. **A threshold derived on the reserve is squared into the price basis -
  there is no other correct conversion, and the error is silent.**
* **The creator fallback was too wide.** "The creator did not buy in its own creation
  slot" swaps the creator on a fifth of all launches; the study's term is "the creator
  field has no trading identity at all". The observable twin is narrower: fall back
  ONLY when the creation event carries no creator wallet. Cost of the wide version,
  measured: **353 of 1,772 fires lost**, every one a token whose real creator never
  sold.

* **A whole term was missing: the study's universe excludes bundler-group
  creations.** `w_sel` is built `WHERE NOT bundler_group` - creation transactions whose
  `cgroup` (instruction count plus terminal instruction) is `3ix:Buy`, `4ix:Buy` or
  `3ix:BuyExactSolIn`. Because the exclusion is applied when the universe is BUILT, the
  study's own check that the term does nothing ("zero bundler-group fires in any of the
  three doors", section 1) is vacuous: there were none to find. Left out, the engine
  fired on 1,164 extra money-rule tokens booking **-1.42 SOL** and turning a 7-of-7 book
  into 6 of 7 with a -2.14 SOL day. The term needs no engine change - the creation
  transaction IS the first buy print, so a tag `bundler` holding those 15 exact `ix_shape`s
  (no `creator`, not `sticky`) makes `m_flow.buy_count @bundler <= 0` read exactly "not
  launched by a bundler build". Restoring it reproduces the offline prediction to the fire: 2,001
  fires and +43.56 SOL, predicted 2,001 and +43.56.

### One term that does not survive, and is therefore not shipped

The rate rule's third gate - *no racer / bundler / farm buy before entry* - counts
noise buys among the arrivals at **5 s or later**. `m_flow.buy_count @volume` counts from
birth or over a trailing window, and **93.0 % of fires carry a noise buy inside the first 5 s**
(the launch scramble), against 17.5 % that carry one after it. Authored as
`m_flow.buy_count @volume = 0` the gate rejected 96 % of the book (71 fires, 5 of 5
periods that produced any).

So the gate is **deferred, not approximated**. What it needs is `m_flow.buy_count @volume`
on the **since-age** span (`[age5s]`) this work already introduced for
`m_crowd.buyer_count` - the span kind, its `AgeAnchor` dedup key and its read path all
exist, so the extension is one more accepted span on an existing quantity (a registry
change plus its compute arm) rather than a new basis. The 234
expanded raw noise sequences are ready on fingerprint
`77777777-7777-4777-8777-777777777777`.

### The books, on the population the rules will actually trade

The engine runs the whole market inside the door - 7,797 tokens in the window against
the 4,150 of them the study's table carried - so the population is the rules' own, not
a sample of it:

| rule | fires | SOL | per trade | days positive | worst day | win | median hold |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| MONEY (no gates) | 2,001 | **+43.56** | +10.89 % | 7 of 7 | +0.53 | 30.3 % | 90 s |
| RATE (buy share 70 %+ / 15 s, depth 40+) | 629 | +30.74 | **+24.44 %** | 7 of 7 | **+1.95** | **44.0 %** | 43 s |

Against the same seven days of the python book - 1,719 fires +42.57 at 12.38 %, and
422 fires +26.90 at 31.87 % - the engine reproduces both rules on a population nearly
twice the size. The study's own ranking holds: the money rule takes three times the
trades for more total SOL, the rate rule converts that into per-trade return and a
floor. Neither has a losing day.

The depth gate is what buys the rate rule its floor: 7 `Dead` exits against the money
rule's 494, because `m_state.liquidity_sol >= 10` keeps the position out of tokens that are already
dead-eligible when it enters.

### Divergences still open, each now with a number

* **Dead beats the trail** - measured at -0.06 SOL over 652 fires. Settled: it is a
  labelling difference, not money.
* **Fee basis.** The study charges the entry fee by inflating the cost basis
  (`/(1+F)`); the engine charges it on the notional. Exactly:
  `pnl_engine = (1 + F) * pnl_study + 2*F*FIX`, so the engine reads **1.25 % higher**
  on a book, +0.55 SOL of the money rule's +0.99 residual and the whole of the rate
  rule's. Not an error on either
  side - one is the study's convention, the other is what `round_trip_multi_leg`
  charges live - but the two are not interchangeable and a comparison must say which.
* **`m_position.held_sec >= 1200` holds report short.** The exit fires on a tick; the fill prices at
  the last print, which on a silent token is much earlier, so `holding_secs` on those
  rows under-reports. A reporting artifact of the fill model, not money.
* **The lake ends at 2026-09-06**, so the study's 09-07 period (263 fires, +3.61 SOL)
  is outside every number here.
* **`prior_launches` is unprimed in simulate** - `creator_launch_counts` times out on
  this database. Neither rule reads that axis, so it does not touch these numbers, but
  a rule that does would read every creator as new.
