import { memo, useMemo } from 'react';
import { cn } from 'lib/cn';
import { formatDecimalTrim } from 'utils/format';
import { formatSigned, signedToneClass } from 'lib/signedTone';
import { CHART_COLORS } from 'components/token-price-chart/constants';
import { datetimeLocalToUtcWallClock } from 'utils/date';
import {
  dowHourInTz,
  monthAbbr,
  shiftDayKey,
  summarizeDailyPnl,
  type PnlDay,
} from './pnlSeries';

interface PnlCalendarProps {
  days: PnlDay[];
  /** How many trailing weeks to render (a GitHub-style contribution grid). */
  weeks?: number;
  /** `YYYY-MM-DD` of "today" in the caller's timezone — the grid's right edge.
   *  Passed in rather than read from the clock so the grid is stable across
   *  re-renders and testable. */
  todayKey: string;
  /** IANA timezone the day keys are expressed in (must match `buildDailyPnl`). */
  timeZone: string;
  /** Currently focused day (`YYYY-MM-DD`), if any. */
  selectedDay?: string | null;
  onSelectDay?: (day: string) => void;
  /** Currently focused week, keyed by its **Sunday** `YYYY-MM-DD`. */
  selectedWeek?: string | null;
  /** Clicking a column header focuses that whole week. Omit to make the header
   *  a plain month axis. */
  onSelectWeek?: (weekStart: string) => void;
  emptyMessage?: string;
}

function withAlpha(hex: string, alpha: number): string {
  const r = parseInt(hex.slice(1, 3), 16);
  const g = parseInt(hex.slice(3, 5), 16);
  const b = parseInt(hex.slice(5, 7), 16);
  return `rgba(${r},${g},${b},${alpha.toFixed(3)})`;
}

function dayColor(pnl: number | undefined, maxAbs: number): string {
  if (pnl == null) return 'rgba(255,255,255,0.02)';
  if (pnl === 0) return 'rgba(255,255,255,0.06)';
  const norm = maxAbs > 0 ? Math.min(1, Math.abs(pnl) / maxAbs) : 0;
  return withAlpha(pnl > 0 ? CHART_COLORS.up : CHART_COLORS.down, 0.12 + 0.78 * norm);
}

/**
 * Border brightness carries **how many** trades made the day, since the fill
 * only carries sign and size. Without it a day that got lucky once is
 * indistinguishable from one that ground out the same SOL over forty closes —
 * the same conflation the wallet work kept running into.
 * Square-rooted so a single blowout day doesn't flatten every ordinary one.
 */
function countBorder(count: number, maxCount: number): string {
  if (count === 0) return 'rgba(255,255,255,0.05)';
  const norm = maxCount > 1 ? Math.sqrt(count / maxCount) : 1;
  return `rgba(255,255,255,${(0.1 + 0.45 * norm).toFixed(3)})`;
}

/**
 * Daily realized PnL as a trailing-weeks calendar grid — "which dates made money
 * and which bled". Cells stretch to fill the card width so the grid stays
 * usable on a wide History section (not a fixed 16px contribution-graph).
 *
 * The column axis is **months**, not weekdays: this sits beside the day×hour
 * heatmap, which already answers "which weekday", and a M/W/F gutter cost width
 * without telling you which dates you were looking at. Rows still run Sun (top)
 * → Sat (bottom); weekend cells carry a dashed outline to keep that readable.
 */
