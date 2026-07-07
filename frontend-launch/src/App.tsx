import { useEffect, useRef, useState } from 'react';
import {
  api,
  formatSig,
  IngestStatus,
  LaunchResult,
  LaunchStatus,
  LaunchTemplate,
  ManagedWalletPool,
  MetadataTemplate,
  shouldKeepPolling,
  solscanMint,
  solscanTx,
} from './api';
import LaunchTemplates from './LaunchTemplates';
import MetadataTemplates from './MetadataTemplates';
import WalletPool from './WalletPool';

function StatusPill({ status }: { status: string }) {
  const cls = status.toLowerCase().replace(/[^a-z]/g, '');
  return <span className={`status-pill ${cls}`}>{status}</span>;
}

// Runtime pause/resume for the LIVE box's Helius ingest stream — see
// `GET/PUT /api/ingest` (`crates/live/src/http.rs`). Polls every 5s so the
// button reflects a toggle made from another tab or a watchdog auto-pause.
function IngestToggle() {
  const [status, setStatus] = useState<IngestStatus | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let cancelled = false;
    const poll = async () => {
      try {
        const s = await api.ingestStatus();
        if (!cancelled) setStatus(s);
      } catch {
        // Transient fetch error — keep the last known status, retry next tick.
      }
    };
    poll();
    const timer = setInterval(poll, 5000);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, []);

  if (!status || !status.configured) return null;

  const onToggle = async () => {
    setBusy(true);
    try {
      setStatus(await api.setIngest(!status.live));
    } catch {
      // Leave status as-is; next poll will resync.
    } finally {
      setBusy(false);
    }
  };

  return (
    <button
      type="button"
      className={`ingest-toggle ${status.live ? 'live' : 'paused'}`}
      disabled={busy}
      onClick={onToggle}
      title="Pause/resume the Helius ingest stream"
    >
      Ingest: {status.live ? 'LIVE' : 'PAUSED'}
    </button>
  );
}

type View = 'launch' | 'wallets' | 'metadata' | 'templates';

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

  // Token identity is a single choice: which metadata_templates row to launch
  // with. Defaults to the selected launch template's own `metadata_template_id`;
  // picking a different one here overrides it for this one launch (the template
  // itself is unchanged). No more free-text name/symbol/uri — those live in the
  // metadata template, the single source of truth.
  const [metaTemplateId, setMetaTemplateId] = useState('');
  const [bundlerCount, setBundlerCount] = useState('');

  const selectedTemplate = templates.find((t) => t.id === templateId);
  const effectiveMetadata = metadataTemplates.find((m) => m.id === metaTemplateId);

  // Only `funded` dev wallets are launch-ready — a `generated` (unfunded) or
  // `reserved`/`used` one would just fail the balance check downstream.
  const fundedDevWallets = wallets.filter((w) => w.role === 'dev' && w.status === 'funded');

  // Extracted so the Templates tab can refresh the Launch Console's picker
  // right after a create/edit, instead of requiring a manual page reload.
  const loadTemplates = async () => {
    try {
      const t = await api.templates();
      setTemplates(t);
      setTemplateId((prev) => prev || t[0]?.id || '');
    } catch (e) {
      setError(String(e));
    }
  };

  useEffect(() => {
    loadTemplates();
    (async () => {
      try {
        const [w, m] = await Promise.all([api.walletPool('dev'), api.metadataTemplates()]);
        setWallets(w);
        setMetadataTemplates(m);
        const firstFunded = w.find((wallet) => wallet.status === 'funded');
        if (firstFunded) setWalletId(firstFunded.id);
      } catch (e) {
        setError(String(e));
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Reseed the per-launch overrides ONLY when the selected template's identity or
  // content actually changes — keyed off `${id}@${updated_at}`. `templates` is in
  // the dep array so a late-arriving list still seeds the initial selection, but
  // an unrelated background refresh (e.g. the Templates tab saving, which swaps
  // the array reference) must NOT clobber an in-progress override choice.
  const appliedTemplateRef = useRef('');
  useEffect(() => {
    const t = templates.find((tpl) => tpl.id === templateId);
    if (!t) return;
    const sig = `${t.id}@${t.updated_at}`;
    if (appliedTemplateRef.current === sig) return;
    appliedTemplateRef.current = sig;
    setMetaTemplateId(t.metadata_template_id ?? '');
    setBundlerCount(t.params.bundle_leg_count != null ? String(t.params.bundle_leg_count) : '');
  }, [templateId, templates]);

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
        metadata_template_id: metaTemplateId || undefined,
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
        <button
          type="button"
          className={`tab${view === 'templates' ? ' active' : ''}`}
          onClick={() => setView('templates')}
        >
          Launch Templates
        </button>
        <div className="tabs-spacer" />
        <IngestToggle />
      </div>

      {view === 'wallets' && <WalletPool />}
      {view === 'metadata' && <MetadataTemplates />}
      {view === 'templates' && <LaunchTemplates onChange={loadTemplates} />}

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
            disabled={loading || !templateId || !walletId || !metaTemplateId}
            onClick={onLaunch}
          >
            {loading ? 'Launching…' : 'Launch'}
          </button>
        </div>

        <div className="row">
          <div className="field">
            <label htmlFor="meta-template-picker">Metadata</label>
            <select
              id="meta-template-picker"
              value={metaTemplateId}
              onChange={(e) => setMetaTemplateId(e.target.value)}
            >
              {metadataTemplates.length === 0 && (
                <option value="">No metadata templates — create one in the Metadata tab</option>
              )}
              {metadataTemplates.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.template_name} ({m.name} / {m.symbol})
                </option>
              ))}
            </select>
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
        {effectiveMetadata && (
          <p className="muted">
            Launching as <strong>{effectiveMetadata.name}</strong> ({effectiveMetadata.symbol}) —{' '}
            <code>{effectiveMetadata.uri}</code>
            {selectedTemplate && metaTemplateId !== (selectedTemplate.metadata_template_id ?? '')
              ? ' · overriding the template default for this launch'
              : ''}
            .
          </p>
        )}
        <p className="muted">
          Token identity (name/symbol/URI) comes from the selected metadata template — the single
          source of truth. Bundler legs are claimed server-side from the <code>funded</code>{' '}
          <code>bundler</code> wallet pool — never a manual wallet pick.
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
