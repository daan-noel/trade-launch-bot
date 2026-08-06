import { lazy, Suspense, type ComponentProps } from 'react';
import { LoadingState } from 'components/ui/LoadingState';
import { Modal } from 'components/ui/Modal';

const LivePositionInspectModal = lazy(() =>
  import('./LivePositionInspectModal').then((m) => ({
    default: m.LivePositionInspectModal,
  })),
);

/** Defers the position chart + fills ledger until a row is opened
 *  (mirrors `LazyLabTokenInspectModal` on the lab side). */
export function LazyLivePositionInspectModal(
  props: ComponentProps<typeof LivePositionInspectModal>,
) {
  const heading =
    props.position.symbol || `${props.position.mint_address.slice(0, 8)}…`;

  return (
    <Suspense
      fallback={
        <Modal title={`${heading} — position`} open onClose={props.onClose} size="xxl">
          <LoadingState variant="panel" label="Loading chart…" />
        </Modal>
      }
    >
      <LivePositionInspectModal {...props} />
    </Suspense>
  );
}
