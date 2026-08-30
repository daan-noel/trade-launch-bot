# Live harvest rule — metric map and `m_burst_slot`

The concentrated harvest to ship. Money and path live in
[ix-concentrate.md](ix-concentrate.md), [ix-cell-exit.md](ix-cell-exit.md),
and [ix-crowd-island.md](ix-crowd-island.md) (every crowd spelling).
This file is the **engine mapping**: two exclusive rules, one door
fingerprint, which existing metrics are the same quantity, which group
to add, how packed/bundles and gaps work, the exit DNF, and what not
to duplicate.

Fill = last print with `ts <= fire + 95 ms`. Cost = 125 bps/leg + own
`B/vsol` at B = 0.10 SOL. Fire is the completing print: the first buy
this slot that makes `entry_event` true. `entry_lock: "slot"` spends
that slot even when the `entry` filters fail. Re-entry when the same
gates match on a later slot.

Do not author this rule on `m_flow_ix` patterns, `unique_wallets`,
`stall`, or `held`. Those are different subjects.

## The rules

**Door** (one fingerprint, once — [ix-door.md](ix-door.md)): create tx
contains ATA; `init_buy_lamports >= 0.2 SOL`; `first_slot_buy_lamports
>= 0.5 SOL`. Cashback-off is not the door. Working-template list lives
on this fingerprint.

Same-template and mixed cannot share one rule, so they are **two
exclusive rules** on that fingerprint. `packed` is unconstrained (hole
and tight pack are both in). Solos out. Create slot is out (`4sl@1`
plus `time >= 20` covers it). Simulate with `curve_only: true`.

Shared **event** (both rules) — AND, once per slot:

| Gate | Spelling | Notes |
| --- | --- | --- |
| this print joined | `m_burst_slot.this_member == 1` | curve buy, has grain, not launch |
| this print working | `m_burst_slot.this_working == 1` | grain on the fingerprint list |
| not all-repeat | `m_burst_slot.has_new == 1` | at least one working-list wallet is first-on-mint |
| known wallets | `m_burst_slot.has_unknown == 0` | missing wallet rejects the event |

Shared **filters** (both rules) — AND, evaluated only on the completing print:

| Gate | Spelling | Notes |
| --- | --- | --- |
| age | `m_state.time >= 20` | seconds since create |
| buy quiet | `m_flow_window.buy_count == 0` on **`4sl@1`** | SQL `dslot >= 5`. **Not** `5sl@1` |
| depth | `m_burst_slot.pre_slot_liquidity < 16` | SQL `vsol_pre < 46`; `liquidity = vsol − 30` |
| dip | `m_burst_slot.pre_print_trail >= 15` | lifetime trail **before** this print |

**Rule A — same-template crowd:** event, plus
`member_template_count == 1`, `same_buy_count >= 2`,
`same_buy_sol` in `[0.9, 4)`, `same_wallet_count >= 2`.
`member_template_count` is distinct grains on the **whole member prefix**
(SQL `run_ntmpl`), not the working list. Axiom plus Pump.Fun reads 2
and is not Rule A.

**Rule B — mixed crowd:** shared event, plus
`working_template_count >= 2`, `working_buy_sol` in `[0.9, 4)`,
`working_wallet_count >= 2`. Mixed size is the working-list total,
not every buy in the slot. Organic-padded same-working (Axiom plus
Pump.Fun, one hunted grain) is neither A nor B.

A later shape split is JSON only (`packed == 0` or `1` on A or B).
No new group.

**Exit** (same DNF on both rules):

```json
"exit": [
  { "m_position": {
      "armed":   [{ "operator": "=",  "value": 1 }],
      "retrace": [{ "operator": ">=", "value": 18 }],
      "arm_above_pct": 10
  } },
  { "m_position": {
      "armed": [{ "operator": "=", "value": 0 }]
    },
    "m_flow_window": {
      "window_size_sec": 8,
      "buy_count": [{ "operator": "=", "value": 0 }]
    }
  }
]
```

That is `(armed AND retrace >= 18) OR (unarmed AND 8 s buy silence)`.
Object-form `exit` (no array) stays today's flat OR, so stored rules
do not change. Entry stays AND. `scale_out` stays object-only (flat
OR of stage reqs); harvest does not use it. See **Exit combinator**
below.

