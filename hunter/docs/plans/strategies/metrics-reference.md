# Metrics reference — volume/organic flow split

Deep-dive for `m_flow_split` / `m_flow_window`. High-level map:
[`arch/strategies.md`](../../arch/strategies.md). Origin roadmap (shipped):
[`roadmap/volume-flow-split-plan.md`](../../roadmap/volume-flow-split-plan.md).

## Classifier (per trade × fingerprint)

A trade is **volume-side** iff any of:

1. its ordered `ix_labels` hash ∈ the fingerprint's configured `volume_ix_patterns`
   (exact ordered sequence — same semantics as fingerprint `ix_labels`);
2. its wallet was previously tagged volume-side on **this token** (wallet contagion);
3. it is the **creator wallet** (unconditionally volume-side).

Otherwise **organic**. Contagion is per-token only (cross-token is a future toggle).

Config lives on the fingerprint (not the rule):

```json
{ "m_flow_split": { "volume_ix_patterns": [["create","buy"], ["buy","closeaccount"]] } }
```

`m_flow_window` reads the **same** `m_flow_split` key (one classifier, two views).
Unconfigured fingerprint (no `m_flow_split` key) ⇒ every flow metric is **NaN**
(satisfies nothing). `ix_hash = None` (pre-0002 / missing lake labels) ⇒ organic
unless wallet-tagged/creator.

Flow state is **fingerprint-scoped** on `TokenTrack` (`BTreeMap<FingerprintId, FlowState>`),
not token-scoped — two fingerprints with different pattern sets diverge.

## Hash SSOT

`hunter_engine::metrics::flow_split::{ix_hash, wallet_hash, ix_hash_opt}` are the
**only** hashers. Every adapter (live producer, lake replay, event-log) calls them;
patterns compile to a hash set at `RulesReloaded`. No interner ⇒ replay parity by
construction. See hunter/CLAUDE.md Gotchas.

## Metric groups

| group | kind | strict params | fingerprint config |
| --- | --- | --- | --- |
| `m_flow_split` | static (fingerprint-scoped) | none | `volume_ix_patterns: string[][]` (required when key present) |
| `m_flow_window` | dynamic | `window_size_sec` | none (reads `m_flow_split`) |

Both expose the same nine JSON metric names; registry `MetricId`s are distinct so
lifetime monotonic flags can differ. All SOL values use absolute trade notional;
buy = +, sell = − for `*_net`.

| metric | meaning | unit | eq-tol | monotonic (lifetime only) |
| --- | --- | --- | --- | --- |
| `vol_buy` | volume-side buy SOL | SOL | 0.1 | ✓ |
| `vol_sell` | volume-side sell SOL | SOL | 0.1 | ✓ |
| `vol_net` | `vol_buy − vol_sell` | SOL | 0.1 | ✗ |
| `vol_gross` | `vol_buy + vol_sell` | SOL | 0.1 | ✓ |
| `nonvol_buy` | organic buy SOL | SOL | 0.1 | ✓ |
| `nonvol_sell` | organic sell SOL | SOL | 0.1 | ✓ |
| `nonvol_net` | `nonvol_buy − nonvol_sell` | SOL | 0.1 | ✗ |
| `nonvol_gross` | `nonvol_buy + nonvol_sell` | SOL | 0.1 | ✓ |
| `vol_share` | `vol_gross / (vol_gross + nonvol_gross)` ×100; NaN when total 0 | % | 1.0 | ✗ |

Windowed variants are never monotonic. Lifetime monotonic ✓ metrics participate in
derived-unsatisfiability disarm (`arm.rs` reads the registry flag).

## NaN rules

| situation | flow metrics |
| --- | --- |
| Fingerprint has no `m_flow_split` key | all NaN |
| Pre-first-trade (no classifier state yet) | NaN (existing convention) |
| Trade `ix_hash = None`, wallet not tagged, not creator | counts as organic |
| Pre-V0 sealed lake days (NULL `ix_labels`) | organic in runtime; **excluded** from discovery score denominators |

Rule save **warns** (does not reject) when params reference flow groups but the
fingerprint is unconfigured.

## Discovery scoring (lab authoring aid)

`lab/src/strategies/flow_discovery.rs` + `POST /api/strategies/flow-discovery`.
Partitions the `with_flow` corpus by sweep `GroupKey`, scores each distinct trade
ix-structure:

| signal | formula (summary) |
| --- | --- |
| `volume_share` | structure gross / group gross ×100 |
| `wash_symmetry` | mean `|net|/gross` over tokens (→0 = wash) |
| `cross_token_recurrence` | % of group tokens with gross ≥ 0.05 SOL |
| `group_lift` | share(S\|G) / share(S\|window) — lift≈1 ⇒ ambiguous |
| `slot_burst` | % of trades in ±1-slot same-structure clusters |
| `wallet_reuse` | `1 − distinct_wallets/trades` (+ secondary Jaccard overlap) |

Ambiguity chip when top structure's `group_lift < 1.25`. Apply writes
`metric_config` via fingerprint `PUT` or promote-style bind. Auto-promote stays
future work (gated on hand-label kit).

## Future toggles (not built)

- Cross-token contagion (fingerprint-keyed bounded set in `EngineState`)
- Baselines / since-entry metric variants
- Transfer ingestion (SOL/token transfers)
- Discovery auto-promote above score thresholds
