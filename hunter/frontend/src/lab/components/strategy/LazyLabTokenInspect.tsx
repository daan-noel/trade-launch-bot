import { lazy, Suspense, type ComponentProps } from 'react';
import { LoadingState } from 'components/ui/LoadingState';

const LabTokenInspect = lazy(() =>
  import('./LabTokenInspect').then((m) => ({ default: m.LabTokenInspect })),
);

/** Defers chart + metric panes until the detail panel mounts. */
export function LazyLabTokenInspect(props: ComponentProps<typeof LabTokenInspect>) {
  return (
    <Suspense fallback={<LoadingState variant="panel" label="Loading chart…" />}>
      <LabTokenInspect {...props} />
    </Suspense>
  );
}
