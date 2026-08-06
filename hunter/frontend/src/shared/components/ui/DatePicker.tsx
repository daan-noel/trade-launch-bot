/**
 * Compact single-date control — MUI DatePicker interaction model on our chrome
 * (no MUI dep). Sibling of {@link DateTimeRangePicker} for one civil day.
 *
 * Trigger is input-shaped (calendar icon + `MM/DD/YYYY` / placeholder). Popover:
 * one month pane (‹ › + month/year chooser) · Today · optional Clear. Day click
 * commits immediately. Wire value is `YYYY-MM-DD` (or `''`).
 *
 * `timeZone` drives civil Today only (default: browser IANA). Callers own what
 * midnight means at the query boundary (e.g. prune → local midnight ISO).
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
  browserTimeZone,
  buildMonthCells,
  defaultZoneBadge,
  formatYmdCompact,
  isYmdOutOfBounds,
  todayInZone,
  ymFromDateString,
  type YearMonth,
} from './dateTimeRangePickerUtils';

export interface DatePickerProps {
  value: string;
  onChange: (next: string) => void;
  /** Inclusive lower bound `YYYY-MM-DD`. */
  min?: string;
  /** Inclusive upper bound `YYYY-MM-DD`. */
  max?: string;
  /**
   * IANA zone for civil Today. Default: browser timezone.
   * Does not rewrite the wire value.
   */
  timeZone?: string;
  /**
   * Zone badge on the trigger / popover. Default: hidden (`null`).
   * Pass a string (or omit override after setting `showZoneBadge`) when useful.
   */
  zoneLabel?: string | null;
  /** Show a short zone badge derived from `timeZone`. Default false. */
  showZoneBadge?: boolean;
  emptyLabel?: string;
  clearable?: boolean;
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

function monthLabel({ y, m }: YearMonth): string {
  return `${MONTHS[m]} ${y}`;
}

const fieldCls = fieldClassName({
  size: 'sm',
  type: 'date',
  className:
    'scheme-dark pr-1 [&::-webkit-calendar-picker-indicator]:mr-0 [&::-webkit-calendar-picker-indicator]:opacity-60',
});

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

