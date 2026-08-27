# Burst wallets: old-on-chain vs born this mint

Among first-on-this-mint wallets, split those that **already trade other
tokens** from those whose **first tape buy is this slot**. Completing print
is unchanged: extra conjunct on the live cell from
[ix-new-wallets.md](ix-new-wallets.md). Reproduce with `ixg-old-wallets.sql`
+ `ixg-old-wallets.py`. Scratch: `ixg.ow_ids`, `ixg.ow_fs`, `ixg.bmem_old`,
`ixg.bold`. Same corpus as [ix-burst-kinds.md](ix-burst-kinds.md).
Thermometer only.

A member is **old** if its first `trades` buy-slot is earlier than this
print (another mint, because it is already `wal_new` here). It is **strict
born** if that first-buy slot equals this slot. `later` / `no_fs` are tape
holes (this print is missing from the current `trades` window, so first-seen
lands after it) — they are not born. **pre_mint** = first buy is before
`tokens.created_at`. **hopper** = old, but first buy is after this mint
launched.

`wallet_dict` has no first-seen column. First-seen is `min(slot)` per
wallet on `trades` (tape starts 2026-07-28). Left-censor can hide older
history; that labels some old wallets born, which shrinks a real split, it
does not invent one.

## Named families do not split

`all_new`, tot in [0.9, 4):

| Kind | all_old resp / causal | all_born (incl. holes) |
| --- | ---: | ---: |
| `multi_tmpl_nwal` | 10.01 / 2.05 | 10.40 / 1.32 |
| `same_tmpl_nwal` | 5.78 / 2.48 | 5.77 / 3.21 |

Working-template wallets inside `same_tmpl_nwal`: 6.03 vs 6.26. Door +
`vsol < 46`: multi 13.21 vs 14.87, same 9.05 vs 9.22. Token-age bands keep
the same shape. ATA + born (6.06) matches ATA + old (5.83).

The equal rates are the hole: strict same-slot born is **n = 27** on
multi (0%) and **n = 231** on same (3.03 / 2.60 vs old 5.78 / 2.48). The
`all_born` mass is almost all `later`/`no_fs`, and those respond like old
because they are old. True creator-batch wallets are rare in the named
burst; when they appear on `same_tmpl_nwal` they are weaker, not a second
family.

## Solo is the only real split, and it is small

`solo` `all_new` tot [0.9, 4):

| | n | resp / causal |
| --- | ---: | ---: |
| all_old | 35,105 | **3.18 / 1.50** |
| all_born (incl. holes) | 4,167 | 1.34 / 0.89 |
| strict born | 2,922 | 0.92 / 0.82 |
| old + working | 11,163 | 3.36 / 1.61 |
| born + working | 1,193 | 2.51 / 1.34 |

Old solos lift; born solos do not. Hopper solos are dead (0.28%, n=358).
This is still the solo family already priced in
[ix-new-wallets.md](ix-new-wallets.md) (first-gap 95 ms **0/12**). Requiring
old does not change the fill.

## Old is the tape, not a preference

Of first-on-mint burst prints, **426k are pre_mint**, 3.7k hopper, 43k
labeled born (of which 30k strict). Hopper is not a kind. **pre_mint ≈
old**: a first-on-mint wallet that already exists almost always predates
this token.

Of his quiet-resume fires, **~90% sit on `all_new` + `all_old`**. That is
prevalence (the burst wallets are already old), not a gate he adds on top.

Chain-age of the youngest new wallet inside `all_new` multi/same is a mild
gradient (`ge7d` a bit above `lt1h`), not a trigger. It is not
[fresh-wallet-entry-rule.md](fresh-wallet-entry-rule.md) (`c_fresh1h` is a
trailing share). First-on-mint stays the live exclusion; chain-age does not
replace it.

## What this is

Do not require old-on-chain as a conjunct on the named burst. Do not treat
born-this-mint as a hard skip there — there is almost no honest born mass,
and the labeled born is a tape hole. On solo, old vs born is real and too
small to price. Do not fund it.
