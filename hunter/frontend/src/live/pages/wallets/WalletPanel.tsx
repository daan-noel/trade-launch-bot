import { useMemo, useState, type ReactNode } from 'react';

import { Badge } from 'components/ui/Badge';
import { Button } from 'components/ui/Button';
import { Checkbox } from 'components/ui/Checkbox';
import { Input } from 'components/ui/Input';
import { InlineAlert, Modal } from 'components/ui/Modal';
import { Tabs, TabsList, TabsPanel, TabsTrigger } from 'components/ui/Tabs';
import { apiErrorMessage } from 'store/baseApi';
import { useGetStrategyRulesQuery } from 'store/sharedEndpoints';
import {
  useArchiveWalletMutation,
  useAssignRuleWalletMutation,
  useAttachNoncesMutation,
  useGetWalletActivityQuery,
  useGetWalletAssignmentsQuery,
  useGetWalletNoncesQuery,
  useProvisionNoncesMutation,
  useRenameWalletMutation,
  useRestartWalletMutation,
  useRestoreWalletMutation,
  useSetPrimaryWalletMutation,
  useSyncWalletMutation,
  walletCanSign,
  type TraderWalletRow,
} from '@live/store/liveEndpoints';

import {
  activityText,
  READINESS_LABEL,
  READINESS_VARIANT,
  shortKey,
  solText,
} from './walletFormat';

type PanelTab = 'overview' | 'rules' | 'nonces' | 'activity';

/** One wallet: what it needs, which rules it signs, its nonces, what was done to it. */
export function WalletPanel({
  wallet,
  wallets,
  moveOpen,
  onClose,
  onMove,
  onSend,
}: {
  wallet: TraderWalletRow;
  wallets: TraderWalletRow[];
  moveOpen: boolean;
  onClose: () => void;
  onMove: () => void;
  onSend: () => void;
}) {
  const [tab, setTab] = useState<PanelTab>('overview');
  const [error, setError] = useState<string | null>(null);
  const run = async (fn: () => Promise<unknown>) => {
    setError(null);
    try {
      await fn();
    } catch (e) {
      setError(apiErrorMessage(e as never) ?? 'Request failed');
    }
  };

  return (
    <Modal
      size="xl"
      open
      onClose={onClose}
      title={
        <div className="flex flex-wrap items-center gap-2">
          <h2 className="text-[15px] font-bold text-text">
            {wallet.is_primary ? '★ ' : ''}
            {wallet.name}
          </h2>
          <Badge variant={READINESS_VARIANT[wallet.readiness]}>{READINESS_LABEL[wallet.readiness]}</Badge>
          <span className="font-mono text-[11px] text-text-dim">{wallet.pubkey}</span>
          <Button size="xs" variant="ghost" onClick={() => void navigator.clipboard.writeText(wallet.pubkey)}>
            Copy
          </Button>
        </div>
      }
    >
      {error && <InlineAlert variant="error">{error}</InlineAlert>}
      <Tabs value={tab} onValueChange={(v) => setTab(v as PanelTab)}>
        <TabsList>
          <TabsTrigger value="overview">Overview</TabsTrigger>
          <TabsTrigger value="rules">Rules ({wallet.rule_count})</TabsTrigger>
          <TabsTrigger value="nonces">Nonces ({wallet.nonce_count})</TabsTrigger>
          <TabsTrigger value="activity">Activity</TabsTrigger>
        </TabsList>
        <TabsPanel value="overview">
          <Overview wallet={wallet} moveOpen={moveOpen} run={run} onMove={onMove} onSend={onSend} onNonces={() => setTab('nonces')} />
        </TabsPanel>
        <TabsPanel value="rules">
          <RulesTab wallet={wallet} wallets={wallets} run={run} />
        </TabsPanel>
        <TabsPanel value="nonces">
          <NoncesTab wallet={wallet} run={run} />
        </TabsPanel>
        <TabsPanel value="activity">
          <ActivityTab wallet={wallet} />
        </TabsPanel>
      </Tabs>
    </Modal>
  );
}

type Run = (fn: () => Promise<unknown>) => Promise<void>;

function Check({ ok, label, children }: { ok: boolean; label: string; children?: ReactNode }) {
  return (
    <li className="flex flex-wrap items-center gap-2">
      <span className={ok ? 'text-green' : 'text-red'}>{ok ? '✓' : '✗'}</span>
      <span className="text-text">{label}</span>
      {children}
    </li>
  );
}

