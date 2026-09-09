# Frozen sentences

Every rule statement frozen **before** its holdout was read, in date order, newest last.

**Nothing in this file is ever edited after its date.** That is the whole point: a sentence
that can be adjusted after the holdout is read is not a test of anything. A sentence that
turns out wrong keeps its text and gains a RESULT line; it is never trimmed, re-worded or
deleted. A better idea becomes a **new** sentence with a new date, and the holdout weeks the
old one consumed are burnt for it.

Verdicts and the reasoning around them live in
[../_!___strategy.md](../_!___strategy.md); the measurements live in
[../_!___evidence.md](../_!___evidence.md).

---

## FROZEN 2026-09-07 19:05 UTC - N4 no-initial-buy door

Frozen before any holdout print is read (weeks 08-11..08-17, 08-18..08-24, 08-25..08-31).

STORY. The creator took no initial buy at creation and has never traded the token, so the
launcher holds no supply to dump into the first strangers; the token's life is carried by
arrivals, and the only extraction line the dev has is fee income, which needs those arrivals
to continue. A terminal user arriving with size mid-tape puts the token in front of the feeds;
with no launcher supply overhanging, the next arrivals are not sold into by the cohort, so
flow continues for tens of seconds after our entry.

```
DOOR        the creation transaction carries no buy: tokens.initial_buy_lamports IS NULL
            (creation-time fact; engine axis init_buy)
EVENT       a buy of >= 0.5 SOL through a router build (terminal brand in the labels: Axiom,
            Terminal, GMGN, Trojan, Bloom, Jupiter, BondingCurveV3, Photon, BullX, Nova,
            Padre); we fire on that print
PHASE       token age 10-600 s; reserve before the print 33-55 SOL
PERMISSION  the creator wallet has not traded the token before the print (no buy, no sell)
RE-ENTRY    one position per token at a time; a fire while a position is open is skipped
SIZE        0.2 SOL
EXIT        primary: clock 45 s (scalper law - sell into the wave)
            declared alternative: trail 30 % of price from the running peak, cap 600 s
            (harvester law). Both read; neither may be swapped after reading
FILL        last print landed by decision + 115 ms on both legs; stress = end of the decision
            slot, both legs
COST        125 bps a leg, 0.000225 SOL a leg, exact curve arithmetic on the virtual reserve
```

FITTING WEEK 09-01..09-07: clock 45 -> +12.58 SOL, +3.10 %/trade, 2,029 fires on 741 tokens,
5/7 days, worst -0.25; trail30 c600 -> +10.99 SOL, +3.33 %, 1,650 fires, 5/7, worst -0.75.
Random buys on the same door tokens: +0.65 %.

ACCEPTANCE: positive total SOL in each of the three weeks; a majority of days positive in each
week; worst day small against a normal day; no_print share and gap-to-next-print reported. A
red week means the story is wrong; the sentence is not trimmed to fix it.

**RESULT: REFUTED on the disjoint holdout.** The frozen sentence reads red on all three earlier
weeks. The fitting week was one launch machine active two days. Runner: `n4_holdout.py`.

---

## FROZEN 2026-09-08 - the documented-project rule

Frozen before any holdout week is read.

