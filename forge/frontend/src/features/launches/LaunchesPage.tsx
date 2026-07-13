import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useLaunchesQuery } from '@shared/store/endpoints';
import { apiErrorMessage } from '@shared/store/baseApi';
import {
  AddressDisplay,
  AgeCell,
  Banner,
  Button,
  Card,
  Column,
  DataTable,
  StatusPill,
} from '@shared/components/ui';
import { formatCount, formatUsd, gmgnMint } from '@shared/lib/format';
import type { LaunchListRow } from '@shared/types';

const PAGE = 100;

export function LaunchesPage() {
  const [offset, setOffset] = useState(0);
  const { data, isFetching, error, refetch } = useLaunchesQuery(
    { limit: PAGE, offset },
    { pollingInterval: 15_000, skipPollingIfUnfocused: true },
  );
  const navigate = useNavigate();

  const rows = data?.launches ?? [];
  const total = data?.total ?? 0;

  const columns: Column<LaunchListRow>[] = [
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
    },
    { header: 'Mint', render: (l) => <AddressDisplay value={l.mint_address} kind="token" /> },
    { header: 'Launch', render: (l) => <StatusPill status={l.status} /> },
    { header: 'Bundle', render: (l) => <StatusPill status={l.bundle_status ?? undefined} /> },
    { header: 'Flags', render: (l) => <Flags l={l} /> },
    { header: 'Trades', align: 'right', render: (l) => <span className="mono">{formatCount(l.trade_count)}</span> },
    { header: 'Mkt cap', align: 'right', render: (l) => <span className="mono">{formatUsd(l.market_cap_usd)}</span> },
    { header: 'Variant', render: (l) => <span className="mono text-xs muted">{l.variant}</span> },
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

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h1 className="text-lg font-semibold">Launched Tokens</h1>
        <Button size="sm" onClick={() => refetch()} loading={isFetching}>
          Refresh
        </Button>
      </div>

      {error && <Banner tone="bad">{apiErrorMessage(error)}</Banner>}

      <Card
        title={`Launches (${total})`}
        actions={
          <div className="flex items-center gap-2 text-xs">
            <Button
              size="sm"
              disabled={offset === 0}
              onClick={() => setOffset((o) => Math.max(0, o - PAGE))}
            >
              ← Prev
            </Button>
            <span className="muted">
              {total === 0 ? 0 : offset + 1}–{Math.min(offset + PAGE, total)} of {total}
            </span>
            <Button
              size="sm"
              disabled={offset + PAGE >= total}
              onClick={() => setOffset((o) => o + PAGE)}
            >
              Next →
            </Button>
          </div>
        }
      >
        <DataTable
          columns={columns}
          rows={rows}
          rowKey={(l) => l.id}
          loading={isFetching}
          empty="No launches yet — run one from the Launch Console."
          onRowClick={(l) => navigate(`/tokens/${l.mint_address}`)}
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
