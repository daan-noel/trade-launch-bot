import { lazy, Suspense, type ComponentProps } from 'react';

const TokenTradeChart = lazy(() =>
  import('./TokenTradeChart').then((m) => ({ default: m.TokenTradeChart })),
);

/** Defers `lightweight-charts` until the chart actually mounts. */
export function LazyTokenTradeChart(props: ComponentProps<typeof TokenTradeChart>) {
  return (
    <Suspense
      fallback={
        <div className="flex items-center justify-center py-16 text-sm text-text-dim">
          Loading chart…
        </div>
      }
    >
      <TokenTradeChart {...props} />
    </Suspense>
  );
}
