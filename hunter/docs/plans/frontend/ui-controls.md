# UI controls — exclusive filters vs panels

Shared chrome for exclusive choice and trade-mode labeling. Build filter bars
from these; do not invent a second Button-row language for All / Paper / Real
or date ranges.

## When to use what

| Control | Use for | Not for |
| --- | --- | --- |
| `ToggleGroup` (`components/ui/ToggleGroup`) | Exclusive filters: date range, scoreboard scope, SOL/USD | Swapping large content panels; reinventing mode chrome |
| `ModeToggle` (`components/strategy/ModeToggle`) | All / Paper / Real (or Real / Paper) everywhere | Per-page option arrays with copied hues |
| `Tabs` (`components/ui/Tabs`) | Panel swaps (RuleEditor Builder/JSON, analytics views) | Mode / range filters |
| `Button` + `active` | One-off toolbar toggles that are not a multi-option filter | Real/Paper/All rows |
| `RuleModeFilter` | Rule boards that need mode counts (Rules, Simulate) | Surfaces without a rule list — use `ModeToggle` |
| `ModeBadge` / `modeBadgeVariant` | Any paper/real pill | `Badge variant="neutral"` for paper |
| `VisibilityToggleButton` | Show/hide cohort toggles (disabled rules, not-fired) | Form checkboxes / settings switches |

## `ToggleGroup`

- Track: `bg-white/5` + `aria-pressed` options.
- `tone="primary"` — ranges, score scope (`bg-primary/20`).
- `tone="mode"` — supply per-option `activeClassName` (prefer `ModeToggle`).
- `tone="neutral"` — dual toggles; optional per-option `activeClassName` (SOL/USD).
- ArrowLeft / ArrowRight moves selection within the group.

Domain wrappers:

- `ModeToggle` — trade-mode chrome SSOT.
- `RuleModeFilter` — `ModeToggle` + rule counts over `useModeFilter`.
- `PriceUnitToggle` — SOL/USD via `ToggleGroup`.

## `ModeToggle`

Chrome lives in `lib/strategy/mode.ts` (`MODE_TOGGLE_ACTIVE`, `MODE_TOGGLE_SWATCH`)
and `components/strategy/ModeToggle`:

| Prop | Behavior |
| --- | --- |
| `layout="filter"` | **All · PAPER · REAL** (rule boards) |
| `layout="ops"` | **REAL · PAPER · All** (Console / History — money first) |
| `includeAll={false}` | **REAL · PAPER** only (Portfolio) |
| `counts` | Optional tabular counts (rule boards only) |

Labels: `PAPER` / `REAL` uppercase (match `ModeBadge`); `All` stays title-case.
Hues: paper=`info`, real=`warning`, all=`primary`. Do not copy these classes
into page files.

## Mode badge SSOT

`lib/strategy/mode.ts` → `modeBadgeVariant(mode)`:

- `paper` → `info`
- `real` → `warning`

`components/strategy/ModeBadge` is the thin UI wrapper: **pill** + **uppercase**
label (`PAPER` / `REAL`), same hues as `ModeToggle` / row rails. Console
History and ops rows must not paint paper as `neutral` or reinvent the chip.

## Toolbar recipe

Left → scope segmenteds (mode, range) · Middle → multi-select / SearchableSelect ·
Right → clear + primary / destructive CTAs.

Defaults stay product-specific (Console mode defaults to `real`; Rules /
Simulate mode filter defaults to `all`) — only the chrome is shared.

## Bulk mode action clusters (Rules Pause All / Stop All)

When a header exposes per-mode bulk actions, keep each mode as a **framed
cluster**, not a loose badge + icon row:

- Container: `rounded-md border … px-1.5 py-1` with mode tint
  (`paper` → `border-info/25 bg-info/5`, `real` → `border-warning/25 bg-warning/5`)
- Label: `ModeBadge` (same hues as the filter / row rail)
- Actions: `IconButton` `size="md"` inside `IconButtonGroup` (ghost Pause,
  danger Stop)

Do not drop the frame or shrink to `sm` — these are high-stakes ops controls.
