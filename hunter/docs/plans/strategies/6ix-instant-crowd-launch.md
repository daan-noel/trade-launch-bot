# The 6ix launch cohort

The `[SetComputeUnitLimit, SetComputeUnitPrice, Create_v2, CreateIdempotent,
Pump.Fun: Buy, System Program: Transfer]` creation shape — 86,744 tokens over
07-28..08-25, the third-largest launch fingerprint in the lake and ~20 % of its trade
volume. Fingerprint `4066892c-8990-4c57-88d8-e5e1addf9240` (`6ix:Transfer · bkt=1000`)
matches the shape with every amount axis open.

**No rule on this cohort is tradable.** Every entry derived here reduces to the price
impact of the buy that triggers it — see
[`2026-08-26-6ix-cohort-rules-are-intra-slot-impact.md`](../../history/2026-08-26-6ix-cohort-rules-are-intra-slot-impact.md)
for the three that were refuted and how. What survives is below.

## What holds: the gates are survival filters, and survival is most of the money

The cohort's base rate is brutal. Take the first print past 60 s on every token, hold
30 s, price a bag with no print left at **-100 %** (not 0 %):

| gate | fires/day | mean @30 s | unsellable |
| --- | ---: | ---: | ---: |
| `m_state.time >= 60` alone | 2,145 | -65.33 % | 64.7 % |
| `+ m_flow_lifetime.gross_flow >= 43.6` | 841 | -32.03 % | 31.3 % |
| `+ m_flow_lifetime.trade_count <= 140` | 473 | -47.45 % | 48.6 % |
| `+ m_flow_window(5).buy >= 2.94` | 84 | +19.72 % | 2.0 % |

Nearly two thirds of tokens that reach 60 s never print again. Cutting that to 2 % is an
85 pp swing and it dwarfs every selection effect measured on this cohort. **Rank a
candidate on the unsellable rate first**; a gate that does not move it is not competing
for the thing that matters here.

The trap is that on this cohort the strongest survival term is also a burst detector, so
the fill lands behind the burst and gives the whole gain back. That is a property of
`buy(W)` gates, not of survival gating — a survival term that is *not* keyed to a buy
landing right now does not have it.

## Fire discipline this cohort forces

**A trailing `m_flow_window(W)` is clipped by the token's own age**, so on a young token
every window returns the same number. Measured on a fire set built as "first qualifying
print per token":

| fire age | n | `gross(3)==gross(60)` | `gross(10)==gross(60)` | `gross(30)==gross(60)` |
| --- | ---: | ---: | ---: | ---: |
| <1 s | 8,258 | 100 % | 100 % | 100 % |
| 1-3 s | 18,647 | 100 % | 100 % | 100 % |
| 3-10 s | 20,636 | 0 % | 100 % | 100 % |
| 10-30 s | 7,119 | 0 % | 0 % | 100 % |
| 30-60 s | 1,156 | 0 % | 0 % | 0 % |

**48 % of that fire set has all five windows identical.** A search over "5 metrics x 5
windows" there is 5 features wearing 25 names, and it reports the multi-window vocabulary
as worthless when it was never populated. First-qualifying-print puts nearly every fire in
a token's first seconds, so **a multi-window study needs its own age floor** — offer a rung
only the windows `W <= its age floor`, and carry that floor into the shipped rule.

## The vocabulary built here, which does hold

Four metrics were added to state these rules in the terms they were derived in. They are
correct measurements of what they name, with a parity harness against real lake tapes in
`engine/src/metrics/track.rs`, and they outlive the rules:

* `m_flow_lifetime.trade_count` — maturity; monotonic, so an upper bound is a one-way door.
* `m_flow_window(W).trades_per_wallet` — a crowd vs one wallet churning, as a **count
  ratio, never an identity**, so wallet rotation does not defeat it.
* `m_flow_window{W, b}.trade_share` — how concentrated the tape is in time, scale-free.
* The `first_slot_buy_lamports` **fingerprint axis** — the launch size as a threshold.
  It is an axis and not a condition: the number is fixed by the creation slot, so it
  selects which tokens arm. `>= 6.41 SOL` is the open range `{"min": "6410000000"}`.

Definitions: [metrics-reference.md](metrics-reference.md). How to read a cohort rule and
re-derive one: [cohort-entry-rule-anatomy.md](cohort-entry-rule-anatomy.md).

## Reproduction cases

Both refuted rules are in `strategy_rules`, inactive, tagged
`stage-refuted,intra-slot-impact`, with their exact JSON pinned in
`engine/tests/six_ix_cohort_rules.rs`:

| rule | id |
| --- | --- |
| 6ix sustained-flow (winner) | `8f5d56ab-8832-40e7-8e84-80ab5272a5d6` |
| 6ix crowd-acceleration (runner-up) | `1c5e0789-a9b5-4e11-8ac3-5d1e2aa99395` |

They are kept so the next cohort search can be checked against a known-bad shape: run one
at `lag_115` / `pumpfun_impact` and it books about -13 %/trade, and its entry-lag ladder
collapses between 0 and 25 ms. That collapse is the signature to look for.
