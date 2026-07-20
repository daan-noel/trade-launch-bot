import { lazy, Suspense } from 'react';
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
        <div className="mt-4 flex items-center justify-center py-12 text-sm text-text-dim">
          Loading charts…
        </div>
      }
    >
      <TokenChartsGrid {...(props as TokenChartsGridProps<unknown>)} />
    </Suspense>
  );
}
