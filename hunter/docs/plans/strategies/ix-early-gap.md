# Early fire and gap duration

Fire is the **first working first-on-mint print** after a buy-gap, not the
0.9 SOL cross. Gap length is a permission band. Later-slot shape is
diagnostic (lookahead). Door + `vsol < 46` + not create slot stay on.
Reproduce: `ixg-early-gap.sql` + `ixg-early-gap.py`. Scratch: `ixg.eg_cand`.
Window 2026-08-11 .. 2026-08-23 exclusive. Same fill and cost as
[ix-combined-machine.md](ix-combined-machine.md). One episode per mint.

`starter` = that print in [0.3, 0.9) SOL after `dslot >= 5`. `big` = [0.9, 4).
`oracle_sep` = starter whose slot later becomes a same-template separated
crowd (lookahead).

## Gap length is a mild gate, not a new family

His-mint thermometer, working new print 1, `vsol < 46`:

All sizes, `dslot >= 2`:

| gap | n | resp | causal |
| --- | ---: | ---: | ---: |
| 2–4 | 19,049 | 3.27 | 0.97 |
| 5–9 | 8,837 | 4.49 | 1.30 |
| 10–19 | 6,024 | 5.44 | 1.31 |
| 20–39 | 3,972 | 5.64 | 1.81 |
| 40+ | 5,296 | 5.80 | 1.60 |

`starter` [0.3, 0.9), `dslot >= 5`: 5–9 is 7.63%; **10–19 is 9.58%**; 20–39
is 7.78%; 40+ is 7.17%. 2–4 is the weak side. Longer quiet is not
monotone on the starter cell. Dust `< 0.3` stays dead (0.65%). `[0.9, 4)`
is 8.77%, close to starter 8.04%.

Later shape on starter (lookahead): `mixed_gap` 12.3%, `separated` 10.3%,
`bundle` 7.1%, `solo` 4.3%. A crowd after print 1 lifts the thermometer;
it is not known at fire time.

## Money

| Book | exit | lag | n | mean | med | win | SOL | days+ | hold p50 |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| starter | first-gap | 0 | 29,088 | +5.00% | +0.72% | 53.4% | +145.5 | **12/12** | 0.1 s |
| starter | first-gap | 95 | 29,088 | **−1.23%** | −2.99% | 20.2% | −35.7 | **0/12** | 0.2 s |
| starter | trail 10/18 | 0 | 29,088 | +4.40% | −1.87% | 40.4% | +128.0 | 12/12 | 0.4 s |
| starter | trail 10/18 | 95 | 29,088 | −1.86% | −3.01% | 15.3% | −54.0 | **0/12** | 0.2 s |
| starter | clock 4 s | 0 | 29,088 | +3.90% | −0.15% | 49.1% | +113.4 | 12/12 | 2.8 s |
| starter | clock 4 s | 95 | 29,088 | −2.18% | −3.04% | 29.0% | −63.5 | **0/12** | 2.9 s |
| starter | clock 20 s | 0 | 29,088 | +0.01% | −4.08% | 37.5% | +0.2 | 5/12 | 16.8 s |
| oracle_sep | first-gap | 0 | 8,651 | +5.73% | +3.65% | 79.7% | +49.6 | **12/12** | 0.1 s |
| oracle_sep | first-gap | 95 | 8,651 | **−1.04%** | −2.97% | 22.1% | −9.0 | **0/12** | 0.1 s |
| oracle_sep | trail 10/18 | 95 | 8,651 | −1.51% | −2.99% | 17.4% | −13.1 | **0/12** | 0.1 s |
| big [0.9, 4) | first-gap | 0 | 37,295 | +10.50% | +5.84% | 67.2% | +391.8 | **12/12** | 0.2 s |
| big [0.9, 4) | first-gap | 95 | 37,295 | −1.22% | −2.98% | 22.1% | −45.4 | **0/12** | 0.3 s |
| starter gap 5–9 | first-gap | 95 | 15,214 | −1.02% | −2.98% | 21.5% | −15.5 | 1/12 | 0.4 s |
| starter gap 10–19 | first-gap | 95 | 9,813 | −1.31% | −2.97% | 18.9% | −12.8 | **0/12** | 0.1 s |
| starter gap 20–39 | first-gap | 95 | 5,811 | −1.45% | −2.97% | 18.7% | −8.4 | **0/12** | 0.0 s |
| starter gap 40+ | first-gap | 95 | 6,306 | −1.25% | −2.98% | 20.8% | −7.9 | **0/12** | 0.0 s |
| starter, his mints | first-gap | 95 | 2,602 | −0.05% | −2.91% | 27.7% | −0.1 | 5/12 | 0.3 s |

First-gap at 0 ms is still the best exit on this entry. Trail and clock-4
give back median; clock-20 is red. Every 95 ms book is **0/12** or 1/12
with a −3% median, including the lookahead `oracle_sep` book and every
gap-length band.

Hold stays 0.1–0.2 s on first-gap even when a separated crowd follows in
the same slot: 95 ms after print 1 fills on those later prints, which is
the completing-print fill. The burst is not a multi-second hold.

## What this is

Gap duration belongs as a mild permission (drop 2–4; 10–19 is the
thermometer peak). It does not flip 95 ms. Firing earlier in the same
slot does not leave remainder past 95 ms. A longer exit on this entry
holds the fade.

Do not fund it. Do not walk another same-slot early print of this burst.
