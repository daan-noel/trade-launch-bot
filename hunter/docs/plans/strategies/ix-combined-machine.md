# Combined machine: all live gates, bundle vs fillable

One rule, not separate books. Door + 5-slot all-buy gap + `vsol < 46` + not
all-repeat + working completing print, then **crowd or turn**. Tight
same-slot packs are unfillable. Reproduce: `ixg-combined-money.sql` +
`ixg-combined-money.py`. Scratch: `ixg.cm_cand`. Reuses `dmint` / `fall` /
`fbuy` / `fquiet` / `nwal0` / `nprev` / `tlag`. Window 2026-08-11 .. 2026-08-23
exclusive. Fill = last print with `ts <= fire + lag` (fallback: the firing
print). Cost = 125 bps/leg + own `B/vsol` at B = 0.10 SOL. One episode per
mint except the re-entry row.

Fire is the first print in the slot that makes any live family true. A no-dip
solo does not consume the slot: a later crowd in the same slot can still fire.

## The three shapes

`tx_index` is the block order. A prefix is **tight** when its members occupy
consecutive indices (`max - min + 1 = count`). Nothing else in the block sits
between them — a Jito bundle. A **hole** means some other transaction landed
in between, so the prints are separated demand, not one pack.

| Shape | What it is | Fillable |
| --- | --- | --- |
| `bundle` | same working template, ≥2 wallets, fam SOL in [0.9, 4), tight | no |
| `separated` | same family, at least one `tx_index` hole | yes |
| `one` | solo first-on-mint working print in [0.9, 4) with `trail >= 15` | yes |
| `mixed_tight` | mixed templates, ≥2 wallets, tot in [0.9, 4), tight | no |
| `mixed_gap` | mixed family with a `tx_index` hole | yes |

All-repeat is out. Completing print on mixed is a working template. Turn
trail reuses `ixg.tlag` (same series as [ix-solo-turn.md](ix-solo-turn.md)).

Events in `cm_cand` (before one-per-mint): `one` 44,806, `separated` 28,364,
`mixed_gap` 7,910, `bundle` 7,532, `mixed_tight` 454. Tight same-ix is the
smaller crowd, not the mass.

## Money

| Book | lag | n | mean | med | win | SOL | days+ | hold p50 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| combined (all shapes) | 0 | 43,462 | +7.70% | +3.33% | 63.2% | +334.7 | **12/12** | 0.2 s |
| combined | 95 | 43,462 | −1.04% | −2.98% | 22.9% | −45.0 | **0/12** | 0.3 s |
| **fillable** (`one` ∨ `separated` ∨ `mixed_gap`) | 0 | 40,254 | **+7.44%** | **+3.14%** | 62.4% | +299.4 | **12/12** | 0.2 s |
| **fillable** | 95 | 40,254 | **−0.98%** | −2.98% | 23.2% | −39.6 | **0/12** | 0.3 s |
| fillable re-entry | 95 | 81,078 | −0.84% | −2.93% | 24.4% | −68.4 | **0/12** | 0.2 s |
| fillable clock 20 s | 0 | 40,254 | +0.38% | −7.11% | 38.4% | +15.2 | 8/12 | 17.4 s |
| `separated` | 0 | 22,119 | +7.66% | +3.18% | 65.1% | +169.5 | **12/12** | 0.2 s |
| `separated` | 95 | 22,119 | −1.05% | −2.96% | 23.4% | −23.2 | **0/12** | 0.3 s |
| `one` | 0 | 22,683 | +6.80% | +3.18% | 61.4% | +154.3 | **12/12** | 0.2 s |
| `one` | 95 | 22,683 | −0.84% | −2.97% | 24.0% | −19.0 | **0/12** | 0.2 s |
| `mixed_gap` | 0 | 7,204 | +5.27% | +0.54% | 51.9% | +38.0 | **12/12** | 0.3 s |
| `mixed_gap` | 95 | 7,204 | −0.56% | −2.94% | 25.5% | −4.0 | 1/12 | 0.4 s |
| `bundle` | 0 | 7,009 | +8.22% | +3.61% | 67.2% | +57.6 | **12/12** | 0.2 s |
| `bundle` | 95 | 7,009 | −0.98% | −2.95% | 23.2% | −6.9 | 2/12 | 0.2 s |
| `mixed_tight` | 0 | 451 | +5.57% | +2.93% | 62.1% | +2.5 | 12/12 | 0.2 s |
| `mixed_tight` | 95 | 451 | −1.88% | −2.94% | 20.4% | −0.8 | 1/12 | 0.2 s |
| fillable, his mints | 95 | 3,082 | +0.61% | −2.80% | 33.9% | +1.9 | 10/12 | 0.5 s |
| fillable, other mints | 95 | 37,172 | −1.12% | −2.98% | 22.3% | −41.4 | **0/12** | 0.3 s |

Fillable clock-20 at 0 ms is median-negative and OOS-negative. Re-entry at 95 ms
doubles the trade count and deepens the SOL loss; every day stays red.

`bundle` at 0 ms is the fattest cell (+8.22%, 67% win). That number fills inside
a consecutive pack. `separated` at 0 ms is still +7.66% with the same 0.2 s
hold — the rest of the crossing slot, not the bundle interior. 95 ms is after
that remainder on every fillable shape, including `separated`.

On his mints only, fillable first-gap at 95 ms is +0.61% (10/12) with median
still −2.80%. The 95 ms full-tape loss is the tokens outside his list, and even
his list is not a body at this fill.

## What this is

The live conjunction is a discriminator at the completing print. Dropping tight
packs does not change the fill: the honest unconcentrated book is the same race
as [ix-machine-money.md](ix-machine-money.md). The unconcentrated body after
this fill dumps ([ix-harvest-path.md](ix-harvest-path.md)). Concentration on
the same print: [ix-concentrate.md](ix-concentrate.md).

Do not fund the unconcentrated book. Do not re-price the 0 ms first-gap mark
as a 20 s trade.
