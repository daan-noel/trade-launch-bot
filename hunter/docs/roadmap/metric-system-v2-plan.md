# Metric system v2: tags, spans, stages

The metric, fingerprint and rule system is restructured around four ideas:

- every metric is **`m_family.quantity @tag [span]`**: what, whose trades, over what stretch;
- a fingerprint keeps **tags**: named trade lists, each splitting a coin's trades in two;
- a rule is **enter, signals, always, stages**: a stage machine with deadlines (checkpoints)
  and events;
- every family, quantity, matcher, span and rule part carries **one definition with an
  example**, served by the registry and rendered as-is by every UI surface and the Guide page.

Every step keeps today's decisions: the same trades, the same exit prints, the same SOL, on
the baseline set (section 9). Section 6.4 shows how today's latches convert exactly.

## 1. Terms

| term | meaning | replaces |
| --- | --- | --- |
| ix shape | a transaction's ordered instruction list, optionally with its fee preset (CU limit, CU price, tip) | "build", "ix pattern" |
| ix template | the coarse shape `program|CU|ATA|N|S|F` (`template_grain`) | "grain", "working template" |
| tag | a named trade list in a fingerprint; a trade carries a tag or not | "tagged", "volume side", "dump list", "copy targets", "working list" |
| `@tag` / `@!tag` | the trades with / without the tag | tagged_* / untagged_* |
| span | the stretch a metric counts over | window, lifetime, after_age anchor |
| family | one subject (money moving, the chart, our position) | metric group |
| quantity | one measured number in a family, unit in its last word | metric |
| signal | a named condition in a rule, written once, used by name | duplicated clauses |
| stage | a step of the rule's plan after the buy, with its own sell / move lines and optional deadline | `arm`, `armed`, `since_armed`, `arm_above_pct`, `scale_out` |
| line | `if <conditions> -> sell "label" / go to <stage>` | exit clause |

## 2. Families and quantities

Unit suffix is the last word: `_sol`, `_tokens`, `_count` (prints), `_tx_count`
(transactions: `leg_index = 0`), `_pct`, `_sec`, `_slots`; no suffix = a 0/1 flag.

| family | subject | tag | span | quantities (old name) |
| --- | --- | --- | --- | --- |
| m_state | the pool now | - | - | age_sec (time), liquidity_sol (liquidity), on_curve |
| m_price | the chart | - | life, window | rise_pct (rise), trail_pct (trail), stall_sec (stall, life only) |
| m_flow | money moving | optional | life, window | buy_sol (buy, tagged_buy, untagged_buy, copy buy_sol), sell_sol, net_sol, gross_sol, buy_count, sell_count, trade_count, buy_tx_count (copy buy_count), sell_tx_count (copy sell_count, dump_sell_count), share_pct (tagged_share), buy_share_pct (buy_share), slice trade_share_pct / sol_share_pct |
| m_holdings | what a tag's trades hold and made | required | - | profit_sol (tagged_pnl), bag_share_pct (public_app_share, bundled_share) |
| m_crowd | who shows up | - | window, since age | unique_wallets, trades_per_wallet, unique_ix_shapes (unique_builds), non_creator_buyers, buyer_is_new (this_buyer_is_new) |
| m_print | the print being read | - | - | since_buy_sec (since_buy) |
| m_slot | this slot's same-slot bundle | optional (`@working`) | - | today's m_burst_slot quantities, `working_` dropped: `buy_count @working` |
| m_wave | the current buy wave | optional (`@working`) | - | today's m_burst_wave quantities |
| m_position | our trade | - | - | pnl_pct, held_sec, retrace_pct, bounce_pct, room_taken_pct, stage_sec |

