import { useCallback, useMemo, useState, type ReactNode } from 'react';
import { DataTable } from 'components/table/DataTable';
import type { ColumnDef, SortDir, TableQuery } from 'components/table/types';
import type { FilterSpec } from 'components/table/numericFilter';
import { appendedTokenColumns } from './sharedTokenColumns';
import { MintSetInput } from './MintSetInput';
import { TokenChartsGrid, type ChartOverlayHook } from './TokenChartsGrid';
import { cn } from 'lib/cn';

/** Persist the charts-grid toggle per `tableId` (localStorage; SSR/test safe). */
const CHARTS_PREF_PREFIX = 'mt:tablecharts:';
function loadChartsPref(tableId?: string): boolean {
  if (!tableId || typeof window === 'undefined') return false;
  try {
    return window.localStorage.getItem(CHARTS_PREF_PREFIX + tableId) === '1';
  } catch {
    return false;
  }
}
function saveChartsPref(tableId: string | undefined, on: boolean) {
  if (!tableId || typeof window === 'undefined') return;
  try {
    window.localStorage.setItem(CHARTS_PREF_PREFIX + tableId, on ? '1' : '0');
  } catch {
    /* ignore */
  }
}

/** Build the wrapper-injected `structuredFilters` for an active mint set (empty →
 *  none). The `in`-on-`mint_address` shape is defined once here — `mint_address` is
 *  the single token-data column key across the whole stack (SQL, grammar, JS field). */
function mintSetFilters(mints: string[]): Record<string, FilterSpec> | undefined {
  return mints.length ? { mint_address: { op: 'in', val: mints } } : undefined;
}

/**
 * The one component for every token-row table. It sits ON TOP of the pure
 * {@link DataTable} primitive and owns the "token recipe":
 *
 *   1. append the shared token-info columns (`appendedTokenColumns`, SSOT) after
 *      the caller's own columns — so every token table gets the same enrichment
 *      column set, defined once (keys already present are skipped via
 *      `existingKeys`);
 *   2. own the two token-table features — the mint-set paste filter
 *      (`mintSetFilter`) and the per-token charts grid (`charts`).
 *
 * Two modes:
 *   • **server** (`serverSide`) — rows arrive backend-enriched and one page at a
 *     time; paging/sort/filter round-trip (the emitted `TableQuery` is handed to the
 *     caller's hook). Mint-set is an `in`-op `structuredFilter`.
 *   • **client** (default) — rows are the FULL, already-enriched dataset;
 *     `DataTable`'s own client paging/sort/filter/search runs in-browser (NO separate
 *     evaluator — that TS twin retired with Wallet). Mint-set is a plain row
 *     pre-filter. Used by tables with no backend paging endpoint (Trader Analysis,
 *     Sweep drill-in).
 *
 * Every token-data row keys its mint under the one canonical field `mint_address`
 * (SSOT across DB → wire → JS), so the mint accessor is fixed internally — callers no
 * longer pass a `mintOf`. It drives the charts grid, the default `rowKey`, and the
 * client mint-set pre-filter, and matches the server mint-set column key.
 *
 * `DataTable` stays a general-purpose primitive with NO token vocabulary; the
 * dependency is one-way (`tokens/` → `table/`), asserted by
 * `components/table/DataTable.boundary.test.ts`.
 */
