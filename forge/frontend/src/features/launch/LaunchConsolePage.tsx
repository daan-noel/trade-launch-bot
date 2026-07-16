import { useEffect, useRef, useState } from 'react';
import { Link, useSearchParams } from 'react-router-dom';
import {
  useBootstrapQuery,
  useQuoteAssetsQuery,
  useExecuteLaunchMutation,
  useLaunchStatusQuery,
  useLaunchRequirementQuery,
} from '@shared/store/endpoints';
import { apiErrorMessage } from '@shared/store/baseApi';
import {
  AddressDisplay,
  Banner,
  Button,
  Card,
  Column,
  DataTable,
  Field,
  Input,
  KV,
  KVRow,
  Select,
  StatusPill,
  TradeTypePill,
} from '@shared/components/ui';
import { canonicalSteps } from '@shared/components/IxLayoutEditor';
import { connectLaunchStatusStream } from '@shared/services/sse';
import { formatSig, formatSol, ipfsToHttp, solscanTx } from '@shared/lib/format';
import { toHumanUnits } from '@features/templates/legForm';
import type {
  LaunchResult,
  LaunchStatus,
  LaunchTemplate,
  ManagedWalletPool,
  MetadataTemplate,
  QuoteAsset,
  TradePriced,
} from '@shared/types';

const TERMINAL_BUNDLE = new Set(['landed', 'dropped', 'partial', 'failed']);
function shouldKeepPolling(status: LaunchStatus | undefined): boolean {
  if (!status) return false;
  if (status.launch.status === 'pending') return true;
  const b = status.bundle?.status;
  if (!b) return false;
  return !TERMINAL_BUNDLE.has(b);
}

