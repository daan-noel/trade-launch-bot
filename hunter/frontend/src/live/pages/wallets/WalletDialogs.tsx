import { useEffect, useState } from 'react';

import { Button } from 'components/ui/Button';
import { Checkbox } from 'components/ui/Checkbox';
import { Input } from 'components/ui/Input';
import { InlineAlert, Modal } from 'components/ui/Modal';
import { Select } from 'components/ui/Select';
import { apiErrorMessage } from 'store/baseApi';
import {
  useBeginWalletMoveMutation,
  useCreateTraderWalletMutation,
  useFundWalletsMutation,
  useGetMovePreviewQuery,
  useRevealWalletKeyMutation,
  useSendWalletSolMutation,
  useSweepWalletsMutation,
  type TraderWalletRow,
  type WalletSweepResult,
} from '@live/store/liveEndpoints';

import { lamportsToSol, shortKey, solText, walletTab } from './walletFormat';

const NEW_WALLET = '__new__';

/**
 * Move everything from one wallet to another: rules, the primary flag, token
 * bags, nonce authority, then spendable SOL. The preview is read before the
 * operator confirms; the destination may be created in the same dialog.
 */
export function MoveDialog({
  source,
  wallets,
  onClose,
}: {
  source: TraderWalletRow;
  wallets: TraderWalletRow[];
  onClose: () => void;
}) {
  const targets = wallets.filter(
    (w) => w.pubkey !== source.pubkey && w.status === 'active' && w.key_loaded,
  );
  const [dest, setDest] = useState(targets[0]?.pubkey ?? NEW_WALLET);
  const [newName, setNewName] = useState('');
  const [confirm, setConfirm] = useState('');
  const [error, setError] = useState<string | null>(null);
  const preview = useGetMovePreviewQuery(source.pubkey, { refetchOnMountOrArgChange: true });
  const [createWallet, createState] = useCreateTraderWalletMutation();
  const [beginMove, beginState] = useBeginWalletMoveMutation();
  const busy = createState.isLoading || beginState.isLoading;
  const p = preview.data;

  const start = async () => {
    setError(null);
    try {
      let to = dest;
      if (dest === NEW_WALLET) {
        const made = await createWallet({ name: newName.trim() }).unwrap();
        to = made.pubkey;
      }
      await beginMove({ from: source.pubkey, to }).unwrap();
      onClose();
    } catch (e) {
      setError(apiErrorMessage(e as never) ?? 'Move failed to start');
    }
  };

  const destReady = dest !== NEW_WALLET || newName.trim().length > 0;
  return (
    <Modal title={`Move ${source.name}`} open onClose={onClose}>
      <div className="flex flex-col gap-3 text-[12px]">
        <section className="rounded-md border border-white/10 bg-white/3 p-3">
          <div className="mb-1 font-medium text-text">What moves</div>
          {preview.isLoading && <p className="text-text-dim">Reading the wallet...</p>}
          {preview.error && (
            <p className="text-red">{apiErrorMessage(preview.error) ?? 'Could not read the wallet'}</p>
          )}
          {p && (
            <ul className="flex flex-col gap-0.5 text-text-mid">
              <li>
                {p.rules.length} rule(s)
                {p.rules.length > 0 && `: ${p.rules.map((r) => r.rule_name).join(', ')}`}
              </li>
              {p.primary && <li>The primary flag</li>}
              <li>
                {p.tokens.length} token balance(s), {p.open_positions} open position(s)
              </li>
              <li>{p.nonces} nonce account(s), same addresses</li>
              <li>{solText(lamportsToSol(p.sol_lamports))} SOL, less a small fee reserve</li>
              {p.in_flight > 0 && (
                <li className="text-warning">
                  {p.in_flight} position(s) buying or selling now; the move waits for them
                </li>
              )}
            </ul>
          )}
        </section>
        <label className="flex flex-col gap-1 text-text-dim">
          Destination
          <Select value={dest} onChange={(e) => setDest(e.target.value)}>
            {targets.map((w) => (
              <option key={w.pubkey} value={w.pubkey}>
                {w.name} ({shortKey(w.pubkey)})
              </option>
            ))}
            <option value={NEW_WALLET}>A new wallet</option>
          </Select>
        </label>
        {dest === NEW_WALLET && (
          <label className="flex flex-col gap-1 text-text-dim">
            New wallet name
            <Input value={newName} onChange={(e) => setNewName(e.target.value)} placeholder="desk-3" autoComplete="off" />
          </label>
        )}
        <p className="text-text-dim">
          The server keeps advancing the move, across waits and restarts. The old wallet is
          archived only after it holds no position, token, or nonce account.
        </p>
        <label className="flex flex-col gap-1 text-text-dim">
          Type {source.name} to confirm
          <Input value={confirm} onChange={(e) => setConfirm(e.target.value)} autoComplete="off" />
        </label>
        {error && <InlineAlert variant="error">{error}</InlineAlert>}
        <div className="flex justify-end gap-2">
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button disabled={busy || !destReady || confirm !== source.name || !p} onClick={start}>
            {busy ? 'Starting...' : 'Start move'}
          </Button>
        </div>
      </div>
    </Modal>
  );
}

