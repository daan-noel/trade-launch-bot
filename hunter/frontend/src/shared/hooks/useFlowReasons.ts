import { useMemo } from 'react';
import { compareTradesChronologically } from 'components/token-price-chart/chartBars';
import { flowReasonsById, type FlowReason } from 'lib/flow/classifyFlow';
import type { TradeRecord } from 'types';

/**
 * Effective (contagion-aware) vol/non-vol classification per trade id — the same
 * verdict the chart's overlay lines are drawn from, so the trades table can show
 * WHY a row counts as volume instead of only whether its own structure matches.
 *
 * Classifies the host's FULL trade history: contagion is forward-only, so
 * running it over one candle's rows would miss the earlier trade that tagged the
 * wallet. Classifies with the fingerprint's SAVED patterns — the same row the
 * chart lines and the backend's `m_flow_split` fold read, so the table's reason
 * and the overlay can never disagree.
 *
 * `null` when nothing can classify — the badge then falls back to structure only.
 *
 * Deep-imports the comparator rather than the `components/token-price-chart`
 * barrel: hosts that mount the trades panel statically must not pull
 * `lightweight-charts` into their chunk.
 */
export function useFlowReasons(
  trades: readonly TradeRecord[],
  keys: ReadonlySet<string> | null | undefined,
  creatorWallet?: string | null,
  /** Lens overrides — structural-only reads and excluded wallets. Omitted ⇒ the
   *  engine's own behavior (contagion on, nothing excluded). */
  opts?: { contagion?: boolean; excludeWallets?: ReadonlySet<string> | null },
): ReadonlyMap<string, FlowReason> | null {
  const contagion = opts?.contagion;
  const excludeWallets = opts?.excludeWallets ?? null;
  return useMemo(() => {
    const hasPatterns = keys != null && keys.size > 0;
    // With contagion off the creator is no longer a classification of its own,
    // so a creator alone is not enough to have anything to report.
    if (!hasPatterns && (contagion === false || !creatorWallet)) return null;
    const sorted = [...trades].sort(compareTradesChronologically);
    return flowReasonsById(
      sorted.map((t) => ({
        id: t.id,
        wallet_address: t.wallet_address ?? '',
        sol: t.amount_sol ?? 0,
        ix_labels: t.instruction_labels,
      })),
      { patternKeys: keys ?? new Set<string>(), creatorWallet, contagion, excludeWallets },
    );
  }, [trades, keys, creatorWallet, contagion, excludeWallets]);
}
