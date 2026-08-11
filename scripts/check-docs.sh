#!/bin/sh
# Docs gate for the "Present tense only" rule (root CLAUDE.md).
#
#   scripts/check-docs.sh          # staged files only (what the pre-commit hook runs)
#   scripts/check-docs.sh --all    # every tracked file — what CI runs, and the real gate
#
# Two checks, both blocking:
#
#   1. PRESENT TENSE — the tiers paid on every session (`CLAUDE.md`, `docs/arch/`) must
#      describe what the system does now. Dates and "no longer / used to / retired in
#      Phase / was deleted" are the tells.
#
#      A deliberate exception is marked ON THE LINE with `pt-ok`, e.g.
#          <!-- pt-ok: cutoff, that data is still on disk -->
#      Use it for a date that is a cutoff someone must check against, never for a
#      timeline. If you cannot write a one-line reason, it belongs in `docs/history/`.
#      A whole region can be exempted between `pt-ok:begin` and `pt-ok:end` — only for
#      prose that quotes the forbidden phrasing in order to define the rule.
#
#      Link TARGETS are not prose: `](…)` is stripped before matching, so a link to a
#      dated history entry or a `#measured-…-2026-07-19` anchor never trips the check.
#
#   2. DOC REFERENCES RESOLVE — every `.md` path cited from a doc or a code comment,
#      and every `.rs`/`.ts`/`.tsx` path cited from a doc, must exist. A pointer to a
#      deleted plan reads as authoritative and goes nowhere; a session told to read a
#      source file that is gone burns a whole context re-deriving why.
#
#      A doc that names a missing file DELIBERATELY — the root CLAUDE.md's
#      "there is no X, and here is why re-adding it breaks something" form — marks the
#      line `ref-ok`. Same shape as `pt-ok`, same bar: if you cannot write the reason,
#      repoint the citation instead. An unchecked `- [ ]` line is exempt unmarked: a
#      proposal names the file it wants written, which is why it is a proposal.
#
# `docs/history/` is exempt from checks 1 and 2c by design: it is where the past lives,
# and an entry about a removed module has to be able to name it. It is NOT exempt from
# 2a — naming a file that is gone is the point of the tier, but a markdown LINK that
# goes nowhere is a dead end in every tier, so links resolve everywhere.
#
# ── Cost ─────────────────────────────────────────────────────────────────────────────
# Each check is ONE `git grep` over its scope piped into ONE `awk`. Nothing is per-file,
# and nothing calls `dirname`/`basename` in a loop — `${f%/*}` and `sub(/^.*\//)` do that
# with no process. A process spawn costs ~70ms on Windows, so a per-file pipeline over the
# ~1000 tracked files runs for minutes where this runs for seconds. Keep any new check in
# that shape: grep once, filter in awk.

set -u
# `-f` because the pathspec lists below are passed to git UNQUOTED, to word-split them.
# Without it the shell expands `*.md` against the CWD first and git only ever sees the
# root-level matches — the checks then pass by scanning almost nothing. A pathspec must
# reach git verbatim; only git may interpret its wildcards.
set -f
# Every list below is newline-separated (git ls-files, git diff --name-only); splitting
# on spaces too would break the first path that contains one.
IFS='
'
repo=$(git rev-parse --show-toplevel) || exit 1
cd "$repo" || exit 1

mode="${1:-}"
fail=0

# Doc basenames that legitimately live OUTSIDE this repo (upstream/vendor docs).
external_docs="BREAKING_FEE_RECIPIENT.md"

# Named in full rather than cleaned up with a `$tmp.*` glob: `set -f` above would leave
# that pattern unexpanded and the files would leak.
tmp_regions="${TMPDIR:-/tmp}/check-docs.$$.regions"
tmp_bases="${TMPDIR:-/tmp}/check-docs.$$.bases"
trap 'rm -f "$tmp_regions" "$tmp_bases"' EXIT INT HUP TERM

# What each check greps. In --all mode a pathspec narrows the scan to the tier that check
# owns; the awk still enforces the tier exactly, the pathspec only keeps the whole tree
# from being piped through it. In staged mode all three are the staged list, and the awk
# tier filter is what narrows.
if [ "$mode" = "--all" ]; then
    scope_pt='CLAUDE.md
