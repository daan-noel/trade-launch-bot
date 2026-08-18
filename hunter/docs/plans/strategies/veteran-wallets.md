# Veteran wallets — the `m_bundle` metric group

`m_bundle` answers a question no other metric group asks: **who** funded a token's launch,
rather than how much SOL moved or where the price went.

* `veteran_share` — percent of launch-window buy SOL from **veteran** wallets
* `veteran_wallets` / `fresh_wallets` — distinct launch-window buyers of each kind

A **veteran** is a wallet that bought at least `veteran_min_launches` (default 25) of the
same fingerprint's **earlier** launches.

## Why it exists

On a bundled-launch fingerprint the launcher is software, not a person — the creator wallet
is one-shot and carries no reputation, so every creator-side screen is a no-op. What repeats
is the set of wallets that fund each launch. Their share of the launch bundle is
**bimodal**: a bundle is either almost entirely veteran money or almost entirely fresh
wallets, with little in between. That split is invisible to every price and flow metric —
it measures |r| <= 0.19 against liquidity, gross flow and `unique_wallets`.

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
`services::veteran_roster::refresh_roster` computes it and parks it on
`fingerprints.metric_config.m_bundle.veteran_wallets`; `metrics::bundle::
veterans_from_metric_config` reads it back when a token track is created.

A roster refreshed at time T contains only launches **before** T and is read by tokens
created **after** T. A token is therefore never scored against its own bundle, nor against
later launches. That property is the whole reason the metric is not look-ahead, and it is
why the roster is a stored snapshot rather than a live query.

**A backtest must rebuild the roster at each evaluation point.** Scoring history against a
roster built from the whole corpus leaks the future: wallets are marked veteran on launches
that had not happened yet, and the gate looks far stronger than it is.

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
share low.

`veteran_min_launches` is not a sensitive knob — 10, 25 and 50 score within 0.5pp of each
other, because the underlying distribution is bimodal.

## Cost

One keyset scan of `tokens` over the lookback plus one aggregate over `trades` restricted to
the matched mints. No per-token round trip and no RPC. It runs hourly: a wallet's launch
count moves by one per launch, so anything finer is below the signal.

The fingerprint match settles in two phases — a cheap `Instant`-phase filter over the token
table (a superset of `Full`), then a `Full` re-check once the same aggregate has supplied
the first-slot buy/sell axes. Only fingerprints carrying an **active** rule that references
`m_bundle` are refreshed.

## Registry placement

`m_bundle` sits in the `FlowSplit` metric family. Both groups partition SOL by a **wallet
classifier** — bot-volume vs organic there, veteran vs fresh here — rather than by
magnitude, so discovery measures their interaction instead of blindly crossing them. It is
fingerprint-scoped, which `is_fingerprint_scoped` reports; that predicate is deliberately
distinct from `is_flow_metric`, which additionally selects a *flow* series column offline
that `m_bundle` does not have.