/** Send SOL from a wallet: to another managed wallet, or outside with a typed confirm. */
export function SendSolDialog({
  source,
  wallets,
  onClose,
}: {
  source: TraderWalletRow;
  wallets: TraderWalletRow[];
  onClose: () => void;
}) {
  const targets = wallets.filter((w) => w.pubkey !== source.pubkey && w.status !== 'archived');
  const [external, setExternal] = useState(false);
  const [to, setTo] = useState(targets[0]?.pubkey ?? '');
  const [outside, setOutside] = useState('');
  const [sol, setSol] = useState('');
  const [confirm, setConfirm] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState<string | null>(null);
  const [send, sendState] = useSendWalletSolMutation();

  const amount = Number(sol);
  const dest = external ? outside.trim() : to;
  const valid = Number.isFinite(amount) && amount > 0 && dest.length > 0 && (!external || confirm === source.name);

  const submit = async () => {
    setError(null);
    setDone(null);
    try {
      const res = await send({ pubkey: source.pubkey, to: dest, sol: amount, external }).unwrap();
      setDone(res.signature);
    } catch (e) {
      setError(apiErrorMessage(e as never) ?? 'Send failed');
    }
  };

  return (
    <Modal title={`Send SOL from ${source.name}`} open onClose={onClose}>
      <div className="flex flex-col gap-3 text-[12px]">
        <p className="text-text-dim">
          Balance {solText(source.sol)} SOL. SOL committed to a buy in flight and a fee reserve stay.
        </p>
        <label className="flex items-center gap-2 text-text-dim">
          <Checkbox checked={external} onChange={(e) => setExternal(e.target.checked)} />
          Send to an address outside these wallets
        </label>
        {external ? (
          <label className="flex flex-col gap-1 text-text-dim">
            Address
            <Input value={outside} onChange={(e) => setOutside(e.target.value)} autoComplete="off" />
          </label>
        ) : (
          <label className="flex flex-col gap-1 text-text-dim">
            To
            <Select value={to} onChange={(e) => setTo(e.target.value)}>
              {targets.map((w) => (
                <option key={w.pubkey} value={w.pubkey}>
                  {w.name} ({solText(w.sol)} SOL)
                </option>
              ))}
            </Select>
          </label>
        )}
        <label className="flex flex-col gap-1 text-text-dim">
          SOL
          <Input value={sol} onChange={(e) => setSol(e.target.value)} inputMode="decimal" placeholder="0.5" />
        </label>
        {external && (
          <label className="flex flex-col gap-1 text-text-dim">
            Type {source.name} to confirm an outside send
            <Input value={confirm} onChange={(e) => setConfirm(e.target.value)} autoComplete="off" />
          </label>
        )}
        {error && <InlineAlert variant="error">{error}</InlineAlert>}
        {done && <InlineAlert variant="success">Sent. Signature {shortKey(done)}</InlineAlert>}
        <div className="flex justify-end gap-2">
          <Button variant="ghost" onClick={onClose}>Close</Button>
          <Button disabled={!valid || sendState.isLoading} onClick={submit}>
            {sendState.isLoading ? 'Sending...' : 'Send'}
          </Button>
        </div>
      </div>
    </Modal>
  );
}

