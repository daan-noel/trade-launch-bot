import { useMemo } from 'react';
import { FloorPositionDetail, type FloorDetailFacts } from './FloorPositionDetail';
import { PositionFillsLedger, fillsToExitLegs } from './PositionFillsLedger';
import { useGetPositionFillsQuery } from '@live/store/liveEndpoints';

/**
 * Console detail body: fact strip + chart (with per-leg exit markers once fills
 * load) + fill ledger table.
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
  const { data: fills = [], isFetching } = useGetPositionFillsQuery(positionId, {
    skip: !positionId,
  });

  const entryTokenAmount = useMemo(() => {
    const buy = fills.find((f) => f.side === 'buy');
    return buy?.token_amount ?? null;
  }, [fills]);

  const inspect = useMemo(() => {
    const exitLegs = fillsToExitLegs(fills, entryTokenAmount);
    if (exitLegs.length === 0) return facts.inspect;
    return { ...facts.inspect, exitLegs };
  }, [facts.inspect, fills, entryTokenAmount]);

  return (
    <div className="flex flex-col gap-3">
      <FloorPositionDetail facts={{ ...facts, inspect }} chartHeight={chartHeight} />
      <div className="flex flex-col gap-1">
        <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim/70">
          Fills
        </span>
        <PositionFillsLedger
          fills={fills}
          entryPrice={facts.entryPrice}
          entryTime={facts.inspect.entryTime}
          entryTokenAmount={entryTokenAmount}
          loading={isFetching && fills.length === 0}
        />
      </div>
    </div>
  );
}
