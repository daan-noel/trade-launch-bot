import type { ColumnDef } from 'components/table/types';
import type { RulePositionRecord, MatchedTokenRecord, SimulatedTokenResult } from 'types';
import { formatAge, formatDecimalTrim } from 'utils/format';
import type { usePriceDisplay } from 'hooks/usePriceDisplay';
import { fmtTime } from './utils';
import { cn } from 'lib/cn';
import { AddressDisplay } from 'components/ui/AddressDisplay';

export function positionColumns(
  price: ReturnType<typeof usePriceDisplay>,
): ColumnDef<RulePositionRecord>[] {
  return [
    {
      key: 'mint',
      label: 'Mint',
      render: (r) => (
        <AddressDisplay
          address={r.mint}
          kind="token"
          display={r.mint.slice(0, 6)}
        />
      ),
      searchValue: (r) => r.mint,
    },
    {
      key: 'entry_price',
      label: 'Entry Price',
      sortable: true,
      render: (r) => price.displayPrice(r.entry_price),
      sortValue: (r) => r.entry_price,
      searchValue: (r) => String(r.entry_price),
    },
    {
      key: 'entry_time',
      label: 'Entry Time',
      sortable: true,
      render: (r) => fmtTime(r.entry_time),
      sortValue: (r) => r.entry_time ?? '',
      searchValue: (r) => r.entry_time ?? '',
    },
    {
      key: 'exit_price',
      label: 'Exit Price',
      sortable: true,
      render: (r) => (r.exit_price != null ? price.displayPrice(r.exit_price) : '—'),
      sortValue: (r) => r.exit_price,
      searchValue: (r) => String(r.exit_price ?? ''),
    },
    {
      key: 'exit_time',
      label: 'Exit Time',
      sortable: true,
      render: (r) => fmtTime(r.exit_time),
      sortValue: (r) => r.exit_time ?? '',
      searchValue: (r) => r.exit_time ?? '',
    },
    {
      key: 'holding',
      label: 'Holding',
      render: (r) =>
        r.exit_amount != null ? formatDecimalTrim(r.exit_amount, 3) : '—',
      searchValue: () => '',
    },
    {
      key: 'pnl_pct',
      label: 'PnL%',
      sortable: true,
      render: (r) => {
        if (r.pnl_percent == null) return <span className="text-text-dim">—</span>;
        return (
          <span className={cn('font-bold', r.pnl_percent >= 0 ? 'text-green' : 'text-red')}>
            {r.pnl_percent >= 0 ? '+' : ''}
            {r.pnl_percent.toFixed(1)}%
          </span>
        );
      },
      sortValue: (r) => r.pnl_percent,
      searchValue: (r) => String(r.pnl_percent ?? ''),
    },
    {
      key: 'pnl_sol',
      label: 'PnL',
      render: (r) => {
        if (r.exit_price == null) return <span className="text-text-dim">—</span>;
        const amt = r.exit_amount ?? 0;
        const positive = (r.pnl_percent ?? 0) >= 0;
        return (
          <span className={cn('font-bold', positive ? 'text-green' : 'text-red')}>
            {price.displayAmount(amt)}
          </span>
        );
      },
      searchValue: () => '',
    },
    {
      key: 'status',
      label: 'Status',
      sortable: true,
      render: (r) => {
        if (r.status === 'TakeProfit') return <span className="font-bold text-green">TP</span>;
        if (r.status === 'StopLoss') return <span className="font-bold text-red">SL</span>;
        return <span className="text-text-dim">{r.status}</span>;
      },
      sortValue: (r) => r.status,
      searchValue: (r) => r.status,
    },
  ];
}

export const matchedColumns: ColumnDef<MatchedTokenRecord>[] = [
  {
    key: 'symbol',
    label: 'Symbol',
    sortable: true,
    render: (r) => (
      <AddressDisplay address={r.mint} kind="token" display={r.symbol} />
    ),
    sortValue: (r) => r.symbol,
    searchValue: (r) => `${r.symbol} ${r.name}`,
  },
  {
    key: 'name',
    label: 'Name',
    sortable: true,
    render: (r) => <span className="text-text-dim">{r.name}</span>,
    sortValue: (r) => r.name,
    searchValue: (r) => r.name,
  },
  {
    key: 'created',
    label: 'Created',
    sortable: true,
    render: (r) => fmtTime(r.created_at),
    sortValue: (r) => r.created_at,
    searchValue: (r) => r.created_at,
  },
  {
    key: 'init_buy',
    label: 'Init Buy (SOL)',
    sortable: true,
    render: (r) => (r.initial_buy_sol != null ? r.initial_buy_sol.toFixed(4) : '—'),
    sortValue: (r) => r.initial_buy_sol,
    searchValue: (r) => String(r.initial_buy_sol ?? ''),
  },
  {
    key: 'cu_limit',
    label: 'CU Limit',
    sortable: true,
    render: (r) => (r.cu_limit != null ? r.cu_limit : '—'),
    sortValue: (r) => r.cu_limit,
    searchValue: (r) => String(r.cu_limit ?? ''),
  },
  {
    key: 'cu_price',
    label: 'CU Price',
    sortable: true,
    render: (r) => (r.cu_price != null ? r.cu_price : '—'),
    sortValue: (r) => r.cu_price,
    searchValue: (r) => String(r.cu_price ?? ''),
  },
];

