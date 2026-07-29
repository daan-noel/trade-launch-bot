// Append-only fill ledger table for a position dialog (entry + every sell leg).
// Per-leg PnL% / hold are derived from the position entry — never stored columns.

import { useMemo } from 'react';
import { formatCompact } from 'utils/format';
import { formatSignedPct, pctGradeClass } from 'lib/signedTone';
import { lamportsToSol } from 'lib/strategy/types';
import type { PositionFill } from 'types';

export interface PositionFillsLedgerProps {
  fills: PositionFill[];
  /** Entry price for per-leg PnL% (from the parent position). */
  entryPrice?: number | null;
  /** Entry time ISO for per-leg hold. */
  entryTime?: string | null;
  /** Entry token amount — for sell_bps of initial bag. */
  entryTokenAmount?: number | null;
  loading?: boolean;
}

function holdSecs(entryTime: string | null | undefined, at: string): number | null {
  if (!entryTime) return null;
  const a = Date.parse(entryTime);
  const b = Date.parse(at);
  if (!Number.isFinite(a) || !Number.isFinite(b)) return null;
  return Math.max(0, Math.floor((b - a) / 1000));
}

function sellBps(fill: PositionFill, entryTokens: number | null | undefined): number | null {
  if (fill.side !== 'sell' || !entryTokens || entryTokens <= 0) return null;
  return Math.min(10_000, Math.floor((fill.token_amount * 10_000) / entryTokens));
}

/**
 * Compact ledger: seq / side / stage / price / pnl% / hold / sol / reason.
 */
export function PositionFillsLedger({
  fills,
  entryPrice,
  entryTime,
  entryTokenAmount,
  loading,
}: PositionFillsLedgerProps) {
  const rows = useMemo(() => fills, [fills]);

  if (loading) {
    return <p className="text-[11px] text-text-dim">loading fills…</p>;
  }
  if (rows.length === 0) {
    return (
      <p className="text-[11px] text-text-dim/60">
        No fill ledger yet (legacy row or entry still in flight).
      </p>
    );
  }

  return (
    <div className="overflow-x-auto rounded border border-white/10">
      <table className="w-full min-w-[32rem] border-collapse text-left text-[11px]">
        <thead>
          <tr className="border-b border-white/10 text-[10px] uppercase tracking-wider text-text-dim/70">
            <th className="px-2 py-1.5 font-semibold">#</th>
            <th className="px-2 py-1.5 font-semibold">Side</th>
            <th className="px-2 py-1.5 font-semibold">Stage</th>
            <th className="px-2 py-1.5 font-semibold">Price</th>
            <th className="px-2 py-1.5 font-semibold">PnL%</th>
            <th className="px-2 py-1.5 font-semibold">Hold</th>
            <th className="px-2 py-1.5 font-semibold">SOL</th>
            <th className="px-2 py-1.5 font-semibold">%</th>
            <th className="px-2 py-1.5 font-semibold">Reason</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((f) => {
            const pnlPct =
              f.side === 'sell' && entryPrice != null && entryPrice > 0
                ? ((f.price - entryPrice) / entryPrice) * 100
                : null;
            const hold = holdSecs(entryTime, f.at);
            const bps = sellBps(f, entryTokenAmount);
            const sol = lamportsToSol(f.sol_lamports);
            return (
              <tr key={`${f.position_id}-${f.seq}`} className="border-b border-white/5 last:border-0">
                <td className="px-2 py-1 tabular-nums text-text-dim">{f.seq}</td>
                <td className="px-2 py-1 font-semibold capitalize">{f.side}</td>
                <td className="px-2 py-1 tabular-nums text-text-dim">
                  {f.stage != null ? f.stage : '—'}
                </td>
                <td className="px-2 py-1 tabular-nums">{formatCompact(f.price, 6)}</td>
                <td className="px-2 py-1 tabular-nums">
                  {pnlPct != null ? (
                    <span className={pctGradeClass(pnlPct)}>{formatSignedPct(pnlPct, 1)}</span>
                  ) : (
                    '—'
                  )}
                </td>
                <td className="px-2 py-1 tabular-nums text-text-dim">
                  {hold != null ? `${hold}s` : '—'}
                </td>
                <td className="px-2 py-1 tabular-nums">
                  {sol != null ? formatCompact(sol, 4) : '—'}
                </td>
                <td className="px-2 py-1 tabular-nums text-text-dim">
                  {bps != null ? `${Math.round(bps / 100)}%` : '—'}
                </td>
                <td className="px-2 py-1 text-text-dim">{f.reason ?? '—'}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

/** Map sell fills → inspect `exitLegs` (one chart marker per leg). */
export function fillsToExitLegs(
  fills: PositionFill[],
  entryTokenAmount?: number | null,
): Array<{
  time: string;
  price: number;
  tx?: string | null;
  sellBps?: number;
  reason?: string | null;
}> {
  return fills
    .filter((f) => f.side === 'sell')
    .map((f) => ({
      time: f.at,
      price: f.price,
      tx: f.tx_signature,
      sellBps: sellBps(f, entryTokenAmount) ?? undefined,
      reason: f.reason,
    }));
}