A quantity declares which tags and spans it accepts; the parser rejects any other. Adding
tag or span support to a quantity is a registry change plus its compute arm, nothing else.
`m_flow` counts keep both bases because today's metrics use both: `buy_count` counts prints
(every leg), `buy_tx_count` counts transactions. The exact old-to-new map, metric by metric,
is the table in `engine/src/metrics/v1_map.rs` (the migration's source, section 7).

## 3. Spans

| span | JSON | meaning |
| --- | --- | --- |
| life | absent | since the coin was created |
| last N seconds / slots / prints | `"10s"`, `"20sl"`, `"5p"` | trailing window, closed `[now - N, now]` |
| lagged | `"10s@2"` | the same window ending `2` units ago |
| since age | `"age60s"` | from coin age 60 s to now (today's `m_crowd_after_age`) |
| slice | `"slice": "2s"` beside the span | the nested second window of `trade_share_pct` / `sol_share_pct` |

## 4. Fingerprint

```json
{
  "criteria": { "ix_labels": {...}, "cu_price": {...}, "name_reuse_count": {...} },
  "tags": {
    "volume": {
      "match": {
        "program":     ["Unknown (9ddjzq...)"],
        "ix_shape":    [["A","B"], {"labels":["A","C"],"cu_price":"1000"}],
        "ix_template": ["pump|CU|ATA|1|0|0"],
        "ix_contains": ["Axiom"],
        "ix_lacks":    ["Photon"],
        "wallet":      ["7xk..."],
        "creator":     true,
        "cluster":     {"min_prints": 3, "sol_tol_pct": 10}
      },
      "side":    "sell",
      "sticky":  true,
      "exclude_creation_slot": true
    }
  }
}
```

A trade carries the tag when **any** `match` entry holds, on the given `side` only. The
matchers are read in a fixed order and `cluster` last, because a cluster counts every trade
it reads into its slot group (today's classifier order). `sticky`: a wallet that carried
the tag once carries it for the rest of the coin. `exclude_creation_slot`: a creation-slot
buyer that matches nothing counts in neither `@tag` nor `@!tag`. Built-in tags need no
config and serve `m_holdings.bag_share_pct` only: `bundled` (first buy in a slot where
>= 3 wallets first bought with one ix shape) and `public_app` (first buy through an app with
> 100 buyers the day before). Tag names are `[a-z0-9_]{1,24}`.

`m_slot` / `m_wave` read a tag at the ix-template level (today's working list), so a tag
used there may carry only `ix_template` and `program` matchers.

| today | v2 |
| --- | --- |
| `m_flow_ix` { ix_patterns, tagged_ix_markers, untagged_ix_markers, tagged_programs, volume_cluster, creator_is_tagged, wallet_contagion, creation_slot_buyers } | tag `volume` { ix_shape, ix_contains, ix_lacks, program, cluster, creator, sticky, exclude_creation_slot } |
| `m_dump_ix` { ix_patterns, creator_is_listed } | tag `dump` { ix_shape, creator, side: sell } |
| `m_copy` { target_wallets } | tag `targets` { wallet } |
| `m_burst_slot` { working_templates } | tag `working` { ix_template, program } |
| axis `prior_identity_launches` | axis `name_reuse_count` |

The DB column `fingerprints.metric_config` is renamed `tags`.

## 5. Rule

```json
{
  "enter": {
    "event":         [cond],
    "filters":       [cond],
    "final_filters": [cond],
    "lock":          "token",
    "size_pct_of_pool": 1.5
  },
  "signals": { "cashout": [[cond, cond], [cond, cond, cond]] },
  "always":  [ {"if": [cond], "sell": "top"} ],
  "stages": [
    { "name": "early", "ends": {"age_sec": 20},
      "on":     [ {"if": [{"signal": "cashout"}], "sell": "spike"} ],
      "at_end": [],
      "then":   "late" },
    { "name": "late",
      "on": [ {"if": [{"signal": "cashout"}], "go": "ride"} ] },
    { "name": "ride", "ends": {"stage_sec": 30},
      "on": [ {"if": [cond], "sell": "burst"} ] }
  ],
  "reentry":   { "cooldown_sec": 30, "max_per_coin": 3 },
  "exclusive": false,
  "priority":  0
}
```

- `cond` = `{"metric": "m_flow.buy_sol", "tag": "!volume", "span": "10s", "is": [{"operator": ">=", "value": 2}]}`
  or `{"signal": "cashout"}` / `{"signal": "cashout", "not": true}`. `is` keeps today's
  condition grammar (a list is AND, a list of lists is OR).
- a line's `if` is AND; OR is two lines or a signal. Actions: `"sell": "label"` (the whole
  bag), `"sell": "label", "sell_pct": 50` (part of the initial bag), `"go": "stage"`; a partial
  sell may also `go`.
- `ends`: `{"age_sec"}` (coin age), `{"held_sec"}` (since our buy), `{"stage_sec"}` (since this
  stage began). At the deadline `at_end` runs once (first match wins), else `then`, else the
  next stage in the list.
- `take_profit` / `stop_loss` stay as sugar: they compile to `always` lines on
  `m_position.pnl_pct` (stop loss first).
- `enter.filters` fail -> keep watching; `enter.final_filters` fail on a print that would
  otherwise buy -> stop watching this coin (today's hidden "leftover" metrics, made explicit).
- `m_position.stage_sec` (seconds since the current stage began) is readable in any line, so
  a condition like "in the first 30 s of the ride" can also be written without a deadline.
- A line without a `sell` label is labelled from its first condition
  (`m_flow.buy_sol @!volume [10s] >= 2`).

### 5.1 Evaluation order (each print and each 200 ms tick, while holding)

**One step per print or tick.** Each evaluation takes at most one action:

1. `always` lines in order; the first that holds acts (sell, or go).
2. Else, if the stage's deadline is reached: the first `at_end` line that holds acts; if
   none holds, the rule moves to `then` (default: the next stage).
3. Else the stage's `on` lines in order; the first that holds acts.

A move takes effect from the next print or tick: the new stage's lines are first read
there. A partial sell moves the stage when its fill lands (today's ladder rule). This is
exactly how today's engine treats an `arm` latch, so no stored rule changes behaviour.

Before the buy, the rule never buys while an `always` line or a start-stage sell line
already holds (today's pre-entry veto).

### 5.2 Today's grammar in v2

| today | v2 |
| --- | --- |
| `entry` / `entry_event` / `entry_lock` / `buy_pct_of_vsol` | `enter.filters` / `.event` / `.lock` / `.size_pct_of_pool` |
| `exit` object (OR of every metric) | one `always` line per metric |
| `exit` array (OR of AND-clauses) | one `always` line per clause |
| `arm_above_pct: X` on a single retrace/bounce clause | the same line plus `m_position.pnl_pct >= X` (the gate is read at each evaluation, not latched) |
| a clause with `m_position.armed = 1` under a pnl latch | stage `armed`, entered by `pnl_pct >= X -> go armed` from the start stage (section 6.4) |
| `arm` clauses + `armed` / `since_armed` | one `go armed` line per clause + `m_position.stage_sec` |
| `scale_out` ladder | one stage per rung: `on cond -> sell_pct, go next`; the remainder rung sells all |
| `disabled` (a separate bag) | `"off": true` on the condition or line itself: kept in place, validated, never compiled |
| exit reason `untagged_buy(2s) >= 0.9` | the line's label; converted lines keep today's generated text |

## 6. Engine

### 6.1 Registry

`FamilySpec { name, subject, summary, example, quantities }`,
`QuantitySpec { name, unit, summary, example, tags: TagUse, spans: SpanUse, monotonic, eq_tolerance, hue }`,
`SpanKindSpec`, `MatcherSpec { name, value_type, summary, example }`, `RulePartSpec` (enter,
signal, always, stage, line, ends). `summary` is one line, `example` one line with a number.
`registry_json()` serves all of it; the frontend renders nothing about a metric, matcher or
rule part that the registry does not say.

### 6.2 Compile once, dense at runtime

- `MetricRef { quantity: QuantityId, tag: Option<(TagIdx, negated)>, span: SpanKey, slice }`
  resolved at rule load to dense indices; no name lookup after load.
- One `TagState` per (fingerprint, tag) replaces the three per-fingerprint states (flow,
  dump, copy): lifetime totals for both halves (SOL, prints, transactions per side), one
  window buffer per span, the sticky set, cluster groups and the holdings bag. Same number
  of states per token as today, so the per-trade cost does not grow. The working list stays
  a read-time template set on the token's one slot state.
- Each quantity declares the spans and tags it accepts, and the parser rejects the rest.
  The token-level flow window stays lean (8 bytes per print, no transaction flag) because
  most rules read it; the transaction counts therefore need a tag.
- Stage deadlines join `ClockHorizons` (`age_sec` -> `time_secs`, `held_sec` -> `held_secs`,
  `stage_sec` -> a per-arm horizon), so the settled-token tick skip never skips a deadline.

### 6.3 Modules

```
engine/src/metrics/
  registry/{families.rs, spans.rs, matchers.rs, rule_parts.rs, json.rs}
  families/{state, price, flow, holdings, crowd, print, slot, wave, position}.rs
  tags/{classifier.rs, matchers/*.rs, state.rs}
  v1_map.rs                  old name -> v2 ref, used by the migration and its tests only
engine/src/rule/
  params.rs  compile.rs  stages.rs  eval.rs
engine/src/rule_v1.rs         today's parser, read-only, deleted after both boxes migrate
```

### 6.4 Exact conversion of today's latches

- `arm` clauses: the start stage's lines are the exit clauses, then one `go armed` line per
  arm clause; `since_armed` becomes `m_position.stage_sec`. One step per evaluation is
  today's "latch after the exits of that event".
- `arm_above_pct` latch with `armed = 1` clauses: today the latch sets BEFORE the exits of
  the event it crosses on. The start stage therefore carries, per armed clause, the line
  `pnl_pct >= X and <clause> -> sell` (it can hold only on the crossing event), then
  `pnl_pct >= X -> go armed`; the armed stage carries `<clause> -> sell`.
- Exit labels: a converted line keeps a fixed label built from its first condition. Today
  the label names whichever condition arm matched, so a label may read differently; the
  parity check compares exit print and SOL, and reports label differences separately.

## 7. Storage and migration

- **Core data migration** (`core/src/storage/data_migrations.rs`, ledger
  `_data_migrations`), run at boot after the SQL migrations, one transaction: converts
  `strategy_rules.params`, `strategy_runs.params_snapshot` and `fingerprints.tags`
  (renamed from `metric_config` by core `0021`) plus fingerprint criteria, validates every
  converted tags document, and clears each `Running` run's `config_hash` (it digests v1
  JSON). A fingerprint or rule that fails aborts the transaction, naming the row; a run
  snapshot that fails stays as written.
- **Lab data migration** (`lab/src/storage/lab_data_migrations.rs`) after lab `0006`
  (renames `grouped_sweep_runs.ix_patterns` / `scale_out` / `scale_out_top_k` to `tags` /
  `stage_plans` / `stage_plans_top_k`): converts each run's `axes_spec`, `tags` and
  `stage_plans`. Combo `params` and group `best_params` stay as written: every reader
  goes through `hunter_engine::v1::parse_params_any`, which is permanent (it also reads a
  v1 rule bundle).
- **Dry run**: `hunter-lab migrate-v2 --dry-run [--database-url <url>]` converts every
  core row in a transaction that is rolled back and prints what would change. It runs no
  schema migration and reads either column name, so it checks the server before the
  deploy.
- Historical text stays as written: `strategy_positions.exit_reason`,
  `strategy_arms.end_detail`. The UI shows old labels raw.
- A cached simulate result carries its rule format (`SimMeta.rule_format`); an older one
  is not loaded, so its rule re-simulates.
- Rule bundle format version 2; a v1 bundle is converted on import.
- Deploy order: the server migrates first. `db-incremental-sync.ps1` reads `tags` off
  the server's table, so it fails before writing anything while the server is on v1.

## 8. Frontend

- **Registry types** mirror 6.1; `strategyHelp.ts` keeps only page-level help, all metric,
  matcher and rule-part text comes from the registry.
- **Fingerprint editor**: two panels. *1. Which coins* (axes, each with its definition and an
  example). *2. Tags*: one card per tag with name, "a trade has this tag if ANY of", matcher
  rows (add from a menu of registry matchers, each showing its summary and example),
  "unless", side, sticky, creation-slot exclusion. A live line under each card: "on the last
  simulated coin: 41 of 212 trades carry `volume`".
- **Rule editor**: sections in the order the engine reads them: Enter, Signals, Always,
  Stages (cards with deadline, `on` lines, `at end` lines, next stage, and a stage strip
  `early -> late -> ride`), Settings (size, caps, re-entry, exclusive). A condition row is
  `[family.quantity] [@tag] [span] [is]` with its sentence written underneath from the
  registry ("SOL bought by trades without `volume` in the last 10 s is at least 2").
- **Rule readout**: the whole rule as sentences, one component, used by the rules list hover,
  simulate, the live console and sweep promote.
- **Staging surfaces** (token trades, Flow discovery, IxPatternBar): "add to tag [volume v]".
- **Guide page** (lab and live): the workflow picture, the terms table, every family,
  quantity, matcher, span and rule part, each with its summary and example, all from the
  registry.
- **Shared fixtures** keep the two sides one: `engine/fixtures/registry.json` (the
  registry document, pinned by `registry_fixture_is_current`) and
  `engine/fixtures/rule_v2_full.json` (every rule part, parsed by the engine test
  `the_shared_full_rule_fixture_parses_and_round_trips` and by `validate.test.ts`).
- Open: the per-tag live line on the fingerprint card ("41 of 212 trades carry
  `volume`") needs a classification read per coin; the editors do not yet show the
  backend's save `warning` (they compute the same warnings client-side).

## 9. Parity baseline and gates

Baseline, captured on commit `3fbe5f2e` before any change: every stored rule simulated over
09-19 .. 09-21 (lag_115, copycat guard off, curve only), plus the six 7ix rules over 09-01 ..
09-21. Per ticket: mint, entry time, exit time, exit reason, pnl SOL.

Each phase passes when:

- `cargo check` / clippy / tests clean on hunter-engine, hunter-core, hunter-live, hunter-lab;
  `npm test`, `npm run build:live`, `npm run lint` for frontend phases;
- the golden tests, ported to v2, produce the same effect streams;
- the baseline re-simulates ticket for ticket: same entries, same exit prints, same pnl SOL to
  the lamport (labels compared separately, section 6.4);
- `every_metric_is_live_reachable` covers every quantity x accepted tag x accepted span.

## 10. Phases

| phase | scope | done when | status |
| --- | --- | --- | --- |
| 0 | baseline capture; this plan | baseline files written | done: 227,997 tickets, 83 rules (3 short-window runs timed out) |
| 1 | engine: registry v2, tags + classifier, families, spans, `MetricRef`; rule v2 parser, compile, stage machine; `v1` converter; golden tests ported | engine gates | done |
| 2 | lab, core, live: sweep axes and kernels, candidates, rule and family search, metric series, engine_sim, readouts, arms end_detail paths, bundle v2, data migration + dry run | backend gates + baseline parity | done: gates pass; local database migrated; baseline parity: all 227,997 tickets match (same entries, exit prints and SOL; 0 differences), only line labels differ; the six 7ix rules rewritten with a signal and deadline stages simulate identically (09-01 .. 09-21) |
| 3 | frontend: registry types, fingerprint editor, rule editor, readout, staging, exit labels, Guide page | frontend gates + a walk through every editor | rule execution first: core lib, editors, readout, live console, exit labels, Guide, chart panes done; staging in progress; sweep, discovery and search pages next (compile-level first) |
| 4 | docs: `_!___metrics.md`, `arch/strategies.md`, `hunter/CLAUDE.md` landmines, `_!___terms.md` | check-docs clean | open |
| 5 | server: dry run on a server dump, deploy, migrate | needs the user's go-ahead | open |
