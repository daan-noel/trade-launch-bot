# Ix build-template gate

The trigger is a **build template**, not a router name and not a learned hash list.
Reproduce with the SQL beside this file (`ixg-template-gate.sql` and the two follow-ons)
and `ixg-honest-exit.py`. Scratch lives in schema `ixg`. Wallet 2720's own fills are out
of the candidate pool.

## The three grains, on Axiom and Photon

Same corpus: curve buys on 8dtx's mints, 2026-07-26..08-22, his fills removed. Response =
he buys the mint in slot S or S+1. Base rate 0.956%.

| Grain | What it keeps | Precision | Recall of his buys |
| --- | --- | ---: | ---: |
| Five working hashes | exact `ix_labels` | 1.78% | 48.8% |
| Template `CU+ATA` of those two routers | program + compute-budget + ATA create, no seed | 1.78% | 48.9% |
| Program name only | every Axiom or Photon variant | 1.09% | 53.2% |

Exact hashes and the template are the same event. The extra hashes inside `CU+ATA` add
nothing. The program grain adds almost no extra fires and dilutes precision because it
pulls in the no-ATA variants.

Quiet (no buy on the mint in the previous 5 slots) raises template precision 1.78 → 3.10
and cuts recall 48.9 → 23.0. He also fires mid-run; silence is a precision knob, not the
whole rule. SOL-quiet (`gross` cap) is a separate permission and is not in this extract.

## Working vs dead inside one router

Axiom Trade, after his fills are removed:

| Template | lift | lift on quiet | lift causal (S+1 only) |
| --- | ---: | ---: | ---: |
| `CU\|ATA\|F` (and `CU\|ATA\|N\|F`) | 1.76 / 2.48 | 2.80 / 3.87 | 1.79 / 2.22 |
| `CU\|F` (no ATA) | 0.30 | 0.20 | 0.47 |
| `ATA\|F` (no compute-budget) | 0.52 | 0.58 | 0.83 |

Photon repeats the split: `CU|ATA` lift 2.91–4.28, `CU` without ATA lift 0.15–0.22.

GMGN's main structure is `CU|ATA|F` and still lift 0.91 — the flags are not enough
without the program. Bloom's paying and dead variants share `CU|F`; that router needs an
extra shape bit (two Bloom instructions vs one) and is not in the money set below.

**Do not sum all "target" routers.** That mix is the program grain plus GMGN.

Racers stay out: `CreateAccountWithSeed` plus a durable nonce, and 8dtx's own
`Pump.Fun: Buy` + nonce + ATA hashes. Those hashes still lift when other wallets use the
same software; they are co-detectors, not the event.

## Door (creation-time, once)

The money table below arms on cashback-off and init in `[0.2, 1)`. That cell is
**15.4% of 8dtx's mints** — not his arm list.

His create-time door is an exclusion: ATA on the create tx, init ≥ 0.2 SOL, create-slot
buy ≥ 0.5 SOL. It keeps 87.7% of him and is 7.7× vs the fail side. Cashback-on / init
2–5 is his largest cell. [ix-door.md](ix-door.md).

## Event (re-entry allowed by construction; money below is first-per-mint)

On an armed mint, vsol in `[33, 85]`: the first buy in a slot whose template is

`program ∈ {Axiom Trade, Photon} AND CU AND ATA AND NOT seed`

after a ≥5 slot buy-gap. Size 0.10 SOL. Cost is 125 bps/leg plus own impact `B/vsol` on
both legs.

Window: 2026-08-11 inclusive to 2026-08-23 exclusive (complete days before the 08-23 ingest
gap). Full tape, not his mints.

## Money

First event per mint, 6,921 tokens. Both legs fill at the last print with
`ts <= fire + lag` (fallback: the firing print).

Zero-lag clock-20 matches the earlier 20 s mark: **+7.75%**, +53.6 SOL, 12/12 days,
median hold 18 s.

**At 95 ms on both legs the book is not a rule.**

| Exit | lag | mean | days + | SOL | IS / OOS mean | hold p50 |
| --- | ---: | ---: | --- | ---: | --- | ---: |
| first-gap (2 slots) | 0 | +8.92% | 12/12 | +61.7 | +6.67 / +8.11 | 0.5 s |
| first-gap | 95 | +1.14% | 6/12 | +7.9 | +0.52 / +0.25 | 0.6 s |
| arm 10 / trail 18, else gap | 0 | +8.15% | 12/12 | +56.4 | +5.69 / +8.48 | 0.8 s |
| arm 10 / trail 18, else gap | 95 | +0.24% | 7/12 | +1.6 | −0.09 / −0.21 | 0.7 s |
| clock 20 s | 0 | +7.75% | 12/12 | +53.6 | +4.61 / +6.63 | 18 s |
| clock 20 s | 95 | +0.22% | 4/12 | +1.5 | −1.26 / −1.06 | 18 s |

First-gap at 95 ms is the least-damaged cell and is still 6/12 with OOS +0.25% (2/5 days).
Drop 08-17 and that cell is +2.1 SOL over 11 days. Clock and trail at 95 ms go negative
out of sample.

Non-overlapping re-entry at 95 ms loses on every exit (gap −49.5 SOL, trail −42.6, clock
−4.4), 0–2 days positive. Extra fires are later bursts; 95 ms already missed the front.

Dead template (same door, quiet, first-per-mint, CU without ATA) at 95 ms: gap **−1.61%**
0/12, clock **−1.75%** 2/12. The flags still separate (~2.7 pp vs dead on first-gap) and
that gap does not clear a stable book.

Same-slot detector: first-gap hold 0.5 s at 0 ms. Zero-lag profit is the rest of the
trigger slot. 95 ms buys the last print in that window.

Age at the first event is 52 s, trigger size 0.53 SOL median, vsol 40.5.

## What this is not

- Not a live rule. The template and the door are real discriminators; the fill at 95 ms
  is not.
- Not a hash whitelist. The engine spelling is the template flags, compiled once.
- Bloom, Maestro, Terminal, and per-template SOL bands are untested in the money table.
- Not the burst. A template names a print; the decision-point event is gap then a
  burst of those prints — every kind, crossing print, no create slot:
  [ix-burst-kinds.md](ix-burst-kinds.md).
