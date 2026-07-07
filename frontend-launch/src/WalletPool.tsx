import { useEffect, useState } from 'react';
import { api, formatAge, formatSol, ManagedWalletPool, WalletStatus } from './api';

// Below this many `funded` wallets for a role, manual funding won't refill
// itself in time for a launch — surface it instead of finding out mid-launch.
const LOW_POOL_THRESHOLD = 3;

const ROLES = ['dev', 'bundler', 'treasury', 'trading'] as const;
const STATUSES: WalletStatus[] = ['generated', 'funded', 'reserved', 'used', 'retired'];

function StatusPill({ status }: { status: string }) {
  return <span className={`status-pill ${status}`}>{status}</span>;
}

export default function WalletPool() {
  const [wallets, setWallets] = useState<ManagedWalletPool[]>([]);
  const [roleFilter, setRoleFilter] = useState('');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const [genRole, setGenRole] = useState<(typeof ROLES)[number]>('bundler');
  const [genCount, setGenCount] = useState(5);
  const [genLabel, setGenLabel] = useState('');
  const [generating, setGenerating] = useState(false);

  const load = async () => {
    setLoading(true);
    setError(null);
    try {
      setWallets(await api.walletPool(roleFilter || undefined));
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [roleFilter]);

  const onGenerate = async () => {
    if (genCount < 1) return;
    setGenerating(true);
    setError(null);
    try {
      await api.generateWallets(genRole, genCount, genLabel);
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setGenerating(false);
    }
  };

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
          <strong>Low pool:</strong>{' '}
          {lowPoolRoles
            .map(([role, byStatus]) => `${role} (${byStatus.funded ?? 0} funded)`)
            .join(', ')}{' '}
          — below {LOW_POOL_THRESHOLD}. Manual funding won't refill itself; fund more
          `generated` wallets or generate + fund a new batch.
        </div>
      )}

      <div className="card">
        <h2>Generate wallets</h2>
        <div className="row">
          <div className="field">
            <label htmlFor="gen-role">Role</label>
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
          </div>
          <div className="field">
            <label htmlFor="gen-count">Count</label>
            <input
              id="gen-count"
              type="number"
              min={1}
              max={200}
              value={genCount}
              onChange={(e) => setGenCount(Number(e.target.value))}
            />
          </div>
          <div className="field">
            <label htmlFor="gen-label">Label prefix (optional)</label>
            <input
              id="gen-label"
              type="text"
              value={genLabel}
              onChange={(e) => setGenLabel(e.target.value)}
              placeholder="e.g. batch-07"
            />
          </div>
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
        <h2>Status counts</h2>
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
        {loading ? (
          <p className="muted">Loading…</p>
        ) : (
          <table>
            <thead>
              <tr>
                <th>Address</th>
                <th>Label</th>
                <th>Role</th>
                <th>Status</th>
                <th>Balance</th>
                <th>Age</th>
              </tr>
            </thead>
            <tbody>
              {wallets.map((w) => (
                <tr key={w.id}>
                  <td>{`${w.address.slice(0, 6)}…${w.address.slice(-6)}`}</td>
                  <td>{w.label ?? '—'}</td>
                  <td>{w.role}</td>
                  <td>
                    <StatusPill status={w.status} />
                  </td>
                  <td>{formatSol(w.balance_lamports)}</td>
                  <td>{formatAge(w.created_at)}</td>
                </tr>
              ))}
              {wallets.length === 0 && (
                <tr>
                  <td colSpan={6} className="muted">
                    No wallets yet — generate a batch above.
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
}