Working templates (fingerprint list, template grain — not full
`ix_hash`):

```
Axiom Trade|CU|ATA|F
Axiom Trade|CU|ATA|N|F
Photon|CU|ATA|F
Terminal|CU|ATA|F
GMGN Bot|CU|ATA|F
GMGN|CU|ATA|F
Bloom Router|CU|F
Bloom|CU|F
```

Bloom’s working shape is `CU|F` (no ATA). Axiom `CU|F` is dead. Do not
spell this as “router AND CU AND ATA”.

## Existing metrics — use these, do not clone

| Wanted | Exact existing spelling |
| --- | --- |
| 5-slot buy quiet (`dslot >= 5`) | `buy_count == 0`, `window_size_slots: 4`, `window_lag: 1` |
| no-tx quiet (not this rule) | `trade_count == 0` on that same window |
| 8 s buy death | `buy_count == 0`, `window_size_sec: 8` |
| age | `m_state.time` |
| armed trail 10 / 18 | `m_position.retrace` + `arm_above_pct` |
| trail latch | `m_position.armed` (0/1; see below) |
| init / first-slot door | fingerprint `init_buy_lamports`, `first_slot_buy_lamports` |

`4sl@1` is slots `S-4 … S-1`. Last buy at `S-5` passes (`dslot == 5`).
`5sl@1` is `dslot >= 6` and is the wrong gate.

`stall` is seconds since ATH. `held` is clock from fill. Neither is
buy silence.

`m_state.liquidity` is reserve **after** this trade. The depth gate is
the last print **before this slot**.

`m_price_lifetime.trail` includes this print. The dip gate is trail on
the previous print (`tlag`).

`m_flow_ix` matches full ordered `ix_labels`. Markers have no ATA, no
CU, no GMGN, and they include Trojan. Not the template grain.

`unique_wallets` counts distinct senders in a window. The gate is
first-on-this-mint in this slot among **working-list** wallets.

## `m_burst_slot` — the only new group

One group for every **new** reading this harvest needs. Named for the
subject (current slot's member prefix × this print's build template), not
for a strategy. One `on_trade`, one slot buffer, reset when `slot`
changes. The working-template **list** lives on the fingerprint so the
group stays reusable. A later shape split (`packed`) is another metric
on this same group, not a second group.

A **member** is a curve buy with a template grain, not a launch create.
Unknown wallet (`wallet_hash == 0`) still joins and sets `has_unknown`.
Every buy with a wallet (including launch / AMM) updates the ever-seen
set. `member_template_count` is `by_template.len()`. Working-list totals
are derived at read from `by_template` intersect the fingerprint list.

Template id (producer, one function, same spelling as SQL `tmpl`):
`program|CU|ATA|N|S|F` from the trade's labels. Needs `tx_index` on
`TradeLite` as `Option<u32>`: `0` is a valid first transaction in the
block, `None` is missing. Missing `tx_index` ⇒ `packed` is `NaN`
(never fires), not "treat as hole."

| Metric | Unit | Meaning |
| --- | --- | --- |
| `this_member` | 0/1 | this print just joined the member prefix |
| `this_working` | 0/1 | this print's grain is on the fingerprint list |
| `same_buy_count` | count | members this slot with **this print's** grain |
| `same_buy_sol` | SOL | their SOL (Rule A size) |
| `same_wallet_count` | count | distinct wallets among those buys |
| `member_template_count` | count | distinct grains among **all** members this slot. Rule A is 1 |
| `working_buy_count` | count | members this slot on the working list |
| `working_buy_sol` | SOL | their SOL (Rule B size). Organic is out |
| `working_wallet_count` | count | distinct wallets among working-list members |
| `working_template_count` | count | distinct working-list grains this slot. Rule B is >= 2 |
| `has_new` | 0/1 | any working-list wallet is first-on-mint |
| `has_unknown` | 0/1 | some member this slot has no wallet |
| `packed` | 0/1 | `1` = consecutive `tx_index`; `0` = hole |
| `pre_slot_liquidity` | SOL | real reserve at last print with `slot < S` |
| `pre_print_trail` | percent | lifetime trail **before** folding this print |

