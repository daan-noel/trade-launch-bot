# Strategy bundle — moving a rule between the two boxes

Editing one ix pattern or one metric param changes **two rows**: a `fingerprints`
row and a `strategy_rules` row. A bundle is the door for those two rows, in both
directions, from the page they were edited on.

`scripts/db-incremental-sync.ps1` (step 7b) already mirrors `fingerprints` /
`strategy_rules` **server → local**, but only inside a full FDW pull — SSH tunnel,
superuser role, backend stopped — and only in that one direction. The bundle is the
small, two-way, in-UI path; the sync script stays the authority on history
(`strategy_runs`, `strategy_positions`, `strategy_run_metrics`), which a bundle
never touches.

Domain + HTTP edges:
[`trading_core::api::handlers::strategies::rule_bundle`](../../../core/src/api/handlers/strategies/rule_bundle.rs).
UI: `RuleSyncModal`, opened from the Rules board's **Sync** button in both apps.

---

## Three steps, and the middle one is the point

| Route | Effect |
| --- | --- |
| `GET /api/strategy-bundle?rules=<uuid>,<uuid>` | Serialize the selection (every rule when the param is absent). |
| `POST /api/strategy-bundle/preview` | The diff against this box, field by field. **Writes nothing.** |
| `POST /api/strategy-bundle/apply` | Execute that same diff. |

Both bins serve all three off the one shared module, so the diff you approve on one
box is computed by the same code that applies it on the other. A copy of this
resolver per bin would approve one change and apply another.

The preview is what makes this different from a faster manual `UPDATE`: a paste you
cannot read first fails exactly the way hand-written SQL does — you cannot see what
you are about to overwrite. The plan reports `identical` / `changed` / `new` per
item, the changed fields with both values, and which side's `updated_at` is newer.

---

## What travels

| Travels — the strategy | Stays with the box |
| --- | --- |
| `fingerprints.criteria`, `.wildcard`, `.metric_config`, `.name` | `strategy_rules.is_active` |
| `strategy_rules.params`, `.buy_amount_lamports` | `strategy_rules.is_enabled` |
| `.max_concurrent_tokens`, `.max_total_tokens`, `.rule_name`, `.tags` | `strategy_rules.trade_mode` |
| | `strategy_runs` / `strategy_positions` / `strategy_run_metrics` |

`is_active` is the safety-critical exclusion: if arming rode along, a paste from the
paper lab would arm a real-money rule on the live box. Three things enforce that,
none of them a convention:

* `BundleRule` has no such field — a unit test asserts the wire shape carries no
  `is_active` / `is_enabled` / `trade_mode`.
* An update goes through `rules::apply_rule_update`, which does not patch
  `is_active` / `is_enabled` / `fingerprint_id` at all, and the bundle's patch body
  omits `trade_mode` (which that function *would* patch).
* An insert goes through `rules::create_with_id`, which builds every new rule
  inactive.

A rule that is new on the target box therefore lands **idle and paper**. Promoting
it to real is a deliberate act there.

---

## Identity is the UUID

The two DBs already share UUIDs for these rows — the incremental sync copies `id`
across — so upsert-by-id is both correct and idempotent: paste the same bundle
twice and the second is `identical`. `create_with_id` exists for exactly this: a
rule that arrived under a fresh UUID would be a second copy, and every later paste
in either direction would fork again.

A fingerprint that is new *by id* can still be present *by identity*. The
`fingerprints_identity_uniq` index keys on `criteria` + `wildcard` +
`metric_config`, so inserting it would be rejected. The plan resolves that to
`reuse_existing`: nothing is inserted, and the bundle's rules are rebound onto the
row already there.

```
For each fingerprint in the bundle:

    UUID exists here? ── yes ─▶ identity now held by a DIFFERENT row?
        │                          ├─ yes ─▶ conflict (the index would reject it)
        │                          └─ no  ─▶ identical / changed
        └─ no ──▶ identity held by some row?
                     ├─ yes ─▶ reuse_existing: rebind this bundle's rules to it
                     └─ no  ─▶ new, keeping the bundle's UUID
```

Rules resolve their fingerprint only through that map. A rule whose fingerprint the
bundle does not carry is a conflict, never a lookup against whatever the target box
happens to hold under that UUID — that fallback lets a hand-assembled bundle point
a rule at an unrelated creation shape.

---

## Everything that can fail, fails in the preview

`plan_bundle` runs every check the write would run, without writing: the axis
registry parse (an unknown axis is an error, never a silent drop — dropping one
*widens* what the fingerprint matches), `Fingerprint::validate`,
`validate_fingerprint_metric_config`, `RuleParams::parse` against the metric
registry, the identity-index collision above, and the duplicate-rule gate.

So a box that lacks a metric group the bundle uses names the rule that cannot land,
in the preview, instead of 500-ing halfway through an apply. Vocabulary drift
between the two boxes surfaces as a per-item message rather than a wire-format
version bump.

Params are canonicalized (`RuleParams::parse` → `to_value`) **before** the diff.
Stored params are already canonical, so without that step an author's JSON key
order reads as a change on every paste.

### Atomicity

One conflict blocks the whole apply, **before the first write**. Past that gate the
writes are sequential — fingerprints, then rules, in FK order — not one
transaction, because the repos are pool-bound. A mid-flight DB error can therefore
leave the fingerprints written and the rules not; re-running the same bundle
finishes the job, because every step is an upsert keyed on the UUID. `apply`
re-plans from the posted bundle and never trusts a plan sent by the client.

The live bin schedules an engine reload whenever anything is written, including
after a mid-apply error: a partial apply still changed what the matcher reads.

---

## Picking what to send

The Copy pane filters on name / tag / fingerprint name, hides archived rules by
default (as the board does), and sorts **newest edit first** — the rule you want is
the one you just changed. It also prints the fingerprint count beside the rule
count, because the fingerprint set is derived and is otherwise invisible.

Its filters are local component state, deliberately **not** `useTagFilter` /
`useModeFilter`: those sync to `?tags=` / `?mode=`, which the Rules board behind the
modal already owns, so sharing them would make narrowing the picker silently
re-filter the board.

**Differs only** closes the loop. Paste the other box's bundle and preview it first;
the plan already says which of its rules are `changed` here, so each row in the Copy
list can be marked and the in-sync ones hidden. You stop picking blind and then
discovering on the far side what actually differed.

One ambiguity is labelled rather than hidden: a rule the plan never mentions shows
as **not in bundle**, which means "the other box lacks it" only if that bundle was a
full export. A partial export is indistinguishable from a missing rule, and no
amount of diffing can tell them apart from this side.

---

## Prerequisite

Both boxes run the same core migration chain. A bundle carrying `criteria` needs
core `0009` on the target; redeploying the live bin applies it (`sqlx::migrate!`
runs at boot). An un-migrated server fails the whole Rules UI, not just this — see
[db-patterns.md](../database/db-patterns.md).