/** How long a revealed key stays on screen. */
const REVEAL_SHOW_MS = 60_000;

/**
 * Show one wallet's secret key after `WALLET_KEK` is typed again. The key lives
 * in this dialog's state only: cleared on close and after a minute.
 */
export function RevealKeyDialog({ wallet, onClose }: { wallet: TraderWalletRow; onClose: () => void }) {
  const [reveal, revealState] = useRevealWalletKeyMutation();
  const [passphrase, setPassphrase] = useState('');
  const [key, setKey] = useState<{ base58: string; json: string } | null>(null);
  const [visible, setVisible] = useState(false);
  const [copied, setCopied] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const busy = revealState.isLoading;

  useEffect(() => {
    if (!key) return;
    const t = setTimeout(() => setKey(null), REVEAL_SHOW_MS);
    return () => clearTimeout(t);
  }, [key]);

  const submit = async () => {
    setError(null);
    try {
      const res = await reveal({ pubkey: wallet.pubkey, passphrase }).unwrap();
      setKey({ base58: res.secret_base58, json: JSON.stringify(res.secret_bytes) });
      setVisible(false);
      setCopied(null);
    } catch (e) {
      setError(apiErrorMessage(e as never) ?? 'Reveal failed');
    } finally {
      setPassphrase('');
      // The reply would otherwise stay in the store as this mutation's data.
      revealState.reset();
    }
  };

  const copy = (label: string, text: string) => {
    void navigator.clipboard.writeText(text);
    setCopied(label);
  };

  return (
    <Modal title={`Private key of ${wallet.name}`} open onClose={onClose}>
      <div className="flex flex-col gap-3 text-[12px]">
        <InlineAlert variant="warning">
          Anyone with this key controls the wallet and everything in it. Each reveal is recorded in the
          wallet activity.
        </InlineAlert>
        {!key ? (
          <>
            <label className="flex flex-col gap-1 text-text-dim">
              WALLET_KEK passphrase
              <Input
                type="password"
                value={passphrase}
                onChange={(e) => setPassphrase(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' && passphrase && !busy) void submit();
                }}
                autoComplete="off"
              />
            </label>
            {error && <InlineAlert variant="error">{error}</InlineAlert>}
            <div className="flex justify-end gap-2">
              <Button variant="ghost" onClick={onClose}>Cancel</Button>
              <Button disabled={!passphrase || busy} onClick={submit}>
                {busy ? 'Decrypting...' : 'Reveal'}
              </Button>
            </div>
          </>
        ) : (
          <>
            <div className="break-all rounded-md border border-white/10 bg-bg-card px-2 py-1.5 font-mono text-[11px] text-text">
              {visible ? key.base58 : '*'.repeat(64)}
            </div>
            <p className="text-text-dim">
              Base58 imports into Phantom and Solflare; the byte array is a Solana CLI keypair file. Hidden
              again after a minute.
            </p>
            <div className="flex flex-wrap justify-end gap-2">
              {copied && <span className="self-center text-green">Copied {copied}</span>}
              <Button variant="ghost" onClick={() => setVisible((v) => !v)}>{visible ? 'Hide' : 'Show'}</Button>
              <Button variant="ghost" onClick={() => copy('byte array', key.json)}>Copy byte array</Button>
              <Button onClick={() => copy('base58', key.base58)}>Copy base58</Button>
            </div>
          </>
        )}
      </div>
    </Modal>
  );
}

