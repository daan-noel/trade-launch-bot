import { useMemo } from 'react';
import { DataTable } from '../../components/table/DataTable';
import { tradeColumns } from '../../components/transactions/tradeColumns';
import { usePriceDisplay } from '../../hooks/usePriceDisplay';
import { useTradeStream } from '../../hooks/useTradeStream';

export function TransactionsPage() {
  const price = usePriceDisplay();
  const events = useTradeStream();
  const columns = useMemo(() => tradeColumns(price), [price]);

  return (
    <div>
      <div className="mb-3.5 flex items-center gap-2.5">
        <h2 className="text-base font-bold text-primary">Live Transactions</h2>
        <span className="rounded-md border border-primary bg-primary/15 px-2.5 py-0.5 font-mono text-[11px] font-bold tracking-wide text-primary">
          {events.length} captured
        </span>
      </div>

      {events.length === 0 ? (
        <p className="text-text-dim">Waiting for live trades from stream…</p>
      ) : (
        <DataTable
          columns={columns}
          rows={events}
          rowKey={(ev) => `${ev.tx_signature}-${ev.slot}`}
          defaultPageSize={25}
          searchable
          colFilters
          hoverable
          emptyMessage="Waiting for live trades…"
        />
      )}
    </div>
  );
}
