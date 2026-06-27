import type { ColumnDef } from 'components/table/types';
import type { RulePositionRecord, MatchedTokenRecord, SimulatedTokenResult } from 'types';
import { ageClass, formatAge, formatDecimalTrim } from 'utils/format';
import { AmountCell, CurrentPriceCell } from 'components/tokens/priceCells';
import { DateCell } from 'components/table/DateCell';
import { cn } from 'lib/cn';
import { AddressDisplay } from 'components/ui/AddressDisplay';
import { appendedTokenColumns, coreTokenColumns } from 'components/tokens/sharedTokenColumns';

/** Render an exit reason as a compact colored badge, shared by the live-position
 * and simulation-result tables. Falsy/unknown reasons (e.g. a still-open
 * position) render as a dim "Open". */
export function exitReasonBadge(reason: string | null | undefined) {
  switch (reason) {
    case 'LiquidityExit':
      return <span className="font-bold text-primary">LIQ</span>;
    case 'TakeProfit':
      return <span className="font-bold text-green">TP</span>;
    case 'StopLoss':
      return <span className="font-bold text-red">SL</span>;
    case 'TrailingStop':
      return <span className="font-bold text-warning">TRAIL</span>;
    case 'Stall':
      return <span className="font-bold text-accent">STALL</span>;
    case 'TimeStop':
      return <span className="font-bold text-info">TIME</span>;
    case 'ExitFailed':
      return <span className="font-bold text-red">FAIL</span>;
    case 'ManualClose':
      return <span className="font-bold text-text-dim">MANUAL</span>;
    default:
      return <span className="text-text-dim">Open</span>;
  }
}

const POSITION_KEYS = new Set([
  'mint', 'symbol', 'name', 'created',
  'entry_price', 'entry_time', 'exit_price', 'exit_time',
  'holding', 'pnl_pct', 'pnl_sol', 'status', 'exit_reason',
]);
const MATCHED_KEYS = new Set([
  'symbol', 'name', 'created',
  'init_buy', 'initial_buy', 'cu_limit', 'cu_price',
]);
const SIM_KEYS = new Set([
  'symbol', 'created', 'entry_price', 'entry_time', 'ath_price', 'exit_price', 'exit_time',
  'holding', 'pnl_pct', 'pnl_sol', 'reason', 'trades',
]);

// Price/amount cells read the unit + USD rate from context themselves (see
// priceCells), so these column arrays are referentially stable across a rate
// tick: only the rate-aware cells re-render, not the whole table.
export const positionColumns: ColumnDef<RulePositionRecord>[] = [
    {
      key: 'mint',
      label: 'Mint',
      group: 'identity',
      render: (r) => (
        <AddressDisplay
          address={r.mint}
          kind="token"
          display={r.mint.slice(0, 6)}
        />
      ),
      searchValue: (r) => r.mint,
    },
    ...coreTokenColumns(),
    {
      key: 'entry_price',
      label: 'Entry Price',
      group: 'entry',
      sortable: true,
      render: (r) => <CurrentPriceCell sol={r.entry_price} />,
      sortValue: (r) => r.entry_price,
      searchValue: (r) => String(r.entry_price),
    },
    {
      key: 'entry_time',
      label: 'Entry Time',
      group: 'entry',
      sortable: true,
      render: (r) => <DateCell iso={r.entry_time} />,
      sortValue: (r) => r.entry_time ?? '',
      searchValue: (r) => r.entry_time ?? '',
    },
    {
      key: 'exit_price',
      label: 'Exit Price',
      group: 'exit',
      sortable: true,
      render: (r) => (r.exit_price != null ? <CurrentPriceCell sol={r.exit_price} /> : '—'),
      sortValue: (r) => r.exit_price,
      searchValue: (r) => String(r.exit_price ?? ''),
    },
    {
      key: 'exit_time',
      label: 'Exit Time',
      group: 'exit',
      sortable: true,
      render: (r) => <DateCell iso={r.exit_time} />,
      sortValue: (r) => r.exit_time ?? '',
      searchValue: (r) => r.exit_time ?? '',
    },
    {
      key: 'holding',
      label: 'Holding',
      group: 'pnl',
      render: (r) =>
        r.exit_token_amount != null ? formatDecimalTrim(r.exit_token_amount, 3) : '—',
      searchValue: () => '',
    },
    {
      key: 'pnl_pct',
      label: 'PnL%',
      group: 'pnl',
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
      group: 'pnl',
      render: (r) => {
        if (r.exit_price == null) return <span className="text-text-dim">—</span>;
        const amt = r.exit_token_amount ?? 0;
        const positive = (r.pnl_percent ?? 0) >= 0;
        return (
          <span className={cn('font-bold', positive ? 'text-green' : 'text-red')}>
            <AmountCell sol={amt} />
          </span>
        );
      },
      searchValue: () => '',
    },
    {
      key: 'status',
      label: 'Status',
      group: 'state',
      sortable: true,
      render: (r) => {
        if (r.status === 'Arming') return <span className="italic text-text-dim">Arming</span>;
        if (r.status === 'BuySubmitted') return <span className="italic text-warning">Buying…</span>;
        if (r.status === 'TakeProfit') return <span className="font-bold text-green">TP</span>;
        if (r.status === 'StopLoss') return <span className="font-bold text-red">SL</span>;
        return <span className="text-text-dim">{r.status}</span>;
      },
      sortValue: (r) => r.status,
      searchValue: (r) => r.status,
    },
    {
      key: 'exit_reason',
      label: 'Exit Reason',
      group: 'state',
      sortable: true,
      render: (r) => exitReasonBadge(r.exit_reason),
      sortValue: (r) => r.exit_reason ?? '',
      searchValue: (r) => r.exit_reason ?? '',
    },
  ...appendedTokenColumns(POSITION_KEYS),
];

