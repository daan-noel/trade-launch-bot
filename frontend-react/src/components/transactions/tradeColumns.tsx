import type { ColumnDef } from '../table/types';
import type { LiveTrade } from '../../types';
import { formatIso } from '../../utils/date';
import { formatDecimal, truncate } from '../../utils/format';
import type { usePriceDisplay } from '../../hooks/usePriceDisplay';
import { cn } from '../../lib/cn';

export function tradeColumns(price: ReturnType<typeof usePriceDisplay>): ColumnDef<LiveTrade>[] {
  const unit = price.unitLabel;

  return [
    {
      key: 'mint',
      label: 'Mint',
      render: (ev) => (
        <a
          href={`https://solscan.io/token/${ev.mint}`}
          target="_blank"
          rel="noopener noreferrer"
          title={ev.mint}
          className="text-accent hover:text-primary"
        >
          {truncate(ev.mint, 10)}
        </a>
      ),
      sortValue: (ev) => ev.mint,
      searchValue: (ev) => ev.mint,
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
      render: (ev) => (
        <a
          href={`https://solscan.io/account/${ev.wallet}`}
          target="_blank"
          rel="noopener noreferrer"
          title={ev.wallet}
          className="text-accent hover:text-primary"
        >
          {truncate(ev.wallet, 10)}
        </a>
      ),
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
            {price.displayAmount(ev.sol_amount)}
          </span>
        );
      },
      sortValue: (ev) => ev.sol_amount,
      searchValue: (ev) => String(ev.sol_amount),
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
            {price.displayPrice(ev.price_per_token)}
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
        <a
          href={`https://solscan.io/tx/${ev.tx_signature}`}
          target="_blank"
          rel="noopener noreferrer"
          title={ev.tx_signature}
          className="text-accent hover:text-primary"
        >
          {truncate(ev.tx_signature, 10)}
        </a>
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
      render: (ev) => formatIso(ev.timestamp),
      sortValue: (ev) => ev.timestamp,
      searchValue: (ev) => ev.timestamp,
    },
  ];
}
