# Strategy redesign — Volume/organic flow split (`m_flow_split` / `m_flow_window`)

Status: **IN PROGRESS** (design settled 2026-07-17; **V0–V3 shipped 2026-07-20**;
**V4 discovery implemented 2026-07-20**; V5 docs trails).
Scope: hunter only. A follow-on to the generic engine —
[fingerprint-metrics-engine-plan.md](fingerprint-metrics-engine-plan.md).
Phase 5 prerequisites are met; the discovery job (§7) is the remaining product work.
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

### V2 — Analysis parity ✅ 2026-07-20

- [x] 2.1 Lake replay / simulate: `with_flow` when rule params reference `m_flow_*`;
      `to_trade_lite` hashes lake columns; `ReplayToken.creator_wallet_hash` from
      `Token.creator_wallet`; dry-run uses the rule's fingerprint `metric_config`.
- [x] 2.2 Sweep: flow metrics as generic axes. Pattern source for a sweep run = the
      run config's optional `volume_ix_patterns` (applied corpus-wide for the run);
      **Promote** writes them into the created fingerprint's `metric_config` (width
      parity precedent).
- [x] 2.3 Extend the scan≡engine guard corpus with flow-metric conditions (drift lock
      inherits decision 13).
- [x] 2.4 `GET /api/tokens/{mint}/metric-series?fingerprint_id=` — loads flow columns
      + patterns from that fingerprint; absent/unconfigured ⇒ flow groups omitted.

### V3 — Frontend (registry-driven, small by design)

- [x] 3.1 FingerprintsPage/editor: `metric_config` section rendered from the
      registry's fingerprint-config declaration (pattern list editor = rows of
      ordered label chips; reuse the ix_labels display vocabulary).
- [x] 3.2 Chart metric panes: pass the selected rule's fingerprint to metric-series;
      flow panes appear in the picker via the registry (no other FE work).
      Sweep inspect uses the promoted group's fingerprint id when available.