export const matchedColumns: ColumnDef<MatchedTokenRecord>[] = [
  {
    key: 'symbol',
    label: 'Symbol',
    group: 'identity',
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
    group: 'identity',
    sortable: true,
    render: (r) => <span className="text-text-dim">{r.name}</span>,
    sortValue: (r) => r.name,
    searchValue: (r) => r.name,
  },
  {
    key: 'created',
    label: 'Created',
    group: 'activity',
    sortable: true,
    render: (r) => <DateCell iso={r.created_at} />,
    sortValue: (r) => r.created_at,
    searchValue: (r) => r.created_at,
  },
  {
    key: 'init_buy',
    label: 'Init Buy (SOL)',
    group: 'params',
    sortable: true,
    render: (r) => (r.initial_buy_sol != null ? r.initial_buy_sol.toFixed(4) : '—'),
    sortValue: (r) => r.initial_buy_sol,
    searchValue: (r) => String(r.initial_buy_sol ?? ''),
  },
  {
    key: 'cu_limit',
    label: 'CU Limit',
    group: 'params',
    sortable: true,
    render: (r) => (r.cu_limit != null ? r.cu_limit : '—'),
    sortValue: (r) => r.cu_limit,
    searchValue: (r) => String(r.cu_limit ?? ''),
  },
  {
    key: 'cu_price',
    label: 'CU Price',
    group: 'params',
    sortable: true,
    render: (r) => (r.cu_price != null ? r.cu_price : '—'),
    sortValue: (r) => r.cu_price,
    searchValue: (r) => String(r.cu_price ?? ''),
  },
  ...appendedTokenColumns(MATCHED_KEYS),
];

export const simColumns: ColumnDef<SimulatedTokenResult>[] = [
    {
      key: 'symbol',
      label: 'Symbol',
      group: 'identity',
      sortable: true,
      render: (r) => (
        <AddressDisplay address={r.mint} kind="token" display={r.symbol} />
      ),
      sortValue: (r) => r.symbol,
      searchValue: (r) => r.symbol,
    },
    ...coreTokenColumns(new Set(['symbol'])),
    {
      key: 'entry_price',
      label: 'Entry Price',
      group: 'entry',
      sortable: true,
      render: (r) => <CurrentPriceCell sol={r.entry_price} />,
      sortValue: (r) => r.entry_price,
      searchValue: (r) => String(r.entry_price),
    },
    {
      key: 'entry_time',
      label: 'Entry Time',
      group: 'entry',
      sortable: true,
      render: (r) => <DateCell iso={r.entry_time} />,
      sortValue: (r) => r.entry_time,
      searchValue: (r) => r.entry_time,
    },
    {
      key: 'ath_price',
      label: 'ATH',
      tooltip: 'All-time-high price across the token’s full trade history.',
      group: 'ath',
      sortable: true,
      render: (r) => <CurrentPriceCell sol={r.ath_price} />,
      sortValue: (r) => r.ath_price,
      searchValue: (r) => String(r.ath_price),
    },
    {
      key: 'exit_price',
      label: 'Exit Price',
      group: 'exit',
      sortable: true,
      render: (r) => (r.exit_price != null ? <CurrentPriceCell sol={r.exit_price} /> : '—'),
      sortValue: (r) => r.exit_price,
      searchValue: (r) => String(r.exit_price ?? ''),
    },
    {
      key: 'exit_time',
      label: 'Exit Time',
      group: 'exit',
      sortable: true,
      render: (r) => <DateCell iso={r.exit_time} />,
      sortValue: (r) => r.exit_time ?? '',
      searchValue: (r) => r.exit_time ?? '',
    },
    {
      key: 'holding',
      label: 'Holding',
      group: 'holding',
      sortable: true,
      render: (r) =>
        r.holding_secs != null ? (
          <span className={ageClass(r.holding_secs)}>{formatAge(r.holding_secs)}</span>
        ) : (
          '—'
        ),
      sortValue: (r) => r.holding_secs,
      searchValue: () => '',
    },
    {
      key: 'pnl_pct',
      label: 'PnL%',
      group: 'pnl',
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
      group: 'pnl',
      sortable: true,
      render: (r) => {
        if (r.pnl_sol == null) return <span className="text-text-dim">—</span>;
        return (
          <span className={cn('font-bold', r.pnl_sol >= 0 ? 'text-green' : 'text-red')}>
            <AmountCell sol={r.pnl_sol} />
          </span>
        );
      },
      sortValue: (r) => r.pnl_sol,
      searchValue: () => '',
    },
    {
      key: 'reason',
      label: 'Reason',
      group: 'result',
      sortable: true,
      render: (r) => exitReasonBadge(r.exit_reason),
      sortValue: (r) => r.exit_reason,
      searchValue: (r) => r.exit_reason,
    },
    {
      key: 'trades',
      label: 'Trades',
      group: 'result',
      sortable: true,
      render: (r) => r.total_trades,
      sortValue: (r) => r.total_trades,
      searchValue: (r) => String(r.total_trades),
    },
  ...appendedTokenColumns(SIM_KEYS),
];
