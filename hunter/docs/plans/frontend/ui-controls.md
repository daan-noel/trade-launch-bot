# UI controls — exclusive filters vs panels

Shared chrome for exclusive choice and trade-mode labeling. Build filter bars
from these; do not invent a second Button-row language for All / Paper / Real
or date ranges.

## When to use what

| Control | Use for | Not for |
| --- | --- | --- |
| `DatePicker` (`components/ui/DatePicker`) | Single civil day (`YYYY-MM-DD`) — sweep prune cutoff, Replay day-file | Ranges; reinventing bare `type="date"` |
| `DateTimeRangePicker` (`components/ui/DateTimeRangePicker`) | Date / datetime windows (History cohort, Created range, token filter dates) | Single-day fields; reinventing dual `datetime-local` inputs |
| `ToggleGroup` (`components/ui/ToggleGroup`) | Exclusive filters: scoreboard scope, SOL/USD, short non-temporal enums | Swapping large content panels; reinventing mode chrome; datetime windows |
| `ModeToggle` (`components/strategy/ModeToggle`) | All / Paper / Real (or Real / Paper) everywhere | Per-page option arrays with copied hues |
| `Tabs` (`components/ui/Tabs`) | Panel swaps (RuleEditor Builder/JSON, analytics views) | Mode / range filters |
| `Button` + `active` | One-off toolbar toggles that are not a multi-option filter | Real/Paper/All rows |
| `RuleModeFilter` | Rule boards that need mode counts (Rules, Simulate) | Surfaces without a rule list — use `ModeToggle` |
| `ModeBadge` / `modeBadgeVariant` | Any paper/real pill | `Badge variant="neutral"` for paper |
| `VisibilityToggleButton` | Show/hide cohort toggles (disabled rules, not-fired) | Form checkboxes / settings switches |

## `DatePicker`

Compact input-shaped trigger (calendar icon + `MM/DD/YYYY` / placeholder) +
popover (editable date field · one month pane with ‹ › / month-year chooser ·
Today · Clear). Day click commits immediately; typed date uses Apply / Enter.
Wire value is `YYYY-MM-DD` (or `''`). `timeZone` (default: browser IANA) drives
civil Today only. Zone badge is off by default (`showZoneBadge` / `zoneLabel`).
Supports inclusive `min` / `max` (disabled days). Shared calendar math lives in
`dateTimeRangePickerUtils.ts`.

| Surface | Notes |
| --- | --- |
| Grouped sweep “Clear runs before” | `max=today` local; prune still uses local midnight ISO |
| Replay “Date” | one day-file |

## `DateTimeRangePicker`

Compact input-shaped trigger (`From → To` placeholders when empty; presets show
`7 days · MM/DD → now`) + popover (shortcuts · editable From/To date+time
fields · two independent month panes · Apply/Cancel). MUI-like interaction:
click From/To to choose which bound the calendar edits; first day click sets
start, second sets end (hover previews the in-progress range); a click after a
complete range restarts at start. Fields sit above the calendars. Each pane has
its own ‹ ›; click the month label for a year stepper + Jan–Dec chooser. Right
pane stays ≥ left. Popover flips above the trigger when space below is tight.
Dialog is `aria-modal` with focus move/restore.

Wire values are bare wall-clock `YYYY-MM-DDTHH:mm` (same as `datetime-local`).
`timeZone` (default `UTC`) drives civil Today + the today ring only — it does
not rewrite wire values. `zoneLabel` defaults to a short badge from `timeZone`;
pass `zoneLabel={null}` to hide (look-back day presets). Callers own zone
semantics at the query boundary:

| Surface | `timeZone` / badge | Boundary |
| --- | --- | --- |
| History / Portfolio / Simulate / lab Created | `UTC` | wall-clock treated as UTC ISO, through the one `isoToPickerInput`/`pickerInputToIso` pair |
| Tokens Created | project IANA (`TokensFilterBar` `timezone`) | `datetimeLocalToUtcWallClock` |
| creation-stats windows | project IANA (display zone) | `CreationWindowPicker` → `resolveCreationWindow` |
| trader look-back | `zoneLabel={null}`, `allowCustom={false}` | day/preset enums only |

Preset clicks commit immediately; calendar edits stay in a draft until Apply.
Clear + Apply with empty bounds commits the `all` / "All" preset when one is
in `presets` (History All time). Pass `presets` for History-style shortcuts;
omit for calendar-only fields (Simulate Created, Tokens Created).
`allowCustom={false}` hides the calendar and only lists shortcuts. Pure helpers
live in `dateTimeRangePickerUtils.ts` (guarded by unit tests).

### `CreationWindowPicker`

Every creation-stats surface (page heatmap/trend, grouped section) takes its
window from this one wrapper, so they share a vocabulary: civil-day shortcuts
(Today / Yesterday, resolved in the DISPLAY zone), the rolling look-backs from
`RANGE_OPTIONS`, and `Custom` for absolute date+time bounds.
`resolveCreationWindow` (in `creationStats.ts`, unit-tested) lowers a window to
the API's `from`/`to` plus the `spanDays` the bucket-granularity gate reads —
civil days convert with `datetimeLocalToUtcWallClock`, rolling presets floor to
the hour so the RTK cache key stays stable, and an open upper bound is sent as
no `to` at all (the server ends the window at its own `now`). Persistence reads
the legacy bare day count through `toCreationWindow`, so a stored look-back
survives the upgrade.

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

## `Accordion` with a live header (`header` prop)

`title` mode makes the whole header row one big collapse button. `header` mode
exists for headers that carry their **own** controls (the Grouped Sweep run
picker: run `<select>`, delete, prune date + button) — there the row cannot
simply be a `<button>`.

The toggle surface is still the whole row; the row's `onClick` bails when the
event's target sits inside `button, select, input, textarea, a, label,
[role="button"]`, so every header control keeps its own semantics (a `<label>`
is in the list because clicking it activates its control, not the panel). The
prepended chevron `<button>` stays the keyboard/a11y trigger and carries
`aria-expanded`; it needs no `stopPropagation` because the same target check
already excludes it.

Give it a `toggleLabel` ("Run details") — a bare ~20px chevron is too small to
be the only visible affordance for a panel toggle. Pair with `storageKey` so a
panel the user opened stays open across reloads instead of snapping back to
`defaultOpen`.

## Explanation tips (`InfoTooltip` / `HoverPopover`)

Both use `hooks/useHoverPinPopover`:

- **Hover** opens; a short close delay keeps the panel alive while the pointer
  crosses the gap into the portaled body (scrollable, pointer-events on).
- **Click** pins (second click / Escape / outside pointer unpins). Nested
  links/buttons inside a `HoverPopover` trigger are ignored for pin.
- Do not reintroduce `pointer-events-none` on the panel or hide-on-trigger-leave
  without a delay — that blocks reading and scrolling long help.
