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

set -u
# Every list below is newline-separated (git ls-files, grep -o); splitting on spaces too
# would break the first path that contains one.
IFS='
'
repo=$(git rev-parse --show-toplevel) || exit 1
cd "$repo" || exit 1

mode="${1:-}"
fail=0

# Doc basenames that legitimately live OUTSIDE this repo (upstream/vendor docs).
external_docs="BREAKING_FEE_RECIPIENT.md"

# Every tracked path, once — the file list in --all mode, and what 2b/2c resolve a
# citation's basename against in both modes.
tracked=$(git ls-files)

# The basenames of those paths, newline-padded at both ends so a `case` glob can test
# membership with no subshell instead of a grep per cited path. Same semantics; the run
# time is dominated by the per-file awk/grep/sed pipelines, not by this.
LF='
'
tracked_bases="$LF$(printf '%s\n' "$tracked" | sed 's|.*/||' | sort -u)$LF"

if [ "$mode" = "--all" ]; then
    files="$tracked"
else
    files=$(git diff --cached --name-only --diff-filter=ACM)
fi
[ -n "$files" ] || exit 0

is_guarded_tier() {
    case "$1" in
        */docs/history/*|docs/history/*) return 1 ;;
        CLAUDE.md|*/CLAUDE.md)           return 0 ;;
        docs/arch/*.md|*/docs/arch/*.md) return 0 ;;
        *)                               return 1 ;;
    esac
}

# ── 1. present tense ─────────────────────────────────────────────────────────
# `\b` on every phrase: without it "focused token" matches "used to".
pt_re='[0-9]{4}-[0-9]{2}-[0-9]{2}|\bno longer\b|\bused to\b|\bpreviously\b|\bformerly\b|\bretired in Phase\b|\bdeleted in Phase\b|\b(was|were)( [a-z]+)? (deleted|retired|renamed|removed|dropped)\b'

for f in $files; do
    [ -f "$f" ] || continue
    is_guarded_tier "$f" || continue
    # Drop pt-ok:begin/end regions, blank out link targets (a path/anchor is not prose),
    # keep line numbers, then match.
    hits=$(awk '
            /pt-ok:begin/ { skip = 1 }
            { if (skip) print NR ":"; else print NR ":" $0 }
            /pt-ok:end/   { skip = 0 }
        ' "$f" 2>/dev/null \
        | sed -E 's/\[[^]]*\]\([^)]*\)/[link]/g; s/\]\([^)]*\)/](…)/g' \
        | grep -EI "$pt_re" \
        | grep -v 'pt-ok')
    if [ -n "$hits" ]; then
        if [ "$fail" -eq 0 ]; then
            printf '\n\033[31mPRESENT-TENSE RULE\033[0m  (root CLAUDE.md, "Present tense only")\n'
            printf 'These tiers are paid on every session — write the rule, not the story.\n'
            printf 'Move it to docs/history/, or mark the line `pt-ok: <one-line reason>`.\n\n'
        fi
        fail=1
        printf '%s\n' "$hits" | sed "s|^|  $f:|"
    fi
done

# ── 2. cited files resolve ───────────────────────────────────────────────────
bad_refs=""

# The two lines a citation check never fires on, in EITHER half of check 2: a line marked
# `ref-ok` (the file is named because its absence is the rule) and an unchecked `- [ ]`
# (a proposal is allowed to name what nobody has written yet, wherever it lives).
citation_prose='ref-ok|^[[:space:]]*[-*][[:space:]]+\[ \]'

# 2a. markdown link targets, resolved relative to the linking file
for f in $files; do
    case "$f" in *.md) ;; *) continue ;; esac
    [ -f "$f" ] || continue
    dir=$(dirname "$f")
    for target in $(grep -vE "$citation_prose" "$f" 2>/dev/null \
                    | grep -ohE '\]\([^)]+\.md(#[^)]*)?\)' \
                    | sed -E 's/^\]\(//; s/\)$//; s/#.*$//'); do
        case "$target" in http*|@*) continue ;; esac
        [ -f "$dir/$target" ] || bad_refs="$bad_refs\n  $f -> $target"
    done
done

# 2b. .md paths cited from code comments, resolved by basename anywhere in the repo
for f in $files; do
    case "$f" in
        *.rs|*.ts|*.tsx) ;;
        *) continue ;;
    esac
    case "$f" in */node_modules/*|*/dist/*|*/target*) continue ;; esac
    [ -f "$f" ] || continue
    for target in $(grep -vE "$citation_prose" "$f" 2>/dev/null \
                    | grep -ohE '[A-Za-z0-9_@./-]+\.md' | sort -u); do
        case "$target" in *//*) continue ;; esac          # URLs
        base=$(basename "$target")
        case " $external_docs " in *" $base "*) continue ;; esac
        case "$tracked_bases" in
            *"$LF$base$LF"*) ;;
            *)               bad_refs="$bad_refs\n  $f -> $target" ;;
        esac
    done
done

# 2c. source files cited from a doc, resolved by basename anywhere in the repo.
#     "read `runtime_cache.rs`" in a top-paid tier is a session sent to a file that
#     does not exist — the same failure as a dead `.md` pointer, and until this ran
#     it was only ever caught by hand. Basename-only on purpose: a doc that names a
#     file is pointing at it, not asserting its directory.
#
#     Only the tiers that describe what EXISTS are checked — `CLAUDE.md`, `docs/arch/`,
#     `docs/plans/`. Two exemptions, both because naming a file that isn't there is
#     correct in them: `docs/history/` (the past) and `docs/roadmap/` (proposals — a
#     roadmap that couldn't name the file it wants written would be useless).
#
#     Per-line exemptions are `$citation_prose`, shared with 2a/2b.
is_existence_tier() {
    case "$1" in
        */docs/history/*|docs/history/*|*/docs/roadmap/*|docs/roadmap/*) return 1 ;;
        CLAUDE.md|*/CLAUDE.md)                                          return 0 ;;
        docs/arch/*.md|*/docs/arch/*.md)                                return 0 ;;
        docs/plans/*.md|*/docs/plans/*.md)                              return 0 ;;
        *)                                                              return 1 ;;
    esac
}

for f in $files; do
    case "$f" in *.md) ;; *) continue ;; esac
    is_existence_tier "$f" || continue
    [ -f "$f" ] || continue
    for target in $(grep -vE "$citation_prose" "$f" 2>/dev/null \
                    | grep -ohE '[A-Za-z0-9_@./-]+\.(rs|ts|tsx)' | sort -u); do
        case "$target" in
            *//*|*'*'*) continue ;;                       # URLs, globs
            *.d.ts)     continue ;;                       # ambient type decls
        esac
        base=$(basename "$target")
        case "$tracked_bases" in
            *"$LF$base$LF"*) ;;
            *)               bad_refs="$bad_refs\n  $f -> $target" ;;
        esac
    done
done

if [ -n "$bad_refs" ]; then
    if [ "$fail" -eq 0 ]; then printf '\n'; fi
    printf '\033[31mDANGLING DOC REFERENCE\033[0m\n'
    printf 'A pointer to a deleted doc reads as authoritative and goes nowhere.\n'
    printf 'Repoint it at the doc that absorbed it, or drop the citation.\n'
    # shellcheck disable=SC2059
    printf "$bad_refs\n"
    fail=1
fi

if [ "$fail" -ne 0 ]; then
    printf '\nBlocked. Fix the above, or re-run with the reasons marked.\n\n'
    exit 1
fi
exit 0
