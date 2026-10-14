import { useEffect, useMemo, useState } from 'react';

import { Badge } from 'components/ui/Badge';
import { Button } from 'components/ui/Button';
import { Input } from 'components/ui/Input';
import { InlineAlert, Modal } from 'components/ui/Modal';
import { PageHeader } from 'components/ui/PageHeader';
import { StatTile } from 'components/ui/StatTile';
import { Tabs, TabsList, TabsTrigger } from 'components/ui/Tabs';
import { apiErrorMessage } from 'store/baseApi';
import {
  useCreateTraderWalletMutation,
  useGetTraderWalletsQuery,
  useImportTraderWalletMutation,
  useAttachNoncesMutation,
  useResumeWalletMoveMutation,
  useSyncAllWalletsMutation,
  walletCanSign,
  type TraderWalletRow,
  type WalletMove,
} from '@live/store/liveEndpoints';

import { MoveDialog, SendSolDialog } from './WalletDialogs';
import { WalletPanel } from './WalletPanel';
import {
  READINESS_LABEL,
  READINESS_VARIANT,
  shortKey,
  solText,
  walletTab,
  type WalletTab,
} from './walletFormat';

const STEPS = ['quiesce', 'inflight', 'bags', 'nonces', 'sol', 'verify', 'done'] as const;

const STEP_LABEL: Record<string, string> = {
  quiesce: 'Point rules and the primary flag at the new wallet',
  inflight: 'Wait for entries and sells in flight',
  bags: 'Move token balances',
  nonces: 'Hand nonce authority across',
  sol: 'Sweep spendable SOL',
  verify: 'Check the old wallet is empty',
  done: 'Archive the old wallet',
};

/** Faster while a move runs, so its steps show as they land. */
const POLL_MS = 15000;
const POLL_MOVE_MS = 2000;

