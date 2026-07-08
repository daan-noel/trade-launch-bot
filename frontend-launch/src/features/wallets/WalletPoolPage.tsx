import { useMemo, useState } from 'react';
import {
  useWalletPoolQuery,
  useGenerateWalletsMutation,
  useFundPoolMutation,
  useExportWalletKeyMutation,
} from '@shared/store/endpoints';
import { apiErrorMessage } from '@shared/store/baseApi';
import {
  Banner,
  Button,
  Card,
  Column,
  DataTable,
  Field,
  Input,
  Select,
  StatusPill,
  AddressDisplay,
} from '@shared/components/ui';
import { formatAge, formatSol } from '@shared/lib/format';
import type { ManagedWalletPool, WalletRole, WalletStatus } from '@shared/types';

const LOW_POOL_THRESHOLD = 3;
const ROLES: WalletRole[] = ['dev', 'bundler', 'treasury', 'trading'];
const STATUSES: WalletStatus[] = ['generated', 'funding', 'funded', 'reserved', 'used', 'retired'];

export function WalletPoolPage() {
  const [roleFilter, setRoleFilter] = useState('');
  const [genRole, setGenRole] = useState<WalletRole>('bundler');
  const [genCount, setGenCount] = useState(5);
  const [genLabel, setGenLabel] = useState('');
  const [msg, setMsg] = useState<string | null>(null);

  const { data: wallets = [], isFetching, error } = useWalletPoolQuery(roleFilter || undefined, {
    // Poll while any wallet is mid-lifecycle (a treasury send in flight or awaiting
    // manual funding); stop once the pool settles.
    pollingInterval: 5000,
  });
  const [generate, gen] = useGenerateWalletsMutation();
  const [fund, funding] = useFundPoolMutation();

  // Private-key export dialog. The secret + revealed key live ONLY in local
  // state (never Redux/RTK cache) and are wiped when the dialog closes.
  const [exportTarget, setExportTarget] = useState<ManagedWalletPool | null>(null);
  const [exportSecret, setExportSecret] = useState('');
  const [revealedKey, setRevealedKey] = useState<string | null>(null);
  const [exportErr, setExportErr] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [exportKey, exporting] = useExportWalletKeyMutation();

  const closeExport = () => {
    setExportTarget(null);
    setExportSecret('');
    setRevealedKey(null);
    setExportErr(null);
    setCopied(false);
  };

  const onExport = async () => {
    if (!exportTarget || !exportSecret) return;
    setExportErr(null);
    try {
      const res = await exportKey({ id: exportTarget.id, secret: exportSecret }).unwrap();
      setRevealedKey(res.private_key_base58);
    } catch (e) {
      setRevealedKey(null);
      setExportErr(apiErrorMessage(e as Parameters<typeof apiErrorMessage>[0]) ?? 'Export failed.');
    }
  };

  const onCopyKey = async () => {
    if (!revealedKey) return;
    await navigator.clipboard.writeText(revealedKey);
    setCopied(true);
  };

  const counts = useMemo(() => {
    const c: Record<string, Record<string, number>> = {};
    for (const role of ROLES) c[role] = Object.fromEntries(STATUSES.map((s) => [s, 0]));
    for (const w of wallets) {
      if (!c[w.role]) c[w.role] = Object.fromEntries(STATUSES.map((s) => [s, 0]));
      c[w.role][w.status] = (c[w.role][w.status] ?? 0) + 1;
    }
    return c;
  }, [wallets]);

  const lowPoolRoles = Object.entries(counts).filter(
    ([, s]) => (s.funded ?? 0) < LOW_POOL_THRESHOLD,
  );

  const onGenerate = async () => {
    if (genCount < 1) return;
    setMsg(null);
    try {
      await generate({ role: genRole, count: genCount, label_prefix: genLabel || undefined }).unwrap();
      setGenLabel('');
    } catch {
      /* surfaced via mutation error below */
    }
  };

  const onFund = async (role?: string) => {
    setMsg(null);
    try {
      const report = await fund({ role }).unwrap();
      if (report.outcomes.length === 0) {
        setMsg('Nothing to fund — pool already warm (or no treasury configured).');
      } else {
        const by = (r: string) => report.outcomes.filter((o) => o.result === r).length;
        const parts = [
          by('sent') && `${by('sent')} sent`,
          by('dry_run') && `${by('dry_run')} dry-run`,
          by('failed') && `${by('failed')} failed`,
          by('skipped_cap') && `${by('skipped_cap')} skipped (safety cap)`,
        ].filter(Boolean);
        setMsg(`Funding pass: ${parts.join(', ') || 'no transfers'} — ${formatSol(report.spent_lamports)} spent.`);
      }
    } catch {
      /* surfaced via mutation error below */
    }
  };

  const columns: Column<ManagedWalletPool>[] = [
    { header: 'Address', render: (w) => <AddressDisplay value={w.address} lead={6} tail={6} /> },
    { header: 'Label', render: (w) => w.label ?? <span className="muted">—</span> },
    { header: 'Role', render: (w) => w.role },
    { header: 'Status', render: (w) => <StatusPill status={w.status} /> },
    { header: 'Balance', align: 'right', render: (w) => <span className="mono">{formatSol(w.balance_lamports)}</span> },
    { header: 'Age', align: 'right', render: (w) => formatAge(w.created_at) },
    {
      header: '',
      align: 'right',
      render: (w) => (
        <Button
          variant="ghost"
          size="sm"
          onClick={() => {
            closeExport();
            setExportTarget(w);
          }}
        >
          Export key
        </Button>
      ),
    },
  ];

  const mutationError = apiErrorMessage(gen.error) ?? apiErrorMessage(funding.error) ?? apiErrorMessage(error);

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h1 className="text-lg font-semibold">Wallet Pool</h1>
        <Button variant="primary" loading={funding.isLoading} onClick={() => onFund()}>
          Fund pool
        </Button>
      </div>

      {lowPoolRoles.length > 0 && (
        <Banner
          tone="warn"
          actions={
            <Button variant="primary" size="sm" loading={funding.isLoading} onClick={() => onFund()}>
              Fund
            </Button>
          }
        >
          <strong>Low pool:</strong>{' '}
          {lowPoolRoles.map(([role, s]) => `${role} (${s.funded ?? 0} funded)`).join(', ')} — below{' '}
          {LOW_POOL_THRESHOLD}.
        </Banner>
      )}
      {msg && <Banner tone="info">{msg}</Banner>}
      {mutationError && <Banner tone="bad">{mutationError}</Banner>}

      <Card title="Generate wallets">
        <div className="flex flex-wrap items-end gap-3">
          <Field label="Role" htmlFor="gen-role" className="w-40">
            <Select id="gen-role" value={genRole} onChange={(e) => setGenRole(e.target.value as WalletRole)}>
              {ROLES.map((r) => (
                <option key={r} value={r}>
                  {r}
                </option>
              ))}
            </Select>
          </Field>
          <Field label="Count" htmlFor="gen-count" className="w-28">
            <Input
              id="gen-count"
              type="number"
              min={1}
              max={200}
              value={genCount}
              onChange={(e) => setGenCount(Number(e.target.value))}
            />
          </Field>
          <Field label="Label prefix (optional)" htmlFor="gen-label" className="flex-1 min-w-48">
            <Input
              id="gen-label"
              value={genLabel}
              onChange={(e) => setGenLabel(e.target.value)}
              placeholder="e.g. batch-07"
            />
          </Field>
          <Button variant="primary" loading={gen.isLoading} onClick={onGenerate}>
            Generate
          </Button>
        </div>
        <p className="mt-2 text-xs muted">
          Creates fresh ed25519 keypairs, envelope-encrypts each into the keystore, and inserts them
          as <code>generated</code>. Fund them (manually, or the treasury pass above) — the balance
          poller promotes to <code>funded</code> once SOL lands.
        </p>
      </Card>

      <Card title="Status counts">
        <table className="dt">
          <thead>
            <tr>
              <th>Role</th>
              {STATUSES.map((s) => (
                <th key={s} className="text-right">
                  {s}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {Object.entries(counts).map(([role, s]) => (
              <tr key={role}>
                <td className="font-medium">{role}</td>
                {STATUSES.map((st) => (
                  <td key={st} className="text-right mono">
                    {s[st] ?? 0}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </Card>

      <Card
        title={`Pool (${wallets.length})`}
        actions={
          <Select value={roleFilter} onChange={(e) => setRoleFilter(e.target.value)} className="w-40">
            <option value="">All roles</option>
            {ROLES.map((r) => (
              <option key={r} value={r}>
                {r}
              </option>
            ))}
          </Select>
        }
      >
        <DataTable
          columns={columns}
          rows={wallets}
          rowKey={(w) => w.id}
          loading={isFetching}
          empty="No wallets yet — generate a batch above."
        />
      </Card>

      {exportTarget && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
          onClick={closeExport}
        >
          <div className="w-full max-w-lg" onClick={(e) => e.stopPropagation()}>
            <Card title="Export private key">
              <Banner tone="bad">
                <strong>Danger:</strong> this reveals the raw private key for{' '}
                <span className="mono">{exportTarget.address}</span>. Anyone with this key controls
                the wallet's funds. Only export to import into a wallet you control, over a trusted
                connection.
              </Banner>

              {!revealedKey ? (
                <div className="mt-3 space-y-3">
                  <Field label="Export secret" htmlFor="export-secret">
                    <Input
                      id="export-secret"
                      type="password"
                      autoComplete="off"
                      value={exportSecret}
                      onChange={(e) => setExportSecret(e.target.value)}
                      placeholder="WALLET_EXPORT_SECRET"
                    />
                  </Field>
                  {exportErr && <Banner tone="bad">{exportErr}</Banner>}
                  <div className="flex justify-end gap-2">
                    <Button variant="ghost" onClick={closeExport}>
                      Cancel
                    </Button>
                    <Button
                      variant="primary"
                      loading={exporting.isLoading}
                      disabled={!exportSecret}
                      onClick={onExport}
                    >
                      Reveal key
                    </Button>
                  </div>
                </div>
              ) : (
                <div className="mt-3 space-y-3">
                  <Field label="Private key (base58)" htmlFor="export-key">
                    <Input id="export-key" readOnly value={revealedKey} className="mono" />
                  </Field>
                  <p className="text-xs muted">
                    Copy it now — it is not stored anywhere and disappears when you close this
                    dialog.
                  </p>
                  <div className="flex justify-end gap-2">
                    <Button variant="ghost" onClick={onCopyKey}>
                      {copied ? 'Copied' : 'Copy'}
                    </Button>
                    <Button variant="primary" onClick={closeExport}>
                      Done
                    </Button>
                  </div>
                </div>
              )}
            </Card>
          </div>
        </div>
      )}
    </div>
  );
}
