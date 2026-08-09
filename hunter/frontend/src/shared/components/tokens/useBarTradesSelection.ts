import { useCallback, useMemo, useRef, useState } from 'react';
import type {
  ChartBarSelection,
  ChartRangeSelectionDetail,
} from 'components/token-price-chart/types';

/** Props a chart wrapper spreads onto `TokenPriceChart` to drive this selection. */
export interface BarTradesChartProps {
  selectedBar: ChartBarSelection | null;
  onBarClick: (selection: ChartBarSelection | null) => void;
  onRangeChange: (range: ChartRangeSelectionDetail | null) => void;
}

export interface BarTradesSelection {
  bar: ChartBarSelection | null;
  range: ChartRangeSelectionDetail | null;
  /** True while either a candle or a drag range is picked. */
  active: boolean;
  clear: () => void;
  chartProps: BarTradesChartProps;
}

/**
 * Candle-click / range-drag selection state for a price chart, paired with
 * {@link BarTradesPanel} which lists the selected trades.
 *
 * Bar and range are **mutually exclusive** — picking one clears the other, so a
 * host never shows two trade tables at once. Deep-imports the chart types
 * (never the `components/token-price-chart` barrel) so a statically-imported
 * host doesn't pull `lightweight-charts` into its chunk.
 *
 * @param onPick fired when the user makes a pick — lets the host drop a
 *   competing selection it owns (e.g. a row picked in a sibling table).
 */
export function useBarTradesSelection(onPick?: () => void): BarTradesSelection {
  const [bar, setBar] = useState<ChartBarSelection | null>(null);
  const [range, setRange] = useState<ChartRangeSelectionDetail | null>(null);

  // Ref, not a dep: the two handlers stay referentially stable so a memoized
  // chart doesn't re-render on every parent render.
  const onPickRef = useRef(onPick);
  onPickRef.current = onPick;

  const onBarClick = useCallback((selection: ChartBarSelection | null) => {
    setBar(selection);
    if (selection) {
      setRange(null);
      onPickRef.current?.();
    }
  }, []);

  const onRangeChange = useCallback((next: ChartRangeSelectionDetail | null) => {
    setRange(next);
    if (next) {
      setBar(null);
      onPickRef.current?.();
    }
  }, []);

  const clear = useCallback(() => {
    setBar(null);
    setRange(null);
  }, []);

  const chartProps = useMemo(
    () => ({ selectedBar: bar, onBarClick, onRangeChange }),
    [bar, onBarClick, onRangeChange],
  );

  return { bar, range, active: bar != null || range != null, clear, chartProps };
}
