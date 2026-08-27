# 2026-08-27 — the multi-window flow gate on 6ix is the price path

Follows [`2026-08-27-6ix-fails-the-pass-through-bar.md`](2026-08-27-6ix-fails-the-pass-through-bar.md),
which closes the *transplanted* 3ix rule on 6ix. This entry tests 6ix on **its own**
geometry with **its own** gates — simultaneous `gross`/`net`/`buy`/`sell`/`buyshare` at
1s/5s/15s/60s — aiming at tokens that rise consistently without a deep dip.

## Deriving 6ix's geometry instead of borrowing one

3ix targets graduation because 3ix tokens reach it. 6ix tokens peak at a median vsol of
40–56, so graduation is a 0.8–2% tail and the correct target is smaller and more frequent.
Standing on a clean 6ix token, room left:

| standing at vsol | mints | reaches 1.3x | reaches 2x | graduates | median room |
| --- | ---: | ---: | ---: | ---: | ---: |
| under 35 | 50,095 | 51.2% | 17.0% | 0.8% | +31% |
| 40–45 | 35,423 | 43.7% | 18.2% | 2.1% | +22% |
| 65–75 | 548 | 64.6% | 36.8% | 26.2% | +61% |

So the trade to test is entry near vsol 35–50 with a ~1.3x target and a tight stop.

## "No deep dip" alone selects dead tokens

6ix tokens still within 2% of their peak at age 10–60 s have a **median vsol of 30.1–34.4**
— the starting line — and a median remaining move of **1.005–1.15x**. There is no dip
because there are no trades. Loosening the tolerance *raises* forward upside
(dip ≤ 2% → 7.8% reach 1.3x; dip ≤ 20% → 14.9%).

The filter only works **conjoined with having climbed**. At the first touch of vsol 50:
clean (dip ≤ 5%) gives 61.8% reaching 1.3x and a 1.47x median room against 56.1% / 1.386x
unfiltered. Real, and median age at that point is **1.0 s** — the launch burst.

## The flow gates are the price path

`vsol` moves by exactly the net SOL in or out, so `net_flow(w)` **is** the price move over
`w` (corr 0.98, [`curve-flow-is-price.md`](../plans/strategies/curve-flow-is-price.md)).
Qualifying on shape (reached vsol 45, dip ≤ 15%) and then letting a flow gate pick the
moment: `net>0 at 5/15/60s`, `buyshare>=60% at 5/15/60s`, `net>=2/5/10 SOL`, and
`buyshare>=70% + net60>=10` each fire on **17,45x of the same 17,456 tokens**, gap 8.4–8.5%,
mean −7.35% to −7.38%. The conjunction never delays an entry, because a token that just
climbed cleanly already satisfies every sustained-buying condition.

Axes orthogonal to price — `ntx`, `gross`, `gross/|net|`, `gross/ntx` — are all negative
across 50 deciles (−5.35% to −9.27%, best 3/25 days). Absorption is **exactly 1.000** in
the bottom six deciles: 60% of climbs to vsol 45 involve no selling at all, so `gross`
collapses onto `net` as well. Only `ntx` stays independent, and it pays nothing.

## The target group barely exists

Climbed to vsol 45 **and still clean** after the burst, over 25 days:

| age >= | dip <= 5% | dip <= 10% | dip <= 20% |
| --- | ---: | ---: | ---: |
| 30 s | 20 tokens, −5.02% | 63, −15.17% | 563, −9.11% |
| 60 s | 12, −16.98% | 26, −20.08% | 245, −11.07% |
| 120 s | 8, −17.54% | 13, −20.20% | 72, −10.33% |

Entry is now genuinely cheap — gap −0.2% to +2.7% versus 8–14% on burst entries — which
confirms the diagnostic works. The tokens are simply not there, and the ones that are lose.
Tightening the target on them (+15%/−8%, +20%/−10%) does not help.

**On 6ix, "rising" and "no deep dip" are in tension.** Clean-at-age selects inactivity;
climbed-and-clean selects the launch burst; the intersection is 8–63 tokens in 25 days.

## What would still be new information

Not the trade tape — every column of it on this venue is the price path, `ntx`, or churn,
and all three are measured. Only something off the price path qualifies: per-trade
instruction composition, or a mempool-side read that leads the buy. The structural fact
under all of it is that 6ix is **94% a single x1.10 launch tool** whose tokens peak at a
median vsol of 40–56, against the x1.08 tool behind every live rule, which does not launch
6ix at all.

## Verification

`scratchpad/feat.py` (8.66M-print feature table), `step9..step11.py`, `survivors.py`,
`climbed.py`, `geom.py`, `late.py`, `ident2.py`. Honest curve pricing per
[`curve-honest-pricing.md`](../plans/strategies/curve-honest-pricing.md), 115 ms on both
legs, `B = 0.10` SOL, 125 bps a leg, one trade per token. The `ident2.py` identity check
supersedes a first attempt that used `lag()` with a `RANGE` frame, which `lag` ignores.
