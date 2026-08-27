# Kind-gap (real-print silence, not all-buy silence)

Quiet of **first-on-mint working-template** prints, while other buys may
continue. Completing print = the first real print after >= 5 slots with no
real print. First buy-slot of the mint is out. Reproduce with
`ixg-kind-gap.sql` + `ixg-kind-gap.py`. Scratch: `ixg.kall`, `ixg.kreal`,
`ixg.kbrk`. Same corpus as [ix-burst-kinds.md](ix-burst-kinds.md).
Thermometer only.

`buy5` = no curve buy of any kind in the previous 5 slots (the old quiet).
`kind5_not_buy5` = real-print gap while fake/repeat volume is still printing
(the mid-run family this cut was meant to name).

## Mid-run kind-gap does not lift

| Set | n | resp | causal |
| --- | ---: | ---: | ---: |
| First real print in a slot (no gap filter) | 386,544 | 1.80 | 0.56 |
| Kind-gap breaker | 204,380 | 2.37 | 0.72 |
| Kind-gap and all-buy quiet | 116,195 | **2.94** | 0.87 |
| Kind-gap, buys still printing | 88,185 | **1.62** | 0.52 |

Mid-run is below the unfiltered real-print base. He prefers **no buys at
all**, not silence of real buyers inside fake volume.

Completing-print SOL in [0.3, 2) keeps the same shape on both sides, quieter
is better: all-buy quiet 5.16–5.52% vs mid-run 3.16–3.55%. Dust `< 0.3` and
`>= 4` stay dead. Longer kind-gaps do not rescue mid-run (1.43–1.97% across
5–40+ slots).

Of his 11,010 fire slots, 4,845 sit on a kind-gap breaker in S or S-1. 3,412
of those are all-buy quiet; **1,433 are mid-run** — coverage, not precision.

## What this is

The all-buy 5-slot gap already is the kind-gap that matters. Fake volume in
that window is not a second trigger; it is dilution. Do not price mid-run
kind-gap. The live quiet remains the old one, already priced and dead at
95 ms in [ix-new-wallets.md](ix-new-wallets.md) / [ix-machine-money.md](ix-machine-money.md).
