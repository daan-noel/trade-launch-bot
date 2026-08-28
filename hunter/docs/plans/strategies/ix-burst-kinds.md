# Gap-then-burst kinds (ix template)

The decision-point event is a **quiet gap, then a burst named by build template**.
Fire is the print that crosses the band, not the last print in the slot and not his
landing. Create/launch templates are not burst members; the mint's first slot is not a
burst. Reproduce with `ixg-burst-kinds.sql` (scratch in `ixg`, keeps the microscope
tables). Wallet 2720's fills are out of the candidate pool.

Corpus: curve buys on 8dtx's mints, 2026-07-26..08-22. Gap = no buy in the previous 5
slots, and this slot is after the mint's first buy-slot. Response = he buys the mint in
S or S+1. Base rate on these resume slots: **1.079%** (causal S+1-only 0.349%). This
table is a thermometer for *which event he fires on*. It is not a door, not permissions,
not a money book. One family does not have to cover every one of his trades.

Template spelling is [ix-template-gate.md](ix-template-gate.md): `program + CU + ATA +
nonce + seed + fee`.

## Every kind, completed slot

| Kind | What it is | n | resp | lift | causal |
| --- | --- | ---: | ---: | ---: | ---: |
| `multi_tmpl_nwal` | ≥2 templates, ≥2 wallets | 89,086 | 3.83% | 3.55 | 0.73% |
| `same_tmpl_nwal` | 1 template, ≥2 wallets | 40,222 | 1.84% | 1.71 | 0.82% |
| `solo` | 1 buy | 467,696 | 0.50% | 0.46 | 0.24% |
| `same_tmpl_1wal` | 1 template, 1 wallet, ≥2 buys | 4,416 | 0.27% | 0.25 | 0.05% |
| `multi_tmpl_1wal` | ≥2 templates, 1 wallet | 623 | 0.00% | 0.00 | 0.00% |

One-wallet bursts are in the extract. He does not fire on them. They can still be
volume-making; they are not his trigger on this corpus.

Of his quiet-resume fires: 52.5% sit on `multi_tmpl_nwal`, 36.0% on `solo`, 11.4% on
`same_tmpl_nwal`, 0.2% on one-wallet kinds.

## Size band (completed slot)

`tot` is burst SOL. Below 0.3 SOL every kind is dead (~0.05%). At ≥4 SOL,
`same_tmpl_nwal` drops (1.22%) and `solo` is 0%; `multi_tmpl_nwal` stays high on `he1`
(6.24%) and peaks causal in **2–4 SOL**.

| Kind | 0.9–2 SOL resp / causal | 2–4 SOL |
| --- | --- | --- |
| `same_tmpl_nwal` | 3.66 / 1.62 | 4.08 / 1.80 |
| `multi_tmpl_nwal` | 3.53 / 0.77 | 6.21 / 1.12 |
| `solo` | 2.20 / 1.07 | 0.75 / 0.37 |

## Crossing print (state after each buy in the slot)

Same-template prefixes, `fam_n` = count of **this** template so far, `fam_sol` = its sum.
The live cell is **fam_n ≥ 2 and fam_sol in [0.9, 4)**. A single 0.9–2 SOL print lifts
less (3.63 / 1.09). `fam_sol ≥ 4` is dead. `fam_sol < 0.3` is dead.

Inside that cell, the print that makes `fam_n = 3` at 0.9–2 SOL reads 5.39 / 1.63 — the
shape "several ~0.3–0.8 prints of one template, sum crosses ~0.9".

Per-print size on Axiom `CU|ATA` in that cell: 0.2–0.4 and 0.4–0.8 SOL hold; ≥1.5 SOL
weakens.

## Which template, in the same-family crossing cell

`run_ntmpl = 1`, `fam_n ≥ 2`, `fam_sol ∈ [0.9, 4)`:

