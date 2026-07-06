import type { ColumnDef } from 'components/table/types';
import type { RulePositionRecord, MatchedTokenRecord, SimulatedTokenResult } from 'types';
import { ageClass, formatAge, formatDecimalTrim } from 'utils/format';
import { AmountCell, CurrentPriceCell } from 'components/tokens/priceCells';
import { DateCell } from 'components/table/DateCell';
import { cn } from 'lib/cn';
import { AddressDisplay } from 'components/ui/AddressDisplay';
import { coreTokenColumns } from 'components/tokens/sharedTokenColumns';

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

// `existingKeys` for each table — the column keys the bespoke columns already
// render, so `TokenTable` skips them when appending the shared token-info set.
// Exported so the pages pass them to `TokenTable` alongside the base columns.
export const POSITION_KEYS = new Set([
  'mint', 'symbol', 'name', 'created',
  'entry_price', 'entry_time', 'exit_price', 'exit_time',
  'holding', 'pnl_pct', 'pnl_sol', 'status', 'exit_reason',
]);
// Init Buy / CU Limit / CU Price now render from the shared enrichment columns
// (`initial_buy`/`cu_limit`/`cu_price`) — no hand columns to suppress. Only the
// identity columns are table-owned (they aren't enrichment keys, so listing them
// is documentation, not suppression).
export const MATCHED_KEYS = new Set(['symbol', 'name', 'created']);
export const SIM_KEYS = new Set([
  'symbol', 'created', 'entry_price', 'entry_time', 'ath_price', 'exit_price', 'exit_time',
  'holding', 'pnl_pct', 'pnl_sol', 'reason',
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
        if (r.exit_price == null || r.pnl_sol == null)
          return <span className="text-text-dim">—</span>;
        // `pnl_sol` is the backend's realized SOL PnL — the canonical win/loss
        // basis (mirrors `StrategyPosition::is_win`/`positions_summary`), so
        // color off `pnl_sol` itself rather than `pnl_percent` (price-basis;
        // can disagree with SOL-basis under slippage/fees in real mode).
        const positive = r.pnl_sol >= 0;
        return (
          <span className={cn('font-bold', positive ? 'text-green' : 'text-red')}>
            <AmountCell sol={r.pnl_sol} />
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
      tooltip: 'All-time-high price from tokens_info (token’s recorded ATH).',
      group: 'ath',
      sortable: true,
      render: (r) => <CurrentPriceCell sol={r.ath_price} />,
      sortValue: (r) => r.ath_price,
      searchValue: (r) => String(r.ath_price ?? ''),
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
  // Trade count comes from the shared "Token Trades" enrichment column
  // (`trade_count`), appended by `TokenTable` — the sim no longer carries its own.
];