*/CLAUDE.md
*docs/arch/*.md'
    scope_md='*.md'
    scope_src='*.rs
*.ts
*.tsx'
else
    staged=$(git diff --cached --name-only --diff-filter=ACM)
    [ -n "$staged" ] || exit 0
    scope_pt="$staged"
    scope_md="$staged"
    scope_src="$staged"
fi

# git grep prints `path:line:text`. Anchoring a line-start pattern therefore has to step
# over that prefix — `^` alone would never match the start of the prose.
prefix='^[^:]*:[0-9]+:'

# The two lines a citation check never fires on, in EITHER half of check 2: a line marked
# `ref-ok` (the file is named because its absence is the rule) and an unchecked `- [ ]`
# (a proposal is allowed to name what nobody has written yet, wherever it lives).
citation_prose="ref-ok|${prefix}[[:space:]]*[-*][[:space:]]+\[ \]"

# Prepended to every awk program below so each one can stay single-quoted: `parse()` fills
# the globals f / ln / s from git grep's `path:line:text`. Splitting on the first two
# colons rather than `-F:` keeps a colon in the prose intact.
awk_parse='
function parse() {
    n = index($0, ":");     f    = substr($0, 1, n - 1)
    rest = substr($0, n + 1)
    m = index(rest, ":");   ln   = substr(rest, 1, m - 1) + 0
                            s    = substr(rest, m + 1)
}
'

# ── 1. present tense ─────────────────────────────────────────────────────────
# `\b` on every phrase: without it "focused token" matches "used to".
pt_re='[0-9]{4}-[0-9]{2}-[0-9]{2}|\bno longer\b|\bused to\b|\bpreviously\b|\bformerly\b|\bretired in Phase\b|\bdeleted in Phase\b|\b(was|were)( [a-z]+)? (deleted|retired|renamed|removed|dropped)\b'

# The `pt-ok:begin`/`:end` line numbers, so the region filter below can drop a hit that
# falls inside one. Both markers sit inside the region they open and close.
git grep -nIE --color=never -e 'pt-ok:(begin|end)' -- $scope_pt > "$tmp_regions" 2>/dev/null

# git grep is only a prefilter here: it matches the RAW line, so a hit whose sole match
# sits in a link target survives it. Blanking the links and re-matching settles that, and
# re-using the same `grep -E` keeps ONE regex dialect for the check — awk's ERE has no
# `\b`, so the phrase list cannot move into awk.
hits=$(git grep -nIE --color=never -e "$pt_re" -- $scope_pt 2>/dev/null \
    | awk "$awk_parse"'
        { parse() }
        f !~ /docs\/history\// && (f ~ /(^|\/)CLAUDE\.md$/ || f ~ /(^|\/)docs\/arch\/.*\.md$/)' \
    | sed -E 's/\[[^]]*\]\([^)]*\)/[link]/g; s/\]\([^)]*\)/](…)/g' \
    | grep -E "$pt_re" \
    | grep -v 'pt-ok' \
    | awk -v rf="$tmp_regions" "$awk_parse"'
        BEGIN {
            while ((getline line < rf) > 0) {
                i = index(line, ":"); rfile = substr(line, 1, i - 1)
                tail = substr(line, i + 1)
                j = index(tail, ":"); rline = substr(tail, 1, j - 1) + 0
                if (substr(tail, j + 1) ~ /pt-ok:begin/) {
                    if (!(rfile in open)) open[rfile] = rline
                } else if (rfile in open) {
                    k = ++cnt[rfile]
                    lo[rfile, k] = open[rfile]; hi[rfile, k] = rline
                    delete open[rfile]
                }
            }
            # An unterminated `pt-ok:begin` exempts to end of file, as the region does.
            for (rfile in open) { k = ++cnt[rfile]; lo[rfile, k] = open[rfile]; hi[rfile, k] = 1e18 }
        }
        {
            parse()
            for (k = 1; k <= cnt[f]; k++) if (ln >= lo[f, k] && ln <= hi[f, k]) next
            print
        }')

if [ -n "$hits" ]; then
    printf '\n\033[31mPRESENT-TENSE RULE\033[0m  (root CLAUDE.md, "Present tense only")\n'
    printf 'These tiers are paid on every session — write the rule, not the story.\n'
    printf 'Move it to docs/history/, or mark the line `pt-ok: <one-line reason>`.\n\n'
    fail=1
    printf '%s\n' "$hits" | sed 's|^|  |'
fi

# ── 2. cited files resolve ───────────────────────────────────────────────────
bad_refs=""
LF='
'
# `$(…)` strips trailing newlines, so the three groups have to be re-joined by one or
# 2a's last finding and 2b's first end up on the same line.
add_refs() {
    [ -n "$1" ] || return 0
    bad_refs="${bad_refs:+$bad_refs$LF}$1"
}

# The tracked basenames, once. 2b/2c resolve a citation by basename against this, on
# purpose: a doc that names a file is pointing at it, not asserting its directory.
git ls-files | sed 's|.*/||' | sort -u > "$tmp_bases"

