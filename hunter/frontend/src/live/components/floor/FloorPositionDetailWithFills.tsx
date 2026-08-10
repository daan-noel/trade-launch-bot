import { useMemo } from 'react';
import { FloorPositionDetail, type FloorDetailFacts } from './FloorPositionDetail';
import {
  PositionFillsLedger,
  fillsFromPositionFacts,
  fillsToExitLegs,
} from './PositionFillsLedger';
import { useGetPositionFillsQuery } from '@live/store/liveEndpoints';
import { useLivePositionFills } from '@live/hooks/useLivePositionFills';
import { useMintEpisodeMarkers } from 'hooks/useMintEpisodeMarkers';

/**
 * Console / Evidence detail body: hero + fact strip + chart ∥ fills.
 *
 * Chart markers come from the position's entry/exit snapshot (`facts.inspect`),
 * with the exit arrows re-derived from the durable `position_fills` ledger — the
 * freshest and most granular source, and the only one an *open* position has
 * (its `facts.inspect` carries no exit at all). When that table is empty we
 * reconstruct display rows from the snapshot so the table matches the chart.
 * {@link useLivePositionFills} refetches the ledger when a leg lands, so a
 * scale-out that fires while this is open draws its new arrow. On wide screens
 * chart and fills sit side-by-side so neither leaves a tall empty column.
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
  useLivePositionFills(positionId, facts.mint);

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

  // Widen the chart to every episode on the mint. `inspect` (this position, with the
  // ledger's legs) is substituted for its server copy, so the focused episode keeps
  // the freshest legs while its siblings come from the read.
  const episodeMarkers = useMintEpisodeMarkers({
    mint: facts.mint,
    mode: facts.mode,
    focus: inspect,
    focusPositionId: positionId,
  });

  return (
    <FloorPositionDetail
      facts={{ ...facts, inspect, episodeMarkers }}
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
