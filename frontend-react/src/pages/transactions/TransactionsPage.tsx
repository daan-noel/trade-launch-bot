import { useMemo } from 'react';
import { DataTable } from 'components/table/DataTable';
import { Badge } from 'components/ui/Badge';
import { tradeColumns } from 'components/transactions/tradeColumns';
import { usePriceDisplay } from 'hooks/usePriceDisplay';
import { useTradeStream } from 'hooks/useTradeStream';

export function TransactionsPage() {
  const price = usePriceDisplay();
  const events = useTradeStream();
  // Key on the unit label only: the rate-dependent cells read the rate from
  // context, so the column array stays stable across USD-rate ticks.
  const columns = useMemo(() => tradeColumns(price.unitLabel), [price.unitLabel]);

  return (
    <div>
      <div className="mb-3.5 flex items-center gap-2.5">
        <h2 className="text-base font-bold text-primary">Live Transactions</h2>
        <Badge variant="primary" className="font-mono">
          {events.length} captured
        </Badge>
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
