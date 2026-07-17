# Strategy redesign — Frontend plan (registry-driven UI)

Status: **PLANNED** (design settled 2026-07-16, not started)
Scope: **hunter frontend only** (`hunter/frontend`, live + lab apps).
Backend contract: [fingerprint-metrics-engine-plan.md](fingerprint-metrics-engine-plan.md)
— every FE phase below names the backend phase it depends on. Do not start an FE phase
before its backend dependency is merged.

## 0. What this replaces

| Today | After |
| --- | --- |
| hand-written TS `StrategySpec`s (`src/shared/lib/params/specs/{tpsl1,tpsl2,swing1}.ts`) duplicating Rust params | UI rendered from `GET /api/meta/strategy-registry` — zero FE work per new metric |
| `SpecRuleForm` accordion of fixed fields | condition **builder** (grammar inputs) + live dry-run in one editor |
| per-strategy pages `/strategies/{tpsl1,tpsl2,swing1}` ×(live, lab) + 3 sweep pages | pages per **concept**: Rules · Fingerprints · Live monitor · Sweep · Simulate |
| sweep→rule promotion via copy-blob → paste → save | one-click Promote → pre-filled editor (fingerprint `find_or_create`) |
| metric-ish values visible only in chart crosshair tooltip | time-synced metric **sub-panes** with the rule's thresholds drawn on them |
| no visibility into armed/tracking state | live monitor with **blocking-condition** ("why hasn't it fired") column |

## 1. Settled design decisions (contract)

1. **Registry-driven UI.** One payload (`/api/meta/strategy-registry`: operators, groups,
   metrics w/ unit+tolerance+monotonic, strict params, fingerprint fields) drives the rule
   builder, sweep axes, chart pane picker, monitor columns, and validation messages.
   Adding a metric in Rust ⇒ appears everywhere on next load. The `lib/params` spec
   engine + blob/combo serializers are deleted; one small generic serializer remains.
2. **One condition grammar everywhere.** Extend the DataTable numeric-filter grammar
   (`src/shared/components/table/numericFilter.ts`) with **comma = AND**
   (`">10, <=30"` → two conditions; `1..10` → `>=1 AND <=10`). The same parser drives
   rule-condition inputs and table filters; rule context is strict (no `contains`
   fallback — malformed fragment = red underline).
3. **Editor = builder + dry-run.** The rule editor evaluates the *unsaved draft* against
   recent history via the generic simulate endpoint (backend 5.2 accepts inline params).
   Promote-blob→paste→save→navigate→simulate collapses to a 5-second in-editor loop.
4. **Live monitor shows distance-to-fire.** Armed table's key column is the first
   *failing* entry condition (`value → need`), fed by `armed_changed` SSE + snapshot
   endpoint (backend 4.5). Includes derived-disarm countdowns.
5. **Metrics as chart sub-panes.** `GET /api/tokens/{mint}/metric-series` (backend 5.7)
   rendered as time-synced panes under the price chart; selecting a rule overlays its
   thresholds as price-line-style levels and its entry/exit markers.
