import { useCallback, useEffect, useMemo, useState } from 'react';
import type { ColumnDef } from 'components/table/types';
import { tokenColumns } from 'components/tokens/tokenColumns';
import { TokenTable } from 'components/tokens/TokenTable';
import { ALL_TOKEN_INFO_KEYS } from 'components/tokens/sharedTokenColumns';
import { DateTimeRangePicker } from 'components/ui/DateTimeRangePicker';
import { IconButton } from 'components/ui/IconButton';
import { SearchIcon, SpinnerIcon } from 'components/ui/icons';
import { Input } from 'components/ui/Input';
import { TraderChartCardExtra } from '@lab/components/analysis/TraderChartCardExtra';
import { WalletAnalyticsPanel } from '@lab/components/analysis/WalletAnalyticsPanel';
import { filterTraderRowsByFocus } from '@lab/components/analysis/walletFocus';
import { LazyLabTokenInspectModal } from '@lab/components/strategy/LazyLabTokenInspectModal';
import { inspectFromMint } from 'components/strategy/inspectTarget';
import type { PositionFocusLens } from 'lib/strategy/positionFocus';
import { Select } from 'components/ui/Select';
import { SectionDivider } from 'components/ui/SectionDivider';
import { useTimezone } from 'context/TimezoneContext';
import { apiErrorMessage } from 'store/apiSlice';
import { useGetTraderTokensQuery } from '@lab/store/labEndpoints';
import { useProfileWallets } from 'hooks/useProfileWallets';
import type { ProfileWalletInfo } from 'components/token-price-chart/types';
import type { TraderTokenRow } from 'types';

// Look-back clamp mirrors the backend. Max tokens uses the zero-as-unbound
// sentinel (`0` / blank ⇒ every mint in the window) — same as mint-trades
// `limit<=0` and the rule editor's Max total.
const DEFAULT_DAYS = 7;
const DEFAULT_LIMIT = 0;
const MAX_DAYS = 90;

const TRADER_LOOKBACK_PRESETS = [
  { value: '1', label: '1 day' },
  { value: '3', label: '3 days' },
  { value: '7', label: '7 days' },
  { value: '14', label: '14 days' },
  { value: '30', label: '30 days' },
  { value: '60', label: '60 days' },
  { value: '90', label: '90 days' },
] as const;

/** Stable empty reference so derived memos don't recompute while loading. */
const EMPTY_ROWS: TraderTokenRow[] = [];

/** A committed query — only set on Analyze, so editing the inputs doesn't
 *  refetch mid-typing. */
interface TraderQuery {
  wallet: string;
  days: number;
  limit: number;
}

const shortAddr = (a: string) => `${a.slice(0, 4)}…${a.slice(-4)}`;

/** The tracked wallets grouped by their profile name, for the picker's optgroups.
 *  `mine`-profile wallets sort first so your own wallet is easy to reach. */
function groupByProfile(wallets: ProfileWalletInfo[]): { profileName: string; wallets: ProfileWalletInfo[] }[] {
  const order: string[] = [];
  const byName = new Map<string, ProfileWalletInfo[]>();
  for (const w of wallets) {
    const name = w.profileName ?? 'Untitled';
    let bucket = byName.get(name);
    if (!bucket) {
      bucket = [];
      byName.set(name, bucket);
      order.push(name);
    }
    bucket.push(w);
  }
  return order
    .map((profileName) => ({ profileName, wallets: byName.get(profileName)! }))
    .sort((a, b) => Number(b.wallets[0]?.isMine) - Number(a.wallets[0]?.isMine));
}

const clampInt = (raw: string, fallback: number, min: number, max: number) => {
  const n = parseInt(raw, 10);
  return Math.min(max, Math.max(min, Number.isFinite(n) ? n : fallback));
};

/** Parse Max tokens: blank / 0 / non-finite ⇒ 0 (unlimited); positive stays as asked. */
const parseLimit = (raw: string) => {
  const trimmed = raw.trim();
  if (!trimmed) return 0;
  const n = parseInt(trimmed, 10);
  if (!Number.isFinite(n) || n <= 0) return 0;
  return n;
};

/**
 * Trader Analysis — paste a wallet address and see every token it traded in the
 * look-back window as the standard full token table (client-side sort / filter /
 * search — identical columns to every other token table), a wallet-level PnL
 * analytics deck (summary + interactive charts with focus chips, re-derived from
 * the table's current filtered cohort — see
 * `@lab/components/analysis/walletPnlStats.ts` / `walletFocus.ts`), and the shared
 * `TokenTable` Charts toggle for a per-token grid mirroring the current page.
 * Each chart card carries the wallet-specific stats (buys/sells/last traded,
 * reconstructed PnL, avg buy/sell price, open/partial-data flags) so the trader
 * dimension never duplicates the token columns.
 *
 * Every PnL figure is an avg-cost reconstruction over this per-mint grain, NOT a
 * true per-episode ledger (a wallet that re-entered a mint many times collapses
 * to one row) — see the backend `kernel::wallet_mint_pnl` doc comment.
 *
 * Scope caveat: only tokens this box ingests appear — a coin the wallet traded
 * that was never tracked won't show. Charts are lazily mounted when the toggle
 * is on.
 */
