# Fingerprint `3ix:Buy · max=4.08 · bkt=exact` — the 4 SOL launcher

What this fingerprint matches, how its launches behave, and why **no authorable rule on it
clears the round-trip cost**. Derived from the local lake (`tokens.parquet` + `trades/`),
cutoff 2026-08-16. Read the execution-timing section before reusing any method here — the
trap it describes applies to every bundled-launch fingerprint.

## What the fingerprint is

The creation tx carries exactly three instructions —
`Pump.Fun: Create_v2` / `Associated Token: CreateIdempotent` / `Pump.Fun: Buy` — with no
compute-budget preamble, and `max_sol_cost = 4.08 SOL`. **Every** match has a dev buy of
exactly `4.0` SOL, so `4.08` is a fixed 2% slippage preset over a fixed size. 1,420 tokens
match, 07-23 .. 08-16, running at ~55/day.

It is a **tool**, not a creator: 60 distinct dev wallets share it, one (`GeUnv1jm…`)
launching 808 of them.

### The launch is a bundle with a fixed cast

Three wallets buy alongside the dev in the creation slot, in ~800 launches each —
`8ZMvQdHB…` (1.65), `HMabyrU7…` (1.11), `E87AXuFw…` (1.75). Dev 4.0 + 4.51 = **8.51 SOL**
first-slot buy, the modal `first_slot_buy_sol`. A second preset — `1.58 / 1.66 / 1.70`,
first-slot **8.94** — accounts for 182 launches and behaves differently (below). Any count
above the bundle's three is an *outside* sniper joining the slot.

The two presets are the tool's only real variation, and their mix moves week to week:

| week | launches | 8.94 preset | 8.51 preset |
| --- | --- | --- | --- |
| 07-20 | 35 | 34.3% | 2.9% |
| 07-27 | 200 | 17.0% | 58.5% |
| 08-03 | 502 | 9.6% | 53.8% |
| 08-10 | 683 | 14.2% | 38.5% |

Launches cluster hard in **01:00–10:00 UTC** (1,285 of 1,420) and stop almost entirely after
15:00 UTC.

### The dev is the dumper, and the launch is the product

The dev sells at **median slot 3** (p25 2, p75 7), taking a median **5.83 SOL** out of the
4.0 SOL buy (+45%), and accounts for **80% of all sell volume in the first 10 slots**. Over
the first 25 slots the creator side nets **+1,423 SOL** and everyone else nets **−12,651
SOL**. Median token life is 146 s; 344 of 1,374 stop trading inside a minute.

## The price path

The first print we can reach is already **+30%** over the creation spot — the bundle *is*
the pop. From there:

| horizon | median max-up | median max-down | median return |
| --- | --- | --- | --- |
| 5 s | +10.9% | −7.1% | +1.4% |
| 10 s | +13.4% | −11.6% | −2.3% |
| 30 s | +15.2% | −29.8% | −26.9% |
| 300 s | +15.6% | −39.5% | −39.3% |

All upside lands inside the first ~10 slots; everything after is bleed. One token of 1,374
migrates. There is **no runner tail** — the median peak is +9.7% and only 10% of tokens ever
exceed +30%.

## What the round trip costs

125 bps/leg + 0.000225 SOL/leg + `B/vsol` impact per leg
([execution-costs.md](execution-costs.md)) — at `B = 0.126` SOL against a ~38 SOL pool,
**~3.5%**. The average launch offers ~1.8% of raw edge, so blind sniping loses
**−1.79%/trade** (n=1,374, fill at slot 1).

## The edge exists and is unreachable

The 8.94-preset cohort priced at a slot-1 or slot-2 fill returns **+3.8% / +3.7% per trade**
(n=191, TP +10 / SL −12 / 4 s hold). That is a real, stable signal. It is also **not
tradeable**, and the reason is the arm clock:

| fill slot | mean | median |
| --- | --- | --- |
| 1 | +3.80% | +5.52% |
| 2 | +3.65% | +5.31% |
| **3** | **+1.46%** | +0.66% |
| 4 | −1.39% | −2.58% |
| 5 | −2.77% | −5.73% |

A first-slot fingerprint axis settles on `Event::FirstSlotSettled`, which
`live/src/strategies/engine/producers.rs` emits **on the first trade from a later slot** —
not on a clock. So the decision lands at the first post-creation print, and our buy lands a
slot after that. For the 8.94 cohort that first print sits at slot 1 for 67 tokens, slot 2
for 93, slot 3 for 29 — so **124 of 191 fill at slot 3 or later**, in the region where the
edge is already spent. Simulated end to end that way, the rule returns **+0.48%** (PF 1.07,
train −0.11%, bootstrap 95% CI [−3.9, +5.2]) and **−1.53% at worst-case fill**.

**Any measurement that reads a gate at the close of the creation slot is optimistic by one
slot**, and on this cohort one slot is the whole edge. The honest emulation arms where the
engine arms, never at a slot boundary the engine cannot observe.

### The gates themselves are sound — they just cannot be reached in time

