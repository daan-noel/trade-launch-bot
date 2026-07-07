import { useEffect, useState } from 'react';
import {
  api,
  formatSig,
  LaunchResult,
  LaunchStatus,
  LaunchTemplate,
  ManagedWallet,
  shouldKeepPolling,
  solscanMint,
  solscanTx,
} from './api';

function StatusPill({ status }: { status: string }) {
  const cls = status.toLowerCase().replace(/[^a-z]/g, '');
  return <span className={`status-pill ${cls}`}>{status}</span>;
}

export default function App() {
  const [templates, setTemplates] = useState<LaunchTemplate[]>([]);
  const [wallets, setWallets] = useState<ManagedWallet[]>([]);
  const [templateId, setTemplateId] = useState('');
  const [walletId, setWalletId] = useState('');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<LaunchResult | null>(null);
  const [status, setStatus] = useState<LaunchStatus | null>(null);

  useEffect(() => {
    (async () => {
      try {
        const [t, w] = await Promise.all([api.templates(), api.wallets('dev')]);
        setTemplates(t);
        setWallets(w);
        if (t[0]) setTemplateId(t[0].id);
        if (w[0]) setWalletId(w[0].id);
      } catch (e) {
        setError(String(e));
      }
    })();
  }, []);

  const launchId = result?.launch_id;

  useEffect(() => {
    if (!launchId) return;

    let cancelled = false;
    let timer: ReturnType<typeof setInterval> | undefined;

    const poll = async () => {
      try {
        const s = await api.launchStatus(launchId);
        if (cancelled) return;
        setStatus(s);
        if (!shouldKeepPolling(s) && timer) {
          clearInterval(timer);
          timer = undefined;
        }
      } catch (e) {
        if (!cancelled) setError(String(e));
      }
    };

    poll();
    timer = setInterval(poll, 3000);
    return () => {
      cancelled = true;
      if (timer) clearInterval(timer);
    };
  }, [launchId]);

  const onLaunch = async () => {
    if (!templateId || !walletId) return;
    setLoading(true);
    setError(null);
    setResult(null);
    setStatus(null);
    try {
      const r = await api.executeLaunch(templateId, walletId);
      setResult(r);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  const selectedTemplate = templates.find((t) => t.id === templateId);

  return (
    <div className="app">
      <h1>Launch Console</h1>
      <p className="muted">
        Create on pump.fun + auto-submit Jito sniper bundle. Requires <code>cargo run -p live</code>{' '}
        with launcher + ingest configured.
      </p>

      <div className="card">
        <h2>Launch</h2>
        <div className="row">
          <div className="field">
            <label htmlFor="template">Template</label>
            <select
              id="template"
              value={templateId}
              onChange={(e) => setTemplateId(e.target.value)}
            >
              {templates.length === 0 && <option value="">No templates — run seed script</option>}
              {templates.map((t) => (
                <option key={t.id} value={t.id}>
                  {t.template_name} ({t.variant})
                </option>
              ))}
            </select>
          </div>
          <div className="field">
            <label htmlFor="wallet">Dev wallet</label>
            <select id="wallet" value={walletId} onChange={(e) => setWalletId(e.target.value)}>
              {wallets.length === 0 && <option value="">No dev wallets — run seed script</option>}
              {wallets.map((w) => (
                <option key={w.id} value={w.id}>
                  {w.label ?? w.address.slice(0, 8)} ({w.role})
                </option>
              ))}
            </select>
          </div>
          <button
            type="button"
            className="primary"
            disabled={loading || !templateId || !walletId}
            onClick={onLaunch}
          >
            {loading ? 'Launching…' : 'Launch'}
          </button>
        </div>
        {selectedTemplate && (
          <p className="muted">
            Bundle legs:{' '}
            {String((selectedTemplate.params as { bundle_leg_count?: number }).bundle_leg_count ?? 0)}
          </p>
        )}
        {error && <p className="error">{error}</p>}
      </div>

      {result && (
        <div className="card">
          <h2>Launch result</h2>
          <dl className="kv">
            <dt>Launch ID</dt>
            <dd>{result.launch_id}</dd>
            <dt>Mint</dt>
            <dd>
              <a href={solscanMint(result.mint_address)} target="_blank" rel="noreferrer">
                {result.mint_address}
              </a>
            </dd>
            <dt>Create tx</dt>
            <dd>
              <a href={solscanTx(result.create_signature)} target="_blank" rel="noreferrer">
                {result.create_signature}
              </a>
            </dd>
            {result.bundle && (
              <>
                <dt>Bundle ID</dt>
                <dd>{result.bundle.bundle_id}</dd>
                <dt>Jito bundle</dt>
                <dd>{result.bundle.jito_bundle_id}</dd>
              </>
            )}
          </dl>
        </div>
      )}

      {status && (
        <div className="card">
          <h2>Status (polls every 3s until bundle terminal)</h2>
          <dl className="kv">
            <dt>Launch</dt>
            <dd>
              <StatusPill status={status.launch.status} />
            </dd>
            <dt>Bundle</dt>
            <dd>
              {status.bundle ? (
                <StatusPill status={status.bundle.status} />
              ) : (
                <span className="muted">none</span>
              )}
            </dd>
            <dt>Trades ingested</dt>
            <dd>{status.trade_count}</dd>
          </dl>
          {status.bundle?.leg_signatures?.length ? (
            <div style={{ marginTop: '0.75rem' }}>
              <strong>Leg signatures</strong>
              <ul>
                {status.bundle.leg_signatures.map((sig) => (
                  <li key={sig}>
                    <a href={solscanTx(sig)} target="_blank" rel="noreferrer">
                      {sig}
                    </a>
                  </li>
                ))}
              </ul>
            </div>
          ) : null}
        </div>
      )}

      {status && status.trades.length > 0 && (
        <div className="card">
          <h2>Recent trades on mint</h2>
          <table>
            <thead>
              <tr>
                <th>Slot</th>
                <th>Type</th>
                <th>Quote</th>
                <th>Tx</th>
              </tr>
            </thead>
            <tbody>
              {status.trades.map((t, i) => {
                const sig = formatSig(t.tx_signature);
                return (
                  <tr key={`${t.slot}-${i}`}>
                    <td>{t.slot}</td>
                    <td>{t.trade_type}</td>
                    <td>{t.amount_quote_display ?? t.amount_quote}</td>
                    <td>
                      {sig !== '—' ? (
                        <a href={solscanTx(sig)} target="_blank" rel="noreferrer">
                          {sig.slice(0, 12)}…
                        </a>
                      ) : (
                        '—'
                      )}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
