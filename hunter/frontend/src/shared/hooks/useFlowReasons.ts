import { useMemo } from 'react';
import { compareTradesChronologically } from 'components/token-price-chart/chartBars';
import { useEffectiveFlowPatternKeys } from 'context/VolumePatternDraftContext';
import { flowReasonsById, type FlowReason } from 'lib/flow/classifyFlow';
import type { TradeRecord } from 'types';

/**
 * Effective (contagion-aware) vol/non-vol classification per trade id — the same
 * verdict the chart's overlay lines are drawn from, so the trades table can show
 * WHY a row counts as volume instead of only whether its own structure matches.
 *
 * Classifies the host's FULL trade history: contagion is forward-only, so
 * running it over one candle's rows would miss the earlier trade that tagged the
 * wallet. Resolves the pattern set through the shared draft, so a staged pattern
 * moves the table and the lines in the same tick.
 *
 * `null` when nothing can classify — the badge then falls back to structure only.
 *
 * Deep-imports the comparator rather than the `components/token-price-chart`
 * barrel: hosts that mount the trades panel statically must not pull
 * `lightweight-charts` into their chunk.
 */
export function useFlowReasons(
  trades: readonly TradeRecord[],
  savedKeys: ReadonlySet<string> | null | undefined,
  creatorWallet?: string | null,
): ReadonlyMap<string, FlowReason> | null {
  const { keys } = useEffectiveFlowPatternKeys(savedKeys);
  return useMemo(() => {
    const hasPatterns = keys != null && keys.size > 0;
    if (!hasPatterns && !creatorWallet) return null;
    const sorted = [...trades].sort(compareTradesChronologically);
    return flowReasonsById(
      sorted.map((t) => ({
        id: t.id,
        wallet_address: t.wallet_address ?? '',
        sol: t.amount_sol ?? 0,
        ix_labels: t.instruction_labels,
      })),
      { patternKeys: keys ?? new Set<string>(), creatorWallet },
    );
  }, [trades, keys, creatorWallet]);
}
