# Full-tape money: door + crossing burst + vsol < 46

The machine from [ix-burst-kinds.md](ix-burst-kinds.md), [ix-door.md](ix-door.md),
[ix-perm.md](ix-perm.md), priced on every door-passed mint, 2026-08-11..08-23
exclusive. Fire = the print that **crosses** the band. Fill = last print with
`ts <= fire + lag` (fallback: the crossing print). Cost = 125 bps/leg + own
`B/vsol` at B = 0.10 SOL. One episode per mint. Scratch: `ixg.fcand` /
`ixg-fulltape-money.sql` + `ixg-fulltape-money-2.sql`. Walk: `ixg-machine-money.py`.

Wallet 2720's buys are not burst members. Create slot and launch templates are out.

## Same working template (the named family)

Door, 5-slot gap, same template, ≥2 wallets, family SOL in [0.9, 4), count ≥ 2,
working tmpl (Axiom/Photon/Terminal/GMGN `CU|ATA|F`, Bloom `CU|F`, Axiom
`CU|ATA|N|F`), `vsol < 46` before the burst. n = 34,023 first-per-mint fills.

| Exit | lag | mean | med | win | SOL | days+ | hold p50 | IS / OOS mean |
| --- | ---: | ---: | ---: | ---: | ---: | --- | ---: | --- |
| first-gap (2 slots) | 0 | **+7.69%** | **+3.23%** | 65.6% | +261.6 | **12/12** | 0.2 s | +7.43 / +8.11 |
| first-gap | 95 | **−0.98%** | −2.97% | 23.6% | −33.3 | **0/12** | 0.3 s | −1.01 / −0.96 |
| clock 20 s | 0 | −0.58% | −8.39% | 38.1% | −19.9 | 4/12 | 17.4 s | −0.63 / −1.03 |

Zero-lag first-gap is body-driven (positive median, 66% win) and every day is green.
The hold is **0.2 s** — the rest of the crossing slot. Clock-20 at the same 0 ms
fill is already red: there is **no 20-second tail** on this family on the full tape.

95 ms on both legs is red **every day**, in sample and out. The race is the book.

Age < 180 s at 95 ms does not rescue it (−1.05%, 0/12).

On **his** mints only, same-work first-gap at 95 ms is +0.71% (10/12, +1.75 SOL,
median still −2.60%). On tokens he never touches it is −1.11% (0/12, −35 SOL).
The 95 ms full-tape loss is the tokens outside his list.

## Mixed template, and controls (first-gap)

| Book | lag | n | mean | med | win | SOL | days+ | hold p50 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| mixed, ≥2 templates, ≥2 wallets | 0 | 32,154 | +1.85% | −2.48% | 36.6% | +59.3 | 12/12 | 0.2 s |
| mixed | 95 | 32,154 | −1.02% | −2.99% | 20.6% | −32.9 | 0/12 | 0.2 s |
| same_dead (Axiom/Photon `CU\|F`, no ATA) | 95 | 6,982 | −1.31% | −2.93% | 19.5% | −9.2 | 0/12 | 0.1 s |
| one-wallet | 95 | 636 | −0.99% | −2.94% | 19.8% | −0.6 | 2/12 | 0.1 s |

Mixed at 0 ms is every-day green but **median-negative** — a tail, not the same-work
body. At 95 ms it matches same_work: red every day. Dead template and one-wallet
stay negative at 95 ms.

## What this is

The decision-point event is real: the same conjunction that precedes his fire is a
full-tape discriminator at the crossing print. It is not his landing, and it does
not need his mint list to light up at 0 ms.

It is not a live rule at this bot's fill. Sending 95 ms later buys a later print in
the same burst. Holding past the first gap gives back the slot remainder.

Do not re-price the 0 ms first-gap mark as a 20 s trade. Do not fund it.

All live gates as one rule, tight packs unfillable:
[ix-combined-machine.md](ix-combined-machine.md).
