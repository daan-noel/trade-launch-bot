import type { ColumnDef } from 'components/table/types';
import type { RulePositionRecord, MatchedTokenRecord, SimulatedTokenResult } from 'types';
import { ageClass, formatAge, formatDecimalTrim } from 'utils/format';
import { AmountCell, CurrentPriceCell } from 'components/tokens/priceCells';
import { DateCell } from 'components/table/DateCell';
import { cn } from 'lib/cn';
import { AddressDisplay } from 'components/ui/AddressDisplay';
import { appendedTokenColumns } from 'components/tokens/sharedTokenColumns';

/** Render an exit reason as a compact colored badge, shared by the live-position
 * and simulation-result tables. Falsy/unknown reasons (e.g. a still-open
 * position) render as a dim "Open". */
export function exitReasonBadge(reason: string | null | undefined) {
  switch (reason) {
    case 'LiquidityExit':
      return <span className="font-bold text-primary">LIQ</span>;
    case 'CohortExit':
      return <span className="font-bold text-red">COHORT</span>;
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

/** Derive SOL from a price and a token count — the bot stores TOKEN amounts; SOL
 * is never persisted, only shown. null when either input is missing. */
const solOf = (price?: number | null, tokens?: number | null): number | null =>
  price != null && tokens != null ? price * tokens : null;

// Keys already present in each table — used to filter out duplicates from the
// appended token-info columns.
const POSITION_KEYS = new Set([
  'mint', 'target_price', 'target_tokens', 'target_sol', 'target_time', 'target_tx',
  'entry_price', 'entry_tokens', 'entry_sol', 'entry_time', 'entry_tx',
  'exit_price', 'exit_tokens', 'exit_sol', 'exit_time', 'exit_tx',
  'holding', 'pnl_pct', 'pnl_sol', 'status', 'exit_reason',
]);
const MATCHED_KEYS = new Set([
  'symbol', 'mint', 'name', 'created',
  // init_buy and cu_limit/cu_price are already shown under different keys
  'init_buy', 'initial_buy', 'cu_limit', 'cu_price',
]);
const SIM_KEYS = new Set([
  'symbol', 'mint', 'target_price', 'target_tokens', 'target_sol', 'target_time', 'target_tx',
  'entry_price', 'entry_tokens', 'entry_sol', 'entry_time', 'ath_price', 'exit_price', 'exit_time',
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
    {
      key: 'target_price',
      label: 'Target Price',
      tooltip:
        'Price of the scalp-entry trigger trade that armed this position. The gap vs. Entry Price is the realized slippage.',
      group: 'target',
      sortable: true,
      render: (r) =>
        r.target_price != null ? <CurrentPriceCell sol={r.target_price} /> : '—',
      sortValue: (r) => r.target_price,
      searchValue: (r) => String(r.target_price ?? ''),
    },
    {
      key: 'target_tokens',
      label: 'Target Tokens',
      tooltip: 'Token count of the trigger trade.',
      group: 'target',
      sortable: true,
      render: (r) =>
        r.target_token_amount != null ? formatDecimalTrim(r.target_token_amount, 3) : '—',
      sortValue: (r) => r.target_token_amount,
      searchValue: (r) => String(r.target_token_amount ?? ''),
    },
    {
      key: 'target_sol',
      label: 'Target Size',
      tooltip: 'SOL size of the trigger trade (price × tokens).',
      group: 'target',
      sortable: true,
      render: (r) => {
        const sol = solOf(r.target_price, r.target_token_amount);
        return sol != null ? <AmountCell sol={sol} /> : '—';
      },
      sortValue: (r) => solOf(r.target_price, r.target_token_amount),
      searchValue: (r) => String(solOf(r.target_price, r.target_token_amount) ?? ''),
    },
    {
      key: 'target_time',
      label: 'Target Time',
      tooltip: 'Block time of the trigger trade. The gap vs. Entry Time is the entry latency.',
      group: 'target',
      sortable: true,
      render: (r) => <DateCell iso={r.target_time} />,
      sortValue: (r) => r.target_time ?? '',
      searchValue: (r) => r.target_time ?? '',
    },
    {
      key: 'target_tx',
      label: 'Target Tx',
      group: 'target',
      render: (r) =>
        r.target_tx ? (
          <AddressDisplay address={r.target_tx} kind="transaction" display={r.target_tx.slice(0, 6)} />
        ) : (
          '—'
        ),
      searchValue: (r) => r.target_tx ?? '',
    },
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
      key: 'entry_tokens',
      label: 'Entry Tokens',
      tooltip: 'Tokens bought at entry.',
      group: 'entry',
      sortable: true,
      render: (r) =>
        r.entry_token_amount != null ? formatDecimalTrim(r.entry_token_amount, 3) : '—',
      sortValue: (r) => r.entry_token_amount,
      searchValue: (r) => String(r.entry_token_amount ?? ''),
    },
    {
      key: 'entry_sol',
      label: 'Entry Size',
      tooltip: 'SOL spent at entry (entry price × tokens).',
      group: 'entry',
      sortable: true,
      render: (r) => {
        const sol = solOf(r.entry_price, r.entry_token_amount);
        return sol != null ? <AmountCell sol={sol} /> : '—';
      },
      sortValue: (r) => solOf(r.entry_price, r.entry_token_amount),
      searchValue: (r) => String(solOf(r.entry_price, r.entry_token_amount) ?? ''),
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
      key: 'entry_tx',
      label: 'Entry Tx',
      group: 'entry',
      render: (r) =>
        r.entry_tx ? (
          <AddressDisplay address={r.entry_tx} kind="transaction" display={r.entry_tx.slice(0, 6)} />
        ) : (
          '—'
        ),
      searchValue: (r) => r.entry_tx ?? '',
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
      key: 'exit_tokens',
      label: 'Exit Tokens',
      tooltip: 'Tokens sold at exit.',
      group: 'exit',
      sortable: true,
      render: (r) =>
        r.exit_token_amount != null ? formatDecimalTrim(r.exit_token_amount, 3) : '—',
      sortValue: (r) => r.exit_token_amount,
      searchValue: (r) => String(r.exit_token_amount ?? ''),
    },
    {
      key: 'exit_sol',
      label: 'Exit Size',
      tooltip: 'SOL received at exit (exit price × tokens).',
      group: 'exit',
      sortable: true,
      render: (r) => {
        const sol = solOf(r.exit_price, r.exit_token_amount);
        return sol != null ? <AmountCell sol={sol} /> : '—';
      },
      sortValue: (r) => solOf(r.exit_price, r.exit_token_amount),
      searchValue: (r) => String(solOf(r.exit_price, r.exit_token_amount) ?? ''),
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
      key: 'exit_tx',
      label: 'Exit Tx',
      group: 'exit',
      render: (r) =>
        r.exit_tx ? (
          <AddressDisplay address={r.exit_tx} kind="transaction" display={r.exit_tx.slice(0, 6)} />
        ) : (
          '—'
        ),
      searchValue: (r) => r.exit_tx ?? '',
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
        if (r.exit_price == null || r.pnl_percent == null)
          return <span className="text-text-dim">—</span>;
        // Realized PnL in SOL = entry notional (entry_price × tokens) × pnl%.
        const entrySol = solOf(r.entry_price, r.entry_token_amount) ?? 0;
        const pnl = entrySol * (r.pnl_percent / 100);
        const positive = r.pnl_percent >= 0;
        return (
          <span className={cn('font-bold', positive ? 'text-green' : 'text-red')}>
            <AmountCell sol={pnl} />
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
    key: 'mint',
    label: 'Mint',
    group: 'identity',
    render: (r) => (
      <AddressDisplay address={r.mint} kind="token" display={r.mint.slice(0, 6)} />
    ),
    searchValue: (r) => r.mint,
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
    {
      key: 'mint',
      label: 'Mint',
      group: 'identity',
      render: (r) => (
        <AddressDisplay address={r.mint} kind="token" display={r.mint.slice(0, 6)} />
      ),
      searchValue: (r) => r.mint,
    },
    {
      key: 'target_price',
      label: 'Target Price',
      tooltip:
        'Price of the scalp-entry trigger trade that armed this position. The gap vs. Entry Price is the modeled adverse slippage.',
      group: 'target',
      sortable: true,
      render: (r) =>
        r.target_price != null ? <CurrentPriceCell sol={r.target_price} /> : '—',
      sortValue: (r) => r.target_price,
      searchValue: (r) => String(r.target_price ?? ''),
    },
    {
      key: 'target_tokens',
      label: 'Target Tokens',
      tooltip: 'Token count of the trigger trade.',
      group: 'target',
      sortable: true,
      render: (r) =>
        r.target_token_amount != null ? formatDecimalTrim(r.target_token_amount, 3) : '—',
      sortValue: (r) => r.target_token_amount,
      searchValue: (r) => String(r.target_token_amount ?? ''),
    },
    {
      key: 'target_sol',
      label: 'Target Size',
      tooltip: 'SOL size of the trigger trade (price × tokens).',
      group: 'target',
      sortable: true,
      render: (r) => {
        const sol = solOf(r.target_price, r.target_token_amount);
        return sol != null ? <AmountCell sol={sol} /> : '—';
      },
      sortValue: (r) => solOf(r.target_price, r.target_token_amount),
      searchValue: (r) => String(solOf(r.target_price, r.target_token_amount) ?? ''),
    },
    {
      key: 'target_time',
      label: 'Target Time',
      tooltip: 'Block time of the trigger trade. The gap vs. Entry Time is the entry latency.',
      group: 'target',
      sortable: true,
      render: (r) => <DateCell iso={r.target_time} />,
      sortValue: (r) => r.target_time ?? '',
      searchValue: (r) => r.target_time ?? '',
    },
    {
      key: 'target_tx',
      label: 'Target Tx',
      group: 'target',
      render: (r) =>
        r.target_tx ? (
          <AddressDisplay address={r.target_tx} kind="transaction" display={r.target_tx.slice(0, 6)} />
        ) : (
          '—'
        ),
      searchValue: (r) => r.target_tx ?? '',
    },
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
