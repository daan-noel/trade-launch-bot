import { lazy, Suspense, type ComponentProps } from 'react';
import { LoadingState } from 'components/ui/LoadingState';

const FloorMintChart = lazy(() =>
  import('./FloorMintChart').then((m) => ({ default: m.FloorMintChart })),
);

/**
 * Defers `lightweight-charts` (~230 kB) until a Console/Portfolio row detail is
 * actually expanded. Every live surface that renders a mint chart imports THIS,
 * not `FloorMintChart` — a static import anywhere would pull the charting
 * library back into that page's chunk.
 */
export function LazyFloorMintChart(props: ComponentProps<typeof FloorMintChart>) {
  return (
    <Suspense
      fallback={
        <div style={{ minHeight: props.height ?? 220 }}>
          <LoadingState variant="panel" label="Loading chart…" />
        </div>
      }
    >
      <FloorMintChart {...props} />
    </Suspense>
  );
}
