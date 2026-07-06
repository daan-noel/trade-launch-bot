import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { DataTable } from 'components/table/DataTable';
import type { ColumnDef } from 'components/table/types';
import { tokenColumns } from 'components/tokens/tokenColumns';
import { TokenTradeChart } from 'components/tokens/TokenTradeChart';
import { Button } from 'components/ui/Button';
import { Input } from 'components/ui/Input';
import { Select } from 'components/ui/Select';
import { SectionDivider } from 'components/ui/SectionDivider';
import { useTimezone } from 'context/TimezoneContext';
import { formatTimestampMs } from 'utils/date';
import { apiErrorMessage, useGetTokenDetailQuery } from 'store/apiSlice';
import { useGetTraderTokensQuery } from '@lab/store/labEndpoints';
import { useProfileWallets } from 'hooks/useProfileWallets';
import type { ProfileWalletInfo } from 'components/token-price-chart/types';
import type { TraderTokenRow } from 'types';
import { AddressDisplay } from '@shared/components/ui/AddressDisplay';

// Look-back + token-count bounds mirror the backend clamps so the UI can't ask
// for more than the endpoint will return.
const DEFAULT_DAYS = 7;
const DEFAULT_LIMIT = 50;
const MAX_DAYS = 90;
const MAX_LIMIT = 300;

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

/**
 * Defer mounting `children` until the placeholder scrolls near the viewport.
 * Each token chart fires its own detail + trades fetch on mount, so with up to
 * `MAX_LIMIT` tokens on the page we must NOT mount them all at once — this keeps
 * the fan-out to what's on screen (plus a 400px pre-load margin). Once shown, it
 * stays mounted (a re-sort reorders the DOM but keeps charts mounted via key).
 */
function LazyMount({ minHeight = 380, children }: { minHeight?: number; children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  const [shown, setShown] = useState(false);

  useEffect(() => {
    if (shown) return;
    const el = ref.current;
    if (!el) return;
    const obs = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) {
          setShown(true);
          obs.disconnect();
        }
      },
      { rootMargin: '400px' },
    );
    obs.observe(el);
    return () => obs.disconnect();
  }, [shown]);

  return (
    <div ref={ref} style={shown ? undefined : { minHeight }}>
      {shown ? children : null}
    </div>
  );
}

/** One token's card: its trade-history chart with the input wallet's markers
 *  spotlighted (plus every other tracked wallet's markers, via TokenTradeChart).
 *  Fetches its own detail on mount — only mounted once scrolled into view. */
function TraderTokenCard({ row, wallet }: { row: TraderTokenRow; wallet: string }) {
  const { timezone } = useTimezone();
  const { data: detail } = useGetTokenDetailQuery(row.mint_address, {
    skip: !row.mint_address,
  });
  const title = row.symbol || row.name || shortAddr(row.mint_address);
  const lastTraded = formatTimestampMs(Date.parse(row.wallet_last_trade_at), timezone);

  return (
    <div className="rounded-lg border border-white/8 bg-bg-card/40 p-4">
      <div className="mb-2 flex flex-wrap items-center gap-2">
        <span className="text-sm font-bold text-text">{title}</span>
        <AddressDisplay address={row.mint_address} kind="token" truncate={false} actionsPlacement='right' iconSize='sm' />
        {/* Wallet-specific stats live here (not in the token table) — the trader
            dimension for this token. */}
        <span className="ml-auto inline-flex items-center gap-2 rounded-md border border-white/8 bg-white/3 px-2 py-0.5 text-[11px]">
          <span className="font-bold uppercase tracking-wide text-text-dim">This wallet</span>
          <span className="text-green">{row.wallet_buy_count} buys</span>
          <span className="text-red">{row.wallet_sell_count} sells</span>
          <span className="text-text-dim">last {lastTraded}</span>
        </span>
      </div>
      <TokenTradeChart
        key={row.mint_address}
        detail={detail ?? null}
        highlightWallet={wallet}
        tableId="trader_analysis_trades"
      />
    </div>
  );
}

