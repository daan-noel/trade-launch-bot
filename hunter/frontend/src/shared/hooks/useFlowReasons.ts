import { useMemo } from 'react';
import { tradeFlowReasons } from 'lib/flow/flowChartData';
import type { FlowClassifyOptions, FlowReason } from 'lib/flow/classifyFlow';
import type { TradeRecord } from 'types';

/**
 * The tag verdict per trade id (`@tag`, `@!tag` or neither, and which matcher held),
 * the same pass the chart's lines draw from, so the trades table can show WHY a row
 * carries the tag.
 *
 * Classifies the host's FULL trade history: a sticky tag is forward-only, so running
 * it over one candle's rows would miss the earlier trade that tagged the wallet. Pass
 * the SAME options the chart built (`classifyOptsForTag`). `null` without options.
 */
export function useFlowReasons(
  trades: readonly TradeRecord[],
  opts: FlowClassifyOptions | null | undefined,
): ReadonlyMap<string, FlowReason> | null {
  return useMemo(() => tradeFlowReasons(trades, opts), [trades, opts]);
}