interface TokenTableCommon<R> {
  /** The caller's own (bespoke) columns. The shared token-info columns are
   *  appended after these; keys already present are skipped via `existingKeys`. */
  columns: ColumnDef<R>[];
  /** Rows. Server mode: the current page (verbatim). Client mode: the full dataset. */
  rows: R[];
  /** Column keys already rendered by `columns` (plus semantic aliases) — excluded
   *  from the appended token columns so enrichment never duplicates a column. Pass
   *  `ALL_TOKEN_INFO_KEYS` to append nothing (tables that own the full layout). */
  existingKeys: Set<string>;
  /** Row key. Defaults to the row's `mint_address`. Override for non-mint keys (e.g.
   *  positions key by `id`). */
  rowKey?: (row: R) => string;
  /** Change when an external control alters the result set → snap to page 1. */
  resetKey?: string | number;
  /** Opt-in: a paste-a-mint-set box whose set ANDs with the table's filters (server:
   *  an `in` op on `mint`; client: a row pre-filter). */
  mintSetFilter?: boolean;
  /** Opt-in: a "Charts" toggle rendering a per-token trade-chart grid for the CURRENT
   *  page below the table (lazy-mounted). Persisted per `tableId`. */
  charts?: boolean;
  /** Per-row entry/exit + swing overlay for the charts grid (when `charts` is on),
   *  matching the row's inspect modal. Called as a hook per card — see
   *  {@link ChartOverlayHook}. */
  useRowOverlay?: ChartOverlayHook<R>;
  /** Per-row extra rendered in a chart card's header when `charts` is on. */
  renderChartCardExtra?: (row: R) => ReactNode;
  /** Per-row chart-card title (else derived from the fetched detail). */
  titleOf?: (row: R) => string;
  /** Wallet to spotlight on every chart (Trader Analysis). */
  highlightWallet?: string | null;
  rowActions?: (row: R) => ReactNode;
  rowClassName?: (row: R) => string | undefined;
  cellGroupClassName?: (group: string | undefined, row: R) => string | undefined;
  selectedKey?: string | null;
  onSelect?: (key: string | null) => void;
  onVisibleRowsChange?: (rows: R[]) => void;
  groupLabels?: Record<string, string>;
  defaultSort?: { col: string; dir?: SortDir };
  selectable?: boolean;
  searchable?: boolean;
  colFilters?: boolean;
  colToggle?: boolean;
  hoverable?: boolean;
  loading?: boolean;
  tableId?: string;
  emptyMessage?: string;
  defaultPageSize?: number;
  pageSizeOptions?: number[];
}

interface TokenTableServerProps<R> extends TokenTableCommon<R> {
  serverSide: true;
  /** Full match count from the backend, so the pager sizes itself. */
  serverTotal: number;
  /** Emitted (debounced) view-state — the caller's hook serializes + fetches. */
  onQueryChange: (q: TableQuery) => void;
}

interface TokenTableClientProps<R> extends TokenTableCommon<R> {
  serverSide?: false;
}

export type TokenTableProps<R> = TokenTableServerProps<R> | TokenTableClientProps<R>;

/** The one mint accessor — every token-data row carries `mint_address` (SSOT). */
function mintAddressOf<R>(row: R): string {
  return (row as { mint_address?: string }).mint_address ?? '';
}

export function TokenTable<R>(props: TokenTableProps<R>) {
  const { charts, useRowOverlay, renderChartCardExtra, titleOf, highlightWallet, tableId, onVisibleRowsChange } = props;
  const mintOf = mintAddressOf;
  const [chartsOn, setChartsOn] = useState(() => (charts ? loadChartsPref(tableId) : false));
  const [visibleRows, setVisibleRows] = useState<R[]>([]);

  // When the grid is showing, mirror the table's current page into it; always
  // forward to the caller's own handler too. Gated so we don't churn state when
  // the grid is hidden — `DataTable` re-fires this on toggle (handler is a dep),
  // so switching on captures the current page immediately.
  const capture = useCallback(
    (rows: R[]) => {
      setVisibleRows(rows);
      onVisibleRowsChange?.(rows);
    },
    [onVisibleRowsChange],
  );
  const showGrid = !!charts && chartsOn;
  const childOnVisible = showGrid ? capture : onVisibleRowsChange;

  const toggleCharts = () =>
    setChartsOn((v) => {
      const next = !v;
      saveChartsPref(tableId, next);
      return next;
    });

  return (
    <>
      {charts && (
        <div className="mb-2 flex justify-end">
          <button
            type="button"
            onClick={toggleCharts}
            className={cn(
              'rounded-md border border-white/8 bg-white/4 px-2.5 py-1 text-[11px] font-semibold uppercase tracking-wider text-text-dim transition hover:text-text',
              chartsOn && 'border-primary/35 bg-primary/12 text-primary',
            )}
          >
            {chartsOn ? 'Charts ✓' : 'Charts'}
          </button>
        </div>
      )}
      {props.serverSide ? (
        <ServerTokenTable {...props} mintOf={mintOf} onVisibleRowsChange={childOnVisible} />
      ) : (
        <ClientTokenTable {...props} mintOf={mintOf} onVisibleRowsChange={childOnVisible} />
      )}
      {showGrid && (
        <TokenChartsGrid
          rows={visibleRows}
          titleOf={titleOf}
          highlightWallet={highlightWallet}
          chartTableId={tableId ? `${tableId}_charts` : undefined}
          useRowOverlay={useRowOverlay}
          renderChartCardExtra={renderChartCardExtra}
        />
      )}
    </>
  );
}

