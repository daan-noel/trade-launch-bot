import { useMemo } from 'react';
import { compareTradesChronologically } from 'components/token-price-chart/chartBars';
import {
  flowReasonsById,
  type FlowClassifyOptions,
  type FlowReason,
} from 'lib/flow/classifyFlow';
import type { TradeRecord } from 'types';

/**
 * Effective (contagion-aware) vol/non-vol classification per trade id — the same
 * verdict the chart's overlay lines are drawn from, so the trades table can show
 * WHY a row counts as volume instead of only whether its own structure matches.
 *
 * Classifies the host's FULL trade history: contagion is forward-only, so
 * running it over one candle's rows would miss the earlier trade that tagged
 * the wallet. Pass the SAME {@link FlowClassifyOptions} the chart's overlay
 * built (`classifyOptsForTape`), so a dump/working list switch cannot leave the
 * badge answering a different question from the lines.
 *
 * `null` when nothing can classify — the badge then falls back to structure only.
 *
 * Deep-imports the comparator rather than the `components/token-price-chart`
 * barrel: hosts that mount the trades panel statically must not pull
 * `lightweight-charts` into their chunk.
 */
export function useFlowReasons(
  trades: readonly TradeRecord[],
  opts: FlowClassifyOptions | null | undefined,
): ReadonlyMap<string, FlowReason> | null {
  const patternKeys = opts?.patternKeys;
  const patternRows = opts?.patternRows;
  const match = opts?.match;
  const creatorWallet = opts?.creatorWallet;
  const contagion = opts?.contagion;
  const excludeWallets = opts?.excludeWallets ?? null;
  const side = opts?.side ?? null;
  return useMemo(() => {
    if (!opts) return null;
    const hasPatterns = patternKeys != null && patternKeys.size > 0;
    if (!hasPatterns && (contagion === false || !creatorWallet)) return null;
    const sorted = [...trades].sort(compareTradesChronologically);
    return flowReasonsById(
      sorted.map((t) => ({
        id: t.id,
        wallet_address: t.wallet_address ?? '',
        sol: t.amount_sol ?? 0,
        ix_labels: t.instruction_labels,
        side: t.trade_type,
        cu_limit: t.cu_limit,
        cu_price: t.cu_price,
        tip_lamports: t.tip_lamports,
      })),
      {
        patternKeys: patternKeys ?? new Set<string>(),
        patternRows,
        match,
        creatorWallet,
        contagion,
        excludeWallets,
        side,
      },
    );
  }, [trades, opts, patternKeys, patternRows, match, creatorWallet, contagion, excludeWallets, side]);
}
