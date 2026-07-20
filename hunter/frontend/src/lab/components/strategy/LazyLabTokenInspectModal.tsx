import { lazy, Suspense, type ComponentProps } from 'react';

const LabTokenInspectModal = lazy(() =>
  import('./LabTokenInspectModal').then((m) => ({ default: m.LabTokenInspectModal })),
);

/** Defers inspect chart + metric panes until a row is opened. */
export function LazyLabTokenInspectModal(
  props: ComponentProps<typeof LabTokenInspectModal>,
) {
  return (
    <Suspense fallback={null}>
      <LabTokenInspectModal {...props} />
    </Suspense>
  );
}