export function TraderAnalysisPage() {
  const { timezone } = useTimezone();
  const [walletInput, setWalletInput] = useState('');
  const [daysInput, setDaysInput] = useState(String(DEFAULT_DAYS));
  const [limitInput, setLimitInput] = useState(String(DEFAULT_LIMIT));
  const [query, setQuery] = useState<TraderQuery | null>(null);
  // Table column/search cohort (pre-pagination). Pinned when focus activates so
  // the analytics deck's parent base doesn't collapse to the focused slice
  // (same pin pattern as Sweep drill-in).
  const [filteredRows, setFilteredRows] = useState<TraderTokenRow[]>(EMPTY_ROWS);
  const [focus, setFocus] = useState<PositionFocusLens[]>([]);
  const [cohortPinned, setCohortPinned] = useState<TraderTokenRow[] | null>(null);
  // Row / chart-card select → lab token detail modal (mint-only; no fill markers).
  const [inspected, setInspected] = useState<{ mint: string; symbol?: string | null } | null>(
    null,
  );

  // Tracked wallets from the shared profiles cache (same SSOT the chart markers
  // read) — a picker to fill the wallet input from a saved profile wallet.
  const profileWallets = useProfileWallets();
  const profileGroups = useMemo(() => groupByProfile(profileWallets), [profileWallets]);
  // Reflect the picker's selection only while the input still holds a known
  // tracked address; typing a custom address falls back to the placeholder.
  const pickedWallet = profileWallets.some((w) => w.address === walletInput) ? walletInput : '';

  const {
    data,
    isFetching,
    error: rawError,
  } = useGetTraderTokensQuery(query ?? { wallet: '', days: DEFAULT_DAYS, limit: DEFAULT_LIMIT }, {
    skip: !query,
  });
  const error = apiErrorMessage(rawError, 'Failed to load trader tokens');
  const rows = data ?? EMPTY_ROWS;

  // New Analyze (wallet / days / limit) drops focus — a stale day lens from the
  // previous window would silently empty the table.
  useEffect(() => {
    setFocus([]);
    setCohortPinned(null);
    setFilteredRows(EMPTY_ROWS);
    setInspected(null);
  }, [query]);

  // Exactly the standard shared token columns — identical to every other token
  // table (SSOT), no additions/removals. The wallet-specific data lives in the
  // chart cards below, not here, so nothing duplicates the token columns. Rows
  // arrive recent-first from the backend, which is the default order.
  const columns = useMemo(
    () => tokenColumns() as unknown as ColumnDef<TraderTokenRow>[],
    [],
  );

  const run = (walletOverride?: string) => {
    const wallet = (walletOverride ?? walletInput).trim();
    if (!wallet) return;
    setQuery({
      wallet,
      days: clampInt(daysInput, DEFAULT_DAYS, 1, MAX_DAYS),
      limit: parseLimit(limitInput),
    });
  };

  // Picking a tracked wallet fills the input and analyzes it immediately (state
  // is async, so pass the address straight to `run`).
  const handlePickWallet = (address: string) => {
    if (!address) return;
    setWalletInput(address);
    run(address);
  };

  // Stable identity so DataTable's memoized pageRows/processed effects don't churn.
  const handleFilteredRows = useCallback(
    (r: TraderTokenRow[]) => {
      // Only track column/search filters while unfocused — once focus is on, the
      // table's row set is already narrowed and would collapse the chart base.
      if (focus.length === 0) setFilteredRows(r);
    },
    [focus.length],
  );

  const onFocusChange = useCallback(
    (next: PositionFocusLens[]) => {
      if (next.length > 0 && focus.length === 0) {
        // Prefer the table's column/search cohort; fall back to the full API
        // set if DataTable hasn't reported filtered rows yet.
        setCohortPinned(filteredRows.length > 0 ? filteredRows : rows);
      }
      if (next.length === 0) setCohortPinned(null);
      setFocus(next);
    },
    [focus.length, filteredRows, rows],
  );

  // Prefer the live table-filter report; fall back to the full API set so the
  // deck paints before DataTable's first onFilteredRowsChange.
  const tableFilterCohort = cohortPinned ?? (filteredRows.length > 0 ? filteredRows : rows);
  const focusOpts = useMemo(() => ({ timeZone: timezone }), [timezone]);

  // Focus narrows the table's input set (client-side); column filters still
  // apply inside DataTable on top of this. When unfocused, pass the full API set.
  const rowsForTable = useMemo(() => {
    if (focus.length === 0) return rows;
    return filterTraderRowsByFocus(rows, focus, focusOpts);
  }, [rows, focus, focusOpts]);

  return (
    <div className="p-4">
      <h2 className="text-lg font-extrabold text-text">Trader Analysis</h2>
      <p className="mt-0.5 text-xs text-text-dim">
        Every token a wallet traded in the window — full token table (sort / filter /
        search) plus a synced charts grid with its buys/sells spotlighted. Recent trade
        first. Only tokens this box ingests appear.
      </p>

      <SectionDivider />

      {/* Inputs */}
      <div className="mb-3 flex flex-wrap items-end gap-3">
        <label className="flex flex-col gap-1 text-[10px] font-bold uppercase tracking-widest text-text-dim">
          Wallet address
          <Input
            value={walletInput}
            onChange={(e) => setWalletInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') run();
            }}
            placeholder="Solana base58 address"
            className="min-w-[420px] font-mono font-normal normal-case tracking-normal"
          />
        </label>
        {profileWallets.length > 0 && (
          <label className="flex flex-col gap-1 text-[10px] font-bold uppercase tracking-widest text-text-dim">
            Tracked wallet
            <Select
              value={pickedWallet}
              onChange={(e) => handlePickWallet(e.target.value)}
              className="min-w-[200px] font-normal normal-case tracking-normal"
            >
              <option value="">Pick a profile wallet…</option>
              {profileGroups.map((group) => (
                <optgroup key={group.profileName} label={group.profileName}>
                  {group.wallets.map((w) => (
                    <option key={w.address} value={w.address}>
                      {shortAddr(w.address)}
                    </option>
                  ))}
                </optgroup>
              ))}
            </Select>
          </label>
        )}
        <label className="flex flex-col gap-1 text-[10px] font-bold uppercase tracking-widest text-text-dim">
          Look-back
          <DateTimeRangePicker
            aria-label="Look-back days"
            size="sm"
            zoneLabel={null}
            allowCustom={false}
            emptyLabel="Days"
            presets={[...TRADER_LOOKBACK_PRESETS]}
            value={{ preset: daysInput, from: '', to: '' }}
            onChange={({ preset }) => setDaysInput(preset)}
          />
        </label>
        <label className="flex flex-col gap-1 text-[10px] font-bold uppercase tracking-widest text-text-dim">
          Max tokens
          <Input
            type="number"
            min={0}
            blankZero
            placeholder="∞"
            value={limitInput}
            onChange={(e) => setLimitInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') run();
            }}
            className="w-[110px] font-normal normal-case tracking-normal"
            title="Blank or 0 = every token in the window"
          />
        </label>
        <IconButton
          variant="primary"
          size="lg"
          onClick={() => run()}
          disabled={isFetching || !walletInput.trim()}
          label={isFetching ? 'Loading…' : 'Analyze'}
          title={isFetching ? 'Loading…' : 'Analyze'}
        >
          {isFetching ? <SpinnerIcon /> : <SearchIcon />}
        </IconButton>
      </div>

      {error && <p className="mb-2 text-sm text-red">{error}</p>}

      {query && !isFetching && !error && (
        <p className="mb-3 text-xs text-text-dim">
          {rows.length === 0
            ? 'No tracked tokens traded by this wallet in the window.'
            : `${rows.length} token${rows.length === 1 ? '' : 's'} traded by ${shortAddr(query.wallet)} in the last ${query.days}d`}
        </p>
      )}

      {/* Wallet-level PnL analytics — summary + interactive chart deck. Driven by
          the table's FULL filtered cohort (pinned under focus), so a column
          filter/search here also re-scopes every chart. Lives above the table
          since this is the "is this wallet good" answer, not a per-token drill-down. */}
      {query && !isFetching && !error && (
        <WalletAnalyticsPanel
          rows={tableFilterCohort}
          timezone={timezone}
          focus={focus}
          onFocusChange={onFocusChange}
        />
      )}

      {/* Token table — shared `TokenTable` client mode + Charts toggle (defaults
          on; persisted per tableId). Wallet stats ride `renderChartCardExtra` so
          the trader dimension never duplicates the token columns. Focus narrows
          the input set; the analytics panel above is fed by the pinned
          table-filter cohort so timing charts keep their parent base. */}
      {query && rows.length > 0 && (
        <TokenTable
          columns={columns}
          rows={rowsForTable}
          existingKeys={ALL_TOKEN_INFO_KEYS}
          mintSetFilter
          charts
          chartsDefaultOn
          searchable
          colFilters
          colToggle
          hoverable
          loading={isFetching}
          tableId="trader_analysis_tokens"
          highlightWallet={query.wallet}
          titleOf={(r) => r.symbol || r.name || shortAddr(r.mint_address)}
          selectedKey={inspected?.mint ?? null}
          onSelect={(mint) => {
            const row = mint ? rowsForTable.find((r) => r.mint_address === mint) : null;
            setInspected(mint ? { mint, symbol: row?.symbol } : null);
          }}
          onFilteredRowsChange={handleFilteredRows}
          emptyMessage="No tokens match the filters"
          renderChartCardExtra={(row) => <TraderChartCardExtra row={row} timezone={timezone} />}
        />
      )}

      {inspected && (
        <LazyLabTokenInspectModal
          target={inspectFromMint(inspected.mint, inspected.symbol)}
          titleSuffix="Token inspect"
          onClose={() => setInspected(null)}
        />
      )}
    </div>
  );
}
