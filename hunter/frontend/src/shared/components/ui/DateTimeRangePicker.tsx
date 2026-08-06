/**
 * Compact date-time range control — MUI DateTimeRangePicker interaction model
 * on our chrome (no MUI dep).
 *
 * Trigger is input-shaped (`From → To` placeholders, calendar icon, UTC badge).
 * Popover: optional shortcuts · two independent month panes (each with ‹ › and
 * a clickable month/year chooser) · editable date+time fields · Apply/Cancel.
 *
 * Wire values are bare wall-clock `YYYY-MM-DDTHH:mm` (same as `datetime-local`).
 * Callers own zone semantics (History treats them as UTC; FilterPanel converts
 * via `datetimeLocalToUtcWallClock` at the query boundary).
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
  /** Zone badge (display only). Default `"UTC"`. */
  zoneLabel?: string;
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

const DT_RE = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})/;
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

type YearMonth = { y: number; m: number };
type MenuPosition = { top: number; left: number; maxHeight?: number };
type Draft = { preset: string; from: string; to: string; picking: 'from' | 'to' };
type ChooserPane = 'left' | 'right' | null;

function pad2(n: number): string {
  return String(n).padStart(2, '0');
}

function splitDt(v: string): { date: string; time: string } {
  if (!v) return { date: '', time: '' };
  const m = DT_RE.exec(v);
  if (!m) return { date: '', time: '' };
  return { date: `${m[1]}-${m[2]}-${m[3]}`, time: `${m[4]}:${m[5]}` };
}

function joinDt(date: string, time: string): string {
  if (!date) return '';
  return `${date}T${time || '00:00'}`;
}

function ymdKey(y: number, m: number, d: number): string {
  return `${y}-${pad2(m + 1)}-${pad2(d)}`;
}

function parseYmd(key: string): (YearMonth & { d: number }) | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(key);
  if (!m) return null;
  return { y: Number(m[1]), m: Number(m[2]) - 1, d: Number(m[3]) };
}

function monthIndex({ y, m }: YearMonth): number {
  return y * 12 + m;
}

function addMonths({ y, m }: YearMonth, delta: number): YearMonth {
  const idx = y * 12 + m + delta;
  return { y: Math.floor(idx / 12), m: ((idx % 12) + 12) % 12 };
}

function daysInMonth(y: number, m: number): number {
  return new Date(Date.UTC(y, m + 1, 0)).getUTCDate();
}

function startDow(y: number, m: number): number {
  return new Date(Date.UTC(y, m, 1)).getUTCDay();
}

function formatCompact(dt: string): string {
  const { date, time } = splitDt(dt);
  if (!date) return '';
  const [, mo, da] = date.split('-');
  return `${mo}/${da} ${time}`;
}

function monthLabel({ y, m }: YearMonth): string {
  return `${MONTHS[m]} ${y}`;
}

function ymFromDateString(date: string): YearMonth | null {
  const parsed = date ? parseYmd(date) : null;
  return parsed ? { y: parsed.y, m: parsed.m } : null;
}

/** Civil "today" in UTC — matches the picker's default zone badge. */
function todayUtc(): { ymd: string; ym: YearMonth } {
  const n = new Date();
  const ym = { y: n.getUTCFullYear(), m: n.getUTCMonth() };
  return { ymd: ymdKey(ym.y, ym.m, n.getUTCDate()), ym };
}

/** Seed left/right panes from bounds (or today UTC / today+1). */
function initialViews(from: string, to: string): { left: YearMonth; right: YearMonth } {
  const today = todayUtc().ym;
  const left = ymFromDateString(splitDt(from).date) ?? today;
  const rightRaw = ymFromDateString(splitDt(to).date) ?? addMonths(left, 1);
  const right = monthIndex(rightRaw) < monthIndex(left) ? left : rightRaw;
  return { left, right };
}

const fieldCls = fieldClassName({
  size: 'sm',
  type: 'date',
  // Native date/time controls pad heavily for the picker glyph — pull it in.
  className:
    'scheme-dark pr-1 [&::-webkit-calendar-picker-indicator]:mr-0 [&::-webkit-calendar-picker-indicator]:opacity-60',
});