export function LaunchConsolePage() {
  const { data: boot } = useBootstrapQuery();
  const { data: quoteAssets = [] } = useQuoteAssetsQuery();
  const templates = boot?.templates ?? [];
  const metadataTemplates = boot?.metadata_templates ?? [];
  const devWallets = boot?.dev_wallets ?? [];

  const [templateId, setTemplateId] = useState('');
  const [walletId, setWalletId] = useState('');
  const [metaTemplateId, setMetaTemplateId] = useState('');
  const [bundlerCount, setBundlerCount] = useState('');
  const [result, setResult] = useState<LaunchResult | null>(null);

  const [executeLaunch, launchState] = useExecuteLaunchMutation();

  // Seed selections once bootstrap arrives.
  useEffect(() => {
    if (!templateId && templates[0]) setTemplateId(templates[0].id);
  }, [templates, templateId]);
  useEffect(() => {
    if (walletId) return;
    const seed =
      devWallets.find((w) => w.status === 'funded') ?? devWallets.find((w) => w.status === 'generated');
    if (seed) setWalletId(seed.id);
  }, [devWallets, walletId]);

  const selectedTemplate = templates.find((t) => t.id === templateId);

  // Reseed the per-launch overrides only when the selected template's identity or
  // content actually changes (keyed off id@updated_at) — a background bootstrap
  // refresh must not clobber an in-progress override choice.
  const appliedRef = useRef('');
  useEffect(() => {
    if (!selectedTemplate) return;
    const sig = `${selectedTemplate.id}@${selectedTemplate.updated_at}`;
    if (appliedRef.current === sig) return;
    appliedRef.current = sig;
    setMetaTemplateId(selectedTemplate.metadata_template_id ?? '');
    setBundlerCount(
      selectedTemplate.params.bundle_leg_count != null
        ? String(selectedTemplate.params.bundle_leg_count)
        : '',
    );
  }, [selectedTemplate]);

  const selectableDevWallets = devWallets.filter(
    (w) => w.role === 'dev' && (w.status === 'funded' || w.status === 'generated'),
  );
  const selectedDevWallet = devWallets.find((w) => w.id === walletId);
  const devWalletReady = selectedDevWallet?.status === 'funded';
  const effectiveMetadata = metadataTemplates.find((m) => m.id === metaTemplateId);
  const quoteAsset = quoteAssets.find((q) => q.id === selectedTemplate?.quote_asset_id);

  // Pre-launch cost preview — the SAME figure the launch gate enforces, so the
  // required amount + any shortfall is visible BEFORE clicking Launch instead of
  // surfacing as a failed request.
  const { data: requirement } = useLaunchRequirementQuery(
    {
      template_id: templateId,
      dev_wallet_id: walletId || undefined,
      bundler_count: bundlerCount ? Number(bundlerCount) : undefined,
    },
    { skip: !templateId },
  );
  const devShortfall =
    requirement && requirement.dev_balance_lamports != null
      ? Math.max(0, requirement.dev_required_lamports - requirement.dev_balance_lamports)
      : 0;
  const devShort = devShortfall > 0;

  // Status: the confirm watcher PUSHES each terminal launch/bundle transition over
  // SSE (audit Phase A), so the primary trigger is the `launch_status` stream below
  // — it refetches the instant the backend settles. The poll is demoted to a slow
  // 30s gap-heal that only covers a missed/disconnected push, then stops once the
  // bundle is terminal. The launch id is mirrored into the URL (?launch=<id>) on
  // execute so an accidental reload rehydrates it here and the Status card
  // reattaches to the in-flight launch (the LaunchResult card can't be rebuilt —
  // that's fine, the live status/bundle/trades all come from the query below).
  const [searchParams, setSearchParams] = useSearchParams();
  const launchId = result?.launch_id ?? searchParams.get('launch') ?? undefined;
  const [pollInterval, setPollInterval] = useState(0);
  useEffect(() => {
    setPollInterval(launchId ? 30000 : 0);
  }, [launchId]);
  const { data: status, refetch: refetchStatus } = useLaunchStatusQuery(launchId ?? '', {
    skip: !launchId,
    pollingInterval: pollInterval,
    skipPollingIfUnfocused: true,
  });
  const polling = pollInterval > 0;
  useEffect(() => {
    if (status && !shouldKeepPolling(status)) setPollInterval(0);
  }, [status]);

  // SSE push: refetch the status the instant the backend pushes a transition for
  // THIS launch (created/failed/partial) — the poll above is just the fallback.
  useEffect(() => {
    if (!launchId) return;
    const h = connectLaunchStatusStream((e) => {
      if (e.launch_id === launchId) refetchStatus();
    });
    return () => h.close();
  }, [launchId, refetchStatus]);

  const onLaunch = async () => {
    if (!templateId || !walletId) return;
    setResult(null);
    try {
      const r = await executeLaunch({
        template_id: templateId,
        dev_wallet_id: walletId,
        overrides: {
          metadata_template_id: metaTemplateId || undefined,
          bundler_count: bundlerCount ? Number(bundlerCount) : undefined,
        },
      }).unwrap();
      setResult(r);
      // Persist the launch id in the URL so a page reload can reattach to it.
      setSearchParams(
        (prev) => {
          prev.set('launch', r.launch_id);
          return prev;
        },
        { replace: true },
      );
    } catch {
      /* surfaced below */
    }
  };

  const error = apiErrorMessage(launchState.error);

  return (
    <div className="space-y-4">
      <div>
        <h1 className="text-lg font-semibold">Launch Console</h1>
        <p className="text-xs muted">
          Create on pump.fun + auto-submit a Jito sniper bundle. Requires <code>cargo run -p live</code>{' '}
          with launcher + ingest configured.
        </p>
      </div>

      <Card title="Launch">
        <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
          <Field label="Template" htmlFor="lc-t">
            <Select id="lc-t" value={templateId} onChange={(e) => setTemplateId(e.target.value)}>
              {templates.length === 0 && <option value="">No templates — author one</option>}
              {templates.map((t) => (
                <option key={t.id} value={t.id}>{t.template_name} ({t.variant})</option>
              ))}
            </Select>
          </Field>
          <Field label="Dev wallet" htmlFor="lc-w">
            <Select id="lc-w" value={walletId} onChange={(e) => setWalletId(e.target.value)}>
              {selectableDevWallets.length === 0 && <option value="">No dev wallets — generate one</option>}
              {selectableDevWallets.map((w) => (
                <option key={w.id} value={w.id}>
                  {devWalletOptionLabel(w)}
                </option>
              ))}
            </Select>
            {selectedDevWallet && (
              <p className="mt-1 text-xs">
                {devWalletReady ? (
                  <span style={{ color: 'var(--color-good)' }}>
                    Funded · <span className="mono">{formatSol(selectedDevWallet.balance_lamports)}</span>
                  </span>
                ) : (
                  <span className="muted">
                    <StatusPill status={selectedDevWallet.status} /> · balance{' '}
                    <span className="mono">{formatSol(selectedDevWallet.balance_lamports)}</span>
                  </span>
                )}
              </p>
            )}
          </Field>
          <Field label="Metadata (token identity)" htmlFor="lc-m">
            <Select id="lc-m" value={metaTemplateId} onChange={(e) => setMetaTemplateId(e.target.value)}>
              {metadataTemplates.length === 0 && <option value="">No metadata templates</option>}
              {metadataTemplates.map((m) => (
                <option key={m.id} value={m.id}>{m.template_name} ({m.name} / {m.symbol})</option>
              ))}
            </Select>
          </Field>
          <Field label="Use N bundlers" htmlFor="lc-b">
            <Input id="lc-b" type="number" min={0} value={bundlerCount} onChange={(e) => setBundlerCount(e.target.value)} />
          </Field>
        </div>

        {requirement && (
          <div className="mt-3 rounded-md border border-[var(--color-line)] p-3 text-xs">
            <div className="flex flex-wrap items-center gap-x-4 gap-y-1">
              <span>
                Dev wallet needs{' '}
                <span className="mono font-semibold">{formatSol(requirement.dev_required_lamports)}</span>
                {requirement.dev_balance_lamports != null && (
                  <span className="muted">
                    {' '}· has <span className="mono">{formatSol(requirement.dev_balance_lamports)}</span>
                  </span>
                )}
              </span>
              {requirement.leg_count > 0 && (
                <span className="muted">
                  + {requirement.leg_count} bundler leg(s) ×{' '}
                  <span className="mono">{formatSol(requirement.per_leg_lamports)}</span>
                </span>
              )}
            </div>
            {devShort && (
              <div className="mt-1" style={{ color: 'var(--color-bad)' }}>
                Dev wallet short <span className="mono font-semibold">{formatSol(devShortfall)}</span> — fund it
                before launching (rent + fees + Jito tip + dev-buy).
              </div>
            )}
          </div>
        )}

        {(selectedTemplate || effectiveMetadata) && (
          <div className="mt-4 grid grid-cols-1 gap-3 md:grid-cols-2">
            {selectedTemplate && (
              <TemplateSummary
                template={selectedTemplate}
                quoteAsset={quoteAsset}
                bundlerCountOverride={bundlerCount}
              />
            )}
            {effectiveMetadata && (
              <MetadataSummary
                metadata={effectiveMetadata}
                overriding={
                  !!selectedTemplate && metaTemplateId !== (selectedTemplate.metadata_template_id ?? '')
                }
              />
            )}
          </div>
        )}

        <div className="mt-4 flex flex-wrap items-center gap-3">
          <Button variant="primary" icon="rocket" loading={launchState.isLoading}
            disabled={!templateId || !walletId || !metaTemplateId || !devWalletReady || devShort} onClick={onLaunch}
            title={
              devShort
                ? `Dev wallet short ${formatSol(devShortfall)} — fund it on the Wallet Pool page first`
                : devWalletReady
                  ? ''
                  : 'Dev wallet not funded yet — fund it on the Wallet Pool page first'
            }>
            Launch
          </Button>
          {!devWalletReady && selectedDevWallet && (
            <span className="text-xs muted">
              Dev wallet is <StatusPill status={selectedDevWallet.status} /> — fund it on the{' '}
              <Link to="/wallets">Wallet Pool</Link> page first.
            </span>
          )}
          {devWalletReady && devShort && (
            <span className="text-xs" style={{ color: 'var(--color-bad)' }}>
              Balance below the launch requirement — short {formatSol(devShortfall)}.
            </span>
          )}
        </div>
        {error && <Banner tone="bad" className="mt-3">{error}</Banner>}
      </Card>

      {result && (
        <Card title="Launch result">
          <KV>
            <KVRow label="Launch ID">
              <Link to={`/tokens/${result.mint_address}`} className="mono">{result.launch_id}</Link>
            </KVRow>
            <KVRow label="Mint"><AddressDisplay value={result.mint_address} kind="token" lead={8} tail={8} /></KVRow>
            <KVRow label="Create tx"><AddressDisplay value={result.create_signature} kind="tx" lead={8} tail={8} /></KVRow>
            {result.bundle && (
              <>
                <KVRow label="Bundle ID"><span className="mono">{result.bundle.bundle_id}</span></KVRow>
                <KVRow label="Jito bundle"><span className="mono">{result.bundle.jito_bundle_id}</span></KVRow>
              </>
            )}
          </KV>
        </Card>
      )}

      {status && (
        <Card
          title="Status"
          actions={polling ? <span className="badge badge-warn">polling…</span> : <span className="badge badge-neutral">settled</span>}
        >
          <KV cols={2}>
            <KVRow label="Launch"><StatusPill status={status.launch.status} /></KVRow>
            <KVRow label="Bundle">{status.bundle ? <StatusPill status={status.bundle.status} /> : <span className="muted">none</span>}</KVRow>
            <KVRow label="Trades ingested"><span className="mono">{status.trade_count}</span></KVRow>
            <KVRow label="Mint"><AddressDisplay value={status.launch.mint_address} kind="token" /></KVRow>
          </KV>
          {status.bundle?.leg_signatures?.length ? (
            <div className="mt-3">
              <div className="field-label">Leg signatures</div>
              <ul className="space-y-0.5">
                {status.bundle.leg_signatures.map((sig) => (
                  <li key={sig}>
                    <a href={solscanTx(sig)} target="_blank" rel="noreferrer" className="mono text-xs">{sig}</a>
                  </li>
                ))}
              </ul>
            </div>
          ) : null}
          {status.trades.length > 0 && <TradesTable trades={status.trades} />}
        </Card>
      )}
    </div>
  );
}

