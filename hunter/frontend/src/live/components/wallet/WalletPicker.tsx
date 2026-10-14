import { useCallback } from 'react';
import { useSearchParams } from 'react-router-dom';

import { Select } from 'components/ui/Select';
import { useGetTraderWalletsQuery } from '@live/store/liveEndpoints';

/**
 * The wallet a page reads and trades for, kept in `?wallet=`. Empty = the
 * primary, so a page without the param behaves as it did with one wallet.
 */
export function useSelectedWallet(): [string | undefined, (wallet: string) => void] {
  const [params, setParams] = useSearchParams();
  const wallet = params.get('wallet') || undefined;
  const setWallet = useCallback(
    (next: string) => {
      setParams(
        (prev) => {
          const out = new URLSearchParams(prev);
          if (next) out.set('wallet', next);
          else out.delete('wallet');
          return out;
        },
        { replace: true },
      );
    },
    [setParams],
  );
  return [wallet, setWallet];
}

/** Active wallets loaded on this box. Hidden while there is only one. */
export function WalletPicker({
  value,
  onChange,
}: {
  value: string | undefined;
  onChange: (wallet: string) => void;
}) {
  const { data } = useGetTraderWalletsQuery();
  const wallets = (data?.wallets ?? []).filter((w) => w.status === 'active' && w.key_loaded);
  if (wallets.length < 2) return null;
  const primary = wallets.find((w) => w.is_primary);
  return (
    <Select
      fieldSize="sm"
      className="w-44"
      value={value ?? ''}
      onChange={(e) => onChange(e.target.value)}
      aria-label="Wallet"
      title="Wallet"
    >
      <option value="">{primary ? `primary (${primary.name})` : 'primary'}</option>
      {wallets
        .filter((w) => !w.is_primary)
        .map((w) => (
          <option key={w.pubkey} value={w.pubkey}>
            {w.name}
          </option>
        ))}
    </Select>
  );
}
