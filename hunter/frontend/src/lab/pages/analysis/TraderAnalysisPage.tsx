import { useCallback, useMemo, useState } from 'react';
import type { ColumnDef } from 'components/table/types';
import { tokenColumns } from 'components/tokens/tokenColumns';
import { TokenTable } from 'components/tokens/TokenTable';
import { ALL_TOKEN_INFO_KEYS } from 'components/tokens/sharedTokenColumns';
import { LazyTokenChartsGrid } from 'components/tokens/LazyTokenChartsGrid';
import { AmountCell, PriceCell } from 'components/tokens/priceCells';
import { Badge } from 'components/ui/Badge';
import { DateTimeRangePicker } from 'components/ui/DateTimeRangePicker';
import { IconButton } from 'components/ui/IconButton';
import { SearchIcon, SpinnerIcon } from 'components/ui/icons';
import { Input } from 'components/ui/Input';
import { WalletAnalyticsPanel } from '@lab/components/analysis/WalletAnalyticsPanel';
import { Select } from 'components/ui/Select';
import { SectionDivider } from 'components/ui/SectionDivider';
import { useTimezone } from 'context/TimezoneContext';
import { formatTimestampMs } from 'utils/date';
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
 * analytics panel (summary stats + heatmap/equity-curve/ranked/distribution/
 * scatter, re-derived from the table's current filtered cohort — see
 * `@lab/components/analysis/walletPnlStats.ts`), and a synced charts grid below
 * that mirrors the table's current sort/filter/page. Each chart card carries the
 * wallet-specific stats (buys/sells/last traded, reconstructed PnL, avg buy/sell
 * price, open/partial-data flags) so the trader dimension never duplicates the
 * token columns.
 *
 * Every PnL figure is an avg-cost reconstruction over this per-mint grain, NOT a
 * true per-episode ledger (a wallet that re-entered a mint many times collapses
 * to one row) — see the backend `kernel::wallet_mint_pnl` doc comment.
 *
 * Scope caveat: only tokens this box ingests appear — a coin the wallet traded
 * that was never tracked won't show. Charts are lazily mounted on scroll.
 */
export function TraderAnalysisPage() {
  const { timezone } = useTimezone();
  const [walletInput, setWalletInput] = useState('');
  const [daysInput, setDaysInput] = useState(String(DEFAULT_DAYS));
  const [limitInput, setLimitInput] = useState(String(DEFAULT_LIMIT));
  const [query, setQuery] = useState<TraderQuery | null>(null);
  // The rows the table currently shows (after sort/filter/paging) — drives the
  // charts grid so both stay in sync. Fed by DataTable's onVisibleRowsChange.
  const [visibleRows, setVisibleRows] = useState<TraderTokenRow[]>(EMPTY_ROWS);
  // The FULL filtered cohort (post search/column-filter, pre-pagination) — feeds
  // the wallet PnL analytics panel (summary + heatmap/equity/ranked/distribution/
  // scatter), so filtering the token table re-derives every chart for free
  // instead of the analytics panel silently only covering page 1.
  const [filteredRows, setFilteredRows] = useState<TraderTokenRow[]>(EMPTY_ROWS);

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
  const handleVisibleRows = useCallback((r: TraderTokenRow[]) => setVisibleRows(r), []);
  const handleFilteredRows = useCallback((r: TraderTokenRow[]) => setFilteredRows(r), []);

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
            zoneLabel="UTC"
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

      {/* Wallet-level PnL analytics — summary stat row + a tabbed chart panel
          (heatmap / equity curve / ranked / distribution / scatter). Driven by
          the table's FULL filtered cohort, so a column filter/search here also
          re-scopes every chart below. Lives above the table since this is the
          "is this wallet good" answer, not a per-token drill-down. */}
      {query && !isFetching && !error && (
        <WalletAnalyticsPanel rows={filteredRows} timezone={timezone} />
      )}

      {/* Token table — routed through the shared `TokenTable` (client mode: rows are
          the full set, `DataTable` pages in-browser). Standard shared token columns
          (append nothing — `tokenColumns()` already lays out the full set). Rows
          arrive recent-first from the backend (no defaultSort). The synced charts
          grid below is fed by the table's current on-screen rows; the analytics
          panel above is fed by its full filtered cohort. */}
      {query && rows.length > 0 && (
        <TokenTable
          columns={columns}
          rows={rows}
          existingKeys={ALL_TOKEN_INFO_KEYS}
          mintSetFilter
          searchable
          colFilters
          colToggle
          hoverable
          loading={isFetching}
          tableId="trader_analysis_tokens"
          onVisibleRowsChange={handleVisibleRows}
          onFilteredRowsChange={handleFilteredRows}
          emptyMessage="No tokens match the filters"
        />
      )}

      {/* Charts grid — the shared grid, mirroring the table's current sort/filter/
          page. The per-wallet stats (buys/sells/last, PnL, avg prices, open/
          partial flags) ride the card-header slot so the trader dimension never
          duplicates the token columns (see `walletPnlStats.ts` / the wallet-level
          analytics panel above for the aggregate view across every token). */}
      {visibleRows.length > 0 && query && (
        <LazyTokenChartsGrid
          rows={visibleRows}
          titleOf={(r) => r.symbol || r.name || shortAddr(r.mint_address)}
          highlightWallet={query.wallet}
          chartTableId="trader_analysis_trades"
          renderChartCardExtra={(row) => (
            <span className="ml-auto flex flex-col items-end gap-1 rounded-md border border-white/8 bg-white/3 px-2 py-1 text-[11px]">
              <span className="flex items-center gap-2">
                <span className="font-bold uppercase tracking-wide text-text-dim">This wallet</span>
                <span className="text-buy">{row.wallet_buy_count} buys</span>
                <span className="text-sell">{row.wallet_sell_count} sells</span>
                <span className="text-text-dim">
                  last {formatTimestampMs(Date.parse(row.wallet_last_trade_at), timezone)}
                </span>
              </span>
              <span className="flex items-center gap-2">
                <span className={`font-bold ${row.wallet_total_pnl_sol >= 0 ? 'text-green' : 'text-red'}`}>
                  <AmountCell sol={row.wallet_total_pnl_sol} /> PnL
                </span>
                <span className="text-text-dim">
                  avg buy <PriceCell sol={row.wallet_avg_buy_price} />
                  {row.wallet_avg_sell_price != null && (
                    <>
                      {' '}
                      · sell <PriceCell sol={row.wallet_avg_sell_price} />
                    </>
                  )}
                </span>
                {row.wallet_is_open && (
                  <Badge variant="info" size="sm">
                    open
                  </Badge>
                )}
                {row.wallet_partial_data && (
                  <Badge
                    variant="warning"
                    size="sm"
                    title="Sold more than bought in this window — the position predates the look-back, so this PnL is a partial estimate"
                  >
                    partial
                  </Badge>
                )}
              </span>
            </span>
          )}
        />
      )}
    </div>
  );
}
