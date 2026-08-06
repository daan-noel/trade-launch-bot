// Append-only fill ledger table for a position dialog (entry + every sell leg).
// Per-leg PnL% / hold are derived from the position entry — never stored columns.

import { useMemo } from 'react';
import { Badge } from 'components/ui/Badge';
import { formatCompact } from 'utils/format';
import { formatSignedPct, pctGradeClass } from 'lib/signedTone';
import { lamportsToSol, solToLamports } from 'lib/strategy/types';
import type { PositionFill } from 'types';
import type { InspectTarget } from 'components/strategy/inspectTarget';

export interface PositionFillsLedgerProps {
  fills: PositionFill[];
  /** Entry price for per-leg PnL% (from the parent position). */
  entryPrice?: number | null;
  /** Entry time ISO for per-leg hold. */
  entryTime?: string | null;
  /** Entry token amount — for sell_bps of initial bag. */
  entryTokenAmount?: number | null;
  loading?: boolean;
  /** True when rows were reconstructed from `strategy_positions` (no ledger). */
  reconstructed?: boolean;
}

/** Inputs for reconstructing a display-only ledger when `position_fills` is empty. */
export interface LegacyFillFacts {
  positionId: string;
  entrySol?: number | null;
  entryTokenAmount?: number | null;
  exitSol?: number | null;
  exitTokenAmount?: number | null;
  exitReason?: string | null;
  pnlSol?: number | null;
  inspect: InspectTarget;
}

/**
 * Build buy/sell display rows from the same entry/exit fields the chart markers
 * use. Used when the durable `position_fills` ledger is empty (pre-mig-0018
 * rows, or a confirm that never wrote the ledger). Not persisted — display only.
 */
export function fillsFromPositionFacts(facts: LegacyFillFacts): PositionFill[] {
  const { inspect, positionId } = facts;
  const out: PositionFill[] = [];
  let seq = 1;

  if (inspect.entryTime && inspect.entryPrice != null) {
    const entrySol =
      facts.entrySol ??
      (facts.entryTokenAmount != null
        ? inspect.entryPrice * facts.entryTokenAmount
        : null);
    out.push({
      position_id: positionId,
      seq: seq++,
      side: 'buy',
      price: inspect.entryPrice,
      sol_lamports: solToLamports(entrySol) ?? 0,
      token_amount: facts.entryTokenAmount ?? 0,
      at: inspect.entryTime,
      reason: null,
      stage: 0,
      tx_signature: inspect.entryTx ?? null,
    });
  }

  if (inspect.exitTime && inspect.exitPrice != null) {
    const exitSol =
      facts.exitSol ??
      (facts.exitTokenAmount != null
        ? inspect.exitPrice * facts.exitTokenAmount
        : facts.entrySol != null && facts.pnlSol != null
          ? facts.entrySol + facts.pnlSol
          : null);
    const exitTokens =
      facts.exitTokenAmount ?? facts.entryTokenAmount ?? 0;
    out.push({
      position_id: positionId,
      seq: seq++,
      side: 'sell',
      price: inspect.exitPrice,
      sol_lamports: solToLamports(exitSol) ?? 0,
      token_amount: exitTokens,
      at: inspect.exitTime,
      reason: facts.exitReason ?? inspect.exitLabel ?? null,
      stage: 0,
      tx_signature: inspect.exitTx ?? null,
    });
  }

  return out;
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
  reconstructed,
}: PositionFillsLedgerProps) {
  const rows = useMemo(() => fills, [fills]);

  if (loading) {
    return <p className="text-[11px] text-text-dim">loading fills…</p>;
  }
  if (rows.length === 0) {
    return (
      <p className="text-[11px] text-text-dim/60">
        No fills yet — entry still in flight, or this row has no entry snapshot.
      </p>
    );
  }

  return (
    <div className="flex flex-col gap-1">
      {reconstructed ? (
        <p className="text-[10px] text-text-dim/70">
          Reconstructed from position row (no <span className="font-mono">position_fills</span> ledger).
        </p>
      ) : null}
    <div className="overflow-x-auto rounded border border-white/10">
      <table className="w-full min-w-[28rem] border-collapse text-left text-[11px]">
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
                <td className="px-2 py-1">
                  <Badge variant={f.side === 'buy' ? 'buy' : 'sell'} size="sm" className="capitalize">
                    {f.side}
                  </Badge>
                </td>
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
                <td className="px-2 py-1">
                  {f.reason ? (
                    <span className="font-semibold text-accent">{f.reason}</span>
                  ) : (
                    <span className="text-text-dim">—</span>
                  )}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
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
