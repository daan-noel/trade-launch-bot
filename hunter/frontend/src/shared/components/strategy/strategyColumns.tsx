import type { ColumnDef } from 'components/table/types';
import type { RulePositionRecord, MatchedTokenRecord, SimulatedTokenResult } from 'types';
import { ageClass, formatAge, formatDecimalTrim } from 'utils/format';
import { AmountCell, CurrentPriceCell } from 'components/tokens/priceCells';
import { DateCell } from 'components/table/DateCell';
import { cn } from 'lib/cn';
import { amountInDisplayUnit } from 'lib/priceUnitSnapshot';
import { formatSignedPct, pctGradeClass, signedToneClass } from 'lib/signedTone';
import { AddressDisplay } from 'components/ui/AddressDisplay';
import { coreTokenColumns } from 'components/tokens/sharedTokenColumns';
import {
  exitReasonLabel,
  exitReasonSearchText,
  isMetricExitReason,
  parseMetricExitParts,
} from 'lib/strategy/exitReason';

/** SOL storage → displayed unit for PriceUnit-aware numeric filters. */
function solFilter(n: number | null | undefined): number | null {
  return n == null ? null : amountInDisplayUnit(n, 'sol');
}

export { exitReasonLabel, exitReasonSearchText } from 'lib/strategy/exitReason';

// ---------------------------------------------------------------------------
// SINGLE SOURCE OF TRUTH for the strategy Positions / Matched / Sim tables.
//
// The target/entry/exit trade-leg columns (Price · Tokens · Size · Time · Tx)
// used to be hand-copied into `tpsl1/tableColumns` and `tpsl2/tableColumns` and
// had drifted (tpsl1 lost the target leg + tokens/size/tx; tpsl2's sim showed
// only price/time on entry/exit). `legColumns()` below emits one leg's columns
// from a prefix + field accessors, so every table renders the identical field
// set and the two strategies can never diverge again.
// ---------------------------------------------------------------------------

/** Render an exit reason as a compact colored badge, shared by the live-position
 * and simulation-result tables. Pass `pnlSol` so metric exits tint by outcome
 * (green / red), matching TP/SL glanceability. */
export function exitReasonBadge(
  reason: string | null | undefined,
  pnlSol?: number | null,
) {
  const label = exitReasonLabel(reason, pnlSol);
  switch (reason) {
    case 'LiquidityExit':
      return <span className="font-bold text-primary">{label}</span>;
    case 'TakeProfit':
      return <span className="font-bold text-green">{label}</span>;
    case 'StopLoss':
      return <span className="font-bold text-red">{label}</span>;
    case 'TrailingStop':
      return <span className="font-bold text-warning">{label}</span>;
    case 'Stall':
      return <span className="font-bold text-accent">{label}</span>;
    case 'TimeStop':
      return <span className="font-bold text-info">{label}</span>;
    case 'ExitFailed':
      return <span className="font-bold text-red">{label}</span>;
    case 'Manual':
    case 'ManualClose':
      return <span className="font-bold text-text-dim">{label}</span>;
    case 'Dead':
      return <span className="font-bold text-red">{label}</span>;
    case 'Migrated':
      return <span className="font-bold text-text-dim">{label}</span>;
    case 'NoEntry':
      return <span className="italic text-text-dim">{label}</span>;
    case 'Open':
    case null:
    case undefined:
    case '':
      return <span className="text-text-dim">{label}</span>;
    default: {
      // Bare `Metrics` + spaced `name op value` (and legacy compact).
      if (isMetricExitReason(reason)) {
        const tone =
          pnlSol != null && Number.isFinite(pnlSol) && pnlSol !== 0
            ? pnlSol > 0
              ? 'text-green'
              : 'text-red'
            : 'text-info';
        const parts = parseMetricExitParts(reason);
        if (parts) {
          // Visually separate name · op · value so the three tokens scan cleanly.
          return (
            <span className={cn('inline-flex items-baseline gap-1 font-bold', tone)}>
              <span className="opacity-90">{parts.name}</span>
              <span className="opacity-55">{parts.op}</span>
              {parts.value ? <span>{parts.value}</span> : null}
            </span>
          );
        }
        return <span className={cn('font-bold', tone)}>{label}</span>;
      }
      return <span className="text-text-dim">{label}</span>;
    }
  }
}