/** Server mode: append the token columns, then hand everything (rows are the current
 *  page verbatim) to `DataTable` in server-side mode. */
function ServerTokenTable<R>({
  columns,
  existingKeys,
  mintSetFilter,
  resetKey,
  onQueryChange,
  rowKey,
  mintOf,
  ...rest
}: TokenTableServerProps<R> & { mintOf: (row: R) => string }) {
  const cols = useMemo<ColumnDef<R>[]>(
    () => [...columns, ...appendedTokenColumns(existingKeys)],
    [columns, existingKeys],
  );

  const [mintSet, setMintSet] = useState<string[]>([]);
  // Fold the active mint set into every emitted query (ANDs with the table's own
  // filters). The set change rides `resetKey` so `DataTable` snaps to page 1 and
  // re-emits through this same `emit` — no manual re-fetch plumbing.
  const emit = useCallback(
    (q: TableQuery) => {
      const inj = mintSetFilters(mintSet);
      onQueryChange(inj ? { ...q, structuredFilters: { ...q.structuredFilters, ...inj } } : q);
    },
    [onQueryChange, mintSet],
  );
  const combinedResetKey = mintSetFilter
    ? `${resetKey ?? ''}::mints=${mintSet.join(',')}`
    : resetKey;

  return (
    <>
      {mintSetFilter && <MintSetInput appliedCount={mintSet.length} onChange={setMintSet} />}
      {/* `rest` carries serverTotal + rows + the shared passthroughs. */}
      <DataTable
        {...rest}
        serverSide
        columns={cols}
        rowKey={rowKey ?? mintOf}
        onQueryChange={emit}
        resetKey={combinedResetKey}
      />
    </>
  );
}

/** Client mode: append the token columns, apply the mint-set as a row pre-filter,
 *  then hand the full dataset to `DataTable`'s native (client-side) paging. */
function ClientTokenTable<R>({
  columns,
  existingKeys,
  mintSetFilter,
  resetKey,
  rows,
  rowKey,
  mintOf,
  ...rest
}: TokenTableClientProps<R> & { mintOf: (row: R) => string }) {
  const cols = useMemo<ColumnDef<R>[]>(
    () => [...columns, ...appendedTokenColumns(existingKeys)],
    [columns, existingKeys],
  );

  const [mintSet, setMintSet] = useState<string[]>([]);
  const shownRows = useMemo(() => {
    if (mintSet.length === 0) return rows;
    const set = new Set(mintSet);
    return rows.filter((r) => set.has(mintOf(r)));
  }, [rows, mintSet, mintOf]);
  const combinedResetKey = mintSetFilter
    ? `${resetKey ?? ''}::mints=${mintSet.join(',')}`
    : resetKey;

  return (
    <>
      {mintSetFilter && <MintSetInput appliedCount={mintSet.length} onChange={setMintSet} />}
      <DataTable
        {...rest}
        columns={cols}
        rows={shownRows}
        rowKey={rowKey ?? mintOf}
        resetKey={combinedResetKey}
      />
    </>
  );
}
