import { useMemo, useState } from 'react';
import {
  useWalletPoolQuery,
  useGenerateWalletsMutation,
  useFundPoolMutation,
  useTransferSolMutation,
  useExportWalletKeyMutation,
} from '@shared/store/endpoints';
import { apiErrorMessage } from '@shared/store/baseApi';
import {
  AgeCell,
  Banner,
  Button,
  Card,
  Column,
  DataTable,
  Field,
  Input,
  Select,
  StatusPill,
  RolePill,
  roleColorVar,
  statusTone,
  AddressDisplay,
} from '@shared/components/ui';
import { formatSol } from '@shared/lib/format';
import type { ManagedWalletPool, TransferReport, WalletRole, WalletStatus } from '@shared/types';

// A wallet in one of these statuses can't be an ad-hoc transfer source (a launch is
// mid-flight against it, or it's retired) — mirrors the backend status guard.
const NON_SOURCE_STATUSES = new Set<WalletStatus>(['funding', 'reserved', 'retired']);

const LOW_POOL_THRESHOLD = 3;
const ROLES: WalletRole[] = ['dev', 'bundler', 'treasury', 'trading'];
const STATUSES: WalletStatus[] = ['generated', 'funding', 'funded', 'reserved', 'used', 'retired'];

// The balance poller only refreshes `generated`/`funding` (+ the treasury by role);
// `balance_lamports` freezes at the funded-era snapshot once a wallet is claimed.
// `used`/`retired` are terminal: the SOL is spent or swept to treasury, so a snapshot
// there reads as phantom holdings — show `—`. A `reserved` wallet, though, STILL HOLDS
// its SOL (a launch reserved it but hasn't spent yet — and a failed launch releases it
// back to `funded` intact), so hiding its balance made a real, funded wallet look empty.
// Show its last-known snapshot, dimmed + tagged, instead.
const SPENT_BALANCE_STATUSES = new Set<WalletStatus>(['used', 'retired']);

// Tone → color var, so the per-role status bar segments match the StatusPill palette.
const TONE_COLOR: Record<string, string> = {
  good: 'var(--color-good)',
  warn: 'var(--color-warn)',
  bad: 'var(--color-bad)',
  info: 'var(--color-info)',
  neutral: 'var(--color-line-2)',
};

/**
 * At-a-glance card for one role: big funded count (red when below threshold),
 * a proportional status bar, and per-status chips. Clickable to filter the pool.
 */