/** Derive SOL from a price and a token count — the bot stores TOKEN amounts; SOL
 * is never persisted, only shown. null when either input is missing. */
export const solOf = (price?: number | null, tokens?: number | null): number | null =>
  price != null && tokens != null ? price * tokens : null;

// --- Trade-leg column builder ----------------------------------------------

export type LegPrefix = 'target' | 'entry' | 'exit';
export type LegField = 'price' | 'tokens' | 'size' | 'time' | 'tx';

const ALL_LEG_FIELDS: LegField[] = ['price', 'tokens', 'size', 'time', 'tx'];
const LEG_CAP: Record<LegPrefix, string> = { target: 'Target', entry: 'Entry', exit: 'Exit' };

/** How to pull each of a leg's five sub-values off a row. `price` and `time` are
 *  required; `tokens`/`size`/`tx` are optional and the matching column is only
 *  emitted when its accessor is present. `size` defaults to `price × tokens`
 *  (`solOf`) when omitted but `tokens` is available — pass an explicit `size`
 *  for records that carry a real SOL field (e.g. the live monitor's entry_sol). */
export interface LegAccessors<T> {
  price: (r: T) => number | null;
  tokens?: (r: T) => number | null;
  size?: (r: T) => number | null;
  time: (r: T) => string | null;
  tx?: (r: T) => string | null;
}

export interface LegOpts {
  /** Sub-fields to emit, in order. Default: all five (those with data). */
  fields?: LegField[];
  tooltips?: Partial<Record<LegField, string>>;
  width?: Partial<Record<LegField, string>>;
}

/**
 * Build the columns for one trade leg (`target` / `entry` / `exit`). Emits, in
 * `fields` order: Price, Tokens, Size, Time, Tx — skipping any whose accessor is
 * absent (Tokens/Tx) or underivable (Size needs `size` or `tokens`). This is the
 * ONLY place a leg's price/tokens/size/time/tx cell is defined.
 */
export function legColumns<T>(prefix: LegPrefix, acc: LegAccessors<T>, opts: LegOpts = {}): ColumnDef<T>[] {
  const fields = opts.fields ?? ALL_LEG_FIELDS;
  const tip = opts.tooltips ?? {};
  const width = opts.width ?? {};
  const cap = LEG_CAP[prefix];
  const sizeOf = acc.size ?? ((r: T) => solOf(acc.price(r), acc.tokens?.(r)));
  const cols: ColumnDef<T>[] = [];

  for (const f of fields) {
    switch (f) {
      case 'price':
        cols.push({
          key: `${prefix}_price`,
          label: `${cap} Price`,
          tooltip: tip.price,
          group: prefix,
          sortable: true,
          width: width.price,
          render: (r) => {
            const v = acc.price(r);
            return v != null ? <CurrentPriceCell sol={v} /> : '—';
          },
          sortValue: (r) => acc.price(r),
          searchValue: (r) => String(acc.price(r) ?? ''),
          filterNumber: (r) => solFilter(acc.price(r)),
          filterAmount: 'sol',
        });
        break;
      case 'tokens':
        if (!acc.tokens) break;
        cols.push({
          key: `${prefix}_tokens`,
          label: `${cap} Tokens`,
          tooltip: tip.tokens,
          group: prefix,
          sortable: true,
          width: width.tokens,
          render: (r) => {
            const v = acc.tokens!(r);
            return v != null ? formatDecimalTrim(v, 3) : '—';
          },
          sortValue: (r) => acc.tokens!(r),
          searchValue: (r) => String(acc.tokens!(r) ?? ''),
        });
        break;
      case 'size':
        if (!acc.size && !acc.tokens) break;
        cols.push({
          key: `${prefix}_sol`,
          label: `${cap} Size`,
          tooltip: tip.size,
          group: prefix,
          sortable: true,
          width: width.size,
          render: (r) => {
            const v = sizeOf(r);
            return v != null ? <AmountCell sol={v} /> : '—';
          },
          sortValue: (r) => sizeOf(r),
          searchValue: (r) => String(sizeOf(r) ?? ''),
          filterNumber: (r) => solFilter(sizeOf(r)),
          filterAmount: 'sol',
        });
        break;
      case 'time':
        cols.push({
          key: `${prefix}_time`,
          label: `${cap} Time`,
          tooltip: tip.time,
          group: prefix,
          sortable: true,
          width: width.time,
          render: (r) => <DateCell iso={acc.time(r)} />,
          sortValue: (r) => acc.time(r) ?? '',
          searchValue: (r) => acc.time(r) ?? '',
        });
        break;
      case 'tx':
        if (!acc.tx) break;
        cols.push({
          key: `${prefix}_tx`,
          label: `${cap} Tx`,
          group: prefix,
          width: width.tx,
          render: (r) => {
            const v = acc.tx!(r);
            return v ? <AddressDisplay address={v} kind="transaction" display={v.slice(0, 6)} /> : '—';
          },
          searchValue: (r) => acc.tx!(r) ?? '',
        });
        break;
    }
  }
  return cols;
}

