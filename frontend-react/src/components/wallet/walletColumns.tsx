import type { ColumnDef } from 'components/table/types';
import type { WalletHolding } from 'types';
import type { useWalletPriceDisplay } from 'hooks/useWalletPriceDisplay';
import { cn } from 'lib/cn';
import { formatCompact, priceClass } from 'utils/format';
import { AddressDisplay } from 'components/ui/AddressDisplay';
import { Badge } from 'components/ui/Badge';

export interface WalletActions {
  onBuy: (mint: string, tokenProgramId: string) => void;
  onSell: (mint: string, tokenAmount: number) => void;
  sellingMint: string | null;
}

export function walletColumns(
  actions: WalletActions,
  price: ReturnType<typeof useWalletPriceDisplay>,
): ColumnDef<WalletHolding>[] {
  return [
    {
      key: 'symbol',
      label: 'Symbol',
      group: 'identity',
      width: '90px',
      sortable: true,
      render: (r) => <span className="font-bold text-text">{r.symbol ?? '—'}</span>,
      sortValue: (r) => r.symbol ?? '',
      searchValue: (r) => r.symbol ?? '',
    },
    {
      key: 'mint',
      label: 'Mint',
      group: 'identity',
      width: '195px',
      sortable: true,
      render: (r) => (
        <AddressDisplay
          address={r.mint}
          kind="token"
          truncateLen={12}
          stopPropagation
        />
      ),
      sortValue: (r) => r.mint,
      searchValue: (r) => r.mint,
    },
    {
      key: 'ui_amount',
      label: 'Amount',
      group: 'position',
      width: '120px',
      sortable: true,
      render: (r) => (
        <span className="tabular-nums text-text">{formatCompact(r.ui_amount, 2)}</span>
      ),
      sortValue: (r) => r.ui_amount,
      searchValue: (r) => String(r.ui_amount),
    },
    {
      key: 'value_usd',
      label: 'Value',
      group: 'position',
      width: '110px',
      sortable: true,
      render: (r) =>
        r.value_usd != null ? (
          <span className="font-semibold tabular-nums text-text">
            {price.displayValue(r.value_usd)}
          </span>
        ) : (
          <span className="text-text-dim">—</span>
        ),
      sortValue: (r) => r.value_usd ?? 0,
      searchValue: (r) => String(r.value_usd ?? ''),
    },
    {
      key: 'price_usd',
      label: 'Price',
      group: 'price',
      width: '110px',
      sortable: true,
      render: (r) =>
        r.price_usd != null ? (
          <span className={cn('tabular-nums', priceClass(r.price_usd))}>
            {price.displayPrice(r.price_usd)}
          </span>
        ) : (
          <span className="text-text-dim">—</span>
        ),
      sortValue: (r) => r.price_usd ?? 0,
      searchValue: (r) => String(r.price_usd ?? ''),
    },
    {
      key: 'price_change_24h',
      label: '24h %',
      group: 'price',
      width: '90px',
      sortable: true,
      render: (r) => {
        if (r.price_change_24h == null) return <span className="text-text-dim">—</span>;
        const c = r.price_change_24h;
        return (
          <span
            className={cn(
              'font-semibold tabular-nums',
              c > 0 && 'text-green',
              c < 0 && 'text-red',
            )}
          >
            {c > 0 ? `+${c.toFixed(2)}%` : `${c.toFixed(2)}%`}
          </span>
        );
      },
      sortValue: (r) => r.price_change_24h ?? 0,
      searchValue: (r) => String(r.price_change_24h ?? ''),
    },
    {
      key: 'liquidity',
      label: 'Liquidity',
      group: 'market',
      width: '110px',
      sortable: true,
      render: (r) =>
        r.liquidity != null ? (
          <span className="tabular-nums text-text-mid">
            {price.displayLiquidity(r.liquidity)}
          </span>
        ) : (
          <span className="text-text-dim">—</span>
        ),
      sortValue: (r) => r.liquidity ?? 0,
      searchValue: (r) => String(r.liquidity ?? ''),
    },
    {
      key: 'migrated',
      label: 'Migrated',
      group: 'flags',
      width: '80px',
      sortable: true,
      render: (r) =>
        r.is_migrated ? (
          <Badge variant="primary">AMM</Badge>
        ) : (
          <span className="text-text-dim">Curve</span>
        ),
      sortValue: (r) => (r.is_migrated ? 1 : 0),
      searchValue: (r) => (r.is_migrated ? 'migrated amm' : 'curve'),
    },
    {
      key: 'cashback',
      label: 'Cashback',
      group: 'flags',
      width: '80px',
      sortable: true,
      render: (r) =>
        r.is_cashback_enabled ? (
          <Badge variant="success">✓</Badge>
        ) : (
          <span className="text-text-dim">—</span>
        ),
      sortValue: (r) => (r.is_cashback_enabled ? 1 : 0),
      searchValue: (r) => String(r.is_cashback_enabled),
    },
    {
      key: 'token_created_at',
      label: 'Token Created',
      group: 'meta',
      width: '110px',
      sortable: true,
      render: (r) =>
        r.token_created_at ? (
          <span className="text-text-dim">
            {r.token_created_at.slice(0, 19).replace('T', ' ')}
          </span>
        ) : (
          '—'
        ),
      sortValue: (r) => r.token_created_at ?? '',
      searchValue: (r) => r.token_created_at ?? '',
    },
    {
      key: 'amount',
      label: 'Raw Amount',
      group: 'meta',
      width: '140px',
      sortable: true,
      defaultVisible: false,
      render: (r) => <span className="tabular-nums text-text-dim">{r.amount}</span>,
      sortValue: (r) => r.amount,
      searchValue: (r) => String(r.amount),
    },
    {
      key: 'decimals',
      label: 'Decimals',
      group: 'meta',
      width: '80px',
      sortable: true,
      defaultVisible: false,
      render: (r) => <span className="text-text-dim">{r.decimals}</span>,
      sortValue: (r) => r.decimals,
      searchValue: (r) => String(r.decimals),
    },
    {
      key: 'token_program',
      label: 'Program',
      group: 'meta',
      width: '100px',
      sortable: true,
      render: (r) => (
        <span className="text-text-dim">
          {r.token_program_id.startsWith('TokenzQdB') ? 'Token-2022' : 'SPL Token'}
        </span>
      ),
      sortValue: (r) => r.token_program_id,
      searchValue: (r) => r.token_program_id,
    },
    {
      key: 'token_account',
      label: 'Token Account',
      group: 'meta',
      width: '195px',
      sortable: false,
      defaultVisible: false,
      render: (r) => (
        <AddressDisplay
          address={r.token_account}
          kind="account"
          truncateLen={12}
          stopPropagation
        />
      ),
      searchValue: (r) => r.token_account,
    },
    {
      key: 'actions',
      label: 'Actions',
      width: '160px',
      sortable: false,
      render: (r) => {
        const isSelling = actions.sellingMint === r.mint;
        return (
          <div className="flex items-center justify-center gap-1.5" onClick={(e) => e.stopPropagation()}>
            <button
              type="button"
              onClick={() => actions.onBuy(r.mint, r.token_program_id)}
              className="rounded border border-primary/50 bg-primary/12 px-2 py-0.5 text-[11px] font-semibold text-primary hover:bg-primary/22"
            >
              Buy
            </button>
            <button
              type="button"
              disabled={isSelling}
              onClick={() => actions.onSell(r.mint, r.amount)}
              className="rounded border border-red/50 bg-red/12 px-2 py-0.5 text-[11px] font-semibold text-red hover:bg-red/22 disabled:opacity-45"
            >
              {isSelling ? 'Selling…' : 'Sell All'}
            </button>
          </div>
        );
      },
      searchValue: () => '',
    },
  ];
}
