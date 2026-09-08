# Machine census: actors as their builds

The census behind [_!___market-model-and-workflow.md](_!___market-model-and-workflow.md)
T5: an actor is his machinery, and the only durable axis is instruction structure. This
file states what the census keys on, what it records, how it is validated, and what it
shows. Schema `aa` on the workstation PG: `pxf` (every curve print post-cutover with fee
fields and both keys), `mach_w` (machine x wallet), `mach` (machine attributes and role).
Window 2026-08-30 17:48 UTC .. 09-06.

## 1. The key

A machine is one build signature: the ordered instruction list of the print, with the
token-account create/close and memo instructions removed (`build_core`), plus whether the
buyer pays his own fee or a proxy pays it. Nothing that costs nothing to change is in the
key: not the priority price, not the tip, not the clip, not the wallet.

Fee, tip and clip are recorded per machine over time as behavior. A machine that changes
its priority price is making a decision on that print, and the change is read as intent,
never as a new identity.

The key resolves the TOOL. Operators inside one tool (a farm of five wallets inside a
12,000-wallet bot service) are visible only through behavior: a fixed fee preset that
differs from the tool's median, a fixed clip, and co-landing in one slot. Behavior can be
faked; structure cannot. A rule term is written on the tool level and the role, never on
an operator.

## 2. What is recorded per machine

Wallets and their rotation, prints and tokens per day, clip distribution, priority-price
distribution and its 90th percentile, tip share, where in a token's life it buys (age
and depth), share of buys in the creation slot, share of first-in-slot prints, nonce or
seed usage, router labels, share of prints by the token's creator, whether its wallets
sell what they buy and how long they hold, and co-landing (how often two of its wallets
buy the same token in the same slot).

## 3. Validation

| check | result |
| --- | --- |
| size | 15,998 machines over 12.8 M prints; 408 machines with 1,000+ prints carry 95 % of prints |
| split-half stability, machines with 200+ prints in both halves (750): days 1-4 vs 5-8 | clip corr 0.89, depth 0.84, sell share 0.999, wallet count 0.97, age 0.63 |
| co-landing, machines with 3+ wallets and 200+ buys | 4 % of buy slots hold two of the machine's wallets at 3-9 wallets, 12-14 % at 10+ (tools: many users, same event); a farm reads 20-85 % |

A machine whose halves disagree is split; two machines whose wallets overlap and whose
halves agree are merged. Both operations are behavior-driven and are re-run weekly.

## 4. What it shows

Roles are assigned from behavior, scale first (a tool with thousands of users is not a
farm), machines with 100+ prints:

| role | rule | machines | prints | wallets | clip | co-landing | net SOL |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| tool_terminal | 1,000+ wallets, router labels | 33 | 5.89 M | 245,901 | 0.21 | 7 % | -45,533 |
| tool_bot | 1,000+ wallets, no router labels | 75 | 3.89 M | 611,867 | 0.19 | 7 % | +206,384 |
| racer | nonce or seed on half its prints | 311 | 1.06 M | 10,939 | 0.64 | 3 % | +60,539 |
| farm | 3-999 wallets, co-landing 20 %+ | 47 | 227 k | 9,555 | 0.63 | 51 % | -120,195 |
| custom_operator | 10 wallets or fewer, 500+ prints | 76 | 198 k | 194 | 0.53 | 2 % | -1,168 |
| creator | half its prints by the token's creator | 53 | 179 k | 43,654 | 5.70 | 2 % | -294,599 |
| bundler | half its buys in the creation slot | 83 | 114 k | 13,934 | 1.50 | 47 % | -178,580 |
| volume_swarm | zero-size clips, co-landing 50 %+ | 1 | 89 k | 2,735 | 0.00 | 85 % | -1,640 |
| terminal_small, other | the rest | 1,316 | 1.0 M | 376,696 | 0.18 | 1 % | +49,904 |

A tool is a buy build paired with a sell build through the wallets that use both: the
two largest terminals pair at 94 % and 80 %, the bot service behind the farm at 68 %.
One generic sell build (`d0516a3d`) serves many buy builds, so the buy side carries the
identity and the sell side carries the pairing.