/** The identity Mint column, shared by every strategy table. */
function mintColumn<T extends { mint_address: string }>(): ColumnDef<T> {
  return {
    key: 'mint_address',
    label: 'Mint',
    group: 'identity',
    render: (r) => (
      <AddressDisplay address={r.mint_address} kind="token" display={r.mint_address.slice(0, 6)} />
    ),
    searchValue: (r) => r.mint_address,
  };
}

// --- Tooltip text (kept beside the tables so the two call sites can't drift) --

const TARGET_TOOLTIPS_POSITION: Partial<Record<LegField, string>> = {
  price:
    'Price of the scalp-entry trigger trade that armed this position. The gap vs. Entry Price is the realized slippage.',
  tokens: 'Token count of the trigger trade.',
  size: 'SOL size of the trigger trade (price × tokens).',
  time: 'Block time of the trigger trade. The gap vs. Entry Time is the entry latency.',
};
const TARGET_TOOLTIPS_SIM: Partial<Record<LegField, string>> = {
  ...TARGET_TOOLTIPS_POSITION,
  price:
    'Price of the scalp-entry trigger trade that armed this position. The gap vs. Entry Price is the modeled adverse slippage.',
};
const ENTRY_TOOLTIPS: Partial<Record<LegField, string>> = {
  tokens: 'Tokens bought at entry.',
  size: 'SOL spent at entry (entry price × tokens).',
};
const EXIT_TOOLTIPS: Partial<Record<LegField, string>> = {
  tokens: 'Tokens sold at exit.',
  size: 'SOL received at exit (exit price × tokens).',
};