### Hot path

Per trade: bump the current-slot prefix, compare `tx_index` to
`first + count`, look the wallet up in a “seen in an earlier slot”
set. No extra RPC, no ring of templates. The prior-wallet set grows
with unique traders on the mint (same class as crowd / ix contagion).

## Plumbing that is not a metric

These are how the fold *sees* the tape, *matches* a build, and
*combines* exits. They are not quantities a rule compares.

**`tx_index` on `TradeLite`.** Where this trade sits in the block,
`Option<u32>`. A pack is consecutive indexes (5, 6, 7); a hole is
5, 7, 9. `0` is a valid first-in-block index; `None` is missing.
Missing ⇒ `packed` is `NaN`. Every producer that has a block position
copies it on.

**Template grain.** One function, SQL `tmpl` spelling
`program|CU|ATA|N|S|F`. Guard-test against SQL. Not full `ix_hash`
and not marker bits. The working list is fingerprint config, not a
metric.

**Create ATA.** The door is ATA **present** on the create tx
(`Associated Token:`). Fingerprint `ix_labels` is an exact ordered
sequence. Create-ATA is a numeric 0/1 fingerprint axis
(`create_ata`), not a full-sequence match and not an `m_burst_slot`
metric. Empty labels fail closed (`None`). Init / first-slot axes
already exist.

**Exit combinator.** See the next section.

## Entry combinator

`entry` stays AND. Optional `entry_event` is a second AND-object: the
completing-print event. `entry_lock: "slot"` fires that event once per
slot; `entry` filters that fail still spend it. Absent `entry_lock`
is today's level-AND on every print. `entry_lock` without a non-empty
`entry_event` is a parse error.

```json
"entry_lock": "slot",
"entry_event": { "m_burst_slot": { "this_member": [{"operator": "=", "value": 1}] } },
"entry": { "m_state": { "time": [{"operator": ">=", "value": 20}] } }
```

Sells and ticks clear `this_member`, so leftover gates cannot fire off
a later print in the same slot once the completing buy has spent it.

## Exit combinator

Object-form `exit` today is a **flat OR of reqs**. Inside one metric
the expr is already DNF; across metrics and groups it is OR. That
cannot author `(a AND b) OR (c AND d)` when `a` and `b` are different
groups.

**Array form:** a list of clauses. Clauses OR. Inside a clause, every
metric AND. Same `{operator, value}` as today. No `phase`. No extra
combinator axes. `(a OR b) AND c` is written `(a AND c) OR (b AND c)`.

```json
"exit": [
  { "m_flow_window": { "window_size_sec": 8, "buy": [{ "operator": ">", "value": 2 }] },
    "m_state":       { "liquidity": [{ "operator": "<", "value": 20 }] } },
  { "m_state":          { "liquidity": [{ "operator": ">", "value": 30 }] },
    "m_price_lifetime": { "trail": [{ "operator": ">", "value": 20 }] } }
]
```

That is `(buy > 2 AND liquidity < 20) OR (liquidity > 30 AND trail > 20)`.

Object form (no array) stays today’s flat OR. Stored rules round-trip
unchanged. `can_enter`, sweep, and readout walk clauses, not a flat
req list. Do not add a one-off `if armed skip death` in `exit_fired`.

### `m_position.armed`

`arm_above_pct` exists because object-form cannot AND `retrace` with
`pnl`. A trail that has armed must **stay** armed: price can fall back
under +10% and the 18% retrace still sells.

`pnl >= 10 AND retrace >= 18` is **not** that. When price comes off
the peak, `pnl >= 10` goes false and the trail dies.

`m_position.armed` is a 0/1 latch: flips to `1` when `pnl` first
reaches `arm_above_pct`, never un-latches for the hold. It is a
quantity you put in a clause, not a combinator axis. `arm_above_pct`
stays the **threshold** that flips it. Object-form rules keep today’s
skip on trailing reqs so they keep working. Array-form rules AND
`armed` explicitly and do not need that skip.

