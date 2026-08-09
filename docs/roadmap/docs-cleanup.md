# Docs cleanup — handoff

Strip accumulated history/status narrative out of the present-tense doc tiers and code
comments, keeping only what describes the system as it is now. The rule this work serves
is locked in [root CLAUDE.md](../../CLAUDE.md) → *"Present tense only"*; read that first, it is
the spec.

Everything below is **measured against the tree**, not estimated. Re-measure before
starting — the commands are given.

---

## 0. READ THIS FIRST — the tree is in a trap state

**Every new file from this work is untracked, and one deletion is unstaged.** A
`git commit -am` right now silently drops the entire history tier, the gate script, and
the hook, while committing a deletion whose replacement does not exist.

```
?? scripts/check-docs.sh          ?? .githooks/pre-commit
?? docs/refactor-plan.md          ?? docs/history/            (2 files)
?? forge/docs/history/            (2 files)
?? hunter/docs/history/           (12 files)
?? hunter/docs/roadmap/ingest-watchdog-kill-recovery.md
 D docs/refactor-audit-2026-07-10.md   <- renamed to docs/refactor-plan.md, NOT staged
```

The rename was staged earlier and got unstaged by a `git restore --staged` during a hook
self-test. Fix before anything else:

```powershell
git add -A scripts/check-docs.sh .githooks docs forge/docs/history hunter/docs
git status --porcelain            # expect the D+?? pair above to become one R
```

`git ls-files` still lists the old `refactor-audit-*` path until this is staged, so any
script that iterates tracked files will try to read a file that is gone.

The tree also carries ~117 modified files: this cleanup **plus** pre-existing uncommitted
work (migration squash, PnL-% fix). Nothing has been committed. Separate the two before
writing a commit message.

---

## 1. What is already done — do not redo

- **The rule** is written and locked in root `CLAUDE.md` (the `pt-ok:begin/end` block is
  deliberate: that section quotes the phrasing it forbids).
- **`docs/history/` exists** in three places (root, `hunter/`, `forge/`) with README
  indexes. 12 hunter entries, incl. the merged `wallet-research-2026-07.md` journal.
- **Swept clean:** both `CLAUDE.md`s, all of `docs/arch/**` (both products),
  `db-patterns.md`, `token-history-chart-functionalities.md`, `wallet-analysis.md`
  (rewritten 1220 → 219 lines as a conclusions reference), `docs/refactor-plan.md`,
  `forge/docs/roadmap-plan.md`.
- **The gate** — `scripts/check-docs.sh` + `.githooks/pre-commit`. Green on `--all`.

Do not re-sweep those files. Spend the effort on §2.

---

## 2. Not finished

### 2a. Code comments — 213 lines across 90 files (biggest remaining chunk)

Never swept systematically. The gate does not check them at all.

```sh
pt_re='no longer|used to|previously|formerly|retired in Phase|deleted in Phase|(was|were)( [a-z]+)? (deleted|retired|renamed|removed|dropped)'
for f in $(git ls-files '*.rs' '*.ts' '*.tsx' | grep -vE 'node_modules|/dist/'); do
  n=$(grep -EI '^\s*(//|/\*|\*)' "$f" | grep -EIc "$pt_re")
  [ "$n" -gt 0 ] && printf '%5d  %s\n' "$n" "$f"
done | sort -rn
```

Worst files: `hunter/live/src/strategies/engine/sinks.rs` (6),
`hunter/core/src/api/handlers/tokens/tokens.rs` (5),
`hunter/frontend/src/shared/lib/storage.ts` (4), `hunter/lab/src/sweep/grouped_engine.rs`
(3), `hunter/core/src/storage/repositories/token_info_repo.rs` (3).

**The exception matters here.** A comment whose job is to stop a future "simplification"
from reintroducing a bug **keeps its past tense** — that is a regression guard, not
history. Judge each hit; this is not a find-and-replace. Known example to keep:
the `?`-strands-SOL-on-token-account-close guard.

Known non-guard to fix: `exec_real.rs:42` — *"mirrors the old 12 × 1 s window"*.

**Verify after:** `cargo check` on the four bins + core, and prove FE edits are
comment-only:
```sh
git diff -- '*.ts' '*.tsx' | grep -E "^[+-]" | grep -v "^[+-][+-]" | grep -vE "^[+-]\s*(//|\*|/\*)"
```
(empty = no logic touched)

