import { useState } from 'react';

import { Button } from 'components/ui/Button';
import { Checkbox } from 'components/ui/Checkbox';
import { Input } from 'components/ui/Input';
import { InlineAlert, Modal } from 'components/ui/Modal';
import { Select } from 'components/ui/Select';
import { apiErrorMessage } from 'store/baseApi';
import {
  useBeginWalletMoveMutation,
  useCreateTraderWalletMutation,
  useGetMovePreviewQuery,
  useSendWalletSolMutation,
  type TraderWalletRow,
} from '@live/store/liveEndpoints';

import { lamportsToSol, shortKey, solText } from './walletFormat';

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
