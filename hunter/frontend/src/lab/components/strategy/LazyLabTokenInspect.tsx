import { lazy, Suspense, type ComponentProps } from 'react';

const LabTokenInspect = lazy(() =>
  import('./LabTokenInspect').then((m) => ({ default: m.LabTokenInspect })),
);

/** Defers chart + metric panes until the detail panel mounts. */
export function LazyLabTokenInspect(props: ComponentProps<typeof LabTokenInspect>) {
  return (
    <Suspense
      fallback={
        <div className="flex items-center justify-center py-16 text-sm text-text-dim">
          Loading chart…
        </div>
      }
    >
      <LabTokenInspect {...props} />
    </Suspense>
  );
}
