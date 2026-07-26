import { useCallback, useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useLaunchesQuery } from '@shared/store/endpoints';
import { apiErrorMessage } from '@shared/store/baseApi';
import {
  AddressDisplay,
  AgeCell,
  Banner,
  Card,
  Column,
  DataTable,
  IconButton,
  StatusPill,
  usePinnedRows,
} from '@shared/components/ui';
import { formatCount, formatUsd, gmgnMint, quoteToHuman } from '@shared/lib/format';
import type { LaunchListRow } from '@shared/types';

const PAGE = 100;

// Module-scope so the array identity is stable across renders — a fresh array
// each render would defeat DataTable's row memoization (audit H2). None of these
// cells close over component state.
const LAUNCH_COLUMNS: Column<LaunchListRow>[] = [
  {
    header: 'Token',
    render: (l) =>
      l.name ? (
        <span>
          <span className="font-medium">{l.name}</span>{' '}
          <span className="muted">{l.symbol}</span>
        </span>
      ) : (
        <span className="muted italic">pending ingest…</span>
      ),
    filterValue: (l) => `${l.name ?? ''} ${l.symbol ?? ''}`,
    filterPlaceholder: 'name / symbol',
    filterKey: 'token',
  },
  {
    header: 'Mint',
    render: (l) => <AddressDisplay value={l.mint_address} kind="token" />,
    filterValue: (l) => l.mint_address,
    filterKey: 'mint',
  },
  {
    header: 'Launch',
    render: (l) => <StatusPill status={l.status} />,
    filterValue: (l) => l.status,
    filterKey: 'status',
  },
  {
    header: 'Bundle',
    render: (l) => <StatusPill status={l.bundle_status ?? undefined} />,
    filterValue: (l) => l.bundle_status ?? '',
    filterKey: 'bundle_status',
  },
  {
    header: 'Flags',
    render: (l) => <Flags l={l} />,
    filterValue: (l) => `${l.is_migrated ? 'migrated' : ''} ${l.is_dead ? 'dead' : ''}`,
    filterPlaceholder: 'migrated / dead',
    filterKey: 'flags',
  },
  {
    header: 'Trades',
    align: 'right',
    render: (l) => <span className="mono">{formatCount(l.trade_count)}</span>,
    // Integer trade count.
    filterNumber: (l) => l.trade_count,
    filterKey: 'trade_count',
  },
  {
    header: 'Mkt cap',
    align: 'right',
    render: (l) => <span className="mono">{formatUsd(l.market_cap_usd)}</span>,
    // USD market cap.
    filterNumber: (l) => l.market_cap_usd,
    filterKey: 'market_cap_usd',
  },
  {
    header: 'Holding',
    align: 'right',
    render: (l) =>
      l.holding_base && l.holding_base > 0 ? (
        <span className="mono">
          {(l.holding_base / 10 ** (l.token_decimals ?? 6)).toLocaleString(undefined, {
            maximumFractionDigits: 2,
          })}
        </span>
      ) : (
        <span className="muted">—</span>
      ),
    // Human token amount (base units ÷ token decimals) — same value the cell shows.
    filterNumber: (l) =>
      l.holding_base && l.holding_base > 0
        ? l.holding_base / 10 ** (l.token_decimals ?? 6)
        : null,
    filterKey: 'holding',
  },
  {
    header: 'Cost (SOL)',
    align: 'right',
    render: (l) =>
      l.holding_base && l.holding_base > 0 ? (
        <span className="mono">{quoteToHuman(l.holding_cost_quote, l.quote_decimals ?? 9)}</span>
      ) : (
        <span className="muted">—</span>
      ),
    // Cost basis in SOL (quote base units ÷ quote decimals).
    filterNumber: (l) =>
      l.holding_base && l.holding_base > 0 && l.holding_cost_quote != null
        ? l.holding_cost_quote / 10 ** (l.quote_decimals ?? 9)
        : null,
    filterKey: 'cost_sol',
  },
  {
    header: 'Value (SOL)',
    align: 'right',
    render: (l) =>
      l.holding_base && l.holding_base > 0 ? (
        <span className="mono">{quoteToHuman(l.holding_value_quote, l.quote_decimals ?? 9)}</span>
      ) : (
        <span className="muted">—</span>
      ),
    // Current SOL value (quote base units ÷ quote decimals).
    filterNumber: (l) =>
      l.holding_base && l.holding_base > 0 && l.holding_value_quote != null
        ? l.holding_value_quote / 10 ** (l.quote_decimals ?? 9)
        : null,
    filterKey: 'value_sol',
  },
  {
    header: 'Variant',
    render: (l) => <span className="mono text-xs muted">{l.variant}</span>,
    filterValue: (l) => l.variant,
    filterKey: 'variant',
  },
  { header: 'Age', align: 'right', render: (l) => <AgeCell iso={l.created_at} /> },
  {
    header: '',
    align: 'right',
    render: (l) =>
      l.mint_address ? (
        <a
          href={gmgnMint(l.mint_address)}
          target="_blank"
          rel="noreferrer"
          className="text-xs"
          onClick={(e) => e.stopPropagation()}
        >
          GMGN ↗
        </a>
      ) : (
        <span className="muted">—</span>
      ),
  },
];

