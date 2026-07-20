# Strategy redesign — Volume/organic flow split (`m_flow_split` / `m_flow_window`)

Status: **IN PROGRESS** (design settled 2026-07-17; **V0+V1 shipped 2026-07-20**).
Scope: hunter only. A follow-on to the generic engine —
[fingerprint-metrics-engine-plan.md](fingerprint-metrics-engine-plan.md).
Phase 5 prerequisites are met; the discovery job (§7) can trail the metrics.
Origin: creator wash-volume tracking idea + reference reading of
`bot-panther-new-main/src/trading/volume_bot/{bot,types}.rs` (concepts borrowed:
pattern classifier, wallet contagion, two signed accumulators; its hardcoded
condition/exit zoo deliberately NOT borrowed — all conditions are expressed in the
engine's generic `{metric, operator, value}` grammar).

## 0. Goal + workflow at a glance

Dev teams rotate wallets freely but their volume-making tooling emits a small,
recognizable set of instruction structures (`ix_labels`). Split every token's SOL flow
into **volume-side** (creator tooling) vs **organic** (real traders), expose both as
ordinary metrics, and let rules condition entries/exits on them ("how much money is the
dev inserting vs. how much real money is arriving").

```
 AUTHORING (lab, once per creator group)
 ┌──────────────────────┐  ranked structures   ┌──────────────┐    writes
 │ discovery job (§7)   │─────────────────────▶│ user reviews │──────────────┐
 │ per-ix-structure     │  + scores + warning  │ + toggles    │              ▼
 │ stats over corpus    │                      └──────────────┘  fingerprints.metric_config
 └──────────────────────┘                                        { "m_flow_split":
                                                                    { "volume_ix_patterns":
                                                                      [["create","buy"], …] } }
                                                                              │
 ═══════════════════════════════════════════════════════════════════════════ │ ═════════
 RUNTIME (live · replay · simulate · sweep — same fold)         RulesReloaded │ compiles
                                                                              ▼
 trade {ix_labels, wallet, sol, side}                            FlowPatterns (hash set,
        │  adapter boundary: SSOT hash fns (ix_hash/wallet_hash)  per fingerprint)
        ▼                                                                     │
 TradeLite {ix_hash, wallet_hash, sol, side, …}                               │
        │                                                                     │
        ▼          CLASSIFIER — per (token × fingerprint)                     │
 ┌────────────────────────────────────────────────────────┐                  │
 │  ix_hash ∈ volume_ix_patterns? ──yes──┐                │◀─────────────────┘
 │  wallet tagged earlier (contagion)? ──┼──▶ VOLUME-side ──▶ tag wallet forever
 │  creator wallet? ─────────────yes─────┘        (this token)
 │  none of the above ─────────────────────▶ ORGANIC-side  │
 └────────────────────────────────────────────────────────┘
        │ volume-side SOL                    │ organic-side SOL
        ▼                                    ▼
 ┌─────────────────────┐            ┌───────────────────────┐
 │ vol_buy   vol_sell  │            │ nonvol_buy nonvol_sell│   m_flow_split  = since creation
 │ vol_net   vol_gross │            │ nonvol_net nonvol_gross│  m_flow_window = trailing
 └──────────┬──────────┘            └──────────┬────────────┘                  window_size_sec
            └────────────┬─────────────────────┘
                         ▼
              vol_share (% of total gross)
                         │
                         ▼
     rule conditions {metric, operator, value} — entry AND exit
     (sweep axes · chart panes · live monitor · validation: all
      registry-driven — appear automatically, zero extra FE work)
```

Unconfigured fingerprint (no `m_flow_split` key) ⇒ every flow metric `NaN` ⇒ no
condition satisfies; `ix_hash` absent (pre-0002 history) ⇒ organic unless
wallet-tagged/creator.

## 1. Settled design decisions (contract — do not re-litigate)

1. **Classification (per trade).** A trade is **volume-side** iff any of:
   - its ix structure ∈ the fingerprint's configured pattern set (**exact ordered**
     sequence match — same semantics as the fingerprint matcher's `ix_labels`, NOT the
     reference bot's lowercased joined strings);
   - its wallet was previously tagged on this token (**wallet contagion** — a
     pattern-matched wallet stays volume-side for the token's lifetime);
   - it is the **creator wallet** (unconditionally volume-side).
   Otherwise organic. Contagion is **per-token only** in v1 (cross-token wallet memory
   is a future toggle, §8).
2. **Config home: `fingerprints.metric_config JSONB NOT NULL DEFAULT '{}'`.**
   Deliberately NOT a `volume_*` column and NOT a vague `meta`/`extra` — the contract
   mirrors `strategy_rules.params`: **top-level keys = metric group names**, values =
   that group's fingerprint-side config, validated against the engine registry at save:
   ```json
   { "m_flow_split": { "volume_ix_patterns": [["create","buy"], ["buy","closeaccount"]] } }
   ```
   The registry gains a per-group *fingerprint-config* declaration (beside strict
   params), so a future group needing fingerprint-side config = one file + registry
   entry, zero schema changes. `m_flow_window` reads the **same** `m_flow_split` key
   (one classifier, two views — never duplicate the pattern set).
3. **Two metric groups** (separate so lifetime/windowed are independently
   addable/removable, mirroring `m_snapshot` vs `m_time_window`):
   - `m_flow_split` — lifetime totals since creation, no strict params.
   - `m_flow_window` — same metrics over a trailing window, strict `window_size_sec`.
   Both are computed from ONE shared classifier state (§3).
4. **Unconfigured fingerprint ⇒ NaN.** If the rule's fingerprint has no `m_flow_split`
   key, every flow metric is `NaN` (satisfies nothing — existing pre-first-trade
   convention). Rule save **warns** (not rejects) when params reference flow groups but
   the fingerprint is unconfigured (the user may configure the fingerprint later).
5. **Flow state is fingerprint-scoped, not token-scoped.** A token matching two
   fingerprints with different pattern sets has two classifier states. `TokenTrack`
   keys flow state by `FingerprintId` (dedup across rules sharing a fingerprint — same
   spirit as window dedup). Static/price-path metrics stay token-scoped, unchanged.
6. **Engine purity via stable hashes, not interners.** `TradeLite` gains
   `ix_hash: Option<u64>` and `wallet_hash: u64`; `Event::TokenCreated` gains
   `creator_wallet_hash: Option<u64>`. One SSOT fn pair in the engine
   (`flow_split::ix_hash(&[impl AsRef<str>])`, `flow_split::wallet_hash(&str)` — FNV-1a
   over the label sequence with a separator / over the address) is called by every
   adapter (live producer, lake replay, event-log recorder writes the hashed form).
   Patterns compile to a hash set at `RulesReloaded`. No interner state ⇒ replay parity
   by construction; no per-event alloc on the hot path.
7. **Data plumbing is forward-only.** PG `trades` already carries per-trade `ix_labels`
   (migration 0002, forward-only) + `wallet_id`. The lake gains per-trade `ix_labels`
   and `wallet` (address, dictionary-encoded) columns; `CorpusTrade` gains both
   (loaded only when the run needs flow metrics — see §4 memory note). Sealed days
   exported before the change stay NULL ⇒ flow metrics NaN there. Accepted.
8. **Deferred to future toggles (§8), designed-not-built:** cross-token contagion,
   baselines (creation-slot / creator-first-sell / sell-dominance anchors),
   since-entry metric variants, token/SOL **transfer** ingestion (transfers are
   invisible to the trades feed today — wallet contagion + patterns are the proxy).
9. **Discovery is semi-automatic.** A lab job scores each distinct ix structure per
   fingerprint group (§7); the user reviews and toggles which structures are "volume",
   which writes the fingerprint's `metric_config`. Auto-promote above a score
   threshold is a later mode, only after the scores are validated against labeled
   examples.

## 2. Metrics (the deliverable)

Both groups expose the same nine metrics; `m_flow_split` = since creation,
`m_flow_window` = over the trailing `window_size_sec`. All SOL values are the trade's
absolute notional split by classifier verdict (buy = +, sell = − for `*_net`).

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
derived-unsatisfiability disarm for free (`arm.rs` reads the registry flag).

Reference-bot conditions map onto these with zero special code:

| Reference behavior | As conditions |
| --- | --- |
| IDLE_NON_VOLUME (organic went silent) | exit `nonvol_gross(30s) = 0` |
| NON/VOLUME_THRESHOLD | exit `nonvol_gross > X` / `vol_gross > X` |
| VOL_FLAT + NONVOL_DROP | exit `vol_gross(30s) = 0` AND `nonvol_net(30s) < 0` |
| sell-dominance drop → recovery entry | entry `vol_net(10s) > X` (optionally after sweep-found context) |
| dev dominates the tape | `vol_share > 80` |

## 3. Engine shape (`hunter/engine`)

```
metrics/flow_split.rs                 ONE new file (both groups + classifier)
├── ix_hash / wallet_hash             SSOT stable-hash fns (used by all adapters)
├── FlowPatterns                      compiled per-fingerprint pattern hash set
│                                     (built at RulesReloaded from metric_config)
├── FlowState                         per (token, fingerprint):
│   ├── tagged_wallets: BTreeSet<u64> contagion set (+ creator_wallet_hash)
│   ├── lifetime: FlowTotals          9 lifetime values (running, O(1)/trade)
│   └── windows: BTreeMap<WindowKey, VecDeque<(Ts, signed_sol: f64, is_vol: bool)>>
│                                     deduped per distinct window_size_sec (existing
│                                     time_window mechanics; evict on trade+tick)
└── classify(trade) -> bool           pattern-hash ∈ set  ||  wallet tagged  ||  creator
```

- `metrics/mod.rs`: `MetricGroupId::{FlowSplit, FlowWindow}` + 18 `MetricId` variants;
  registry entries carry the new **fingerprint-config declaration**
  (`m_flow_split` requires fingerprint key `volume_ix_patterns: [[label]]`);
  `registry_json()` includes it (FE contract).
- `metrics/track.rs`: `TokenTrack` gains `flow: BTreeMap<FingerprintId, FlowState>`;
  `value()/values()` take the requesting rule's fingerprint id for flow groups.
- `metrics/series.rs`: flow columns emitted per fingerprint-config in the run (sweep
  precompute unchanged in shape).
- `event.rs`: `TradeLite { …, ix_hash: Option<u64>, wallet_hash: u64 }`;
  `TokenCreated { …, creator_wallet_hash: Option<u64> }`. `ix_hash = None` (pre-0002
  rows, missing labels) classifies as organic unless wallet-tagged/creator.
- `rule_params.rs` / rules validation: flow metrics validated like any group; the
  fingerprint-unconfigured **warning** is produced in `rules.rs` (core, has the repo).

## 4. DB + lake schema

```sql
-- hunter/core/migrations/0006_fingerprint_metric_config.sql
-- (0005 was already taken by retire_legacy_strategies)
ALTER TABLE fingerprints
    ADD COLUMN metric_config JSONB NOT NULL DEFAULT '{}';
```

- `models/fingerprint.rs` + `FingerprintRepo`: carry/persist `metric_config`;
  **identity predicate unchanged** (`find_or_create` does NOT match on it — patterns
  are configuration, not identity).
- Lake (`lab/src/lake/{schema,export,duck}.rs`): trades files gain `ix_labels`
  (JSON-string, dict-encoded — same normalized form as the token-level `fp_ix_labels`)
  and `wallet` (address). Export joins `wallet_dict` **LEFT JOIN + COALESCE**
  (`'unknown:'||wallet_id` fallback — hunter gotcha rule; fallback rows hash unlike any
  live address, i.e. classify organic, which is the honest behavior for a dict gap).
- `CorpusTrade`: `ix_labels: Option<Box<str>>`, `wallet: Option<Box<str>>` — loaded
  **only when the run's rules/axes reference a flow group** (projection flag, like
  `with_signatures`), so sweeps that don't use flow metrics pay zero extra RAM.

## 5. Adapters

- **Live producer** (`live/src/strategies/engine/producers.rs`/`convert.rs`): compute
  `ix_hash`/`wallet_hash` from the in-hand ingest values (labels + wallet address are
  already on the ingest trade event — no lookup); `TokenCreated` carries the creator's
  wallet hash. Hot-path cost: two FNV hashes per trade, no alloc.
- **Event log**: `LoggedEvent` bumps its format (hashed fields ride the event);
  replay/inspect (`POST /api/replay/inspect`) works unchanged since decisions replay
  from the recorded hashes.
- **Lake replay** (`lab/src/strategies/replay.rs`): hash from the corpus columns via
  the same SSOT fns. Missing columns (old sealed days) ⇒ `ix_hash = None`.
- **Fingerprint CRUD** (`http.rs` live + lab authoring): `metric_config` accepted on
  create/update, validated against the registry (unknown group key / malformed
  patterns rejected at save — typos can't silently no-op).

## 6. Implementation phases

> DoD per phase: workspace `cargo check` clean on `hunter-engine`/`-live`/`-lab`,
> clippy on touched code, listed tests green, no new warnings
> (`--target-dir "C:/Users/User/Documents/Bot/target-check"` if a bin is running).

### V0 — Data plumbing (no behavior change) ✅ 2026-07-20

> Plan §4 said migration `0005_…`; repo already had `0005_retire_legacy_strategies.sql`,
> so this landed as **`0006_fingerprint_metric_config.sql`**.

- [x] 0.1 Migration 0006 + `models/fingerprint.rs` + `FingerprintRepo`
      (`metric_config` round-trip; `IDENTITY_WHERE` untouched — patterns are config).
- [x] 0.2 Engine: `metrics/flow_split::{ix_hash,wallet_hash,ix_hash_opt}` +
      `TradeLite::{ix_hash,wallet_hash}` + `TokenCreated::creator_wallet_hash`. Live
      hashes at `CachedTrade::from_trade`; producer/replay/sweep use SSOT. Fixtures
      default via `TradeLite::Default` / `creator_wallet_hash: None`.
- [x] 0.3 Lake: `T_IX_LABELS`/`T_WALLET` + export (LEFT JOIN `wallet_dict` +
      `unknown:{id}` COALESCE) + `CorpusTrade` fields + `Selection::with_flow`
      (default false; DuckDB conditional SELECT + `lake_hash`).
- [x] 0.4 Event-log: `LoggedEvent::TokenCreated.creator_wallet_hash` +
      `TradeLite` serde `default` — old JSONL → organic.

### V1 — Engine metrics ✅ 2026-07-20

- [x] 1.1 `metrics/flow_split.rs` (§3): classifier + `FlowState` + lifetime compute.
- [x] 1.2 `m_flow_window`: window deques (dedup by `(fingerprint, window_key)`),
      evict on trade + tick, O(1) reads.
- [x] 1.3 Registry: two groups, 18 metrics (distinct `MetricId`s per group so
      lifetime monotonic flags can differ; JSON names shared), units/tolerances,
      **fingerprint-config declaration** + `registry_json()` extension.
- [x] 1.4 `TokenTrack`/`series.rs` routing (fingerprint-scoped state, §3);
      `SeriesColumn::Flow` + `MetricReq.fingerprint`.
- [x] 1.5 `RulesReloaded` / `EngineState::reload` compiles `FlowPatterns` from
      `Fingerprint.metric_config`; rule-save soft warning via
      `create_with_fp_check` / `save_with_fp_check`; fingerprint CRUD validates
      `metric_config` shape.
- [x] 1.6 Tests: classifier unit tests; golden-log tests (entry on `vol_net`;
      exit on `nonvol_gross(w)=0` via tick; two fingerprints diverge). Series
      determinism for non-flow columns unchanged; sweep pattern injection is V2.

### V2 — Analysis parity

- [ ] 2.1 Lake replay produces hashed events (§5); flow metrics live in simulate +
      dry-run automatically (generic simulate resolves them via the rule's
      fingerprint).
- [ ] 2.2 Sweep: flow metrics as generic axes. Pattern source for a sweep run = the
      run config's optional `volume_ix_patterns` (applied corpus-wide for the run);
      **Promote** writes them into the created fingerprint's `metric_config` (width
      parity precedent).
- [ ] 2.3 Extend the scan≡engine guard corpus with flow-metric conditions (drift lock
      inherits decision 13).
- [ ] 2.4 `GET /api/tokens/{mint}/metric-series` gains optional `fingerprint_id` (flow
      panes need a pattern context; absent ⇒ flow columns omitted).

### V3 — Frontend (registry-driven, small by design)

- [ ] 3.1 FingerprintsPage/editor: `metric_config` section rendered from the
      registry's fingerprint-config declaration (pattern list editor = rows of
      ordered label chips; reuse the ix_labels display vocabulary).
- [ ] 3.2 Chart metric panes: pass the selected rule's fingerprint to metric-series;
      flow panes appear in the picker via the registry (no other FE work).
- [ ] 3.3 Rule editor / monitor / sweep axis builder: **zero work** — verify the new
      groups appear and conditions round-trip (the §8-of-backend-plan extensibility
      contract's first real exercise).

### V4 — Discovery job (§7)

- [ ] 4.1 Lab endpoint: per-fingerprint-group structure stats + scores over a corpus
      window (streamed like sweeps if slow; bounded queries — data-scale rule).
- [ ] 4.2 Lab page: ranked structure table (scores + ambiguity warning), toggle →
      writes `metric_config` via fingerprint update; show current config diff.
- [ ] 4.3 Validate scores against a handful of hand-labeled creator groups before
      enabling any auto-promote mode (which stays future work).

### V5 — Docs

- [ ] 5.1 `metrics-reference.md`: the 18 metrics (formula/unit/tolerance/monotonic +
      classifier definition). `arch/strategies.md` + `arch/sweep.md` +
      `arch/database.md` (metric_config column, lake columns).
      hunter/CLAUDE.md only if a new hard rule emerges (hash SSOT likely qualifies).

## 7. Discovery scoring (the automated version of the manual hunt)

Per fingerprint group, aggregate the corpus per distinct ix structure and score:

| signal | definition | catches |
| --- | --- | --- |
| volume share | % of group's total gross SOL from this structure | "the biggest trader" heuristic |
| wash symmetry | `|net| / gross` per token, averaged (→0 = wash) | wash loops net to ~0 while gross balloons |
| cross-token recurrence | % of the group's tokens where the structure has meaningful volume | creator tooling appears on every token in the batch |
| **group lift** | share within group ÷ share across all tokens (TF-IDF-style) | the discriminator; lift≈1 on `["buy"]` honestly flags "indistinguishable by structure" |
| slot-burst clustering | % of the structure's trades in 1–2-slot same-structure clusters | bundlers |
| wallet reuse | distinct wallets per structure + overlap across the group's tokens | rotation isn't free within a batch |

UI shows all six + an **ambiguity warning** when the top structure's lift ≈ 1 (the
flow split will be noisy for that group — surface it next to the metric rather than
letting the user discover it from bad fills).

## 8. Future toggles — designed now, built later

- **Cross-token contagion**: wallets tagged on token A pre-tagged on token B of the
  same fingerprint. Needs a bounded shared set inside `EngineState` keyed by
  fingerprint (size-capped, log-replayable). Powerful; risky (one false tag poisons a
  whole group) — build only after v1 data shows rotation defeats per-token contagion.
- **Baselines / since-entry variants**: anchor metrics to lifecycle moments (creator
  first sell, entry fill). New metrics inside `flow_split.rs`, no structural change.
- **Transfer ingestion**: direct wallet-linking via SOL/token transfers — a separate,
  expensive ingest feature; only if the proxy demonstrably fails.

## 9. Risks / open edges

- **Hash-set classification is only as good as the patterns.** Vanilla `["buy"]`
  tooling degrades the split to contagion + creator only; the lift score (§7) tells
  you which groups to trust. This ceiling is inherent to the approach, not a bug.
- **`ix_hash=None` history**: pre-0002 PG rows and pre-V0 sealed lake days classify
  everything organic — backtests over that range under-count `vol_*`. Forward-only,
  accepted (decision 7).
- **Wallet-dict gaps at export** hash as `unknown:{id}` ⇒ organic; consistent with the
  LEFT-JOIN gotcha, negligible volume, noted here so it isn't rediscovered as a bug.
- **State growth**: `FlowState` per (armed token × matched fingerprint) — bounded by
  the same armed-set bounds as everything else (≥1-criterion guard, MAX_SNIPE_AGE,
  derived disarm); tagged-wallet sets are per token and die with the track.
- **CorpusTrade width**: two optional columns behind a projection flag keep
  non-flow sweeps at today's memory footprint (§4).
