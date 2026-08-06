import { memo, useMemo } from 'react';
import { cn } from 'lib/cn';
import { formatDecimalTrim } from 'utils/format';
import { CHART_COLORS } from 'components/token-price-chart/constants';
import type { PnlDay } from './pnlSeries';

interface PnlCalendarProps {
  days: PnlDay[];
  /** How many trailing weeks to render (a GitHub-style contribution grid). */
  weeks?: number;
  /** `YYYY-MM-DD` of "today" in the caller's timezone — the grid's right edge.
   *  Passed in rather than read from the clock so the grid is stable across
   *  re-renders and testable. */
  todayKey: string;
  onSelectDay?: (day: string) => void;
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

const DOW_LABELS = ['S', 'M', 'T', 'W', 'T', 'F', 'S'];

/** `YYYY-MM-DD` → the UTC-noon instant (noon dodges any DST edge). */
function dayKeyToMs(key: string): number {
  return Date.parse(`${key}T12:00:00Z`);
}
function msToDayKey(ms: number): string {
  return new Date(ms).toISOString().slice(0, 10);
}

/**
 * Daily realized PnL as a trailing-weeks calendar grid — "which days made money
 * and which bled". Green/red wash by sign, intensity by magnitude, on the same
 * SSOT candle palette as the heatmap beside it. Pure CSS grid, static per
 * cohort.
 */
export const PnlCalendar = memo(function PnlCalendar({
  days,
  weeks = 14,
  todayKey,
  onSelectDay,
  emptyMessage = 'No closed trades in this window.',
}: PnlCalendarProps) {
  const { columns, maxAbs, total } = useMemo(() => {
    const byDay = new Map(days.map((d) => [d.day, d]));
    const max = days.reduce((m, d) => Math.max(m, Math.abs(d.pnlSol)), 0);

    const todayMs = dayKeyToMs(todayKey);
    // End the grid on the Saturday of the current week so columns are whole
    // weeks and the weekday rows line up.
    const todayDow = new Date(todayMs).getUTCDay();
    const endMs = todayMs + (6 - todayDow) * 86_400_000;
    const startMs = endMs - (weeks * 7 - 1) * 86_400_000;

    const cols: { key: string; cells: { day: string; data: PnlDay | undefined; future: boolean }[] }[] =
      [];
    for (let w = 0; w < weeks; w++) {
      const cells: { day: string; data: PnlDay | undefined; future: boolean }[] = [];
      for (let d = 0; d < 7; d++) {
        const ms = startMs + (w * 7 + d) * 86_400_000;
        const key = msToDayKey(ms);
        cells.push({ day: key, data: byDay.get(key), future: key > todayKey });
      }
      cols.push({ key: `w${w}`, cells });
    }
    return { columns: cols, maxAbs: max, total: days.length };
  }, [days, weeks, todayKey]);

  if (total === 0) {
    return <p className="text-xs text-text-dim">{emptyMessage}</p>;
  }

  return (
    <div className="flex gap-1 overflow-x-auto">
      <div className="flex flex-col gap-px pt-px text-[9px] text-text-dim">
        {DOW_LABELS.map((l, i) => (
          <div key={i} className="flex h-4 items-center justify-end pr-1">
            {i % 2 === 1 ? l : ''}
          </div>
        ))}
      </div>
      <div className="flex gap-px">
        {columns.map((col) => (
          <div key={col.key} className="flex flex-col gap-px">
            {col.cells.map((cell) => {
              const pnl = cell.data?.pnlSol;
              const title = cell.future
                ? cell.day
                : `${cell.day}\n${
                    cell.data
                      ? `${pnl! >= 0 ? '+' : ''}${formatDecimalTrim(pnl!, 3)} SOL · ${cell.data.count} trade${
                          cell.data.count === 1 ? '' : 's'
                        } · ${cell.data.wins}W`
                      : 'No trades'
                  }`;
              return (
                <button
                  key={cell.day}
                  type="button"
                  title={title}
                  disabled={cell.future || !cell.data}
                  onClick={() => cell.data && onSelectDay?.(cell.day)}
                  className={cn(
                    'h-4 w-4 rounded-[2px] border border-white/5',
                    cell.future && 'opacity-20',
                    cell.data && onSelectDay && 'cursor-pointer hover:border-white/40',
                  )}
                  style={{ background: cell.future ? 'transparent' : dayColor(pnl, maxAbs) }}
                />
              );
            })}
          </div>
        ))}
      </div>
    </div>
  );
});