- [x] 3.3 Rule editor / monitor / sweep axis builder: **zero work** — verify the new
      groups appear and conditions round-trip (the §8-of-backend-plan extensibility
      contract's first real exercise). Sweep start form also sends
      `volume_ix_patterns` when flow axes are selected.

### V4 — Discovery job (§7) — design settled 2026-07-20; implement next

> No stubs yet. Reuse sweep admission/SSE + `Selection`/`GroupKey` + fingerprint
> `PUT`/`VolumeIxPatternsEditor`. New work = scorer + lake aggregation + lab page.
> Settled decisions for discovery are in §7.0 (do not re-litigate).

- [x] 4.1 Scorer module (`lab/src/strategies/flow_discovery.rs`): per-group fold over
      `with_flow` corpus → ranked `StructureScore` rows (§7.1–7.2). Unit tests on a
      synthetic mini-corpus covering wash / organic / ambiguous-lift cases.
- [x] 4.2 Lab API (`lab/src/api/handlers/strategies/flow_discovery.rs` + route in
      `api/mod.rs`):
      - `POST /api/strategies/flow-discovery` → `202 { run_id, status }` (single-flight
        **separate** from `sweep_running` — own `discovery_running` AtomicBool +
        `discovery_progress: ProgressCell`; both contend for Duck/CPU so refuse with
        `409` if the *other* job is active too).
      - Body mirrors the corpus half of `StartGroupedSweepBody` (date window,
        `group_by`, `bucket_width_sol`, `token_cap`, `field_filters`,
        `ix_labels_filter`, `min_tokens`) — **no** combo axes.
      - Progress: new `SseEvent::{FlowDiscoveryProgress, FlowDiscoveryFinished,
        FlowDiscoveryNotice}` + `JobsStatus.discovery` seed (same shape as sweep's
        `{processed,total,phase}`).
      - `GET /api/strategies/flow-discovery/{run_id}` → full result JSON (ephemeral
        in-RAM; no PG persistence — discovery runs are authoring aids, not audit
        trails). Cap response: top `MAX_STRUCTURES_PER_GROUP` (64) per group, groups
        sorted by `n_tokens` desc, drop groups with `n_tokens < min_tokens` (default 3).
      - `POST /api/strategies/flow-discovery/cancel`.
      - Apply path = existing `PUT /api/fingerprints/{id}` + promote-style
        `POST .../bind` (`find_or_create` + `update` for unbound `GroupKey` — §7.0.4).
- [x] 4.3 Lab page `/strategies/flow-discovery` (`lab/pages/strategies/FlowDiscoveryPage.tsx`
      + nav + `LabHomePage` shortcut + `labEndpoints`):
      - Config: reuse `FingerprintGroupPicker` + corpus window knobs.
      - Results: per-group list → ranked structure table (six scores + ambiguity
        chip). Toggle rows → draft `volume_ix_patterns`; diff vs bound fingerprint.
      - Apply: `VolumeIxPatternsEditor` + `updateFingerprint` / bind.
      - `BackgroundJobsContext` job kind `discovery` (progress + cancel + status seed).
- [x] 4.4 Hand-label validation kit (blocks auto-promote, which stays §8):
      - `lab/testdata/flow_discovery_labels.json` + synthetic guard test
        `hand_label_kit_synthetic` (top-5 / `expected_ambiguous`).

### V5 — Docs

- [ ] 5.1 `docs/plans/strategies/metrics-reference.md` (or extend the existing metrics
      deep-dive if one already exists): the 18 flow metrics
      (formula / unit / eq-tol / monotonic) + classifier definition (§1.1) + NaN
      rules (unconfigured FP, missing `ix_hash`).
- [ ] 5.2 Arch tier: `arch/strategies.md` (flow groups + fingerprint-scoped state +
      hash SSOT); `arch/sweep.md` (run-config `volume_ix_patterns` + promote write);
      `arch/database.md` (`fingerprints.metric_config`, lake `ix_labels`/`wallet`);
      `arch/frontend.md` (Fingerprints `metric_config` editor + Flow Discovery page).
- [ ] 5.3 hunter/CLAUDE.md gotcha: **flow hash SSOT** —
      `hunter_engine::metrics::flow_split::{ix_hash,wallet_hash}` is the only hasher;
      adapters never roll their own. (Qualifies as a hard rule — add one bullet under
      Gotchas, keep CLAUDE thin.)
- [ ] 5.4 Fold or delete this roadmap once V4+V5 land (per CLAUDE roadmap hygiene);
      durable bits already live in the arch/plans tiers above.

## 7. Discovery scoring (the automated version of the manual hunt)

### 7.0 Settled discovery decisions (contract — do not re-litigate)

1. **Partition key = sweep/creation-stats `GroupKey`, not `fingerprint_id`.**
   Discovery answers "which ix structures look like volume tooling inside this
   creator-tooling cluster?" — the same axes the user already groups by
   (`GroupField` + `bucket_width_sol`). Binding a result to a saved fingerprint is
   a separate Apply step (§7.0.4). Empty `group_by` ⇒ one `"ALL"` group (same as
   sweeps) — useful for a first pass, noisy for lift.
2. **Aggregation engine = Rust fold over a `with_flow` corpus load**, not a new Duck
   SQL surface. Rationale: scores need per-trade `(ix_labels, wallet, sol, side,
   slot)` plus per-token membership; the lake already projects those behind
   `Selection.with_flow`; a fold reuses `Corpus`/`GroupKey` and stays testable
   offline. Duck stays the loader. Bound the run with the same `token_cap` /
   date-window knobs as sweeps (data-scale rule).
3. **Ephemeral results.** No `flow_discovery_*` tables. Result lives in
   `LocalState` keyed by `run_id` until the next run or process restart. Authoring
   output is the fingerprint's `metric_config`, which is already persisted.
4. **Apply = existing fingerprint write paths.**
   - Bound to an existing FP → `PUT /api/fingerprints/{id}` with merged
     `metric_config` (FE already has `metricConfigWithVolumePatterns`).
   - Unbound group → promote-style: `FingerprintRepo::find_or_create` from the
     group's axes (identity only), then `update` with the toggled patterns —
     same shape as `promote_group`'s metric_config patch. UI labels this
     "Create / bind fingerprint".
5. **Structure identity = ordered label sequence**, hashed with SSOT `ix_hash`
   for map keys; wire/API still expose `ix_labels: string[]` (never raw hashes).
   Parse lake `ix_labels` via the same normalize path export uses
   (`normalize_labels` / ordered JSON array). Missing/NULL labels (pre-V0 sealed
   days) are **excluded from scoring** (not counted as a structure, not in
   denominators) — honest "no signal", not fake-organic.
6. **Job isolation.** Own single-flight flag + progress cell; `409` if sweep **or**
   discovery is already running (shared Duck + RAM). Cancel via AtomicBool polled
   in the fold (same observer shape as `SweepObserver`, thinner — no group-done
   stream needed; one Finished frame carries the run_id).
7. **No auto-promote in V4.** Scores + toggles only. Auto-promote above a threshold
   is §8, gated on the hand-label kit (V4.4).

### 7.1 Signals (formulas + defaults)

Per fingerprint **group** G, for each distinct structure S observed on ≥1 trade in G
with non-NULL `ix_labels`:

| signal | formula | range | default knobs |
| --- | --- | --- | --- |
| `volume_share` | `gross_SOL(S,G) / gross_SOL(*,G) × 100` | 0–100 | — |
| `wash_symmetry` | mean over tokens t∈G with `gross(S,t)>0` of `\|net(S,t)\| / gross(S,t)` | 0–1 (→0 = wash) | — |
| `cross_token_recurrence` | `%` of tokens in G where `gross(S,t) ≥ min_structure_sol` | 0–100 | `min_structure_sol = 0.05` |
| **`group_lift`** | `share(S\|G) / share(S\|W)` where W = all tokens in the loaded corpus window (after the same filters, before group split) | ≥0; 1 = indistinguishable | — |
| `slot_burst` | `%` of S-trades in G whose slot is shared with ≥1 other S-trade in a ±1-slot window | 0–100 | window = 1 slot |
| `wallet_reuse` | `1 − distinct_wallets(S,G) / max(trades(S,G),1)`, plus a secondary `wallet_overlap` = mean Jaccard of S-wallets across token pairs in G (UI shows both; rank key uses the first) | 0–1 | — |

**Gross / net** use absolute trade SOL notional; buy and sell both add to gross;
net = buy − sell (same sign convention as the flow metrics). Creator-wallet trades
are **included** in discovery aggregates (discovery is hunting tooling shape, not
applying the runtime classifier).

**Ambiguity warning** (per group, boolean on the top-ranked structure and any
toggled row): `group_lift < LIFT_AMBIGUOUS` with default `LIFT_AMBIGUOUS = 1.25`.
Surface as a chip next to the score — do not block Apply (the user may still want
contagion+creator-only patterns).

**Ranking key** (desc): `group_lift` primary, `volume_share` secondary,
`wash_symmetry` ascending (more wash-like first) tertiary. Expose all six in the
table so the user can re-sort in the UI.

### 7.2 Result wire shape

```jsonc
// GET /api/strategies/flow-discovery/{run_id}
{
  "run_id": "…",
  "selection": { "created_after": "…", "created_before": "…", "token_cap": 5000 },
  "group_by": ["cu_limit", "ix_labels"],
  "groups": [
    {
      "group_key": { "cu_limit": "200000", "ix_labels": "create | buy" },
      "n_tokens": 42,
      "n_trades_scored": 12004,          // excl. NULL ix_labels
      "ambiguity": true,                 // top structure lift < LIFT_AMBIGUOUS
      "structures": [
        {
          "ix_labels": ["create", "buy"],
          "volume_share": 61.2,
          "wash_symmetry": 0.08,
          "cross_token_recurrence": 95.0,
          "group_lift": 4.7,
          "slot_burst": 72.0,
          "wallet_reuse": 0.55,
          "wallet_overlap": 0.31,
          "n_trades": 800,
          "gross_sol": 120.5
        }
      ]
    }
  ]
}
```

`POST` body fields (all optional except that an empty body is a valid "ALL / uncapped
window" run — still clamped by server-side max `token_cap` like sweeps):

```jsonc
{
  "created_after": "…", "created_before": "…",
  "group_by": ["cu_limit", "ix_labels"],
  "bucket_width_sol": 0.1,
  "token_cap": 5000,
  "min_tokens": 3,
  "field_filters": { "cu_limit": ["200000"] },
  "ix_labels_filter": ["create", "buy"]
}
```

### 7.3 Hand-label kit (V4.4) — what "validated" means

Before any auto-promote mode ships, each fixture group must satisfy:

- every labeled volume structure appears in that group's top-5 by the ranking key, **or**
- the fixture sets `expected_ambiguous: true` and the scorer flags `ambiguity`.

Kit failures ⇒ retune knobs (`min_structure_sol`, `LIFT_AMBIGUOUS`, ranking weights),
not silent UI workarounds. Keep the kit small (≤10 groups); quality over coverage.

### 7.4 Signal intuition (unchanged)

| signal | catches |
| --- | --- |
| volume share | "the biggest trader" heuristic |
| wash symmetry | wash loops net to ~0 while gross balloons |
| cross-token recurrence | creator tooling appears on every token in the batch |
| **group lift** | the discriminator; lift≈1 on `["buy"]` honestly flags "indistinguishable by structure" |
| slot-burst clustering | bundlers |
| wallet reuse | rotation isn't free within a batch |

## 8. Future toggles — designed now, built later

- **Cross-token contagion**: wallets tagged on token A pre-tagged on token B of the
  same fingerprint. Needs a bounded shared set inside `EngineState` keyed by
  fingerprint (size-capped, log-replayable). Powerful; risky (one false tag poisons a
  whole group) — build only after v1 data shows rotation defeats per-token contagion.
- **Baselines / since-entry variants**: anchor metrics to lifecycle moments (creator
  first sell, entry fill). New metrics inside `flow_split.rs`, no structural change.
- **Transfer ingestion**: direct wallet-linking via SOL/token transfers — a separate,
  expensive ingest feature; only if the proxy demonstrably fails.
- **Discovery auto-promote**: above a score threshold (likely `group_lift` +
  `cross_token_recurrence` gates), write `volume_ix_patterns` without a toggle pass.
  **Blocked on V4.4 hand-label kit.** Even then, default remains review-then-apply;
  auto-promote is an opt-in mode on the discovery page, never a silent background job.

## 9. Risks / open edges

- **Hash-set classification is only as good as the patterns.** Vanilla `["buy"]`
  tooling degrades the split to contagion + creator only; the lift score (§7) tells
  you which groups to trust. This ceiling is inherent to the approach, not a bug.
- **`ix_hash=None` history**: pre-0002 PG rows and pre-V0 sealed lake days classify
  everything organic — backtests over that range under-count `vol_*`. Forward-only,
  accepted (decision 7). Discovery **excludes** those rows from score denominators
  (§7.0.5) so pre-V0 days don't dilute lift.
- **Wallet-dict gaps at export** hash as `unknown:{id}` ⇒ organic; consistent with the
  LEFT-JOIN gotcha, negligible volume, noted here so it isn't rediscovered as a bug.
- **State growth**: `FlowState` per (armed token × matched fingerprint) — bounded by
  the same armed-set bounds as everything else (≥1-criterion guard, MAX_SNIPE_AGE,
  derived disarm); tagged-wallet sets are per token and die with the track.
- **CorpusTrade width**: two optional columns behind a projection flag keep
  non-flow sweeps at today's memory footprint (§4).
- **Discovery vs sweep contention**: both are lab-only and Duck/RAM hungry — the
  mutual `409` (§7.0.6) is deliberate; don't "fix" it into parallel runs on a
  4 GB analysis box.
- **Lift denominator = loaded window W, not the whole lake.** A narrow date filter
  can inflate lift for structures that are common outside the window; the UI should
  show the selection summary on the results header so the user sees the frame.