/** Dropdown label: funded wallets show their SOL balance, others show they need funding. */
function devWalletOptionLabel(w: ManagedWalletPool): string {
  const name = w.label ?? w.address.slice(0, 8);
  return w.status === 'funded'
    ? `${name} · ${formatSol(w.balance_lamports)}`
    : `${name} · needs funding`;
}

/** Static, at-a-glance view of the selected launch template's economics. */
function TemplateSummary({
  template,
  quoteAsset,
  bundlerCountOverride,
}: {
  template: LaunchTemplate;
  quoteAsset: QuoteAsset | undefined;
  bundlerCountOverride: string;
}) {
  const decimals = quoteAsset?.decimals ?? 9;
  const sym = quoteAsset?.symbol ?? 'SOL';
  const amt = (v: number | null | undefined) =>
    v == null ? '—' : `${toHumanUnits(v, decimals)} ${sym}`;
  const p = template.params;
  const baseLegs = p.leg_structures?.length ?? p.bundle_leg_count ?? 0;
  const legCount = bundlerCountOverride !== '' ? Number(bundlerCountOverride) : baseLegs;
  const legsOverridden = bundlerCountOverride !== '' && Number(bundlerCountOverride) !== baseLegs;
  return (
    <Card title="Launch template">
      <KV>
        <KVRow label="Name">
          {template.template_name} <span className="muted">({template.variant})</span>
        </KVRow>
        <KVRow label="Dev buy">
          <span className="mono">{amt(p.dev_buy_quote)}</span>
          <span className="muted mono text-xs"> · {p.dev_buy_variant ?? 'buy_exact_sol_in'}</span>
        </KVRow>
        <KVRow label="Slippage">{p.slippage_bps != null ? `${p.slippage_bps} bps` : '—'}</KVRow>
        <KVRow label="Bundle legs">
          <span className="mono">{legCount}</span>
          {legsOverridden && <span className="muted"> · overridden</span>}
          {p.bundle_quote_per_leg != null && (
            <span className="muted"> × {amt(p.bundle_quote_per_leg)}/leg</span>
          )}
        </KVRow>
        <KVRow label="Bundle tip">
          <span className="mono">{amt(p.bundle_tip_quote)}</span>
        </KVRow>
        <KVRow label="Create ix layout">
          <span className="mono text-xs">
            {(p.create_layout ?? canonicalSteps('create')).join(' · ')}
          </span>
        </KVRow>
        {p.leg_structures?.length ? (
          <KVRow label="Leg shapes">
            <div className="space-y-0.5">
              {p.leg_structures.map((r, i) => (
                <div key={i} className="text-xs">
                  <span className="mono">{r.variant}</span>{' '}
                  <span className="muted mono">[{(r.layout ?? canonicalSteps('buy')).join(' · ')}]</span>
                </div>
              ))}
            </div>
          </KVRow>
        ) : null}
        {(p.is_mayhem_mode || p.cashback_enabled) && (
          <KVRow label="Flags">
            {p.is_mayhem_mode && <span className="badge badge-warn">mayhem</span>}{' '}
            {p.cashback_enabled && <span className="badge badge-info">cashback</span>}
          </KVRow>
        )}
      </KV>
    </Card>
  );
}