/** Shallow equality over a flat filterKey→text map (order-insensitive). */
function sameFilters(a: Record<string, string>, b: Record<string, string>): boolean {
  const ak = Object.keys(a);
  const bk = Object.keys(b);
  return ak.length === bk.length && ak.every((k) => a[k] === b[k]);
}

export function LaunchesPage() {
  const [offset, setOffset] = useState(0);
  // Active per-column filters (filterKey → raw grammar text), emitted by the
  // DataTable in server mode and forwarded as query params. The backend filters
  // BOTH the page and the count, so `total` reflects the filtered set.
  const [filters, setFilters] = useState<Record<string, string>>({});
  const { data, isFetching, error, refetch } = useLaunchesQuery(
    { limit: PAGE, offset, filters },
    { pollingInterval: 15_000, skipPollingIfUnfocused: true },
  );
  const navigate = useNavigate();

  // Adopt a new filter set only when it actually changed (the table re-emits on
  // every debounce tick, incl. the empty map on mount).
  const handleFilterChange = useCallback((next: Record<string, string>) => {
    setFilters((prev) => (sameFilters(prev, next) ? prev : next));
  }, []);
  // Any filter change snaps back to page 1 — else we could sit past the filtered
  // total and show an empty page.
  useEffect(() => {
    setOffset(0);
  }, [filters]);

  const rows = data?.launches ?? [];
  const total = data?.total ?? 0;
  const pinning = usePinnedRows('launches', (l: LaunchListRow) => l.id, rows);

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h1 className="text-lg font-semibold">Launched Tokens</h1>
        <IconButton icon="refresh" label="Refresh" onClick={() => refetch()} loading={isFetching} />
      </div>

      {error && <Banner tone="bad">{apiErrorMessage(error)}</Banner>}

      <Card
        title={`Launches (${total})`}
        actions={
          <div className="flex items-center gap-2 text-xs">
            <IconButton
              icon="chevron-left"
              label="Previous page"
              disabled={offset === 0}
              onClick={() => setOffset((o) => Math.max(0, o - PAGE))}
            />
            <span className="muted">
              {total === 0 ? 0 : offset + 1}–{Math.min(offset + PAGE, total)} of {total}
            </span>
            <IconButton
              icon="chevron-right"
              label="Next page"
              disabled={offset + PAGE >= total}
              onClick={() => setOffset((o) => o + PAGE)}
            />
          </div>
        }
      >
        <DataTable
          columns={LAUNCH_COLUMNS}
          rows={rows}
          rowKey={(l) => l.id}
          loading={isFetching}
          empty="No launches yet — run one from the Launch Console."
          onRowClick={(l) => navigate(`/tokens/${l.mint_address}`)}
          pinning={pinning}
          filterable
          serverFilter={{ onChange: handleFilterChange }}
        />
      </Card>
    </div>
  );
}

function Flags({ l }: { l: LaunchListRow }) {
  return (
    <span className="flex gap-1">
      {l.is_migrated && <span className="badge badge-info">migrated</span>}
      {l.is_dead && <span className="badge badge-bad">dead</span>}
      {!l.is_migrated && !l.is_dead && <span className="muted">—</span>}
    </span>
  );
}