/** Pay several wallets from one, in a single transaction. */
export function FundDialog({ wallets, onClose }: { wallets: TraderWalletRow[]; onClose: () => void }) {
  const sources = wallets.filter((w) => w.status === 'active' && w.key_loaded);
  const [from, setFrom] = useState(sources.find((w) => w.is_primary)?.pubkey ?? sources[0]?.pubkey ?? '');
  const [amounts, setAmounts] = useState<Record<string, string>>({});
  const [each, setEach] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState<string | null>(null);
  const [fund, fundState] = useFundWalletsMutation();
  const source = wallets.find((w) => w.pubkey === from);
  const targets = wallets.filter((w) => w.pubkey !== from && w.status !== 'archived');
  const picked = targets
    .map((w) => ({ to: w.pubkey, sol: Number(amounts[w.pubkey] ?? '') }))
    .filter((t) => Number.isFinite(t.sol) && t.sol > 0);
  const total = picked.reduce((s, t) => s + t.sol, 0);

  const submit = async () => {
    setError(null);
    setDone(null);
    try {
      const res = await fund({ from, targets: picked }).unwrap();
      setDone(res.signature);
      setAmounts({});
    } catch (e) {
      setError(apiErrorMessage(e as never) ?? 'Fund failed');
    }
  };

  return (
    <Modal title="Fund wallets" open onClose={onClose}>
      <div className="flex flex-col gap-3 text-[12px]">
        <label className="flex flex-col gap-1 text-text-dim">
          From
          <Select value={from} onChange={(e) => setFrom(e.target.value)}>
            {sources.map((w) => (
              <option key={w.pubkey} value={w.pubkey}>
                {w.name} ({solText(w.sol)} SOL)
              </option>
            ))}
          </Select>
        </label>
        <div className="flex items-end gap-2">
          <label className="flex flex-col gap-1 text-text-dim">
            Same amount for each
            <Input value={each} onChange={(e) => setEach(e.target.value)} inputMode="decimal" placeholder="0.2" className="w-28" />
          </label>
          <Button size="sm" variant="ghost" onClick={() => setAmounts(Object.fromEntries(targets.map((w) => [w.pubkey, each])))}>
            Fill all
          </Button>
          <Button size="sm" variant="ghost" onClick={() => setAmounts({})}>
            Clear
          </Button>
        </div>
        <ul className="flex flex-col gap-1">
          {targets.map((w) => (
            <li key={w.pubkey} className="flex items-center gap-2">
              <span className="flex-1 text-text">{w.name}</span>
              <span className="w-24 text-right font-mono text-text-dim">{solText(w.sol)}</span>
              <Input
                value={amounts[w.pubkey] ?? ''}
                onChange={(e) => setAmounts((a) => ({ ...a, [w.pubkey]: e.target.value }))}
                inputMode="decimal"
                placeholder="0"
                className="w-24"
              />
            </li>
          ))}
        </ul>
        <p className="text-text-dim">
          {picked.length} wallet(s), {solText(total)} SOL from {source?.name ?? '-'} ({solText(source?.sol)} SOL). One
          transaction, up to 16 wallets.
        </p>
        {error && <InlineAlert variant="error">{error}</InlineAlert>}
        {done && <InlineAlert variant="success">Sent. Signature {shortKey(done)}</InlineAlert>}
        <div className="flex justify-end gap-2">
          <Button variant="ghost" onClick={onClose}>Close</Button>
          <Button disabled={!from || picked.length === 0 || fundState.isLoading} onClick={submit}>
            {fundState.isLoading ? 'Sending...' : 'Fund'}
          </Button>
        </div>
      </div>
    </Modal>
  );
}

