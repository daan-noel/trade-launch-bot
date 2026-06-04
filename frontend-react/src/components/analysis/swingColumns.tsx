import type { ColumnDef } from '../table/types';
import type { SwingLegRecord } from '../../types';
import { formatDecimalTrim } from '../../utils/format';
import type { usePriceDisplay } from '../../hooks/usePriceDisplay';
import { cn } from '../../lib/cn';

function formatMs(ms: number): string {
  const d = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${d.getUTCFullYear()}-${pad(d.getUTCMonth() + 1)}-${pad(d.getUTCDate())} ${pad(d.getUTCHours())}:${pad(d.getUTCMinutes())}:${pad(d.getUTCSeconds())}`;
}

export function swingColumns(
  price: ReturnType<typeof usePriceDisplay>,
): ColumnDef<SwingLegRecord>[] {
  const unit = price.unitLabel;

  return [
    {
      key: 'type',
      label: 'Type',
      render: (leg) => {
        const isHigh = leg.type === 'swing_high';
        return (
          <span
            className={cn(
              'inline-block rounded px-2 py-0.5 text-[11px] font-bold tracking-wide',
              isHigh
                ? 'border border-primary bg-primary/15 text-primary'
                : 'border border-red bg-red/15 text-red',
            )}
          >
            {isHigh ? 'SWING HIGH' : 'SWING LOW'}
          </span>
        );
      },
      sortValue: (leg) => leg.type,
      searchValue: (leg) => leg.type,
    },
    {
      key: 'start_at',
      label: 'Start (UTC)',
      render: (leg) => <span className="font-mono text-[11px]">{formatMs(leg.start_at)}</span>,
      sortValue: (leg) => leg.start_at,
      searchValue: (leg) => formatMs(leg.start_at),
    },
    {
      key: 'end_at',
      label: 'End (UTC)',
      render: (leg) => <span className="font-mono text-[11px]">{formatMs(leg.end_at)}</span>,
      sortValue: (leg) => leg.end_at,
      searchValue: (leg) => formatMs(leg.end_at),
    },
    {
      key: 'duration_ms',
      label: 'Duration',
      render: (leg) => {
        const sec = leg.duration_ms / 1000;
        if (sec < 60) return `${formatDecimalTrim(sec, 1)}s`;
        if (sec < 3600) return `${formatDecimalTrim(sec / 60, 1)}m`;
        return `${formatDecimalTrim(sec / 3600, 1)}h`;
      },
      sortValue: (leg) => leg.duration_ms,
      searchValue: (leg) => String(leg.duration_ms),
    },
    {
      key: 'start_price',
      label: `Start (${unit})`,
      render: (leg) => price.displayPrice(leg.start_price),
      sortValue: (leg) => leg.start_price,
      searchValue: (leg) => String(leg.start_price),
    },
    {
      key: 'end_price',
      label: `End (${unit})`,
      render: (leg) => price.displayPrice(leg.end_price),
      sortValue: (leg) => leg.end_price,
      searchValue: (leg) => String(leg.end_price),
    },
    {
      key: 'inflow',
      label: `Inflow (${unit})`,
      render: (leg) => price.displayAmount(leg.inflow),
      sortValue: (leg) => leg.inflow,
      searchValue: (leg) => String(leg.inflow),
    },
    {
      key: 'outflow',
      label: `Outflow (${unit})`,
      render: (leg) => price.displayAmount(leg.outflow),
      sortValue: (leg) => leg.outflow,
      searchValue: (leg) => String(leg.outflow),
    },
    {
      key: 'net_flow',
      label: `Net (${unit})`,
      render: (leg) => (
        <span className={leg.net_flow >= 0 ? 'text-primary' : 'text-red'}>
          {price.displayAmount(leg.net_flow)}
        </span>
      ),
      sortValue: (leg) => leg.net_flow,
      searchValue: (leg) => String(leg.net_flow),
    },
    {
      key: 'trade_count',
      label: 'Trades',
      render: (leg) => leg.trade_count,
      sortValue: (leg) => leg.trade_count,
      searchValue: (leg) => String(leg.trade_count),
    },
  ];
}