export const PnlCalendar = memo(function PnlCalendar({
  days,
  weeks = 14,
  todayKey,
  timeZone,
  selectedDay = null,
  onSelectDay,
  selectedWeek = null,
  onSelectWeek,
  emptyMessage = 'No closed trades in this window.',
}: PnlCalendarProps) {
  const { columns, maxAbs, maxCount, total, summary } = useMemo(() => {
    const byDay = new Map(days.map((d) => [d.day, d]));
    const max = days.reduce((m, d) => Math.max(m, Math.abs(d.pnlSol)), 0);
    const maxN = days.reduce((m, d) => Math.max(m, d.count), 0);

    const todayNoonUtc = datetimeLocalToUtcWallClock(`${todayKey}T12:00:00`, timeZone, 'lower');
    const todayDow = dowHourInTz(Date.parse(`${todayNoonUtc}Z`), timeZone).dow;
    const endKey = shiftDayKey(todayKey, 6 - todayDow);
    const startKey = shiftDayKey(endKey, -(weeks * 7 - 1));

    const cols: {
      key: string;
      /** Sunday of this column — the week focus key. */
      weekStart: string;
      /** Month abbreviation when this column opens a new month, else `''`. */
      monthLabel: string;
      pnlSol: number;
      count: number;
      cells: { day: string; data: PnlDay | undefined; future: boolean }[];
    }[] = [];
    for (let w = 0; w < weeks; w++) {
      const cells: { day: string; data: PnlDay | undefined; future: boolean }[] = [];
      let pnlSol = 0;
      let count = 0;
      for (let d = 0; d < 7; d++) {
        const key = shiftDayKey(startKey, w * 7 + d);
        const data = byDay.get(key);
        if (data) {
          pnlSol += data.pnlSol;
          count += data.count;
        }
        cells.push({ day: key, data, future: key > todayKey });
      }
      const weekStart = cells[0]!.day;
      // Label a column when it opens a month — either its Sunday is the 1st, or
      // the 1st falls inside it (the usual case, since months rarely start Sun).
      const monthStartInside = cells.some((c) => c.day.slice(8) === '01');
      cols.push({
        key: `w${w}`,
        weekStart,
        monthLabel: monthStartInside || w === 0 ? monthAbbr(cells[6]!.day) : '',
        pnlSol,
        count,
        cells,
      });
    }
    // Summarize only the window actually drawn, so the strip and the grid agree.
    const inWindow = days.filter((d) => d.day >= startKey && d.day <= todayKey);
    return {
      columns: cols,
      maxAbs: max,
      maxCount: maxN,
      total: days.length,
      summary: summarizeDailyPnl(inWindow),
    };
  }, [days, weeks, todayKey, timeZone]);

  if (total === 0) {
    return <p className="text-xs text-text-dim">{emptyMessage}</p>;
  }

  const greenPct =
    summary.tradedDays > 0 ? (summary.greenDays / summary.tradedDays) * 100 : null;
  const gridCols = { gridTemplateColumns: `repeat(${weeks}, minmax(0, 1fr))` } as const;

  return (
    <div className="flex w-full min-w-0 flex-col gap-1">
      {/* Month axis — doubles as the week-focus row when `onSelectWeek` is given. */}
      <div className="grid min-w-0 gap-1 text-[10px] text-text-dim" style={gridCols}>
        {columns.map((col) => {
          const selected = selectedWeek === col.weekStart;
          const clickable = !!onSelectWeek && col.count > 0;
          return (
            <button
              key={col.key}
              type="button"
              disabled={!clickable}
              onClick={() => onSelectWeek?.(col.weekStart)}
              title={
                `Week of ${col.weekStart}\n` +
                (col.count > 0
                  ? `${col.pnlSol >= 0 ? '+' : ''}${formatDecimalTrim(col.pnlSol, 3)} SOL · ${col.count} trade${col.count === 1 ? '' : 's'}`
                  : 'No trades') +
                (clickable ? '\nClick to focus table on this week' : '')
              }
              className={cn(
                'min-w-0 truncate rounded-sm px-0.5 text-left leading-tight',
                clickable && 'cursor-pointer hover:bg-white/8 hover:text-text',
                !clickable && 'cursor-default',
                selected && 'bg-primary/15 font-semibold text-text ring-1 ring-primary/50',
              )}
            >
              {col.monthLabel || ' '}
            </button>
          );
        })}
      </div>

      <div className="grid min-h-52 min-w-0 gap-1" style={gridCols}>
        {columns.map((col) => (
          <div
            key={col.key}
            className={cn(
              'flex min-h-0 min-w-0 flex-col gap-1',
              selectedWeek === col.weekStart && 'rounded-sm bg-primary/8 ring-1 ring-primary/40',
            )}
          >
            {col.cells.map((cell, dow) => {
              const pnl = cell.data?.pnlSol;
              const selected = selectedDay === cell.day;
              const isToday = cell.day === todayKey;
              const weekend = dow === 0 || dow === 6;
              const title = cell.future
                ? cell.day
                : `${cell.day}${isToday ? ' (today)' : ''}\n${
                    cell.data
                      ? `${pnl! >= 0 ? '+' : ''}${formatDecimalTrim(pnl!, 3)} SOL · ${cell.data.count} trade${
                          cell.data.count === 1 ? '' : 's'
                        } · ${cell.data.wins}W`
                      : 'No trades'
                  }${onSelectDay && cell.data ? '\nClick to focus table' : ''}`;
              const pnlLabel =
                cell.data && pnl != null
                  ? `${pnl >= 0 ? '+' : ''}${formatDecimalTrim(pnl, Math.abs(pnl) >= 1 ? 1 : 2)}`
                  : '';
              return (
                <button
                  key={cell.day}
                  type="button"
                  title={title}
                  disabled={cell.future || !cell.data}
                  onClick={() => cell.data && onSelectDay?.(cell.day)}
                  className={cn(
                    'flex min-h-7 w-full flex-1 items-center justify-center rounded-sm border px-0.5 text-center font-mono text-[9px] leading-none tabular-nums text-white/90',
                    weekend ? 'border-dashed' : 'border-solid',
                    cell.future && 'opacity-20',
                    // `!` is load-bearing: the border colour is an inline style
                    // (count-encoded), and only an important rule outranks it.
                    cell.data && onSelectDay && 'cursor-pointer hover:border-white/60!',
                    isToday && 'ring-1 ring-white/50',
                    selected && 'ring-2 ring-primary ring-offset-1 ring-offset-bg-panel',
                  )}
                  style={{
                    background: cell.future ? 'transparent' : dayColor(pnl, maxAbs),
                    borderColor: cell.future
                      ? 'rgba(255,255,255,0.05)'
                      : countBorder(cell.data?.count ?? 0, maxCount),
                  }}
                >
                  <span className="max-w-full truncate">{pnlLabel}</span>
                </button>
              );
            })}
          </div>
        ))}
      </div>

      {/* The reads a colour wash can't give: how often green, the two extremes,
          and how long the worst bleed ran. */}
      <div className="flex flex-wrap items-baseline gap-x-3 gap-y-0.5 pt-0.5 text-[10px] text-text-dim">
        <span title="Days with at least one close that finished green">
          <span className={cn('font-semibold tabular-nums', signedToneClass(greenPct != null ? greenPct - 50 : null))}>
            {summary.greenDays}
          </span>
          /{summary.tradedDays} green days
          {greenPct != null && <span className="ml-1 tabular-nums">({greenPct.toFixed(0)}%)</span>}
        </span>
        {summary.best && (
          <span title={`Best day: ${summary.best.day} over ${summary.best.count} trades`}>
            best{' '}
            <span className="font-mono tabular-nums text-green">
              {formatSigned(summary.best.pnlSol, 2)} ◎
            </span>{' '}
            <span className="text-text-dim/60">{summary.best.day.slice(5)}</span>
          </span>
        )}
        {summary.worst && (
          <span title={`Worst day: ${summary.worst.day} over ${summary.worst.count} trades`}>
            worst{' '}
            <span className="font-mono tabular-nums text-red">
              {formatSigned(summary.worst.pnlSol, 2)} ◎
            </span>{' '}
            <span className="text-text-dim/60">{summary.worst.day.slice(5)}</span>
          </span>
        )}
        {summary.longestRedStreak > 1 && (
          <span title="Longest run of consecutive trading days in the red (no-trade days don't break the run)">
            worst streak{' '}
            <span className="font-semibold tabular-nums text-red">
              {summary.longestRedStreak}
            </span>{' '}
            red days
          </span>
        )}
      </div>
    </div>
  );
});
