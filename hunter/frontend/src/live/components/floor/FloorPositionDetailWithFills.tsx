import { useMemo } from 'react';
import { FloorPositionDetail, type FloorDetailFacts } from './FloorPositionDetail';
import {
  PositionFillsLedger,
  fillsFromPositionFacts,
  fillsToExitLegs,
} from './PositionFillsLedger';
import { useGetPositionFillsQuery } from '@live/store/liveEndpoints';

/**
 * Console / Evidence detail body: hero + fact strip + chart ∥ fills.
 *
 * Chart markers come from the position's entry/exit snapshot (`facts.inspect`).
 * The ledger prefers durable `position_fills` rows; when that table is empty
 * we reconstruct display rows from the same snapshot so the table matches the
 * chart. On wide screens chart and fills sit side-by-side so neither leaves a
 * tall empty column.
 */
export function FloorPositionDetailWithFills({
  positionId,
  facts,
  chartHeight = 220,
}: {
  positionId: string;
  facts: FloorDetailFacts;
  chartHeight?: number;
}) {
  const { data: apiFills = [], isFetching } = useGetPositionFillsQuery(positionId, {
    skip: !positionId,
  });

  const { fills, reconstructed } = useMemo(() => {
    if (apiFills.length > 0) return { fills: apiFills, reconstructed: false };
    const legacy = fillsFromPositionFacts({
      positionId,
      entrySol: facts.entrySol,
      entryTokenAmount: facts.entryTokenAmount,
      exitSol: facts.exitSol,
      exitTokenAmount: facts.exitTokenAmount,
      exitReason: facts.exitReason,
      pnlSol: facts.pnlSol,
      inspect: facts.inspect,
    });
    return { fills: legacy, reconstructed: legacy.length > 0 };
  }, [
    apiFills,
    positionId,
    facts.entrySol,
    facts.entryTokenAmount,
    facts.exitSol,
    facts.exitTokenAmount,
    facts.exitReason,
    facts.pnlSol,
    facts.inspect,
  ]);

  const entryTokenAmount = useMemo(() => {
    const buy = fills.find((f) => f.side === 'buy');
    return buy?.token_amount || facts.entryTokenAmount || null;
  }, [fills, facts.entryTokenAmount]);

  const inspect = useMemo(() => {
    const exitLegs = fillsToExitLegs(fills, entryTokenAmount);
    if (exitLegs.length === 0) return facts.inspect;
    return { ...facts.inspect, exitLegs };
  }, [facts.inspect, fills, entryTokenAmount]);

  return (
    <FloorPositionDetail
      facts={{ ...facts, inspect }}
      chartHeight={chartHeight}
      chartAside={
        <div className="flex flex-col gap-1">
          <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim/70">
            Fills
          </span>
          <PositionFillsLedger
            fills={fills}
            entryPrice={facts.entryPrice ?? facts.inspect.entryPrice}
            entryTime={facts.inspect.entryTime}
            entryTokenAmount={entryTokenAmount}
            loading={isFetching && apiFills.length === 0 && !reconstructed}
            reconstructed={reconstructed}
          />
        </div>
      }
    />
  );
}
