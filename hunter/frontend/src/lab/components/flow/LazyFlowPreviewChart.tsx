import { lazy, Suspense, type ComponentProps } from 'react';
import { LoadingState } from 'components/ui/LoadingState';

const FlowPreviewChart = lazy(() =>
  import('./FlowPreviewChart').then((m) => ({ default: m.FlowPreviewChart })),
);

/** Defers the dual-axis flow chart (+ `lightweight-charts`) until a token is
 *  picked in the discovery roster — most of a Flow Discovery session never
 *  opens it. */
export function LazyFlowPreviewChart(props: ComponentProps<typeof FlowPreviewChart>) {
  return (
    <Suspense fallback={<LoadingState variant="panel" label="Loading chart…" />}>
      <FlowPreviewChart {...props} />
    </Suspense>
  );
}
