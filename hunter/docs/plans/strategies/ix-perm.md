# Permissions at the burst

State that must already be true when the gap-then-burst crosses. Scored **only
inside** door-passed named families (`same_tmpl_nwal` and `multi_tmpl_nwal`, tot in
[0.9, 4)), looking **backward** from the first print of the burst (`t0`). Burst
prints are out of the windows. Reproduce with `ixg-perm.sql`. Scratch: `ixg.perm`
(63,042 rows). Metrics are the engine names.

| Permission | Engine | At `t0` |
| --- | --- | --- |
| depth | `m_state.liquidity` = vsol − 30 | last print before the burst |
| SOL-quiet | `m_flow_window(10).gross_flow` | `[t0−10s, t0)` |
| net/gross | `m_flow_window(10).buy` / `sell` | same window |
| trail / rise | `m_price_lifetime.trail` / `rise` | peak/trough before `t0` |
| age | seconds since `tokens.created_at` | |

Door: [ix-door.md](ix-door.md). Burst kinds: [ix-burst-kinds.md](ix-burst-kinds.md).
This is still a thermometer on his mints until [ix-machine-money.md](ix-machine-money.md).

## Depth is the hard permission

`vsol < 46` (liquidity < 16). Same-template bursts at vsol ≥ 46: **0%** response.
Mixed-template ≥ 46: ~0%. The cut keeps **2,723 / 2,775** of his fires in this
set (98%). Below 33 vsol he still fires — there is no 33 floor here.

| vsol | same_tmpl resp | multi resp |
| --- | ---: | ---: |
| lt33 | 7.36 | 10.57 |
| 33–36 | 8.19 | 11.18 |
| 36–40 | 6.80 | 8.06 |
| 40–46 | 5.31 | 6.26 |
| 46–55 | **0** | 0.02 |
| ge55 | **0** | 0 |

## What does not add, inside this event

**SOL-quiet.** After the 5-slot buy-gap, `gross_10` bands are flat (same-tmpl ~3.5–4.5%,
mixed ~4.3–4.8%). The gap already is the quiet. A 10 s gross cap is not a second gate.

**net/gross.** Empty / buy-heavy / sell-heavy 10 s windows sit in the same resp
band. Do not require a pre-burst sell-off or buy-share.

**trail.** Inside `vsol < 46`, trail < 15 (no dip) is live — mixed 11.5%, same-tmpl
6.3%. This event is not “buy a 5–60% dip.” Solos are the other shape:
[ix-solo-turn.md](ix-solo-turn.md).

**rise.** `rise >= 100` looks dead universe-wide because it is mostly vsol ≥ 46.
Inside `vsol < 46` it is mild (same-tmpl 4.4% vs 7.7% at rise < 50). Not a hard cut.

## Age is a precision knob, not a door

Peak resp is 20–60 s. `age < 180` inside `vsol < 46` doubles resp (same-tmpl 8.4%
vs 4.3%; mixed 11.4% vs 6.1%) and keeps 1,619 / 2,775 hits (58%). He still fires
on older deep tokens. Do not treat “too old” like “failed the create door.”
On the full-tape harvest that cut is the wrong direction: `age < 20` is the
dump and `age < 180` keeps it ([ix-concentrate.md](ix-concentrate.md)).

## Conjunction that is actually true at the fire

All of:

1. door (create ATA, init ≥ 0.2, first-slot ≥ 0.5)
2. gap, then named burst, tot in [0.9, 4)
3. **vsol < 46** at the last print before the burst

Optional: `age < 180` if tightening. Working same-template (Axiom/Photon/Terminal/GMGN
`CU|ATA|F`, Bloom `CU|F`) plus (3) plus age < 180: n=3,385, resp **10.4%**, causal
**4.46%**. Mixed-template plus (3) plus age < 180: n=10,540, resp **11.4%**, causal
**2.30%**.