function MonthGrid({
  y,
  m,
  fromDate,
  toDate,
  todayYmd,
  onPick,
}: {
  y: number;
  m: number;
  fromDate: string;
  toDate: string;
  todayYmd: string;
  onPick: (ymd: string) => void;
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
    <div className="w-fit">
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
          const isFrom = ymd === fromDate;
          const isTo = ymd === toDate;
          const isToday = ymd === todayYmd;
          const inRange =
            !!fromDate && !!toDate && ymd > fromDate && ymd < toDate;
          const isEndpoint = isFrom || isTo;
          return (
            <button
              key={c.key}
              type="button"
              onClick={() => onPick(ymd)}
              title={isToday ? 'Today' : undefined}
              className={cn(
                'relative flex size-7 items-center justify-center text-[11px] tabular-nums transition-colors',
                inRange && 'bg-primary/12 text-text',
                isEndpoint && 'rounded bg-primary font-semibold text-bg-panel',
                !isEndpoint && !inRange && 'rounded text-text-mid hover:bg-white/8 hover:text-text',
                isFrom && !isTo && 'rounded-r-none',
                isTo && !isFrom && 'rounded-l-none',
                inRange && 'rounded-none',
                // Today ring sits under the endpoint fill when they coincide.
                isToday &&
                  !isEndpoint &&
                  'font-semibold text-primary ring-1 ring-inset ring-primary/55',
                isToday && isEndpoint && 'ring-1 ring-inset ring-white/70',
              )}
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
  todayYmd,
  onToggleChooser,
  onStep,
  onPickMonth,
  onPickDay,
  ariaPrev,
  ariaNext,
}: {
  view: YearMonth;
  chooserOpen: boolean;
  fromDate: string;
  toDate: string;
  todayYmd: string;
  onToggleChooser: () => void;
  onStep: (delta: number) => void;
  onPickMonth: (next: YearMonth) => void;
  onPickDay: (ymd: string) => void;
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
          todayYmd={todayYmd}
          onPick={onPickDay}
        />
      )}
    </div>
  );
}