Each was measured and each separates cleanly, at a fill the engine cannot deliver:

* **`first_slot_buy = 8.94` (exact).** The preset separator. 641 trades at +1.33% below the
  threshold, 177 at +4.53% above — a cliff, not a slope.
* **`m_flow_window.unique_wallets <= 4 @2s`.** Exactly the dev plus three bundle insiders and
  nobody else. Outcome falls monotonically with creation-slot buyer count: 3 legs −0.3%,
  4 legs −3.3%, 5 legs −5.2%, 6 legs −10.4%. An outside buyer raises the price we pay the
  bundle *and* becomes a second seller into the same thin follow-on demand.
* Neither survives the extra slot. Nor does the creator wallet, the only zero-latency
  selector available: no dev uses one preset exclusively (best split 56% / 54% / 75%), and a
  causal creator allowlist scores +0.37% with train −0.52% and −2.02% at worst fill.

## The engine's verdict — the rule does not survive

Everything above comes from a standalone path simulator. Run through
`hunter-engine::reduce` (rule `6b1d2c3e`, fingerprint `5c9a4e10` =
`first_slot_buy_lamports 8_940_000_000` + `bkt=exact`, 0.25 SOL, TP +10 / SL −12 /
`m_position.held >= 4`, entry `m_snapshot.time <= 3` + `m_flow_window.unique_wallets
<= 4 @2s`), the same rule reads:

| fill model | cost model | trades | mean | median | win | PF | total |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `first_in_window` | `fee_only` | 157 | +4.10% | +8.79% | 67.5% | 2.00 | +1.61 SOL |
| **`next_slot_median`** | **`impact`** | **157** | **−1.88%** | −0.16% | 49.7% | **0.74** | **−0.74 SOL** |
| `worst_case` | `impact` | 157 | −3.82% | −1.04% | 49.0% | 0.50 | −1.50 SOL |

181 tokens matched, 157 entered. **The sign flips with the fill model**, so the rule is
an execution bet, not an edge — the same kill gate that rejects any config whose money
is a race. At the realistic middle it loses. The standalone simulator scored the same
config at **+3.80%**, so it runs ~5.7 pp optimistic against the kernel: its median-fill
entry and exit both pick the middle of a slot's prints, where the engine takes the
adverse side of the window and closes ~2.2 s in rather than riding the 4 s clock.

**Trust the kernel, not the harness.** Every standalone number in this file is an upper
bound on what the engine books.

## Refuted on this cohort

* **No-TP/no-SL metric exits** (the shape that pays on other launcher fingerprints).
  `retrace >= 25` OR `stall >= 10` scores −11.6% ungated and −13.6% to −15.9% under every
  gate that helps a TP rule. Single-metric exits — `retrace`, `stall`, `trail`, `gross_flow`
  decay, `net_flow < 0`, `sell`, `liquidity` — land between −2.3% and −7.6%, all worse than
  a plain take-profit. Without a runner tail there is nothing for a trailing exit to catch.
* **Selling into the first organic buy.** `nonvol_buy >= 0.3 @2s` as an OR-exit costs
  −2.7 pp. Organic arrival is what a take-profit is *paid by* here, not a reason to leave.
* **Buy the dev dump.** Entering 1/2/3/5 slots after the dev's first sell: −8% to −18% per
  trade at every exit. There is no bounce.
* **Dip entry.** `win_trail >= 6/10/15/25` at 2 s, 5 s or 10 s: −6% to −8% everywhere.
* **Late entry / momentum.** Edge decays monotonically with entry slot.
* **Launch-bundle wallet gates.** A veteran-share screen over the launch window scores
  weakly positive but is dominated by the first-slot sum and adds nothing on top of it.
  The `m_bundle` group that measured it is removed — see
  [`history/2026-08-22-m-bundle-removed.md`](../../history/2026-08-22-m-bundle-removed.md).
* **Trailing stops, exit-on-dev-sell, `unique_wallets` exits, time-of-day, launch cadence** —
  negative or inert.

## If it is revisited

Only a lower decide-to-land latency changes the answer: the edge sits at a slot-1/slot-2
fill and dies at slot 3. That means settling the first slot **on a clock** rather than on the
next trade (a one-second launch window from `created_at` approximates the creation slot
closely enough to reproduce the slot-exact share at r = 0.999), or a creation-slot signal
that needs no settle at all. Absent one of those, this fingerprint is a **negative
screen** — the 8.51 preset loses −1.32% per trade over 642 launches — and nothing more.

A `m_flow_split`-based gate additionally depends on the fingerprint's configured
`volume_ix_patterns`: the same gate scores +5.05% when only the launcher's own
`Unknown (HQ2UU…)` legs are volume-side and −1.16% when the durable-nonce snipers are added
to the list. The stored 56-pattern config on this fingerprint is the latter kind, so a split
metric here does not mean what an author who has not read the list assumes.

Numbers come from a standalone path simulator over the lake, not from
`hunter-engine::reduce` — [strategies.md](../../arch/strategies.md).
