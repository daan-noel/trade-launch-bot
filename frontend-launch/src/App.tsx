import { useEffect, useState } from 'react';
import {
  api,
  formatSig,
  LaunchResult,
  LaunchStatus,
  LaunchTemplate,
  ManagedWalletPool,
  MetadataTemplate,
  shouldKeepPolling,
  solscanMint,
  solscanTx,
} from './api';
import MetadataTemplates from './MetadataTemplates';
import WalletPool from './WalletPool';

function StatusPill({ status }: { status: string }) {
  const cls = status.toLowerCase().replace(/[^a-z]/g, '');
  return <span className={`status-pill ${cls}`}>{status}</span>;
}

type View = 'launch' | 'wallets' | 'metadata';

export default function App() {
  const [view, setView] = useState<View>('launch');
  const [templates, setTemplates] = useState<LaunchTemplate[]>([]);
  const [wallets, setWallets] = useState<ManagedWalletPool[]>([]);
  const [metadataTemplates, setMetadataTemplates] = useState<MetadataTemplate[]>([]);
  const [templateId, setTemplateId] = useState('');
  const [walletId, setWalletId] = useState('');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<LaunchResult | null>(null);
  const [status, setStatus] = useState<LaunchStatus | null>(null);

  // Metadata editing panel — pre-filled from the selected launch template,
  // editable per launch (overrides sent to the backend; the template itself is
  // unchanged). `metaTemplateId` is a one-shot "load from" picker over the
  // Metadata Templates tab's saved content, not a stored selection — picking a
  // different launch template still resets these fields from its own params.
  const [metaName, setMetaName] = useState('');
  const [metaSymbol, setMetaSymbol] = useState('');
  const [metaUri, setMetaUri] = useState('');
  const [metaTemplateId, setMetaTemplateId] = useState('');
  const [bundlerCount, setBundlerCount] = useState('');

  // Only `funded` dev wallets are launch-ready — a `generated` (unfunded) or
  // `reserved`/`used` one would just fail the balance check downstream.
  const fundedDevWallets = wallets.filter((w) => w.role === 'dev' && w.status === 'funded');

  useEffect(() => {
    (async () => {
      try {
        const [t, w, m] = await Promise.all([
          api.templates(),
          api.walletPool('dev'),
          api.metadataTemplates(),
        ]);
        setTemplates(t);
        setWallets(w);
        setMetadataTemplates(m);
        if (t[0]) setTemplateId(t[0].id);
        const firstFunded = w.find((wallet) => wallet.status === 'funded');
        if (firstFunded) setWalletId(firstFunded.id);
      } catch (e) {
        setError(String(e));
      }
    })();
  }, []);

  useEffect(() => {
    const t = templates.find((tpl) => tpl.id === templateId);
    if (!t) return;
    const p = t.params as { name?: string; symbol?: string; uri?: string; bundle_leg_count?: number };
    setMetaName(p.name ?? '');
    setMetaSymbol(p.symbol ?? '');
    setMetaUri(p.uri ?? '');
    setMetaTemplateId('');
    setBundlerCount(p.bundle_leg_count != null ? String(p.bundle_leg_count) : '');
  }, [templateId, templates]);

  const onLoadMetadataTemplate = (id: string) => {
    setMetaTemplateId(id);
    const mt = metadataTemplates.find((m) => m.id === id);
    if (mt) {
      setMetaName(mt.name);
      setMetaSymbol(mt.symbol);
      setMetaUri(mt.uri);
    }
  };

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
      const r = await api.executeLaunch(templateId, walletId, {
        name: metaName || undefined,
        symbol: metaSymbol || undefined,
        uri: metaUri || undefined,
        bundler_count: bundlerCount ? Number(bundlerCount) : undefined,
      });
      setResult(r);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="app">
      <div className="tabs">
        <button
          type="button"
          className={`tab${view === 'launch' ? ' active' : ''}`}
          onClick={() => setView('launch')}
        >
          Launch Console
        </button>
        <button
          type="button"
          className={`tab${view === 'wallets' ? ' active' : ''}`}
          onClick={() => setView('wallets')}
        >
          Wallet Pool
        </button>
        <button
          type="button"
          className={`tab${view === 'metadata' ? ' active' : ''}`}
          onClick={() => setView('metadata')}
        >
          Metadata Templates
        </button>
      </div>

      {view === 'wallets' && <WalletPool />}
      {view === 'metadata' && <MetadataTemplates />}

      {view === 'launch' && (
    <>
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
            <label htmlFor="wallet">Dev wallet (funded only)</label>
            <select id="wallet" value={walletId} onChange={(e) => setWalletId(e.target.value)}>
              {fundedDevWallets.length === 0 && (
                <option value="">No funded dev wallets — fund one in the Wallet Pool tab</option>
              )}
              {fundedDevWallets.map((w) => (
                <option key={w.id} value={w.id}>
                  {w.label ?? w.address.slice(0, 8)}
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

        <div className="row">
          <div className="field">
            <label htmlFor="meta-template-picker">Load from metadata template</label>
            <select
              id="meta-template-picker"
              value={metaTemplateId}
              onChange={(e) => onLoadMetadataTemplate(e.target.value)}
            >
              <option value="">— none (edit fields directly) —</option>
              {metadataTemplates.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.template_name} ({m.symbol})
                </option>
              ))}
            </select>
          </div>
        </div>

        <div className="row">
          <div className="field">
            <label htmlFor="meta-name">Name</label>
            <input id="meta-name" type="text" value={metaName} onChange={(e) => setMetaName(e.target.value)} />
          </div>
          <div className="field">
            <label htmlFor="meta-symbol">Symbol</label>
            <input
              id="meta-symbol"
              type="text"
              value={metaSymbol}
              onChange={(e) => setMetaSymbol(e.target.value)}
            />
          </div>
          <div className="field">
            <label htmlFor="meta-uri">Metadata URI</label>
            <input id="meta-uri" type="text" value={metaUri} onChange={(e) => setMetaUri(e.target.value)} />
          </div>
          <div className="field">
            <label htmlFor="bundler-count">Use N bundlers</label>
            <input
              id="bundler-count"
              type="number"
              min={0}
              value={bundlerCount}
              onChange={(e) => setBundlerCount(e.target.value)}
            />
          </div>
        </div>
        <p className="muted">
          Name/symbol/metadata URI default from the template but are editable per launch. Bundler
          legs are claimed server-side from the <code>funded</code> <code>bundler</code> wallet pool
          — never a manual wallet pick.
        </p>
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
    </>
      )}
    </div>
  );
}