# 2a. markdown link targets, resolved relative to the linking file. The one check that has
#     to touch the filesystem — a link may point at a tracked file, a generated one, or up
#     through `../` — so awk emits `path<TAB>target` pairs and the shell tests them with
#     the `[ -f ]` builtin, no process per target.
bad_a=$(git grep -nIE --color=never -e '\]\([^)]+\.md' -- $scope_md 2>/dev/null \
    | awk "$awk_parse"'{ parse() } f ~ /\.md$/' \
    | grep -vE "$citation_prose" \
    | awk "$awk_parse"'
        {
            parse()
            while (match(s, /\]\([^)]+\.md(#[^)]*)?\)/)) {
                t = substr(s, RSTART + 2, RLENGTH - 3)      # inside the "](" … ")"
                s = substr(s, RSTART + RLENGTH)
                sub(/#.*$/, "", t)
                if (t ~ /^http/ || t ~ /^@/) continue
                print f "\t" t
            }
        }' \
    | sort -u \
    | while IFS='	' read -r f target; do
          dir="${f%/*}"
          [ "$dir" = "$f" ] && dir='.'
          [ -f "$dir/$target" ] || printf '  %s -> %s\n' "$f" "$target"
      done)
add_refs "$bad_a"

# 2b. .md paths cited from code comments, resolved by basename anywhere in the repo.
bad_b=$(git grep -nIE --color=never -e '[A-Za-z0-9_@./-]+\.md' -- $scope_src 2>/dev/null \
    | awk "$awk_parse"'
        { parse() }
        f ~ /\.(rs|ts|tsx)$/ && f !~ /(^|\/)(node_modules|dist)\// && f !~ /(^|\/)target/' \
    | grep -vE "$citation_prose" \
    | awk -v bf="$tmp_bases" -v ext="$external_docs" "$awk_parse"'
        BEGIN {
            while ((getline b < bf) > 0) bases[b]
            nx = split(ext, e, " "); for (i = 1; i <= nx; i++) skip[e[i]]
        }
        {
            parse()
            while (match(s, /[A-Za-z0-9_@.\/-]+\.md/)) {
                t = substr(s, RSTART, RLENGTH); s = substr(s, RSTART + RLENGTH)
                if (t ~ /\/\//) continue                    # URLs
                b = t; sub(/^.*\//, "", b)
                if (b in skip || b in bases) continue
                print "  " f " -> " t
            }
        }' \
    | sort -u)
add_refs "$bad_b"

# 2c. source files cited from a doc, resolved by basename anywhere in the repo.
#     "read `runtime_cache.rs`" in a top-paid tier is a session sent to a file that does
#     not exist — the same failure as a dead `.md` pointer.
#
#     Only the tiers that describe what EXISTS are checked — `CLAUDE.md`, `docs/arch/`,
#     `docs/plans/`. Two exemptions, both because naming a file that isn't there is
#     correct in them: `docs/history/` (the past) and `docs/roadmap/` (proposals — a
#     roadmap that couldn't name the file it wants written would be useless).
#
#     Per-line exemptions are `$citation_prose`, shared with 2a/2b.
bad_c=$(git grep -nIE --color=never -e '[A-Za-z0-9_@./-]+\.(rs|ts|tsx)' -- $scope_md 2>/dev/null \
    | awk "$awk_parse"'
        { parse() }
        f ~ /\.md$/ && f !~ /docs\/(history|roadmap)\// &&
        (f ~ /(^|\/)CLAUDE\.md$/ || f ~ /(^|\/)docs\/(arch|plans)\/.*\.md$/)' \
    | grep -vE "$citation_prose" \
    | awk -v bf="$tmp_bases" "$awk_parse"'
        BEGIN { while ((getline b < bf) > 0) bases[b] }
        {
            parse()
            while (match(s, /[A-Za-z0-9_@.\/-]+\.(rs|ts|tsx)/)) {
                t = substr(s, RSTART, RLENGTH); s = substr(s, RSTART + RLENGTH)
                if (t ~ /\/\// || t ~ /\*/) continue         # URLs, globs
                if (t ~ /\.d\.ts$/) continue                # ambient type decls
                b = t; sub(/^.*\//, "", b)
                if (b in bases) continue
                print "  " f " -> " t
            }
        }' \
    | sort -u)
add_refs "$bad_c"

if [ -n "$bad_refs" ]; then
    if [ "$fail" -eq 0 ]; then printf '\n'; fi
    printf '\033[31mDANGLING DOC REFERENCE\033[0m\n'
    printf 'A pointer to a deleted doc reads as authoritative and goes nowhere.\n'
    printf 'Repoint it at the doc that absorbed it, or drop the citation.\n'
    printf '%s\n' "$bad_refs"
    fail=1
fi

if [ "$fail" -ne 0 ]; then
    printf '\nBlocked. Fix the above, or re-run with the reasons marked.\n\n'
    exit 1
fi
exit 0