function Overview({
  wallet,
  moveOpen,
  run,
  onMove,
  onSend,
  onNonces,
}: {
  wallet: TraderWalletRow;
  moveOpen: boolean;
  run: Run;
  onMove: () => void;
  onSend: () => void;
  onNonces: () => void;
}) {
  const [setPrimary] = useSetPrimaryWalletMutation();
  const [restart, restartState] = useRestartWalletMutation();
  const [rename, renameState] = useRenameWalletMutation();
  const [archive, archiveState] = useArchiveWalletMutation();
  const [restore, restoreState] = useRestoreWalletMutation();
  const [name, setName] = useState(wallet.name);
  const active = wallet.status === 'active';
  const archived = wallet.status === 'archived';
  const canSign = walletCanSign(wallet.readiness);

  return (
    <div className="flex flex-col gap-4 text-[12px]">
      <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
        <Fact label="SOL" value={solText(wallet.sol)} />
        <Fact label="Open positions" value={String(wallet.open_positions)} />
        <Fact label="Rules" value={String(wallet.rule_count)} />
        <Fact
          label="PnL 24h"
          value={`${wallet.pnl_24h_sol >= 0 ? '+' : ''}${solText(wallet.pnl_24h_sol)}`}
          tone={wallet.pnl_24h_sol > 0 ? 'text-green' : wallet.pnl_24h_sol < 0 ? 'text-red' : 'text-text'}
        />
      </div>

      {!archived && (
        <section>
          <div className="mb-1.5 font-medium text-text">Can it trade?</div>
          <ul className="flex flex-col gap-1.5">
            <Check ok={wallet.key_loaded} label="Key loaded on this box">
              {!wallet.key_loaded && (
                <span className={wallet.load_error ? 'text-red' : 'text-text-dim'}>
                  {wallet.load_error ?? 'Run wallet-sync to bring the key file here, then Refresh.'}
                </span>
              )}
            </Check>
            <Check ok={wallet.nonce_count > 0} label={`${wallet.nonce_count} nonce account(s)`}>
              {wallet.nonce_count === 0 && (
                <Button size="xs" variant="primary" onClick={onNonces}>
                  Add nonces
                </Button>
              )}
            </Check>
            <Check ok={!wallet.init_error} label="Trader started">
              {wallet.init_error && (
                <>
                  <span className="text-red">{wallet.init_error}</span>
                  <Button
                    size="xs"
                    variant="ghost"
                    disabled={restartState.isLoading || wallet.nonce_count === 0}
                    onClick={() => run(() => restart(wallet.pubkey).unwrap())}
                  >
                    {restartState.isLoading ? 'Starting...' : 'Retry start'}
                  </Button>
                </>
              )}
            </Check>
            <Check ok={wallet.readiness !== 'needs_sol'} label={`${solText(wallet.sol)} SOL`}>
              {wallet.readiness === 'needs_sol' && (
                <span className="text-text-dim">Low. Send SOL to {shortKey(wallet.pubkey)} from another wallet.</span>
              )}
            </Check>
          </ul>
        </section>
      )}

      <section className="flex flex-col gap-2">
        <div className="font-medium text-text">Actions</div>
        <div className="flex flex-wrap gap-2">
          {active && !wallet.is_primary && (
            <Button
              size="sm"
              variant="ghost"
              disabled={!canSign}
              title={canSign ? undefined : `Cannot sign: ${wallet.readiness_reason}`}
              onClick={() => run(() => setPrimary(wallet.pubkey).unwrap())}
            >
              Set as primary
            </Button>
          )}
          {active && wallet.key_loaded && (
            <Button size="sm" variant="ghost" onClick={onSend}>
              Send SOL
            </Button>
          )}
          {active && wallet.key_loaded && !moveOpen && (
            <Button size="sm" variant="ghost" onClick={onMove}>
              Move everything...
            </Button>
          )}
          {active && !wallet.is_primary && wallet.rule_count === 0 && (
            <Button
              size="sm"
              variant="ghost"
              disabled={archiveState.isLoading}
              onClick={() => {
                if (window.confirm(`Archive ${wallet.name}? It must hold no SOL, token, or nonce account.`)) {
                  void run(() => archive(wallet.pubkey).unwrap());
                }
              }}
            >
              Archive
            </Button>
          )}
          {archived && (
            <Button
              size="sm"
              variant="ghost"
              disabled={restoreState.isLoading}
              onClick={() => run(() => restore(wallet.pubkey).unwrap())}
            >
              Restore
            </Button>
          )}
        </div>
        {!archived && (
          <div className="flex items-end gap-2">
            <label className="flex flex-col gap-1 text-text-dim">
              Name
              <Input value={name} onChange={(e) => setName(e.target.value)} maxLength={32} autoComplete="off" className="w-48" />
            </label>
            <Button
              size="sm"
              variant="ghost"
              disabled={renameState.isLoading || name.trim() === wallet.name || !name.trim()}
              onClick={() => run(() => rename({ pubkey: wallet.pubkey, name: name.trim() }).unwrap())}
            >
              Rename
            </Button>
          </div>
        )}
      </section>
    </div>
  );
}