Token shape by launch group (post-cutover, 5+ prints) is a door fact the census reads
directly. The bundler launch `3ix:Buy` (dev buy 0.9 SOL) peaks at a median depth of 78,
39 % reach the wall and 36 % dump by half from the peak: a continuous rise and one dump.
The four mainstream groups the readers trade (`6ix:Transfer`, `5ix:BuyV2`,
`7ix:Transfer`, `6ix:BuyExactSolIn`, 55 k tokens) peak at 34-39, only 7-10 % ever reach
60, 5-7 % dump by half, median life ~200 s and 9 buyers. In that universe a runner is a
one-in-twelve event and the door does not select it; the event must.

Named machines worth knowing:

- The largest machine is a retail terminal: 60,608 wallets, 3.3 M prints, 229,857
  distinct priority prices. Structure identifies the tool; its users are indistinguishable.
- The farm behind reader 796 lives inside a 12,256-wallet bot service (buy build
  `1b55e2c4`, sell build `cd828583`, priority 4 M, tips on 93 % of prints). The farm keeps
  a fixed 700 k preset, one variant per wallet, against the tool's 4 M median.
- A zero-size volume swarm: 2,735 wallets, 522 tokens, clip 0.000, 85 % co-landing, up to
  46 wallets in one slot. This is the volume build of T5 with rotating wallets.
- A racer operator: 66 wallets, priority 12.9 M, nonce or seed on every print, clip 0.49.
- 8dtx (2720) runs a custom build shared by four wallets (`77a7cd2f`, 2,548 prints). An
  operator is separable by structure only when he builds his own transactions.

## 5. Limits

The workstation holds no raw transactions, so the key cannot use lookup tables, account
keys or failed transactions. A tool's users are one machine. Fee-based operator
separation inside a tool is a behavior read and is not used as a rule term.

## 6. The launch-build door for runners

A launch build's wall rate is a stable property of the build and it is known before the
token exists. Door = launch build ranked by its TRAILING wall rate (tokens created before
day D; a wall counts only if reached before D starts). Forward validity on this window,
tokens created on days 2-8 (`scratchpad/door2.sql`, `door3.sql`, `door4.sql`; tables
`aa.door`, `aa.door2`, `aa.door3`):

| door | tokens | builds | forward wall rate | pool |
| --- | ---: | ---: | ---: | ---: |
| trailing wall >= 10 %, 30+ tokens, outside the bundler groups | 1,059 | 9 | 36.8 % | 0.7 % |
| trailing wall 4-10 % | 4,312 | 16 | 8.1 % | |
| trailing SLOW wall >= 5 % (wall reached after 60 s of life) | 1,507 | 10 | 8.6 % slow walls | 0.76 % |

The first row is mostly instant pumps in disguise: its tokens sit at depth 60 by age 15 s
and decay, negative at every entry age. The slow-wall door excludes them and is the
runner selector: 11x the pool, holding on the later days.

Money inside the slow-wall door, organic builds, creator not sold, entry at a fixed age,
slot-end fill: +3.3 / +4.1 / +2.9 / +1.8 / +2.5 % at ages 15 / 30 / 60 / 120 / 240 s on a
600 s clock, positive at every age, 2-4 days of 7. With the creator already sold the same
entries read -4 to -9 % at every age: the creator's state is the permission that matters
on a door token. The dip re-entry event on door tokens with the wide trail reads +17 % a
trade on 721 fires, 5 of 6 days; split by build, 523 of those fires are the two-day
client `5ix:ix#6f` and 67 are the one-day client `18ix:ix#06`, together most of the SOL.

So the door works as a selector and its money is the rotating client: one or two builds
per day carry it and they live one to two days. A weekly re-screen is the wrong cadence;
the door is refreshed daily from the previous day alone (20+ tokens, slow-wall rate
5 %+) and it reads the same forward (8.2 % slow walls on 1,475 tokens, 13 builds).

## 7. The door is a selector; the trade built on it is refuted at our seat

The study runs on 2026-08-08 .. 09-07 (30 days, 736,800 tokens). Days 08-09 .. 08-30 form a
22-day holdout no pass has fitted on; 08-31 onward is the window every earlier result came from.