6. **Generic sweep axes + one-click promote.** Axis rows = (side, group, metric, operator,
   values[, window]) from the registry; Promote deep-links into the editor pre-filled
   (fingerprint find-or-created at the run's bucket width — width parity preserved).
7. **IA by concept, live/lab split stays build-time** (no runtime gating), boundary lint
   rules unchanged (shared ⊬ @live/@lab, live ⊬ @lab, lab ⊬ @live).

## 2. Information architecture

```
 BOTH apps                          LAB only                 LIVE only
 ├─ Rules        (list, all modes)  ├─ Sweep    (§3.4)       ├─ Live monitor (§3.2)
 ├─ Rule editor  (§3.1)             ├─ Simulate (full runs;  ├─ Trade / Wallet /
 ├─ Fingerprints (library: list ·   │   dry-run lives in     │   Portfolio / Home
 │   used-by-N · 👁 recent matches) │   the editor)          │   (unchanged)
 └─ Tokens + chart panes (§3.3)     ├─ Creation stats (kept) │
                                    └─ Replay viewer (§3.5,  │
                                        needs backend Ph 6)  │
 deleted routes: /strategies/{tpsl1,tpsl2,swing1} (both apps),
                 /strategies/grouped-sweep-{tpsl1,tpsl2,swing1},
                 /analysis/{swing-detection,swing1-detect}
```

## 3. Screens (mockups = build target)

### 3.1 Rule editor (both apps; dry-run panel lab-only until live bin proxies simulate)

```
┌─ Rule editor ─────────────────────────────────────────────────────────────────┐
│ Name [alpha-3.5-fast   ]  Mode (•paper ○real)  Buy [0.5 ◎]  Caps [3]/[50]     │
│ Fingerprint  [ fp: 3.5◎ create_v2 6-ix   ▼ ]  (used by 4 rules)  [+ new] [👁] │
│ TP [100 %]   SL [30 %]                                                        │
├───────────────────────────────┬───────────────────────────────────────────────┤
│ ENTRY                         │ EXIT                                          │
│ ▾ m_snapshot                  │ ▾ m_snapshot                                  │
│    time       [ >10, <30    ] │    time       [ >120        ]                 │
│    liquidity  [ >=20        ] │    liquidity  [ <5          ]                 │
│ ▾ m_price_path                │ ▸ m_price_path                (unset)         │
│    stall      [ <10         ] │ ▾ m_time_window   window_size_sec [ 30 ]      │
│    trail      [ <10         ] │    net_flow   [ <0          ]                 │
│ ▾ m_time_window               │                                               │
│    window_size_sec [ 10 ]     │   (empty metric = unconstrained)              │
│    net_flow   [ >5          ] │                                               │
├───────────────────────────────┴───────────────────────────────────────────────┤
│ [Builder] [JSON]   ← JSON tab: raw params, registry-validated, copy/paste     │
├────────────────────────────────────────────────────────────────────────────────┤
│ DRY RUN — replay last [24h ▼] ............................ [▶ Run]            │
│ armed 214 → entered 12 → TP 5 · SL 4 · metrics-exit 2 · dead 1   PnL +1.8 ◎  │
│ [ entered tokens table … click row → token chart with decision markers ]      │
└────────────────────────────────────────────────────────────────────────────────┘
```

- Groups/metrics/units/placeholders all from the registry; strict params (window) render
  as plain inputs inside their group header.
- Each metric input: grammar-parsed on blur/keystroke; parsed conditions echoed as chips
  under the input; malformed fragment underlined red with the registry unit in the hint.
- Editing a live rule keeps today's lock semantics: sizing/caps editable, conditions
  locked while `is_active || open_positions > 0`.

### 3.2 Live monitor (live app) — armed + holding in one place

```
┌─ Live monitor ──────────────────────────────────────────────────────────────────┐
│ ● armed 37   ● holding 4   ● today: entered 21 · TP 9 · SL 7 · disarmed 156    │
├─ ARMED (token × rule) ──────────────────────────────────────────────────────────┤
│ token      rule          age    blocking condition        value → need    ~ETA  │
│ 7xKq…pump  alpha-3.5     8.2s   time > 10                 8.2  → 10      1.8s  │
│ 7xKq…pump  slow-scalp    8.2s   net_flow(30s) > 5◎        3.1◎ → 5◎       —    │
│ 9mTt…pump  alpha-3.5    22.1s   liquidity >= 20◎         14.9◎ → 20◎      —    │
│            ⚠ disarms in 7.9s (time < 30 unsatisfiable at 30s)                  │
├─ HOLDING ───────────────────────────────────────────────────────────────────────┤
│ token      rule        entry    now      pnl     nearest exit                   │
│ 3fGh…pump  alpha-3.5   0.031    0.052   +68%    TP at 0.062 (+100%)            │
└─────────────────────────────────────────────────────────────────────────────────┘
```

- Data: `GET /api/strategies/armed` snapshot + `armed_changed` SSE deltas (coalesced
  ≤2/s per token server-side); holding section reuses the position-delta SSE path.
- "Blocking condition" and disarm countdown come straight off the `ArmedDelta` payload
  (the engine already evaluates every condition — surfacing the first false one is free).
- Built with `DataTable` + the `useRulePositions`-style SSE-patch pattern. Replaces
  `ArmedHistoryPanel`; `LiveTradingPage`/Home widgets keep working (position SSE
  unchanged) and get relabeled later.

### 3.3 Token chart metric panes (both apps)

```
┌ price ─────────────────────────────────────────────────────────────┐
│      ▲E                                    ▲X                       │  E=entry X=exit
│   ╱╲╱ ╲╱╲_╱╲╱‾╲                        ╱╲╱                          │  (existing marker
│  ╱             ╲╱╲╱╲╱╲╱╲╱╲╱╲╱╲╱╲╱╲╱╲╱╲╱                             │   plugin reused)
├ liquidity (◎) ── rule: >=20 ────────────────────────────────────────┤
│ ────────────────────20◎─────────────────────────────  ← threshold   │
│  ▁▂▄▆▇▇▆▆▅▅▆▇▇▇▆▅▄▄▃▃▄▅▆▆▇                                          │
├ net_flow 10s (◎) ── rule: >5 ───────────────────────────────────────┤
│ ─────5◎──────                                                       │
│  ▂▅▇▆▃▁▁▂▄▆▅▃▂▁ ▁▁▂▁                                                │
├ stall (s) ── rule: <10 ─────────────────────────────────────────────┤
│  ▁▁▁▂▁▁▁▃▅▇ ▁▁▂▁▁▁▁▂▃▄                                              │
└─────────────────────────────────────────────────────────────────────┘
   pane picker: [time] [liquidity✓] [stall✓] [trail] [net_flow✓] …  rule: [alpha-3.5 ▼]
```

- Data: `GET /api/tokens/{mint}/metric-series?windows=…` (computed on demand — metrics
  are never persisted). Pane picker + units from the registry; rule selector overlays
  that rule's thresholds + entry/exit markers via the existing
  `buildEventSeriesMarkers` infra.
- Rendering: `lightweight-charts` **v5 native panes** — verify the installed version
  first; upgrade if <5 (v4→v5 migration touches `TokenPriceChart.tsx`'s series-creation
  calls). Fallback if upgrade is deferred: stacked chart instances with a shared
  time-scale sync hook. Panes live in a new `MetricPanes` layer so the 1.9k-line
  `TokenPriceChart.tsx` core is extended, not rewritten.

### 3.4 Sweep (lab app) — generic axes + promote

```
┌─ Sweep config ────────────────────────────────────────────────────────────────┐
│ corpus [last 7d ▼]  group by [init_buy, cu_price ▼]  width [0.1◎]  method …   │
│ AXES                                                                          │
│  side   group          metric      op    values                               │
│  entry  m_snapshot     time        >     [5, 10, 15, 20]              [×]     │
│  entry  m_snapshot     liquidity   >=    [10 .. 40 step 10]           [×]     │
│  entry  m_time_window  net_flow    >     [0, 2.5, 5]   window [10s]   [×]     │
│  exit   —  TP %              [50, 100, 200]                           [×]     │
│  [+ add axis]                                    projected combos: 288        │
├─ results (grouped, streamed — pattern unchanged) ─────────────────────────────┤
│ group ▸ best combo ▸ [Promote…] ──▶ Rule editor pre-filled (fingerprint       │
│   find-or-created at the run's bucket width) → review → dry-run → save        │
└────────────────────────────────────────────────────────────────────────────────┘
```

- Keeps: `SweepConfigForm` shell (corpus/method/caps/localStorage), grouped results +
  streaming (`useStreamedSweepResults`, `GroupedSweepView`), `FingerprintGroupPicker`
  (group-by + value filters + bucket width — still valid, fields from the registry's
  `fingerprint_fields`).
- Replaces: `TPSL{1,2}_AXES`/`SWING1_AXES` static axis defs → registry-driven axis-row
  builder; `serializeGroupFingerprint`/`serializeCombo` blob path → `[Promote…]` calling
  backend 5.6 and routing to the editor.

### 3.5 Replay viewer (lab, optional — backend Phase 6)

Load a recorded live event log (or mint/time slice) via `POST /api/replay/inspect`;
timeline of event→effect decisions on the left, token chart with synchronized cursor on
the right. Step/play controls. Ship last; nothing else depends on it.

## 4. Target frontend structure

```
src/shared/
├── lib/strategy/                       NEW — replaces lib/params entirely
│   ├── registry.ts                     types + useStrategyRegistry() (RTK Query, cached)
│   ├── grammar.ts                      comma-AND grammar: parse/format (wraps numericFilter)
│   ├── ruleParams.ts                   RuleParams TS types + toJson/fromJson (generic)
│   └── validate.ts                     client-side mirror of backend §5 validation
├── components/strategy/
│   ├── ConditionInput.tsx              grammar input + chips + unit hint (one metric)
│   ├── ConditionSideEditor.tsx         ENTRY/EXIT column (groups from registry)
│   ├── RuleEditor.tsx                  §3.1 (builder + JSON tab + dry-run slot)
│   ├── FingerprintPicker.tsx           select + used-by + 👁 matches + inline create
│   └── DryRunPanel.tsx                 draft simulate results (lab)
├── components/table/numericFilter.ts   EXTENDED: compound comma-AND parse (shared SSOT)
└── components/token-price-chart/
    └── MetricPanes.tsx                 NEW pane layer (+ registry pane picker)

src/live/pages/MonitorPage.tsx          §3.2 (+ live/components/monitor/*)
src/live/pages/RulesPage.tsx            list + RuleEditor (replaces TpslPage/Swing1Page)
src/lab/pages/RulesPage.tsx             authoring twin (+ DryRunPanel active)
src/{live,lab}/pages/FingerprintsPage.tsx
src/lab/pages/SweepPage.tsx             §3.4 (replaces 3 sweep pages)
src/lab/pages/ReplayViewerPage.tsx      §3.5 (optional)
src/shared/services/sse.ts              +connectArmedChanged(cb)
```

**Deleted at the end:** `lib/params/` (types/engine/specs), `SpecRuleForm.tsx`,
`PasteParamsSection.tsx` (JSON tab covers paste), `components/{tpsl1,tpsl2}/ruleColumns.tsx`
per-strategy variants, sweep `groupedTypes.ts` static axes + blob serializers, per-strategy
pages listed in §2. **Kept:** `DataTable` + server-table plumbing, `sse.ts` multiplexer,
`RunPositionsPanel`/`useRulePositions`, chart plugin infra, RTK Query split-api layering,
`positionColumns`/`strategyColumns` status SSOT (minus strategy names).

## 5. Phases

> DoD every phase (hunter rules): `npm run build:live` clean, `npm run lint` clean
> (boundary gate), no extra re-render on SOL/USD tick or live-trade stream; update
> `docs/arch/frontend.md` when structure changes.

### FE0 — Registry + grammar foundation  (needs backend Phase 1 + 4.8 registry endpoint) ✅ 2026-07-17

- [x] 0.1 `lib/strategy/registry.ts`: types mirroring the registry payload +
      `useStrategyRegistry()` (RTK Query `getStrategyRegistry` in `sharedEndpoints`,
      1 h cache, one fetch) + `unitSuffix`/`findGroup`/`findMetric` helpers.
- [x] 0.2 Extend `numericFilter.ts`: compound comma-AND (`parseConditionList`), strict
      mode (no contains fallback), `formatConditionList` round-trip + `conditionListPredicate`;
      10 unit tests (`numericFilter.conditions.test.ts`) incl. `">10, <=30"`, `"1..10"`,
      `"!=0"`, `"=="→"="`, malformed fragments. `Comparison`/`CompareOp` are the shared SSOT
      the strategy grammar wraps. Server-side `TableRequestBody.filters` stays single-spec
      for now (wire extension = later follow-up).
- [x] 0.3 `lib/strategy/ruleParams.ts` (generic JSONB ⇄ form model, registry-guided
      strict/metric split) + `validate.ts` (registry-driven, ports backend §5
      `check_satisfiable`; same error vocabulary).
- [x] 0.4 `components/strategy/ConditionInput.tsx` (grammar input + parsed chips +
      red-underline malformed hint + unit adornment; text-first with blur snap-back).

### FE1 — Fingerprints + Rules pages  (needs backend Phase 0 + 2 + 4.8 CRUD) ✅ 2026-07-17 (live app)

> **Scoping note:** the CRUD/registry/armed endpoints are **live-bin only** (Phase 4.8
> mounted them on live, not lab), and amounts are **lamports** on the wire. FE1 mounts
> the pages in the **live app** (where the operable create→rule→activate flow lives);
> lab-app authoring parity + the lab-bin CRUD routes are folded into **FE3** (where the
> lab dry-run makes authoring there meaningful). Backend add this phase: `used_by` rule
> count folded into `GET /api/fingerprints`. The 👁 recent-matches preview
> (`GET /api/fingerprints/{id}/matches`) is **deferred** with its backend endpoint.

- [x] 1.1 `@live/pages/strategies/FingerprintsPage.tsx`: DataTable list (criteria chips,
      bucket width, used-by badge) + create/edit `FingerprintForm` (SOL inputs → lamports
      at the API boundary) + used-by-guarded delete.
- [x] 1.2 `components/strategy/FingerprintPicker.tsx`: select + per-row used-by + inline
      "+ new" (modal `FingerprintForm`, auto-selects the result). 👁 recent-matches
      preview deferred (backend endpoint absent).
- [x] 1.3 `components/strategy/RuleEditor.tsx` + `ConditionSideEditor.tsx` +
      `ConditionInput` (FE0) + Builder/JSON tabs; lock semantics ported (sizing/caps
      editable while live, fingerprint + conditions locked when `is_active`).
- [x] 1.4 `@live/pages/strategies/RulesPage.tsx`: one DataTable for all generic rules
      (mode/status badges), row actions edit/duplicate/activate-pause/delete; RTK Query
      cache-tag invalidation on every mutation (SSE live-count refresh lands with FE2).
- [x] 1.5 Live route/nav entries for `strategies/rules` + `strategies/fingerprints`; old
      per-strategy pages kept (relabeled "legacy"), removed in FE5.
- [x] 1.6 **Operable end-to-end** (create fingerprint → rule → activate) — code-complete;
      live-stack runtime smoke pending (same gate as backend 4.9).

### FE2 — Live monitor  (needs backend Phase 4) ✅ 2026-07-17 (functional; rich columns deferred)

> **Scoping note:** the engine's `ArmedDelta` / armed snapshot carry only
> `{mint, rule, state, reason?}` — **not** per-condition evaluation detail. The rich
> "blocking condition / value→need / ETA / disarm countdown" columns (§3.2) need the
> pure engine to compute + attach the first-failing-condition + current value to every
> `ArmedDelta` (a hot-tick-path change) — **deferred** as its own backend item. FE2
> ships the functional monitor built on the data that exists today.

- [x] 2.1 `sse.ts`: `connectArmedChanged` (arm/disarm deltas + `onReopen` snapshot
      refetch) + `connectStrategyPositionUpdate`. `getArmed` snapshot in `liveEndpoints`.
- [x] 2.2 `MonitorPage` armed table (token · rule · age) + holding table (token · rule ·
      status · entry price), both live via SSE. Rich distance-to-fire columns deferred
      (see note).
- [x] 2.3 Header stat strip (armed / holding / entered-session / disarmed-by-reason
      session counters from snapshot + deltas).
- [x] 2.4 Perf: bounded armed/holding sets held in `Map` state, rows memoized, one
      page-level 1 s age tick (no per-SOL/trade re-render). Live route + nav.

### FE3 — Dry-run + generic simulate  (needs backend Phase 5.1–5.3) ✅ 2026-07-17

> **Backend added this phase (the FE1-deferred lab mount):** the CRUD/registry parse
> helpers moved to core SSOT (`Fingerprint::from_json`, `RuleDraft::from_json`,
> `apply_rule_update`, `opt_i64`) — live `engine.rs` refactored onto them; the **lab bin
> now serves** `/api/meta/strategy-registry` + fingerprint/rule CRUD
> (`lab/.../engine_crud.rs`, `reload_rules` is a no-op — no running engine, live picks up
> rules on reload). This unblocks lab authoring + the dry-run editor.
> **Boundary structure:** page bodies extracted to shared `RulesView`/`FingerprintsView`;
> `RuleEditor` gained a `renderDryRun` render-prop; the lab-only `DryRunPanel` is injected
> by the lab page (shared editor never imports lab-only endpoints).

- [x] 3.1 `@lab/components/strategy/DryRunPanel.tsx`: draft (inline) → `POST
      /api/strategies/simulate` over a chosen window → `simulation_finished` SSE → funnel
      summary (`SimSummary`). Injected into the shared editor via `renderDryRun`.
- [x] 3.2 `@lab/pages/strategies/SimulatePage.tsx`: full-corpus runs for saved rules
      (one generic surface; per-rule Run → SSE → inline `SimSummary`). Lab authoring twins
      `RulesPage`/`FingerprintsPage` + routes + nav.
- [~] 3.3 Entered-token row → token chart with decision markers — **deferred** (needs the
      paged entered-tokens table + chart-marker wiring; the summary funnel ships now). The
      per-exit-reason breakdown (TP/SL/metrics/dead) also needs a richer summary aggregate.

### FE4 — Chart metric panes  (needs backend Phase 5.7)

- [ ] 4.1 Check `lightweight-charts` version; upgrade to v5 if needed (migration commit
      isolated from feature commits).
- [ ] 4.2 `MetricPanes.tsx`: pane add/remove from registry picker, series fetch via
      `metric-series` endpoint, shared crosshair/time-scale with the price pane.
- [ ] 4.3 Rule overlay: rule selector → threshold lines per visible pane + entry/exit
      markers (existing plugin); persists pane/rule prefs to localStorage like the
      toolbar does.

### FE5 — Sweep UI + promote + cleanup  (needs backend Phase 5.4–5.6)

- [ ] 5.1 Axis-row builder (side/group/metric/op/values/window) with projected-combo
      count; values accept list `a, b, c` and range `lo .. hi step s` (grammar reuse).
- [ ] 5.2 `SweepPage`: config + streamed grouped results on the kept
      `GroupedSweepView`/`useStreamedSweepResults` pattern; single generic endpoint set.
- [ ] 5.3 `[Promote…]`: backend 5.6 call → navigate to RuleEditor pre-filled → dry-run →
      save. Delete the blob copy path.
- [ ] 5.4 **Cleanup sweep**: delete everything in §4 "Deleted at the end", remove dead
      routes/nav, grep-sweep FE for `tpsl`, `swing1`, `strategy_id`, `serializeCombo`.
- [ ] 5.5 `npm run build:live` + `build:lab` + `lint` + manual smoke of every §2 page;
      update `docs/arch/frontend.md`.

### FE6 — Replay viewer (optional; needs backend Phase 6)

- [ ] 6.1 `ReplayViewerPage`: log slice loader, decision timeline, chart cursor sync,
      step/play. Ship only after FE1–FE5 are stable.

## 6. Risks / notes

- **Grammar server round-trip**: rule text inputs format from parsed JSONB, so `">10,<30"`
  vs `"<30, >10"` normalizes on save — display order = JSONB order; document in the input
  hint to avoid "it reordered my text" confusion.
- **lightweight-charts v5 upgrade** is the one dependency risk (touches the 1.9k-line
  chart core); the stacked-synced-charts fallback keeps FE4 shippable without it.
- **Live monitor volume**: a permissive fingerprint could arm hundreds of tokens; server
  coalescing + virtualized table rows if needed (measure first).
- **Boundary gate**: monitor is `@live`-only, dry-run/sweep/replay are `@lab`-only, all
  builders/pickers/grammar live in `shared` — anything that would import across the gate
  gets relocated, per the lint rule.