Do not add `m_position.buy_silence`. Do not OR the 8 s window in
without `armed == 0` on that clause — it would sell a runner that
pauses.

## Gaps: buy quiet vs no-tx quiet

The priced quiet is **no buys**, SQL `dslot >= 5` on the first buy of
the slot ([ix-burst-kinds.md](ix-burst-kinds.md) `fquiet`).
`trail >= 15` is a dip; that dip is often **sells in the same gap**.
A no-tx gap (`trade_count == 0` on `4sl@1`) drops that shape. It is
already expressible; it is not this rule. Kind-gap (silence of
working prints only) is not this family.

## Bundles (`packed`)

**Packed** = the all-buy prefix so far in this slot occupies consecutive
`tx_index`: `last − first + 1 == count` (e.g. 5,6,7). **Hole** =
something else landed in between (e.g. 5,7,9). O(1): two integers on
the slot prefix. Same test for same-template and mixed; leave
unconstrained on A/B.

A tight pack is a Jito-style bundle: you cannot land **in the middle
of it**. A 0 ms first-gap fill inside the pack is fiction
([ix-combined-machine.md](ix-combined-machine.md)). The harvest fires
on the completing print and fills 95 ms later, **after** the pack.
That book is in the island ([ix-crowd-island.md](ix-crowd-island.md)):
`bundle` clock-20 +2.64% / 11/12 / OOS +3.18%. Do not require
`packed == 0`.

## What not to do

- Do not fold solos into this rule.
- Do not use `he1` or his mint list as a gate.
- Do not swap this harvest for first-gap or `harvest_clock` (they
  exit at the 0.8 s pause, before the peak).
- Do not sweep trail width or death seconds on this print.
- Do not drop mixed-template or `packed == 1` — they are this island
  ([ix-crowd-island.md](ix-crowd-island.md)).
- Do not add a metric that an existing group already states.
- Do not put these readings in `m_flow_ix` / `m_state` / `m_price_lifetime`
  as a second copy. New readings stay in `m_burst_slot`.
- Do not add a windowed twin of `m_burst_slot` until a finding needs
  a span other than this slot.
- Do not nest `any` / `all` trees. DNF is enough.

## In the engine

Exit DNF (array of clauses) + `m_position.armed` latch. Object-form OR
and trailing skip stay; array-form ANDs `armed` and does not skip
trailing reqs inside a multi-req clause. Sweep / `can_enter` /
`try_enter` / readout walk clauses. `scale_out` stays object-only.

`entry_event` + `entry_lock: "slot"` on `CompiledRule::try_enter`.
`tx_index` is `Option<u32>` on `TradeLite`. `on_curve` / `is_launch`
on the tape. Template helper (guard tests vs SQL `tmpl`). Fingerprint
working-template list + create-ATA axis. `m_burst_slot` is the group.
Two exclusive rules as in **The rules**, re-entry on (`cooldown_sec: 0`).
Compile-pinned in `engine/tests/harvest_crowd_rules.rs`. Seed:
`hunter/scripts/seed-harvest-crowd-rules.sql`.

Simulate on 2026-08-12 .. 2026-08-23 exclusive, `curve_only: true`,
`fill_model=lag_115`, `cost_model=pumpfun_impact`. Do not treat the
Python walk as live PnL.

## Related

- Door: [ix-door.md](ix-door.md)
- Burst kinds / gap: [ix-burst-kinds.md](ix-burst-kinds.md)
- Permissions / vsol: [ix-perm.md](ix-perm.md)
- Combined machine / shapes: [ix-combined-machine.md](ix-combined-machine.md)
- Concentration: [ix-concentrate.md](ix-concentrate.md)
- Path and `arm_death 8`: [ix-cell-exit.md](ix-cell-exit.md)
- Every crowd spelling: [ix-crowd-island.md](ix-crowd-island.md)
- Template grain: [ix-template-gate.md](ix-template-gate.md)
- First-on-mint: [ix-new-wallets.md](ix-new-wallets.md)
- Armed trail (object-form skip): [armed-trailing-stop.md](armed-trailing-stop.md)
- Engine metric registry: [metrics-reference.md](metrics-reference.md)