### 2b. Unguarded doc tiers — 49 lines across 27 files

The gate only guards `CLAUDE.md` + `docs/arch/**`. `docs/plans/**`, `docs/roadmap/**`,
and the READMEs are unchecked, and real staleness was found in both this session.

```sh
for f in $(git ls-files '*.md' | grep -vE 'docs/history/|docs/arch/|CLAUDE\.md$'); do
  n=$(sed -E 's/\[[^]]*\]\([^)]*\)/[link]/g' "$f" | grep -EIc "$pt_re")
  [ "$n" -gt 0 ] && printf '%5d  %s\n' "$n" "$f"
done | sort -rn
```

Worst: `hunter/docs/plans/sweep/sweep-engine-detail.md` (5), `sweep/ram-sizing.md` (4),
`strategies/pnl-percent-definition.md` (4), `ingest/laserstream-workflow.md` (4),
`forge/docs/roadmap-plan.md` (3).

`docs/plans/` is the **deep-dive reference** tier — a design rationale that explains *why*
a constant is what it is legitimately cites the measurement that set it. Keep the number,
cut the story. Decide per hit whether to widen the gate to this tier afterward.

### 2c. Gate gaps (each one cost real time this session)

- **Code-file references are unvalidated.** Check 2 resolves cited `.md` paths only. Two
  dead `.rs` pointers sat in the top-paid tier and were found by hand:
  `hunter/CLAUDE.md` told every session to read `runtime_cache.rs`, a file that does not
  exist. Extend check 2 to `.rs`/`.ts`/`.tsx` paths cited from docs.
- **`core.hooksPath` is local config, not tracked.** It does not travel with a clone and
  is not wired into CI. Either add a bootstrap step or call `scripts/check-docs.sh`
  directly from CI.
- Guarded tier is narrow — see 2b before widening; a noisy gate gets disabled.

### 2d. `hunter/_local/rule-research/` — 8 docs, ~1700 lines, never triaged

Gitignored, so no duplicate-of-tracked-doc problem, and content is unique (not copies).
Untouched by this work. Each needs a call: promote to `docs/history/`, fold a conclusion
into `docs/plans/strategies/`, or leave local deliberately. Only decide with the user —
this is research judgment, not cleanup.

### 2e. Doc-sweep items still open in `docs/refactor-plan.md`

These are cleanup, not engineering, so they belong to this task:

- `cargo run -p lab` / `-p live` **do not work** — `lab`/`live` are `[lib]` target names;
  the packages are `hunter-lab`/`hunter-live`. Wrong in `hunter/docs/RUN-MODES.md:39-42`,
  `hunter/docs/arch/sweep.md:269`, `hunter/docs/plans/database/lake-pg-read-paths.md:35`,
  `hunter/docs/plans/deploy/api-auth-deploy-flow.md:103`,
  `forge/docs/roadmap-plan.md:93,100`, and
  `forge/frontend/src/features/launch/LaunchConsolePage.tsx:186` (user-visible UI text).
- `frontend-react` → `frontend` in comments at
  `hunter/core/src/api/handlers/tokens/tokens.rs:514,1374`.
- `../hunter/pump-trader::bundle_buy` → `shared/executor/pumpfun` at
  `forge/docs/roadmap-plan.md:84`.

**Do not** "fix" `pump-trader` / `ingest-laserstream` elsewhere — those are the current,
intentional Cargo dep keys, and the item was already refuted once as a false premise. See
[venue-quote-portability.md](../../hunter/docs/roadmap/venue-quote-portability.md).

---

## 3. Definition of done

1. `sh scripts/check-docs.sh --all` → exit 0.
2. The two sweeps in 2a/2b return only hits deliberately kept (regression guards, and
   numbers that are themselves the rule).
3. `cargo check` clean on `hunter-live`, `hunter-lab`, `forge-live`, `forge-lab`,
   `hunter-core`; no new warnings. Use
   `--target-dir "C:/Users/User/Documents/Bot/target-check"` if a bin is running.
4. FE diff proven comment-only (command in 2a).
5. No BOMs, UTF-8, CRLF to match the tree (`autocrlf=true`).
6. §0 staging resolved — the history tier and the gate are actually tracked.
