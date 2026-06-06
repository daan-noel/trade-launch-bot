import type { ColumnDef } from 'components/table/types';
import type { WalletHolding } from 'types';
import { cn } from 'lib/cn';
import { AddressDisplay } from 'components/ui/AddressDisplay';

function formatPriceUsd(p: number): string {
  if (p < 0.0001) return `$${p.toFixed(8)}`;
  if (p < 0.01) return `$${p.toFixed(6)}`;
  return `$${p.toFixed(4)}`;
}

function formatLiquidity(l: number): string {
  if (l >= 1_000_000) return `$${(l / 1_000_000).toFixed(2)}M`;
  if (l >= 1_000) return `$${(l / 1_000).toFixed(2)}K`;
  return `$${l.toFixed(2)}`;
}

export interface WalletActions {
  onBuy: (mint: string, tokenProgramId: string) => void;
  onSell: (mint: string, tokenAmount: number) => void;
  sellingMint: string | null;
}

export function walletColumns(actions: WalletActions): ColumnDef<WalletHolding>[] {
  return [
    {
      key: 'symbol',
      label: 'Symbol',
      width: '90px',
      sortable: true,
      render: (r) => r.symbol ?? '—',
      sortValue: (r) => r.symbol ?? '',
      searchValue: (r) => r.symbol ?? '',
    },
    {
      key: 'mint',
      label: 'Mint',
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
      key: 'price_usd',
      label: 'Price ($)',
      width: '120px',
      sortable: true,
      render: (r) => (r.price_usd != null ? formatPriceUsd(r.price_usd) : '—'),
      sortValue: (r) => r.price_usd ?? 0,
      searchValue: (r) => String(r.price_usd ?? ''),
    },
    {
      key: 'ui_amount',
      label: 'Amount',
      width: '140px',
      sortable: true,
      render: (r) => r.ui_amount.toFixed(6),
      sortValue: (r) => r.ui_amount,
      searchValue: (r) => String(r.ui_amount),
    },
    {
      key: 'value_usd',
      label: 'Value ($)',
      width: '110px',
      sortable: true,
      render: (r) => (r.value_usd != null ? `$${r.value_usd.toFixed(2)}` : '—'),
      sortValue: (r) => r.value_usd ?? 0,
      searchValue: (r) => String(r.value_usd ?? ''),
    },
    {
      key: 'liquidity',
      label: 'Liquidity ($)',
      width: '120px',
      sortable: true,
      render: (r) => (r.liquidity != null ? formatLiquidity(r.liquidity) : '—'),
      sortValue: (r) => r.liquidity ?? 0,
      searchValue: (r) => String(r.liquidity ?? ''),
    },
    {
      key: 'price_change_24h',
      label: '24h %',
      width: '90px',
      sortable: true,
      render: (r) => {
        if (r.price_change_24h == null) return '—';
        const c = r.price_change_24h;
        return (
          <span className={cn(c > 0 && 'text-green', c < 0 && 'text-red')}>
            {c > 0 ? `+${c.toFixed(2)}%` : `${c.toFixed(2)}%`}
          </span>
        );
      },
      sortValue: (r) => r.price_change_24h ?? 0,
      searchValue: (r) => String(r.price_change_24h ?? ''),
    },
    {
      key: 'token_created_at',
      label: 'Token Created',
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
      width: '140px',
      sortable: true,
      defaultVisible: false,
      render: (r) => r.amount,
      sortValue: (r) => r.amount,
      searchValue: (r) => String(r.amount),
    },
    {
      key: 'decimals',
      label: 'Decimals',
      width: '80px',
      sortable: true,
      render: (r) => r.decimals,
      sortValue: (r) => r.decimals,
      searchValue: (r) => String(r.decimals),
    },
    {
      key: 'token_program',
      label: 'Program',
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
      width: '195px',
      sortable: false,
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
