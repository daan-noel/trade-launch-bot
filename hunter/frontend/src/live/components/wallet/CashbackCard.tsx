import { useCallback, useState } from 'react';
import type { FetchBaseQueryError } from '@reduxjs/toolkit/query';
import type { SerializedError } from '@reduxjs/toolkit';
import { Badge } from 'components/ui/Badge';
import { Button } from 'components/ui/Button';
import { InlineAlert } from 'components/ui/Modal';
import { apiErrorMessage } from 'store/apiSlice';
import {
  useGetCashbackStatusQuery,
  useClaimCashbackMutation,
} from '@live/store/liveEndpoints';

const LAMPORTS_PER_SOL = 1e9;

const fmtSol = (lamports: number): string =>
  (lamports / LAMPORTS_PER_SOL).toLocaleString(undefined, {
    minimumFractionDigits: 4,
    maximumFractionDigits: 6,
  });

/** Accrued pump.fun cashback + a one-click reclaim. The status is a read-only
 *  on-chain read (cached, not polled); claim sweeps both pots to native SOL and
 *  refreshes the figure. Self-contained so it can drop onto any wallet view. */
export function CashbackCard() {
  const { data, isLoading, isFetching, refetch } = useGetCashbackStatusQuery();
  const [claim, { isLoading: isClaiming }] = useClaimCashbackMutation();
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);

  const total = data?.total_claimable_lamports ?? 0;
  const hasClaimable = total > 0;

  const handleClaim = useCallback(async () => {
    setError(null);
    setSuccess(null);
    try {
      const res = await claim().unwrap();
      const failed = res.pots.filter((p) => p.error);
      const parts: string[] = [];
      if (res.claimed_lamports > 0) parts.push(`${fmtSol(res.claimed_lamports)} SOL`);
      // Stable cashback is a separate SPL token (raw units, not lamports) and
      // lands as a token balance rather than native SOL.
      if (res.claimed_stable > 0) parts.push(`${res.claimed_stable} stable units`);
      if (parts.length > 0) {
        setSuccess(`Reclaimed ${parts.join(' + ')} to your wallet.`);
      } else if (failed.length === 0) {
        setSuccess('Nothing to reclaim right now.');
      }
      if (failed.length > 0) {
        setError(`Some pots failed: ${failed.map((p) => `${p.label} (${p.error})`).join(', ')}`);
      }
    } catch (e) {
      setError(
        `Reclaim failed: ${apiErrorMessage(e as FetchBaseQueryError | SerializedError) ?? 'unknown error'}`,
      );
    }
  }, [claim]);

  return (
    <div className="mb-4 rounded-lg border border-border bg-bg-panel/50 p-4">
      <div className="flex flex-wrap items-center gap-3">
        <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">
          Cashback
        </span>
        <span className="font-mono text-lg font-extrabold text-text">
          {isLoading ? '—' : `${fmtSol(total)} SOL`}
        </span>
        {data?.pots.map((p) => (
          <Badge key={p.label} variant={p.claimable_lamports > 0 ? 'primary' : 'neutral'} className="font-mono">
            {p.label}: {fmtSol(p.claimable_lamports)}
          </Badge>
        ))}
        <Button variant="subtle" size="sm" onClick={() => refetch()} disabled={isFetching || isClaiming}>
          {isFetching ? 'Loading…' : '↻'}
        </Button>
        <div className="flex-grow" />
        <Button
          variant="primary"
          size="sm"
          onClick={handleClaim}
          disabled={!hasClaimable || isClaiming || isFetching}
        >
          {isClaiming ? 'Reclaiming…' : 'Reclaim'}
        </Button>
      </div>
      {error && (
        <div className="mt-3">
          <InlineAlert variant="error">{error}</InlineAlert>
        </div>
      )}
      {success && (
        <div className="mt-3">
          <InlineAlert variant="success">{success}</InlineAlert>
        </div>
      )}
    </div>
  );
}
