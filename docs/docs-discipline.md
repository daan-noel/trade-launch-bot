<!-- pt-ok:begin — this doc defines the rule, so it quotes the phrasing it forbids -->
# Docs discipline — tiers, present tense, and the gate

The rule and the tier table live in the root [../CLAUDE.md](../CLAUDE.md). This is the
reasoning behind them and the detail that does not need to be paid every session.

## Why the tiers exist

A `CLAUDE.md` is read into context on every session; a `docs/` file costs nothing until
someone Reads it. So `CLAUDE.md` stays a thin **index + hard rules**, and every
explanation moves down a tier:

| Changed | Write it in |
| --- | --- |
| A rule / command / constraint | the nearest **CLAUDE.md** (super-root, else the product's) |
| Module structure / data flow / behavior | `docs/arch/<subsystem>.md` — high-level map (crates, files, flow) |
| Implementation detail / algorithm / decision | `docs/plans/<subsystem>/<topic>.md` — deep-dive reference |
| Work that is still open | `docs/roadmap/<topic>.md` — deleted or folded into a deep-dive once it lands |
| An incident/RCA, a superseded approach, a research journal | `docs/history/<YYYY-MM-DD-slug>.md` |

`docs/arch/` is the "read this instead of re-exploring source" tier. `docs/plans/` holds
permanent deep-dive references — column rationale, invariants, tuning constants, design
decisions — never a throwaway plan.

## Present tense only

**Everything outside `docs/history/` describes what the system does *now*.** Write the
**rule**, never the story that produced it. This is not a style preference: `CLAUDE.md`
and `docs/arch/` are paid on every session, so a paragraph about deleted code is pure cost
that also reads as if it were still true.

**The one test — does this past fact change what someone does today?**

- **Yes → it stays**, rewritten present-tense with the narrative stripped. Real examples:
  *"runs stored before 2026-07-28 were priced at 100 bps — they do not compare to a new
  run"* (that data is still on disk), *"grouped runs before 2026-07-26 carry poisoned
  aggregates — re-run them"*. Keep the date **only** because it is the cutoff someone has
  to check against, never as a timeline.
- **No → it moves to `docs/history/`, or is deleted.** "We used to do Y", "X was removed
  in Phase 7", "fixed on <date> after N hours of outage" — none of that changes an action.

### What that forbids, concretely

| Never write in `CLAUDE.md`, `docs/arch/`, or a code comment | Write instead |
| --- | --- |
| "X was deleted / retired / no longer exists" | describe what *does* exist; if X's absence is load-bearing, say "there is no X" and why re-adding it breaks something |
| An outage narrative — dates, durations, row counts, SOL figures | the invariant it produced, plus the measurement if the *number* is the rule (a threshold, a cost, a speedup) |
| "Phase N" / "step 2" labels from a plan doc | what the code does; a phase number outlives the plan and then points nowhere |
| A pointer to a `.md` that no longer exists | the doc that absorbed it, or nothing |
| "NOTE: the paths below are stale" | fix the paths and delete the note — a doc that warns it is wrong is worse than one that is right |
| A second copy of a tracked doc (e.g. under `_local/`) | one copy; two copies drift and each ends up holding what the other lost |

### Code comments — same rule, one exception

A comment whose job is to stop a future "simplification" from reintroducing a bug **keeps
its cautionary form** — ``// `?` here used to strand a wallet's SOL when a token-account
close failed`` earns its past tense, because deleting it invites the bug back. That is a
regression guard, not history. Everything else is present-tense: no phase labels, no
"previously", no describing code that was removed.

## `docs/history/` — the escape valve

One file per entry, `<YYYY-MM-DD-slug>.md`, shape: **Symptom** (with the numbers) →
**Cause** (the mechanism) → **Fix** → **The rule this produced** (one line + a link to
where that rule now lives). A `README.md` indexes each history dir.

- **Never linked** from a `CLAUDE.md` or an `arch/` link table. It is a grep target, so it
  costs nothing per session. An `arch/` doc may carry at most one inline link to a history
  entry, where a reader would otherwise ask "why is this odd rule here?".
- **The bar: the past left a live consequence.** Stored data is now wrong, a rule looks
  arbitrary without the story, or a whole approach was refuted (so nobody re-runs it). An
  ordinary bug fix gets a code comment and **nothing else** — history that grows
  per-commit is the same bloat in a new place.
- Every extraction is a **move**: delete from the source and write to history in the same
  change, so one diff shows both sides. Nothing is lost, ever.

## The gate — `scripts/check-docs.sh`

Two blocking checks:

1. **Present tense** on the tiers paid every session (`CLAUDE.md`, `docs/arch/`).
2. **Every cited path resolves** — `.md` from any doc or code comment, `.rs`/`.ts`/`.tsx`
   from `CLAUDE.md` / `docs/arch/` / `docs/plans/`.

Deliberate exceptions are marked on the line: `pt-ok: <reason>` for a date that is a real
cutoff, `ref-ok: <reason>` for a file named because its **absence** is the rule. An
unchecked `- [ ]` is exempt unmarked, since a proposal names what nobody has written yet.
A whole region can be exempted between `pt-ok:begin` and `pt-ok:end`, only for prose that
quotes the forbidden phrasing in order to define the rule.

`docs/history/` and `docs/roadmap/` may **name** a source file that is gone or unwritten —
the past and the not-yet-written both have to. A markdown **link** resolves in every tier,
including those two: a citation that goes nowhere is a dead end wherever it sits.

`.github/workflows/docs.yml` runs the whole-tree sweep on every push and PR:

```powershell
sh scripts/check-docs.sh --all        # whole tree — the same command CI runs
git config core.hooksPath .githooks   # optional, once per clone: also gate staged files
```

`core.hooksPath` lives in the untracked `.git/config`, so a fresh clone has **no** local
hook until that one-liner runs, and `--no-verify` skips it — CI depends on neither.
<!-- pt-ok:end -->