/**
 * Trader Analysis — paste a wallet address and see every token it traded in the
 * look-back window as the standard full token table (client-side sort / filter /
 * search — identical columns to every other token table), plus a synced charts
 * grid below that mirrors the table's current sort/filter/page. Each chart card
 * carries the wallet-specific stats (buys / sells / last traded) so the trader
 * dimension never duplicates the token columns.
 *
 * Scope caveat: only tokens this box ingests appear — a coin the wallet traded
 * that was never tracked won't show. Charts are lazily mounted on scroll.
 */
export function TraderAnalysisPage() {
  const [walletInput, setWalletInput] = useState('');
  const [daysInput, setDaysInput] = useState(String(DEFAULT_DAYS));
  const [limitInput, setLimitInput] = useState(String(DEFAULT_LIMIT));
  const [query, setQuery] = useState<TraderQuery | null>(null);
  // The rows the table currently shows (after sort/filter/paging) — drives the
  // charts grid so both stay in sync. Fed by DataTable's onVisibleRowsChange.
  const [visibleRows, setVisibleRows] = useState<TraderTokenRow[]>(EMPTY_ROWS);

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
      limit: clampInt(limitInput, DEFAULT_LIMIT, 1, MAX_LIMIT),
    });
  };

  // Picking a tracked wallet fills the input and analyzes it immediately (state
  // is async, so pass the address straight to `run`).
  const handlePickWallet = (address: string) => {
    if (!address) return;
    setWalletInput(address);
    run(address);
  };

  // Stable identity so DataTable's memoized pageRows effect doesn't churn.
  const handleVisibleRows = useCallback((r: TraderTokenRow[]) => setVisibleRows(r), []);

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
          Days
          <Input
            type="number"
            min={1}
            max={MAX_DAYS}
            value={daysInput}
            onChange={(e) => setDaysInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') run();
            }}
            className="w-[90px] font-normal normal-case tracking-normal"
          />
        </label>
        <label className="flex flex-col gap-1 text-[10px] font-bold uppercase tracking-widest text-text-dim">
          Max tokens
          <Input
            type="number"
            min={1}
            max={MAX_LIMIT}
            value={limitInput}
            onChange={(e) => setLimitInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') run();
            }}
            className="w-[110px] font-normal normal-case tracking-normal"
          />
        </label>
        <Button variant="primary" onClick={() => run()} disabled={isFetching || !walletInput.trim()}>
          {isFetching ? 'Loading…' : 'Analyze'}
        </Button>
      </div>

      {error && <p className="mb-2 text-sm text-red">{error}</p>}

      {query && !isFetching && !error && (
        <p className="mb-3 text-xs text-text-dim">
          {rows.length === 0
            ? 'No tracked tokens traded by this wallet in the window.'
            : `${rows.length} token${rows.length === 1 ? '' : 's'} traded by ${shortAddr(query.wallet)} in the last ${query.days}d`}
        </p>
      )}

      {/* Token table — the standard shared token columns, client-side sort /
          filter / search. Rows arrive recent-first from the backend (no
          defaultSort, so that order holds until the user sorts). */}
      {query && rows.length > 0 && (
        <DataTable
          columns={columns}
          rows={rows}
          rowKey={(r) => r.mint_address}
          searchable
          colFilters
          colToggle
          hoverable
          loading={isFetching}
          tableId="trader_analysis_tokens"
          onVisibleRowsChange={handleVisibleRows}
          emptyMessage="No tokens match the filters"
        />
      )}

      {/* Charts grid — mirrors the table's current sort/filter/page */}
      {visibleRows.length > 0 && query && (
        <div className="mt-4 flex flex-col gap-4">
          {visibleRows.map((row) => (
            <LazyMount key={row.mint_address}>
              <TraderTokenCard row={row} wallet={query.wallet} />
            </LazyMount>
          ))}
        </div>
      )}
    </div>
  );
}