STORY. A launch that PRESENTS as a project - the metadata document offers a website plus either
a channel or a written description - is one whose dev intends to keep promoting it, because his
fee income needs continued flow and the presentation is what converts feed placement into
arrivals. Several INDEPENDENT professional operators (builds run by <= 50 wallets, so one
operator's own tool rather than a retail terminal) have already committed to it, which is
agreement between separate research processes. The token is past its scramble, so the sniper
and bundle supply has already been dumped, and it sits in the reserve band where there is still
room to the wall at 114.9 but the curve is deep enough that our own 0.2 SOL clip barely moves
it. In that state the arrivals keep coming after our entry.

```
DOOR        the metadata JSON behind tokens.meta->>'uri' has a non-empty website AND
            (a non-empty telegram OR a description longer than 80 characters)
PERMISSION  token age >= 300 s; reserve before the print in [50, 85]; at least 8 DISTINCT
            professional buy builds have already bought this token
EVENT       any buy of >= 0.5 SOL
RE-ENTRY    unlimited, one position at a time
SIZE        0.2 SOL
EXIT        trail 40 % of price from the running peak, cap 1800 s   (declared primary)
            alternative read, not swappable after the fact: trail 40 % cap 900 s
FILL        last print landed by decision + 115 ms on both legs; stress = end of the decision
            slot
COST        125 bps a leg, 0.000225 SOL a leg, exact curve arithmetic on the virtual reserve
```

FITTING WEEK 09-01..09-07: +1.18 SOL, +2.36 %/trade, 251 tickets on 96 tokens, 6/7 days, worst
-0.30, win 36.7 %, max 4 positions open. Standard error on the mean is 3.18 points, so the
fitting week ALONE does not establish it. Every neighbouring cell inside reserve 50-85 is
positive (+1.5 to +2.4 %) and widening the band to 40-95 or 33-115 turns it negative, so the
reserve term is load-bearing and the region is a plateau rather than a spike.

ACCEPTANCE on the three holdout weeks (08-11..08-17, 08-18..08-24, 08-25..08-31): positive
total SOL in each week and a majority of days positive in each.

**RESULT: WEAKLY POSITIVE, and it is a lead rather than a rule.** Only two holdout weeks exist
(metadata capture starts 2026-08-18): +1.46 % and +0.64 %/trade, 4 of 7 days each, +3.77 SOL on
2,156 tickets. Every week sits under one standard error from zero, and the declared alternative
cap (900 s) is not consistently positive. Runner: `holdout_v2.py`.

**Note added 2026-09-08, after the result, and changing nothing above:** the door is very
likely mis-weighted - see the next sentence. This one's verdict stands exactly as recorded.

---

## FROZEN 2026-09-08 - the telegram-first survival door

Frozen before any qualifying day is read. **Read only on days from 2026-09-08 forward**: the
two holdout weeks that exist are burnt by the sentence above, and this is a new sentence rather
than a trim of it.

STORY. Identical to the documented-project rule, with one term re-weighted. A telegram channel
is the strongest available evidence that a dev intends to keep promoting: it is the only one of
the three presentation fields that is a live channel to an audience rather than a static page,
so it proxies creator effort, discoverability to buy-side bots, and self-selection by creators
who already expect to succeed. The previous sentence made the weakest field mandatory and the
strongest optional.

```
DOOR        the metadata JSON behind tokens.meta->>'uri' has a non-empty telegram AND
            (a non-empty website OR a description longer than 80 characters)
PERMISSION  unchanged: age >= 300 s; reserve in [50, 85]; >= 8 distinct professional builds
EVENT       unchanged: any buy of >= 0.5 SOL
RE-ENTRY    unlimited, one position at a time
SIZE        0.2 SOL
EXIT        both families read side by side, neither swappable after reading:
              harvester - trail 40 % from the running peak, cap 1800 s (declared primary)
              scalper   - resolved on prints, cap 300 s
            tail concentration reported beside both: share of net from the top 1 % of trades
            and from the single largest token
FILL        last print landed by decision + 115 ms on both legs; stress = end of the decision
            slot
COST        125 bps a leg, 0.000225 SOL a leg, exact curve arithmetic on the virtual reserve
```

WHY THIS WEIGHTING, declared before reading: our own single-field gradients on a 30 % trail are
telegram 6.6 points, website 5.8, description 3.3; an independent survival analysis of 832,941
launches ([arXiv 2607.02823](https://arxiv.org/html/2607.02823)) reports telegram 8.94x lift /
Cox HR 5.40 against website 1.67x / HR 1.19 and twitter 1.52x / HR 1.31.

ACCEPTANCE: positive total SOL in each read block and a majority of days positive in each,
under the harvester exit, with the scalper column reported beside it. A red block means the
story is wrong; the sentence is not trimmed to fix it.

**RESULT: not yet read.** No qualifying day has accumulated.
