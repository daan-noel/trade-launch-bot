/**
 * Compact date-time range control — MUI DateTimeRangePicker interaction model
 * on our chrome (no MUI dep).
 *
 * Trigger is input-shaped (`From → To` placeholders, calendar icon, zone badge).
 * Popover: optional shortcuts · editable From/To date+time fields (click to set
 * which bound the calendar edits) · two month panes · hover range preview ·
 * Apply/Cancel. Each pane has its own ‹ ›; click the month label for a year
 * stepper + Jan–Dec chooser. Right pane stays ≥ left. Popover flips above the
 * trigger when space below is tight.
 *
 * Wire values are bare wall-clock `YYYY-MM-DDTHH:mm` (same as `datetime-local`).
 * Callers own zone semantics via `timeZone` (History/Simulate: UTC; Tokens: project
 * IANA zone, converted with `datetimeLocalToUtcWallClock` at the query boundary).
 */

import {
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import { createPortal } from 'react-dom';
import { cn } from 'lib/cn';
import { Button } from './Button';
import { CalendarIcon } from './icons';
import { fieldClassName } from './Input';
import {
  addMonths,
  applyDayPick,
  daysInMonth,
  defaultZoneBadge,
  formatCompact,
  initialViews,
  joinDt,
  monthIndex,
  normalizeTime,
  rangeDayRole,
  splitDt,
  startDow,
  todayInZone,
  ymFromDateString,
  ymdKey,
  type RangeDayRole,
  type YearMonth,
} from './dateTimeRangePickerUtils';

export type DateTimeRangePreset<T extends string = string> = {
  value: T;
  label: string;
  /** Optional secondary line under the shortcut label. */
  description?: string;
};

export type DateTimeRangeValue<T extends string = string> = {
  preset: T;
  /** Wall-clock `YYYY-MM-DDTHH:mm`, or `''` for an open lower bound. */
  from: string;
  /** Wall-clock `YYYY-MM-DDTHH:mm`, or `''` for an open upper bound. */
  to: string;
};

export interface DateTimeRangePickerProps<T extends string = string> {
  value: DateTimeRangeValue<T>;
  onChange: (next: DateTimeRangeValue<T>) => void;
  /**
   * Shortcut list. Include the custom sentinel (see `customPreset`) so the
   * sidebar can highlight it while editing. Omit / empty ⇒ calendar-only.
   */
  presets?: DateTimeRangePreset<T>[];
  /** Preset value that means "use from/to". Default `"custom"`. */
  customPreset?: T;
  /**
   * IANA zone for civil "today" (Today jump + today ring). Default `"UTC"`.
   * Does not rewrite wire values — callers convert at the query boundary.
   */
  timeZone?: string;
  /**
   * Zone badge on the trigger / popover. Default: short label from `timeZone`.
   * Pass `null` to hide (look-back day presets that are not wall-clock ranges).
   */
  zoneLabel?: string | null;
  /** Tooltip / aria hint when custom bounds are empty. */
  emptyLabel?: string;
  /**
   * When false, only preset shortcuts are offered (no calendar / date inputs).
   * Use for APIs that accept fixed windows only (Portfolio `range`, creation-stats
   * look-back days). Default true.
   */
  allowCustom?: boolean;
  size?: 'sm' | 'md';
  'aria-label'?: string;
  className?: string;
  disabled?: boolean;
}

const DOW = ['Su', 'Mo', 'Tu', 'We', 'Th', 'Fr', 'Sa'] as const;
const MONTHS = [
  'Jan',
  'Feb',
  'Mar',
  'Apr',
  'May',
  'Jun',
  'Jul',
  'Aug',
  'Sep',
  'Oct',
  'Nov',
  'Dec',
] as const;

const MENU_GAP_PX = 6;
const MENU_EDGE_PX = 8;

type MenuPosition = { top: number; left: number; maxHeight?: number };
type Draft = { preset: string; from: string; to: string; picking: 'from' | 'to' };
type ChooserPane = 'left' | 'right' | null;

function monthLabel({ y, m }: YearMonth): string {
  return `${MONTHS[m]} ${y}`;
}

const fieldCls = fieldClassName({
  size: 'sm',
  type: 'date',
  // Native date/time controls pad heavily for the picker glyph — pull it in.
  className:
    'scheme-dark pr-1 [&::-webkit-calendar-picker-indicator]:mr-0 [&::-webkit-calendar-picker-indicator]:opacity-60',
});

function dayCellClass(role: RangeDayRole, isToday: boolean): string {
  const endpoint =
    role === 'start' || role === 'end' || role === 'single' || role === 'preview-end';
  const middle = role === 'middle' || role === 'preview-middle';
  return cn(
    'relative flex size-7 items-center justify-center text-[11px] tabular-nums transition-colors',
    role === 'middle' && 'rounded-none bg-primary/14 text-text',
    role === 'preview-middle' && 'rounded-none bg-primary/8 text-text',
    endpoint && 'font-semibold',
    (role === 'start' || role === 'end' || role === 'single') &&
      'rounded bg-primary text-bg-panel',
    role === 'preview-end' && 'rounded bg-primary/55 text-bg-panel',
    role === 'start' && 'rounded-r-none',
    role === 'end' && 'rounded-l-none',
    role === 'preview-end' && 'rounded-l-none',
    !endpoint && !middle && 'rounded text-text-mid hover:bg-white/8 hover:text-text',
    isToday && !endpoint && 'font-semibold text-primary ring-1 ring-inset ring-primary/55',
    isToday && endpoint && 'ring-1 ring-inset ring-white/70',
  );
}

function MonthGrid({
  y,
  m,
  fromDate,
  toDate,
  picking,
  hoverYmd,
  todayYmd,
  onPick,
  onHover,
}: {
  y: number;
  m: number;
  fromDate: string;
  toDate: string;
  picking: 'from' | 'to';
  hoverYmd: string | null;
  todayYmd: string;
  onPick: (ymd: string) => void;
  onHover: (ymd: string | null) => void;
}) {
  const cells = useMemo(() => {
    const dim = daysInMonth(y, m);
    const lead = startDow(y, m);
    const out: { key: string; day: number | null; ymd: string | null }[] = [];
    for (let i = 0; i < lead; i++) out.push({ key: `b${i}`, day: null, ymd: null });
    for (let d = 1; d <= dim; d++) {
      const ymd = ymdKey(y, m, d);
      out.push({ key: ymd, day: d, ymd });
    }
    while (out.length % 7 !== 0) {
      out.push({ key: `t${out.length}`, day: null, ymd: null });
    }
    return out;
  }, [y, m]);

  return (
    <div
      className="w-fit"
      onMouseLeave={() => onHover(null)}
    >
      <div className="mb-1 grid w-[196px] grid-cols-7 text-center text-[9px] font-semibold uppercase tracking-wider text-text-dim/70">
        {DOW.map((d) => (
          <div key={d} className="h-5 leading-5">
            {d}
          </div>
        ))}
      </div>
      <div className="grid w-[196px] grid-cols-7">
        {cells.map((c) => {
          if (c.day == null || !c.ymd) {
            return <div key={c.key} className="size-7" />;
          }
          const ymd = c.ymd;
          const role = rangeDayRole(ymd, fromDate, toDate, picking, hoverYmd);
          const isToday = ymd === todayYmd;
          return (
            <button
              key={c.key}
              type="button"
              onClick={() => onPick(ymd)}
              onMouseEnter={() => onHover(ymd)}
              title={isToday ? 'Today' : undefined}
              className={dayCellClass(role, isToday)}
            >
              {c.day}
            </button>
          );
        })}
      </div>
    </div>
  );
}

/** Month/year chooser for one pane — year stepper + Jan–Dec grid. */
function MonthYearChooser({
  view,
  onPick,
}: {
  view: YearMonth;
  onPick: (next: YearMonth) => void;
}) {
  const [year, setYear] = useState(view.y);
  useEffect(() => setYear(view.y), [view.y]);

  return (
    <div className="flex w-[196px] flex-col gap-2">
      <div className="flex items-center justify-between">
        <button
          type="button"
          aria-label="Previous year"
          onClick={() => setYear((y) => y - 1)}
          className="rounded px-2 py-1 text-text-dim hover:bg-white/6 hover:text-text"
        >
          ‹
        </button>
        <span className="text-[12px] font-semibold tabular-nums text-text">{year}</span>
        <button
          type="button"
          aria-label="Next year"
          onClick={() => setYear((y) => y + 1)}
          className="rounded px-2 py-1 text-text-dim hover:bg-white/6 hover:text-text"
        >
          ›
        </button>
      </div>
      <div className="grid grid-cols-3 gap-1">
        {MONTHS.map((label, m) => {
          const selected = year === view.y && m === view.m;
          return (
            <button
              key={label}
              type="button"
              onClick={() => onPick({ y: year, m })}
              className={cn(
                'rounded-md px-1 py-1.5 text-[11px] font-semibold transition-colors',
                selected
                  ? 'bg-primary text-bg-panel'
                  : 'text-text-mid hover:bg-white/8 hover:text-text',
              )}
            >
              {label}
            </button>
          );
        })}
      </div>
    </div>
  );
}

function PaneHeader({
  view,
  chooserOpen,
  onToggleChooser,
  onStep,
  ariaPrev,
  ariaNext,
}: {
  view: YearMonth;
  chooserOpen: boolean;
  onToggleChooser: () => void;
  onStep: (delta: number) => void;
  ariaPrev: string;
  ariaNext: string;
}) {
  return (
    <div className="mb-1 flex w-[196px] items-center justify-between gap-0.5">
      <button
        type="button"
        aria-label={ariaPrev}
        onClick={() => onStep(-1)}
        className="rounded px-1.5 py-0.5 text-text-dim hover:bg-white/6 hover:text-text"
      >
        ‹
      </button>
      <button
        type="button"
        aria-label="Choose month and year"
        aria-expanded={chooserOpen}
        onClick={onToggleChooser}
        className={cn(
          'rounded px-1.5 py-0.5 text-[12px] font-semibold transition-colors',
          chooserOpen ? 'bg-primary/15 text-primary' : 'text-text hover:bg-white/6',
        )}
      >
        {monthLabel(view)}
        <span className="ml-1 text-[9px] opacity-60">{chooserOpen ? '▴' : '▾'}</span>
      </button>
      <button
        type="button"
        aria-label={ariaNext}
        onClick={() => onStep(1)}
        className="rounded px-1.5 py-0.5 text-text-dim hover:bg-white/6 hover:text-text"
      >
        ›
      </button>
    </div>
  );
}

function CalendarPane({
  view,
  chooserOpen,
  fromDate,
  toDate,
  picking,
  hoverYmd,
  todayYmd,
  onToggleChooser,
  onStep,
  onPickMonth,
  onPickDay,
  onHover,
  ariaPrev,
  ariaNext,
}: {
  view: YearMonth;
  chooserOpen: boolean;
  fromDate: string;
  toDate: string;
  picking: 'from' | 'to';
  hoverYmd: string | null;
  todayYmd: string;
  onToggleChooser: () => void;
  onStep: (delta: number) => void;
  onPickMonth: (next: YearMonth) => void;
  onPickDay: (ymd: string) => void;
  onHover: (ymd: string | null) => void;
  ariaPrev: string;
  ariaNext: string;
}) {
  return (
    <div className="w-[196px]">
      <PaneHeader
        view={view}
        chooserOpen={chooserOpen}
        onToggleChooser={onToggleChooser}
        onStep={onStep}
        ariaPrev={ariaPrev}
        ariaNext={ariaNext}
      />
      {chooserOpen ? (
        <MonthYearChooser view={view} onPick={onPickMonth} />
      ) : (
        <MonthGrid
          y={view.y}
          m={view.m}
          fromDate={fromDate}
          toDate={toDate}
          picking={picking}
          hoverYmd={hoverYmd}
          todayYmd={todayYmd}
          onPick={onPickDay}
          onHover={onHover}
        />
      )}
    </div>
  );
}

function BoundField({
  label,
  active,
  date,
  time,
  dateId,
  timeId,
  onActivate,
  onDate,
  onTime,
}: {
  label: string;
  active: boolean;
  date: string;
  time: string;
  dateId: string;
  timeId: string;
  onActivate: () => void;
  onDate: (date: string) => void;
  onTime: (time: string) => void;
}) {
  return (
    <div
      role="group"
      aria-label={label}
      onMouseDown={onActivate}
      className={cn(
        'flex flex-col gap-1 rounded-lg border px-2 py-1.5 text-left transition-colors',
        active
          ? 'border-primary/45 bg-primary/10'
          : 'border-white/8 bg-white/3 hover:border-white/14 hover:bg-white/5',
      )}
    >
      <span
        className={cn(
          'text-[9px] font-bold uppercase tracking-wider',
          active ? 'text-primary' : 'text-text-dim/70',
        )}
      >
        {label}
      </span>
      <div className="flex items-center gap-1">
        <input
          id={dateId}
          type="date"
          value={date}
          onFocus={onActivate}
          onChange={(e) => onDate(e.target.value)}
          className={cn(fieldCls, 'min-w-0 flex-1')}
          aria-label={`${label} date`}
          title={`${label} date`}
        />
        <input
          id={timeId}
          type="time"
          step={60}
          value={time}
          disabled={!date}
          onFocus={onActivate}
          onChange={(e) => onTime(e.target.value)}
          className={cn(fieldCls, 'w-24 shrink-0')}
          aria-label={`${label} time`}
          title={`${label} time`}
        />
      </div>
    </div>
  );
}

export function DateTimeRangePicker<T extends string = string>({
  value,
  onChange,
  presets = [],
  customPreset: customPresetProp,
  timeZone = 'UTC',
  zoneLabel: zoneLabelProp,
  emptyLabel = 'Select date range',
  allowCustom = true,
  size = 'sm',
  'aria-label': ariaLabel = 'Date range',
  className,
  disabled = false,
}: DateTimeRangePickerProps<T>) {
  const customPreset = (customPresetProp ?? ('custom' as T)) as T;
  const zoneBadge =
    zoneLabelProp === null
      ? null
      : (zoneLabelProp ?? defaultZoneBadge(timeZone));
  const menuId = useId();
  const titleId = useId();
  const fromDateId = useId();
  const fromTimeId = useId();
  const toDateId = useId();
  const toTimeId = useId();
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);
  const [menuPos, setMenuPos] = useState<MenuPosition>({ top: 0, left: 0 });
  const [draft, setDraft] = useState<Draft>({
    preset: value.preset,
    from: value.from,
    to: value.to,
    picking: 'from',
  });
  const init = initialViews(value.from, value.to, timeZone);
  const [leftView, setLeftView] = useState<YearMonth>(init.left);
  const [rightView, setRightView] = useState<YearMonth>(init.right);
  const [chooser, setChooser] = useState<ChooserPane>(null);
  const [hoverYmd, setHoverYmd] = useState<string | null>(null);

  const isCustom = value.preset === customPreset;
  const hasBounds = !!(value.from || value.to);
  const presetLabel =
    !isCustom
      ? (presets.find((p) => p.value === value.preset)?.label ?? value.preset)
      : null;
  const fromLabel = formatCompact(value.from);
  const toLabel = formatCompact(value.to);
  const fromParts = splitDt(draft.from);
  const toParts = splitDt(draft.to);
  const todayYmd = todayInZone(timeZone).ymd;
  const invalidRange =
    draft.preset === customPreset &&
    !!draft.from &&
    !!draft.to &&
    draft.from > draft.to;

  const revealMonth = useCallback((ymd: string, pane: 'left' | 'right') => {
    const ym = ymFromDateString(ymd);
    if (!ym) return;
    if (pane === 'left') {
      setLeftView(ym);
      setRightView((r) => (monthIndex(ym) > monthIndex(r) ? ym : r));
    } else {
      setRightView(ym);
      setLeftView((l) => (monthIndex(ym) < monthIndex(l) ? ym : l));
    }
  }, []);

  const syncDraftFromValue = useCallback(() => {
    setDraft({
      preset: value.preset,
      from: value.from,
      to: value.to,
      picking: value.from && !value.to ? 'to' : 'from',
    });
    const views = initialViews(value.from, value.to, timeZone);
    setLeftView(views.left);
    setRightView(views.right);
    setChooser(null);
    setHoverYmd(null);
  }, [value, timeZone]);

  const updateMenuPosition = useCallback(() => {
    const el = triggerRef.current;
    const menu = menuRef.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    const menuW = menu?.offsetWidth ?? 560;
    const menuH = menu?.offsetHeight ?? 420;
    const left = Math.min(
      Math.max(MENU_EDGE_PX, rect.left),
      window.innerWidth - menuW - MENU_EDGE_PX,
    );
    const spaceBelow = window.innerHeight - rect.bottom - MENU_GAP_PX - MENU_EDGE_PX;
    const spaceAbove = rect.top - MENU_GAP_PX - MENU_EDGE_PX;
    const placeAbove = spaceBelow < menuH && spaceAbove > spaceBelow;
    const top = placeAbove
      ? Math.max(MENU_EDGE_PX, rect.top - menuH - MENU_GAP_PX)
      : rect.bottom + MENU_GAP_PX;
    const maxHeight = placeAbove
      ? Math.max(240, rect.top - MENU_GAP_PX - MENU_EDGE_PX)
      : Math.max(240, spaceBelow);
    setMenuPos({ top, left, maxHeight });
  }, []);

  useLayoutEffect(() => {
    if (!open) return;
    updateMenuPosition();
    window.addEventListener('resize', updateMenuPosition);
    window.addEventListener('scroll', updateMenuPosition, true);
    return () => {
      window.removeEventListener('resize', updateMenuPosition);
      window.removeEventListener('scroll', updateMenuPosition, true);
    };
  }, [open, updateMenuPosition, chooser, draft.from, draft.to]);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (e: PointerEvent) => {
      const target = e.target as Node;
      if (triggerRef.current?.contains(target)) return;
      if (menuRef.current?.contains(target)) return;
      setOpen(false);
    };
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      if (chooser) {
        setChooser(null);
        return;
      }
      setOpen(false);
    };
    document.addEventListener('pointerdown', onPointerDown);
    document.addEventListener('keydown', onKeyDown);
    return () => {
      document.removeEventListener('pointerdown', onPointerDown);
      document.removeEventListener('keydown', onKeyDown);
    };
  }, [open, chooser]);

  // Move focus into the dialog on open; restore the trigger on close.
  useEffect(() => {
    if (!open) return;
    const prev = document.activeElement as HTMLElement | null;
    const t = window.setTimeout(() => {
      menuRef.current
        ?.querySelector<HTMLElement>('button:not([disabled]), input:not([disabled])')
        ?.focus();
    }, 0);
    return () => {
      window.clearTimeout(t);
      if (prev && typeof prev.focus === 'function' && document.contains(prev)) {
        prev.focus();
      } else {
        triggerRef.current?.focus();
      }
    };
  }, [open]);

  const openMenu = () => {
    if (disabled) return;
    syncDraftFromValue();
    setOpen(true);
  };

  const commit = (next: DateTimeRangeValue<T>) => {
    onChange(next);
    setOpen(false);
  };

  const applyCustom = () => {
    if (draft.preset !== customPreset) {
      setOpen(false);
      return;
    }
    if (invalidRange) return;
    if (!draft.from && !draft.to) {
      const all = presets.find(
        (p) => p.value === 'all' || p.label.toLowerCase() === 'all',
      );
      if (all) {
        commit({ preset: all.value, from: '', to: '' });
        return;
      }
    }
    commit({
      preset: customPreset,
      from: draft.from,
      to: draft.to,
    });
  };

  const pickPreset = (p: T) => {
    if (allowCustom && p === customPreset) {
      setDraft((d) => ({
        ...d,
        preset: customPreset,
        // Keep any seeded bounds from a prior preset so Custom starts editable.
        picking: d.from && !d.to ? 'to' : 'from',
      }));
      return;
    }
    commit({ preset: p, from: '', to: '' });
  };

  const pickDay = (ymd: string) => {
    setChooser(null);
    setHoverYmd(null);
    const next = applyDayPick(draft, ymd, customPreset);
    setDraft(next);
    const fromDate = splitDt(next.from).date;
    const toDate = splitDt(next.to).date;
    if (fromDate) revealMonth(fromDate, 'left');
    if (toDate) revealMonth(toDate, 'right');
  };

  const setBoundDate = (bound: 'from' | 'to', date: string) => {
    const prev = splitDt(bound === 'from' ? draft.from : draft.to);
    if (!date) {
      setDraft({
        ...draft,
        preset: customPreset,
        [bound]: '',
        picking: bound === 'from' ? 'from' : draft.picking,
      });
      return;
    }
    const time = prev.time || (bound === 'from' ? '00:00' : '23:59');
    setDraft({
      ...draft,
      preset: customPreset,
      [bound]: joinDt(date, time),
      picking: bound === 'from' && !draft.to ? 'to' : bound,
    });
    revealMonth(date, bound === 'from' ? 'left' : 'right');
  };

  const setBoundTime = (bound: 'from' | 'to', time: string) => {
    setDraft((d) => {
      const parts = splitDt(bound === 'from' ? d.from : d.to);
      if (!parts.date) return d;
      const normalized = normalizeTime(time);
      const next = joinDt(
        parts.date,
        normalized || (bound === 'from' ? '00:00' : '23:59'),
      );
      return {
        ...d,
        preset: customPreset,
        [bound]: next,
        picking: bound,
      };
    });
  };

  const stepLeft = (delta: number) => {
    setChooser(null);
    setLeftView((v) => {
      const next = addMonths(v, delta);
      setRightView((r) => (monthIndex(next) > monthIndex(r) ? next : r));
      return next;
    });
  };

  const stepRight = (delta: number) => {
    setChooser(null);
    setRightView((v) => {
      const next = addMonths(v, delta);
      setLeftView((l) => (monthIndex(next) < monthIndex(l) ? next : l));
      return next;
    });
  };

  const setLeftMonth = (next: YearMonth) => {
    setLeftView(next);
    setRightView((r) => (monthIndex(next) > monthIndex(r) ? next : r));
    setChooser(null);
  };

  const setRightMonth = (next: YearMonth) => {
    setRightView(next);
    setLeftView((l) => (monthIndex(next) < monthIndex(l) ? next : l));
    setChooser(null);
  };

  const goToToday = () => {
    const { ym } = todayInZone(timeZone);
    setChooser(null);
    setLeftView(ym);
    setRightView(addMonths(ym, 1));
  };

  const sizeCls =
    size === 'sm'
      ? 'min-h-7 px-2 py-1 pr-1.5 text-[11px]'
      : 'min-h-8 px-2.5 py-1.5 pr-2 text-[13px]';

  const pickingHint =
    draft.picking === 'to' && fromParts.date && !toParts.date
      ? 'Select end date'
      : draft.picking === 'from' && !fromParts.date
        ? 'Select start date'
        : draft.picking === 'from' && fromParts.date
          ? 'Editing start · click a day to restart range'
          : null;

  const title =
    presetLabel ??
    (fromLabel || toLabel
      ? `${fromLabel || '…'} → ${toLabel || '…'}${zoneBadge ? ` (${zoneBadge})` : ''}`
      : `${emptyLabel}${zoneBadge ? ` (${zoneBadge})` : ''}`);

  const menu = open
    ? createPortal(
        <div
          ref={menuRef}
          id={menuId}
          role="dialog"
          aria-modal="true"
          aria-labelledby={titleId}
          style={{
            top: menuPos.top,
            left: menuPos.left,
            maxHeight: menuPos.maxHeight,
          }}
          className="fixed z-200 flex flex-col overflow-hidden rounded-xl border border-white/8 bg-bg-panel shadow-[0_16px_48px_rgba(0,0,0,0.55)]"
        >
          <div className="flex shrink-0 items-center justify-between gap-2 border-b border-white/6 px-3 py-2">
            <span id={titleId} className="text-[11px] font-semibold text-text">
              Date range
            </span>
            <div className="flex items-center gap-1.5">
              {allowCustom && (
                <button
                  type="button"
                  onClick={goToToday}
                  title={`Jump calendars to today (${zoneBadge ?? timeZone})`}
                  className="rounded-md border border-primary/30 bg-primary/10 px-2 py-0.5 text-[10px] font-semibold text-primary hover:bg-primary/20"
                >
                  Today
                </button>
              )}
              {zoneBadge && (
                <span className="rounded bg-white/6 px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-text-dim">
                  {zoneBadge}
                </span>
              )}
            </div>
          </div>

          <div
            className={cn(
              'flex min-h-0 flex-1 overflow-y-auto overflow-x-hidden',
              allowCustom ? 'flex-col sm:flex-row' : 'flex-col',
            )}
          >
            {presets.length > 0 && (
              <div
                className={cn(
                  'flex shrink-0 gap-0.5 p-2',
                  allowCustom
                    ? 'flex-row overflow-x-auto border-b border-white/6 sm:w-28 sm:flex-col sm:overflow-visible sm:border-b-0 sm:border-r'
                    : 'min-w-48 flex-col',
                )}
              >
                {presets.map((p) => {
                  const selected = draft.preset === p.value;
                  return (
                    <button
                      key={p.value}
                      type="button"
                      onClick={() => pickPreset(p.value)}
                      className={cn(
                        'rounded-md px-2.5 py-1.5 text-left text-[11px] font-semibold whitespace-nowrap transition-colors',
                        selected
                          ? 'bg-primary/20 text-primary'
                          : 'text-text-dim hover:bg-white/6 hover:text-text',
                      )}
                    >
                      <div>{p.label}</div>
                      {p.description && (
                        <div className="text-[9px] font-normal text-text-dim/70">
                          {p.description}
                        </div>
                      )}
                    </button>
                  );
                })}
              </div>
            )}

            {allowCustom && (
              <div className="flex w-fit flex-col gap-2.5 p-3">
                <div className="grid w-full max-w-[408px] grid-cols-1 gap-2 sm:grid-cols-2">
                  <BoundField
                    label="From"
                    active={draft.picking === 'from'}
                    date={fromParts.date}
                    time={fromParts.time}
                    dateId={fromDateId}
                    timeId={fromTimeId}
                    onActivate={() =>
                      setDraft((d) => ({ ...d, preset: customPreset, picking: 'from' }))
                    }
                    onDate={(date) => setBoundDate('from', date)}
                    onTime={(time) => setBoundTime('from', time)}
                  />
                  <BoundField
                    label="To"
                    active={draft.picking === 'to'}
                    date={toParts.date}
                    time={toParts.time}
                    dateId={toDateId}
                    timeId={toTimeId}
                    onActivate={() =>
                      setDraft((d) => ({ ...d, preset: customPreset, picking: 'to' }))
                    }
                    onDate={(date) => setBoundDate('to', date)}
                    onTime={(time) => setBoundTime('to', time)}
                  />
                </div>

                {pickingHint && (
                  <p className="text-[10px] font-medium text-primary">{pickingHint}</p>
                )}

                <div className="flex flex-col gap-4 sm:flex-row sm:gap-3">
                  <CalendarPane
                    view={leftView}
                    chooserOpen={chooser === 'left'}
                    fromDate={fromParts.date}
                    toDate={toParts.date}
                    picking={draft.picking}
                    hoverYmd={hoverYmd}
                    todayYmd={todayYmd}
                    onToggleChooser={() =>
                      setChooser((c) => (c === 'left' ? null : 'left'))
                    }
                    onStep={stepLeft}
                    onPickMonth={setLeftMonth}
                    onPickDay={pickDay}
                    onHover={setHoverYmd}
                    ariaPrev="Previous month (start pane)"
                    ariaNext="Next month (start pane)"
                  />
                  <CalendarPane
                    view={rightView}
                    chooserOpen={chooser === 'right'}
                    fromDate={fromParts.date}
                    toDate={toParts.date}
                    picking={draft.picking}
                    hoverYmd={hoverYmd}
                    todayYmd={todayYmd}
                    onToggleChooser={() =>
                      setChooser((c) => (c === 'right' ? null : 'right'))
                    }
                    onStep={stepRight}
                    onPickMonth={setRightMonth}
                    onPickDay={pickDay}
                    onHover={setHoverYmd}
                    ariaPrev="Previous month (end pane)"
                    ariaNext="Next month (end pane)"
                  />
                </div>

                {invalidRange && (
                  <p className="text-[10px] text-red">End must be on or after start.</p>
                )}

                <div className="flex items-center justify-between gap-2 border-t border-white/6 pt-2">
                  <button
                    type="button"
                    className="text-[11px] font-semibold text-text-dim hover:text-text"
                    title={
                      presets.some(
                        (p) => p.value === 'all' || p.label.toLowerCase() === 'all',
                      )
                        ? 'Clear bounds; Apply commits All time when an All preset exists'
                        : 'Clear both bounds'
                    }
                    onClick={() =>
                      setDraft({
                        preset: customPreset,
                        from: '',
                        to: '',
                        picking: 'from',
                      })
                    }
                  >
                    Clear dates
                  </button>
                  <div className="flex items-center gap-1.5">
                    <Button size="sm" variant="ghost" onClick={() => setOpen(false)}>
                      Cancel
                    </Button>
                    <Button
                      size="sm"
                      variant="primary"
                      onClick={applyCustom}
                      disabled={invalidRange}
                    >
                      Apply
                    </Button>
                  </div>
                </div>
              </div>
            )}
          </div>
        </div>,
        document.body,
      )
    : null;

  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        disabled={disabled}
        aria-label={ariaLabel}
        aria-expanded={open}
        aria-haspopup="dialog"
        aria-controls={open ? menuId : undefined}
        onClick={() => (open ? setOpen(false) : openMenu())}
        title={title}
        className={cn(
          'inline-flex w-auto max-w-80 cursor-pointer items-center gap-1.5 rounded-md border font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-50',
          sizeCls,
          open
            ? 'border-primary/50 bg-primary/10 text-primary'
            : isCustom && hasBounds
              ? 'border-primary/35 bg-primary/8 text-primary'
              : 'border-white/12 bg-white/5 text-text hover:border-white/22 hover:bg-white/7',
          className,
        )}
      >
        <CalendarIcon
          className={cn(
            'size-3.5 shrink-0',
            open || (isCustom && hasBounds) ? 'text-primary' : 'text-text-dim',
          )}
        />

        {presetLabel ? (
          <span className="flex min-w-0 items-center gap-1 truncate text-left">
            <span className="font-semibold">{presetLabel}</span>
            {(fromLabel || toLabel) && (
              <span className="truncate text-text-dim/70 tabular-nums">
                · {fromLabel || '…'}
                {toLabel ? ` → ${toLabel}` : ' → now'}
              </span>
            )}
          </span>
        ) : (
          <span className="flex min-w-0 items-center gap-1 truncate text-left tabular-nums">
            <span
              className={cn(
                'truncate',
                fromLabel ? 'font-semibold text-text' : 'text-text-dim/70',
              )}
            >
              {fromLabel || 'From'}
            </span>
            <span className="shrink-0 text-text-dim/50" aria-hidden>
              →
            </span>
            <span
              className={cn(
                'truncate',
                toLabel ? 'font-semibold text-text' : 'text-text-dim/70',
              )}
            >
              {toLabel || 'To'}
            </span>
          </span>
        )}

        {zoneBadge && (
          <span className="shrink-0 rounded bg-white/6 px-1 py-0.5 text-[9px] font-bold uppercase tracking-wider text-text-dim">
            {zoneBadge}
          </span>
        )}
        <span
          className={cn(
            'shrink-0 text-[9px] leading-none text-text-dim/70 transition-transform',
            open && 'rotate-180',
          )}
          aria-hidden
        >
          ▾
        </span>
      </button>
      {menu}
    </>
  );
}
