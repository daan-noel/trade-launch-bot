import { useMemo, useState } from 'react';

import { Button } from 'components/ui/Button';
import { Input } from 'components/ui/Input';
import { Modal } from 'components/ui/Modal';
import { PageHeader } from 'components/ui/PageHeader';
import { Select } from 'components/ui/Select';
import { apiErrorMessage } from 'store/baseApi';
import {
  useBeginWalletMoveMutation,
  useCreateTraderWalletMutation,
  useGetTraderWalletsQuery,
  useImportTraderWalletMutation,
  useProvisionNoncesMutation,
  useResumeWalletMoveMutation,
  useSetPrimaryWalletMutation,
  type TraderWalletRow,
  type WalletMove,
} from '@live/store/liveEndpoints';

const STEPS = ['quiesce', 'inflight', 'bags', 'nonces', 'sol', 'verify', 'done'] as const;

const STEP_LABEL: Record<string, string> = {
  quiesce: 'Point rules at the new wallet',
  inflight: 'Wait for entries and sells in flight',
  bags: 'Move token balances',
  nonces: 'Hand nonce authority across',
  sol: 'Sweep spendable SOL',
  verify: 'Check the old wallet is empty',
  done: 'Archive the old wallet',
};

function shortKey(pubkey: string): string {
  if (pubkey.length <= 12) return pubkey;
  return `${pubkey.slice(0, 4)}...${pubkey.slice(-4)}`;
}

function solText(sol: number | null): string {
  if (sol == null) return '-';
  return sol.toFixed(4);
}