function RoleSummaryCard({
  role,
  counts,
  total,
  active,
  onSelect,
}: {
  role: string;
  counts: Record<string, number>;
  total: number;
  active: boolean;
  onSelect: () => void;
}) {
  const funded = counts.funded ?? 0;
  const low = funded < LOW_POOL_THRESHOLD;
  const accent = roleColorVar(role);
  return (
    <button
      type="button"
      onClick={onSelect}
      className="panel px-3.5 py-3 text-left transition-colors hover:border-[var(--color-line-2)]"
      style={{
        borderLeft: `3px solid ${accent}`,
        outline: active ? `1px solid ${accent}` : undefined,
      }}
    >
      <div className="flex items-center justify-between">
        <RolePill role={role} />
        <span className="text-xs muted">{total} total</span>
      </div>

      <div className="mt-2 flex items-baseline gap-1.5">
        <span
          className="text-2xl font-semibold leading-none"
          style={{ color: low ? 'var(--color-bad)' : undefined }}
        >
          {funded}
        </span>
        <span className="text-xs muted">funded{low ? ` · low (<${LOW_POOL_THRESHOLD})` : ''}</span>
      </div>

      {total > 0 && (
        <div className="mt-2 flex h-1.5 overflow-hidden rounded-full bg-[var(--color-line)]">
          {STATUSES.map((s) => {
            const n = counts[s] ?? 0;
            if (n === 0) return null;
            return (
              <div
                key={s}
                title={`${s}: ${n}`}
                style={{ width: `${(n / total) * 100}%`, background: TONE_COLOR[statusTone(s)] }}
              />
            );
          })}
        </div>
      )}

      <div className="mt-2 flex flex-wrap gap-x-2.5 gap-y-0.5 text-xs">
        {STATUSES.filter((s) => (counts[s] ?? 0) > 0).map((s) => (
          <span key={s} className="muted">
            <span style={{ color: TONE_COLOR[statusTone(s)] }}>●</span> {counts[s]} {s}
          </span>
        ))}
      </div>
    </button>
  );
}

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
    skipPollingIfUnfocused: true,
  });
  const [generate, gen] = useGenerateWalletsMutation();
  const [fund, funding] = useFundPoolMutation();

  // Wallet-to-wallet transfer dialog, keyed by the source wallet.
  const [transferTarget, setTransferTarget] = useState<ManagedWalletPool | null>(null);
  const [transferDest, setTransferDest] = useState('');
  const [transferAmount, setTransferAmount] = useState('');
  const [transferMax, setTransferMax] = useState(false);
  const [transferResult, setTransferResult] = useState<TransferReport | null>(null);
  const [transferErr, setTransferErr] = useState<string | null>(null);
  const [transferSol, transferring] = useTransferSolMutation();

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

  const openTransfer = (w: ManagedWalletPool) => {
    setTransferTarget(w);
    setTransferDest('');
    setTransferAmount('');
    setTransferMax(false);
    setTransferResult(null);
    setTransferErr(null);
  };

  const closeTransfer = () => {
    setTransferTarget(null);
    setTransferDest('');
    setTransferAmount('');
    setTransferMax(false);
    setTransferResult(null);
    setTransferErr(null);
  };

  // Candidate destinations for the open transfer: any other wallet that isn't
  // retired (topping up a shredded wallet is pointless).
  const transferDestinations = useMemo(
    () =>
      transferTarget
        ? wallets.filter((w) => w.id !== transferTarget.id && w.status !== 'retired')
        : [],
    [wallets, transferTarget],
  );

  const onTransfer = async () => {
    if (!transferTarget || !transferDest) return;
    if (!transferMax && !(Number(transferAmount) > 0)) return;
    setTransferErr(null);
    try {
      const res = await transferSol({
        from_id: transferTarget.id,
        to_id: transferDest,
        ...(transferMax ? { max: true } : { amount_sol: Number(transferAmount) }),
      }).unwrap();
      setTransferResult(res);
    } catch (e) {
      setTransferResult(null);
      setTransferErr(apiErrorMessage(e as Parameters<typeof apiErrorMessage>[0]) ?? 'Transfer failed.');
    }
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

  // Cluster the flat pool by role (in canonical ROLES order) so same-role wallets
  // read as a group even without filtering; status/age keep a stable sub-order.
  const sortedWallets = useMemo(() => {
    const roleIdx = (r: string) => {
      const i = ROLES.indexOf(r as WalletRole);
      return i === -1 ? ROLES.length : i;
    };
    return [...wallets].sort(
      (a, b) =>
        roleIdx(a.role) - roleIdx(b.role) ||
        STATUSES.indexOf(a.status) - STATUSES.indexOf(b.status) ||
        (a.label ?? '').localeCompare(b.label ?? ''),
    );
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
    { header: 'Role', render: (w) => <RolePill role={w.role} /> },
    { header: 'Status', render: (w) => <StatusPill status={w.status} /> },
    {
      header: 'Balance',
      align: 'right',
      render: (w) =>
        SPENT_BALANCE_STATUSES.has(w.status) ? (
          <span className="muted" title="Not tracked after a wallet is used/retired — its SOL is spent or swept to the treasury">
            —
          </span>
        ) : w.status === 'reserved' ? (
          // Reserved by an in-flight launch but still holding its SOL — show the
          // last-known snapshot, dimmed, so a funded wallet never looks empty.
          <span className="mono muted" title="Reserved by an in-flight launch — last-known balance (not live-polled while reserved)">
            {formatSol(w.balance_lamports)}
          </span>
        ) : (
          <span className="mono">{formatSol(w.balance_lamports)}</span>
        ),
    },
    { header: 'Age', align: 'right', render: (w) => <AgeCell iso={w.created_at} /> },
    {
      header: '',
      align: 'right',
      render: (w) => (
        <div className="flex justify-end gap-1">
          <Button
            variant="ghost"
            size="sm"
            disabled={NON_SOURCE_STATUSES.has(w.status)}
            title={
              NON_SOURCE_STATUSES.has(w.status)
                ? 'A launch is mid-flight against this wallet, or it is retired — not a valid source'
                : undefined
            }
            onClick={() => openTransfer(w)}
          >
            Transfer
          </Button>
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
        </div>
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

      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-4">
        {ROLES.map((role) => {
          const s = counts[role] ?? {};
          const total = STATUSES.reduce((n, st) => n + (s[st] ?? 0), 0);
          return (
            <RoleSummaryCard
              key={role}
              role={role}
              counts={s}
              total={total}
              active={roleFilter === role}
              onSelect={() => setRoleFilter(roleFilter === role ? '' : role)}
            />
          );
        })}
      </div>

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
          rows={sortedWallets}
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

      {transferTarget && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
          onClick={closeTransfer}
        >
          <div className="w-full max-w-lg" onClick={(e) => e.stopPropagation()}>
            <Card title="Transfer SOL">
              <p className="text-xs muted">
                From <RolePill role={transferTarget.role} />{' '}
                <span className="mono">{transferTarget.address}</span>. The source signs and pays
                the fee.
              </p>

              {!transferResult ? (
                <div className="mt-3 space-y-3">
                  <Field label="Destination wallet" htmlFor="transfer-dest">
                    <Select
                      id="transfer-dest"
                      value={transferDest}
                      onChange={(e) => setTransferDest(e.target.value)}
                    >
                      <option value="">Select a wallet…</option>
                      {transferDestinations.map((w) => (
                        <option key={w.id} value={w.id}>
                          {w.role} · {w.address.slice(0, 6)}…{w.address.slice(-6)}
                          {w.label ? ` · ${w.label}` : ''}
                        </option>
                      ))}
                    </Select>
                  </Field>
                  <div className="flex items-end gap-3">
                    <Field label="Amount (SOL)" htmlFor="transfer-amount" className="flex-1">
                      <Input
                        id="transfer-amount"
                        type="number"
                        min={0}
                        step="any"
                        value={transferAmount}
                        disabled={transferMax}
                        onChange={(e) => setTransferAmount(e.target.value)}
                        placeholder="0.05"
                      />
                    </Field>
                    <label className="mb-2 flex items-center gap-2 text-sm">
                      <input
                        type="checkbox"
                        checked={transferMax}
                        onChange={(e) => setTransferMax(e.target.checked)}
                      />
                      Max (sweep to ~0)
                    </label>
                  </div>
                  {transferErr && <Banner tone="bad">{transferErr}</Banner>}
                  <div className="flex justify-end gap-2">
                    <Button variant="ghost" onClick={closeTransfer}>
                      Cancel
                    </Button>
                    <Button
                      variant="primary"
                      loading={transferring.isLoading}
                      disabled={!transferDest || (!transferMax && !(Number(transferAmount) > 0))}
                      onClick={onTransfer}
                    >
                      Send
                    </Button>
                  </div>
                </div>
              ) : (
                <div className="mt-3 space-y-3">
                  <Banner tone="good">
                    {transferResult.signature ? (
                      <>
                        Sent <strong>{formatSol(transferResult.lamports_sent)}</strong> —{' '}
                        <span className="mono break-all">{transferResult.signature}</span>
                      </>
                    ) : (
                      'Nothing to send — the source balance was at or below the sweep floor.'
                    )}
                  </Banner>
                  <div className="flex justify-end">
                    <Button variant="primary" onClick={closeTransfer}>
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
