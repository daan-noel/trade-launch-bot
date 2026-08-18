# Veteran wallets — the `m_bundle` metric group

`m_bundle` answers a question no other metric group asks: **whose** SOL opened the token,
rather than how much moved or where the price went.

* `veteran_share` — percent of launch-window buy SOL from **veteran** wallets
* `veteran_wallets` / `fresh_wallets` — distinct launch-window buyers of each kind

A **veteran** is a wallet that bought at least `veteran_min_launches` (default 25) of the
same fingerprint's **earlier** launches.

## What the number actually measures

Read it as *"the regulars showed up, rather than wallets nobody has seen before"* — **not**
as "the launcher's own bundler wallets funded this". The two readings give the same number
and completely different reasons to trust it, so the distinction decides how the metric is
used.

The `3ix:Buy · max=0.108` cohort, 30 days to 2026-08-18, is the shape to check any new
cohort against:

| launches | distinct creation-slot wallets | wallets with >= 25 launches | one-shot wallets | busiest wallet |
| --- | --- | --- | --- | --- |
| 1276 | 2008 | 91 | 1463 (72.9%) | 363 launches |

Three of every four wallets appear exactly once and never return — those are the rotating
bundlers, and no roster can or should hold them. What the roster catches is the other tail:
a small, persistent crowd that keeps showing up, one member of it in 28% of all launches.
`veteran_share` is high when that crowd opened the token and low when it did not.

The split is invisible to every price and flow metric — it measures |r| <= 0.19 against
liquidity, gross flow and `unique_wallets`.

### The roster does not rotate away

Built from the older half of a 30-day window and applied **only** to the newer half — a
strict out-of-sample test, no wallet counted from a launch it is being scored on:

| later launches | >= 90% veteran | <= 10% veteran | in between |
| --- | --- | --- | --- |
| 596 | 286 (48.0%) | 69 (11.6%) | 12 (2.0%) |

A two-week-old roster still explains at least 90% of the creation slot on nearly half of
later launches, and only 2% land in the middle. That 2% is what makes a `>= 90%` gate a
near-binary classifier rather than a threshold on a continuum.

### The failure mode is silence, not error

If the crowd ever did rotate completely, no wallet would reach `veteran_min_launches`, the
roster would persist empty, `veteran_share` would read 0 everywhere, and a rule gated on
`>= 90` would take **zero** trades. Total rotation costs coverage, never correctness.

Storage cannot run away either: the entire 30-day wallet universe is ~2000 addresses and
only the qualifying ~90 are persisted (a few KB on the fingerprint row).

## The launch window

Buys fold only while `at <= created_at + LAUNCH_WINDOW_SECS` (1 s); afterwards the values
**freeze**. Three consequences worth stating:

* A rule reads the same value at `time = 2` as at `time = 200`, so an entry condition on it
  can never be re-triggered by later trading.
* Nothing is monotonic — the values stop moving rather than trending — so no derived
  unsatisfiability applies.
* Sells are ignored. A launch-window sell exits a position opened in the same window and
  would double-count the wallet.

The engine sees timestamps, not slots, so 1 s is the online approximation of the creation
slot. Against a slot-exact ground truth it reproduces the per-token share at r = 0.999 and
agrees on a `>= 90%` gate for 97.8% of tokens.

`veteran_share` is `NaN` until the first launch-window buy — and a `NaN` satisfies no
condition, so a rule can never fire on absent data.

## Causality — the correctness contract

The engine is a pure fold and cannot query launch history, so the roster is **injected**.
`services::veteran_roster::launch_history` is the ONE place that history is read; both
consumers below fold its output.

**Live** — `refresh_roster` counts recurrence over the lookback and parks the result on
`fingerprints.metric_config.m_bundle.veteran_wallets`. A roster refreshed at T contains only
launches before T and is read by tokens created after T, so a token is never scored against
its own bundle nor against later launches. That ordering is free live, and it is why the
roster is a stored snapshot rather than a live query.

