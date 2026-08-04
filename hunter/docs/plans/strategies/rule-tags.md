# Rule tags — free-form labels on `strategy_rules`

Deep-dive reference for the rule **tag** feature: a set of free-form labels on each
generic rule, used to slice the Rules board (show only `fam:scalper`, hide
`stage:experiment`). Tags are **presentational metadata** — they never reach the
decision kernel.

Structure/data-flow lives in [@arch/strategies.md](../../arch/strategies.md) and
[@arch/frontend.md](../../arch/frontend.md); this file holds the *why*.

---

## 1. Why a dedicated column and not `params`

`params` (JSONB) looks like the cheap home for a `tags` key. It is the wrong one, for
three reasons that are all load-bearing in existing code:

1. **`params` is canonicalized on every write.** `build_rule` stores
   `RuleParams::parse(&draft.params)?.to_value()` — a *re-serialization* from the typed
   `RuleParams` struct. An unknown `tags` key is silently dropped on the first save.
   Making it survive would mean adding `tags` to `hunter_engine::RuleParams`, i.e.
   putting a UI label inside the engine's decision struct.
2. **`params` is trading identity.** `RuleRepo::find_identical` compares
   `params = $6::jsonb` and the doc comment is explicit that `rule_name` / `is_active` /
   `is_enabled` are *labels, not identity*. A tag inside `params` would make two
   behaviourally identical rules stop colliding on the Duplicate gate — the gate would
   silently weaken as soon as anyone tagged a rule.
3. **`params` is frozen into run history.** `strategy_runs.params_snapshot` captures it
   at activation so later edits don't rewrite history. A tag rename would then read as a
   strategy change in the run navigator.

Tags belong in the same bucket as `rule_name`: a typed column, excluded from identity.

## 2. Why an array column and not a join table

`rule_tags(rule_id, tag)` is the normalized answer and is **not** worth it here:

- The rules list is fetched whole (`GET /api/strategy-rules` returns every row) and
  filtered client-side. There is no server-side tag query to index for.
- A join table adds a second query (or a lateral aggregate) to every list read, on a
  path that already folds in position counters.
- The tag catalog is derivable: `SELECT DISTINCT unnest(tags) FROM strategy_rules`.

`tags TEXT[] NOT NULL DEFAULT '{}'` it is. **No GIN index** — nothing filters by tag in
SQL today, and an unused index is a write cost on a table the live engine reloads from.
Add one only alongside a real server-side `WHERE tags && $1` query.

Per-tag colour is **hashed from the tag string**, not stored. A `rule_tag_defs(name,
color)` table would be a second thing to keep in sync for zero functional gain; a stable
string→hue hash gives every tag a consistent colour with no management surface at all.

## 3. Normalization — one authority, server-side

`trading_core::strategies::rules::normalize_tags` is the SSOT. It is **total** (never
errors) and canonicalizes shape only:

| Step | Reason |
| --- | --- |
| trim, lowercase | `Scalper` and `scalper` must not be two chips |
| whitespace / `_` → `-` | one word separator |
| drop chars outside `[a-z0-9:-]` | kills `,` — the DataTable filter grammar's separator |
| collapse repeated `-`, trim leading/trailing `-` | `a--b-` → `a-b` |
| drop empties, dedupe, **sort** | deterministic array ⇒ stable UI order + comparable rows |

**Count and length are validated, not truncated.** `validate_rule_fields` rejects
`> MAX_TAGS_PER_RULE` (8) or any tag longer than `MAX_TAG_LEN` (24) with a 400. Silently
dropping the 9th tag would be the "writer clamps the sentinel away" failure shape this
codebase has already been bitten by three times (see the zero-as-unbound section in
[hunter/CLAUDE.md](../../../CLAUDE.md)) — a loud rejection is correct for user input.

Normalization runs at **both** ends of the write path: `RuleDraft::from_json` (wire →
draft) and `rules::save` (loaded row → storage), mirroring exactly what `params` already
does. So the stored array is canonical no matter which door the write came through.

### The frontend deliberately does NOT reimplement this

`lib/strategy/tags.ts` has no copy of the grammar. The editor's chip input applies only
`trim().toLowerCase()` — a cosmetic transform that cannot meaningfully drift — and the UI
re-renders from the mutation response, which carries the server's canonical array. This
is a deliberate refusal to create a second normalizer that would need a guard test to
stay honest.