export function simColumns(
  price: ReturnType<typeof usePriceDisplay>,
): ColumnDef<SimulatedTokenResult>[] {
  return [
    {
      key: 'symbol',
      label: 'Symbol',
      sortable: true,
      render: (r) => (
        <AddressDisplay address={r.mint} kind="token" display={r.symbol} />
      ),
      sortValue: (r) => r.symbol,
      searchValue: (r) => r.symbol,
    },
    {
      key: 'entry_price',
      label: 'Entry Price',
      sortable: true,
      render: (r) => price.displayPrice(r.entry_price),
      sortValue: (r) => r.entry_price,
      searchValue: (r) => String(r.entry_price),
    },
    {
      key: 'ath_price',
      label: 'ATH',
      tooltip: 'All-time-high price across the token’s full trade history.',
      sortable: true,
      render: (r) => price.displayPrice(r.ath_price),
      sortValue: (r) => r.ath_price,
      searchValue: (r) => String(r.ath_price),
    },
    {
      key: 'entry_time',
      label: 'Entry Time',
      sortable: true,
      render: (r) => fmtTime(r.entry_time),
      sortValue: (r) => r.entry_time,
      searchValue: (r) => r.entry_time,
    },
    {
      key: 'exit_price',
      label: 'Exit Price',
      sortable: true,
      render: (r) => (r.exit_price != null ? price.displayPrice(r.exit_price) : '—'),
      sortValue: (r) => r.exit_price,
      searchValue: (r) => String(r.exit_price ?? ''),
    },
    {
      key: 'exit_time',
      label: 'Exit Time',
      sortable: true,
      render: (r) => fmtTime(r.exit_time),
      sortValue: (r) => r.exit_time ?? '',
      searchValue: (r) => r.exit_time ?? '',
    },
    {
      key: 'holding',
      label: 'Holding',
      sortable: true,
      render: (r) => (r.holding_secs != null ? formatAge(r.holding_secs) : '—'),
      sortValue: (r) => r.holding_secs,
      searchValue: () => '',
    },
    {
      key: 'pnl_pct',
      label: 'PnL%',
      sortable: true,
      render: (r) => {
        if (r.pnl_percent == null) return <span className="text-text-dim">—</span>;
        return (
          <span className={cn('font-bold', r.pnl_percent >= 0 ? 'text-green' : 'text-red')}>
            {r.pnl_percent >= 0 ? '+' : ''}
            {r.pnl_percent.toFixed(1)}%
          </span>
        );
      },
      sortValue: (r) => r.pnl_percent,
      searchValue: (r) => String(r.pnl_percent ?? ''),
    },
    {
      key: 'pnl_sol',
      label: 'PnL',
      sortable: true,
      render: (r) => {
        if (r.pnl_sol == null) return <span className="text-text-dim">—</span>;
        return (
          <span className={cn('font-bold', r.pnl_sol >= 0 ? 'text-green' : 'text-red')}>
            {price.displayAmount(r.pnl_sol)}
          </span>
        );
      },
      sortValue: (r) => r.pnl_sol,
      searchValue: () => '',
    },
    {
      key: 'reason',
      label: 'Reason',
      sortable: true,
      render: (r) => {
        if (r.exit_reason === 'TakeProfit') return <span className="font-bold text-green">TP</span>;
        if (r.exit_reason === 'StopLoss') return <span className="font-bold text-red">SL</span>;
        if (r.exit_reason === 'TrailingStop')
          return <span className="font-bold text-warning">TRAIL</span>;
        if (r.exit_reason === 'Stall')
          return <span className="font-bold text-accent">STALL</span>;
        if (r.exit_reason === 'TimeStop')
          return <span className="font-bold text-info">TIME</span>;
        return <span className="text-text-dim">Open</span>;
      },
      sortValue: (r) => r.exit_reason,
      searchValue: (r) => r.exit_reason,
    },
    {
      key: 'trades',
      label: 'Trades',
      sortable: true,
      render: (r) => r.total_trades,
      sortValue: (r) => r.total_trades,
      searchValue: (r) => String(r.total_trades),
    },
  ];
}
