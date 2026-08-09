#!/bin/sh
# Docs gate for the "Present tense only" rule (root CLAUDE.md).
#
#   scripts/check-docs.sh          # staged files only (what the pre-commit hook runs)
#   scripts/check-docs.sh --all    # every tracked file (use after a big refactor / in CI)
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
#   2. DOC REFERENCES RESOLVE — every `.md` path cited from a doc or a code comment must
#      exist. A pointer to a deleted plan reads as authoritative and goes nowhere.
#
# `docs/history/` is exempt from check 1 by design: it is where the past lives.

set -u
repo=$(git rev-parse --show-toplevel) || exit 1
cd "$repo" || exit 1

mode="${1:-}"
fail=0

# Doc basenames that legitimately live OUTSIDE this repo (upstream/vendor docs).
external_docs="BREAKING_FEE_RECIPIENT.md"

if [ "$mode" = "--all" ]; then
    files=$(git ls-files)
else
    files=$(git diff --cached --name-only --diff-filter=ACM)
fi
[ -n "$files" ] || exit 0

is_guarded_tier() {
    case "$1" in
        */docs/history/*|docs/history/*) return 1 ;;
        CLAUDE.md|*/CLAUDE.md)           return 0 ;;
        */docs/arch/*.md)                return 0 ;;
        *)                               return 1 ;;
    esac
}

# ── 1. present tense ─────────────────────────────────────────────────────────
pt_re='[0-9]{4}-[0-9]{2}-[0-9]{2}|no longer|used to|previously|formerly|retired in Phase|deleted in Phase|(was|were)( [a-z]+)? (deleted|retired|renamed|removed|dropped)'

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

# ── 2. cited .md files resolve ───────────────────────────────────────────────
bad_refs=""

# 2a. markdown link targets, resolved relative to the linking file
for f in $files; do
    case "$f" in *.md) ;; *) continue ;; esac
    [ -f "$f" ] || continue
    dir=$(dirname "$f")
    for target in $(grep -ohE '\]\([^)]+\.md(#[^)]*)?\)' "$f" 2>/dev/null \
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
    for target in $(grep -ohE '[A-Za-z0-9_@./-]+\.md' "$f" 2>/dev/null | sort -u); do
        case "$target" in *//*) continue ;; esac          # URLs
        base=$(basename "$target")
        case " $external_docs " in *" $base "*) continue ;; esac
        git ls-files --error-unmatch "*/$base" >/dev/null 2>&1 && continue
        git ls-files | grep -q "/$base\$\|^$base\$" || bad_refs="$bad_refs\n  $f -> $target"
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
