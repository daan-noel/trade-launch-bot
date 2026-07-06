import type { ColumnDef } from 'components/table/types';
import type { LiveTrade } from 'types';
import { DateCell } from 'components/table/DateCell';
import { formatDecimal } from 'utils/format';
import { AmountCell, PriceCell } from 'components/tokens/priceCells';
import { cn } from 'lib/cn';
import { AddressDisplay } from 'components/ui/AddressDisplay';

/**
 * Takes only the unit *label* (not the whole `usePriceDisplay` object) so the
 * column array stays referentially stable across USD-rate ticks — the rate
 * changes the price object's identity every tick, which would otherwise rebuild
 * every column and re-render the entire live table. The two rate-dependent value
 * cells use the memoized `AmountCell`/`PriceCell`, which read the rate from
 * context themselves and re-render in isolation when it changes.
 */
export function tradeColumns(unit: string): ColumnDef<LiveTrade>[] {
  return [
    {
      key: 'mint_address',
      label: 'Mint',
      render: (ev) => <AddressDisplay address={ev.mint_address} kind="token" />,
      sortValue: (ev) => ev.mint_address,
      searchValue: (ev) => ev.mint_address,
    },
    {
      key: 'side',
      label: 'Side',
      render: (ev) => {
        const isBuy = ev.trade_type === 'buy';
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
      sortValue: (ev) => ev.trade_type,
      searchValue: (ev) => ev.trade_type,
    },
    {
      key: 'wallet',
      label: 'Wallet',
      render: (ev) => <AddressDisplay address={ev.wallet} kind="account" />,
      sortValue: (ev) => ev.wallet,
      searchValue: (ev) => ev.wallet,
    },
    {
      key: 'sol',
      label: unit,
      render: (ev) => {
        const isBuy = ev.trade_type === 'buy';
        return (
          <span className={cn('font-semibold', isBuy ? 'text-primary' : 'text-red')}>
            <AmountCell sol={ev.amount_sol} />
          </span>
        );
      },
      sortValue: (ev) => ev.amount_sol,
      searchValue: (ev) => String(ev.amount_sol),
    },
    {
      key: 'tokens',
      label: 'Tokens',
      render: (ev) => {
        const isBuy = ev.trade_type === 'buy';
        return (
          <span className={cn('font-semibold', isBuy ? 'text-primary' : 'text-red')}>
            {formatDecimal(ev.token_amount, 0)}
          </span>
        );
      },
      sortValue: (ev) => ev.token_amount,
      searchValue: (ev) => String(ev.token_amount),
    },
    {
      key: 'price',
      label: `Price (${unit})`,
      render: (ev) => {
        const isBuy = ev.trade_type === 'buy';
        return (
          <span className={cn('font-semibold', isBuy ? 'text-primary' : 'text-red')}>
            <PriceCell sol={ev.price_per_token} />
          </span>
        );
      },
      sortValue: (ev) => ev.price_per_token,
      searchValue: (ev) => String(ev.price_per_token),
    },
    {
      key: 'signature',
      label: 'Signature',
      render: (ev) => (
        <AddressDisplay address={ev.tx_signature} kind="transaction" />
      ),
      sortValue: (ev) => ev.tx_signature,
      searchValue: (ev) => ev.tx_signature,
    },
    {
      key: 'slot',
      label: 'Slot',
      render: (ev) => ev.slot,
      sortValue: (ev) => ev.slot,
      searchValue: (ev) => String(ev.slot),
    },
    {
      key: 'time',
      label: 'Time (UTC)',
      width: '108px',
      render: (ev) => <DateCell iso={ev.timestamp} />,
      sortValue: (ev) => ev.timestamp,
      searchValue: (ev) => ev.timestamp,
    },
  ];
}