function Fact({ label, value, tone = 'text-text' }: { label: string; value: string; tone?: string }) {
  return (
    <div className="rounded-md border border-white/5 bg-white/2 px-3 py-2">
      <div className="text-[10px] font-semibold uppercase tracking-wider text-text-dim">{label}</div>
      <div className={`font-mono text-sm ${tone}`}>{value}</div>
    </div>
  );
}

/** Every real rule, checked when this wallet signs it. Unchecking returns it to the primary. */
function RulesTab({ wallet, wallets, run }: { wallet: TraderWalletRow; wallets: TraderWalletRow[]; run: Run }) {
  const { data: rules = [] } = useGetStrategyRulesQuery();
  const { data: assignments = [] } = useGetWalletAssignmentsQuery();
  const [assign, assignState] = useAssignRuleWalletMutation();
  const byRule = useMemo(() => new Map(assignments.map((a) => [a.rule_id, a.wallet])), [assignments]);
  const nameOf = useMemo(() => new Map(wallets.map((w) => [w.pubkey, w.name])), [wallets]);
  const real = rules
    .filter((r) => r.trade_mode === 'real' && r.is_enabled)
    .sort((a, b) => a.rule_name.localeCompare(b.rule_name));
  const canSign = walletCanSign(wallet.readiness) && wallet.status === 'active';

  if (real.length === 0) return <p className="text-[12px] text-text-dim">No real rules.</p>;
  return (
    <div className="flex flex-col gap-1 text-[12px]">
      {!canSign && (
        <InlineAlert variant="warning">
          This wallet cannot sign ({wallet.readiness_reason}). Rules can be taken off it, not put on it.
        </InlineAlert>
      )}
      {real.map((r) => {
        const own = byRule.get(r.id);
        const here = own === wallet.pubkey;
        const where = own ? (nameOf.get(own) ?? shortKey(own)) : 'primary';
        return (
          <label key={r.id} className="flex items-center gap-2 rounded px-1 py-0.5 hover:bg-white/3">
            <Checkbox
              checked={here}
              disabled={assignState.isLoading || (!here && !canSign)}
              onChange={(e) =>
                run(() => assign({ ruleId: r.id, wallet: e.target.checked ? wallet.pubkey : null }).unwrap())
              }
            />
            <span className="text-text">{r.rule_name}</span>
            {r.is_active && <Badge size="sm" variant="success">active</Badge>}
            {!here && <span className="text-text-dim">on {where}</span>}
          </label>
        );
      })}
    </div>
  );
}