/** Static, at-a-glance view of the token identity that will be minted. */
function MetadataSummary({
  metadata,
  overriding,
}: {
  metadata: MetadataTemplate;
  overriding: boolean;
}) {
  const socials = ([
    ['Twitter', metadata.twitter],
    ['Telegram', metadata.telegram],
    ['Website', metadata.website],
  ] as [string, string | null][]).filter((s): s is [string, string] => !!s[1]);
  return (
    <Card
      title="Token metadata"
      actions={overriding ? <span className="badge badge-warn">override</span> : undefined}
    >
      <div className="flex gap-3">
        {metadata.image_uri && (
          <img
            src={ipfsToHttp(metadata.image_uri) ?? undefined}
            alt=""
            className="h-14 w-14 shrink-0 rounded-md border border-[var(--color-line)] object-cover"
          />
        )}
        <div className="min-w-0 flex-1">
          <KV>
            <KVRow label="Name">
              <strong>{metadata.name}</strong> <span className="muted">({metadata.symbol})</span>
            </KVRow>
            {metadata.description && <KVRow label="Description">{metadata.description}</KVRow>}
            {socials.map(([k, v]) => (
              <KVRow key={k} label={k}>
                <a href={v} target="_blank" rel="noreferrer" className="mono text-xs">
                  {v}
                </a>
              </KVRow>
            ))}
            <KVRow label="URI">
              <code className="mono text-xs">{metadata.uri}</code>
            </KVRow>
          </KV>
        </div>
      </div>
    </Card>
  );
}

function TradesTable({ trades }: { trades: TradePriced[] }) {
  const columns: Column<TradePriced>[] = [
    { header: 'Slot', align: 'right', render: (t) => <span className="mono">{t.slot}</span> },
    { header: 'Type', render: (t) => <TradeTypePill type={t.trade_type} /> },
    { header: 'Quote', align: 'right', render: (t) => <span className="mono">{t.amount_quote_display ?? t.amount_quote}</span> },
    {
      header: 'Tx',
      render: (t) => {
        const sig = formatSig(t.tx_signature);
        return sig !== '—' ? <AddressDisplay value={sig} kind="tx" /> : <span className="muted">—</span>;
      },
    },
  ];
  return (
    <div className="mt-4">
      <div className="field-label mb-1">Recent trades on mint</div>
      <DataTable columns={columns} rows={trades} rowKey={(t, i) => `${t.slot}-${i}`} maxHeight={280} />
    </div>
  );
}