## 4. Tags are not `is_enabled`

`is_enabled` is a **lifecycle** flag: a disabled rule is soft-archived *and cannot be
activated*. Tags are a **view** filter with zero behavioural effect. Keep them
orthogonal — do not reimplement archiving as an `#archived` tag, and do not let a tag
gate arming. `RuleRepo::list_active` (what the live runtime cache loads) stays untouched
by this feature.

The API is shaped so a future `pause-all?tag=…` is a natural extension (the endpoint
already takes `mode`), but that is **not** in this change.

## 5. Namespacing convention

Tags may contain `:`, and the filter bar groups chips by the prefix before the first
colon. Suggested taxonomy so the tag set stays navigable rather than becoming a junk
drawer:

| Prefix | Meaning | Example |
| --- | --- | --- |
| `fam:` | strategy family | `fam:scalper`, `fam:ignition` |
| `src:` | provenance | `src:sweep`, `src:wallet-analysis` |
| `stage:` | where it is in the pipeline | `stage:paper-test`, `stage:promoted` |
| `risk:` | sizing / risk band | `risk:high` |

Nothing enforces this — it is convention, and an un-namespaced tag is legal.

## 6. Filter semantics

Chips are **tri-state**: off → include → exclude → off.

- **Include chips OR together.** Selecting `fam:scalper` + `fam:ignition` shows rules in
  either family. (AND across includes is the rarer intent and is not offered — it is
  reachable by typing in the column filter row.)
- **Exclude chips AND together** — a rule carrying *any* excluded tag is hidden.
- Exclude wins over include when a rule matches both.
- An untagged rule is visible unless at least one include chip is active.

State lives in the URL (`?tags=a,b&notags=c`), which composes with the existing
`?rule=<id>` selection param, so a filtered board is shareable and survives reload. The
last-used state also mirrors to `localStorage` per app, so the default view is sticky
across sessions — the URL wins when present.

## 7. Sync-script coupling (the thing that will bite)

`scripts/db-incremental-sync.ps1` hand-maintains the `strategy_rules` INSERT/SELECT
column lists and is documented to track `rule_repo::RULE_COLS`. Adding a column without
updating it means the mirror silently stops carrying tags. Both lists were updated with
this change.

**Deploy ordering.** The sync reads `r.tags` off a `postgres_fdw` foreign table, which
mirrors the *server's* schema. EC2 must have run core migration `0002` (redeploy the live
bin — migrations run at startup) **before** the next `db-incremental-sync.ps1`, or the
SELECT fails on an unknown column.

Second-order consequence, worth knowing before you tag anything: **that upsert is
server-wins on changed rows.** Tagging a live-owned rule *in the lab app* is clobbered on
the next sync. Author tags in the app that owns the rule; lab-only rules (never present
on the server) are unaffected.

## 8. Files

| Layer | File | What |
| --- | --- | --- |
| DB | `core/migrations/0002_rule_tags.sql` | `tags TEXT[] NOT NULL DEFAULT '{}'` |
| Model | `core/src/models/strategy.rs` | `StrategyRule.tags` (`#[serde(default)]`) |
| Domain | `core/src/strategies/rules.rs` | `normalize_tags` SSOT + caps + validation + draft/patch wiring |
| Repo | `core/src/storage/repositories/rule_repo.rs` | `RULE_COLS`, row struct, insert/update binds |
| Sync | `scripts/db-incremental-sync.ps1` | mirrored column lists |
| Wire | (none) | live/lab CRUD handlers serialize `StrategyRule` wholesale — no handler change |
| FE lib | `frontend/src/shared/lib/strategy/tags.ts` | colour hash, filter predicate, URL codec |
| FE hook | `frontend/src/shared/hooks/useTagFilter.ts` | URL + localStorage state |
| FE ui | `components/strategy/{TagChip,RuleTagFilter,RuleTagsInput}.tsx` | chip, filter bar, editor input |
| FE column | `components/strategy/ruleTagsColumn.tsx` | the ONE `tags` `ColumnDef`, shared by every rule table |
| FE wiring | `components/strategy/{RulesView,RuleEditor,useRuleActions}.tsx`, `lab/pages/strategies/SimulatePage.tsx` | column, filter bar, draft field |

On **Simulate** the same chip bar narrows `visibleRules`, which is what the
paper/real run buttons and *Simulate Filtered* target — so "run this family" is
one chip click plus one button.