export function WalletsPage() {
  const { data, isLoading, error, refetch } = useGetTraderWalletsQuery(undefined, {
    pollingInterval: 15000,
  });
  const [createWallet, createState] = useCreateTraderWalletMutation();
  const [importWallet, importState] = useImportTraderWalletMutation();
  const [setPrimary] = useSetPrimaryWalletMutation();
  const [provision] = useProvisionNoncesMutation();
  const [beginMove, beginState] = useBeginWalletMoveMutation();
  const [resumeMove, resumeState] = useResumeWalletMoveMutation();

  const [creating, setCreating] = useState(false);
  const [importing, setImporting] = useState(false);
  const [name, setName] = useState('');
  const [secret, setSecret] = useState('');
  const [formError, setFormError] = useState<string | null>(null);
  const [moving, setMoving] = useState<TraderWalletRow | null>(null);
  const [dest, setDest] = useState('');
  const [confirmName, setConfirmName] = useState('');
  const [nonceFor, setNonceFor] = useState<TraderWalletRow | null>(null);
  const [nonceCount, setNonceCount] = useState('5');
  const [actionError, setActionError] = useState<string | null>(null);

  const wallets = data?.wallets ?? [];
  const move = data?.move && !data.move.finished_at ? data.move : null;
  const active = wallets.filter((w) => w.status === 'active');
  const ordered = useMemo(() => {
    const rank = (w: TraderWalletRow) =>
      w.status === 'draining' ? 1 : w.status === 'archived' ? 2 : w.is_primary ? -1 : 0;
    return [...wallets].sort((a, b) => rank(a) - rank(b) || a.name.localeCompare(b.name));
  }, [wallets]);

  const busy = createState.isLoading || importState.isLoading || beginState.isLoading || resumeState.isLoading;

  const submitCreate = async () => {
    setFormError(null);
    try {
      await createWallet({ name: name.trim() }).unwrap();
      setCreating(false);
      setName('');
    } catch (e) {
      setFormError(apiErrorMessage(e as never) ?? 'Create failed');
    }
  };

  const submitImport = async () => {
    setFormError(null);
    try {
      await importWallet({ name: name.trim(), private_key: secret }).unwrap();
      setSecret('');
      setName('');
      setImporting(false);
    } catch (e) {
      setFormError(apiErrorMessage(e as never) ?? 'Import failed');
    }
  };

  const run = async (fn: () => Promise<unknown>) => {
    setActionError(null);
    try {
      await fn();
    } catch (e) {
      setActionError(apiErrorMessage(e as never) ?? 'Request failed');
    }
  };

  return (
    <div className="flex flex-col gap-4 p-4">
      <PageHeader
        title="Wallets"
        description="Active wallets sign entries. A move carries balances and nonce authority, then archives the old wallet."
        actions={
          <div className="flex gap-2">
            <Button size="sm" variant="ghost" onClick={() => refetch()}>
              Refresh
            </Button>
            <Button
              size="sm"
              variant="ghost"
              onClick={() => {
                setFormError(null);
                setName('');
                setImporting(true);
              }}
            >
              Import
            </Button>
            <Button
              size="sm"
              onClick={() => {
                setFormError(null);
                setName('');
                setCreating(true);
              }}
            >
              Create
            </Button>
          </div>
        }
      />

      {error && <p className="text-[12px] text-red">{apiErrorMessage(error) ?? 'Could not load wallets'}</p>}
      {actionError && <p className="text-[12px] text-red">{actionError}</p>}
      {isLoading && <p className="text-[12px] text-text-dim">Loading wallets...</p>}

      {move && <MoveCard move={move} wallets={wallets} busy={resumeState.isLoading} onResume={() => run(() => resumeMove().unwrap())} />}

      <div className="overflow-x-auto rounded-md border border-white/10">
        <table className="w-full text-left text-[12px]">
          <thead className="text-[11px] text-text-dim">
            <tr className="border-b border-white/10">
              <th className="px-3 py-2 font-medium">Wallet</th>
              <th className="px-3 py-2 font-medium">Status</th>
              <th className="px-3 py-2 font-medium">SOL</th>
              <th className="px-3 py-2 font-medium">Nonces</th>
              <th className="px-3 py-2 font-medium">Rules</th>
              <th className="px-3 py-2 font-medium">Open</th>
              <th className="px-3 py-2 font-medium">Actions</th>
            </tr>
          </thead>
          <tbody>
            {ordered.map((w) => (
              <tr key={w.pubkey} className="border-b border-white/5">
                <td className="px-3 py-2">
                  <div className="font-medium text-text">
                    {w.name}
                    {w.is_primary ? ' (primary)' : ''}
                  </div>
                  <div className="font-mono text-[11px] text-text-dim" title={w.pubkey}>
                    {shortKey(w.pubkey)}
                    {w.key_loaded ? '' : ' (key not on this box)'}
                  </div>
                </td>
                <td className="px-3 py-2 capitalize">{w.status}</td>
                <td className="px-3 py-2 tabular-nums">{solText(w.sol)}</td>
                <td className="px-3 py-2 tabular-nums">{w.nonce_count}</td>
                <td className="px-3 py-2 tabular-nums">{w.rule_count}</td>
                <td className="px-3 py-2 tabular-nums">{w.open_positions}</td>
                <td className="px-3 py-2">
                  <div className="flex flex-wrap gap-1">
                    {w.status === 'active' && !w.is_primary && (
                      <Button size="xs" variant="ghost" onClick={() => run(() => setPrimary(w.pubkey).unwrap())}>
                        Set as primary
                      </Button>
                    )}
                    {w.status === 'active' && w.key_loaded && (
                      <Button size="xs" variant="ghost" onClick={() => { setNonceFor(w); setNonceCount('5'); setActionError(null); }}>
                        Provision nonces
                      </Button>
                    )}
                    {w.status === 'active' && w.key_loaded && !move && (
                      <Button
                        size="xs"
                        variant="ghost"
                        onClick={() => {
                          setMoving(w);
                          setDest(active.find((o) => o.pubkey !== w.pubkey)?.pubkey ?? '');
                          setConfirmName('');
                          setActionError(null);
                        }}
                      >
                        Move
                      </Button>
                    )}
                  </div>
                </td>
              </tr>
            ))}
            {!isLoading && ordered.length === 0 && (
              <tr>
                <td className="px-3 py-4 text-text-dim" colSpan={7}>
                  No wallets in the registry. The env key still signs until one is created.
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>

      <Modal title="Create wallet" open={creating} onClose={() => setCreating(false)}>
        <div className="flex flex-col gap-3 text-[12px]">
          <p className="text-text-dim">The server generates the key. This page only sees the pubkey. Fund it and provision nonces before it can trade.</p>
          <label className="flex flex-col gap-1 text-text-dim">
            Name
            <Input value={name} onChange={(e) => setName(e.target.value)} placeholder="desk-2" autoComplete="off" />
          </label>
          {formError && <p className="text-red">{formError}</p>}
          <div className="flex justify-end gap-2">
            <Button variant="ghost" onClick={() => setCreating(false)}>Cancel</Button>
            <Button disabled={busy || !name.trim()} onClick={submitCreate}>Create</Button>
          </div>
        </div>
      </Modal>

      <Modal title="Import wallet" open={importing} onClose={() => { setImporting(false); setSecret(''); }}>
        <div className="flex flex-col gap-3 text-[12px]">
          <p className="text-text-dim">Paste a base58 secret or a JSON byte array. It is encrypted on arrival and dropped from the request.</p>
          <label className="flex flex-col gap-1 text-text-dim">
            Name
            <Input value={name} onChange={(e) => setName(e.target.value)} placeholder="desk-2" autoComplete="off" />
          </label>
          <label className="flex flex-col gap-1 text-text-dim">
            Private key
            <Input type="password" value={secret} onChange={(e) => setSecret(e.target.value)} autoComplete="off" />
          </label>
          {formError && <p className="text-red">{formError}</p>}
          <div className="flex justify-end gap-2">
            <Button variant="ghost" onClick={() => { setImporting(false); setSecret(''); }}>Cancel</Button>
            <Button disabled={busy || !name.trim() || !secret.trim()} onClick={submitImport}>Import</Button>
          </div>
        </div>
      </Modal>

      <Modal title={moving ? `Move ${moving.name}` : 'Move'} open={moving != null} onClose={() => setMoving(null)}>
        {moving && (
          <div className="flex flex-col gap-3 text-[12px]">
            <p className="text-text-dim">
              Rules that name {moving.name} move first. In-flight entries and sells finish, then balances, nonce authority, and SOL move. The old wallet is archived only after that checks out.
            </p>
            <label className="flex flex-col gap-1 text-text-dim">
              Destination
              <Select value={dest} onChange={(e) => setDest(e.target.value)}>
                <option value="">Choose a wallet</option>
                {active.filter((w) => w.pubkey !== moving.pubkey && w.key_loaded).map((w) => (
                  <option key={w.pubkey} value={w.pubkey}>{w.name}</option>
                ))}
              </Select>
            </label>
            <label className="flex flex-col gap-1 text-text-dim">
              Type {moving.name} to confirm
              <Input value={confirmName} onChange={(e) => setConfirmName(e.target.value)} autoComplete="off" />
            </label>
            <div className="flex justify-end gap-2">
              <Button variant="ghost" onClick={() => setMoving(null)}>Cancel</Button>
              <Button
                disabled={busy || !dest || confirmName !== moving.name}
                onClick={() =>
                  run(async () => {
                    await beginMove({ from: moving.pubkey, to: dest }).unwrap();
                    setMoving(null);
                  })
                }
              >
                Start move
              </Button>
            </div>
          </div>
        )}
      </Modal>

      <Modal title={nonceFor ? `Nonces for ${nonceFor.name}` : 'Nonces'} open={nonceFor != null} onClose={() => setNonceFor(null)}>
        {nonceFor && (
          <div className="flex flex-col gap-3 text-[12px]">
            <p className="text-text-dim">Creates rent-exempt nonce accounts owned by this wallet. 1 to 16.</p>
            <Input value={nonceCount} onChange={(e) => setNonceCount(e.target.value)} inputMode="numeric" />
            <div className="flex justify-end gap-2">
              <Button variant="ghost" onClick={() => setNonceFor(null)}>Cancel</Button>
              <Button
                disabled={busy}
                onClick={() =>
                  run(async () => {
                    const count = Number(nonceCount);
                    await provision({ pubkey: nonceFor.pubkey, count }).unwrap();
                    setNonceFor(null);
                  })
                }
              >
                Provision
              </Button>
            </div>
          </div>
        )}
      </Modal>
    </div>
  );
}

function MoveCard({
  move,
  wallets,
  busy,
  onResume,
}: {
  move: WalletMove;
  wallets: TraderWalletRow[];
  busy: boolean;
  onResume: () => void;
}) {
  const nameOf = (pubkey: string) => wallets.find((w) => w.pubkey === pubkey)?.name ?? shortKey(pubkey);
  const at = STEPS.indexOf(move.step as (typeof STEPS)[number]);
  return (
    <section className="rounded-md border border-white/10 bg-white/3 p-3 text-[12px]">
      <div className="mb-2 flex items-center justify-between gap-3">
        <div>
          <div className="font-medium text-text">Move in progress</div>
          <div className="text-text-dim">{nameOf(move.from_wallet)} to {nameOf(move.to_wallet)}</div>
        </div>
        <Button size="sm" disabled={busy || move.step === 'done'} onClick={onResume}>
          {busy ? 'Working…' : 'Resume'}
        </Button>
      </div>
      {move.error && <p className="mb-2 text-red">{move.error}</p>}
      <ol className="flex flex-col gap-1">
        {STEPS.map((step, i) => (
          <li key={step} className={i < at ? 'text-text-dim' : i === at ? 'text-text' : 'text-text-dim/70'}>
            {i < at ? 'done' : i === at ? 'now' : 'wait'} - {STEP_LABEL[step]}
          </li>
        ))}
      </ol>
    </section>
  );
}