**What survives.** The launch-build door is a genuine, forward-valid selector of tokens that
run. On every one of the 30 days its tokens reach the soft label (reserve 60 with the peak
60 s+ after birth) at 8.5-19.6 % against a pool of 1.9-4.3 %, a 4-6x lift with no decay across
the holdout. The creator permission also discriminates: creator-sold entries are worse on every
book tested. Both facts are properties of token outcomes and are independent of any fill model.

**What does not.** Every entry rule built on that door is negative once the exit leg is priced
at a fill we can actually reach. This closes the door-rule line as a trade, not as a screen.

## 8. The exit leg is the whole result, and it is unreachable

A trailing stop breaches on a print we observe. That print has already happened, so our sell
lands after it. Pricing the exit at the breaching print's own reserve is `signal_price`, the
zero-slippage ceiling, and it is what every book in sections 7 of the earlier revisions used.
The entry was priced conservatively (the end of the trigger's slot) the whole time, so the
study charged the buy leg for latency and gave the sell leg away.

Holding the entry at that same conservative fill and moving only the exit:

| exit fill | MAX SOL rule | SAFETY rule | previously shipped rule |
| --- | ---: | ---: | ---: |
| the breaching print (unreachable ceiling) | +151.10 | +49.79 | +53.17 |
| 50 ms after it | -2.26 | -8.21 | -74.25 |
| 115 ms - the bot's measured decide-to-fill p50 | -29.06 | -18.94 | -99.70 |
| 235 ms - its p90 | -51.10 | -27.15 | -117.54 |
| 400 ms - one slot | -78.86 | -39.10 | -143.51 |

Break-even sits under 50 ms. The bot runs at 115 ms p50, and no latency buys the right to be
sequenced immediately after a chosen print, so the band above 0 ms is the only reachable one.

Three things make the loss this large, and all three are measured here rather than assumed:

* The breaching print has already gapped past the trail level - median reserve 0.978 of the
  level, p10 0.690 - so the fill was never at the threshold. That part was already honest.
* 68 % of breaches sit in a slot that holds further prints. The price ratio from the breach to
  the end of its own slot has median 0.995 but mean 0.883 and p10 0.488: a minority of breaches
  are the leading edge of a cascade, which is exactly the adverse selection
  the operator memory records for down-triggered exits.
* The tail, not the median, carries it. At the median slip the rules survive; at the mean they
  do not.

**Non-reactive exits do not rescue it.** A clock exit is unbiased and a take-profit is
favourably selected, so both should fare better than a trail. On the same populations at 115 ms
the best of 24 clock and take-profit variants is a 30 s clock at -0.28 % a trade on the MAX SOL
population, -0.66 % on the previously shipped one. Removing the reactive exit removes the
apparent edge with it, which says the edge was the exit fill rather than the entry.

**This is not specific to the new rules.** The previously shipped rule loses hardest of the
three (-99.70 SOL at 115 ms), so the refutation covers the whole line, including the armed
trail and the unarmed-stop variant that preceded it.

**The unarmed-trail finding stands only as a relative one.** Against the armed trail on
identical fires the unarmed trail is better at every exit fill tested, because an armed trail
leaves a position with no stop at all below its arm level. It is a real improvement to the exit
and it does not make the book positive.

## Reproduce

`scratchpad/census.sql` (build), `census2.sql`, `census2a.sql` and `roles2.sql` (validation, roles, pairing), `shape2.sql` (token shape per launch group, table `aa.shape`), `door2.sql`..`door5.sql` (the launch-build door and its money), `w1.sql`, `w1b.sql` (span-wide door statistics and the path export), `w2.py`, `w2v.py` (events, permissions, exits per token), `w3.py` (books, walk-forward, null); the 30-day rebuild is `x1.sql` (door and path export), `x4.sql` (label-derived role map), `x2.py` (fires and a 236-exit grid), `x5.py` (census columns), `x3.py`, `x6.py`, `x7.py` (search and honesty tests), `x8.py`..`x12.py` (the fill-model audit that closed it).
