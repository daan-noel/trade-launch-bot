# Burst wallets: first-on-this-mint vs repeat

The signal wallets in a gap-then-burst are either **new to this mint** (no
curve buy on it in any earlier slot) or **repeat**. Wallet grain: a second
print of a new wallet in the same slot is still a new wallet. Reproduce with
`ixg-new-wallets.sql` + `ixg-new-wallets.py`. Scratch: `ixg.wal0`,
`ixg.bmem_new`, `ixg.bnew`. Same corpus as [ix-burst-kinds.md](ix-burst-kinds.md)
(his mints, his fills out of the burst). Thermometer only.

`new_kind` on the completed slot: `all_new` / `mixed` / `all_rep`.

## He does not fire on repeat-only bursts

Quiet-resume slots, any size:

| Kind | all_new resp / causal | all_rep |
| --- | ---: | ---: |
| `multi_tmpl_nwal` | 7.43 / 1.52 | 0.02 |
| `same_tmpl_nwal` | 3.74 / 1.64 | 0.07 |
| `solo` | 0.90 / 0.41 | 0.04 |

Tot in [0.9, 4):

| Kind | all_new | mixed | all_rep |
| --- | ---: | ---: | ---: |
| `multi_tmpl_nwal` | **9.77 / 2.04** | 3.28 / 0.54 | 0.01 |
| `same_tmpl_nwal` | **5.68 / 2.50** | 2.80 / 0.85 | 0.15 |
| `solo` | **2.98 / 1.43** | - | 0.10 |

Of his quiet-resume fires: **78.3%** sit on `all_new`, 18.3% on `mixed`,
**1.7% on `all_rep`**. Repeat-only is the exclusion. Mixed still precedes
fires (mostly `multi_tmpl_nwal`); it is not required that every wallet is new.

The 36% solo share in [ix-burst-kinds.md](ix-burst-kinds.md) is almost all
`all_new` (2,210 hits vs 92 repeat). Repeat solos are not a family.

## Not the ATA flag

Working-template prints are 89.6% first-on-mint, so the named same-template
family is already mostly this cut. ATA itself is not a substitute:

| Members | n share |
| --- | ---: |
| ATA and new | 44.5% |
| ATA and repeat | 22.3% |
| no ATA and new | 11.0% |
| no ATA and repeat | 22.2% |

Inside `same_tmpl_nwal` tot [0.9, 4): ATA + `all_rep` is 0.39% resp; no-ATA +
`all_new` is 4.74%. Repeat with ATA is dead; new without ATA still lives.

This is not [fresh-wallet-entry-rule.md](fresh-wallet-entry-rule.md)
(`c_fresh1h` = chain-age share, a trailing screen). First buy **on this mint**
is a property of the event.

## Not age, not a vsol proxy

`all_rep` stays dead in every age band (0.00-0.59%). `all_new` lives in each
and peaks at 20-60 s (12.66%). Older tokens lower the rate; they do not flip
the sign.

Inside the door + named + tot-band + `vsol < 46` set
([ix-perm.md](ix-perm.md)):

| Kind | all_new resp / causal | mixed | all_rep |
| --- | ---: | ---: | ---: |
| `multi_tmpl_nwal` | **13.00 / 2.62** | 6.39 / 1.08 | 0.03 |
| `same_tmpl_nwal` | **8.96 / 3.87** | 7.07 / 2.02 | 0.34 |

`vsol >= 46` remains 0% for `all_new` and `all_rep`. Depth stays a permission.
First-on-mint is the extra exclusion inside that permission.

## What this names

An extra conjunct on the gap-then-burst event: **the signal wallets are new
to this mint**. Completing print is unchanged in shape - the print that makes
the conjunction true - but a solo `all_new` completes on print 1, which is
earlier than `fam_n >= 2`.

Do not require `all_new` as the only family (`mixed` still hits). Do require
the exclusion: do not fire on an all-repeat burst.

## Full-tape money (earlier completing print)

Door, in-window create, 5-slot gap, tot in [0.9, 4), `vsol < 46`. Fire = the
print that first makes the family true. Same fill as
[ix-machine-money.md](ix-machine-money.md) (95 ms = last print with
`ts <= fire + 95 ms`). One episode per mint. Reproduce:
`ixg-new-money.sql` + `ixg-new-money.py`. Scratch: `ixg.ncand`.

`solo_new` = first-on-mint print 1 in that band (working-template subset
separate). `same_new_work` / `mixed_new` = all-new bursts. `*_rep` = all-repeat
controls.

| Book | lag | n | mean | med | win | SOL | days+ | hold p50 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| solo_new_work first-gap | 0 | 29,855 | **+3.36%** | **+1.84%** | 55.4% | +100.3 | **12/12** | 0.1 s |
| solo_new_work first-gap | 95 | 29,855 | **−1.35%** | −2.98% | 20.0% | −40.3 | **0/12** | 0.2 s |
| solo_new_work clock 20 s | 0 | 29,855 | −1.34% | −5.15% | 38.2% | −40.0 | 2/12 | 16.9 s |
| solo_new first-gap | 0 | 51,044 | +2.05% | −1.94% | 43.9% | +104.7 | 12/12 | 0.1 s |
| solo_new first-gap | 95 | 51,044 | −1.32% | −3.01% | 19.1% | −67.2 | 0/12 | 0.1 s |
| same_new_work first-gap | 0 | 25,986 | +2.30% | +0.67% | 53.6% | +59.8 | 12/12 | 0.2 s |
| same_new_work first-gap | 95 | 25,986 | −1.05% | −2.97% | 21.8% | −27.3 | 0/12 | 0.2 s |
| mixed_new first-gap | 0 | 30,955 | +0.08% | −2.90% | 29.3% | +2.6 | 8/12 | 0.1 s |
| mixed_new first-gap | 95 | 30,955 | −1.09% | −3.00% | 18.9% | −33.6 | 0/12 | 0.2 s |
| solo_rep first-gap | 95 | 13,522 | −1.69% | −2.95% | 17.3% | −22.9 | 0/12 | 0.0 s |
| same_rep first-gap | 95 | 9,464 | −1.45% | −2.94% | 18.5% | −13.7 | 0/12 | 0.1 s |
| mixed_rep first-gap | 95 | 5,716 | −1.91% | −2.93% | 13.8% | −10.9 | 0/12 | 0.0 s |

Zero-lag `solo_new_work` is body-driven (positive median, 55% win, every day
green). The hold is **0.1 s** — that one print *is* the move, then the gap.
Clock-20 at 0 ms is already red: no 20-second tail.

95 ms is red **every day** on every live book. Repeat-only controls stay red.
On his mints only, `solo_new_work` at 95 ms is +0.25% (8/12, median still
−2.88%); on other mints −1.51% (0/12).

A 0.9–4 SOL first-on-mint print is earlier in print count than `fam_n >= 2`
and later in the price path than a burst of ~0.3 SOL prints. It does not
leave more remainder. Do not fund it. Do not re-price the 0 ms mark as a
20 s trade.

Old-on-chain vs born-this-mint on the same cell:
[ix-old-wallets.md](ix-old-wallets.md). Solo as a turn (dip already
true behind the print): [ix-solo-turn.md](ix-solo-turn.md) — same
completing print, 95 ms still **0/12**. Combined with the crowd, tight
packs unfillable: [ix-combined-machine.md](ix-combined-machine.md).