/** Send spare SOL from several wallets to one. Idle wallets start picked. */
export function SweepDialog({ wallets, onClose }: { wallets: TraderWalletRow[]; onClose: () => void }) {
  const dests = wallets.filter((w) => w.status !== 'archived');
  const [to, setTo] = useState(dests.find((w) => w.is_primary)?.pubkey ?? dests[0]?.pubkey ?? '');
  const sources = wallets.filter((w) => w.pubkey !== to && w.status === 'active' && w.key_loaded);
  const [picked, setPicked] = useState<Set<string>>(
    () => new Set(wallets.filter((w) => walletTab(w) === 'idle' && w.key_loaded).map((w) => w.pubkey)),
  );
  const [keep, setKeep] = useState('0');
  const [results, setResults] = useState<WalletSweepResult[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [sweep, sweepState] = useSweepWalletsMutation();
  const chosen = sources.filter((w) => picked.has(w.pubkey));
  const keepSol = Number(keep);
  const nameOf = (pk: string) => wallets.find((w) => w.pubkey === pk)?.name ?? shortKey(pk);

  const toggle = (pk: string, on: boolean) =>
    setPicked((prev) => {
      const next = new Set(prev);
      if (on) next.add(pk);
      else next.delete(pk);
      return next;
    });

  const submit = async () => {
    setError(null);
    setResults(null);
    try {
      setResults(await sweep({ wallets: chosen.map((w) => w.pubkey), to, keep_sol: keepSol }).unwrap());
    } catch (e) {
      setError(apiErrorMessage(e as never) ?? 'Sweep failed');
    }
  };

  return (
    <Modal title="Sweep SOL" open onClose={onClose}>
      <div className="flex flex-col gap-3 text-[12px]">
        <label className="flex flex-col gap-1 text-text-dim">
          To
          <Select value={to} onChange={(e) => setTo(e.target.value)}>
            {dests.map((w) => (
              <option key={w.pubkey} value={w.pubkey}>
                {w.name} ({solText(w.sol)} SOL)
              </option>
            ))}
          </Select>
        </label>
        <ul className="flex flex-col gap-1">
          {sources.map((w) => (
            <li key={w.pubkey}>
              <label className="flex items-center gap-2">
                <Checkbox checked={picked.has(w.pubkey)} onChange={(e) => toggle(w.pubkey, e.target.checked)} />
                <span className="flex-1 text-text">{w.name}</span>
                {w.open_positions > 0 && <span className="text-text-dim">{w.open_positions} open</span>}
                <span className="w-24 text-right font-mono text-text-dim">{solText(w.sol)}</span>
              </label>
            </li>
          ))}
        </ul>
        <label className="flex flex-col gap-1 text-text-dim">
          SOL each wallet keeps
          <Input value={keep} onChange={(e) => setKeep(e.target.value)} inputMode="decimal" className="w-28" />
        </label>
        <p className="text-text-dim">
          SOL committed to a buy in flight and a fee reserve stay. A wallet with open positions keeps at least
          0.01 SOL for its sells. One transaction per wallet.
        </p>
        {error && <InlineAlert variant="error">{error}</InlineAlert>}
        {results && (
          <ul className="flex flex-col gap-0.5">
            {results.map((r) => (
              <li key={r.wallet} className={r.error ? 'text-red' : 'text-text-mid'}>
                {nameOf(r.wallet)}:{' '}
                {r.error ?? (r.signature ? `sent ${solText(lamportsToSol(r.lamports))} SOL` : 'nothing to send')}
              </li>
            ))}
          </ul>
        )}
        <div className="flex justify-end gap-2">
          <Button variant="ghost" onClick={onClose}>Close</Button>
          <Button
            disabled={!to || chosen.length === 0 || !Number.isFinite(keepSol) || keepSol < 0 || sweepState.isLoading}
            onClick={submit}
          >
            {sweepState.isLoading ? 'Sweeping...' : `Sweep ${chosen.length} wallet(s)`}
          </Button>
        </div>
      </div>
    </Modal>
  );
}