**Backtest** — a replay has no such ordering: the stored roster was built from launches that
lie in the *future* of nearly every token in the corpus, so reading it scores a three-week-old
token against today's answer. A simulate therefore ignores the stored roster entirely.
`walk_forward_timeline` rebuilds one snapshot per day over the run window — each counting
only launches strictly **before** its own anchor — and installs it on an in-memory
`metric_config` as `m_bundle.veteran_timeline`. `metrics::bundle::RosterTimeline` picks the
snapshot in force at each token's birth instant, so every token reads a roster older than
itself. A timeline overrides a flat roster on the same config, so the two can never
disagree.

A token older than the first snapshot reads **no** roster at all — the metric stays `NaN`
and the rule stands down, rather than borrowing a roster assembled from its own future.

`m_bundle` is the only metric group with an input that does not come from the corpus, which
is why it is also the only one that needs this. Every other group is folded from the trades
being replayed.

## Bootstrapping a new fingerprint

A roster is *derived*, never hand-authored, so a freshly created fingerprint carries none:
`m_bundle` metrics read `NaN` and a rule on one can never fire. Three paths fill it in:

* **`POST /api/fingerprints/{id}/refresh-roster`** (the `rebuild` button on the fingerprint
  form) — immediate, and reports `launches / wallets / veterans` so a zero-launch answer
  reads as a *fingerprint* problem rather than a roster one. Live also schedules an engine
  reload, so a running rule picks the new set up without a restart.
* **Rule activation** — activating a rule that references `m_bundle` refreshes its
  fingerprint's roster *before* the rule goes live. Synchronous on purpose: a rule activated
  against an absent roster enters nothing, which reads like a bad rule rather than a missing
  input.
* **The hourly refresher** — covers every fingerprint carrying a rule that references
  `m_bundle`, active or not, so a rule being tuned already has a roster when it is switched
  on.

A simulate needs none of them: it builds its own walk-forward roster per run. Rebuilding the
stored one is how to see what **live** would read.

## Configuration

Written by the refresher, never hand-authored:

```json
{ "m_bundle": { "veteran_min_launches": 25, "veteran_wallets": ["Addr1", "Addr2"] } }
```

An absent `m_bundle` key means unconfigured — the metrics read `NaN` and the rule never
fires, which `bundle_unconfigured_warning` reports at rule save. An **empty**
`veteran_wallets` is a configured-empty roster (every wallet reads fresh); the distinction
separates "not set up" from "set up, nobody qualifies yet".

The roster is seeded before the token's first trade. The launch window is one second wide,
so a roster arriving later classifies part of the bundle against an empty set and reads the
share low. `EngineState` parses each fingerprint's roster once per reload rather than per
token, because a roster is a list of wallet *addresses* and re-hashing them per token is
real cost on a corpus-wide replay.

`veteran_min_launches` is not a sensitive knob — 10, 25 and 50 score within 0.5pp of each
other, because the underlying distribution is bimodal.

## Cost

One keyset scan of `tokens` over the lookback plus one aggregate over `trades` restricted to
the matched mints. No per-token round trip and no RPC. It runs hourly: a wallet's launch
count moves by one per launch, so anything finer is below the signal.

The fingerprint match settles in two phases — a cheap `Instant`-phase filter over the token
table (a superset of `Full`), then a `Full` re-check once the same aggregate has supplied
the first-slot buy/sell axes.

A backtest pays that scan once per run (timed as `sim_roster`), and only when the rule
actually references `m_bundle`.

## Registry placement

`m_bundle` sits in the `FlowSplit` metric family. Both groups partition SOL by a **wallet
classifier** — bot-volume vs organic there, veteran vs fresh here — rather than by
magnitude, so discovery measures their interaction instead of blindly crossing them. It is
fingerprint-scoped, which `is_fingerprint_scoped` reports; that predicate is deliberately
distinct from `is_flow_metric`, which additionally selects a *flow* series column offline
that `m_bundle` does not have.
