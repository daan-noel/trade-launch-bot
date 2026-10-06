import { useCallback, useState } from 'react';
import type { FetchBaseQueryError } from '@reduxjs/toolkit/query';
import type { SerializedError } from '@reduxjs/toolkit';
import { Badge } from 'components/ui/Badge';
import { Button } from 'components/ui/Button';
import { InlineAlert, Modal } from 'components/ui/Modal';
import { apiErrorMessage } from 'store/apiSlice';
import { useGetRentStatusQuery, useRecoverRentMutation } from '@live/store/liveEndpoints';
import type { RentRecoverResult } from 'types';

const LAMPORTS_PER_SOL = 1e9;

const fmtSol = (lamports: number): string =>
  (lamports / LAMPORTS_PER_SOL).toLocaleString(undefined, {
    minimumFractionDigits: 4,
    maximumFractionDigits: 6,
  });

function resultMessage(res: RentRecoverResult): string {
  const parts = [`Closed ${res.closed} account${res.closed === 1 ? '' : 's'}`];
  if (res.lamports_returned > 0) parts.push(`${fmtSol(res.lamports_returned)} SOL returned`);
  if (res.still_open > 0) parts.push(`${res.still_open} still open`);
  if (res.skipped_inflight > 0) parts.push(`${res.skipped_inflight} left because an exit is in flight`);
  return `${parts.join(', ')}.`;
}

/** Rent still locked in token accounts the exit did not close. Empty accounts
 *  and wrapped SOL recover with a close. Dust is a separate burn: the tokens
 *  are destroyed. Open positions are left alone. */
export function RentCard({ onRecovered }: { onRecovered?: () => void }) {
  const { data, isLoading, isFetching, error, refetch } = useGetRentStatusQuery();
  const [recover, { isLoading: isRecovering }] = useRecoverRentMutation();
  const [burnOpen, setBurnOpen] = useState(false);
  const [mode, setMode] = useState<'close' | 'burn' | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);

  const empty = data?.empty.accounts ?? 0;
  const wrapped = data?.wrapped.accounts ?? 0;
  const dust = data?.dust.accounts ?? 0;
  const recoverable =
    (data?.empty.lamports ?? 0) + (data?.wrapped.lamports ?? 0) + (data?.dust.lamports ?? 0);
  const closeableLamports = (data?.empty.lamports ?? 0) + (data?.wrapped.lamports ?? 0);

  const run = useCallback(
    async (burnDust: boolean) => {
      setMode(burnDust ? 'burn' : 'close');
      setActionError(null);
      setSuccess(null);
      setBurnOpen(false);
      try {
        const res = await recover({ burn_dust: burnDust }).unwrap();
        setSuccess(resultMessage(res));
        if (res.error_count > 0) {
          const more = res.error_count > res.errors.length ? ` (+${res.error_count - res.errors.length} more)` : '';
          setActionError(`${res.errors.join('; ')}${more}`);
        }
        onRecovered?.();
      } catch (e) {
        setActionError(
          `Recover failed: ${apiErrorMessage(e as FetchBaseQueryError | SerializedError) ?? 'unknown error'}`,
        );
      } finally {
        setMode(null);
      }
    },
    [recover, onRecovered],
  );

  const loadError = error ? apiErrorMessage(error, 'Failed to read token accounts') : null;
  const busy = isRecovering || mode != null;

  return (
    <div className="mb-4 rounded-lg border border-border bg-bg-panel/50 p-4">
      <div className="flex flex-wrap items-center gap-3">
        <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">
          Account rent
        </span>
        <span className="font-mono text-lg font-extrabold text-text">
          {isLoading ? '—' : `${fmtSol(recoverable)} SOL`}
        </span>
        <Badge variant={empty > 0 ? 'primary' : 'neutral'} className="font-mono">
          Empty {empty}
          {data && empty > 0
            ? ` · ${data.empty_token2022} Token-2022 · ${data.empty_legacy} legacy`
            : ''}
        </Badge>
        {wrapped > 0 && (
          <Badge variant="primary" className="font-mono">
            Wrapped {wrapped}: {fmtSol(data?.wrapped.lamports ?? 0)}
          </Badge>
        )}
        {dust > 0 && (
          <Badge variant="primary" className="font-mono">
            Dust {dust}: {fmtSol(data?.dust.lamports ?? 0)}
          </Badge>
        )}
        <Button variant="subtle" size="sm" onClick={() => refetch()} disabled={isFetching || busy}>
          {isFetching ? 'Loading…' : '↻'}
        </Button>
        <div className="flex-grow" />
        <Button
          variant="primary"
          size="sm"
          onClick={() => run(false)}
          disabled={busy || isFetching || empty + wrapped === 0}
        >
          {mode === 'close'
            ? 'Recovering…'
            : closeableLamports > 0
              ? `Recover ${fmtSol(closeableLamports)}`
              : 'Recover'}
        </Button>
        <Button
          variant="subtle"
          size="sm"
          onClick={() => setBurnOpen(true)}
          disabled={busy || isFetching || dust === 0}
        >
          {mode === 'burn' ? 'Burning…' : 'Burn dust'}
        </Button>
      </div>
      {data && (data.blocked.accounts > 0 || data.open_position.accounts > 0 || data.multi_account_mints > 0) && (
        <p className="mt-2 text-[11px] text-text-dim">
          {data.blocked.accounts > 0 && (
            <span className="mr-3">
              Blocked {data.blocked.accounts} ({fmtSol(data.blocked.lamports)} SOL, frozen or fees withheld)
            </span>
          )}
          {data.open_position.accounts > 0 && (
            <span className="mr-3">
              Open positions {data.open_position.accounts} ({fmtSol(data.open_position.lamports)} SOL, left alone)
            </span>
          )}
          {data.multi_account_mints > 0 && (
            <span>
              {data.multi_account_mints} mint{data.multi_account_mints === 1 ? '' : 's'} with more than one account
            </span>
          )}
        </p>
      )}
      {(loadError || actionError) && (
        <div className="mt-3">
          <InlineAlert variant="error">{actionError ?? loadError}</InlineAlert>
        </div>
      )}
      {success && (
        <div className="mt-3">
          <InlineAlert variant="success">{success}</InlineAlert>
        </div>
      )}
      <Modal title="Burn dust and recover rent" open={burnOpen} onClose={() => setBurnOpen(false)}>
        <p className="mb-4 text-xs text-text-mid">
          This burns the tokens in {dust} account{dust === 1 ? '' : 's'} (balance at or under{' '}
          {data?.dust_raw_max ?? 0} raw units) and closes them, returning about{' '}
          {fmtSol(data?.dust.lamports ?? 0)} SOL of rent. USDC and any account on an open position
          are not touched.
        </p>
        <div className="flex items-center justify-end gap-2.5">
          <Button variant="ghost" onClick={() => setBurnOpen(false)}>
            Cancel
          </Button>
          <Button variant="danger" size="sm" onClick={() => run(true)} disabled={busy}>
            Burn and recover
          </Button>
        </div>
      </Modal>
    </div>
  );
}