| Template | actor | n | resp | causal |
| --- | --- | ---: | ---: | ---: |
| Photon `CU\|ATA\|F` | retail | 143 | 12.59 | 5.59 |
| Bloom `CU\|F` | app | 312 | 8.65 | 1.92 |
| Axiom `CU\|ATA\|F` | retail | 19,776 | 7.31 | 2.00 |
| Terminal `CU\|ATA\|F` | retail | 3,524 | 7.12 | 1.93 |
| GMGN `CU\|ATA\|F` | retail | 1,812 | 6.40 | 1.71 |
| Axiom `CU\|ATA\|N\|F` | prepared | 3,610 | 6.01 | 1.52 |
| Axiom `CU\|F` (no ATA) | app | 7,050 | 0.33 | 0.18 |
| Photon `CU\|F` (no ATA) | app | 54 | 0.00 | 0.00 |

ATA vs no-ATA is still the split inside a burst. GMGN as a **same-template burst** lifts;
GMGN as a lone presence gate does not (see the template-gate file). Bloom's paying
variant is `CU|F` with no ATA — do not require ATA on Bloom. Photon is the same ATA
split as Axiom, small n.

Retail `CU|ATA|F` of Axiom/Terminal/GMGN/Photon, same-template, ≥2 wallets, fam 0.9–4:
**7.89 / 7.29 / 7.29 / 6.95** resp by week, four weeks out of four.

Completed-slot form of that cell (top template one of those four): n=8,379, resp
**5.79%**, causal **~2.3%** on the broader retail-ATA same-tmpl 0.9–4 cut (n=10,455,
550 hits).

## Mixed-template bursts

`multi_tmpl_nwal` at tot 0.9–4:

| | n | resp | causal |
| --- | ---: | ---: | ---: |
| no racer | 39,995 | 3.25 | 0.65 |
| has racer | 14,224 | 8.72 | 1.67 |

Racer programs (`sss5N9…`, Pump.Fun `|N|S`, `B5wU3…`, L2TEx) top raw resp when they are
the crossing print in a mixed burst. They are in the extract. Causal stays above
retail-only mixed, so they are not dropped here — they are also the co-detector grain
from [trigger-ix-derivation-method.md](trigger-ix-derivation-method.md). Treat "has
racer" as a **kind**, not as the named trigger, until symmetry/money say otherwise.

Retail Axiom `CU|ATA` as the crossing print *inside* a mixed burst: 4.44 / 1.82 — same
direction as the same-template cell, weaker.

## What this names

Two families that precede his fire, both gap-then-burst:

1. **Same working template, several wallets, family SOL in [0.9, 4), count ≥ 2.**
   Working = Axiom/Photon/Terminal/GMGN `CU|ATA|F` (Bloom `CU|F` is the extra shape).
   Fire = the print that crosses that band.
2. **Several templates, several wallets, tot in [0.9, 4).** Broader, more of his
   fires, mixed with racers.

One-wallet repeats and dust (`< 0.3`) are checked and are not the trigger.

Signal wallets first-on-this-mint vs repeat: [ix-new-wallets.md](ix-new-wallets.md).
`all_rep` is dead; 78% of his quiet-resume fires sit on `all_new`. That cut is
not the ATA flag and is not age. Old-on-chain vs born-this-mint:
[ix-old-wallets.md](ix-old-wallets.md) — not a conjunct on the named burst.
Solo as a turn (dip already true): [ix-solo-turn.md](ix-solo-turn.md).

Door is [ix-door.md](ix-door.md). Permissions at the burst: [ix-perm.md](ix-perm.md)
(`vsol < 46` is required; 10 s gross, trail, and net/gross do not add inside the gap).
Full-tape money: [ix-machine-money.md](ix-machine-money.md). All live gates
as one rule, tight packs unfillable:
[ix-combined-machine.md](ix-combined-machine.md). Early fire + gap
duration: [ix-early-gap.md](ix-early-gap.md). Harvest path after the
95 ms fill: [ix-harvest-path.md](ix-harvest-path.md). Live concentration
of that harvest: [ix-concentrate.md](ix-concentrate.md). Path and
exit on the concentrated cell: [ix-cell-exit.md](ix-cell-exit.md).
Every crowd spelling of that island (same-client or mixed, hole or
tight pack): [ix-crowd-island.md](ix-crowd-island.md).
Live engine mapping: [ix-live-rule.md](ix-live-rule.md).