// Price/amount cells read the unit + USD rate from context themselves (see
// priceCells), so these column arrays are referentially stable across a rate
// tick: only the rate-aware cells re-render, not the whole table.
export const positionColumns: ColumnDef<RulePositionRecord>[] = [
  mintColumn<RulePositionRecord>(),
  ...coreTokenColumns(),
  ...legColumns<RulePositionRecord>(
    'target',
    {
      price: (r) => r.target_price,
      tokens: (r) => r.target_token_amount,
      time: (r) => r.target_time,
      tx: (r) => r.target_tx,
    },
    { tooltips: TARGET_TOOLTIPS_POSITION },
  ),
  ...legColumns<RulePositionRecord>(
    'entry',
    {
      price: (r) => r.entry_price,
      tokens: (r) => r.entry_token_amount,
      time: (r) => r.entry_time,
      tx: (r) => r.entry_tx,
    },
    { tooltips: ENTRY_TOOLTIPS },
  ),
  ...legColumns<RulePositionRecord>(
    'exit',
    {
      price: (r) => r.exit_price,
      tokens: (r) => r.exit_token_amount,
      time: (r) => r.exit_time,
      tx: (r) => r.exit_tx,
    },
    { tooltips: EXIT_TOOLTIPS },
  ),
  {
    key: 'holding',
    label: 'Holding',
    group: 'pnl',
    sortable: true,
    render: (r) =>
      r.exit_token_amount != null ? formatDecimalTrim(r.exit_token_amount, 3) : '—',
    sortValue: (r) => r.exit_token_amount ?? null,
    searchValue: () => '',
    filterNumber: (r) => r.exit_token_amount ?? null,
  },
  {
    key: 'pnl_pct',
    label: 'PnL%',
    group: 'pnl',
    sortable: true,
    render: (r) => {
      if (r.pnl_percent == null) return <span className="text-text-dim">—</span>;
      return (
        <span className={cn('tabular-nums', pctGradeClass(r.pnl_percent))}>
          {formatSignedPct(r.pnl_percent, 1)}
        </span>
      );
    },
    sortValue: (r) => r.pnl_percent,
    searchValue: (r) => String(r.pnl_percent ?? ''),
    filterNumber: (r) => r.pnl_percent ?? null,
  },
  {
    key: 'pnl_sol',
    label: 'PnL',
    group: 'pnl',
    sortable: true,
    render: (r) => {
      if (r.exit_price == null || r.pnl_sol == null)
        return <span className="text-text-dim">—</span>;
      // `pnl_sol` is the backend's realized SOL PnL — the canonical win/loss
      // basis (mirrors `StrategyPosition::is_win`/`positions_summary`), so color
      // off `pnl_sol` itself rather than `pnl_percent` (price-basis; can disagree
      // with SOL-basis under slippage/fees in real mode).
      return (
        <span className={cn('font-bold', signedToneClass(r.pnl_sol))}>
          <AmountCell sol={r.pnl_sol} />
        </span>
      );
    },
    sortValue: (r) => r.pnl_sol ?? null,
    searchValue: () => '',
    filterNumber: (r) => solFilter(r.pnl_sol),
    filterAmount: 'sol',
  },
  {
    key: 'status',
    label: 'Status',
    group: 'state',
    sortable: true,
    render: (r) => {
      if (r.status === 'Arming') return <span className="italic text-text-dim">Arming</span>;
      if (r.status === 'BuySubmitted') return <span className="italic text-warning">Buying…</span>;
      // A sell is in flight (manual "Sell ALL", a rule Stop & close, or a ladder/time
      // exit): the position is marked ExitPending and the on-chain sell is running.
      // Amber "Selling…" so the row itself shows live progress, not just the button.
      if (r.status === 'ExitPending') return <span className="italic text-warning">Selling…</span>;
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
    render: (r) => exitReasonBadge(r.exit_reason, r.pnl_sol),
    sortValue: (r) => exitReasonLabel(r.exit_reason, r.pnl_sol),
    searchValue: (r) => exitReasonSearchText(r.exit_reason, r.pnl_sol),
    filterValue: (r) => exitReasonSearchText(r.exit_reason, r.pnl_sol),
  },
];

export const matchedColumns: ColumnDef<MatchedTokenRecord>[] = [
  {
    key: 'symbol',
    label: 'Symbol',
    group: 'identity',
    sortable: true,
    render: (r) => <AddressDisplay address={r.mint_address} kind="token" display={r.symbol} />,
    sortValue: (r) => r.symbol,
    searchValue: (r) => `${r.symbol} ${r.name}`,
  },
  mintColumn<MatchedTokenRecord>(),
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
    render: (r) => <AddressDisplay address={r.mint_address} kind="token" display={r.symbol} />,
    sortValue: (r) => r.symbol,
    searchValue: (r) => r.symbol,
  },
  mintColumn<SimulatedTokenResult>(),
  {
    key: 'fired',
    label: 'Fired',
    group: 'result',
    sortable: true,
    render: (r) => {
      const fired = r.fired !== false && r.exit_reason !== 'NoEntry';
      return (
        <span className={fired ? 'text-success' : 'text-text-dim'}>{fired ? 'Yes' : 'No'}</span>
      );
    },
    searchValue: (r) =>
      r.fired !== false && r.exit_reason !== 'NoEntry' ? 'yes' : 'no',
    sortValue: (r) => (r.fired !== false && r.exit_reason !== 'NoEntry' ? 1 : 0),
  },
  ...coreTokenColumns(new Set(['symbol', 'mint_address'])),
  ...legColumns<SimulatedTokenResult>(
    'target',
    {
      price: (r) => r.target_price,
      tokens: (r) => r.target_token_amount,
      time: (r) => r.target_time,
      tx: (r) => r.target_tx,
    },
    { tooltips: TARGET_TOOLTIPS_SIM },
  ),
  ...legColumns<SimulatedTokenResult>(
    'entry',
    {
      price: (r) => r.entry_price,
      tokens: (r) => r.entry_token_amount,
      time: (r) => r.entry_time,
      tx: (r) => r.entry_tx,
    },
    { tooltips: ENTRY_TOOLTIPS },
  ),
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
  // The sim result payload carries no exit token count (unlike a live position),
  // so the exit leg omits Tokens + the derived Size — the builder drops both when
  // no `tokens`/`size` accessor is given. Add `exit_token_amount` to the sim
  // pipeline to light them up.
  ...legColumns<SimulatedTokenResult>(
    'exit',
    {
      price: (r) => r.exit_price,
      time: (r) => r.exit_time,
      tx: (r) => r.exit_tx,
    },
    { tooltips: EXIT_TOOLTIPS },
  ),
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
    filterNumber: (r) => r.holding_secs ?? null,
  },
  {
    key: 'pnl_pct',
    label: 'PnL%',
    group: 'pnl',
    sortable: true,
    render: (r) => {
      if (r.pnl_percent == null) return <span className="text-text-dim">—</span>;
      return (
        <span className={cn('tabular-nums', pctGradeClass(r.pnl_percent))}>
          {formatSignedPct(r.pnl_percent, 1)}
        </span>
      );
    },
    sortValue: (r) => r.pnl_percent,
    searchValue: (r) => String(r.pnl_percent ?? ''),
    filterNumber: (r) => r.pnl_percent ?? null,
  },
  {
    key: 'pnl_sol',
    label: 'PnL',
    group: 'pnl',
    sortable: true,
    render: (r) => {
      if (r.pnl_sol == null) return <span className="text-text-dim">—</span>;
      return (
        <span className={cn('font-bold', signedToneClass(r.pnl_sol))}>
          <AmountCell sol={r.pnl_sol} />
        </span>
      );
    },
    sortValue: (r) => r.pnl_sol,
    searchValue: () => '',
    filterNumber: (r) => solFilter(r.pnl_sol),
    filterAmount: 'sol',
  },
  {
    key: 'reason',
    label: 'Reason',
    group: 'result',
    sortable: true,
    render: (r) => exitReasonBadge(r.exit_reason, r.pnl_sol),
    sortValue: (r) => exitReasonLabel(r.exit_reason, r.pnl_sol),
    searchValue: (r) => exitReasonSearchText(r.exit_reason, r.pnl_sol),
    filterValue: (r) => exitReasonSearchText(r.exit_reason, r.pnl_sol),
  },
  // Trade count comes from the shared "Token Trades" enrichment column
  // (`trade_count`), appended by `TokenTable` — the sim no longer carries its own.
];

// `*_KEYS` are the column keys each table already renders, so `TokenTable` skips
// them when appending the shared token-info set. Derived straight from the column
// arrays above so they can NEVER drift from what is actually shown.
export const POSITION_KEYS = new Set(positionColumns.map((c) => c.key));
export const MATCHED_KEYS = new Set(matchedColumns.map((c) => c.key));
export const SIM_KEYS = new Set(simColumns.map((c) => c.key));
