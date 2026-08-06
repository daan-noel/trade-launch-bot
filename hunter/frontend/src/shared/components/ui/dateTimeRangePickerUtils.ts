/**
 * Pure helpers for {@link DateTimeRangePicker}. Wire values are bare wall-clock
 * `YYYY-MM-DDTHH:mm` (same as `datetime-local`).
 */

export const DT_RE = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})/;

export type YearMonth = { y: number; m: number };

export type RangeDayRole =
  | 'outside'
  | 'start'
  | 'end'
  | 'single'
  | 'middle'
  | 'preview-middle'
  | 'preview-end';

export function pad2(n: number): string {
  return String(n).padStart(2, '0');
}

export function splitDt(v: string): { date: string; time: string } {
  if (!v) return { date: '', time: '' };
  const m = DT_RE.exec(v);
  if (!m) return { date: '', time: '' };
  return { date: `${m[1]}-${m[2]}-${m[3]}`, time: `${m[4]}:${m[5]}` };
}

export function joinDt(date: string, time: string): string {
  if (!date) return '';
  return `${date}T${time || '00:00'}`;
}

export function ymdKey(y: number, m: number, d: number): string {
  return `${y}-${pad2(m + 1)}-${pad2(d)}`;
}

export function parseYmd(key: string): (YearMonth & { d: number }) | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(key);
  if (!m) return null;
  return { y: Number(m[1]), m: Number(m[2]) - 1, d: Number(m[3]) };
}

export function monthIndex({ y, m }: YearMonth): number {
  return y * 12 + m;
}

export function addMonths({ y, m }: YearMonth, delta: number): YearMonth {
  const idx = y * 12 + m + delta;
  return { y: Math.floor(idx / 12), m: ((idx % 12) + 12) % 12 };
}

export function daysInMonth(y: number, m: number): number {
  return new Date(Date.UTC(y, m + 1, 0)).getUTCDate();
}

export function startDow(y: number, m: number): number {
  return new Date(Date.UTC(y, m, 1)).getUTCDay();
}

export function formatCompact(dt: string): string {
  const { date, time } = splitDt(dt);
  if (!date) return '';
  const [, mo, da] = date.split('-');
  return `${mo}/${da} ${time}`;
}

export function ymFromDateString(date: string): YearMonth | null {
  const parsed = date ? parseYmd(date) : null;
  return parsed ? { y: parsed.y, m: parsed.m } : null;
}

/** Display `YYYY-MM-DD` as `MM/DD/YYYY`. */
export function formatYmdCompact(ymd: string): string {
  const p = parseYmd(ymd);
  if (!p) return '';
  return `${pad2(p.m + 1)}/${pad2(p.d)}/${p.y}`;
}

/** Browser IANA zone (fallback UTC) — default for date-only pickers. */
export function browserTimeZone(): string {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC';
  } catch {
    return 'UTC';
  }
}

export type MonthCell = { key: string; day: number | null; ymd: string | null };

/** Calendar cells for one month (leading/trailing blanks). */
export function buildMonthCells(y: number, m: number): MonthCell[] {
  const dim = daysInMonth(y, m);
  const lead = startDow(y, m);
  const out: MonthCell[] = [];
  for (let i = 0; i < lead; i++) out.push({ key: `b${i}`, day: null, ymd: null });
  for (let d = 1; d <= dim; d++) {
    const ymd = ymdKey(y, m, d);
    out.push({ key: ymd, day: d, ymd });
  }
  while (out.length % 7 !== 0) {
    out.push({ key: `t${out.length}`, day: null, ymd: null });
  }
  return out;
}

/** Inclusive min/max gate for a civil day. */
export function isYmdOutOfBounds(ymd: string, min?: string, max?: string): boolean {
  if (min && ymd < min) return true;
  if (max && ymd > max) return true;
  return false;
}

/** Clamp a native `time` value to wire `HH:mm` (strip seconds). */
export function normalizeTime(time: string): string {
  if (!time) return '';
  const m = /^(\d{2}):(\d{2})/.exec(time);
  return m ? `${m[1]}:${m[2]}` : time;
}

/** Compact trigger/popover badge from an IANA id (`UTC`, else city segment). */
export function defaultZoneBadge(timeZone: string): string {
  if (timeZone === 'UTC' || timeZone === 'Etc/UTC' || timeZone === 'Etc/GMT') return 'UTC';
  const slash = timeZone.lastIndexOf('/');
  const segment = slash >= 0 ? timeZone.slice(slash + 1) : timeZone;
  return segment.replace(/_/g, ' ');
}

/**
 * Civil "today" in an IANA zone (or UTC). Drives the Today jump + today ring so
 * they match the wall-clock the picker is editing.
 */
