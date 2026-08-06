# UI controls — exclusive filters vs panels

Shared chrome for exclusive choice and trade-mode labeling. Build filter bars
from these; do not invent a second Button-row language for All / Paper / Real
or date ranges.

## When to use what

| Control | Use for | Not for |
| --- | --- | --- |
| `DateTimeRangePicker` (`components/ui/DateTimeRangePicker`) | Date / datetime windows (History cohort, Created range, token filter dates) | Single `type="date"` fields; reinventing dual `datetime-local` inputs |
| `ToggleGroup` (`components/ui/ToggleGroup`) | Exclusive filters: scoreboard scope, SOL/USD, short non-temporal enums | Swapping large content panels; reinventing mode chrome; datetime windows |
| `ModeToggle` (`components/strategy/ModeToggle`) | All / Paper / Real (or Real / Paper) everywhere | Per-page option arrays with copied hues |
| `Tabs` (`components/ui/Tabs`) | Panel swaps (RuleEditor Builder/JSON, analytics views) | Mode / range filters |
| `Button` + `active` | One-off toolbar toggles that are not a multi-option filter | Real/Paper/All rows |
| `RuleModeFilter` | Rule boards that need mode counts (Rules, Simulate) | Surfaces without a rule list — use `ModeToggle` |
| `ModeBadge` / `modeBadgeVariant` | Any paper/real pill | `Badge variant="neutral"` for paper |
| `VisibilityToggleButton` | Show/hide cohort toggles (disabled rules, not-fired) | Form checkboxes / settings switches |

## `DateTimeRangePicker`

Compact input-shaped trigger (`From → To` placeholders when empty; presets show
`7 days · MM/DD → now`) + popover (shortcuts · two independent month panes
always visible/stacked · editable date+time fields · Apply/Cancel). Each pane
has its own ‹ ›; click the month label for a year stepper + Jan–Dec chooser.
Right pane stays ≥ left. Popover flips above the trigger when space below is
tight. Wire values are bare wall-clock `YYYY-MM-DDTHH:mm` (same as
`datetime-local`); callers own zone semantics — History treats them as UTC ISO
bounds, FilterPanel converts via `datetimeLocalToUtcWallClock` at the query
boundary. Preset clicks commit immediately; calendar edits stay in a draft
until Apply. Pass `presets` for History-style shortcuts; omit for calendar-only
fields (Simulate Created, token filter dates). `allowCustom={false}` for
preset-only APIs (Portfolio `range`, creation-stats / trader look-back days) —
hides the calendar and only lists shortcuts.

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

Left → scope controls (`ModeToggle`, `DateTimeRangePicker`) · Middle →
multi-select / SearchableSelect · Right → clear + primary / destructive CTAs.

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

## Explanation tips (`InfoTooltip` / `HoverPopover`)

Both use `hooks/useHoverPinPopover`:

- **Hover** opens; a short close delay keeps the panel alive while the pointer
  crosses the gap into the portaled body (scrollable, pointer-events on).
- **Click** pins (second click / Escape / outside pointer unpins). Nested
  links/buttons inside a `HoverPopover` trigger are ignored for pin.
- Do not reintroduce `pointer-events-none` on the panel or hide-on-trigger-leave
  without a delay — that blocks reading and scrolling long help.
