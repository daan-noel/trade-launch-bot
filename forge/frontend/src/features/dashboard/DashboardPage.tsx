import { Link, useNavigate } from 'react-router-dom';
import {
  useIngestStatusQuery,
  useLaunchesQuery,
  useQuoteAssetsQuery,
  useWalletPoolQuery,
} from '@shared/store/endpoints';
import {
  AddressDisplay,
  Banner,
  Button,
  Card,
  Column,
  DataTable,
  StatCard,
  StatusPill,
} from '@shared/components/ui';
import { formatAge, formatCount, formatSol, gmgnMint } from '@shared/lib/format';
import type { LaunchListRow, ManagedWalletPool } from '@shared/types';

const ROLES = ['dev', 'bundler', 'treasury', 'trading'] as const;
const LOW_POOL_THRESHOLD = 3;

export function DashboardPage() {
  const navigate = useNavigate();
  const { data: wallets = [] } = useWalletPoolQuery(undefined, { pollingInterval: 15_000 });
  const { data: launchesPage } = useLaunchesQuery({ limit: 8 }, { pollingInterval: 15_000 });
  const { data: ingest } = useIngestStatusQuery(undefined, { pollingInterval: 5000 });
  const { data: quotes = [] } = useQuoteAssetsQuery();

  const sol = quotes.find((q) => q.is_native)?.usd_rate ?? null;
  const treasuryLamports = wallets
    .filter((w) => w.role === 'treasury')
    .reduce((sum, w) => sum + (w.balance_lamports ?? 0), 0);

  const fundedByRole = (role: string) => wallets.filter((w) => w.role === role && w.status === 'funded').length;
  const totalFunded = wallets.filter((w) => w.status === 'funded').length;
  const lowRoles = ROLES.filter((r) => (r === 'dev' || r === 'bundler') && fundedByRole(r) < LOW_POOL_THRESHOLD);

  const launches = launchesPage?.launches ?? [];

  const columns: Column<LaunchListRow>[] = [
    {
      header: 'Token',
      render: (l) => (l.name ? <span className="font-medium">{l.name} <span className="muted">{l.symbol}</span></span> : <span className="muted italic">pending…</span>),
    },
    { header: 'Launch', render: (l) => <StatusPill status={l.status} /> },
    { header: 'Bundle', render: (l) => <StatusPill status={l.bundle_status ?? undefined} /> },
    { header: 'Trades', align: 'right', render: (l) => <span className="mono">{formatCount(l.trade_count)}</span> },
    { header: 'Age', align: 'right', render: (l) => formatAge(l.created_at) },
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
        <h1 className="text-lg font-semibold">Dashboard</h1>
        <div className="flex gap-2">
          <Button variant="primary" onClick={() => navigate('/launch')}>New launch</Button>
          <Button onClick={() => navigate('/wallets')}>Manage wallets</Button>
        </div>
      </div>

      {lowRoles.length > 0 && (
        <Banner tone="warn" actions={<Button size="sm" onClick={() => navigate('/wallets')}>Fund</Button>}>
          <strong>Low pool:</strong> {lowRoles.map((r) => `${r} (${fundedByRole(r)} funded)`).join(', ')} — below{' '}
          {LOW_POOL_THRESHOLD}.
        </Banner>
      )}

      <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
        <StatCard
          label="Ingest"
          value={ingest?.configured ? (ingest.live ? 'Live' : 'Paused') : 'Off'}
          tone={ingest?.live ? 'good' : ingest?.configured ? 'bad' : undefined}
          sub={ingest?.configured ? 'Helius gRPC' : 'no Helius creds'}
        />
        <StatCard label="SOL / USD" value={sol != null ? `$${sol.toFixed(2)}` : '—'} />
        <StatCard label="Treasury" value={formatSol(treasuryLamports, 3)} sub="role=treasury balance" />
        <StatCard label="Funded wallets" value={totalFunded} sub={`${launchesPage?.total ?? 0} launches total`} />
      </div>

      <Card title="Pool health">
        <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
          {ROLES.map((role) => {
            const funded = fundedByRole(role);
            const total = wallets.filter((w) => w.role === role).length;
            const low = (role === 'dev' || role === 'bundler') && funded < LOW_POOL_THRESHOLD;
            return (
              <div key={role} className="panel px-4 py-3">
                <div className="field-label">{role}</div>
                <div className={`text-xl font-semibold ${low ? 'text-[var(--color-warn)]' : ''}`}>
                  {funded} <span className="text-sm muted font-normal">funded</span>
                </div>
                <div className="text-xs muted">{total} total</div>
              </div>
            );
          })}
        </div>
      </Card>

      <Card
        title="Recent launches"
        actions={<Link to="/launches" className="text-xs">View all →</Link>}
      >
        <DataTable
          columns={columns}
          rows={launches}
          rowKey={(l) => l.id}
          empty="No launches yet — run one from the Launch Console."
          onRowClick={(l) => navigate(`/tokens/${l.mint_address}`)}
        />
      </Card>

      {wallets.filter((w) => w.role === 'treasury').length > 0 && (
        <Card title="Treasury wallets">
          <TreasuryTable wallets={wallets.filter((w) => w.role === 'treasury')} />
        </Card>
      )}
    </div>
  );
}

function TreasuryTable({ wallets }: { wallets: ManagedWalletPool[] }) {
  const columns: Column<ManagedWalletPool>[] = [
    { header: 'Address', render: (w) => <AddressDisplay value={w.address} lead={6} tail={6} /> },
    { header: 'Label', render: (w) => w.label ?? <span className="muted">—</span> },
    { header: 'Balance', align: 'right', render: (w) => <span className="mono">{formatSol(w.balance_lamports)}</span> },
    { header: 'Checked', align: 'right', render: (w) => formatAge(w.balance_checked_at) },
  ];
  return <DataTable columns={columns} rows={wallets} rowKey={(w) => w.id} />;
}
