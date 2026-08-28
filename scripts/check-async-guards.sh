#!/bin/sh
# Fails when a DashMap/DashSet shard guard is still alive at an `.await`.
#
# Why this is a gate and not a review note: dashmap 4.0.2's shard lock is an
# UNBOUNDED SPINLOCK -- `dashmap::lock::RwLock::write` is
# `loop { try_write() else cpu_relax() }`, which never parks and never yields.
# A guard held across `.await` lets the worker thread pick up another task, hit
# the same shard, and spin at 100% CPU inside a non-async loop; it can then never
# return to the scheduler to poll the future that would release the guard. On the
# 2-worker deploy runtime that wedges every task in the process until the ingest
# watchdog force-exits it ~90 s later. It costs a ~105 s hole in `trades` that
# nothing replays, and it is invisible in a review diff.
#
# The rule: copy what you need out of the map, drop the guard, then `.await`.
# `hunter/core/src/state/token_cache.rs` (`DeadFlush`) is the reference shape.
#
#     sh scripts/check-async-guards.sh
#
# Exit 1 on any finding. No arguments; always sweeps the whole tree (it is fast).
#
# Scope, stated honestly. It flags a binding whose value IS the guard --
# `let g = map.get(k) {`, `if let Some(g) = map.get(k) {`, `for e in map.iter() {`
# -- and then any `.await` before that block closes. It deliberately does NOT
# flag a chain that consumes the guard into a plain value (`.map(..)`,
# `.cloned()`, `.unwrap_or(..)`), because those release it at the end of the
# statement; that is the shape the fix uses and the shape review should prefer.
# Receiver names are derived from the tree, so a new DashMap field is covered the
# day it is added -- at the cost that a same-named non-DashMap binding elsewhere
# can be flagged. Copy the value out and both the warning and the hazard go away.

set -eu

cd "$(dirname "$0")/.."

# Names are derived per PRODUCT, not tree-wide: `hunter/` and `forge/` share only
# `shared/`, so a `mints: DashMap` in hunter must not make forge's
# `mints: &HashSet` look like a guard. Each pass scans with its own vocabulary.

# --- 1. Every identifier a product declares as a DashMap/DashSet ------------
derive_names() {
  DIRS="$1"
  {
    # `field: DashMap<..>` / `field: Arc<DashSet<..>>` / `static X: LazyLock<DashMap<..`
    grep -rhoE '[A-Za-z_][A-Za-z0-9_]*[[:space:]]*:[[:space:]]*(Arc<|LazyLock<)*Dash(Map|Set)<' \
      --include=*.rs $DIRS 2>/dev/null | sed -E 's/[[:space:]]*:.*//'
    # Fields typed through a `pub type Alias = DashMap<..>`.
    for alias in $(grep -rhoE 'type[[:space:]]+[A-Za-z_][A-Za-z0-9_]*[[:space:]]*=[[:space:]]*Dash(Map|Set)' \
                     --include=*.rs $DIRS 2>/dev/null | awk '{print $2}' | sort -u); do
      grep -rhoE "[A-Za-z_][A-Za-z0-9_]*[[:space:]]*:[[:space:]]*(Arc<)*${alias}\\b" \
        --include=*.rs $DIRS 2>/dev/null | sed -E 's/[[:space:]]*:.*//'
    done
  } | sort -u | grep -vE '^(self|_)$' | tr '\n' '|' | sed 's/|$//'
}

# --- 2. Scope-aware scan -----------------------------------------------------
scan() {
  NAMES="$1"; DIRS="$2"
  find $DIRS -name '*.rs' -not -path '*/target/*' -print0 |
  xargs -0 awk -v NAMES="$NAMES" '
    FNR == 1 { stmt = ""; depth = 0; open = 0 }

    {
      code = $0; sub(/\/\/.*/, "", code)

      # Accumulate a logical statement so a chain split over lines is judged whole.
      stmt = (stmt == "" ? code : stmt " " code)

      opens = gsub(/\{/, "{", code); closes = gsub(/\}/, "}", code)
      depth += opens - closes

      if (open && depth <= guard_depth) open = 0

      if (open && $0 ~ /\.await/) {
        printf "%s:%d: `.await` while the guard bound at line %d (`%s`) is still alive\n", FILENAME, FNR, gline, recv
        sub(/^[[:space:]]+/, "", $0); printf "        %s\n", $0
        open = 0
      }

      # A statement ends at `;` or at the `{` that opens its block.
      if (code ~ /[;{]/) {
        if (!open && opens > closes &&
            stmt ~ ("(^|[^A-Za-z0-9_])(let|if let Some\\(|while let Some\\(|for)[[:space:]]") &&
            stmt ~ ("(^|[^A-Za-z0-9_])(" NAMES ")[[:space:]]*\\.[[:space:]]*(get|get_mut|entry|iter|iter_mut)[[:space:]]*\\(") &&
            stmt !~ /\.(map|and_then|cloned|copied|is_some|is_none|unwrap_or|unwrap_or_else|unwrap_or_default|contains|count|len)[[:space:]]*[(<]/) {
          match(stmt, "(" NAMES ")[[:space:]]*\\.[[:space:]]*(get|get_mut|entry|iter|iter_mut)")
          recv = substr(stmt, RSTART, RLENGTH); sub(/[[:space:]]*\..*/, "", recv)
          open = 1; guard_depth = depth - 1; gline = FNR
        }
        stmt = ""
      }
    }
  '
}

# One pass per product. `shared/` is scanned alongside hunter, so it is covered
# once rather than reported twice.
pass() {
  names=$(derive_names "$1")
  [ -n "$names" ] || {
    echo "check-async-guards: no DashMap declarations under '$1' - refusing to pass vacuously" >&2
    exit 1
  }
  scan "$names" "$2"
}

found=$(
  pass "hunter shared" "hunter shared"
  pass "forge shared" "forge"
)

if [ -n "$found" ]; then
  echo "$found"
  printf '\n%s\n' 'A DashMap guard is alive across an .await -- see the header of scripts/check-async-guards.sh.'
  printf '%s\n' 'Copy the fields out, let the guard drop, then await.'
  exit 1
fi

echo "check-async-guards: OK - no DashMap guard reaches an .await"