function NoncesTab({ wallet, run }: { wallet: TraderWalletRow; run: Run }) {
  const { data: nonces = [], isFetching } = useGetWalletNoncesQuery(wallet.pubkey);
  const [sync, syncState] = useSyncWalletMutation();
  const [provision, provisionState] = useProvisionNoncesMutation();
  const [attach, attachState] = useAttachNoncesMutation();
  const [count, setCount] = useState('5');
  const [pasted, setPasted] = useState('');
  const [report, setReport] = useState<string | null>(null);
  const active = wallet.status === 'active';

  // Finds every nonce account the chain names this wallet the authority of,
  // attaches the new ones, retires the ones it lost, and re-reads the balance.
  const doSync = () =>
    run(async () => {
      const res = await sync(wallet.pubkey).unwrap();
      const note = res.history_error ? `. History not read: ${res.history_error}` : '';
      setReport(`${res.nonces} on chain: attached ${res.attached}, retired ${res.retired}${note}`);
    });

  const doAttach = () =>
    run(async () => {
      const list = pasted.split(/[\s,]+/).filter(Boolean);
      const res = await attach({ pubkey: wallet.pubkey, nonces: list }).unwrap();
      const rejected = res.rejected.map((r) => `${shortKey(r.pubkey)}: ${r.reason}`).join('; ');
      setReport(`Attached ${res.attached}, already ${res.already}${rejected ? `. Rejected ${rejected}` : ''}`);
      setPasted('');
    });

  return (
    <div className="flex flex-col gap-4 text-[12px]">
      <section>
        <div className="mb-1.5 flex items-center gap-2">
          <span className="font-medium text-text">Registered</span>
          <Button size="xs" variant="ghost" onClick={doSync} disabled={isFetching || syncState.isLoading}>
            {syncState.isLoading ? 'Reading chain...' : 'Find on chain'}
          </Button>
        </div>
        {nonces.length === 0 ? (
          <p className="text-text-dim">None. The trader cannot start without at least one.</p>
        ) : (
          <ul className="flex flex-col gap-0.5 font-mono text-[11px]">
            {nonces.map((n) => (
              <li key={n.pubkey} className="flex items-center gap-2">
                <span className={n.ok ? 'text-green' : 'text-red'}>{n.ok ? '✓' : '✗'}</span>
                <span className="text-text">{n.pubkey}</span>
                {n.error && <span className="text-red">{n.error}</span>}
              </li>
            ))}
          </ul>
        )}
      </section>
      {active && wallet.key_loaded && (
        <section className="flex flex-col gap-1.5">
          <div className="font-medium text-text">Create new</div>
          <p className="text-text-dim">Each is a rent-exempt account (about 0.0015 SOL) paid by this wallet. 1 to 16.</p>
          <div className="flex items-center gap-2">
            <Input value={count} onChange={(e) => setCount(e.target.value)} inputMode="numeric" className="w-20" />
            <Button
              size="sm"
              disabled={provisionState.isLoading}
              onClick={() => run(() => provision({ pubkey: wallet.pubkey, count: Number(count) }).unwrap())}
            >
              {provisionState.isLoading ? 'Creating...' : 'Create'}
            </Button>
          </div>
        </section>
      )}
      {active && (
        <section className="flex flex-col gap-1.5">
          <div className="font-medium text-text">Attach ones it already owns</div>
          <p className="text-text-dim">
            Paste nonce addresses, one per line or comma separated. One chain read checks each names this
            wallet as its authority; any other is rejected.
          </p>
          <textarea
            value={pasted}
            onChange={(e) => setPasted(e.target.value)}
            rows={4}
            className="rounded-md border border-white/10 bg-bg-card px-2 py-1 font-mono text-[11px] text-text"
          />
          <div>
            <Button size="sm" disabled={attachState.isLoading || !pasted.trim()} onClick={doAttach}>
              {attachState.isLoading ? 'Checking...' : 'Attach'}
            </Button>
          </div>
          {report && <p className="text-text-mid">{report}</p>}
        </section>
      )}
    </div>
  );
}

function ActivityTab({ wallet }: { wallet: TraderWalletRow }) {
  const { data: rows = [], isLoading } = useGetWalletActivityQuery(wallet.pubkey);
  if (isLoading) return <p className="text-[12px] text-text-dim">Loading...</p>;
  if (rows.length === 0) return <p className="text-[12px] text-text-dim">Nothing recorded yet.</p>;
  return (
    <ul className="flex flex-col gap-1 text-[12px]">
      {rows.map((a) => (
        <li key={a.id} className="flex flex-wrap gap-2">
          <span className="w-36 shrink-0 font-mono text-[11px] text-text-dim">{new Date(a.at).toLocaleString()}</span>
          <span className="text-text">{activityText(a.kind, a.detail)}</span>
          {a.signatures.length > 0 && (
            <span className="font-mono text-[11px] text-text-dim">
              {a.signatures.length} tx · {shortKey(a.signatures[0])}
            </span>
          )}
        </li>
      ))}
    </ul>
  );
}