export function WalletsPage() {
  const [fastPoll, setFastPoll] = useState(false);
  const { data, isLoading, error } = useGetTraderWalletsQuery(undefined, {
    pollingInterval: fastPoll ? POLL_MOVE_MS : POLL_MS,
  });
  const wallets = useMemo(() => data?.wallets ?? [], [data]);
  const move = data?.move && !data.move.finished_at ? data.move : null;
  useEffect(() => setFastPoll(move != null), [move]);

  const [tab, setTab] = useState<WalletTab>('in_use');
  const [adding, setAdding] = useState<'create' | 'import' | null>(null);
  const [openKey, setOpenKey] = useState<string | null>(null);
  const [moving, setMoving] = useState<string | null>(null);
  const [sending, setSending] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [resumeMove, resumeState] = useResumeWalletMoveMutation();
  const [syncAll, syncState] = useSyncAllWalletsMutation();

  // Refresh re-reads every wallet's balance and nonce accounts from the chain
  // and stores them; the list refetches on the mutation's invalidation.
  const refresh = async () => {
    setActionError(null);
    try {
      const res = await syncAll().unwrap();
      const failed = Object.entries(res).flatMap(([pk, r]) => {
        const msg = r.error ?? r.ok?.history_error;
        return msg ? [`${byKey.get(pk)?.name ?? shortKey(pk)}: ${msg}`] : [];
      });
      if (failed.length) setActionError(failed.join('; '));
    } catch (e) {
      setActionError(apiErrorMessage(e as never) ?? 'Refresh failed');
    }
  };

  const byKey = useMemo(() => new Map(wallets.map((w) => [w.pubkey, w])), [wallets]);
  const shown = useMemo(
    () =>
      wallets
        .filter((w) => walletTab(w) === tab)
        .sort((a, b) => Number(b.is_primary) - Number(a.is_primary) || a.name.localeCompare(b.name)),
    [wallets, tab],
  );
  const counts = useMemo(() => {
    const c: Record<WalletTab, number> = { in_use: 0, idle: 0, archived: 0 };
    for (const w of wallets) c[walletTab(w)] += 1;
    return c;
  }, [wallets]);

  const live = wallets.filter((w) => w.status !== 'archived');
  const totalSol = live.reduce((s, w) => s + (w.sol ?? 0), 0);
  const ready = live.filter((w) => walletCanSign(w.readiness)).length;
  const pnl24h = live.reduce((s, w) => s + w.pnl_24h_sol, 0);
  const primary = wallets.find((w) => w.is_primary);
  const opened = openKey ? byKey.get(openKey) : undefined;
  const movingRow = moving ? byKey.get(moving) : undefined;
  const sendingRow = sending ? byKey.get(sending) : undefined;

  return (
    <div className="flex flex-col gap-4 p-4">
      <PageHeader
        title="Wallets"
        description="Each rule signs with its own wallet or the primary. A move carries rules, bags, nonce authority, and SOL, then archives the old wallet."
        actions={
          <div className="flex gap-2">
            <Button size="sm" variant="ghost" onClick={refresh} disabled={syncState.isLoading}>
              {syncState.isLoading ? 'Reading chain...' : 'Refresh'}
            </Button>
            <Button size="sm" variant="ghost" onClick={() => setAdding('import')}>
              Import
            </Button>
            <Button size="sm" onClick={() => setAdding('create')}>
              Create
            </Button>
          </div>
        }
      />

      <div className="grid grid-cols-2 gap-2 sm:grid-cols-5">
        <StatTile label="Total SOL" value={solText(totalSol)} />
        <StatTile
          label="Can trade"
          value={`${ready} / ${live.length}`}
          tone={ready === live.length ? 'default' : 'red'}
        />
        <StatTile label="Primary" value={primary?.name ?? '-'} />
        <StatTile
          label="PnL 24h"
          value={`${pnl24h >= 0 ? '+' : ''}${solText(pnl24h)}`}
          tone={pnl24h > 0 ? 'green' : pnl24h < 0 ? 'red' : 'default'}
          info="Realized SOL of real positions closed in the last 24 h, summed over the wallets that bought them."
        />
        <StatTile label="Move" value={move ? STEP_LABEL[move.step] ?? move.step : 'none'} size="md" />
      </div>

      {data?.primary_missing && (
        <InlineAlert variant="error">
          No primary wallet is set. Rules without their own wallet do not buy until you set one.
        </InlineAlert>
      )}
      {error && <InlineAlert variant="error">{apiErrorMessage(error) ?? 'Could not load wallets'}</InlineAlert>}
      {actionError && <InlineAlert variant="error">{actionError}</InlineAlert>}
      {isLoading && <p className="text-[12px] text-text-dim">Loading wallets...</p>}

      {move && (
        <MoveCard
          move={move}
          wallets={wallets}
          busy={resumeState.isLoading}
          onResume={async () => {
            setActionError(null);
            try {
              await resumeMove().unwrap();
            } catch (e) {
              setActionError(apiErrorMessage(e as never) ?? 'Resume failed');
            }
          }}
        />
      )}

      <Tabs value={tab} onValueChange={(v) => setTab(v as WalletTab)}>
        <TabsList>
          <TabsTrigger value="in_use">In use ({counts.in_use})</TabsTrigger>
          <TabsTrigger value="idle">Idle ({counts.idle})</TabsTrigger>
          <TabsTrigger value="archived">Archived ({counts.archived})</TabsTrigger>
        </TabsList>
      </Tabs>

      <div className="overflow-x-auto rounded-md border border-white/10">
        <table className="w-full text-left text-[12px]">
          <thead className="text-[11px] text-text-dim">
            <tr className="border-b border-white/10">
              <th className="px-3 py-2 font-medium">Wallet</th>
              <th className="px-3 py-2 font-medium">State</th>
              <th className="px-3 py-2 text-right font-medium">SOL</th>
              <th className="px-3 py-2 text-right font-medium">Nonces</th>
              <th className="px-3 py-2 text-right font-medium">Rules</th>
              <th className="px-3 py-2 text-right font-medium">Open</th>
              <th className="px-3 py-2 text-right font-medium">PnL 24h</th>
              <th className="px-3 py-2 font-medium" />
            </tr>
          </thead>
          <tbody>
            {shown.map((w) => (
              <tr
                key={w.pubkey}
                className="cursor-pointer border-b border-white/5 hover:bg-white/3"
                onClick={() => setOpenKey(w.pubkey)}
              >
                <td className="px-3 py-2">
                  <div className="font-medium text-text">
                    {w.is_primary ? '★ ' : ''}
                    {w.name}
                  </div>
                  <div className="font-mono text-[11px] text-text-dim" title={w.pubkey}>
                    {shortKey(w.pubkey)}
                  </div>
                </td>
                <td className="px-3 py-2">
                  {w.status === 'active' ? (
                    <Badge variant={READINESS_VARIANT[w.readiness]} title={w.init_error ?? w.readiness_reason}>
                      {READINESS_LABEL[w.readiness]}
                    </Badge>
                  ) : (
                    <Badge variant="neutral">{w.status}</Badge>
                  )}
                </td>
                <td className="px-3 py-2 text-right font-mono tabular-nums">{solText(w.sol)}</td>
                <td className="px-3 py-2 text-right font-mono tabular-nums">{w.nonce_count}</td>
                <td className="px-3 py-2 text-right font-mono tabular-nums">{w.rule_count}</td>
                <td className="px-3 py-2 text-right font-mono tabular-nums">{w.open_positions}</td>
                <td
                  className={`px-3 py-2 text-right font-mono tabular-nums ${
                    w.pnl_24h_sol > 0 ? 'text-green' : w.pnl_24h_sol < 0 ? 'text-red' : 'text-text-dim'
                  }`}
                >
                  {w.pnl_24h_sol === 0 ? '-' : `${w.pnl_24h_sol > 0 ? '+' : ''}${solText(w.pnl_24h_sol)}`}
                </td>
                <td className="px-3 py-2 text-right">
                  <Button size="xs" variant={walletCanSign(w.readiness) ? 'ghost' : 'primary'}>
                    {fixLabel(w)}
                  </Button>
                </td>
              </tr>
            ))}
            {!isLoading && shown.length === 0 && (
              <tr>
                <td className="px-3 py-4 text-text-dim" colSpan={8}>
                  {tab === 'in_use'
                    ? 'No wallet in use. The env key still signs until one is registered.'
                    : tab === 'idle'
                      ? 'No idle wallet.'
                      : 'No archived wallet.'}
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>

      {adding && <AddWalletDialog mode={adding} onClose={() => setAdding(null)} />}
      {opened && (
        <WalletPanel
          wallet={opened}
          wallets={wallets}
          moveOpen={move != null}
          onClose={() => setOpenKey(null)}
          onMove={() => {
            setOpenKey(null);
            setMoving(opened.pubkey);
          }}
          onSend={() => setSending(opened.pubkey)}
        />
      )}
      {movingRow && <MoveDialog source={movingRow} wallets={wallets} onClose={() => setMoving(null)} />}
      {sendingRow && <SendSolDialog source={sendingRow} wallets={wallets} onClose={() => setSending(null)} />}
    </div>
  );
}

/** The row button names the first thing to fix; a wallet that can trade just opens. */
function fixLabel(w: TraderWalletRow): string {
  if (w.status !== 'active') return 'Open';
  if (w.readiness === 'needs_nonces') return 'Add nonces';
  if (w.readiness === 'init_failed') return 'See error';
  return 'Open';
}

/** Create a key on the server, or import one with the nonce accounts it already owns. */
function AddWalletDialog({ mode, onClose }: { mode: 'create' | 'import'; onClose: () => void }) {
  const [create, createState] = useCreateTraderWalletMutation();
  const [importWallet, importState] = useImportTraderWalletMutation();
  const [attach, attachState] = useAttachNoncesMutation();
  const [name, setName] = useState('');
  const [secret, setSecret] = useState('');
  const [nonces, setNonces] = useState('');
  const [error, setError] = useState<string | null>(null);
  const busy = createState.isLoading || importState.isLoading || attachState.isLoading;

  const submit = async () => {
    setError(null);
    try {
      if (mode === 'create') {
        await create({ name: name.trim() }).unwrap();
      } else {
        const made = await importWallet({ name: name.trim(), private_key: secret }).unwrap();
        setSecret('');
        if (made.sync.Err) {
          setError(`Imported. The chain read failed (${made.sync.Err}); press Refresh to read it again.`);
          return;
        }
        const list = nonces.split(/[\s,]+/).filter(Boolean);
        if (list.length > 0) {
          const res = await attach({ pubkey: made.pubkey, nonces: list }).unwrap();
          if (res.rejected.length > 0) {
            setError(`Imported. Rejected nonces: ${res.rejected.map((r) => `${shortKey(r.pubkey)} (${r.reason})`).join(', ')}`);
            return;
          }
        }
      }
      onClose();
    } catch (e) {
      setError(apiErrorMessage(e as never) ?? 'Failed');
    }
  };

  return (
    <Modal
      title={mode === 'create' ? 'Create wallet' : 'Import wallet'}
      open
      onClose={() => {
        setSecret('');
        onClose();
      }}
    >
      <div className="flex flex-col gap-3 text-[12px]">
        <p className="text-text-dim">
          {mode === 'create'
            ? 'The server generates the key. This page only sees the pubkey. Fund it and add nonces before it can trade.'
            : 'Paste a base58 secret or a JSON byte array. It is encrypted on arrival and never returned. Its balance and the nonce accounts it uses are read from the chain.'}
        </p>
        <label className="flex flex-col gap-1 text-text-dim">
          Name
          <Input value={name} onChange={(e) => setName(e.target.value)} placeholder="Desk 2" maxLength={32} autoComplete="off" />
        </label>
        {mode === 'import' && (
          <>
            <label className="flex flex-col gap-1 text-text-dim">
              Private key
              <Input type="password" value={secret} onChange={(e) => setSecret(e.target.value)} autoComplete="off" />
            </label>
            <label className="flex flex-col gap-1 text-text-dim">
              More nonce accounts it owns (optional; the chain read finds the ones it uses)
              <textarea
                value={nonces}
                onChange={(e) => setNonces(e.target.value)}
                rows={3}
                placeholder="One per line"
                className="rounded-md border border-white/10 bg-bg-card px-2 py-1 font-mono text-[11px] text-text"
              />
            </label>
          </>
        )}
        {error && <InlineAlert variant="error">{error}</InlineAlert>}
        <div className="flex justify-end gap-2">
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button disabled={busy || !name.trim() || (mode === 'import' && !secret.trim())} onClick={submit}>
            {busy ? 'Working...' : mode === 'create' ? 'Create' : 'Import'}
          </Button>
        </div>
      </div>
    </Modal>
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
  const waiting = move.step === 'inflight' && move.error != null;
  const failed = move.error != null && !waiting;
  return (
    <section className="rounded-md border border-white/10 bg-white/3 p-3 text-[12px]">
      <div className="mb-2 flex items-center justify-between gap-3">
        <div>
          <div className="font-medium text-text">Move in progress</div>
          <div className="text-text-dim">
            {nameOf(move.from_wallet)} to {nameOf(move.to_wallet)}. The server advances it on its own.
          </div>
        </div>
        {failed && (
          <Button size="sm" disabled={busy} onClick={onResume}>
            {busy ? 'Working...' : 'Retry step'}
          </Button>
        )}
      </div>
      {move.error && <p className={`mb-2 ${failed ? 'text-red' : 'text-warning'}`}>{move.error}</p>}
      <ol className="flex flex-col gap-1">
        {STEPS.map((step, i) => (
          <li key={step} className={i < at ? 'text-green' : i === at ? 'text-text' : 'text-text-dim/70'}>
            {i < at ? '✓' : i === at ? '→' : '·'} {STEP_LABEL[step]}
          </li>
        ))}
      </ol>
    </section>
  );
}
