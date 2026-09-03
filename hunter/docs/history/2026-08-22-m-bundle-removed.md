# 2026-08-22 — `m_bundle` removed

The launch-bundle metric group (`veteran_share`, `veteran_wallets`, `fresh_wallets`) and
everything that fed it is deleted: `engine/src/metrics/bundle.rs`,
`core/src/services/veteran_roster.rs`, the hourly refresher in `live/src/main.rs`, the
`POST /api/fingerprints/{id}/refresh-roster` route on both bins, the read-only roster
panel in `FingerprintForm`, and `metric_config.m_bundle` on the one fingerprint carrying
it. The only rule that read it, `FP108-VET-1` (paper, never activated, 0 arms and 0
positions), is deleted with it.

## Why

The group produced one rule and that rule was refuted —
[the refuted-lines ledger](2026-09-03-refuted-lines-ledger.md): the +5.95%/trade
headline was roster leak plus unpriced impact, and the left tail it appeared to avoid is
an entry property, not a bundle property. Nothing else ever used it. It was, however,
still charging real cost:

* a launch-history token-table sweep every hour on the live box, per fingerprint carrying
  a rule that referenced the group — on a 2vCPU/4GB deploy;
* a walk-forward roster rebuild inside every simulate whose rule referenced it, before
  the corpus fold could start;
* a wallet-address list on the fingerprint row (4.4 KB on the one that carried it),
  re-hashed on each engine reload.

## What it cost to keep, structurally

`m_bundle` was the only fingerprint-scoped group that was not a flow group, which forced
`is_fingerprint_scoped` to exist as something wider than `is_flow_metric`. With it gone
the two are the same predicate. They stay as separate functions on purpose: a future
fingerprint-scoped group that is not a flow group belongs to
[`is_fingerprint_scoped`](../../../engine/src/metrics/mod.rs) and **not** to
`is_flow_metric`, which additionally selects a flow series column offline.

`ensure_track_windows_and_flow` also loses its `created_at` argument — the roster was the
only thing that needed the token's birth instant at track-construction time.

## The one idea worth keeping

"Who is on the other side" is a real axis; the launch window was the wrong place to read
it and a wallet roster was the wrong instrument. The instruction-structure work in
on instruction structure
asks the same question against the transaction's own ordered `ix_labels`, which is a
static fact on every token and needs no derived, refreshed, leak-prone side table.
