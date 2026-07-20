import { lazy, Suspense, type ComponentProps } from 'react';
import { LoadingState } from 'components/ui/LoadingState';

const TokenTradeChart = lazy(() =>
  import('./TokenTradeChart').then((m) => ({ default: m.TokenTradeChart })),
);

/** Defers `lightweight-charts` until the chart actually mounts. */
export function LazyTokenTradeChart(props: ComponentProps<typeof TokenTradeChart>) {
  return (
    <Suspense fallback={<LoadingState variant="panel" label="Loading chart…" />}>
      <TokenTradeChart {...props} />
    </Suspense>
  );
}
