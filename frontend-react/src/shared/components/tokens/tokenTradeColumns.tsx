import type { ColumnDef } from 'components/table/types';
import type { TradeRecord } from 'types';
import { DateCell } from 'components/table/DateCell';
import { formatDecimal } from 'utils/format';
import { AmountCell, PriceCell } from 'components/tokens/priceCells';
import { cn } from 'lib/cn';
import { AddressDisplay } from 'components/ui/AddressDisplay';

/**
 * Takes only the unit *label* (not the whole `usePriceDisplay` object) so the
 * column array stays referentially stable across USD-rate ticks — the rate
 * changes the price object's identity every tick, which would otherwise rebuild
 * every column and re-render the entire trades table. The two rate-dependent
 * value cells use the memoized `AmountCell`/`PriceCell`, which read the rate from
 * context themselves and re-render in isolation when it changes.
 */
export function tokenTradeColumns(unit: string): ColumnDef<TradeRecord>[] {
  return [
    {
      key: 'side',
      label: 'Side',
      render: (t) => {
        const isBuy = t.trade_type === 'buy';
        return (
          <span
            className={cn(
              'inline-block rounded px-2 py-0.5 text-[11px] font-bold tracking-wide',
              isBuy
                ? 'border border-primary bg-primary/15 text-primary'
                : 'border border-red bg-red/15 text-red',
            )}
          >
            {isBuy ? 'BUY' : 'SELL'}
          </span>
        );
      },
      sortValue: (t) => t.trade_type,
      searchValue: (t) => t.trade_type,
    },
    {
      key: 'wallet',
      label: 'Wallet',
      render: (t) => (
        <AddressDisplay address={t.wallet_address} kind="account" />
      ),
      sortValue: (t) => t.wallet_address,
      searchValue: (t) => t.wallet_address,
    },
    {
      key: 'sol',
      label: unit,
      render: (t) => {
        const isBuy = t.trade_type === 'buy';
        return (
          <span className={cn('font-semibold', isBuy ? 'text-primary' : 'text-red')}>
            <AmountCell sol={t.amount_sol} />
          </span>
        );
      },
      sortValue: (t) => t.amount_sol,
      searchValue: (t) => String(t.amount_sol),
    },
    {
      key: 'tokens',
      label: 'Tokens',
      render: (t) => {
        const isBuy = t.trade_type === 'buy';
        return (
          <span className={cn('font-semibold', isBuy ? 'text-primary' : 'text-red')}>
            {formatDecimal(t.token_amount, 0)}
          </span>
        );
      },
      sortValue: (t) => t.token_amount,
      searchValue: (t) => String(t.token_amount),
    },
    {
      key: 'price',
      label: `Price (${unit})`,
      render: (t) => {
        const isBuy = t.trade_type === 'buy';
        return (
          <span className={cn('font-semibold', isBuy ? 'text-primary' : 'text-red')}>
            <PriceCell sol={t.price_per_token} />
          </span>
        );
      },
      sortValue: (t) => t.price_per_token,
      searchValue: (t) => String(t.price_per_token),
    },
    {
      key: 'signature',
      label: 'Signature',
      render: (t) => (
        <AddressDisplay address={t.tx_signature} kind="transaction" />
      ),
      sortValue: (t) => t.tx_signature,
      searchValue: (t) => t.tx_signature,
    },
    {
      key: 'slot',
      label: 'Slot',
      render: (t) => t.slot,
      sortValue: (t) => t.slot,
      searchValue: (t) => String(t.slot),
    },
    {
      key: 'time',
      label: 'Time (UTC)',
      width: '108px',
      render: (t) => <DateCell iso={t.received_at ?? t.block_time} />,
      sortValue: (t) => t.received_at ?? t.block_time,
      searchValue: (t) => t.received_at ?? t.block_time,
    },
  ];
}