export function DateTimeRangePicker<T extends string = string>({
  value,
  onChange,
  presets = [],
  customPreset: customPresetProp,
  zoneLabel = 'UTC',
  emptyLabel = 'Select date range',
  allowCustom = true,
  size = 'sm',
  'aria-label': ariaLabel = 'Date range',
  className,
  disabled = false,
}: DateTimeRangePickerProps<T>) {
  const customPreset = (customPresetProp ?? ('custom' as T)) as T;
  const menuId = useId();
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
  const init = initialViews(value.from, value.to);
  const [leftView, setLeftView] = useState<YearMonth>(init.left);
  const [rightView, setRightView] = useState<YearMonth>(init.right);
  const [chooser, setChooser] = useState<ChooserPane>(null);

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
  const todayYmd = todayUtc().ymd;
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
    const views = initialViews(value.from, value.to);
    setLeftView(views.left);
    setRightView(views.right);
    setChooser(null);
  }, [value]);

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
    const fromDate = splitDt(draft.from).date;
    const toDate = splitDt(draft.to).date;
    if (draft.picking === 'from' || (fromDate && toDate)) {
      const time = splitDt(draft.from).time || '00:00';
      setDraft({
        preset: customPreset,
        from: joinDt(ymd, time),
        to: '',
        picking: 'to',
      });
      revealMonth(ymd, 'left');
      return;
    }
    const fromTime = splitDt(draft.from).time || '00:00';
    const toTime = splitDt(draft.to).time || '23:59';
    if (ymd < fromDate) {
      setDraft({
        preset: customPreset,
        from: joinDt(ymd, fromTime),
        to: joinDt(fromDate, toTime),
        picking: 'from',
      });
      revealMonth(ymd, 'left');
      revealMonth(fromDate, 'right');
      return;
    }
    setDraft({
      preset: customPreset,
      from: draft.from,
      to: joinDt(ymd, toTime),
      picking: 'from',
    });
    revealMonth(ymd, 'right');
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
      picking: bound === 'from' && !draft.to ? 'to' : draft.picking,
    });
    revealMonth(date, bound === 'from' ? 'left' : 'right');
  };

  const setBoundTime = (bound: 'from' | 'to', time: string) => {
    setDraft((d) => {
      const parts = splitDt(bound === 'from' ? d.from : d.to);
      if (!parts.date) return d;
      const next = joinDt(
        parts.date,
        time || (bound === 'from' ? '00:00' : '23:59'),
      );
      return {
        ...d,
        preset: customPreset,
        [bound]: next,
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
    const { ym } = todayUtc();
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
        : null;

  const title =
    presetLabel ??
    (fromLabel || toLabel
      ? `${fromLabel || '…'} → ${toLabel || '…'} (${zoneLabel})`
      : `${emptyLabel} (${zoneLabel})`);

  const menu = open
    ? createPortal(
        <div
          ref={menuRef}
          id={menuId}
          role="dialog"
          aria-label={ariaLabel}
          style={{
            top: menuPos.top,
            left: menuPos.left,
            maxHeight: menuPos.maxHeight,
          }}
          className="fixed z-200 flex flex-col overflow-hidden rounded-xl border border-white/8 bg-bg-panel shadow-[0_16px_48px_rgba(0,0,0,0.55)]"
        >
          <div className="flex shrink-0 items-center justify-between gap-2 border-b border-white/6 px-3 py-2">
            <span className="text-[11px] font-semibold text-text">Date range</span>
            <div className="flex items-center gap-1.5">
              {allowCustom && (
                <button
                  type="button"
                  onClick={goToToday}
                  title="Jump calendars to today (UTC)"
                  className="rounded-md border border-primary/30 bg-primary/10 px-2 py-0.5 text-[10px] font-semibold text-primary hover:bg-primary/20"
                >
                  Today
                </button>
              )}
              <span className="rounded bg-white/6 px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-text-dim">
                {zoneLabel}
              </span>
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
              <div className="flex w-fit flex-col gap-2 p-3">
                {pickingHint && (
                  <p className="text-[10px] font-medium text-primary">{pickingHint}</p>
                )}

                <div className="flex flex-col gap-4 sm:flex-row sm:gap-3">
                  <CalendarPane
                    view={leftView}
                    chooserOpen={chooser === 'left'}
                    fromDate={fromParts.date}
                    toDate={toParts.date}
                    todayYmd={todayYmd}
                    onToggleChooser={() =>
                      setChooser((c) => (c === 'left' ? null : 'left'))
                    }
                    onStep={stepLeft}
                    onPickMonth={setLeftMonth}
                    onPickDay={pickDay}
                    ariaPrev="Previous month (start pane)"
                    ariaNext="Next month (start pane)"
                  />
                  <CalendarPane
                    view={rightView}
                    chooserOpen={chooser === 'right'}
                    fromDate={fromParts.date}
                    toDate={toParts.date}
                    todayYmd={todayYmd}
                    onToggleChooser={() =>
                      setChooser((c) => (c === 'right' ? null : 'right'))
                    }
                    onStep={stepRight}
                    onPickMonth={setRightMonth}
                    onPickDay={pickDay}
                    ariaPrev="Previous month (end pane)"
                    ariaNext="Next month (end pane)"
                  />
                </div>

                <div className="grid w-full max-w-[408px] grid-cols-1 gap-2 border-t border-white/6 pt-2 sm:grid-cols-2">
                  <div className="flex flex-col gap-1">
                    <span className="text-[9px] font-bold uppercase tracking-wider text-text-dim/70">
                      From
                    </span>
                    <div className="flex items-center gap-1">
                      <input
                        type="date"
                        value={fromParts.date}
                        onChange={(e) => setBoundDate('from', e.target.value)}
                        className={cn(fieldCls, 'min-w-0 flex-1')}
                        title="Start date"
                      />
                      <input
                        type="time"
                        value={fromParts.time}
                        disabled={!fromParts.date}
                        onChange={(e) => setBoundTime('from', e.target.value)}
                        className={cn(fieldCls, 'w-24 shrink-0')}
                        title="Start time"
                      />
                    </div>
                  </div>
                  <div className="flex flex-col gap-1">
                    <span className="text-[9px] font-bold uppercase tracking-wider text-text-dim/70">
                      To
                    </span>
                    <div className="flex items-center gap-1">
                      <input
                        type="date"
                        value={toParts.date}
                        onChange={(e) => setBoundDate('to', e.target.value)}
                        className={cn(fieldCls, 'min-w-0 flex-1')}
                        title="End date"
                      />
                      <input
                        type="time"
                        value={toParts.time}
                        disabled={!toParts.date}
                        onChange={(e) => setBoundTime('to', e.target.value)}
                        className={cn(fieldCls, 'w-24 shrink-0')}
                        title="End time"
                      />
                    </div>
                  </div>
                </div>

                {invalidRange && (
                  <p className="text-[10px] text-red">End must be on or after start.</p>
                )}

                <div className="flex items-center justify-between gap-2 pt-1">
                  <button
                    type="button"
                    className="text-[11px] font-semibold text-text-dim hover:text-text"
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

        <span className="shrink-0 rounded bg-white/6 px-1 py-0.5 text-[9px] font-bold uppercase tracking-wider text-text-dim">
          {zoneLabel}
        </span>
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
