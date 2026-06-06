import type { ColumnDef } from 'components/table/types';
import type { TradeRecord } from 'types';
import { DateCell } from 'components/table/DateCell';
import { formatDecimal } from 'utils/format';
import type { usePriceDisplay } from 'hooks/usePriceDisplay';
import { cn } from 'lib/cn';
import { AddressDisplay } from 'components/ui/AddressDisplay';

export function tokenTradeColumns(
  price: ReturnType<typeof usePriceDisplay>,
): ColumnDef<TradeRecord>[] {
  const unit = price.unitLabel;

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
            {price.displayAmount(t.sol_amount)}
          </span>
        );
      },
      sortValue: (t) => t.sol_amount,
      searchValue: (t) => String(t.sol_amount),
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
            {price.displayPrice(t.price_per_token)}
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
