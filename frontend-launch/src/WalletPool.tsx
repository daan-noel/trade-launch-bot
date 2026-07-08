import { useState } from 'react';
import { api, formatAge, formatSol, ManagedWalletPool, WalletStatus } from './api';
import { StatusPill } from './components/StatusPill';
import { useResource } from './components/useResource';
import { Column, DataTable } from './components/DataTable';
import { Field } from './components/Field';

// Below this many `funded` wallets for a role, the pool can't refill itself in
// time for a launch — surface it instead of finding out mid-launch.
const LOW_POOL_THRESHOLD = 3;

const ROLES = ['dev', 'bundler', 'treasury', 'trading'] as const;
const STATUSES: WalletStatus[] = ['generated', 'funding', 'funded', 'reserved', 'used', 'retired'];

export default function WalletPool() {
  const [roleFilter, setRoleFilter] = useState('');
  const {
    data: wallets = [],
    loading,
    error,
    reload,
    setError,
  } = useResource(() => api.walletPool(roleFilter || undefined), [roleFilter]);

  const [genRole, setGenRole] = useState<(typeof ROLES)[number]>('bundler');
  const [genCount, setGenCount] = useState(5);
  const [genLabel, setGenLabel] = useState('');
  const [generating, setGenerating] = useState(false);

  const [funding, setFunding] = useState(false);
  const [fundMsg, setFundMsg] = useState<string | null>(null);

  const onGenerate = async () => {
    if (genCount < 1) return;
    setGenerating(true);
    setError(null);
    try {
      await api.generateWallets(genRole, genCount, genLabel);
      await reload();
    } catch (e) {
      setError(String(e));
    } finally {
      setGenerating(false);
    }
  };

  // Trigger a treasury -> pool funding pass. No role = top every fundable role
  // (dev + bundler) up to its warm target. Requires FUND_ENABLED on the box.
  const onFund = async (role?: string) => {
    setFunding(true);
    setError(null);
    setFundMsg(null);
    try {
      const report = await api.fundPool(role);
      if (report.outcomes.length === 0) {
        setFundMsg('Nothing to fund — pool already warm (or no treasury configured).');
      } else {
        const by = (r: string) => report.outcomes.filter((o) => o.result === r).length;
        const parts = [
          by('sent') && `${by('sent')} sent`,
          by('dry_run') && `${by('dry_run')} dry-run`,
          by('failed') && `${by('failed')} failed`,
          by('skipped_cap') && `${by('skipped_cap')} skipped (safety cap)`,
        ].filter(Boolean);
        setFundMsg(
          `Funding pass: ${parts.join(', ') || 'no transfers'} — ${formatSol(
            report.spent_lamports,
          )} spent.`,
        );
      }
      await reload();
    } catch (e) {
      setError(String(e));
    } finally {
      setFunding(false);
    }
  };

  const columns: Column<ManagedWalletPool>[] = [
    { header: 'Address', render: (w) => `${w.address.slice(0, 6)}…${w.address.slice(-6)}` },
    { header: 'Label', render: (w) => w.label ?? '—' },
    { header: 'Role', render: (w) => w.role },
    { header: 'Status', render: (w) => <StatusPill status={w.status} /> },
    { header: 'Balance', render: (w) => formatSol(w.balance_lamports) },
    { header: 'Age', render: (w) => formatAge(w.created_at) },
  ];

  // Status counts summary + low-pool banner, both derived client-side from the
  // one pool fetch — no separate aggregate endpoint to keep in sync.
  const counts: Record<string, Record<string, number>> = {};
  for (const role of ROLES) counts[role] = Object.fromEntries(STATUSES.map((s) => [s, 0]));
  for (const w of wallets) {
    if (!counts[w.role]) counts[w.role] = Object.fromEntries(STATUSES.map((s) => [s, 0]));
    counts[w.role][w.status] = (counts[w.role][w.status] ?? 0) + 1;
  }
  const lowPoolRoles = Object.entries(counts).filter(
    ([, byStatus]) => (byStatus.funded ?? 0) < LOW_POOL_THRESHOLD,
  );

  return (
    <div>
      {lowPoolRoles.length > 0 && (
        <div className="card banner-warning">
          <div className="row" style={{ justifyContent: 'space-between', alignItems: 'center' }}>
            <div>
              <strong>Low pool:</strong>{' '}
              {lowPoolRoles
                .map(([role, byStatus]) => `${role} (${byStatus.funded ?? 0} funded)`)
                .join(', ')}{' '}
              — below {LOW_POOL_THRESHOLD}. Fund from the treasury to top up.
            </div>
            <button type="button" className="primary" disabled={funding} onClick={() => onFund()}>
              {funding ? 'Funding…' : 'Fund pool'}
            </button>
          </div>
        </div>
      )}

      {fundMsg && <div className="card banner-info">{fundMsg}</div>}

      <div className="card">
        <h2>Generate wallets</h2>
        <div className="row">
          <Field label="Role" htmlFor="gen-role">
            <select
              id="gen-role"
              value={genRole}
              onChange={(e) => setGenRole(e.target.value as (typeof ROLES)[number])}
            >
              {ROLES.map((r) => (
                <option key={r} value={r}>
                  {r}
                </option>
              ))}
            </select>
          </Field>
          <Field label="Count" htmlFor="gen-count">
            <input
              id="gen-count"
              type="number"
              min={1}
              max={200}
              value={genCount}
              onChange={(e) => setGenCount(Number(e.target.value))}
            />
          </Field>
          <Field label="Label prefix (optional)" htmlFor="gen-label">
            <input
              id="gen-label"
              type="text"
              value={genLabel}
              onChange={(e) => setGenLabel(e.target.value)}
              placeholder="e.g. batch-07"
            />
          </Field>
          <button type="button" className="primary" disabled={generating} onClick={onGenerate}>
            {generating ? 'Generating…' : 'Generate'}
          </button>
        </div>
        <p className="muted">
          Creates fresh ed25519 keypairs, envelope-encrypts each into the keystore, and
          inserts them as <code>generated</code>. Fund them manually — the balance poller
          promotes to <code>funded</code> automatically once SOL lands.
        </p>
      </div>

      <div className="card">
        <div className="row" style={{ justifyContent: 'space-between', alignItems: 'center' }}>
          <h2 style={{ margin: 0 }}>Status counts</h2>
          <button type="button" disabled={funding} onClick={() => onFund()}>
            {funding ? 'Funding…' : 'Fund pool'}
          </button>
        </div>
        <p className="muted">
          Sends SOL from the <code>treasury</code> wallet to top <code>dev</code> +{' '}
          <code>bundler</code> up to their warm targets. Wallets show <code>funding</code> while a
          transfer is in flight, then flip to <code>funded</code> when it lands. Requires{' '}
          <code>FUND_ENABLED</code> on the box.
        </p>
        <table>
          <thead>
            <tr>
              <th>Role</th>
              {STATUSES.map((s) => (
                <th key={s}>{s}</th>
              ))}
            </tr>
          </thead>
          <tbody>
            {Object.entries(counts).map(([role, byStatus]) => (
              <tr key={role}>
                <td>{role}</td>
                {STATUSES.map((s) => (
                  <td key={s}>{byStatus[s] ?? 0}</td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <div className="card">
        <div className="row" style={{ justifyContent: 'space-between' }}>
          <h2 style={{ margin: 0 }}>Pool ({wallets.length})</h2>
          <div className="field" style={{ minWidth: 160 }}>
            <select value={roleFilter} onChange={(e) => setRoleFilter(e.target.value)}>
              <option value="">All roles</option>
              {ROLES.map((r) => (
                <option key={r} value={r}>
                  {r}
                </option>
              ))}
            </select>
          </div>
        </div>
        {error && <p className="error">{error}</p>}
        <DataTable
          columns={columns}
          rows={wallets}
          rowKey={(w) => w.id}
          loading={loading}
          empty="No wallets yet — generate a batch above."
        />
      </div>
    </div>
  );
}
