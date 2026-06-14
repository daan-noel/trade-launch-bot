import { useMemo } from 'react';
import { DataTable } from 'components/table/DataTable';
import { tradeColumns } from 'components/transactions/tradeColumns';
import { usePriceDisplay } from 'hooks/usePriceDisplay';
import { useTradeStream } from 'hooks/useTradeStream';

export function DashboardPage() {
  const price = usePriceDisplay();
  const events = useTradeStream();
  // Key on the unit label only: the rate-dependent cells read the rate from
  // context, so the column array stays stable across USD-rate ticks.
  const columns = useMemo(() => tradeColumns(price.unitLabel), [price.unitLabel]);

  return (
    <div>
      <div className="mb-3.5 flex items-center gap-2.5">
        <h2 className="text-base font-bold text-primary">Live Trades</h2>
        <span className="flex items-center gap-1.5 rounded border border-primary/30 bg-primary/12 px-2 py-0.5 text-[10px] font-bold tracking-wider text-primary">
          <span className="size-1.5 animate-pulse rounded-full bg-primary" />
          LIVE
        </span>
      </div>

      {events.length === 0 ? (
        <p className="text-text-dim">Waiting for live trades from stream…</p>
      ) : (
        <DataTable
          columns={columns}
          rows={events}
          rowKey={(ev) => `${ev.tx_signature}-${ev.slot}`}
          searchable={false}
          colFilters={false}
          selectable={false}
          emptyMessage="Waiting for live trades…"
        />
      )}
    </div>
  );
}
