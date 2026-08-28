# Live harvest rule — metric map and `m_burst_slot`

The concentrated harvest to ship. Money and path live in
[ix-concentrate.md](ix-concentrate.md), [ix-cell-exit.md](ix-cell-exit.md),
and [ix-crowd-island.md](ix-crowd-island.md) (every crowd spelling).
This file is the **engine mapping**: two exclusive rules, one door
fingerprint, which existing metrics are the same quantity, which group
to add, how packed/bundles and gaps work, the exit DNF, and what not
to duplicate.

Fill = last print with `ts <= fire + 95 ms`. Cost = 125 bps/leg + own
`B/vsol` at B = 0.10 SOL. Fire is the completing print (the engine
already decides per trade). Re-entry when the same gates match.

Do not author this rule on `m_flow_ix` patterns, `unique_wallets`,
`stall`, or `held`. Those are different subjects.

## The rules

**Door** (one fingerprint, once — [ix-door.md](ix-door.md)): create tx
contains ATA; `init_buy_lamports >= 0.2 SOL`; `first_slot_buy_lamports
>= 0.5 SOL`. Cashback-off is not the door. Working-template list lives
on this fingerprint.

Entry is AND. Same-template and mixed cannot share one rule, so they
are **two exclusive rules** on that fingerprint. `packed` is
unconstrained (hole and tight pack are both in). Solos out. Create
slot is out (`4sl@1` plus `time >= 20` covers it).

Shared entry (both rules):

| Gate | Spelling | Notes |
| --- | --- | --- |
| age | `m_state.time >= 20` | seconds since create |
| buy quiet | `m_flow_window.buy_count == 0` on **`4sl@1`** | SQL `dslot >= 5`. **Not** `5sl@1` |
| depth | `m_burst_slot.pre_slot_liquidity < 16` | SQL `vsol_pre < 46`; `liquidity = vsol − 30` |
| dip | `m_burst_slot.pre_print_trail >= 15` | lifetime trail **before** this print |
| this print | `m_burst_slot.working_template == 1` | template on the fingerprint list |
| not all-repeat | `m_burst_slot.new_on_mint_wallets >= 1` | first curve-buy on this mint this slot |

**Rule A — same-template crowd:** shared, plus
`slot_template_count == 1`, `template_buy_count >= 2`,
`template_buy_sol` in `[0.9, 4)`, `template_wallet_count >= 2`.

**Rule B — mixed crowd:** shared, plus
`slot_template_count >= 2`, `slot_buy_sol` in `[0.9, 4)`,
`slot_wallet_count >= 2`.

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
first-on-this-mint in this slot’s template run.

## `m_burst_slot` — the only new group

One group for every **new** reading this harvest needs. Named for the
subject (current slot’s buy prefix × this print’s build template), not
for a strategy. One `on_trade`, one slot buffer, reset when `slot`
changes. The working-template **list** lives on the fingerprint so the
group stays reusable. A later shape split (`packed`) is another metric
on this same group, not a second group.

Template id (producer, one function, same spelling as SQL `tmpl`):
`program|CU|ATA|N|S|F` from the trade’s labels. Needs `tx_index` on
`TradeLite` as `Option<u32>`: `0` is a valid first transaction in the
block, `None` is missing. Missing `tx_index` ⇒ `packed` is `NaN`
(never fires), not “treat as hole.”

| Metric | Unit | Meaning |
| --- | --- | --- |
| `working_template` | 0/1 | this print’s template is on the fingerprint list |
| `template_buy_count` | count | buys this slot with **this print’s** template |
| `template_buy_sol` | SOL | their SOL |
| `template_wallet_count` | count | distinct wallets among those buys |
| `slot_buy_count` | count | all buys this slot so far |
| `slot_buy_sol` | SOL | their SOL (mixed size) |
| `slot_wallet_count` | count | distinct wallets among those buys |
| `slot_template_count` | count | distinct templates among those buys |
| `new_on_mint_wallets` | count | of this print’s template buys, first curve-buy on **this mint** this slot |
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
trailing reqs inside a multi-req clause. Sweep / `can_enter` / readout
walk clauses. `scale_out` stays object-only.

`tx_index` is `Option<u32>` on `TradeLite`. Template helper (guard
tests vs SQL `tmpl`). Fingerprint working-template list + create-ATA
axis. `m_burst_slot` is the group. Two exclusive rules as in **The
rules**, re-entry on (`cooldown_sec: 0`). Compile-pinned in
`engine/tests/harvest_crowd_rules.rs` — not live DB rows.

Simulate on 2026-08-11 .. 2026-08-23 exclusive before paper. Do not
treat the Python walk as live PnL.

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