export function DatePicker({
  value,
  onChange,
  min,
  max,
  timeZone: timeZoneProp,
  zoneLabel: zoneLabelProp,
  showZoneBadge = false,
  emptyLabel = 'Select date',
  clearable = true,
  size = 'sm',
  'aria-label': ariaLabel = 'Date',
  className,
  disabled = false,
}: DatePickerProps) {
  const timeZone = timeZoneProp ?? browserTimeZone();
  const zoneBadge =
    zoneLabelProp === null
      ? null
      : zoneLabelProp !== undefined
        ? zoneLabelProp
        : showZoneBadge
          ? defaultZoneBadge(timeZone)
          : null;

  const menuId = useId();
  const titleId = useId();
  const dateFieldId = useId();
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);
  const [menuPos, setMenuPos] = useState<MenuPosition>({ top: 0, left: 0 });
  const [chooserOpen, setChooserOpen] = useState(false);
  const [draftDate, setDraftDate] = useState(value);

  const today = todayInZone(timeZone);
  const seedYm = ymFromDateString(value) ?? today.ym;
  const [view, setView] = useState<YearMonth>(seedYm);

  const display = formatYmdCompact(value);
  const sizeCls =
    size === 'sm'
      ? 'min-h-7 px-2 py-1 pr-1.5 text-[11px]'
      : 'min-h-8 px-2.5 py-1.5 pr-2 text-[13px]';

  const cells = useMemo(() => buildMonthCells(view.y, view.m), [view.y, view.m]);

  const syncFromValue = useCallback(() => {
    setDraftDate(value);
    setView(ymFromDateString(value) ?? todayInZone(timeZone).ym);
    setChooserOpen(false);
  }, [value, timeZone]);

  const updateMenuPosition = useCallback(() => {
    const el = triggerRef.current;
    const menu = menuRef.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    const menuW = menu?.offsetWidth ?? 240;
    const menuH = menu?.offsetHeight ?? 320;
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
      ? Math.max(200, rect.top - MENU_GAP_PX - MENU_EDGE_PX)
      : Math.max(200, spaceBelow);
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
  }, [open, updateMenuPosition, chooserOpen]);

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
      if (chooserOpen) {
        setChooserOpen(false);
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
  }, [open, chooserOpen]);

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
    syncFromValue();
    setOpen(true);
  };

  const commit = (ymd: string) => {
    if (ymd && isYmdOutOfBounds(ymd, min, max)) return;
    onChange(ymd);
    setOpen(false);
  };

  const pickDay = (ymd: string) => {
    if (isYmdOutOfBounds(ymd, min, max)) return;
    setDraftDate(ymd);
    commit(ymd);
  };

  const goToToday = () => {
    const { ymd, ym } = todayInZone(timeZone);
    setChooserOpen(false);
    setView(ym);
    if (!isYmdOutOfBounds(ymd, min, max)) {
      setDraftDate(ymd);
    }
  };

  const title = display
    ? `${display}${zoneBadge ? ` (${zoneBadge})` : ''}`
    : `${emptyLabel}${zoneBadge ? ` (${zoneBadge})` : ''}`;

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
              Pick date
            </span>
            <div className="flex items-center gap-1.5">
              <button
                type="button"
                onClick={goToToday}
                title={`Jump to today (${zoneBadge ?? timeZone})`}
                className="rounded-md border border-primary/30 bg-primary/10 px-2 py-0.5 text-[10px] font-semibold text-primary hover:bg-primary/20"
              >
                Today
              </button>
              {zoneBadge && (
                <span className="rounded bg-white/6 px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-text-dim">
                  {zoneBadge}
                </span>
              )}
            </div>
          </div>

          <div className="flex flex-col gap-2 p-3">
            <div className="flex items-center gap-1">
              <input
                id={dateFieldId}
                type="date"
                value={draftDate}
                min={min}
                max={max}
                onChange={(e) => {
                  const next = e.target.value;
                  setDraftDate(next);
                  if (next) {
                    const ym = ymFromDateString(next);
                    if (ym) setView(ym);
                  }
                }}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' && draftDate) {
                    e.preventDefault();
                    commit(draftDate);
                  }
                }}
                className={cn(fieldCls, 'min-w-0 flex-1')}
                aria-label={`${ariaLabel} value`}
              />
              <Button
                size="sm"
                variant="primary"
                disabled={!draftDate || isYmdOutOfBounds(draftDate, min, max)}
                onClick={() => commit(draftDate)}
              >
                Apply
              </Button>
            </div>

            <div className="w-[196px]">
              <div className="mb-1 flex w-[196px] items-center justify-between gap-0.5">
                <button
                  type="button"
                  aria-label="Previous month"
                  onClick={() => {
                    setChooserOpen(false);
                    setView((v) => addMonths(v, -1));
                  }}
                  className="rounded px-1.5 py-0.5 text-text-dim hover:bg-white/6 hover:text-text"
                >
                  ‹
                </button>
                <button
                  type="button"
                  aria-label="Choose month and year"
                  aria-expanded={chooserOpen}
                  onClick={() => setChooserOpen((c) => !c)}
                  className={cn(
                    'rounded px-1.5 py-0.5 text-[12px] font-semibold transition-colors',
                    chooserOpen ? 'bg-primary/15 text-primary' : 'text-text hover:bg-white/6',
                  )}
                >
                  {monthLabel(view)}
                  <span className="ml-1 text-[9px] opacity-60">
                    {chooserOpen ? '▴' : '▾'}
                  </span>
                </button>
                <button
                  type="button"
                  aria-label="Next month"
                  onClick={() => {
                    setChooserOpen(false);
                    setView((v) => addMonths(v, 1));
                  }}
                  className="rounded px-1.5 py-0.5 text-text-dim hover:bg-white/6 hover:text-text"
                >
                  ›
                </button>
              </div>

              {chooserOpen ? (
                <MonthYearChooser
                  view={view}
                  onPick={(next) => {
                    setView(next);
                    setChooserOpen(false);
                  }}
                />
              ) : (
                <>
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
                      const selected = ymd === (draftDate || value);
                      const isToday = ymd === today.ymd;
                      const out = isYmdOutOfBounds(ymd, min, max);
                      return (
                        <button
                          key={c.key}
                          type="button"
                          disabled={out}
                          onClick={() => pickDay(ymd)}
                          title={isToday ? 'Today' : undefined}
                          className={cn(
                            'relative flex size-7 items-center justify-center rounded text-[11px] tabular-nums transition-colors',
                            out && 'cursor-not-allowed opacity-30',
                            !out &&
                              !selected &&
                              'text-text-mid hover:bg-white/8 hover:text-text',
                            selected && 'bg-primary font-semibold text-bg-panel',
                            isToday &&
                              !selected &&
                              'font-semibold text-primary ring-1 ring-inset ring-primary/55',
                            isToday && selected && 'ring-1 ring-inset ring-white/70',
                          )}
                        >
                          {c.day}
                        </button>
                      );
                    })}
                  </div>
                </>
              )}
            </div>

            <div className="flex items-center justify-between gap-2 border-t border-white/6 pt-2">
              {clearable ? (
                <button
                  type="button"
                  className="text-[11px] font-semibold text-text-dim hover:text-text"
                  onClick={() => commit('')}
                >
                  Clear
                </button>
              ) : (
                <span />
              )}
              <Button size="sm" variant="ghost" onClick={() => setOpen(false)}>
                Cancel
              </Button>
            </div>
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
          'inline-flex w-auto max-w-56 cursor-pointer items-center gap-1.5 rounded-md border font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-50',
          sizeCls,
          open
            ? 'border-primary/50 bg-primary/10 text-primary'
            : value
              ? 'border-primary/35 bg-primary/8 text-primary'
              : 'border-white/12 bg-white/5 text-text hover:border-white/22 hover:bg-white/7',
          className,
        )}
      >
        <CalendarIcon
          className={cn(
            'size-3.5 shrink-0',
            open || value ? 'text-primary' : 'text-text-dim',
          )}
        />
        <span
          className={cn(
            'min-w-0 truncate tabular-nums',
            display ? 'font-semibold' : 'text-text-dim/70',
          )}
        >
          {display || emptyLabel}
        </span>
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