export function todayInZone(timeZone = 'UTC'): { ymd: string; ym: YearMonth } {
  if (timeZone === 'UTC' || timeZone === 'Etc/UTC' || timeZone === 'Etc/GMT') {
    const n = new Date();
    const ym = { y: n.getUTCFullYear(), m: n.getUTCMonth() };
    return { ymd: ymdKey(ym.y, ym.m, n.getUTCDate()), ym };
  }
  try {
    const parts = new Intl.DateTimeFormat('en-US', {
      timeZone,
      year: 'numeric',
      month: 'numeric',
      day: 'numeric',
    }).formatToParts(new Date());
    const y = Number(parts.find((p) => p.type === 'year')?.value);
    const m = Number(parts.find((p) => p.type === 'month')?.value) - 1;
    const d = Number(parts.find((p) => p.type === 'day')?.value);
    if (Number.isFinite(y) && Number.isFinite(m) && Number.isFinite(d)) {
      return { ymd: ymdKey(y, m, d), ym: { y, m } };
    }
  } catch {
    /* fall through to UTC */
  }
  return todayInZone('UTC');
}

/** Seed left/right panes from bounds (or today in `timeZone` / today+1). */
export function initialViews(
  from: string,
  to: string,
  timeZone = 'UTC',
): { left: YearMonth; right: YearMonth } {
  const today = todayInZone(timeZone).ym;
  const left = ymFromDateString(splitDt(from).date) ?? today;
  const rightRaw = ymFromDateString(splitDt(to).date) ?? addMonths(left, 1);
  const right = monthIndex(rightRaw) < monthIndex(left) ? left : rightRaw;
  return { left, right };
}

/**
 * Resolve the visible [lo, hi] while picking — committed bounds, or a hover
 * preview when the end date is still being chosen (MUI range hover).
 */
export function resolvePreviewBounds(
  fromDate: string,
  toDate: string,
  picking: 'from' | 'to',
  hoverYmd: string | null,
): { lo: string; hi: string; previewing: boolean } {
  if (picking === 'to' && fromDate && hoverYmd && !toDate) {
    if (hoverYmd < fromDate) return { lo: hoverYmd, hi: fromDate, previewing: true };
    if (hoverYmd > fromDate) return { lo: fromDate, hi: hoverYmd, previewing: true };
    return { lo: fromDate, hi: fromDate, previewing: true };
  }
  if (fromDate && toDate) {
    return fromDate <= toDate
      ? { lo: fromDate, hi: toDate, previewing: false }
      : { lo: toDate, hi: fromDate, previewing: false };
  }
  if (fromDate) return { lo: fromDate, hi: fromDate, previewing: false };
  if (toDate) return { lo: toDate, hi: toDate, previewing: false };
  return { lo: '', hi: '', previewing: false };
}

/** Day cell role for committed + hover-preview range painting. */
export function rangeDayRole(
  ymd: string,
  fromDate: string,
  toDate: string,
  picking: 'from' | 'to',
  hoverYmd: string | null,
): RangeDayRole {
  const { lo, hi, previewing } = resolvePreviewBounds(fromDate, toDate, picking, hoverYmd);
  if (!lo) return 'outside';

  if (!previewing) {
    if (ymd === lo && ymd === hi) return 'single';
    if (ymd === lo) return 'start';
    if (ymd === hi) return 'end';
    if (ymd > lo && ymd < hi) return 'middle';
    return 'outside';
  }

  // Preview uses ordered [lo, hi] so hovering before start still paints L→R.
  if (ymd === lo && ymd === hi) return 'single';
  if (ymd === lo) return 'start';
  if (ymd === hi) return 'preview-end';
  if (ymd > lo && ymd < hi) return 'preview-middle';
  return 'outside';
}

/**
 * Apply a calendar day click (MUI: first click = start, second = end;
 * clicking again after a complete range restarts at start).
 */
export function applyDayPick(
  draft: { from: string; to: string; picking: 'from' | 'to' },
  ymd: string,
  customPreset: string,
): { preset: string; from: string; to: string; picking: 'from' | 'to' } {
  const fromDate = splitDt(draft.from).date;
  const toDate = splitDt(draft.to).date;

  if (draft.picking === 'from' || (fromDate && toDate)) {
    const time = splitDt(draft.from).time || '00:00';
    return {
      preset: customPreset,
      from: joinDt(ymd, time),
      to: '',
      picking: 'to',
    };
  }

  const fromTime = splitDt(draft.from).time || '00:00';
  const toTime = splitDt(draft.to).time || '23:59';
  if (ymd < fromDate) {
    return {
      preset: customPreset,
      from: joinDt(ymd, fromTime),
      to: joinDt(fromDate, toTime),
      picking: 'from',
    };
  }
  return {
    preset: customPreset,
    from: draft.from,
    to: joinDt(ymd, toTime),
    picking: 'from',
  };
}
