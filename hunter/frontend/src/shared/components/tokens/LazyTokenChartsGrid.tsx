import { lazy, Suspense } from 'react';
import { LoadingState } from 'components/ui/LoadingState';
import type { TokenChartsGridProps } from './TokenChartsGrid';

const TokenChartsGrid = lazy(() =>
  import('./TokenChartsGrid').then((m) => ({ default: m.TokenChartsGrid })),
);

/**
 * Defers the charts-grid chunk (`lightweight-charts`) until the Charts toggle
 * mounts it. `lazy()` erases generics — cast keeps call-site typing.
 */
export function LazyTokenChartsGrid<R>(props: TokenChartsGridProps<R>) {
  return (
    <Suspense
      fallback={
        <LoadingState
          variant="panel"
          label="Loading charts…"
          className="mt-4"
        />
      }
    >
      <TokenChartsGrid {...(props as TokenChartsGridProps<unknown>)} />
    </Suspense>
  );
}
