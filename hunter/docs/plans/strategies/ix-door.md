# Creation-time door (arm vs never-touch)

Layer 1 is a **once-at-create** conjunction. A mint that fails is not watched. This is
not a burst identity and not a band picked by return. Reproduce with `ixg-door.sql`.
Scratch: `ixg.tok`.

Universe: tokens with `created_at` in 2026-07-26..08-23 exclusive (754,405). His = mint
in `w8.buys` (8,640, **1.145%**). Create-tx flags use the same template grain as the
burst work (`CU` / `ATA` / nonce / seed / fee on `tokens.ix_labels`). First-slot buy
SOL is the create-slot sum on `tokens_info` — still arm-time, still not a mid-tape burst.

## What does not arm him

`is_cashback_enabled` off is **not** his door. Cashback-on is 54.4% of his mints
(rate 1.22% vs 1.07% off). The cell cashback-off and init in `[0.2, 1)` is **15.4%** of
his mints — a money slice from [ix-template-gate.md](ix-template-gate.md), not the arm
list. `is_mayhem_mode` is unused in this window (all false).

Init-buy **bands he actually sits in** (coverage of his mints). Rates among 0.2–10 SOL
are all near 1.8–2.0%. The only init cut that changes the rate is dust:

| init_band | n | hits | P(his) | cov of him |
| --- | ---: | ---: | ---: | ---: |
| lt0.2 | 199,750 | 550 | 0.275% | 6.4% |
| 0.2–1 | 93,922 | 1,709 | 1.82% | 19.8% |
| 1–2 | 70,452 | 1,431 | 2.03% | 16.6% |
| 2–5 | 176,947 | 3,467 | 1.96% | **40.1%** |
| 5–10 | 54,990 | 1,018 | 1.85% | 11.8% |
| ge10 | 17,422 | 273 | 1.57% | 3.2% |
| unk | 140,922 | 192 | 0.136% | 2.2% |

His mass is 2–5 SOL creator buy, then the rest of ≥0.2. Not 0.2–1.

First-slot buy: `lt0.5` is 0.123% P(his) and 4.7% of him. Bands 2–20 SOL hold ~2.1–2.2%
and 77.7% of him.

Create-tx template: no ATA is the poison. `|F` (fee, no ATA) is 107,400 mints and **6**
hits (0.006%). `c_ata` true is 95.4% of him at 1.39%; `c_ata` false is 0.244%.

## Exclusion conjunction (the door)

Keep a mint if all of:

1. create tx has ATA
2. `initial_buy_lamports >= 0.2 SOL` (drop `unk` and `lt0.2`)
3. create-slot buy SOL `>= 0.5`

| | n | hits | P(his) | cov of him |
| --- | ---: | ---: | ---: | ---: |
| universe | 754,405 | 8,640 | 1.145% | 100% |
| ATA + init ≥ 0.2 | 396,233 | 7,696 | 1.94% | 89.1% |
| + first-slot ≥ 0.5 | 363,473 | 7,579 | **2.085%** | **87.7%** |

Fail side: 390,932 mints, 1,061 hits, **0.271%**. Pass is **7.7×** fail. The 12.3% of
his mints that fail are split across dust init, dust first-slot, and no-ATA create —
no second tight family.

Adding “create template ∈ {`CU|ATA`, `CU|ATA|F`, `|ATA|F`}” only moves 87.7 → 85.5%
coverage and 2.09 → 2.16% rate. Optional.

This is a **blacklist of launch poison**, not a small watchlist. It still leaves
~363k mints in the window. The burst is what fires inside it.

## Burst on mints he already touched

Burst on mints he already touched is in [ix-burst-kinds.md](ix-burst-kinds.md).
Permissions at the burst: [ix-perm.md](ix-perm.md). Full-tape money is not this file.
