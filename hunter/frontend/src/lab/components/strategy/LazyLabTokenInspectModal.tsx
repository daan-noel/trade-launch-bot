import { lazy, Suspense, type ComponentProps } from 'react';
import { LoadingState } from 'components/ui/LoadingState';
import { Modal } from 'components/ui/Modal';

const LabTokenInspectModal = lazy(() =>
  import('./LabTokenInspectModal').then((m) => ({ default: m.LabTokenInspectModal })),
);

/** Defers inspect chart + metric panes until a row is opened. */
export function LazyLabTokenInspectModal(
  props: ComponentProps<typeof LabTokenInspectModal>,
) {
  const heading = props.target.symbol || props.target.mint_address.slice(0, 8);
  const titleSuffix = props.titleSuffix ?? 'Token inspect';

  return (
    <Suspense
      fallback={
        <Modal
          title={`${heading} — ${titleSuffix}`}
          open
          onClose={props.onClose}
          size="xl"
        >
          <LoadingState variant="panel" label="Loading chart…" />
        </Modal>
      }
    >
      <LabTokenInspectModal {...props} />
    </Suspense>
  );
}
